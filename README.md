# trixui

Hybrid TUI/OpenGL framework — ratatui-style widgets rendered via OpenGL ES 3.

Works as a **standalone windowed app** (winit + glutin) or as the **chrome layer
inside a Wayland compositor** (Smithay). Same widget code, same API, two backends.

## Quick start

```toml
[dependencies]
trixui = "0.1"
```

```rust
use trixui::prelude::*;

struct MyApp { count: i32 }

impl App for MyApp {
    type Message = ();

    fn update(&mut self, event: Event<()>) -> Cmd<()> {
        if let Event::Key(k) = event {
            match k.code {
                KeyCode::Char('q') => return Cmd::quit(),
                KeyCode::Up        => self.count += 1,
                KeyCode::Down      => self.count -= 1,
                _ => {}
            }
        }
        Cmd::none()
    }

    fn view(&self, frame: &mut Frame) {
        let area = frame.area();
        let t    = frame.theme().clone();
        Block::bordered()
            .title(format!(" count: {} ", self.count).as_str())
            .border_style(Style::default().fg(t.active_border))
            .render(frame.canvas(), area, frame.cell_w(), frame.cell_h(), &t);
    }
}

fn main() -> trixui::Result<()> {
    Terminal::new(WinitBackend::new()?)?.run(MyApp { count: 0 })
}
```

## Widgets

| Widget      | Trait           | Notes |
|-------------|-----------------|-------|
| `Block`     | direct `.render()` | borders, title, top_accent |
| `Paragraph` | `Widget`        | text, wrap, scroll |
| `List`      | `StatefulWidget`| highlight, symbol, auto-scroll |
| `Table`     | `StatefulWidget`| header, column widths, highlight |
| `Tabs`      | `Widget`        | underline active tab, dividers |
| `Gauge`     | `Widget`        | filled bar, label |

## Layout

```rust
let [left, right] = Layout::horizontal(vec![
    Constraint::Percentage(40),
    Constraint::Fill(1),
])
.spacing(cell_w)
.split(area, cell_w, cell_h)[..] else { return };
```

Constraints: `Fixed`, `Length`, `Percentage`, `Ratio`, `Min`, `Max`, `Fill`.
Flex modes: `Start`, `Center`, `End`, `SpaceBetween`, `SpaceAround`, `Stretch`.

## Event model

Hybrid ratatui/bubbletea:

- `App::view()` renders directly into `&mut Frame` (ratatui style)
- `App::update()` returns `Cmd<Msg>` (bubbletea style)
- `Cmd::msg(m)` schedules an app-defined message
- `Cmd::batch(vec![...])` combines commands
- `Cmd::quit()` exits the loop

## Backends

### Winit (default)
Standalone windowed app. GL context owned by trixui.

```toml
trixui = { version = "0.1", features = ["backend-winit"] }
```

### Wayland/Smithay
For use inside a compositor. The compositor creates a `ChromeRenderer`,
wraps it in `WaylandBackend`, and delivers events manually.

```toml
trixui = { version = "0.1", features = ["backend-wayland"] }
```

```rust
// Inside your compositor:
let backend = WaylandBackend::new(renderer, vp_w, vp_h);
backend.push_key(KeyEvent::plain(KeyCode::Char('j')));
// trixui renders chrome; compositor handles surfaces.
```

## Theme

Catppuccin Mocha by default. Switch or customise:

```rust
fn theme(&self) -> Theme {
    Theme::macchiato()  // or Theme::latte() or a custom Theme { ... }
}
```

## Assets

Place your font at `assets/JetBrainsMonoNerdFont-Regular.ttf` in the crate root,
or call `WinitBackend::with_font(font_data, size_px, title)` with your own bytes.
