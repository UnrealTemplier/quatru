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
- **Rust** (strict mode, no memory leaks)
- **Slint 1.9** (UI-фреймворк)
- **libmpv** (бэкенд, динамическая линковка через `libloading`)
- **glow 0.16** (OpenGL-обёртка)

### База: Lumiere
Референс-проект [Lumiere](https://github.com/vgarleanu/lumiere) — минимальный плеер на Slint 0.2 + libmpv-rs. Подход: mpv рендерит кадры в OpenGL-фреймбуфер, Slint рисует этот фреймбуфер как текстуру через `set_rendering_notifier`. UI-элементы накладываются поверх видео средствами Slint.

---

## Текущая архитектура (после Спринта 1)

### Структура проекта
```
quatru/
├── Cargo.toml          # slint 1.9, glow 0.16, libloading 0.8
├── build.rs            # slint-build компиляция scene.slint
└── src/
    ├── main.rs         # Точка входа: инициализация окна, mpsc, rendering notifier, ESC callback
    ├── video.rs        # VideoUnderlay: mpv lifecycle, OpenGL FBO/texture/VAO, render loop
    ├── mpv.rs          # Runtime loading libmpv: dlopen, FFI-типы, обёртка функций
    ├── scene.slint     # UI: FocusScope + TextInput + контрол-бар
    ├── Theme.slint     # Тема: цвета, размеры, шрифты (global Theme)
    ├── vertex.glsl     # Вершинный шейдер: quad + texCoords
    └── fragment.glsl   # Фрагментный шейдер: sampling screenTexture
```

### Архитектурная диаграмма
```
┌─────────────────────────────────────────────────────────┐
│                     Slint Window                        │
│  ┌───────────────────────────────────────────────────┐  │
│  │              OpenGL Underlay (mpv)                │  │
│  │         (рендерит видео в наш FBO)                │  │
│  └───────────────────────────────────────────────────┘  │
│  ┌───────────────────────────────────────────────────┐  │
│  │           Контрол-бар (Rectangle)                 │  │
│  │  [Play/Pause] [========●========] [00:01:23/..]  │  │
│  └───────────────────────────────────────────────────┘  │
│  ┌───────────────────────────────────────────────────┐  │
│  │  FocusScope (capture-key-pressed → ESC handling)  │  │
│  │  TextInput (key-pressed → пробел/пауза)           │  │
│  └───────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────┘
         ▲                              │
         │  mpsc::Sender<MpvState>      │  mpsc::Receiver
         │                              ▼
┌───────────────────┐        ┌─────────────────────────┐
│   VideoUnderlay   │        │  Slint Timer (100ms)    │
│  ┌─────────────┐  │        │  rx_state.try_recv()   │
│  │  mpv handle │  │        │  → UI update           │
│  │  render_ctx │  │        └─────────────────────────┘
│  └─────────────┘  │
│  send_command()   │
│  get_position()   │
│  get_duration()   │
│  render()         │
└───────────────────┘
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
  ├─ Timer(100ms) → rx_state.try_recv() → update UI state
  │
  ├─ set_rendering_notifier(closure)
  │   │
  │   ├─ RenderingSetup:
  │   │   ├─ create VideoUnderlay::new(file, tx_state)
  │   │   │   ├─ MpvLib::load() → dlopen libmpv.so
  │   │   │   ├─ mpv_create()
  │   │   │   ├─ set_option_string("vo", "libmpv")  ← КРИТИЧНО: до initialize
  │   │   │   ├─ mpv_initialize()
  │   │   │   ├─ render_context_create()            ← инициализирует mpv автоматически
  │   │   │   └─ command("loadfile", file, "replace")
  │   │   │
  │   │   ├─ init_gl(gl, gpa_box, w, h)
  │   │   │   ├─ compile & link shaders
  │   │   │   ├─ create VBO, VAO, FBO, texture
  │   │   │   └─ store all GL handles
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

#### 3. mpsc каналы
`tx_state` → `rx_state` — единственный канал связи mpv → UI. Обновление UI происходит каждые 100ms через `slint::Timer`. Команды UI → mpv отправляются напрямую через `send_command()` (синхронно, в rendering thread).

#### 4. Обработка клавиатуры
`FocusScope` как первый элемент в иерархии + `capture-key-pressed` перехватывает клавиши ДО `TextInput`. Control-символы (ESC, Enter и т.д.) передаются в Rust через callback `control-key-pressed(string)`. Rust проверяет `bytes[0] == 27` для ESC.

#### 5. Без собственного окна mpv
`set_option_string(handle, "vo", "libmpv")` ДО `initialize()` заставляет mpv рендерить ТОЛЬКО через render API, не создавая своего окна.

---

## Подводные камни и решения

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

**Статус:** Не решено. Работает только после клика. Автоматический фокус при старте — задача для будущего спринта.

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

### Что НЕ делать
- НЕ пробовать `set_property_string` для отключения окна mpv — это не работает
- НЕ использовать `key-released` вместо `key-pressed` для ESC — TextInput тоже поглощает
- НЕ вызывать `mpv_initialize()` перед `render_context_create()` — assertion crash
- НЕ рассчитывать на `event.text == ""` для ESC — там символ `\x1b`
- НЕ использовать `\u001b` в `.slint` строках — Slint это не парсит
- НЕ удалять callback'и при рефакторинге, не проверив, что они используются

---
