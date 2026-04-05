# Роль
Ты — Senior Rust Systems Engineer & UI UX Expert. Я — Product Owner. Я не пишу код руками, ты делаешь всю работу автономно. 

# Цель
Создать мультимедийный плеер (аудио/видео) с упором на запредельную скорость реакции (blazing fast) и премиальный тёмный UI (в стиле Spotify / DAW плагинов).

# Стек
- Rust (strict mode, no memory leaks)
- Slint (UI)
- libmpv (Backend, dynamic linking)

# База (Lumiere)[https://github.com/vgarleanu/lumiere]. Используй этот подход для связи Slint и mpv.

# Архитектурные Заповеди
1. СКОРОСТЬ ВЫШЕ ВСЕГО: Мгновенное закрытие процесса без ожидания mpv (exit 0). 
2. ОПТИМИСТИЧНЫЙ UI: Интерфейс никогда не блокируется потоком mpv. Связь строго через `mpsc` каналы.
3. **ESTHETICS**: Окно без рамок, кастомные элементы управления, все цвета и размеры вынесены в `Theme.slint`. Никаких нативных виджетов ОС.
4. **ЗАВИСИМОСТИ**: `libmpv` подгружается в рантайме динамически (лежит в папке с бинарником).

# Текущая задача (Спринт 1)
Сгенерируй структуру проекта (Cargo.toml) и перенеси логику инициализации окна и mpv из Lumiere. Сделай чёрный экран (без рамок), который по нажатию `Esc` мгновенно завершает процесс. Отправь код.

# Итог

## Реализованные фичи

- [x] **Одно окно** — mpv НЕ создаёт собственное окно (`vo=libmpv` через `set_option_string()` ДО `initialize()`)
- [x] **Воспроизведение видео** — mpv рендерит в наш OpenGL-фреймбуфер через `mpv_render_context_render()` (OpenGL underlay)
- [x] **Звук** — воспроизводится штатно через mpv (ALSA/PulseAudio)
- [x] **Тёмный UI** — кастомный контрол-бар с кнопкой Play/Pause, прогресс-баром и таймкодом
- [x] **Theme.slint** — все цвета, размеры и шрифты вынесены в отдельный файл (Spotify/DAW стиль)
- [x] **ESC** — мгновенное закрытие программы (`std::process::exit(0)`), без ожидания mpv
- [x] **Пробел** — пауза/продолжение воспроизведения
- [x] **Крестик** — закрытие окна через `on_close_requested`
- [x] **mpsc каналы** — UI → mpv команды и mpv state → UI обновление (таймер 100ms)
- [x] **Runtime loading libmpv** — `libloading::dlopen`, не линкуется на этапе сборки
- [x] **OpenGL шейдеры** — vertex + fragment шейдеры для отображения видео-текстуры

## Проблемы и решения

### 1. Два окна вместо одного
**Проблема:** mpv создавал собственное окно для видео поверх нашего окна Slint.
**Решение:** `set_option_string(handle, "vo", "libmpv")` ДО вызова `initialize()`. Опция `libmpv` заставляет mpv рендерить ТОЛЬКО через render API, не создавая своего окна.
**Что НЕ сработало:** `force-window=no`, `geometry=1x1`, `border=no`, `window-minimized=yes` — эти опции игнорировались или не предотвращали создание окна.

### 2. ESC не работал (callback был удалён)
**Проблема:** `app.on_esc_pressed(|| exit(0))` callback был случайно удалён при рефакторинге `main.rs`.
**Решение:** Восстановлен вызов `app.on_esc_pressed()`.

### 3. TextInput поглощал ESC
**Проблема:** `TextInput` (единственный элемент, получающий фокус клавиатуры в Slint) полностью перехватывает `key-pressed` для ESC — наше событие никогда не доходило.
**Решение:** `FocusScope` как первый элемент в иерархии + `capture-key-pressed`. Этот callback вызывается ДО того как событие клавиатуры доходит до дочерних элементов (включая TextInput).
**Ключевой инсайт:** `event.text` для ESC содержит непечатаемый символ `\x1b` (байт 27), а НЕ пустую строку и НЕ `"Escape"`.

### 4. Невозможно сравнить с символом ESC в Slint
**Проблема:** Slint `.slint` не поддерживает escape-последовательности типа `\u001b` в строковых литералах. Нельзя написать `event.text == "\u001b"`.
**Решение:** Передаём текст control-клавиши в Rust через callback `control-key-pressed(string)`. В Rust проверяем `text.as_bytes()[0] == 27`. Это единственный надёжный способ отличить ESC от других control-символов (Enter, Backspace, Tab — все имеют `text < " "`).
**Что НЕ сработало:** `event.text == ""`, `event.text < " "`, `event.text != " " && event.text != "a" && ...` — первое ложно, второе ловит ВСЕ control-клавиши, третье ненадёжно.

### 5. Double initialization crash
**Проблема:** Вызов `mpv_initialize()` вручную ПЕРЕД `render_context_create()` приводил к assertion `!mpctx->initialized` — `render_context_create` уже инициализирует mpv.
**Решение:** Убран ручной вызов `mpv_initialize()`. `render_context_create()` инициализирует mpv автоматически.

### 6. Неправильная структура `MpvOpenGLFbo`
**Проблема:** В оригинальном коде передавался голый `i64` как параметр FBO. mpv ожидает структуру `mpv_opengl_fbo { fbo, w, h, internal_format }`.
**Решение:** Создана правильная `#[repr(C)] pub struct MpvOpenGLFbo` в `mpv.rs`.

### 7. `load_sym` borrow checker
**Проблема:** `load_sym(&lib, ...)` после мувинга `lib` в struct — borrow after move.
**Решение:** Загружаем все символы в локальные переменные ДО создания struct, затем мувим `lib`.

### 8. Фокус клавиатуры
**Проблема:** `FocusScope` не получает фокус автоматически. Пользователь должен кликнуть на окно, чтобы клавиатура заработала.
**Статус:** Работает после клика. Автоматический фокус при старте — задача для будущего спринта.

### Подходы, которые НЕ сработали вообще
- **`key-released` вместо `key-pressed`** — TextInput тоже поглощает release для ESC
- **`FocusScope` без `capture-key-pressed`** — обычный `key-pressed` не вызывается, если фокус на TextInput
- **`Window.key-pressed`** — Window не имеет такого callback в Slint 1.x
- **`winit event filter`** — в Slint 1.9 нет API для установки winit event filter
- **`set_property_string` после `initialize`** — вызывает re-init assertion в mpv 0.40
- **`set_option_string` после `initialize`** — опции игнорируются, нужно ДО
- **`FocusScope` + `TouchArea` для фокуса** — TouchArea не передаёт фокус родителю
- **Различные std-widgets (SpinBox, CheckBox, Button)** — тоже поглощают ESC
