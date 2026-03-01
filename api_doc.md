# trixui API Reference

All coordinates are **pixel-space**: top-left origin, X right, Y down.  
NDC conversion lives exclusively in GLSL vertex shaders — never do it in Rust.

---

## Table of Contents

1. [Core types](#core-types)
2. [App trait](#app-trait)
3. [Frame](#frame)
4. [Event & Cmd](#event--cmd)
5. [Layout](#layout)
6. [PixelCanvas](#pixelcanvas)
7. [Widgets](#widgets)
8. [Theme](#theme)
9. [Renderer internals](#renderer-internals)
10. [Backends](#backends)
11. [Wayland compositor usage](#wayland-compositor-usage)
12. [Widget code contract](#widget-code-contract)

---

## Core types

```rust
pub struct Rect     { pub x: u32, pub y: u32, pub w: u32, pub h: u32 }
pub struct CellRect { pub x: u16, pub y: u16, pub w: u16, pub h: u16 }
pub struct Color(pub u8, pub u8, pub u8, pub u8);
```

### `Rect`

| Method | Description |
|---|---|
| `Rect::new(x, y, w, h)` | Constructor |
| `.is_empty()` | `w == 0 \|\| h == 0` |
| `.split_top(top_h)` | Returns `(top, bottom)` |
| `.split_cols(n)` | Equal column split |
| `.split_ratios(&[f32])` | Proportional split |
| `.inset(px)` | Shrink all sides by `px` |

### `CellRect`

| Method | Description |
|---|---|
| `CellRect::new(x, y, w, h)` | Constructor |
| `.is_empty()` | `w == 0 \|\| h == 0` |
| `.to_px(cell_w, cell_h)` | Raw conversion — prefer `ScreenLayout::cell_rect_to_px` |

### `Color`

```rust
Color::rgb(r, g, b)        // opaque
Color::rgba(r, g, b, a)    // with alpha
Color::hex(0xRRGGBB)       // opaque from 24-bit hex literal; top byte ignored
Color::TRANSPARENT          // a == 0
color.is_transparent() -> bool
```

### `TextStyle`

```rust
pub struct TextStyle {
    pub fg:     Color,
    pub bg:     Color,   // TRANSPARENT = no background quad
    pub bold:   bool,
    pub italic: bool,
}
TextStyle::fg(color)   // transparent bg, not bold/italic
```

### `BorderSide`

Bitmask used by `DrawCmd::BorderLine` and `canvas.border()`.
Same bit layout as `widget::Borders` — the cast is free.

```rust
BorderSide::NONE / TOP / BOTTOM / LEFT / RIGHT / ALL
side.contains(other) -> bool
side.or(other)       -> BorderSide
```

### `CornerRadius`

Per-corner pixel radii for `DrawCmd::RoundRect`.

```rust
CornerRadius::all(r)    // uniform radius
CornerRadius::none()    // all zeros → plain rect
    .top_left(r)
    .top_right(r)
    .bottom_left(r)
    .bottom_right(r)    // builder methods, chainable
radius.is_none() -> bool
```

### `PowerlineDir`

Direction/style for `DrawCmd::PowerlineArrow` and `canvas.powerline()`.

```rust
PowerlineDir::RightFill    // filled ▶  (≈ U+E0B0)
PowerlineDir::LeftFill     // filled ◀  (≈ U+E0B2)
PowerlineDir::RightChevron // outline ❯ (≈ U+E0B1)
PowerlineDir::LeftChevron  // outline ❮ (≈ U+E0B3)
```

---

## App trait

```rust
pub trait App: Sized + 'static {
    type Message: 'static;
    fn update(&mut self, event: Event<Self::Message>) -> Cmd<Self::Message>;
    fn view(&self, frame: &mut Frame);
    // Optional overrides:
    fn init(&mut self) -> Cmd<Self::Message> { Cmd::none() }
    fn theme(&self) -> Theme                 { Theme::default() }
    fn tick_rate(&self) -> u64               { 60 }  // Hz
}
```

**Standalone (winit)** — use `WinitBackend::run_app`, not `Terminal::run`:
```rust
WinitBackend::new()?.run_app(MyApp::new())?;
```

> **Note:** `Terminal<WinitBackend>` compiles but `Terminal::run()` cannot
> receive winit events. Always use `WinitBackend::run_app()` for standalone apps.

**Compositor (Smithay)** — drive the loop manually each frame; see
[§Wayland compositor usage](#wayland-compositor-usage).

### `Terminal`

For non-winit backends (e.g. `WaylandBackend` in a headless test):

```rust
Terminal::new(backend)?.run(app)?;
```

Do **not** use this with `WinitBackend`.

---

## Frame

```rust
frame.area()         -> Rect   // full viewport
frame.content_area() -> Rect   // above the status bar
frame.bar_area()     -> Rect   // status bar row
frame.canvas()       -> &mut PixelCanvas
frame.layout()       -> &ScreenLayout
frame.theme()        -> &Theme
frame.cell_w()       -> u32
frame.cell_h()       -> u32
```

### Render helpers

These are the idiomatic way to render widgets — they thread `cell_w`, `cell_h`,
and `theme` automatically.

```rust
// Render any Widget:
frame.render(widget, area: Rect);

// Render a StatefulWidget:
frame.render_stateful(widget, area: Rect, &mut state);

// Render a Block, returns the inner content Rect:
let inner: Rect = frame.render_block(Block::bordered().title(" Pane "), area);
```

Direct low-level call (when you need explicit control):
```rust
SomeWidget::new(...)
    .render(frame.canvas(), area, frame.cell_w(), frame.cell_h(), frame.theme());
```

### `ScreenLayout`

```rust
ScreenLayout::new(vp_w, vp_h, cell_w, cell_h, bar_h_cells) -> ScreenLayout

layout.vp       // Rect — full viewport
layout.content  // Rect — above bar
layout.bar      // Rect — status bar
layout.cell_w   // u32
layout.cell_h   // u32

layout.content_cols()      -> u16
layout.content_rows()      -> u16
layout.content_cell_rect() -> CellRect
layout.cell_rect_to_px(cr: CellRect) -> Rect  // the canonical CellRect→Rect conversion
```

Invariant: `content.h + bar.h == vp.h` always holds.

---

## Event & Cmd

```rust
pub enum Event<Msg> {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Resize(u32, u32),
    Tick,
    Message(Msg),
}

pub enum Cmd<Msg> { None, Quit, Msg(Msg), Batch(Vec<Cmd<Msg>>) }
Cmd::none()
Cmd::quit()
Cmd::msg(value)
Cmd::batch(vec![...])
```

### `KeyEvent`

```rust
pub struct KeyEvent { pub code: KeyCode, pub modifiers: KeyModifiers }
KeyEvent::plain(KeyCode::Char('q'))          // NONE modifiers
KeyEvent::new(code, mods)

pub enum KeyCode {
    Char(char), Enter, Backspace, Delete, Esc, Tab, BackTab,
    Up, Down, Left, Right, Home, End, PageUp, PageDown, Insert,
    F(u8), Null,
}

bitflags! { pub struct KeyModifiers: u8 { NONE SHIFT CTRL ALT SUPER } }
```

### `MouseEvent`

```rust
pub struct MouseEvent {
    pub kind:   MouseEventKind,   // Down | Up | Drag | Moved | ScrollUp | ScrollDown
    pub x:      u32,
    pub y:      u32,
    pub button: MouseButton,      // Left | Right | Middle | None
}
```

---

## Layout

```rust
Layout::horizontal(constraints)
Layout::vertical(constraints)
    .flex(Flex::Start | Center | End | SpaceBetween | SpaceAround | Stretch)
    .spacing(px)
    .split(area, cell_w, cell_h) -> Vec<Rect>

pub enum Constraint {
    Fixed(u32),        // exact pixels
    Length(u32),       // n cells (multiplied by cell_w or cell_h)
    Percentage(u8),    // % of available space
    Ratio(u32, u32),   // a/b of available space
    Min(u32),          // minimum n cells
    Max(u32),          // maximum n cells
    Fill(u32),         // weighted share of remaining space
}
```

`Fixed` is in **pixels**. `Length`, `Min`, `Max` are in **cells** — multiplied
by `cell_w` or `cell_h` internally. Do not mix pixel and cell values.

`Flex::Stretch` is an alias for `Flex::Start`.

---

## PixelCanvas

All drawing goes through `PixelCanvas`. Every method emits a typed `DrawCmd`.

```rust
let mut canvas = PixelCanvas::new(vp_w, vp_h);
canvas.set_clip(Some(rect));
let cmds: Vec<DrawCmd> = canvas.finish();
```

### Primitive methods

| Method | `DrawCmd` emitted | GL pass |
|---|---|---|
| `canvas.fill(x, y, w, h, color)` | `FillRect` | BG quads |
| `canvas.stroke(x, y, w, h, color)` | `StrokeRect` | BG quads |
| `canvas.hline(x, y, w, color)` | `HLine` | BG quads |
| `canvas.vline(x, y, h, color)` | `VLine` | BG quads |
| `canvas.border(x, y, w, h, sides, color, thickness)` | `BorderLine` | BG quads |
| `canvas.round_rect(x, y, w, h, radii, fill, stroke, stroke_w)` | `RoundRect` | SDF pass |
| `canvas.round_fill(x, y, w, h, radii, fill)` | `RoundRect` | SDF pass |
| `canvas.round_stroke(x, y, w, h, radii, stroke, stroke_w)` | `RoundRect` | SDF pass |
| `canvas.powerline(x, y, w, h, dir, color)` | `PowerlineArrow` | Tri pass |
| `canvas.text(x, y, str, style)` | `Text` | Glyph pass |
| `canvas.text_maxw(x, y, str, style, max_w)` | `Text` | Glyph pass |

> **`canvas.text()` / `canvas.text_maxw()`** — pass **actual text only**.
> Do not embed box-drawing (U+2500–U+259F) or Powerline (U+E0B0–U+E0B3)
> codepoints. Use the primitive methods above instead. The renderer retains
> a legacy decode path for those codepoints only to support raw crossterm /
> ratatui output piped through the GL backend.

### `ChildCanvas` (clipped sub-view)

```rust
let mut child = canvas.child(clip_rect);

// All primitive methods available, auto-clipped to clip_rect:
child.fill / stroke / hline / vline / border
child.round_rect / round_fill / round_stroke
child.powerline
child.text(x, y, s, style)
child.text_maxw(x, y, s, style, max_w)

child.x()      -> u32
child.y()      -> u32
child.width()  -> u32
child.height() -> u32
child.rect()   -> Rect
```

---

## Widgets

```rust
pub trait Widget {
    fn render(self, canvas: &mut PixelCanvas, area: Rect, cell_w: u32, cell_h: u32, t: &Theme);
}
pub trait StatefulWidget {
    type State;
    fn render(self, canvas: &mut PixelCanvas, area: Rect,
              state: &mut Self::State, cell_w: u32, cell_h: u32, t: &Theme);
}
```

All widgets emit only primitive `DrawCmd` variants. No box-drawing characters
are ever produced by widget code.

---

### `Block`

Bordered container with optional title. Emits `DrawCmd::BorderLine` (default)
or `DrawCmd::RoundRect` when `.rounded(r)` is set.

```rust
Block::new()
Block::bordered()          // shorthand for Block::new().borders(Borders::ALL)

// Builder methods:
.borders(Borders::ALL)     // bitflags: TOP | BOTTOM | LEFT | RIGHT | NONE
.border_style(Style)       // colour/style of border lines
.style(Style)              // background colour of the block interior
.title(" Title ")          // drawn on the top border
.title_style(Style)        // colour/style of title text
.border_px(1)              // border thickness in pixels (default 1)
.top_accent(color)         // extra 1px HLine drawn over the top border
.rounded(r)                // corner radius → SDF RoundRect path

// Render — returns the inner content Rect:
block.render(canvas, area, cell_w, cell_h, theme) -> Rect
// Or via Frame:
let inner = frame.render_block(block, area);
```

> `Borders` is a bitflags type. Test for no borders with `.is_empty()`,
> not `== Borders::NONE`.

---

### `Paragraph`

```rust
Paragraph::new(text: &str)
    .style(Style)
    .wrap(bool)       // word-wrap to area width
    .scroll(n_lines)  // vertical scroll offset

// Render:
frame.render(paragraph, area);
// or:
paragraph.render(canvas, area, cell_w, cell_h, theme);
```

Uses `theme.normal_fg` / `theme.normal_bg` by default.

---

### `List` (stateful)

```rust
List::new(vec![
    ListItem::new("item").style(Style),
])
    .highlight_style(Style)        // style of the selected row
    .highlight_symbol("▶ ")        // symbol prepended to the selected row
    .selected_bar(color)           // VLine accent on left edge of selected row
    .selected_bar_px(2)            // width of accent bar in pixels (default 2)
    .row_separator(color)          // HLine between rows

// Render (stateful):
frame.render_stateful(list, area, &mut state);
// or:
list.render(canvas, area, &mut state, cell_w, cell_h, theme);

// State:
let mut state = ListState::default();
state.select(Some(idx));
state.selected() -> Option<usize>
state.offset     // scroll offset — managed automatically
```

The `highlight_symbol` column width is always reserved so text stays aligned
regardless of which row is selected.

Uses `theme.normal_fg/bg` and `theme.highlight_fg/bg` by default.

---

### `Table` (stateful)

```rust
Table::new(rows: Vec<Row>, col_widths: Vec<ColWidth>)
    .header(Row::new(header_cells))
    .highlight_style(Style)
    .header_style(Style)
    .col_spacing(px)               // pixel gap between columns (default 1)
    .header_separator(color)       // HLine after header (enabled by default)
    .no_header_separator()         // disable header HLine
    .row_separator(color)          // HLine between data rows

// Render (stateful):
frame.render_stateful(table, area, &mut state);
// or:
table.render(canvas, area, &mut state, cell_w, cell_h, theme);

// Column width variants:
pub enum ColWidth {
    Fixed(u32),    // exact pixels
    Cells(u32),    // n × cell_w
    Pct(u8),       // percentage of table width (0–100)
    Fill(u32),     // weighted share of remaining space
}

// Row / Cell construction:
Row::new(vec![Cell::new("text").style(Style)])
    .style(Style)
    .bottom_margin(px)   // extra pixels below this row

Cell::new("text").style(Style)

// State:
let mut state = TableState::default();
state.select(Some(idx));
state.selected() -> Option<usize>
state.offset     // scroll offset — managed automatically
```

Header text uses `theme.dim_fg` by default (muted, column-header convention).

---

### `Tabs`

Horizontal tab bar. Tab backgrounds are `FillRect`; dividers are `VLine`
primitives; the Powerline separator is `DrawCmd::PowerlineArrow`.

```rust
Tabs::new(vec!["Tab A", "Tab B"])
    .select(idx)
    .style(Style)                  // base tab style (uses bar_fg/bar_bg)
    .highlight_style(Style)        // active tab style (uses ws_active_fg/bg)
    .tab_padding(cells)            // padding cells each side of title (default 1)
    .divider(color)                // VLine between inactive tabs
    .underline(color)              // HLine along the bottom of the entire bar
    .powerline(color)              // PowerlineArrow ▶ after the active tab

// Render:
frame.render(tabs, area);
// or:
tabs.render(canvas, area, cell_w, cell_h, theme);
```

> `.divider()`, `.underline()`, and `.powerline()` all take a `Color`,
> not a string. There is no `divider("│")` string overload.

Uses `theme.bar_fg` / `theme.bar_bg` for inactive tabs and
`theme.ws_active_fg` / `theme.ws_active_bg` for the active tab by default.

---

### `Gauge`

Horizontal progress bar rendered entirely with `FillRect` — no Unicode block
characters.

```rust
Gauge::new()
    .ratio(0.65)                         // 0.0 – 1.0
    .percent(65)                         // convenience: ratio(n as f64 / 100.0)
    .style(Style)                        // empty-region background
    .filled_style(Style)                 // filled-region colour
    .label("65%")                        // centred text overlay
    .label_style(Style)

// Render:
frame.render(gauge, area);
// or:
gauge.render(canvas, area, cell_w, cell_h, theme);
```

---

### `Style`

```rust
Style::default()
    .fg(color)
    .bg(color)
    .bold()
    .italic()

style.patch(other: Style) -> Style   // merge; `other` wins on conflict

// Convert to TextStyle using content theme slots (Paragraph, List, Table):
style.to_text_style(theme) -> TextStyle

// Convert to TextStyle using bar theme slots (status bar, chrome text):
style.to_bar_style(theme) -> TextStyle
```

### Helpers

```rust
bar_text_y(inner: Rect, cell_h: u32) -> u32
    // y-offset that vertically centres one text row within `inner`

center_text_x(inner: Rect, text_w_px: u32) -> u32
    // x-offset that horizontally centres text of `text_w_px` within `inner`

truncate_chars(s: &str, max: usize) -> String
    // truncate to `max` Unicode scalar values, appending '…' if needed
```

---

## Theme

```rust
pub struct Theme {
    // Content slots — used by Paragraph, List, Table:
    pub normal_fg:    Color,
    pub normal_bg:    Color,
    pub highlight_fg: Color,
    pub highlight_bg: Color,
    pub dim_fg:       Color,   // muted; used for column headers

    // Chrome / border slots — used by Block, Tabs, compositor decorations:
    pub active_border:   Color,
    pub inactive_border: Color,
    pub active_title:    Color,
    pub inactive_title:  Color,
    pub pane_bg:         Color,

    // Status bar slots:
    pub bar_bg:     Color,
    pub bar_fg:     Color,
    pub bar_accent: Color,
    pub bar_dim:    Color,

    // Workspace / tab pill slots:
    pub ws_active_fg: Color,
    pub ws_active_bg: Color,
}

Theme::default()    // Catppuccin Mocha
Theme::latte()      // Catppuccin Latte (light)
Theme::macchiato()  // Catppuccin Macchiato
```

Override individual fields in your `App`:
```rust
fn theme(&self) -> Theme {
    Theme { active_border: Color::hex(0xcba6f7), ..Theme::default() }
}
```

---

## Renderer internals

Only needed when integrating with an external GL context (compositor path).

### `GlyphAtlas`

```rust
GlyphAtlas::new(
    regular_data:  &[u8],
    bold_data:     Option<&[u8]>,
    italic_data:   Option<&[u8]>,
    size_px:       f32,
    line_spacing:  f32,
) -> Result<GlyphAtlas, String>

atlas.cell_w    // u32 — advance width of '0'
atlas.cell_h    // u32 — line height + 3px padding
atlas.ascender  // i32 — pixels above baseline
atlas.dirty     // bool — true when new glyphs were rasterised this frame

atlas.glyph(ch, bold, italic)           -> Option<GlyphUv>
atlas.glyph_by_id(id, bold, italic)     -> Option<GlyphUv>
```

**Codepoints NOT rasterised into the atlas** (rendered as GL geometry by the
legacy TUI shim inside `ChromeRenderer::flush`):

| Range | Characters | Rendering |
|---|---|---|
| U+2500–U+257F | Box-drawing | 1px `BgInst` rects via `box_to_lines()` |
| U+2580–U+259F | Block elements (█▀▄▌▐▘▝▖▗) | Solid `BgInst` quads |
| U+2591–U+2593 | Shade blocks ░▒▓ | Premultiplied `BgInst` at 25/50/75% alpha |
| U+E0B0–U+E0B3 | Powerline arrows | GL triangles via tri VAO |

U+2800–U+28FF (braille) is still atlas-rendered.

> Widget code must never rely on this legacy decode path. Use the primitive
> `DrawCmd` variants directly.

### `Shaper`

```rust
// font_data MUST be 'static — leak before passing:
let data: &'static [u8] = Box::leak(bytes.into_boxed_slice());
let shaper = Shaper::new(data);

shaper.shape(text: &str) -> Vec<ShapedGlyph>

pub struct ShapedGlyph {
    pub glyph_id:      u16,    // font-internal index, NOT Unicode codepoint
    pub cluster_width: usize,  // chars consumed (>1 for ligatures)
    pub advance_px:    f32,    // HarfBuzz x_advance in font units
}
```

### `ChromeRenderer`

```rust
ChromeRenderer::new(
    atlas:           GlyphAtlas,
    shaper:          Shaper,
    hb_units_per_em: f32,   // from font head table (e.g. IosevkaJless = 1000)
    size_px:         f32,
) -> Result<ChromeRenderer, String>

renderer.cell_w  // u32
renderer.cell_h  // u32

renderer.flush(cmds: &[DrawCmd], vp_w: u32, vp_h: u32);
```

#### Render passes

`flush()` executes four instanced draw passes per frame in order:

| Pass | Contents |
|---|---|
| 1 — BG quads | `FillRect`, `StrokeRect`, `HLine`, `VLine`, `BorderLine` (decomposed to per-side rects), box-draw / block / shade legacy shim |
| 2 — SDF round-rects | `RoundRect` — per-corner radii, fill + stroke in one SDF shader |
| 3 — Glyph quads | `Text` — HarfBuzz-shaped, glyph atlas |
| 4 — Triangles | `PowerlineArrow` — solid arrows (3 verts) and outline chevrons (12 verts) |

#### `RoundRect` / SDF pass

Each instance carries `(x, y, w, h)`, per-corner radii `(tl, tr, bl, br)`, fill
colour, stroke colour, and stroke width. The fragment shader evaluates a box-SDF,
giving sub-pixel anti-aliased edges without multisampling.

Setting `stroke_w == 0.0` and `fill == TRANSPARENT` is a no-op (skipped before
upload). Use `FillRect` for non-rounded rects — it uses the cheaper BG pass.

#### `BorderLine` decomposition

`DrawCmd::BorderLine` is decomposed inside `flush()` to one `BgInst` rect per
enabled side. Widget code should use `canvas.border()` or `canvas.round_stroke()`
rather than constructing box-draw text runs.

#### Blend mode

All passes use premultiplied alpha: `ONE / ONE_MINUS_SRC_ALPHA` throughout.
Shade blocks ░▒▓ (legacy shim only) are output as premultiplied RGBA with
alpha of 0.25 / 0.50 / 0.75 respectively.

---

## Backends

### `Backend` trait

```rust
pub trait Backend: Sized {
    fn size(&self)      -> (u32, u32);
    fn cell_size(&self) -> (u32, u32);
    fn poll_event<Msg: 'static>(&mut self) -> Option<Event<Msg>>;
    fn render(&mut self, cmds: &[DrawCmd], vp_w: u32, vp_h: u32);
}
```

### `WinitBackend` (feature = `backend-winit`)

```rust
WinitBackend::new() -> Result<Self>
WinitBackend::with_font(data: &[u8], size_px: f32) -> Result<Self>

// Correct entry-point — blocks until quit:
WinitBackend::new()?.run_app(app)?;

// Do NOT use Terminal::run() with WinitBackend.
```

Events delivered to `App::update` from `run_app`:

| Event | Trigger |
|---|---|
| `Event::Key` | Key press (release is ignored) |
| `Event::Mouse(Down/Up)` | Mouse button press / release |
| `Event::Mouse(Moved)` | Cursor movement |
| `Event::Mouse(ScrollUp/Down)` | Mouse wheel |
| `Event::Resize(w, h)` | Window resize |
| `Event::Tick` | Each frame at `App::tick_rate()` Hz |

### `WaylandBackend` (feature = `backend-wayland`)

For use inside a Smithay compositor. The compositor retains full control of
Wayland protocol handling, XDG shell, DRM/KMS, etc. — trixui only owns the
chrome `DrawCmd` layer.

```rust
WaylandBackend::new(renderer: ChromeRenderer, vp_w: u32, vp_h: u32) -> Self

backend.push_key(KeyEvent)           // deliver a key event from your input handler
backend.set_size(w, h)               // call from your output resize handler;
                                     // queues Event::Resize internally
backend.renderer_mut() -> &mut ChromeRenderer
backend.size()      -> (u32, u32)    // via Backend trait
backend.cell_size() -> (u32, u32)    // via Backend trait
backend.poll_event::<Msg>() -> Option<Event<Msg>>  // via Backend trait
```

`WaylandBackend` has **no `push_mouse` method**. To forward mouse input,
either convert scroll events to `KeyCode::Up` / `KeyCode::Down`, add a
`push_mouse` wrapper in the trixui source, or drop mouse forwarding to the
chrome layer.

### `SmithayApp` (feature = `backend-smithay`)

A higher-level Smithay-native wrapper. Not used in the compositor when
`WaylandBackend` is already in use — do not mix the two.

```rust
use trixui::smithay::SmithayApp;

let mut ui = SmithayApp::new(font_bytes, size_px, vp_w, vp_h, MyApp::new())?;

// Each DRM frame:
ui.push_key(KeyEvent::plain(KeyCode::Char('j')));
ui.render_frame();   // flushes via ChromeRenderer::flush

// On resize:
ui.resize(new_w, new_h);
```

---

## Wayland compositor usage

`Terminal::run()` is **not** used with `WaylandBackend`. Drive the loop
manually each DRM frame.

### Initialisation (once, after GL context is current)

```rust
use trixui::renderer::gl::{GlyphAtlas, Shaper};
use trixui::renderer::ChromeRenderer;
use trixui::backend::wayland::WaylandBackend;
use trixui::backend::Backend as TrixuiBackend;
use trixui::prelude::*;

let font_bytes: &'static [u8] = Box::leak(fs::read(&config.font.path)?.into_boxed_slice());
let atlas    = GlyphAtlas::new(font_bytes, None, None, size_px, 1.2)?;
let shaper   = Shaper::new(font_bytes);   // requires 'static data
let renderer = ChromeRenderer::new(atlas, shaper, 1000.0, size_px)?;
let backend  = WaylandBackend::new(renderer, vp_w, vp_h);
```

### Per-frame render loop

```rust
// 1. Push input (from your Smithay keyboard handler):
backend.push_key(KeyEvent::new(code, mods));

// 2. Drain compositor→app messages (via a VecDeque on compositor state):
while let Some(msg) = state.pending_chrome_msgs.pop_front() {
    app.update(Event::Message(msg));
}

// 3. Drain backend events → update app:
while let Some(ev) = backend.poll_event::<MyMsg>() {
    app.update(ev);
}
app.update(Event::Tick);

// 4. Build draw commands:
let (vp_w, vp_h)   = backend.size();
let (cell_w, cell_h) = backend.cell_size();
let sl     = ScreenLayout::new(vp_w, vp_h, cell_w, cell_h, 1);
let theme  = app.theme();
let mut canvas = PixelCanvas::new(vp_w, vp_h);
let mut frame  = Frame::new(&mut canvas, sl, &theme);
app.view(&mut frame);
let cmds = canvas.finish();

// 5. Flush while Smithay's DRM FBO is bound:
backend.renderer_mut().flush(&cmds, vp_w, vp_h);
```

### Viewport resize

```rust
backend.set_size(new_w, new_h);
// Event::Resize is queued and delivered on the next poll_event drain.
```

### Delivering compositor messages to the app

Use a side-channel `VecDeque` on your compositor state (drain it before
the `poll_event` loop, as shown above):

```rust
// In KittyCompositor state:
pub pending_chrome_msgs: VecDeque<ChromeMsg>,

// Enqueue from any compositor handler:
state.pending_chrome_msgs.push_back(ChromeMsg::FullSnapshot { ... });

// Drain at the top of each frame (before poll_event):
while let Some(msg) = state.pending_chrome_msgs.pop_front() {
    app.update(Event::Message(msg));
}
```

---

## Widget code contract

Widget code must emit only the primitive `DrawCmd` variants listed below.
`DrawCmd::Text` is for **actual text content only** — no embedded box-draw
or Powerline codepoints.

| Canvas method | `DrawCmd` | Use for |
|---|---|---|
| `canvas.fill()` | `FillRect` | solid backgrounds, filled regions |
| `canvas.stroke()` | `StrokeRect` | debug outlines |
| `canvas.hline()` | `HLine` | separators, underlines, accent lines |
| `canvas.vline()` | `VLine` | separators, selection bars |
| `canvas.border()` | `BorderLine` | widget borders — replaces box-draw chars |
| `canvas.round_rect/fill/stroke()` | `RoundRect` | rounded panels, badges, pills |
| `canvas.powerline()` | `PowerlineArrow` | status bar arrows — replaces U+E0B0–U+E0B3 |
| `canvas.text() / text_maxw()` | `Text` | labels, titles, content text only |

### Legacy TUI shim (do not rely on in widget code)

The following are decoded automatically inside `ChromeRenderer::flush` when
they appear inside a `DrawCmd::Text` string. They exist **only** to support
raw crossterm / ratatui output routed through the GL backend.

| Codepoints | Characters | Primitive equivalent |
|---|---|---|
| U+2500–U+257F | Box-drawing `─│╭╮╯╰┼` etc. | `canvas.border()` |
| U+2580–U+259F | Block elements `█▀▄▌▐` etc. | `canvas.fill()` |
| U+2591–U+2593 | Shade blocks `░▒▓` | `canvas.fill()` with alpha |
| U+E0B0–U+E0B3 | Powerline arrows | `canvas.powerline()` |
