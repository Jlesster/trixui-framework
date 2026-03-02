# trixui — API Reference

> Hybrid TUI/OpenGL framework. Ratatui-style widgets rendered via OpenGL ES 3.
> Works as a standalone windowed app (`winit` + `glutin`) **or** as the chrome
> layer inside a Wayland compositor (Smithay). Same widget API, two backends.

---

## Table of Contents

1. [Crate Overview](#1-crate-overview)
2. [App Trait](#2-app-trait)
3. [Event System](#3-event-system)
4. [Cmd — Effects & Commands](#4-cmd--effects--commands)
5. [Frame — Render Target](#5-frame--render-target)
6. [Backends](#6-backends)
   - 6.1 [WinitBackend](#61-winitbackend-feature-backend-winit)
   - 6.2 [WaylandBackend](#62-waylandbackend-feature-backend-wayland)
   - 6.3 [SmithayApp](#63-smithayapp-feature-backend-smithay)
   - 6.4 [Backend Trait](#64-backend-trait)
7. [Layout](#7-layout)
   - 7.1 [Rect](#71-rect)
   - 7.2 [CellRect](#72-cellrect)
   - 7.3 [ScreenLayout](#73-screenlayout)
8. [Renderer](#8-renderer)
   - 8.1 [Color](#81-color)
   - 8.2 [TextStyle](#82-textstyle)
   - 8.3 [BorderSide](#83-borderside)
   - 8.4 [CornerRadius](#84-cornerradius)
   - 8.5 [PowerlineDir](#85-powerlinedir)
   - 8.6 [DrawCmd](#86-drawcmd)
   - 8.7 [PixelCanvas](#87-pixelcanvas)
   - 8.8 [Theme](#88-theme)
9. [Widgets](#9-widgets)
   - 9.1 [Widget & StatefulWidget Traits](#91-widget--statefulwidget-traits)
   - 9.2 [Style](#92-style)
   - 9.3 [Borders](#93-borders)
   - 9.4 [Block](#94-block)
   - 9.5 [Paragraph](#95-paragraph)
   - 9.6 [List](#96-list)
   - 9.7 [Table](#97-table)
   - 9.8 [Tabs](#98-tabs)
   - 9.9 [Gauge](#99-gauge)
   - 9.10 [TextInput](#910-textinput)
   - 9.11 [Spinner](#911-spinner)
   - 9.12 [Scrollbar](#912-scrollbar)
   - 9.13 [Popup](#913-popup)
   - 9.14 [TitleBar](#914-titlebar)
   - 9.15 [Chrome — PaneOpts & BarBuilder](#915-chrome--paneopts--barbuilder)
10. [Layout Engine](#10-layout-engine)
    - 10.1 [Constraint](#101-constraint)
    - 10.2 [Flex](#102-flex)
    - 10.3 [Direction](#103-direction)
    - 10.4 [Layout](#104-layout)
11. [Helper Functions](#11-helper-functions)
12. [Flat Re-exports & Prelude](#12-flat-re-exports--prelude)
13. [Error Handling](#13-error-handling)
14. [Renderer Internals (GL)](#14-renderer-internals-gl)

---

## 1. Crate Overview

```
trixui
├── app/          App trait, Cmd, Frame, Event, Terminal
├── backend/      Backend trait + winit / wayland implementations
├── layout/       Rect, CellRect, ScreenLayout
├── renderer/     Color, DrawCmd, PixelCanvas, Theme, GL renderer
├── widget/       All widgets + layout engine
│   └── chrome/   PaneOpts, BarBuilder, draw_pane, draw_bar
└── smithay/      Plug-and-play Smithay compositor integration
```

**All coordinates are pixel-space: top-left origin, X right, Y down.** There is no cell-grid coordinate system in the public API — everything is pixels except where `Constraint::Length`, `Constraint::Min`, and `Constraint::Max` accept cell counts that are multiplied by `cell_w` / `cell_h` internally.

---

## 2. App Trait

```rust
pub trait App: Sized + 'static {
    type Message: Send + 'static;

    fn update(&mut self, event: Event<Self::Message>) -> Cmd<Self::Message>;
    fn view(&self, frame: &mut Frame);

    // Optional overrides:
    fn init(&mut self) -> Cmd<Self::Message> { Cmd::none() }
    fn theme(&self) -> Theme { Theme::default() }
    fn tick_rate(&self) -> u64 { 60 }
}
```

| Method | Description |
|--------|-------------|
| `update` | **Required.** Called for every event. Returns a `Cmd` describing the next effect. |
| `view` | **Required.** Called every frame. Push draw calls to `frame`. Must be pure — no state mutation. |
| `init` | Called once before the event loop starts. Use to fire initial `Cmd::Spawn` tasks or set state. |
| `theme` | Return a custom `Theme` each frame. Default returns Catppuccin Mocha. |
| `tick_rate` | Target frame rate in Hz for `Event::Tick`. Default `60`. |

### Minimal implementation

```rust
struct Counter { count: i32 }

impl App for Counter {
    type Message = ();

    fn update(&mut self, event: Event<()>) -> Cmd<()> {
        if let Event::Key(k) = event {
            match k.code {
                KeyCode::Char('+') => self.count += 1,
                KeyCode::Char('-') => self.count -= 1,
                KeyCode::Char('q') => return Cmd::quit(),
                _ => {}
            }
        }
        Cmd::none()
    }

    fn view(&self, frame: &mut Frame) {
        let area = frame.area();
        let inner = frame.render_block(Block::bordered().title(" Counter "), area);
        frame.render(Paragraph::new(&format!("count: {}", self.count)), inner);
    }
}
```

---

## 3. Event System

```rust
pub enum Event<Msg> {
    Key(KeyEvent),
    KeyUp(KeyEvent),
    Mouse(MouseEvent),
    Scroll { x: f32, y: f32 },
    Resize(u32, u32),
    Tick,
    FocusGained,
    FocusLost,
    Message(Msg),
}
```

| Variant | When fired |
|---------|-----------|
| `Key(KeyEvent)` | Key pressed **or** key-repeat while held. Check `k.repeat` to distinguish. |
| `KeyUp(KeyEvent)` | Key released. |
| `Mouse(MouseEvent)` | Any mouse action (click, drag, move, scroll). |
| `Scroll { x, y }` | High-resolution scroll delta in logical pixels (touchpad / hi-res mouse). Positive Y = scroll down. |
| `Resize(w, h)` | Viewport resized. |
| `Tick` | Regular heartbeat at `App::tick_rate()` Hz. Drive animations here. |
| `FocusGained` / `FocusLost` | Window / compositor surface focus changed. |
| `Message(Msg)` | Delivered by `Cmd::msg(m)` or the result of a `Cmd::Spawn` task. |

---

### KeyEvent

```rust
pub struct KeyEvent {
    pub code:      KeyCode,
    pub modifiers: KeyModifiers,
    pub repeat:    bool,   // true when this is a key-repeat, not initial press
}
```

**Constructors:**

| | |
|---|---|
| `KeyEvent::new(code, modifiers)` | Initial press, no repeat. |
| `KeyEvent::plain(code)` | No modifiers, no repeat. |
| `KeyEvent::repeated(code, modifiers)` | Marks as a repeat event. |

---

### KeyCode

```rust
pub enum KeyCode {
    Char(char),
    Enter, Backspace, Delete, Esc,
    Tab, BackTab,
    Up, Down, Left, Right,
    Home, End, PageUp, PageDown, Insert,
    F(u8),   // F1–F12
    Null,
}
```

---

### KeyModifiers

Bitflags. Combine with `|`.

```rust
KeyModifiers::NONE   // 0b0000
KeyModifiers::SHIFT  // 0b0001
KeyModifiers::CTRL   // 0b0010
KeyModifiers::ALT    // 0b0100
KeyModifiers::SUPER  // 0b1000
```

Usage:

```rust
if k.modifiers.contains(KeyModifiers::CTRL) { … }
```

---

### MouseEvent

```rust
pub struct MouseEvent {
    pub kind:   MouseEventKind,
    pub x:      u32,        // pixel X from surface top-left
    pub y:      u32,        // pixel Y from surface top-left
    pub button: MouseButton,
}
```

**`MouseEvent::in_rect(x, y, w, h) -> bool`** — convenience hit-test.

**MouseEventKind:**

| Variant | Description |
|---------|-------------|
| `Down` | Button pressed. |
| `Up` | Button released. |
| `Drag` | Cursor moved with a button held. |
| `Moved` | Cursor moved with no button held. |
| `ScrollUp` / `ScrollDown` | Discrete scroll step. |

**MouseButton:** `Left`, `Right`, `Middle`, `None`.

---

## 4. Cmd — Effects & Commands

```rust
pub enum Cmd<Msg: 'static> {
    None,
    Quit,
    Msg(Msg),
    Batch(Vec<Cmd<Msg>>),
    Spawn(Box<dyn FnOnce() -> Msg + Send + 'static>),
}
```

| Constructor | Effect |
|-------------|--------|
| `Cmd::none()` | No-op. |
| `Cmd::quit()` | Shut down the event loop. |
| `Cmd::msg(m)` | Immediately re-queue `m` as `Event::Message(m)`. |
| `Cmd::batch(v)` | Execute multiple commands; quits if any element is `Quit`. |
| `Cmd::spawn(f)` | Run `f` on a thread-pool thread. The returned `Msg` is delivered as `Event::Message` on the next tick. |

### Background tasks with Cmd::spawn

```rust
fn update(&mut self, event: Event<MyMsg>) -> Cmd<MyMsg> {
    match event {
        Event::Key(k) if k.code == KeyCode::Enter => {
            return Cmd::spawn(|| {
                let data = std::fs::read_to_string("/etc/hostname").unwrap_or_default();
                MyMsg::HostnameLoaded(data)
            });
        }
        Event::Message(MyMsg::HostnameLoaded(s)) => {
            self.hostname = s;
        }
        _ => {}
    }
    Cmd::none()
}
```

---

## 5. Frame — Render Target

`Frame` is passed to `App::view`. It owns the `PixelCanvas` and provides helpers so you rarely need to call canvas methods directly.

```rust
pub struct Frame<'a> { … }
```

### Constructors

```rust
// Full constructor — supply font metrics explicitly.
Frame::new_with_metrics(
    canvas: &mut PixelCanvas,
    layout: ScreenLayout,
    theme:  &Theme,
    glyph_w: u32,
    line_h:  u32,
) -> Frame<'_>

// Convenience — metrics default to (0, 0). Use when font metrics are unavailable.
Frame::new(canvas: &mut PixelCanvas, layout: ScreenLayout, theme: &Theme) -> Frame<'_>
```

In practice both constructors are called by the backend runtime — user code receives `frame` as a parameter to `App::view` and never constructs it directly.

### Area accessors

| Method | Returns |
|--------|---------|
| `frame.area()` | Full viewport `Rect`. |
| `frame.content_area()` | Area above the status bar. |
| `frame.bar_area()` | Status bar `Rect`. |
| `frame.layout()` | `&ScreenLayout` — all geometry in one struct. |
| `frame.theme()` | `&Theme`. |
| `frame.cell_w()` | `u32` — cell width in pixels. |
| `frame.cell_h()` | `u32` — cell height in pixels. |
| `frame.canvas()` | `&mut PixelCanvas` — direct draw access. |

### Render helpers

```rust
// Render any Widget into area.
frame.render(widget: impl Widget, area: Rect)

// Render a StatefulWidget into area.
frame.render_stateful(widget: W, area: Rect, state: &mut W::State)

// Render a Block and return the inner content Rect.
frame.render_block(block: Block, area: Rect) -> Rect
```

### Chrome helpers

High-level compositor chrome drawing. Cell metrics and the active theme are
supplied implicitly — no manual plumbing needed.

```rust
// Draw a pane border + title in one call.
// See §9.15 for PaneOpts builder.
frame.draw_pane(area: Rect, opts: PaneOpts)

// Begin building a status bar. Chain .left() / .center() / .right(),
// then call .finish() to flush. Returns BarBuilder.
// See §9.15 for the full BarBuilder / SectionBuilder API.
frame.bar(area: Rect) -> BarBuilder<'_>
```

**Minimal example:**

```rust
fn view(&self, frame: &mut Frame) {
    // Pane border + title — focused state drives colour automatically
    for pane in &self.panes {
        frame.draw_pane(
            pane.rect,
            PaneOpts::new(&pane.title)
                .icon("󰖟 ")
                .focused(pane.id == self.focused_id),
        );
    }

    // Status bar — three zones, all metrics implicit
    frame.bar(frame.bar_area())
        .left(|b| b
            .workspace_state(1, true,  false)
            .workspace_state(2, false, true)
            .workspace_state(3, false, false))
        .center(|b| b.layout("BSP"))
        .right(|b| b.clock("14:32"))
        .finish();
}
```

### Hit region API

Register named interactive zones during `view()`. Query them from the compositor or input handler after the frame.

```rust
// In view():
frame.register_region("titlebar", title_rect);
frame.register_region("close_btn", close_rect);

// Consume the frame and retrieve all regions (in registration order):
let regions: Vec<(String, Rect)> = frame.into_regions();

// Static hit-test against a region list:
// Returns the LAST-registered region containing (x, y), or None.
Frame::hit_test_regions(regions: &[(String, Rect)], x: u32, y: u32) -> Option<&str>
```

> **Note:** Last-registered wins on overlap — matches draw order (top-most widget).

---

## 6. Backends

### 6.1 WinitBackend *(feature: `backend-winit`)*

Standalone windowed application. Owns the winit event loop — do **not** use `Terminal::run()` with it.

```rust
// Default font (embedded Iosevka), default 1280×800 window.
WinitBackend::new()?.run_app(MyApp::new())?;

// Custom font.
WinitBackend::with_font(font_data: &[u8], size_px: f32)?.run_app(MyApp::new())?;
```

**Builder methods** (chain before `run_app`):

| Method | Default | Description |
|--------|---------|-------------|
| `.title(t: impl Into<String>)` | `"trixui"` | Window title. |
| `.window_size(w: u32, h: u32)` | `1280×800` | Initial window size in logical pixels. |
| `.resizable(r: bool)` | `true` | Whether the user can resize the window. |

```rust
WinitBackend::new()?
    .title("My App")
    .window_size(1920, 1080)
    .resizable(false)
    .run_app(MyApp::new())?;
```

The backend manages:
- Window creation (1280×800 default, resizable)
- GL context creation (OpenGL ES 3.0 via glutin)
- Key-repeat: 300 ms initial delay, 30 ms interval
- Mouse drag / move synthesis
- Both discrete `MouseEventKind::ScrollUp/Down` and precise `Event::Scroll` for wheel events
- `Event::Tick` at `App::tick_rate()` Hz (driven in `about_to_wait`)

The default embedded font is Iosevka Jless Brains Nerd Font Regular.

---

### 6.2 WaylandBackend *(feature: `backend-wayland`)*

Used from inside a Smithay compositor. Does not own a window or event loop — the compositor calls in.

```rust
let backend = WaylandBackend::new(renderer, vp_w, vp_h);
backend.push_key(KeyEvent::plain(KeyCode::Char('j')));
backend.set_size(new_w, new_h);
// backend.renderer_mut() — borrow the ChromeRenderer directly.
```

| Method | Description |
|--------|-------------|
| `new(renderer, vp_w, vp_h)` | Wrap an existing `ChromeRenderer`. |
| `push_key(ev: KeyEvent)` | Enqueue a key event for the next poll. |
| `set_size(w, h)` | Resize the viewport; enqueues `Event::Resize`. |
| `renderer_mut()` | Borrow the inner `ChromeRenderer` for direct use. |

> **Note:** Prefer `SmithayApp` over raw `WaylandBackend` — it handles the full render loop including ticking, damage tracking, and hit regions.

---

### 6.3 SmithayApp *(feature: `backend-smithay`)*

The recommended integration point for Smithay compositors. Self-contained — takes an `App` and manages the full render loop internally. The compositor only needs to call three methods per frame.

#### Construction

```rust
// Minimal — uses embedded font, 1920×1080.
let mut ui = SmithayApp::new(my_app, vp_w, vp_h)?;

// Full builder.
let mut ui = SmithayApp::builder(my_app)
    .viewport(2560, 1440)
    .font(font_bytes, 20.0)                   // optional; defaults to embedded font
    .font_config(FontConfig::new(reg, 20.0)   // or supply bold/italic variants
        .with_bold(bold_bytes)
        .with_italic(italic_bytes))
    .bar_height_px(28)                        // optional status bar reservation
    .build()?;                                // requires current GL context
```

`build()` calls `app.init()` and processes its `Cmd` tree before returning.

#### Per-frame workflow

```rust
// Standard — one call per DRM frame.
let flushed: bool = ui.render();
// Returns true if a GL flush was performed (damage tracking skips identical frames).

// Two-phase (explicit-sync / multi-GPU):
let cmds = ui.collect();          // CPU only — safe before FBO bind
if ui.needs_flush() {
    ui.flush_collected(cmds);     // GL only — call inside DRM render callback
}
```

#### Input delivery

```rust
ui.push_key(key_event: KeyEvent);
ui.push_mouse(mouse_event: MouseEvent);
ui.push_scroll(x: f32, y: f32);
ui.focus_gained();
ui.focus_lost();
ui.send(msg: A::Message);   // inject a message directly, bypasses event queue
```

All push methods enqueue; events are processed at the top of the next `render()` / `collect()` call.

#### Geometry

```rust
ui.resize(w: u32, h: u32);             // viewport resize; fires Event::Resize
ui.set_bar_height_px(h: u32);          // update status bar height
ui.cell_w() -> u32
ui.cell_h() -> u32
ui.layout() -> ScreenLayout            // current layout without re-rendering
```

#### Hit testing

```rust
// Test a physical-pixel coordinate against regions from the last render.
ui.hit_test(x: u32, y: u32) -> Option<&str>

// Direct access to last frame's regions.
ui.regions() -> &[(String, Rect)]
```

#### Damage tracking

`render()` compares the new `DrawCmd` list against the previous frame structurally. If they are identical the GL flush is skipped and `render()` returns `false`. This means the compositor can call `render()` unconditionally every frame at zero GPU cost when nothing changed.

`needs_flush()` returns `true` if there is dirty state, pending input, or background spawn results queued.

#### FontConfig

```rust
pub struct FontConfig {
    pub regular:  Arc<[u8]>,
    pub bold:     Option<Arc<[u8]>>,
    pub italic:   Option<Arc<[u8]>>,
    pub size_px:  f32,
}

FontConfig::new(regular_bytes, size_px)
    .with_bold(bold_bytes)
    .with_italic(italic_bytes)
```

`FontConfig::default()` uses the embedded Iosevka at 20px.

---

### 6.4 Backend Trait

```rust
pub trait Backend: Sized {
    fn size(&self) -> (u32, u32);                         // physical pixel dimensions
    fn cell_size(&self) -> (u32, u32);                    // (cell_w, cell_h) from font atlas
    fn poll_event<Msg: 'static>(&mut self) -> Option<Event<Msg>>;
    fn render(&mut self, cmds: &[DrawCmd], vp_w: u32, vp_h: u32);
}
```

Used internally by `Terminal`. Implement this to add a new platform backend.

---

## 7. Layout

All pixel-space. Top-left origin, X right, Y down.

### 7.1 Rect

```rust
pub struct Rect { pub x: u32, pub y: u32, pub w: u32, pub h: u32 }
```

| Method | Description |
|--------|-------------|
| `Rect::new(x, y, w, h)` | Constructor. |
| `is_empty()` | True if `w == 0 \|\| h == 0`. |
| `contains_point(px, py)` | Pixel hit-test. |
| `intersect(other) -> Option<Rect>` | Intersection; `None` if no overlap. |
| `union(other) -> Rect` | Bounding box union. |
| `inset(px) -> Rect` | Shrink uniformly on all sides. |
| `pad(top, right, bottom, left) -> Rect` | CSS-order independent padding. |
| `split_top(top_h) -> (Rect, Rect)` | Split into (top, bottom). |
| `split_left(left_w) -> (Rect, Rect)` | Split into (left, right). |
| `split_cols(n) -> Vec<Rect>` | Split into `n` equal columns; remainder to last. |
| `split_ratios(ratios: &[f32]) -> Vec<Rect>` | Split by normalised ratios; remainder to last. |

---

### 7.2 CellRect

```rust
pub struct CellRect { pub x: u16, pub y: u16, pub w: u16, pub h: u16 }
```

Used only for animation interpolation. Convert to pixels with `to_px(cell_w, cell_h) -> Rect`.

---

### 7.3 ScreenLayout

```rust
pub struct ScreenLayout {
    pub vp:      Rect,   // full viewport
    pub content: Rect,   // everything above the status bar
    pub bar:     Rect,   // status bar at bottom
}
```

**Construction:**

```rust
ScreenLayout::new(vp_w: u32, vp_h: u32, bar_h_px: u32)
```

`bar_h_px = 0` means no bar — `content` fills the entire viewport.

**Invariant:** `content.h + bar.h == vp.h` always.

`ScreenLayout` is pure pixel geometry and has no knowledge of font metrics. Cell-unit accessors require `cell_w` / `cell_h` to be supplied by the caller:

| Method | Returns |
|--------|---------|
| `content_cols(cell_w: u32)` | `u16` — content width in cells. |
| `content_rows(cell_h: u32)` | `u16` — content height in cells. |
| `content_cell_rect(cell_w, cell_h)` | Full content area as a `CellRect`. |
| `cell_rect_to_px(cr, cell_w, cell_h)` | Convert a `CellRect` to a pixel `Rect` within the content area. |

---

## 8. Renderer

### 8.1 Color

```rust
pub struct Color(pub u8, pub u8, pub u8, pub u8);  // RGBA8
```

**Constants:** `Color::TRANSPARENT` (alpha = 0).

**Constructors:**

| | |
|---|---|
| `Color::rgb(r, g, b)` | Fully opaque. |
| `Color::rgba(r, g, b, a)` | With explicit alpha. |
| `Color::hex(v: u32)` | From `0xRRGGBB` literal. Top byte ignored. Fully opaque. |

**Conversions:** `From<(u8,u8,u8)>`, `From<(u8,u8,u8,u8)>`, `From<u32>` (same as `hex`).

**Methods:**

| Method | Description |
|--------|-------------|
| `alpha(a: u8) -> Color` | Replace the alpha channel. |
| `lighten(factor: f32) -> Color` | Mix toward white. `factor` clamped 0–1. |
| `darken(factor: f32) -> Color` | Mix toward black. `factor` clamped 0–1. |
| `blend_over(bg: Color) -> Color` | Alpha-composite `self` over `bg`. |
| `is_transparent() -> bool` | True when `alpha == 0`. |

---

### 8.2 TextStyle

```rust
pub struct TextStyle {
    pub fg:     Color,
    pub bg:     Color,
    pub bold:   bool,
    pub italic: bool,
}
```

**Construction:** `TextStyle` is a plain struct — initialise all four fields directly.

```rust
let ts = TextStyle { fg: my_color, bg: Color::TRANSPARENT, bold: false, italic: false };
```

---

### 8.3 BorderSide

Bitflag for selecting which sides of a border to draw.

```rust
BorderSide::NONE | TOP | BOTTOM | LEFT | RIGHT | ALL
```

Methods: `contains(other) -> bool`, `or(other) -> BorderSide`.

---

### 8.4 CornerRadius

Per-corner radii for `DrawCmd::RoundRect`.

```rust
pub struct CornerRadius { pub tl: f32, pub tr: f32, pub bl: f32, pub br: f32 }
```

| Constructor / Method | Description |
|----------------------|-------------|
| `CornerRadius::all(r)` | Uniform radius. |
| `CornerRadius::none()` | Zero radii (sharp corners). |
| `.top_left(r)` / `.top_right(r)` / `.bottom_left(r)` / `.bottom_right(r)` | Builder-style per-corner setters. |
| `is_none()` | All radii are zero. |

---

### 8.5 PowerlineDir

Direction for powerline separator glyphs.

```rust
pub enum PowerlineDir {
    RightFill,    // solid right-pointing arrow
    LeftFill,     // solid left-pointing arrow
    RightChevron, // thin right chevron
    LeftChevron,  // thin left chevron
}
```

---

### 8.6 DrawCmd

The atomic GPU draw unit. All coordinates are pixel-space, top-left origin.

```rust
pub enum DrawCmd {
    FillRect   { x, y, w, h: u32, color: Color },
    StrokeRect { x, y, w, h: u32, color: Color },
    HLine      { x, y, w: u32, color: Color },
    VLine      { x, y, h: u32, color: Color },

    BorderLine {
        x, y, w, h: u32,
        sides: BorderSide, color: Color, thickness: u32,
    },

    RoundRect {
        x, y, w, h: f32,         // f32 for sub-pixel precision
        radii: CornerRadius,
        fill: Color, stroke: Color, stroke_w: f32,
    },

    PowerlineArrow { x, y, w, h: u32, dir: PowerlineDir, color: Color },

    Text { x, y: u32, text: String, style: TextStyle, max_w: Option<u32> },
}
```

> You rarely construct `DrawCmd` directly — use `PixelCanvas` methods or widget render calls instead.

---

### 8.7 PixelCanvas

Immediate-mode draw list. Passed to widgets through `Frame::canvas()`.

**Two layers:** calls on the main canvas are emitted first; calls pushed via `with_overlay()` are appended after all main-layer content, ensuring overlay draws on top regardless of call order in `view()`.

```rust
PixelCanvas::new(vp_w: u32, vp_h: u32) -> Self
canvas.set_clip(clip: Option<Rect>)
canvas.with_overlay() -> ClippedCanvas   // push subsequent calls to the overlay layer
canvas.finish() -> Vec<DrawCmd>           // consume and return the full draw list
```

**Draw methods** (all clip-aware):

| Method | Description |
|--------|-------------|
| `fill(x, y, w, h, color)` | Filled rectangle. |
| `stroke(x, y, w, h, color)` | Stroked rectangle (outline only). |
| `hline(x, y, w, color)` | Horizontal line. |
| `vline(x, y, h, color)` | Vertical line. |
| `border(x, y, w, h, sides, color, thickness)` | Per-side border. |
| `round_rect(x, y, w, h, radii, fill, stroke, stroke_w)` | Rounded rectangle (SDF, anti-aliased). |
| `round_fill(x, y, w, h, radii, fill)` | Rounded rect, fill only. |
| `round_stroke(x, y, w, h, radii, stroke, stroke_w)` | Rounded rect, stroke only. |
| `powerline(x, y, w, h, dir, color)` | Powerline glyph. |
| `text(x, y, s, style)` | Text, clipped to canvas bounds. |
| `text_maxw(x, y, s, style, max_w)` | Text with an explicit right-clip pixel width. |

**ClippedCanvas** — returned by `canvas.sub(rect)` or implicitly used in `with_overlay()`. Exposes the same draw API but all coordinates are clipped to the given `Rect`. Access clip rect geometry: `x()`, `y()`, `width()`, `height()`, `rect()`.

---

### 8.8 Theme

Colour theme passed to all widget renders. Defaults to **Catppuccin Mocha**.

```rust
pub struct Theme {
    // Content
    pub normal_fg:    Color,   // default text
    pub normal_bg:    Color,   // default background
    pub highlight_fg: Color,   // selected item text
    pub highlight_bg: Color,   // selected item background
    pub dim_fg:       Color,   // muted / secondary text

    // Chrome
    pub active_border:   Color,
    pub inactive_border: Color,
    pub active_title:    Color,
    pub inactive_title:  Color,
    pub pane_bg:         Color,

    // Status bar
    pub bar_bg:     Color,
    pub bar_fg:     Color,
    pub bar_accent: Color,
    pub bar_dim:    Color,

    // Workspace / tab
    pub ws_active_fg: Color,
    pub ws_active_bg: Color,

    // Semantic
    pub error_fg:   Color,   // red — errors, destructive
    pub warning_fg: Color,   // yellow — warnings
    pub success_fg: Color,   // green — success, online, healthy

    // Widget-specific
    pub cursor_color:  Color,   // TextInput cursor
    pub selection_bg:  Color,   // TextInput selection highlight
    pub tooltip_bg:    Color,
    pub tooltip_fg:    Color,
    pub modal_overlay: Color,   // Popup backdrop (semi-transparent)
}
```

**Provided themes:**

| Constructor | Description |
|-------------|-------------|
| `Theme::default()` | Catppuccin Mocha (dark). |
| `Theme::latte()` | Catppuccin Latte (light). |
| `Theme::macchiato()` | Catppuccin Macchiato (dark, mid-tone). |

Override `App::theme()` to supply a custom or runtime-selectable theme.

---

## 9. Widgets

### 9.1 Widget & StatefulWidget Traits

```rust
pub trait Widget {
    fn render(self, canvas: &mut PixelCanvas, area: Rect, cell_w: u32, cell_h: u32, t: &Theme);
}

pub trait StatefulWidget {
    type State;
    fn render(
        self,
        canvas: &mut PixelCanvas,
        area: Rect,
        state: &mut Self::State,
        cell_w: u32, cell_h: u32,
        t: &Theme,
    );
}
```

Prefer the `Frame` helpers (`frame.render`, `frame.render_stateful`, `frame.render_block`) over calling trait methods directly.

---

### 9.2 Style

Per-widget colour and text overrides. All fields are optional — `None` falls back to the active `Theme`.

```rust
pub struct Style {
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub bold:   bool,
    pub italic: bool,
}
```

**Builder methods:** `.fg(c)`, `.bg(c)`, `.bold()`, `.italic()`.

**Patch:** `style.patch(other) -> Style` — merge two styles, with `other` taking precedence.

**Conversion:**

| Method | Uses theme slots |
|--------|-----------------|
| `to_text_style(t)` | `t.normal_fg` / `t.normal_bg` — for content text. |
| `to_bar_style(t)` | `t.bar_fg` / `t.bar_bg` — for chrome / status bar text. |

---

### 9.3 Borders

Bitflags for `Block` border sides.

```rust
Borders::NONE | TOP | BOTTOM | LEFT | RIGHT | ALL
```

---

### 9.4 Block

A bordered container with an optional title. Renders and returns the inner content `Rect`.

```rust
Block::new()          // no borders by default
Block::bordered()     // shortcut: Block::new().borders(Borders::ALL)
```

**Builder methods:**

| Method | Description |
|--------|-------------|
| `.borders(Borders)` | Which sides to draw. |
| `.border_style(Style)` | Border line colour / weight. |
| `.style(Style)` | Background colour. |
| `.title(s: &str)` | Text in the top border. |
| `.title_style(Style)` | Title text colour. |
| `.title_alignment(TitleAlignment)` | `Left` (default), `Center`, `Right`. |
| `.border_px(px: u32)` | Border thickness in pixels. Default `1`. |
| `.top_accent(Color)` | Draw a coloured accent line along the top edge. |
| `.rounded(r: f32)` | Corner radius for a rounded border (SDF, anti-aliased). |

**Render:**

```rust
let inner: Rect = block.render(canvas, area, cell_w, cell_h, theme);
// or via Frame:
let inner: Rect = frame.render_block(block, area);
```

Returns the inner `Rect` after subtracting border pixels and title row.

---

### 9.5 Paragraph

Renders a single string of text, word-wrapped to fit the area.

```rust
Paragraph::new(text: &str)
    .style(Style)             // text colour / bg
    .wrap(true / false)       // word-wrap (default: false)
    .scroll(offset_rows: u32) // vertical scroll offset in rows
```

Implements `Widget`.

---

### 9.6 List

Scrollable list of items with optional selection highlight.

```rust
List::new(items: Vec<ListItem>)
    .highlight_style(Style)
    .highlight_symbol(s: &str)   // e.g. "» "
    .selected_bar(c: Color)      // draw a left-edge selection bar in colour c
    .selected_bar_px(px: u32)    // bar thickness (default 2)
    .row_separator(c: Color)     // draw horizontal separator lines between rows
```

```rust
pub struct ListItem<'a> {
    pub content: &'a str,
    pub style:   Style,
}

impl<'a> ListItem<'a> {
    pub fn new(content: &'a str) -> Self
    pub fn style(mut self, s: Style) -> Self
}
```

Implements `StatefulWidget<State = ListState>`.

**ListState:**

```rust
pub struct ListState {
    // selected: private — use select()/selected()
    pub offset: usize,          // first visible item index (scroll offset)
}
impl ListState {
    // implements Default
    pub fn select(&mut self, i: Option<usize>)
    pub fn selected(&self) -> Option<usize>
}
```

---

### 9.7 Table

Columnar data display with optional header row and row selection.

Column widths are specified via the `ColWidth` enum (not `Constraint`):

```rust
pub enum ColWidth {
    Fixed(u32),   // exact pixels
    Cells(u32),   // n × cell_w
    Pct(u8),      // percentage of table width (0–100)
    Fill(u32),    // share of remaining space after fixed/pct columns
}
```

```rust
// col_widths is passed directly to the constructor — there is no .widths() builder.
Table::new(rows: Vec<Row>, col_widths: Vec<ColWidth>)
    .header(Row)               // sticky header row
    .highlight_style(Style)    // selected row style
    .header_style(Style)       // header row text style
    .col_spacing(px: u32)      // gap between columns in pixels (default 1)
    .header_separator(c: Color)  // draw a line below the header in colour c
    .no_header_separator()       // disable the header separator
    .row_separator(c: Color)     // draw horizontal separator lines between data rows
```

```rust
pub struct Row<'a> {
    pub cells:         Vec<Cell<'a>>,
    pub style:         Style,
    pub bottom_margin: u32,    // extra pixels below the row (default 0)
}
impl<'a> Row<'a> {
    pub fn new(cells: Vec<Cell<'a>>) -> Self
    pub fn style(mut self, s: Style) -> Self
    pub fn bottom_margin(mut self, px: u32) -> Self
}

pub struct Cell<'a> {
    pub content: &'a str,
    pub style:   Style,
}
impl<'a> Cell<'a> {
    pub fn new(content: &'a str) -> Self
    pub fn style(mut self, s: Style) -> Self
}
```

Implements `StatefulWidget<State = TableState>`.

**TableState:**

```rust
pub struct TableState {
    // selected: private — use select()/selected()
    pub offset: usize,
}
impl TableState {
    // implements Default
    pub fn select(&mut self, i: Option<usize>)
    pub fn selected(&self) -> Option<usize>
}
```

---

### 9.8 Tabs

Horizontal tab bar with optional powerline arrows and underline.

```rust
Tabs::new(titles: Vec<&str>)
    .select(i: usize)              // active tab index (default 0)
    .style(Style)                  // inactive tab style
    .highlight_style(Style)        // active tab style
    .tab_padding(cells: u32)       // horizontal padding per tab (default 1)
    .powerline(color: Color)       // enable powerline arrow on active tab
    .underline(color: Color)       // draw underline along the full bar
    .divider(color: Color)         // draw vertical dividers between inactive tabs
```

Implements `Widget`. Uses `bar_*` theme slots.

---

### 9.9 Gauge

Horizontal progress bar with optional centred label.

```rust
Gauge::new()
    .ratio(r: f64)               // 0.0–1.0
    .percent(p: u8)              // 0–100 (converts to ratio)
    .style(Style)                // empty portion background
    .filled_style(Style)         // filled portion background
    .label(s: &str)              // centred overlay text
    .label_style(Style)          // label colour
```

Implements `Widget`. The label is rendered in two passes — inverted colour over the filled region, normal colour over the empty region — for clean readability at any fill level.

---

### 9.10 TextInput

Single-line editable text field with cursor, scrolling, and Emacs-style keybindings.

```rust
TextInput::new()
    .placeholder(s: &str)
    .style(Style)
    .focused(f: bool)      // shows cursor and active border when true
    .max_len(n: usize)
```

Implements `StatefulWidget<State = TextInputState>`.

**TextInputState:**

```rust
pub struct TextInputState { pub value: String, … }

impl TextInputState {
    pub fn new() -> Self
    pub fn value(&self) -> &str
    pub fn set_value(&mut self, s: impl Into<String>)  // moves cursor to end
    pub fn clear(&mut self)
    pub fn handle_key(&mut self, k: &KeyEvent) -> bool  // returns true if state changed
    pub fn cursor_char_idx(&self) -> usize
}
```

**handle_key keybindings:**

| Key | Action |
|-----|--------|
| `Char(c)` | Insert character at cursor |
| `Backspace` | Delete character before cursor |
| `Delete` | Delete character after cursor |
| `Left` / `Right` | Move cursor one character |
| `Ctrl+Left` / `Ctrl+Right` | Move cursor one word |
| `Home` / `Ctrl+A` | Move to start |
| `End` / `Ctrl+E` | Move to end |
| `Ctrl+K` | Kill to end of line |
| `Ctrl+U` | Kill to start of line |
| `Ctrl+W` | Delete word backward |

---

### 9.11 Spinner

Animated loading indicator. Advance one frame per `Event::Tick`.

```rust
Spinner::new()
    .style(Style)
    .kind(SpinnerStyle)
    .label(s: impl Into<String>)
```

Implements `StatefulWidget<State = SpinnerState>`.

**SpinnerStyle:**

| Variant | Frames |
|---------|--------|
| `Braille` (default) | `⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏` (10 frames) |
| `Quarters` | `◐◓◑◒` |
| `Arc` | `◜◝◞◟` |
| `Ascii` | `\|/-\\` |
| `Bar` | `▁▂▃▄▅▆▇█▇▆▅▄▃▂` (14 frames) |

**SpinnerState:**

```rust
pub struct SpinnerState { … }
impl SpinnerState {
    pub fn new() -> Self
    pub fn tick(&mut self)   // call once per Event::Tick
    pub fn reset(&mut self)
}
```

---

### 9.12 Scrollbar

Non-interactive scrollbar indicator. Renders a track with a proportional thumb.

```rust
Scrollbar::vertical()           // or ::horizontal()
    .total(n: usize)            // total number of items
    .visible(n: usize)          // items visible in the viewport
    .position(p: usize)         // current scroll offset (first visible item index)
    .track_color(Color)
    .thumb_color(Color)
```

Implements `Widget`. Pass a 1-pixel-wide `Rect` on the right or bottom edge of your content area.

```rust
// Typical usage:
if items.len() > visible_rows {
    let sb_rect = Rect::new(area.x + area.w - 1, area.y, 1, area.h);
    frame.render(
        Scrollbar::vertical()
            .total(items.len())
            .visible(visible_rows)
            .position(list_state.offset),
        sb_rect,
    );
}
```

Hides itself automatically when `total <= visible`.

---

### 9.13 Popup

Geometry helpers and backdrop rendering for modal overlays.

```rust
// Centred rect of cols×rows cells inside parent.
Popup::centered(parent: Rect, cols: u32, rows: u32, cell_w: u32, cell_h: u32) -> Rect

// Centred rect of exact pixel size inside parent.
Popup::centered_px(parent: Rect, w: u32, h: u32) -> Rect

// Render a full-viewport semi-transparent backdrop.
// Call BEFORE rendering popup content.
Popup::render_backdrop(canvas: &mut PixelCanvas, viewport: Rect, t: &Theme)

// Return a pre-styled Block: rounded corners, active border, pane_bg fill.
Popup::block(t: &Theme) -> Block<'static>

// Render backdrop + block in one call. Returns inner content Rect.
Popup::render(canvas, viewport, popup_rect, cell_w, cell_h, t) -> Rect
```

```rust
// Typical usage in view():
if self.show_modal {
    let popup_rect = Popup::centered(frame.area(), 60, 20, frame.cell_w(), frame.cell_h());
    let inner = Popup::render(
        frame.canvas(), frame.area(), popup_rect,
        frame.cell_w(), frame.cell_h(), frame.theme(),
    );
    frame.render(Paragraph::new("Are you sure?"), inner);
}
```

---

### 9.14 TitleBar

Compositor window titlebar decoration. Renders a bar with title text and window control buttons, and returns typed hit regions for compositor mouse routing.

```rust
TitleBar::new(title: &str)
    .focused(f: bool)
    .buttons(TitleBarButtons)
    .style(Style)
    .button_radius(r: u32)    // button circle radius in pixels (default 5)
```

**TitleBarButtons** (bitflags):

```rust
TitleBarButtons::CLOSE | MINIMIZE | MAXIMIZE | ALL
```

**Primary render method:**

```rust
pub fn render_with_regions(
    self,
    canvas: &mut PixelCanvas,
    area: Rect,
    cell_w: u32, cell_h: u32,
    t: &Theme,
) -> Vec<(TitleBarHit, Rect)>
```

Returns hit regions for each interactive area.

**TitleBarHit:**

```rust
pub enum TitleBarHit { Drag, Close, Minimize, Maximize }
```

```rust
// Typical compositor usage:
let hits = TitleBar::new(win.title())
    .focused(win.is_focused())
    .buttons(TitleBarButtons::ALL)
    .render_with_regions(canvas, bar_rect, cell_w, cell_h, theme);

// In mouse handler:
for (hit, rect) in &hits {
    if rect.contains_point(ptr_x, ptr_y) {
        match hit {
            TitleBarHit::Drag    => start_interactive_move(),
            TitleBarHit::Close   => close_window(),
            TitleBarHit::Maximize => toggle_maximize(),
            TitleBarHit::Minimize => minimize_window(),
        }
    }
}
```

Also implements `Widget` (discards regions).

---

### 9.15 Chrome — PaneOpts & BarBuilder

> **`widget::chrome`** — high-level compositor chrome drawing. All types are
> re-exported from the crate root and included in `prelude::*`.

The chrome module eliminates the manual pixel arithmetic involved in drawing
pane borders and status bars. Instead of calling `canvas.border(…)`,
`canvas.fill(…)`, `canvas.text_maxw(…)` by hand, you pass semantic inputs
(`title`, `focused`, workspace states, layout name, clock string) and trixui
handles all the geometry.

The primary entry-points are the two [`Frame`](#5-frame--render-target) methods:
**`frame.draw_pane`** and **`frame.bar`**.

---

#### PaneOpts

Builder for pane decoration options. All colour fields default to
`Color::TRANSPARENT`, which causes the renderer to fall back to the active
`Theme` slot automatically — you only set what you want to override.

```rust
PaneOpts::new(title: impl Into<String>) -> Self
```

**Builder methods:**

| Method | Default | Description |
|--------|---------|-------------|
| `.icon(s)` | none | Nerd Font icon prepended to the title (e.g. `"󰖟 "`). |
| `.focused(f: bool)` | `false` | Active focus state — drives border colour and bold title. |
| `.border_w(px: u32)` | `1` | Border thickness in pixels. |
| `.corner_radius(r: f32)` | `0.0` | SDF rounded corners. `0.0` = sharp. |
| `.active_border(Color)` | `theme.active_border` | Override focused border colour. |
| `.inactive_border(Color)` | `theme.inactive_border` | Override unfocused border colour. |
| `.bg(Color)` | `theme.pane_bg` | Background used to erase behind the title text. |

```rust
// All colours from theme — the common case
frame.draw_pane(rect, PaneOpts::new("nvim").focused(true));

// With icon + rounded corners
frame.draw_pane(rect,
    PaneOpts::new("term")
        .icon("󰖟 ")
        .focused(true)
        .border_w(2)
        .corner_radius(6.0));

// Explicit colour override (e.g. per-window accent)
frame.draw_pane(rect,
    PaneOpts::new("ssh: server.lan")
        .focused(true)
        .active_border(Color::hex(0xf38ba8)));
```

The title is automatically truncated with `…` if it exceeds the available
horizontal space (two cell-widths of padding on each side). When `focused` is
`true` the title text is drawn bold.

---

#### BarBuilder & SectionBuilder

Obtained from `frame.bar(area)`. Fills three horizontal zones — left, center,
right — then flushes everything with `.finish()`.

```rust
frame.bar(area: Rect) -> BarBuilder<'_>
```

**BarBuilder configuration:**

| Method | Default | Description |
|--------|---------|-------------|
| `.bg(Color)` | `theme.bar_bg` | Override bar background. |
| `.separator_color(Color)` | `theme.inactive_border` | Override the 1-px separator line. |
| `.separator_bottom()` | top edge | Draw the separator on the bottom edge instead. |
| `.left(f)` | empty | Fill the left zone via a `SectionBuilder` closure. |
| `.center(f)` | empty | Fill the centre zone. |
| `.right(f)` | empty | Fill the right zone. |
| `.finish()` | — | **Required.** Flush all items to the canvas. |

`.finish()` must be called explicitly. The builder does not auto-flush on drop.

---

**SectionBuilder** is the argument to each zone closure. Calls chain — each
method returns `self`.

| Method | Description |
|--------|-------------|
| `.item(BarItem)` | Push a pre-built `BarItem` directly. |
| `.separator()` | Force a 1-px separator before the *next* item. |
| `.text(s)` | Plain text in `theme.bar_fg`. |
| `.accent(s, fg)` | Bold text in explicit `fg` colour. |
| `.pill(s, fg, bg, padding)` | Filled pill — solid background, `fg` text. |
| `.workspace(n, active)` | Workspace number pill. Active → filled accent. Inactive → dim bare text. |
| `.workspace_state(n, active, occupied)` | As above with explicit occupied state (occupied → accent-coloured bare text). |
| `.clock(time_str)` | Bold accent-filled clock pill (default right-section style). |
| `.clock_plain(time_str)` | Plain dim clock text, no fill. |
| `.layout(name)` | Layout mode label with matching Nerd Font icon. `"BSP"` → `"󰙀 BSP"` in `theme.bar_accent`. |

**Layout icon map** (`.layout()`):

| Name | Icon |
|------|------|
| `"BSP"` | `󰙀 ` |
| `"Columns"` | `󰕘 ` |
| `"Rows"` | `󰕛 ` |
| `"ThreeCol"` | `󱗼 ` |
| `"Monocle"` | `󱕻 ` |

```rust
// Full three-zone bar
frame.bar(frame.bar_area())
    .left(|b| b
        .workspace_state(1, snap.active == 0, snap.workspaces[0].occupied)
        .workspace_state(2, snap.active == 1, snap.workspaces[1].occupied)
        .workspace_state(3, snap.active == 2, snap.workspaces[2].occupied))
    .center(|b| b.layout(&snap.layout_name))
    .right(|b| b
        .text(&self.battery_text)
        .separator()
        .clock(&self.clock_text))
    .finish();

// Custom colours — override only what you need
frame.bar(frame.bar_area())
    .bg(Color::hex(0x1e1e2e))
    .left(|b| b
        .pill(" main ", Color::hex(0x11111b), Color::hex(0xb4befe), 6)
        .item(BarItem::text(" src/main.rs").fg(Color::hex(0xa6adc8))))
    .finish();
```

---

#### BarItem — manual construction

When you need full control over a single item, construct `BarItem` directly
and push it with `.item(…)`:

```rust
pub struct BarItem { … }

BarItem::text(s: impl Into<String>)               // plain, no fill
BarItem::accent(s: impl Into<String>, fg: Color)  // bold, no fill
BarItem::pill(s, fg: Color, bg: Color, padding: u32) // filled background

// Builder methods (all return Self):
.fg(Color)        // foreground; TRANSPARENT → bar default_fg
.bg(Color)        // background fill; TRANSPARENT → no fill
.padding(px: u32) // horizontal padding each side
.bold(bool)       // bold text
.sep()            // prepend a 1-px vertical separator before this item
.sep_color(Color) // separator colour; TRANSPARENT → theme.inactive_border

// Pixel width (for layout math):
item.width(cell_w: u32) -> u32
```

---

#### Free functions (raw PixelCanvas)

For code paths that own a `PixelCanvas` directly (e.g. inside a Smithay render
element) the same logic is available as free functions:

```rust
// widget::chrome — also re-exported from trixui root
use trixui::{draw_pane, draw_bar, PaneOpts, BarItem};

draw_pane(
    canvas:  &mut PixelCanvas,
    rect:    Rect,
    opts:    &PaneOpts,
    glyph_w: u32,
    line_h:  u32,
    theme:   &Theme,
)

draw_bar(
    canvas:       &mut PixelCanvas,
    rect:         Rect,
    cell_w:       u32,
    cell_h:       u32,
    theme:        &Theme,
    bg_override:  Color,   // TRANSPARENT = use theme.bar_bg
    sep_override: Color,   // TRANSPARENT = use theme.inactive_border
    sep_on_top:   bool,
    left:         &[BarItem],
    center:       &[BarItem],
    right:        &[BarItem],
)
```

---

### 10.1 Constraint

```rust
pub enum Constraint {
    Fixed(u32),       // exact pixels
    Length(u32),      // n cells × cell_w or cell_h
    Percentage(u8),   // 0–100 % of available space
    Ratio(u32, u32),  // fraction a/b of available space
    Min(u32),         // minimum n cells (expands if slack allows)
    Max(u32),         // capped at n cells
    Fill(u32),        // absorb remaining space, weighted
}
```

> **Unit note:** `Fixed` is pixels. `Length`, `Min`, `Max` are **cells** — multiplied by `cell_w` / `cell_h` internally. Do not mix pixels and cells in the same value.

`Fill(2)` receives twice the remaining space as `Fill(1)`.

---

### 10.2 Flex

Controls distribution of leftover space among items without fill weight.

```rust
pub enum Flex {
    Start,         // pack to start (default)
    Center,        // centre the group
    End,           // pack to end
    SpaceBetween,  // distribute slack evenly between items
    SpaceAround,   // distribute slack evenly around items
    Stretch,       // alias for Start
}
```

---

### 10.3 Direction

```rust
pub enum Direction { Horizontal, Vertical }
```

---

### 10.4 Layout

```rust
Layout::horizontal(constraints: impl Into<Vec<Constraint>>)
Layout::vertical(constraints: impl Into<Vec<Constraint>>)
    .flex(Flex)
    .spacing(px: u32)     // gap between items in pixels

layout.split(area: Rect, cell_w: u32, cell_h: u32) -> Vec<Rect>
```

The returned `Vec<Rect>` has the same length as the constraints slice. The last element always expands to fill any rounding remainder.

```rust
// Example: 3-column split, centre column fills remaining space
let [left, centre, right] = Layout::horizontal([
    Constraint::Length(20),  // 20 cells wide
    Constraint::Fill(1),     // remaining space
    Constraint::Fixed(120),  // exactly 120 px
]).split(frame.content_area(), frame.cell_w(), frame.cell_h())[..] else { return };
```

---

## 11. Helper Functions

These are exported from `trixui::widget`.

| Function | Description |
|----------|-------------|
| `bar_text_y(rect: Rect, cell_h: u32) -> u32` | Pixel Y to vertically centre one text row in `rect`. Use for status bar text. |
| `center_text_x(rect: Rect, text_w_px: u32) -> u32` | Pixel X to horizontally centre text of width `text_w_px` in `rect`. |
| `truncate_chars(s: &str, max: usize) -> String` | Truncate to `max` Unicode scalars, appending `…` if truncated. |

---

## 12. Flat Re-exports & Prelude

### Top-level re-exports (`use trixui::*`)

```rust
// app
App, Cmd, Event, Frame, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent

// layout
CellRect, Rect as PixRect, ScreenLayout

// renderer
Color as PixColor, DrawCmd, PixelCanvas, TextStyle, Theme

// widget
bar_text_y, center_text_x,
Block, Borders, Cell, Constraint, Direction, Flex, Gauge, Layout,
List, ListItem, ListState, Paragraph, Row, StatefulWidget, Style, Table, TableState, Tabs,
Widget,
// chrome
BarBuilder, BarItem, PaneOpts, SectionBuilder, draw_pane, draw_bar

// backends (feature-gated)
WinitBackend    // feature = "backend-winit"
WaylandBackend  // feature = "backend-wayland"
```

### Prelude (`use trixui::prelude::*`)

```rust
use trixui::prelude::*;
```

Includes everything above plus:
- `Terminal`
- `SmithayApp` (feature = `"backend-smithay"`)
- All of `widget::*` (including `TextInput`, `Spinner`, `Scrollbar`, `Popup`, `TitleBar`, `PaneOpts`, `BarBuilder`, `BarItem`, `SectionBuilder`, etc.)

---

## 13. Error Handling

```rust
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
```

Used by backend constructors and `SmithayApp::build()`. Errors from GL shader compilation and font atlas construction are returned as `Box<dyn Error>` with a descriptive string message.

---

## 14. Renderer Internals (GL)

The `renderer::gl` module is not part of the public widget API but is relevant when integrating with a compositor or building a custom backend.

### ChromeRenderer

```rust
pub struct ChromeRenderer {
    pub cell_w: u32,
    pub cell_h: u32,
    …
}
```

Created once after the GL context is current. Requires `GlyphAtlas` and `Shaper`.

```rust
ChromeRenderer::new(atlas: GlyphAtlas, shaper: Shaper, window_w: f32, font_size: f32)
    -> Result<Self, String>
```

**Primary method:**

```rust
renderer.flush(cmds: &[DrawCmd], vp_w: u32, vp_h: u32)
```

Uploads instance data and issues four instanced draw passes into the currently-bound FBO:

1. **BG pass** — `FillRect`, `StrokeRect`, `HLine`, `VLine`, `BorderLine`, shade blocks (░▒▓), box-drawing geometry.
2. **RoundRect pass** — `RoundRect` (SDF shader, per-corner radii, anti-aliased fill + stroke).
3. **Glyph pass** — `Text` (HarfBuzz-shaped, alpha texture atlas).
4. **Tri pass** — `PowerlineArrow` + Powerline chars embedded in `Text` strings.

NDC conversion (`ndc = (px / vp) * 2.0 - 1.0`) lives only here. No Y-flip — the DRM FBO uses a top-left origin.

### GlyphAtlas

```rust
GlyphAtlas::new(
    regular:  &[u8],
    bold:     Option<&[u8]>,
    italic:   Option<&[u8]>,
    size_px:  f32,
    line_height_factor: f32,  // e.g. 1.2
) -> Result<Self, String>
```

Rasterises glyphs on demand into a 2048×2048 RGBA texture. New glyphs are patched into the texture with `glTexSubImage2D`.

### Shaper

```rust
Shaper::new(font_data: &[u8]) -> Self
```

HarfBuzz text shaper. Used by `ChromeRenderer` for glyph sequence resolution.

---

*End of trixui API Reference*
