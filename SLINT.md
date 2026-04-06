# SLINT — Справочник по Slint 1.15

Все знания о Slint, накопленные в процессе разработки quatru.

---

## Версия

- **Slint 1.15** (Cargo: `slint = "1.15"`, `slint-build = "1.15"`)
- Документация: https://slint.dev, https://docs.rs/crate/slint/latest, https://github.com/slint-ui/slint

---

## Бэкенды и рендереры

### Автодетект
Без `SLINT_BACKEND` Slint последовательно пробует: `qt` → `winit` → `linuxkms`.
`SLINT_BACKEND=GL` фиксирует OpenGL backend. **Не рекомендуется** — ломает кроссплатформенность (macOS/Metal, Windows/D3D).

### Отладка
- `SLINT_BACKEND=<backend>` — принудительный выбор бэкенда
- `SLINT_DEBUG=1` — выводит в stderr точное имя загруженного бэкенда и рендерера при старте

### set_rendering_notifier
Требует OpenGL (femtovg). На Linux Slint 1.15 выбирает GL сам через автодетект. На других платформах — `Err` → graceful exit.

---

## Синтаксис .slint

### Именование элементов
```slint
// НЕЛЬЗЯ — нет свойства id
Rectangle {
    id: my_rect;
}

// МОЖНО — через := (присваивание имени)
my-rect := Rectangle {
    // ...
}
```

### Свойства
```slint
// Объявление свойства
property<int> counter: 0;

// in-out (публичное, привязка извне)
in-out property<bool> is-playing: false;

// Привязка
property<color> bg: Theme.color-bg-primary;

// Привязка из родителя
width: parent.width;
```

### Callbacks
```slint
// Объявление
callback clicked();
callback seek-requested(float);

// Обработка
clicked => { root.clicked(); }
seek-requested(ratio) => { root.seek-requested(ratio); }
```

### Двусторонняя привязка
```slint
// <=> связывает свойство с источником в обе стороны
is-playing <=> root.player-state.is_paused;
```

### Условия и выражения
```slint
// ТЕРНАРНЫЙ оператор — ЕДИНСТВЕННЫЙ if/else
background: root.pressed ? Theme.color-accent : Theme.color-bg-elevated;

// НЕЛЬЗЯ использовать if/else блоки в свойствах:
// background: { if (x) { a; } else { b; } }  ← НЕ работает в некоторых контекстах

// let — для локальных переменных в выражениях
let parts = root.ts_label.split(" / ");
if (length(parts) >= 1) { parts[0]; } else { "00:00"; }
```

### Анимации
```slint
property<color> btn-bg: #fff;

Rectangle {
    background: root.btn-bg;
    animate background { duration: 150ms; }
}
```

---

## TouchArea API

### Свойства (out)
| Свойство | Тип | Описание |
|---|---|---|
| `has-hover` | `bool` | `true`, когда курсор над областью |
| `pressed` | `bool` | `true`, пока кнопка мыши удерживается над областью |
| `mouse-x` | `length` | Координата X курсора внутри TouchArea |
| `mouse-y` | `length` | Координата Y курсора внутри TouchArea |
| `pressed-x` | `length` | Координата X в момент нажатия |
| `pressed-y` | `length` | Координата Y в момент нажатия |
| `enabled` | `bool` | Включает/выключает перехват событий |
| `mouse-cursor` | `enum` | Тип курсора: `default`, `pointer`, `grab`, `text`, `crosshair` |

### Callbacks
| Callback | Сигнатура | Описание |
|---|---|---|
| `clicked` | `clicked() => { ... }` | Успешный клик (нажатие + отпускание) |
| `double-clicked` | `double-clicked() => { ... }` | Двойной клик |
| `moved` | `moved() => { ... }` | Перемещение **во время удержания** |
| `pointer-event` | `pointer-event(event: PointerEvent) => { ... }` | Универсальный. `event.kind`: `Down`, `Up`, `Move`, `Cancel` |
| `scroll-event` | `scroll-event(event) -> EventResult` | Колесо мыши |

### НЕ существует
- ❌ `hovered-change` — нет такого callback
- ❌ `exited` — нет такого callback
- ❌ `released` — нет такого callback
- ❌ `pressed` как callback — это **свойство** (out bool)
- ❌ `ancestor` — нет такого свойства
- ❌ `id` — нет такого свойства

### Паттерн: hover + pressed эффекты
```slint
btn-touch := TouchArea {
    mouse-cursor: pointer;
    clicked => { root.clicked(); }
}

// Используем свойства:
background: {
    if (btn-touch.pressed) { Theme.color-accent; }
    else if (btn-touch.has-hover) { Theme.color-accent-hover; }
    else { Theme.color-bg-elevated; }
}
```

---

## Keyboard API

### FocusScope
```slint
key-handler := FocusScope {
    x: 0px; y: 0px;
    width: parent.width;
    height: parent.height;
    init => { self.focus(); }

    // capture-key-pressed — ПЕРЕД дочерними элементами (включая TextInput)
    capture-key-pressed(event) => {
        if (event.text == " ") {
            // пробел
            return EventResult.accept;
        }
        if (event.text < " ") {
            // Control-символы (ESC = байт 27)
            root.control-key-pressed(event.text);
            return EventResult.accept;
        }
        EventResult.reject
    }
}
```

### key-pressed / key-released
```slint
// Обычный key-pressed — поглощается TextInput!
key-pressed(event) => {
    if (event.text == " ") {
        accept;
    }
    reject;
}
```

### Критичные факты
- **ESC** содержит байт 27 (`\x1b`), а НЕ пустую строку
- **Slint не парсит** `\u001b` в строковых литералах `.slint`
- Проверка ESC — **только в Rust** через `text.as_bytes()[0] == 27`
- **TextInput поглощает ВСЕ** `key-pressed` и `key-released` — включая ESC
- `capture-key-pressed` в FocusScope — **единственный** способ перехватить клавиши ДО TextInput

---

## Window

### Критично для video underlay
```slint
export component MainWindow inherits Window {
    background: transparent;  // ОБЯЗАТЕЛЬНО для OpenGL underlay!
}
```

Если `background` — непрозрачный цвет, Slint рисует его **ПОСЛЕ** `BeforeRendering`, перекрывая видео полностью.

Порядок рендеринга:
1. `BeforeRendering` → mpv рисует видео через OpenGL в FBO
2. Slint рисует `Window.background` → **ПЕРЕКРЫВАЕТ** если непрозрачный
3. Slint рисует UI-элементы (control bar)

---

## Layouts

### VerticalLayout / HorizontalLayout
```slint
VerticalLayout {
    padding: 0px;
    spacing: 0px;

    Rectangle { /* занимает всё доступное пространство */ }
    Rectangle { height: 80px; /* фиксированная высота */ }
}
```

### Padding в HorizontalLayout
```slint
HorizontalLayout {
    padding-left: 20px;
    padding-right: 20px;
    padding-top: 12px;
    padding-bottom: 12px;
    spacing: 16px;
}
```

---

## Чего НЕТ в Slint 1.15

| Что хочется | Что есть |
|---|---|
| `id: name` | `name := Element { ... }` |
| `var x = ...` | `let x = ...` |
| `string.split(" / ")` | Парсить на стороне Rust |
| `horizontal-alignment: end` | `horizontal-alignment: center` |
| `parent.parent.width` | Нет доступа к `parent` как к свойству из выражений |
| `ancestor: rect` у TouchArea | Нет такого свойства |
| `released` callback | `pressed` property + `clicked` callback |
| `hovered-change` callback | `has-hover` property |
| `length(array)` | Нет встроенной функции для длины массива/строки |

---

## Типичные ошибки компиляции и решения

| Ошибка | Причина | Решение |
|---|---|---|
| `Unknown property id in TouchArea` | `id:` не существует | Использовать `name := Element { ... }` |
| `Cannot access id 'touch'` | `id:` не работает | TouchArea именуется через `touch := TouchArea { ... }` внутри родительского элемента |
| `Cannot assign to private property 'is-playing'` | Свойство не `in-out` | Объявить `in-out property<bool> is-playing: false;` |
| `Unknown unqualified identifier 'var'` | `var` не существует | Использовать `let` |
| `Cannot access the field 'split' of string` | `split` не существует у строк | Парсить время на стороне Rust, передавать отдельные свойства |
| `Unknown unqualified identifier 'length'` | Нет встроенной функции `length()` для строк | Передавать длину из Rust или использовать другие подходы |
| `Unknown unqualified identifier 'end'` | `horizontal-alignment: end` не существует | `horizontal-alignment: center` + `min-width` |
| `Element 'Rectangle' does not have a property 'parent'` | `parent` не доступен в `let` выражениях | Использовать `parent.width` только в свойствах элементов |
| `'hovered-change' is not a callback` | Такого callback нет | Использовать `has-hover` свойство |
| `'exited' is not a callback` | Такого callback нет | Использовать `clicked` + `pressed` property |
| `'released' is not a callback` | Такого callback нет | `pointer-event(event)` с `event.kind == PointerEventKind.Up` |
| `Expected '{', keyword 'implements' or keyword 'inherits'` | `component Foo := Bar` | `component Foo inherits Bar { ... }` |

---

## Компоненты: паттерны

### Кастомная кнопка
```slint
component PlayButton inherits Rectangle {
    width: 40px;
    height: 40px;
    in-out property<bool> is-playing: false;
    callback clicked();

    btn-bg := Rectangle {
        width: parent.width - 4px;
        height: parent.height - 4px;
        x: 2px; y: 2px;
        border-radius: 20px;
        background: {
            if (btn-touch.pressed) { Theme.color-accent; }
            else if (btn-touch.has-hover) { Theme.color-accent-hover; }
            else { Theme.color-bg-elevated; }
        }
        animate background { duration: 150ms; }

        Text {
            text: root.is-playing ? "⏸" : "▶";
            horizontal-alignment: center;
            vertical-alignment: center;
        }

        btn-touch := TouchArea {
            mouse-cursor: pointer;
            clicked => { root.clicked(); }
        }
    }
}
```

### Кастомный слайдер
```slint
component TimelineSlider inherits Rectangle {
    height: 6px;
    background: Theme.color-progress-track;
    in-out property<float> progress: 0.0;
    callback seek-requested(float);

    Rectangle {
        width: parent.width * root.progress;
        height: parent.height;
        background: Theme.color-progress-fill;
    }

    thumb-rect := Rectangle {
        x: parent.width * root.progress - 7px;
        width: 14px; height: 14px;
        opacity: thumb-touch.has-hover || thumb-touch.pressed ? 1.0 : 0.0;
        animate opacity { duration: 150ms; }

        thumb-touch := TouchArea {
            mouse-cursor: pointer;
            clicked => {
                let ratio = self.mouse-x / root.width;
                let clamped = min(1.0, max(0.0, ratio));
                root.seek-requested(clamped);
            }
        }
    }
}
```

---

## Сборка

```rust
// build.rs
fn main() {
    slint_build::compile("src/scene.slint").unwrap();
}
```

```rust
// main.rs
slint::include_modules!();
// Теперь доступны: MainWindow, PlayerState, Theme, и все callbacks
```
