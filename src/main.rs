pub mod mpv;
pub mod video;
slint::include_modules!();

use std::cell::RefCell;
use std::ffi::CStr;
use std::os::raw::c_void;
use std::sync::mpsc;
use video::{MpvCommand, MpvState, VideoUnderlay};

// VideoUnderlay хранится в thread-local, доступен из rendering notifier
thread_local! {
    static UNDERLAY: RefCell<Option<VideoUnderlay>> = RefCell::new(None);
}

pub fn main() {
    // Аргументы командной строки: опциональный путь к файлу
    let mut args = std::env::args();
    args.next();
    let file: Option<String> = args.next();

    // Канал: mpv state → UI (обновляется таймером каждые 100ms)
    let (tx_state, rx_state) = mpsc::channel::<MpvState>();

    let app = MainWindow::new().expect("Failed");
    app.set_player_state(PlayerState {
        is_paused: false,
        seek_position: 0.0,
        new_position: 0.0,
        position_ackd: true,
        ts_label: "00:00:00 / 00:00:00".into(),
        window_width: 1200.0,
        window_height: 800.0,
    });

    // Callback из Slint: FocusScope → control-key-pressed → ESC
    app.on_esc_pressed(|| std::process::exit(0));

    // Rust проверяет что это именно ESC (байт 27), а не другой control-символ
    app.on_control_key_pressed(move |text: slint::SharedString| {
        let bytes = text.as_bytes();
        if bytes.len() == 1 && bytes[0] == 27 {
            std::process::exit(0);
        }
    });

    let win = app.window();
    win.on_close_requested(move || std::process::exit(0));

    // Таймер обновления UI: получает MpvState от mpv, обновляет прогресс и таймкод
    let app_wk_t = app.as_weak();
    slint::Timer::default().start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(100),
        move || {
            while let Ok(st) = rx_state.try_recv() {
                if let Some(a) = app_wk_t.upgrade() {
                    let mut s = a.get_player_state();
                    s.ts_label = st.ts_label.into();
                    if st.duration > 0.0 {
                        s.seek_position = st.position / st.duration;
                    }
                    a.set_player_state(s);
                }
            }
        },
    );

    let app_wk = app.as_weak();
    let mut tx_state_opt = Some(tx_state);
    let mut prev_paused = false;

    // Rendering notifier: основной цикл связи Slint ↔ mpv
    if let Err(_) = app.window().set_rendering_notifier(move |state, ga| match state {
        slint::RenderingState::RenderingSetup => {
            let Some(ref f) = file else { return };
            let Some(tx) = tx_state_opt.take() else { return };
            let slint::GraphicsAPI::NativeOpenGL { get_proc_address } = ga else { return };

            unsafe {
                // SAFETY: get_proc_address живёт до RenderingTeardown
                let gpa_static: &'static dyn Fn(&CStr) -> *const c_void =
                    std::mem::transmute(*get_proc_address);
                let captured_gpa: Box<dyn Fn(&CStr) -> *const c_void> =
                    Box::new(move |name: &CStr| gpa_static(name));

                let gl = glow::Context::from_loader_function_cstr(|n| get_proc_address(n));
                let (w, h) = if let Some(a) = app_wk.upgrade() {
                    let sf = a.window().scale_factor();
                    let sz = a.window().size();
                    (
                        (sz.width as f32 * sf).round() as i32,
                        (sz.height as f32 * sf).round() as i32,
                    )
                } else {
                    (1200, 800)
                };

                // VideoUnderlay::new создаёт mpv handle, render context, GL объекты, загружает файл
                let underlay = VideoUnderlay::new(f, gl, captured_gpa, w, h, tx);
                UNDERLAY.with(|cell| *cell.borrow_mut() = Some(underlay));
            }
        }
        slint::RenderingState::BeforeRendering => {
            UNDERLAY.with(|cell| {
                let mut b = cell.borrow_mut();
                let Some(u) = b.as_mut() else { return };
                let Some(a) = app_wk.upgrade() else { return };

                let st = a.get_player_state();

                // Пауза / воспроизведение
                if prev_paused != st.is_paused {
                    u.send_command(if st.is_paused {
                        MpvCommand::Pause
                    } else {
                        MpvCommand::Play
                    });
                    prev_paused = st.is_paused;
                }

                // Seek
                if !st.position_ackd {
                    let dur = u.get_duration().unwrap_or(0.0) as f64;
                    u.send_command(MpvCommand::SeekAbsolute(
                        st.new_position as f64 / 100.0 * dur,
                    ));
                    let mut ns = st.clone();
                    ns.position_ackd = true;
                    a.set_player_state(ns);
                }

                // Обновить UI state и отрендерить кадр
                u.send_state();
                let sf = a.window().scale_factor();
                let sz = a.window().size();
                let w = (sz.width as f32 * sf).round() as i32;
                let h = (sz.height as f32 * sf).round() as i32;
                u.render(w, h);
                a.window().request_redraw();
            });
        }
        slint::RenderingState::RenderingTeardown => {
            UNDERLAY.with(|u| *u.borrow_mut() = None);
        }
        _ => {}
    }) {
        eprintln!("GL backend required. Run with: SLINT_BACKEND=GL");
        std::process::exit(1);
    }

    app.run().expect("Failed");
}
