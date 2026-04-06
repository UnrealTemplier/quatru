# ARCHITECTURE — quatru

---

## Цели и ориентиры

### Видение продукта
Мультимедийный плеер (аудио/видео) с упором на **запредельную скорость реакции** (blazing fast) и **премиальный тёмный UI** в стиле Spotify / DAW плагинов. Пользователь открывает файл — он мгновенно воспроизводится. Нажимает Escape — программа закрывается мгновенно, без задержек и ожиданий.

### Архитектурные заповеди

1. **Скорость выше всего** — мгновенное закрытие процесса без ожидания mpv (`exit 0`). Интерфейс никогда не блокируется.
2. **Оптимистичный UI** — интерфейс никогда не блокируется потоком mpv. Связь строго через `mpsc` каналы.
3. **Esthetics** — окно без рамок, кастомные элементы управления, все цвета и размеры вынесены в `Theme.slint`. Никаких нативных виджетов ОС.
4. **Зависимости** — `libmpv` подгружается в рантайме динамически (лежит в папке с бинарником), не линкуется на этапе компиляции.

### Стек

| Зависимость | Версия | Документация |
|---|---|---|
| slint | 1.15 | https://slint.dev, https://docs.rs/crate/slint/latest, https://github.com/slint-ui/slint |
| slint-build | 1.15 | https://docs.rs/crate/slint-build/latest |
| glow | 0.17 | https://github.com/grovesNL/glow, https://docs.rs/crate/glow/latest |
| libloading | 0.9 | https://crates.io/crates/libloading/0.9.0, https://docs.rs/crate/libloading/latest |

- **Rust** (strict mode, no memory leaks)
- **libmpv** (бэкенд, динамическая линковка через libloading)

### База: Lumiere
Референс-проект [Lumiere](https://github.com/vgarleanu/lumiere) — минимальный плеер на Slint 0.2 + libmpv-rs. Подход: mpv рендерит кадры в OpenGL-фреймбуфер, Slint рисует этот фреймбуфер как текстуру через `set_rendering_notifier`. UI-элементы накладываются поверх видео средствами Slint.

---

## Текущая архитектура (после Спринта 3)

### Структура проекта
```
quatru/
├── Cargo.toml          # slint 1.15, glow 0.17, libloading 0.9
├── build.rs            # slint-build компиляция scene.slint
└── src/
    ├── main.rs         # Точка входа: инициализация окна, mpsc, rendering notifier, ESC/callback handlers
    ├── video.rs        # VideoUnderlay: mpv lifecycle, OpenGL FBO/texture/VAO, render loop
    ├── mpv.rs          # Runtime loading libmpv: dlopen, FFI-типы, обёртка функций
    ├── scene.slint     # UI: PlayButton, TimelineSlider, FocusScope, control bar
    ├── Theme.slint     # Тема: цвета, размеры, шрифты, анимации (global Theme)
    ├── vertex.glsl     # Вершинный шейдер: quad + texCoords
    └── fragment.glsl   # Фрагментный шейдер: sampling screenTexture
```

### Архитектурная диаграмма
```
┌─────────────────────────────────────────────────────────┐
│                     Slint Window                        │
│              background: transparent (КРИТИЧНО!)        │
│  ┌───────────────────────────────────────────────────┐  │
│  │              OpenGL Underlay (mpv)                │  │
│  │         (рендерит видео в наш FBO)                │  │
│  │         Видим через прозрачный фон окна           │  │
│  └───────────────────────────────────────────────────┘  │
│  ┌───────────────────────────────────────────────────┐  │
│  │       Control Bar (Rectangle, opacity 0.95)       │  │
│  │                                                   │  │
│  │  [00:42] [⏸] [====●============] [97:43]         │  │
│  │   elapsed  Play    TimelineSlider   duration      │  │
│  └───────────────────────────────────────────────────┘  │
│  ┌───────────────────────────────────────────────────┐  │
│  │  FocusScope (capture-key-pressed → ESC/Space)     │  │
│  └───────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────┘
         ▲                              │
         │  mpsc::Sender<MpvState>      │  mpsc::Receiver
         │                              ▼
┌───────────────────┐        ┌─────────────────────────┐
│   VideoUnderlay   │        │  Slint Timer (100ms)    │
│  ┌─────────────┐  │        │  rx_state.try_recv()    │
│  │  mpv handle │  │        │  → UI update            │
│  │  render_ctx │  │        │  (seek_position,        │
│  └─────────────┘  │        │   elapsed_label,        │
│  send_command()   │        │   duration_label)       │
│  get_position()   │        └─────────────────────────┘
│  get_duration()   │
│  render()         │
└───────────────────┘

  UI Callbacks → Rust → mpv:
  play-pause-requested() → toggle is_paused → BeforeRendering
  seek-requested(ratio) → position_ackd=false → seek command
```

### Call Flow

```
main()
  │
  ├─ parse CLI args → Option<String> file
  ├─ create mpsc channels (tx_state, rx_state)
  ├─ MainWindow::new()
  ├─ on_esc_pressed → exit(0)
  ├─ on_control_key_pressed → check byte == 27 → exit(0)
  ├─ on_close_requested → exit(0)
  ├─ on_play_pause_requested → toggle is_paused → set_player_state
  ├─ on_seek_requested(ratio) → position_ackd=false, seek_position=ratio
  ├─ Timer(100ms) → rx_state.try_recv() → update UI state
  │   (seek_position, elapsed_label, duration_label)
  │
  ├─ set_rendering_notifier(closure)
  │   │
  │   ├─ RenderingSetup:
  │   │   ├─ create VideoUnderlay::new(file, tx_state)
  │   │   │   ├─ MpvLib::load() → dlopen libmpv.so
  │   │   │   ├─ mpv_create()
  │   │   │   ├─ set_option_string("vo", "libmpv")  ← КРИТИЧНО: до initialize
  │   │   │   ├─ set_option_string("force-window", "no")
  │   │   │   ├─ set_option_string("terminal", "no")
  │   │   │   ├─ mpv_initialize()
  │   │   │   ├─ render_context_create()            ← инициализирует mpv
  │   │   │   ├─ command("loadfile", file, "replace")
  │   │   │   └─ init_gl(gl, gpa_box, w, h):
  │   │   │       ├─ compile & link shaders
  │   │   │       ├─ create VBO, VAO, FBO, texture
  │   │   │       └─ store all GL handles
  │   │
  │   ├─ BeforeRendering (каждый кадр):
  │   │   ├─ check pause change → send_command(Pause/Play)
  │   │   ├─ check seek needed → send_command(SeekAbsolute)
  │   │   ├─ send_state() → tx_state → Timer → UI
  │   │   ├─ render(w, h):
  │   │   │   ├─ bind FBO, set viewport
  │   │   │   ├─ resize texture if needed
  │   │   │   ├─ mpv_render_context_render(FBO)    ← mpv рисует в наш FBO
  │   │   │   ├─ unbind FBO, clear default FB
  │   │   │   └─ draw quad with video texture      ← показываем видео на экране
  │   │   └─ request_redraw()
  │   │
  │   └─ RenderingTeardown:
  │       └─ drop VideoUnderlay → render_context_free + terminate_destroy
  │
  └─ app.run() → Slint event loop
```

### Ключевые решения архитектуры

#### 1. Runtime loading libmpv
`libmpv` загружается через `libloading::dlopen` в рантайме. Не требуется `libmpv-dev` для компиляции. Функции загружаются через `Symbol::into_raw()`.

#### 2. OpenGL underlay
mpv рендерит видео в наш FBO через `mpv_render_context_render()`. Затем мы рисуем текстуру FBO на экран через quad + шейдеры. Это позволяет наложить кастомный UI поверх видео.

`set_rendering_notifier` требует OpenGL (femtovg). На Linux автодетект Slint 1.15 выбирает GL сам. На других платформах (macOS/Metal, Windows/D3D) — fallback через `Err` → graceful exit с подсказкой. **Не фиксируем `SLINT_BACKEND` в коде** — это сломает кроссплатформенность. Автодетект + graceful degradation.

#### 3. mpsc каналы
`tx_state` → `rx_state` — единственный канал связи mpv → UI. Обновление UI происходит каждые 100ms через `slint::Timer`. Команды UI → mpv отправляются напрямую через `send_command()` (синхронно, в rendering thread).

#### 4. Обработка клавиатуры
`FocusScope` как первый элемент в иерархии + `capture-key-pressed` перехватывает клавиши ДО `TextInput`. Control-символы (ESC, Enter и т.д.) передаются в Rust через callback `control-key-pressed(string)`. Rust проверяет `bytes[0] == 27` для ESC.

#### 5. Без собственного окна mpv
`set_option_string(handle, "vo", "libmpv")` ДО `initialize()` заставляет mpv рендерить ТОЛЬКО через render API, не создавая своего окна.

---

## Post-Mortem Engineering Log

Журнал проблем, решений и антипаттернов. Каждая запись — это проблема, с которой мы столкнулись, тупики, которые мы исследовали, и финальное решение.

### 1. Два окна вместо одного
**Проблема:** mpv создавал собственное окно для видео поверх окна Slint.

**Что пробовали и НЕ сработало:**
- `set_option_string("force-window", "no")` — игнорировалось
- `set_property_string("force-window", "no")` после `initialize()` — игнорировалось
- `set_option_string("geometry", "1x1+10000+10000")` — окно создавалось крошечным, но всё равно видно
- `set_option_string("border", "no")` — убирало рамки, но не окно
- `set_property_string("window-minimized", "yes")` — ошибка xdg_surface

**Решение:** `set_option_string("vo", "libmpv")` ДО `initialize()`. Опция `libmpv` — специальный video output, который рендерит ТОЛЬКО через render API.

**Важно:** `set_option_string` должен вызываться ДО `initialize()`. После `initialize()` опции игнорируются. `set_property_string` работает после, но не для всех опций.

### 2. ESC не работал — поглощался TextInput
**Проблема:** `TextInput` (единственный элемент, получающий фокус клавиатуры) полностью поглощал `key-pressed` для ESC. Наш handler никогда не вызывался.

**Что пробовали и НЕ сработало:**
- `key-released` вместо `key-pressed` — TextInput тоже поглощал release для ESC
- `FocusScope` без `capture-key-pressed` — обычный `key-pressed` не вызывается, если фокус на TextInput
- `Window.key-pressed` — Window не имеет такого callback в Slint 1.x
- Различные std-widgets (SpinBox, CheckBox, Button) — тоже поглощают ESC
- `FocusScope` + `TouchArea` для фокуса — TouchArea не передаёт фокус родителю

**Решение:** `FocusScope` как первый элемент в иерархии + `capture-key-pressed`. Этот callback вызывается **ДО** того как событие клавиатуры доходит до дочерних элементов (включая TextInput).

### 3. event.text для ESC — не пустая строка
**Проблема:** `event.text` для ESC содержит символ `\x1b` (байт 27), а НЕ пустую строку `""` и НЕ `"Escape"`.

**Что пробовали и НЕ сработало:**
- `event.text == ""` — ложь, в тексте `\x1b`
- `event.text == "Escape"` — ложь
- `event.text < " "` — работает для ESC, но ловит ВСЕ control-символы (Enter, Backspace, Tab)
- Длинный список исключений букв и цифр — ненадёжно

**Решение:** Передаём текст control-клавиши в Rust через callback `control-key-pressed(string)`. В Rust проверяем `text.as_bytes()[0] == 27`. Это единственный надёжный способ.

**Причина:** Slint `.slint` не поддерживает escape-последовательности типа `\u001b` в строковых литералах.

### 4. app.on_esc_pressed() был случайно удалён
**Проблема:** При рефакторинге `main.rs` callback `on_esc_pressed` был удалён. ESC вызывал callback из Slint, но в Rust не было обработчика.

**Решение:** Восстановлен `app.on_esc_pressed(|| std::process::exit(0))`.

### 5. Double initialization crash
**Проблема:** Вызов `mpv_initialize()` вручную ПЕРЕД `render_context_create()` приводил к assertion `!mpctx->initialized`.

**Решение:** Убран ручной вызов `mpv_initialize()`. `render_context_create()` инициализирует mpv автоматически. Порядок: `mpv_create()` → `set_option_string()` → `render_context_create()`.

### 6. Неправильная структура MpvOpenGLFbo
**Проблема:** В оригинальном коде Lumiere передавался голый `i64` как параметр FBO. mpv ожидает структуру `mpv_opengl_fbo { fbo, w, h, internal_format }`.

**Решение:** Создана правильная `#[repr(C)] pub struct MpvOpenGLFbo` в `mpv.rs`.

### 7. load_sym borrow checker
**Проблема:** `load_sym(&lib, ...)` после мувинга `lib` в struct — borrow after move.

**Решение:** Загружаем все символы в локальные переменные ДО создания struct, затем мувим `lib`.

### 8. Фокус клавиатуры при старте
**Проблема:** `FocusScope` не получает фокус автоматически. Пользователь должен кликнуть на окно, чтобы клавиатура заработала.

**Корневая причина:** При запуске **из терминала** (Alacritty) — терминал очищает `XDG_ACTIVATION_TOKEN` и `DESKTOP_STARTUP_ID`. Композитор Wayland не может передать фокус без токена. При запуске **из лаунчера** (Dolphin, KDE menu) — токен передаётся, фокус работает сразу.

**Решение:**
- `slint::set_xdg_app_id("quatru")` в `main()` до создания окна
- `forward-focus: key-handler` на уровне `Window` в `.slint`
- `init => { self.focus(); }` в `FocusScope`
- **Тестировать запуск из файлового менеджера, не из терминала**

**Статус (2025-04):** Фокус из терминала **стал работать** без видимых причин. Ни slint, ни система не обновлялись. Причина неизвестна — возможно, побочный эффект других изменений или кэша композитора. Факт: фокус работает. Оставляем как есть.

### 9. `winit::focus_window()` на Wayland — пустая функция
**Проблема:** `focus_window()` в winit 0.30 на Wayland — `pub fn focus_window(&self) {}` — ничего не делает. Вызов бесполезен.

**Решение:** Не вызывать. Фокус приходит автоматически при запуске из лаунчера с токеном активации.

### 10. Slint `Window.background` перекрывает видео (чёрный экран)
**Проблема:** После рефакторинга UI (Спринт 3) видео перестало отображаться — чёрный экран, хотя mpv воспроизводит (position растёт, duration определяется).

**Что пробовали и НЕ сработало:**
- Проверка exit code (124 = timeout) — ничего не говорит о видимости видео
- Анализ логов `pos=0.1s dur=5863.4s` — mpv работает, но видео не видно
- Восстановление старого scene.slint — видео всё равно не видно

**Корневая причина:** `Window.background: Theme.color-bg-primary` (#0a0a0a). Slint рисует фон окна **ПОСЛЕ** `BeforeRendering` callback, где мы рендерим видео через OpenGL. Порядок: (1) BeforeRendering → mpv рисует в FBO, (2) Slint рисует `background` → **ЧЁРНЫЙ ПРЯМОУГОЛЬНИК ПОВЕРХ ВИДЕО**, (3) Slint рисует control bar.

**Решение:** `background: transparent` в MainWindow. Видео рендерится в OpenGL underlay → прозрачный фон не перекрывает → control bar (Rectangle с opacity 0.95) поверх видео.

**Антипаттерн:** Попытка использовать `Theme.color-bg-primary` как фон окна при video underlay архитектуре. Это работает только для приложений без видео.

### 11. Slint 1.x TouchArea API — нет `hovered-change`, `released`, `exited`
**Проблема:** При написании кастомных компонентов (PlayButton, TimelineSlider) использовал несуществующие callback'и: `hovered-change`, `exited`, `pressed`, `released`.

**Что пробовали и НЕ сработало:**
- `id: touch` — в Slint нет свойства `id` у элементов
- `ancestor: some-rect` — нет такого свойства у TouchArea
- `var ratio = ...` — в Slint нет `var`, есть `let`
- `string.split(" / ")` — у строк нет метода `split`
- `horizontal-alignment: end` — такого значения нет, есть `center`
- `parent.parent.width` — `parent` не является доступным свойством

**Решение:**
- Именование элементов: `name := Element { ... }` (через `:=`)
- Hover/pressed: использовать out-свойства TouchArea — `has-hover`, `pressed`
- TouchArea callbacks: `clicked` — единственный надёжный callback для кликов
- Для drag: `pressed` property + `clicked` callback
- Строки: парсить время на стороне Rust, передавать отдельные `elapsed_label` и `duration_label`
- `horizontal-alignment: center` для правого выравнивания (нет `end`)

---

## Архитектурные риски (на будущее)

### 1. Интервал таймера UI (100ms)
Сейчас обновление UI (позиция видео, таймкод) происходит каждые 100ms. На 60 Гц мониторе это незаметно, но на 144 Гц ползунок может выглядеть «дёрганым».

**Если заметишь:** микро-дёрганье прогресс-бара.
**Решение:** уменьшить интервал до 16-33ms (60-30 fps). Следить за нагрузкой на CPU.

### 2. Синхронные команды в render-потоке
Команды UI → mpv (`send_command`) вызываются синхронно в `BeforeRendering`. Если команда mpv (seek, смена дорожки) заблокируется на 50ms, это вызовет микро-фриз видео-кадра — render-поток ждёт ответа.

**Если заметишь:** «заикание» видео при нажатии кнопок.
**Решение:** сделать `Command Queue` — render-поток «вычерпывает» команды без ожидания, не блокируя отрисовку кадра.

### 3. Почему схема mpv → FBO → Slint правильная
mpv рендерит в наш OpenGL FBO, Slint рисует этот FBO как текстуру. Пиксели НЕ копируются через CPU — только переключение контекстов GPU. Это и есть «blazing fast» путь. Любая попытка копировать кадры в RAM убьёт производительность.

---

## Что модель должна помнить

### КРИТИЧНО для будущих спринтов
- **Никогда не удаляй `app.on_esc_pressed()`** — это был самый тихий и опасный баг
- **`vo=libmpv` через `set_option_string` ДО `initialize()`** — единственный способ предотвратить создание окна mpv
- **`capture-key-pressed` в FocusScope** — единственный способ перехватить клавиши ДО TextInput
- **Проверка ESC в Rust через `bytes[0] == 27`** — единственный надёжный способ (Slint не поддерживает `\u001b`)
- **`render_context_create()` инициализирует mpv** — НЕ вызывать `mpv_initialize()` вручную
- **`set_option_string` ДО `initialize()`** — после `initialize()` опции игнорируются
- **TextInput поглощает ВСЕ `key-pressed` и `key-released`** — включая ESC
- **`Window.background` ДОЛЖЕН быть `transparent`** — иначе чёрный фон Slint перекроет видео
- **AI не видит экран** — НЕ делать выводов о видимости видео по exit code или логам, спрашивать PO
- **НЕ коммитить** — git только для чтения, коммитит PO

### Что НЕ делать
- НЕ пробовать `set_property_string` для отключения окна mpv — это не работает
- НЕ использовать `key-released` вместо `key-pressed` для ESC — TextInput тоже поглощает
- НЕ вызывать `mpv_initialize()` перед `render_context_create()` — assertion crash
- НЕ рассчитывать на `event.text == ""` для ESC — там символ `\x1b`
- НЕ использовать `\u001b` в `.slint` строках — Slint это не парсит
- НЕ удалять callback'и при рефакторинге, не проверив, что они используются
- НЕ вызывать `focus_window()` на Wayland — это пустая функция в winit
- НЕ тестировать фокус из терминала (Alacritty) — он очищает токены активации
- НЕ использовать `Window.background: Theme.color-bg-primary` — фон перекроет видео
- НЕ использовать `id:`, `ancestor:`, `var`, `string.split()` — их нет в Slint 1.x
- НЕ использовать `horizontal-alignment: end` — такого значения нет

---
