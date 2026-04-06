# Текущие задачи (Спринт 3)

Улучшение интерфейса:

[Контекст]
Отличная работа, интеграция ядра прошла успешно, фокус и хоткеи работают. Теперь переходим к Фазе 2: Построение современного премиального UI. Я хочу, чтобы плеер выглядел как современный DAW-плагин или Spotify: темная тема, никаких нативных элементов управления ОС.

[Задача]
Сделай рефакторинг UI-кода в Slint. Вынеси цвета в отдельный блок свойств окна (например, property <color> bg-color: #121212;, accent-color: #1DB954;).
Раздели интерфейс на две зоны (используй VerticalBox или Layout):
Верхняя часть (занимает 100% оставшегося пространства): Зона видео (наш рендер libmpv).
Нижняя часть (высота около 80px): Панель управления (Control Bar).
На Панели управления размести:
Слева: Текст с текущим временем (00:00).
По центру: Горизонтальный Slider (кастомный, без стандартного стиля) для таймлайна видео.
Справа: Текст с общей длительностью видео.
Прямо под таймлайном по центру добавь кнопку Play/Pause (пока можно просто текст или прямоугольник с текстом, иконки прикрутим позже). Кнопка должна менять цвет при hover и pressed (анимация 150ms).

[Архитектурное правило]
При нажатии на ползунок таймлайна или кнопку Play/Pause, UI не должен сам менять видео. Slint должен отправить событие (callback) в Rust, а Rust уже дернет API libmpv.

# Итог

## Выполнено

### Theme.slint
- Добавлены новые свойства: `color-progress-thumb`, `animation-duration: 150ms`, `control-bar-height: 80px`, `control-bar-padding-h/v`, `progress-track-height`, `progress-thumb-size`, `play-btn-size`, `play-btn-radius`, `time-label-width`

### scene.slint (полный рефакторинг)
- **VerticalLayout**: видео-зона (занимает всё доступное пространство) + Control Bar (80px фиксированная высота)
- **PlayButton** — кастомный компонент: круглая кнопка 40px с ▶/⏸, анимации hover (зелёный акцент) и pressed (тёмный фон), длительность 150ms
- **TimelineSlider** — кастомный компонент: трек 6px + thumb-ползунок 14px, thumb появляется при hover/click, анимация 150ms
- **Время**: слева `elapsed_label` (MM:SS), справа `duration_label` (MM:SS) — парсинг на стороне Rust (Slint не имеет split для строк)
- Callbacks: `play-pause-requested()` и `seek-requested(float)` → Rust → mpv (UI не меняет видео напрямую)

### main.rs
- Добавлены обработчики `on_play_pause_requested` и `on_seek_requested`
- Таймер UI обновляет `elapsed_label` и `duration_label` из MpvState

### video.rs
- `MpvState` расширен: `elapsed_label`, `duration_label`
- Новые методы: `get_elapsed_label()` (MM:SS), `get_duration_label()` (MM:SS)
- `send_state()` заполняет все 5 полей

### Архитектурные решения
- Slint 1.x TouchArea: использует `has-hover`, `pressed` (out-свойства), `clicked` (callback). Нет `hovered-change`, `released`, `exited`
- Slint не поддерживает `split` для строк — время передаётся отдельными полями из Rust
- Нет `id:` в Slint — элементы именуются через `name := Element { ... }`
- Видео рендерится под UI через OpenGL underlay, control bar наложен поверх

### Статус
- `cargo build` проходит успешно
- Плеер запускается с файлом и без, `SLINT_BACKEND=GL` больше не требуется
- Фокус клавиатуры работает из терминала (причина неизвестна, факт подтверждён)
- Play/Pause кнопка работает корректно
- **Баги:** seek по клику не срабатывает, метки времени показывают "00:00", нет tooltip на ползунке

### Критическое открытие (баг с чёрным экраном)
- `Window.background: Theme.color-bg-primary` (чёрный) рисуется Slint'ом **ПОСЛЕ** BeforeRendering — перекрывает видео полностью
- **Решение:** `background: transparent` в MainWindow. Видео рисуется в BeforeRendering → прозрачный фон не перекрывает → control bar поверх
- Это архитектурное ограничение: video underlay рендерится ДО Slint UI, поэтому фон окна ДОЛЖЕН быть прозрачным
