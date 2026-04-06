use glow::HasContext;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::sync::mpsc;

use crate::mpv::{
    MpvLib, MpvHandle, MpvOpenGLFbo, MpvOpenGLInitParams, MpvRenderContext, MpvRenderParam,
    MPV_RENDER_PARAM_API_TYPE, MPV_RENDER_PARAM_FLIP_Y, MPV_RENDER_PARAM_OPENGL_FBO,
    MPV_RENDER_PARAM_OPENGL_INIT_PARAMS, MPV_RENDER_PARAM_INVALID, MPV_RENDER_API_TYPE_OPENGL,
};

/// Команды от UI к mpv
pub enum MpvCommand { Play, Pause, SeekAbsolute(f64) }
/// Состояние mpv, отправляемое в UI через mpsc
pub struct MpvState {
    pub position: f32,
    pub duration: f32,
    pub ts_label: String,
    pub elapsed_label: String,
    pub duration_label: String,
}

// Мост: mpv вызывает эту функцию для получения GL-процедур,
// мы делегируем в closure, сохранённый через gpa_raw.
unsafe extern "C" fn bridge_gpa(ctx: *mut c_void, name: *const c_char) -> *mut c_void {
    if ctx.is_null() { return std::ptr::null_mut(); }
    let closure: &Box<dyn Fn(&CStr) -> *const c_void> = &*(ctx as *const _);
    closure(CStr::from_ptr(name)) as *mut c_void
}

/// Обёртка вокруг mpv handle и OpenGL-ресурсов.
/// Рендерит видео в FBO через mpv render API, затем рисует текстуру на экран.
pub struct VideoUnderlay {
    gl: Option<glow::Context>,
    fbo: u32, texture: u32, program: u32, vao: u32,
    prev_w: i32, prev_h: i32,
    mpv_lib: MpvLib,
    mpv_handle: *mut MpvHandle,
    render_ctx: *mut MpvRenderContext,
    /// Владеет closure get_proc_address — держит его живым пока жив underlay
    _gpa_box: Option<Box<dyn Fn(&CStr) -> *const c_void>>,
    tx_state: mpsc::Sender<MpvState>,
}

unsafe impl Send for VideoUnderlay {}

impl VideoUnderlay {
    /// Создаёт mpv handle, render context, затем загружает файл.
    /// Порядок критичен: render_context ДО loadfile — иначе mpv создаст своё окно.
    pub fn new(
        file: &str,
        gl: glow::Context,
        gpa: Box<dyn Fn(&CStr) -> *const c_void>,
        width: i32, height: i32,
        tx_state: mpsc::Sender<MpvState>,
    ) -> Self {
        let mpv_lib = MpvLib::load().expect("Failed to load libmpv.so");
        unsafe {
            let handle = mpv_lib.create();
            assert!(!handle.is_null(), "mpv_create failed");

            // set_option ДО render_context_create
            // vo=libmpv — mpv НЕ создаёт своё окно, рендерит ТОЛЬКО через render API
            mpv_lib.set_option_string(handle, "vo", "libmpv");
            mpv_lib.set_option_string(handle, "force-window", "no");
            mpv_lib.set_option_string(handle, "terminal", "no");
            mpv_lib.set_option_string(handle, "input-default-bindings", "no");
            mpv_lib.set_option_string(handle, "border", "no");
            mpv_lib.set_option_string(handle, "keepaspect", "no");
            mpv_lib.set_option_string(handle, "config", "no");

            let gpa_raw = Box::into_raw(gpa);
            let _gpa_box = Some(Box::from_raw(gpa_raw));

            let api = CString::new(MPV_RENDER_API_TYPE_OPENGL).unwrap();
            let params = [
                MpvRenderParam { type_: MPV_RENDER_PARAM_API_TYPE, data: api.as_ptr() as *mut _ },
                MpvRenderParam {
                    type_: MPV_RENDER_PARAM_OPENGL_INIT_PARAMS,
                    data: &MpvOpenGLInitParams {
                        get_proc_address: bridge_gpa,
                        get_proc_address_ctx: gpa_raw as *mut c_void,
                        extra_exts: std::ptr::null(),
                    } as *const _ as *mut _,
                },
                MpvRenderParam { type_: MPV_RENDER_PARAM_INVALID, data: std::ptr::null_mut() },
            ];
            let mut ctx: *mut MpvRenderContext = std::ptr::null_mut();

            // initialize ДО render_context_create
            assert_eq!(mpv_lib.initialize(handle), 0, "mpv_initialize failed");
            assert_eq!(mpv_lib.render_context_create(&mut ctx, handle, params.as_ptr()), 0, "render_context_create failed");
            std::mem::forget(api);

            // После render_context_create
            mpv_lib.set_property_string(handle, "force-window", "no");
            mpv_lib.set_property_string(handle, "input-default-bindings", "no");

            // Загружаем файл
            let c_load = CString::new("loadfile").unwrap();
            let c_file = CString::new(file).unwrap();
            let c_mode = CString::new("replace").unwrap();
            let args: [*const c_char; 4] = [c_load.as_ptr(), c_file.as_ptr(), c_mode.as_ptr(), std::ptr::null()];
            if mpv_lib.command(handle, args.as_ptr()) != 0 {
                eprintln!("Warning: mpv loadfile returned error for: {}", file);
            }

            // GL объекты
            let prog = gl.create_program().expect("program");
            for (ty, src) in [(glow::VERTEX_SHADER, include_str!("./vertex.glsl")), (glow::FRAGMENT_SHADER, include_str!("./fragment.glsl"))] {
                let s = gl.create_shader(ty).expect("shader");
                gl.shader_source(s, src); gl.compile_shader(s);
                assert!(gl.get_shader_compile_status(s), "{}", gl.get_shader_info_log(s));
                gl.attach_shader(prog, s);
            }
            gl.link_program(prog);
            assert!(gl.get_program_link_status(prog));

            let verts: [f32; 24] = [
                -1.0,1.0,0.0,1.0, -1.0,-1.0,0.0,0.0, 1.0,-1.0,1.0,0.0,
                -1.0,1.0,0.0,1.0, 1.0,-1.0,1.0,0.0, 1.0,1.0,1.0,1.0,
            ];
            let vbo = gl.create_buffer().unwrap();
            gl.bind_buffer(glow::ARRAY_BUFFER, Some(vbo));
            gl.buffer_data_u8_slice(glow::ARRAY_BUFFER, verts.align_to().1, glow::STATIC_DRAW);
            let vao = gl.create_vertex_array().unwrap();
            gl.bind_vertex_array(Some(vao));
            gl.enable_vertex_attrib_array(0); gl.vertex_attrib_pointer_f32(0, 2, glow::FLOAT, false, 16, 0);
            gl.enable_vertex_attrib_array(1); gl.vertex_attrib_pointer_f32(1, 2, glow::FLOAT, false, 16, 8);

            let fbo = gl.create_framebuffer().unwrap();
            let tex = gl.create_texture().unwrap();
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
            gl.bind_texture(glow::TEXTURE_2D, Some(tex));
            gl.tex_image_2d(glow::TEXTURE_2D, 0, glow::RGBA8 as i32, width, height, 0, glow::RGBA, glow::UNSIGNED_BYTE, glow::PixelUnpackData::Slice(None));
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
            gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
            gl.framebuffer_texture_2d(glow::FRAMEBUFFER, glow::COLOR_ATTACHMENT0, glow::TEXTURE_2D, Some(tex), 0);
            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            gl.bind_texture(glow::TEXTURE_2D, None);

            Self {
                gl: Some(gl), fbo: fbo.0.get(), texture: tex.0.get(),
                program: prog.0.get(), vao: vao.0.get(),
                prev_w: width, prev_h: height, mpv_lib, mpv_handle: handle,
                render_ctx: ctx, _gpa_box, tx_state,
            }
        }
    }

    /// Отправляет команду mpv (play/pause/seek)
    pub fn send_command(&self, cmd: MpvCommand) {
        unsafe {
            let (a,b,v): (CString, CString, CString) = match cmd {
                MpvCommand::Play => (cs("set"), cs("pause"), cs("no")),
                MpvCommand::Pause => (cs("set"), cs("pause"), cs("yes")),
                MpvCommand::SeekAbsolute(p) => (cs("seek"), cs("absolute"), cs(&format!("{}", p))),
            };
            let ptrs = match cmd {
                MpvCommand::Play | MpvCommand::Pause => vec![a.as_ptr(), b.as_ptr(), v.as_ptr(), std::ptr::null()],
                MpvCommand::SeekAbsolute(_) => vec![a.as_ptr(), v.as_ptr(), b.as_ptr(), std::ptr::null()],
            };
            self.mpv_lib.command(self.mpv_handle, ptrs.as_ptr());
        }
    }
    /// Заглушка — команды отправляются напрямую через send_command
    pub fn process_commands(&mut self) {}
    /// Текущая позиция воспроизведения (секунды)
    pub fn get_position(&self) -> Option<f32> { unsafe { self.mpv_lib.get_property_double(self.mpv_handle, "time-pos").map(|v| v as f32) } }
    /// Длительность файла (секунды)
    pub fn get_duration(&self) -> Option<f32> { unsafe { self.mpv_lib.get_property_double(self.mpv_handle, "duration").map(|v| v as f32) } }
    /// Метка таймкода: "HH:MM:SS / HH:MM:SS"
    pub fn get_ts_label(&self) -> String {
        fn p(s: f32) -> String { let t=s as i64; format!("{:02}:{:02}:{:02}",t/3600,(t%3600)/60,t%60) }
        format!("{} / {}", p(self.get_position().unwrap_or(0.0)), p(self.get_duration().unwrap_or(0.0)))
    }
    /// Метка elapsed: "MM:SS"
    pub fn get_elapsed_label(&self) -> String {
        let s = self.get_position().unwrap_or(0.0) as i64;
        format!("{:02}:{:02}", s/60, s%60)
    }
    /// Метка duration: "MM:SS"
    pub fn get_duration_label(&self) -> String {
        let s = self.get_duration().unwrap_or(0.0) as i64;
        format!("{:02}:{:02}", s/60, s%60)
    }
    /// Отправляет текущее состояние в UI через mpsc
    pub fn send_state(&self) {
        let _ = self.tx_state.send(MpvState {
            position: self.get_position().unwrap_or(0.0),
            duration: self.get_duration().unwrap_or(0.0),
            ts_label: self.get_ts_label(),
            elapsed_label: self.get_elapsed_label(),
            duration_label: self.get_duration_label(),
        });
    }

    /// Рендерит кадр: mpv рисует в FBO → очищаем экран → рисуем видео-текстуру
    pub fn render(&mut self, w: i32, h: i32) {
        let gl = self.gl.as_ref().unwrap();
        let fbo_gl = glow::NativeFramebuffer(std::num::NonZeroU32::new(self.fbo).unwrap());
        let tex_gl = glow::NativeTexture(std::num::NonZeroU32::new(self.texture).unwrap());
        let prog_gl = glow::NativeProgram(std::num::NonZeroU32::new(self.program).unwrap());
        let vao_gl = glow::NativeVertexArray(std::num::NonZeroU32::new(self.vao).unwrap());
        unsafe {
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo_gl));
            gl.viewport(0, 0, w, h);
            if w != self.prev_w || h != self.prev_h {
                gl.bind_texture(glow::TEXTURE_2D, Some(tex_gl));
                gl.tex_image_2d(glow::TEXTURE_2D, 0, glow::RGBA8 as i32, w, h, 0, glow::RGBA, glow::UNSIGNED_BYTE, glow::PixelUnpackData::Slice(None));
                gl.bind_texture(glow::TEXTURE_2D, None);
                self.prev_w = w; self.prev_h = h;
            }
            gl.clear_color(0.0, 0.0, 0.0, 1.0); gl.clear(glow::COLOR_BUFFER_BIT);

            let gl_fbo = MpvOpenGLFbo { fbo: self.fbo as i32, w, h, internal_format: glow::RGBA8 as i32 };
            let flip_y: c_int = 1;
            let rparams = [
                MpvRenderParam { type_: MPV_RENDER_PARAM_OPENGL_FBO, data: &gl_fbo as *const _ as *mut _ },
                MpvRenderParam { type_: MPV_RENDER_PARAM_FLIP_Y, data: &flip_y as *const _ as *mut _ },
                MpvRenderParam { type_: MPV_RENDER_PARAM_INVALID, data: std::ptr::null_mut() },
            ];
            let rc = self.mpv_lib.render_context_render(self.render_ctx, rparams.as_ptr());
            if rc != 0 {
                eprintln!("[render] mpv_render_context_render error: {}", rc);
            }

            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            gl.viewport(0, 0, w, h);
            gl.clear_color(0.0, 0.0, 0.0, 1.0); gl.clear(glow::COLOR_BUFFER_BIT);
            gl.use_program(Some(prog_gl));
            gl.bind_vertex_array(Some(vao_gl));
            gl.bind_texture(glow::TEXTURE_2D, Some(tex_gl));
            gl.draw_arrays(glow::TRIANGLES, 0, 6);
            gl.bind_texture(glow::TEXTURE_2D, None);
            gl.bind_vertex_array(None);
            gl.use_program(None);
        }
    }
}

impl Drop for VideoUnderlay {
    fn drop(&mut self) {
        unsafe {
            if !self.render_ctx.is_null() { self.mpv_lib.render_context_free(self.render_ctx); self.render_ctx = std::ptr::null_mut(); }
            if !self.mpv_handle.is_null() { self.mpv_lib.terminate_destroy(self.mpv_handle); self.mpv_handle = std::ptr::null_mut(); }
            // _gpa_box дропается автоматически
        }
    }
}

fn cs(s: &str) -> CString { CString::new(s).unwrap() }
