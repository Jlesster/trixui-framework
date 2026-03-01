# trixui API Documentation

`trixui` is a hybrid TUI/OpenGL framework — ratatui-style widgets rendered via OpenGL ES 3. It works as a standalone windowed app (winit + glutin) or as the chrome layer inside a Wayland compositor (Smithay). Same widget code, same API, two backends.

---

## Table of Contents

1. [Crate Re-exports](#crate-re-exports)
2. [Application Model](#application-model)
   - [App Trait](#app-trait)
   - [Cmd](#cmd)
   - [Event](#event)
   - [Frame](#frame)
3. [Widgets](#widgets)
   - [Widget Trait](#widget-trait)
   - [StatefulWidget Trait](#statefulwidget-trait)
   - [Block](#block)
   - [Paragraph](#paragraph)
   - [List](#list)
   - [Table](#table)
   - [Tabs](#tabs)
   - [Gauge](#gauge)
4. [Layout System](#layout-system)
   - [Rect](#rect)
   - [CellRect](#cellrect)
   - [ScreenLayout](#screenlayout)
   - [Constraint](#constraint)
   - [Flex](#flex)
   - [Layout](#layout)
5. [Renderer](#renderer)
   - [PixelCanvas](#pixelcanvas)
   - [DrawCmd](#drawcmd)
   - [Color](#color)
   - [TextStyle](#textstyle)
   - [BorderSide](#borderside)
   - [CornerRadius](#cornerradius)
   - [PowerlineDir](#powerlinedir)
6. [Theme](#theme)
7. [Backends](#backends)
   - [WinitBackend](#winitbackend)
   - [WaylandBackend](#waylandbackend)
   - [Backend Trait](#backend-trait)
8. [Input Events](#input-events)
   - [KeyCode](#keycode)
   - [KeyEvent](#keyevent)
   - [KeyModifiers](#keymodifiers)
   - [MouseEvent](#mouseevent)
   - [MouseButton](#mousebutton)
9. [Utility Types](#utility-types)
   - [Style](#style)
   - [Borders](#borders)
   - [Cell](#cell)
   - [Row](#row)
   - [ListItem](#listitem)
   - [ListState](#liststate)
   - [TableState](#tablestate)
   - [ColWidth](#colwidth)

---

## Crate Re-exports

The `trixui` crate provides a comprehensive prelude for typical usage:

```rust
use trixui::prelude::*;
```

The prelude re-exports:

- **App**: [`app::App`](#app-trait)
- **Cmd**: [`app::Cmd`](#cmd)
- **Event**: [`app::Event`](#event)
- **Frame**: [`app::Frame`](#frame)
- **KeyCode**: [`app::KeyCode`](#keycode)
- **KeyEvent**: [`app::KeyEvent`](#keyevent)
- **KeyModifiers**: [`app::KeyModifiers`](#keymodifiers)
- **Terminal**: [`app::Terminal`](#terminal)
- **PixRect**: [`layout::Rect`](#rect)
- **ScreenLayout**: [`layout::ScreenLayout`](#screenlayout)
- **PixColor**: [`renderer::Color`](#color)
- **PixelCanvas**: [`renderer::PixelCanvas`](#pixelcanvas)
- **Theme**: [`renderer::Theme`](#theme)
- **Widget types**: [`widget::*`](#widgets)

Additionally, platform-specific backends are conditionally exported:
- `WinitBackend` (when `backend-winit` feature is enabled)
- `WaylandBackend` (when `backend-wayland` feature is enabled)
- `SmithayApp` (when `backend-smithay` feature is enabled)

---

## Application Model

### App Trait

The central trait that users implement to create trixui applications.

**Signature:**
```rust
pub trait App: Sized + 'static {
    type Message: 'static;

    fn update(&mut self, event: Event<Self::Message>) -> Cmd<Self::Message>;
    fn view(&self, frame: &mut Frame);

    fn init(&mut self) -> Cmd<Self::Message> { Cmd::none() }
    fn theme(&self) -> Theme { Theme::default() }
    fn tick_rate(&self) -> u64 { 60 }
}
```

**Methods:**

- `type Message: 'static` — User-defined message type for intra-app communication.

- `fn update(&mut self, event: Event<Self::Message>) -> Cmd<Self::Message>` — Process an incoming event and return a [`Cmd`](#cmd) representing the desired action.

- `fn view(&self, frame: &mut Frame)` — Render the current application state into the provided frame. This is called every frame after event processing.

- `fn init(&mut self) -> Cmd<Self::Message>` — Called once before the event loop starts. Override for async initialization. Default: returns `Cmd::none()`.

- `fn theme(&self) -> Theme` — Override to supply a custom theme. Called each frame. Default: [`Theme::default()`](#theme) (Catppuccin Mocha).

- `fn tick_rate(&self) -> u64` — Target frame rate in Hz. Default: 60.

**Minimal Example:**
```rust
use trixui::prelude::*;

struct MyApp { count: i32 }

impl App for MyApp {
    type Message = ();

    fn update(&mut self, event: Event<()>) -> Cmd<()> {
        if let Event::Key(k) = event {
            match k.code {
                KeyCode::Char('q') | KeyCode::Esc => return Cmd::quit(),
                KeyCode::Up   => self.count += 1,
                KeyCode::Down => self.count -= 1,
                _ => {}
            }
        }
        Cmd::none()
    }

    fn view(&self, frame: &mut Frame) {
        let inner = frame.render_block(
            Block::bordered().title(format!(" count: {} ", self.count).as_str()),
            frame.area(),
        );
        frame.render(
            Paragraph::new("↑/↓ to change, q to quit"),
            inner,
        );
    }
}

fn main() -> trixui::Result<()> {
    WinitBackend::new()?.run_app(MyApp { count: 0 })
}
```

---

### Cmd

Represents an effect returned from `App::update`. The hybrid ratatui/bubbletea model combines rendering (ratatui style) with command-based state updates (bubbletea style).

**Signature:**
```rust
pub enum Cmd<Msg> {
    None,
    Quit,
    Msg(Msg),
    Batch(Vec<Cmd<Msg>>),
}
```

**Variants:**

- `None` — No operation, continue normally.

- `Quit` — Exit the event loop.

- `Msg(Msg)` — Schedule a user-defined message to be delivered to the app in the next update cycle. This is the primary mechanism for intra-app communication.

- `Batch(Vec<Cmd<Msg>>)` — Execute multiple commands. Used to combine multiple effects (e.g., update state and schedule a message).

**Associated Functions:**

```rust
impl<Msg> Cmd<Msg> {
    pub fn none() -> Self
    pub fn quit() -> Self
    pub fn msg(m: Msg) -> Self
    pub fn batch(v: Vec<Cmd<Msg>>) -> Self
}
```

**Usage:**
```rust
// Exit the application
return Cmd::quit();

// Schedule a message
return Cmd::msg(MyMessage::Increment);

// Combine multiple commands
return Cmd::batch(vec![
    Cmd::msg(MyMessage::Increment),
    Cmd::msg(MyMessage::LogAction),
]);
```

---

### Event

All events the app can receive in `App::update()`.

**Signature:**
```rust
pub enum Event<Msg> {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Resize(u32, u32),
    Tick,
    Message(Msg),
}
```

**Variants:**

- `Key(KeyEvent)` — A key was pressed.

- `Mouse(MouseEvent)` — A mouse action occurred.

- `Resize(u32, u32)` — The viewport was resized. Contains new width and height in pixels.

- `Tick` — Regular tick driven by `App::tick_rate()`. Use for animations or periodic updates.

- `Message(Msg)` — A user-defined message delivered via `Cmd::msg()`.

---

### Frame

The render target passed to `App::view`. Provides access to the drawing canvas, layout information, and theme.

**Signature:**
```rust
pub struct Frame<'a> {
    canvas: &'a mut PixelCanvas,
    layout: ScreenLayout,
    theme: &'a Theme,
}
```

**Methods:**

```rust
impl<'a> Frame<'a> {
    // Area accessors
    pub fn area(&self) -> Rect           // Full viewport rect
    pub fn content_area(&self) -> Rect   // Content area (above status bar)
    pub fn bar_area(&self) -> Rect       // Status bar rect
    pub fn layout(&self) -> &ScreenLayout
    pub fn theme(&self) -> &Theme
    pub fn cell_w(&self) -> u32          // Cell width in pixels
    pub fn cell_h(&self) -> u32          // Cell height in pixels

    // Raw canvas accessor
    pub fn canvas(&mut self) -> &mut PixelCanvas

    // Ergonomic render helpers
    pub fn render(&mut self, widget: impl Widget, area: Rect)
    pub fn render_stateful<W: StatefulWidget>(&mut self, widget: W, area: Rect, state: &mut W::State) -> Rect
    pub fn render_block(&mut self, block: Block<'_>, area: Rect) -> Rect
}
```

**Usage:**
```rust
fn view(&self, frame: &mut Frame) {
    // Get full viewport area
    let area = frame.area();

    // Render a block and get inner content area
    let inner = frame.render_block(
        Block::bordered().title(" My App "),
        area,
    );

    // Render widgets into the inner area
    frame.render(Paragraph::new("Hello, world!"), inner);

    // For stateful widgets
    frame.render_stateful(
        List::new(items.clone()),
        inner,
        &mut self.list_state,
    );
}
```

---

### Terminal

Drives the event loop for backends that are not `WinitBackend`.

**Signature:**
```rust
pub struct Terminal<B: Backend> {
    backend: B,
}
```

**Methods:**
```rust
impl<B: Backend> Terminal<B> {
    pub fn new(backend: B) -> crate::Result<Self>
    pub fn run<A: App>(mut self, mut app: A) -> crate::Result<()>
}
```

**Note:** For `WinitBackend` (standalone windows), use `WinitBackend::run_app(app)` instead — winit owns its own event loop. `Terminal` is the right choice for:
- [`WaylandBackend`](#waylandbackend) inside a Smithay compositor
- Custom test/headless backends

---

## Widgets

### Widget Trait

The fundamental trait for rendering widgets.

**Signature:**
```rust
pub trait Widget {
    fn render(self, canvas: &mut PixelCanvas, area: Rect, cell_w: u32, cell_h: u32, t: &Theme);
}
```

---

### StatefulWidget Trait

For widgets that maintain state (e.g., List with selection, Table with cursor).

**Signature:**
```rust
pub trait StatefulWidget {
    type State;
    fn render(
        self,
        canvas: &mut PixelCanvas,
        area: Rect,
        state: &mut Self::State,
        cell_w: u32,
        cell_h: u32,
        t: &Theme,
    );
}
```

---

### Block

A bordered container widget with optional title and top accent.

**Signature:**
```rust
pub struct Block<'a> {
    borders: Borders,
    border_style: Style,
    style: Style,
    title: Option<&'a str>,
    title_style: Style,
    border_px: u32,
    top_accent: Option<Color>,
    corner_radius: f32,
}
```

**Methods:**
```rust
impl<'a> Block<'a> {
    pub fn new() -> Self
    pub fn bordered() -> Self              // Convenience: borders(ALL)
    pub fn borders(mut self, b: Borders) -> Self
    pub fn border_style(mut self, s: Style) -> Self
    pub fn style(mut self, s: Style) -> Self
    pub fn title(mut self, t: &'a str) -> Self
    pub fn title_style(mut self, s: Style) -> Self
    pub fn border_px(mut self, px: u32) -> Self
    pub fn top_accent(mut self, c: Color) -> Self
    pub fn rounded(mut self, r: f32) -> Self

    // Returns inner content Rect after rendering borders
    pub fn render(self, canvas: &mut PixelCanvas, area: Rect, cell_w: u32, cell_h: u32, t: &Theme) -> Rect
}
```

**Usage:**
```rust
// Simple bordered block
let inner = frame.render_block(
    Block::bordered().title(" Panel "),
    area,
);

// With custom styling
let inner = frame.render_block(
    Block::bordered()
        .title(" Important ")
        .border_style(Style::default().fg(Theme::default().active_border))
        .top_accent(Theme::default().bar_accent)
        .rounded(8.0),
    area,
);
```

---

### Paragraph

A text paragraph widget with optional word wrapping and scrolling.

**Signature:**
```rust
pub struct Paragraph<'a> {
    text: &'a str,
    style: Style,
    wrap: bool,
    scroll: u32,
}
```

**Methods:**
```rust
impl<'a> Paragraph<'a> {
    pub fn new(text: &'a str) -> Self
    pub fn style(mut self, s: Style) -> Self
    pub fn wrap(mut self, w: bool) -> Self          // Enable word wrapping
    pub fn scroll(mut self, n: u32) -> Self          // Scroll offset in lines
}
```

**Usage:**
```rust
// Basic text
frame.render(Paragraph::new("Hello, world!"), area);

// With wrapping
frame.render(
    Paragraph::new("Long text that should wrap...")
        .wrap(true),
    area,
);

// With custom style
frame.render(
    Paragraph::new("Styled text")
        .style(Style::default().fg(Color::hex(0xff0000))),
    area,
);
```

---

### List

A scrollable list widget with optional selection highlight.

**Signature:**
```rust
pub struct List<'a> {
    items: Vec<ListItem<'a>>,
    highlight_style: Style,
    highlight_symbol: &'a str,
    selected_bar: bool,
    selected_bar_color: Option<Color>,
    selected_bar_px: u32,
    row_separator: bool,
    row_separator_color: Option<Color>,
}
```

**Methods:**
```rust
impl<'a> List<'a> {
    pub fn new(items: Vec<ListItem<'a>>) -> Self
    pub fn highlight_style(mut self, s: Style) -> Self
    pub fn highlight_symbol(mut self, s: &'a str) -> Self   // Symbol for selected item
    pub fn selected_bar(mut self, c: Color) -> Self         // Vertical bar on left
    pub fn selected_bar_px(mut self, px: u32) -> Self
    pub fn row_separator(mut self, c: Color) -> Self        // Horizontal line between rows
}
```

**Usage:**
```rust
let items = vec![
    ListItem::new("Item 1"),
    ListItem::new("Item 2"),
    ListItem::new("Item 3"),
];

let inner = frame.render_block(Block::bordered().title(" List "), area);
frame.render_stateful(
    List::new(items)
        .highlight_symbol("▶")
        .highlight_style(Style::default().bg(Color::hex(0x333333))),
    inner,
    &mut self.list_state,
);
```

---

### Table

A table widget with header, columns, and row selection.

**Signature:**
```rust
pub struct Table<'a> {
    header: Option<Row<'a>>,
    rows: Vec<Row<'a>>,
    col_widths: Vec<ColWidth>,
    highlight_style: Style,
    col_spacing: u32,
    header_style: Style,
    header_separator: bool,
    header_separator_color: Option<Color>,
    row_separator: bool,
    row_separator_color: Option<Color>,
}
```

**Methods:**
```rust
impl<'a> Table<'a> {
    pub fn new(rows: Vec<Row<'a>>, col_widths: Vec<ColWidth>) -> Self
    pub fn header(mut self, r: Row<'a>) -> Self
    pub fn highlight_style(mut self, s: Style) -> Self
    pub fn header_style(mut self, s: Style) -> Self
    pub fn col_spacing(mut self, px: u32) -> Self
    pub fn header_separator(mut self, c: Color) -> Self
    pub fn row_separator(mut self, c: Color) -> Self
    pub fn no_header_separator(mut self) -> Self
}
```

**Usage:**
```rust
let header = Row::new(vec![
    Cell::new("Name").style(Style::default().bold()),
    Cell::new("Age"),
]);

let rows = vec![
    Row::new(vec![Cell::new("Alice"), Cell::new("30")]),
    Row::new(vec![Cell::new("Bob"), Cell::new("25")]),
];

let col_widths = vec![ColWidth::Cells(10), ColWidth::Fill(1)];

let inner = frame.render_block(Block::bordered().title(" Table "), area);
frame.render_stateful(
    Table::new(rows, col_widths)
        .header(header)
        .header_separator(Theme::default().inactive_border),
    inner,
    &mut self.table_state,
);
```

---

### Tabs

A tab bar widget for switching between views.

**Signature:**
```rust
pub struct Tabs<'a> {
    titles: Vec<&'a str>,
    selected: usize,
    style: Style,
    highlight_style: Style,
    tab_padding: u32,
    powerline: bool,
    powerline_color: Option<Color>,
    underline: bool,
    underline_color: Option<Color>,
    divider: bool,
    divider_color: Option<Color>,
}
```

**Methods:**
```rust
impl<'a> Tabs<'a> {
    pub fn new(titles: Vec<&'a str>) -> Self
    pub fn select(mut self, i: usize) -> Self
    pub fn style(mut self, s: Style) -> Self
    pub fn highlight_style(mut self, s: Style) -> Self
    pub fn tab_padding(mut self, cells: u32) -> Self
    pub fn powerline(mut self, c: Color) -> Self          // Arrow indicator
    pub fn underline(mut self, c: Color) -> Self         // Bottom underline
    pub fn divider(mut self, c: Color) -> Self          // Vertical dividers
}
```

**Usage:**
```rust
frame.render(
    Tabs::new(vec!["Files", "Edit", "View"])
        .select(self.active_tab)
        .highlight_style(Style::default().bold()),
    area,
);
```

---

### Gauge

A progress bar/gauge widget.

**Signature:**
```rust
pub struct Gauge<'a> {
    ratio: f64,
    style: Style,
    filled_style: Style,
    label: Option<&'a str>,
    label_style: Style,
}
```

**Methods:**
```rust
impl<'a> Gauge<'a> {
    pub fn new() -> Self
    pub fn ratio(mut self, r: f64) -> Self             // 0.0 to 1.0
    pub fn percent(mut self, p: u8) -> Self            // 0 to 100
    pub fn style(mut self, s: Style) -> Self           // Empty portion style
    pub fn filled_style(mut self, s: Style) -> Self    // Filled portion style
    pub fn label(mut self, l: &'a str) -> Self          // Label text
    pub fn label_style(mut self, s: Style) -> Self
}
```

**Usage:**
```rust
frame.render(
    Gauge::new()
        .ratio(0.6)
        .label("60%")
        .filled_style(Style::default().bg(Color::hex(0x00ff00))),
    area,
);
```

---

## Layout System

### Rect

A pixel-space rectangle. Top-left origin, X right, Y down.

**Signature:**
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}
```

**Methods:**
```rust
impl Rect {
    pub fn new(x: u32, y: u32, w: u32, h: u32) -> Self
    pub fn is_empty(&self) -> bool
    pub fn split_top(self, top_h: u32) -> (Self, Self)
    pub fn split_cols(self, n: usize) -> Vec<Self>
    pub fn split_ratios(self, ratios: &[f32]) -> Vec<Self>
    pub fn inset(self, px: u32) -> Self
}
```

---

### CellRect

A cell-grid rectangle. Used ONLY for animation interpolation. Convert to `Rect` via `ScreenLayout::cell_rect_to_px`.

**Signature:**
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CellRect {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}
```

**Methods:**
```rust
impl CellRect {
    pub fn new(x: u16, y: u16, w: u16, h: u16) -> Self
    pub fn is_empty(self) -> bool
    pub fn to_px(self, cell_w: u32, cell_h: u32) -> Rect
}
```

---

### ScreenLayout

Single layout pass. Produced once per frame. Invariant: `content.h + bar.h == vp.h` always.

**Signature:**
```rust
#[derive(Debug, Clone, Copy)]
pub struct ScreenLayout {
    pub vp: Rect,       // Full viewport
    pub content: Rect,  // Content area (above status bar)
    pub bar: Rect,      // Status bar
    pub cell_w: u32,    // Cell width in pixels
    pub cell_h: u32,    // Cell height in pixels
}
```

**Methods:**
```rust
impl ScreenLayout {
    pub fn new(vp_w: u32, vp_h: u32, cell_w: u32, cell_h: u32, bar_h_cells: u32) -> Self
    pub fn content_cols(&self) -> u16
    pub fn content_rows(&self) -> u16
    pub fn content_cell_rect(&self) -> CellRect
    pub fn cell_rect_to_px(&self, cr: CellRect) -> Rect
}
```

---

### Constraint

Column or row size constraint for the layout system.

**Signature:**
```rust
#[derive(Debug, Clone, Copy)]
pub enum Constraint {
    Fixed(u32),      // Exact pixel size
    Length(u32),     // Exactly n cells
    Percentage(u8),   // Percentage 0-100
    Ratio(u32, u32), // Fraction a/b
    Min(u32),        // At least n cells
    Max(u32),        // At most n cells
    Fill(u32),       // Fill remaining space (weighted)
}
```

**Notes:**
- `Fixed` is in **pixels**
- `Length`, `Min`, `Max` are in **cells**
- `Percentage`, `Ratio`, `Fill` are relative

---

### Flex

How children are distributed within leftover space.

**Signature:**
```rust
#[derive(Debug, Clone, Copy, Default)]
pub enum Flex {
    #[default] Start,        // Pack to start
    Center,                  // Center the group
    End,                    // Pack to end
    SpaceBetween,            // Distribute evenly between items
    SpaceAround,            // Distribute evenly around items
    Stretch,                // Alias for Start
}
```

---

### Layout

Constraint-based layout engine (ratatui Flex style).

**Signature:**
```rust
pub struct Layout {
    direction: Direction,
    constraints: Vec<Constraint>,
    flex: Flex,
    spacing: u32,
}
```

**Methods:**
```rust
impl Layout {
    pub fn horizontal(c: impl Into<Vec<Constraint>>) -> Self
    pub fn vertical(c: impl Into<Vec<Constraint>>) -> Self
    pub fn flex(mut self, f: Flex) -> Self
    pub fn spacing(mut self, px: u32) -> Self

    // Split the given area according to constraints
    pub fn split(self, area: Rect, cell_w: u32, cell_h: u32) -> Vec<Rect>
}
```

**Usage:**
```rust
let [left, right] = Layout::horizontal(vec![
    Constraint::Percentage(40),
    Constraint::Fill(1),
])
.spacing(cell_w)
.split(area, cell_w, cell_h)[..] else { return };
```

---

## Renderer

### PixelCanvas

Immediate-mode draw list. All widget rendering goes through here.

**Signature:**
```rust
pub struct PixelCanvas {
    cmds: Vec<DrawCmd>,
    clip: Option<Rect>,
    vp_w: u32,
    vp_h: u32,
}
```

**Methods:**
```rust
impl PixelCanvas {
    pub fn new(vp_w: u32, vp_h: u32) -> Self
    pub fn set_clip(&mut self, r: Option<Rect>)
    pub fn finish(self) -> Vec<DrawCmd>
    pub fn child(&mut self, clip: Rect) -> ChildCanvas<'_>

    // Primitives
    pub fn fill(&mut self, x: u32, y: u32, w: u32, h: u32, color: Color)
    pub fn stroke(&mut self, x: u32, y: u32, w: u32, h: u32, color: Color)
    pub fn hline(&mut self, x: u32, y: u32, w: u32, color: Color)
    pub fn vline(&mut self, x: u32, y: u32, h: u32, color: Color)
    pub fn border(&mut self, x: u32, y: u32, w: u32, h: u32, sides: BorderSide, color: Color, thickness: u32)
    pub fn round_rect(&mut self, x: f32, y: f32, w: f32, h: f32, radii: CornerRadius, fill: Color, stroke: Color, stroke_w: f32)
    pub fn round_fill(&mut self, x: f32, y: f32, w: f32, h: f32, radii: CornerRadius, fill: Color)
    pub fn round_stroke(&mut self, x: f32, y: f32, w: f32, h: f32, radii: CornerRadius, stroke: Color, stroke_w: f32)
    pub fn powerline(&mut self, x: u32, y: u32, w: u32, h: u32, dir: PowerlineDir, color: Color)
    pub fn text(&mut self, x: u32, y: u32, s: &str, style: TextStyle)
    pub fn text_maxw(&mut self, x: u32, y: u32, s: &str, style: TextStyle, max_w: u32)
}
```

---

### DrawCmd

A single GPU draw call. All coordinates are pixel-space, top-left origin.

**Signature:**
```rust
pub enum DrawCmd {
    FillRect { x: u32, y: u32, w: u32, h: u32, color: Color },
    StrokeRect { x: u32, y: u32, w: u32, h: u32, color: Color },
    HLine { x: u32, y: u32, w: u32, color: Color },
    VLine { x: u32, y: u32, h: u32, color: Color },
    BorderLine { x: u32, y: u32, w: u32, h: u32, sides: BorderSide, color: Color, thickness: u32 },
    RoundRect { x: f32, y: f32, w: f32, h: f32, radii: CornerRadius, fill: Color, stroke: Color, stroke_w: f32 },
    PowerlineArrow { x: u32, y: u32, w: u32, h: u32, dir: PowerlineDir, color: Color },
    Text { x: u32, y: u32, text: String, style: TextStyle, max_w: Option<u32> },
}
```

---

### Color

RGBA8 colour.

**Signature:**
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Color(pub u8, pub u8, pub u8, pub u8);
```

**Associated Constants and Methods:**
```rust
impl Color {
    pub const TRANSPARENT: Self
    pub fn rgb(r: u8, g: u8, b: u8) -> Self
    pub fn hex(v: u32) -> Self              // 0xRRGGBB, always opaque
    pub fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self
    pub fn is_transparent(self) -> bool
}
```

**Usage:**
```rust
let red = Color::rgb(255, 0, 0);
let blue = Color::hex(0x0000ff);
let translucent = Color::rgba(255, 0, 0, 128);
```

---

### TextStyle

Text rendering style for `DrawCmd::Text`.

**Signature:**
```rust
#[derive(Debug, Clone, Copy)]
pub struct TextStyle {
    pub fg: Color,
    pub bg: Color,
    pub bold: bool,
    pub italic: bool,
}
```

**Methods:**
```rust
impl TextStyle {
    pub fn fg(color: Color) -> Self
}
```

---

### BorderSide

Which sides to draw for `DrawCmd::BorderLine`.

**Signature:**
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BorderSide(pub u8);

impl BorderSide {
    pub const NONE: Self
    pub const TOP: Self
    pub const BOTTOM: Self
    pub const LEFT: Self
    pub const RIGHT: Self
    pub const ALL: Self

    pub fn contains(self, other: Self) -> bool
    pub fn or(self, other: Self) -> Self
}
```

---

### CornerRadius

Per-corner pixel radii for `DrawCmd::RoundRect`.

**Signature:**
```rust
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CornerRadius {
    pub tl: f32,
    pub tr: f32,
    pub bl: f32,
    pub br: f32,
}
```

**Methods:**
```rust
impl CornerRadius {
    pub fn all(r: f32) -> Self
    pub fn none() -> Self
    pub fn top_left(mut self, r: f32) -> Self
    pub fn top_right(mut self, r: f32) -> Self
    pub fn bottom_left(mut self, r: f32) -> Self
    pub fn bottom_right(mut self, r: f32) -> Self
    pub fn is_none(self) -> bool
}
```

---

### PowerlineDir

Arrow style for `DrawCmd::PowerlineArrow`.

**Signature:**
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PowerlineDir {
    RightFill = 0,
    LeftFill = 1,
    RightChevron = 2,
    LeftChevron = 3,
}
```

---

## Theme

Colour theme. Two categories of slots: content (`normal_*`, `highlight_*`, `dim_fg`) used by content widgets, and chrome (`active_border`, `bar_*`, `ws_*`) used by chrome widgets.

**Signature:**
```rust
#[derive(Clone, Copy, Debug)]
pub struct Theme {
    // Content slots
    pub normal_fg: Color,
    pub normal_bg: Color,
    pub highlight_fg: Color,
    pub highlight_bg: Color,
    pub dim_fg: Color,

    // Chrome / border slots
    pub active_border: Color,
    pub inactive_border: Color,
    pub active_title: Color,
    pub inactive_title: Color,
    pub pane_bg: Color,

    // Status bar slots
    pub bar_bg: Color,
    pub bar_fg: Color,
    pub bar_accent: Color,
    pub bar_dim: Color,

    // Workspace / tab pill slots
    pub ws_active_fg: Color,
    pub ws_active_bg: Color,
}
```

**Preset Themes:**

```rust
impl Theme {
    fn default() -> Self      // Catppuccin Mocha (default)
    pub fn latte() -> Self    // Catppuccin Latte (light)
    pub fn macchiato() -> Self // Catppuccin Macchiato
}
```

---

## Backends

### WinitBackend

Standalone windowed backend using winit 0.30 + glutin. Creates a window with OpenGL ES 3 context.

**Note:** Do NOT use `Terminal::run()` with this backend — winit owns the event loop. Use `WinitBackend::run_app()` instead.

**Signature:**
```rust
pub struct WinitBackend {
    font_data: Vec<u8>,
    size_px: f32,
    pending: std::collections::VecDeque<RawInput>,
}
```

**Methods:**
```rust
impl WinitBackend {
    pub fn new() -> crate::Result<Self>
    pub fn with_font(font_data: &[u8], size_px: f32) -> crate::Result<Self>
    pub fn run_app<A: App>(self, app: A) -> crate::Result<()>
}
```

**Usage:**
```rust
fn main() -> trixui::Result<()> {
    WinitBackend::new()?.run_app(MyApp::new())
}

// Or with custom font:
fn main() -> trixui::Result<()> {
    let font_data = std::fs::read("my-font.ttf")?;
    WinitBackend::with_font(&font_data, 16.0)?.run_app(MyApp::new())
}
```

---

### WaylandBackend

Smithay compositor backend. Designed to be used from inside a Smithay compositor.

**Signature:**
```rust
pub struct WaylandBackend {
    renderer: ChromeRenderer,
    vp_w: u32,
    vp_h: u32,
    pending: std::collections::VecDeque<RawInput>,
}
```

**Methods:**
```rust
impl WaylandBackend {
    pub fn new(renderer: ChromeRenderer, vp_w: u32, vp_h: u32) -> Self
    pub fn push_key(&mut self, ev: KeyEvent)
    pub fn set_size(&mut self, w: u32, h: u32)
    pub fn renderer_mut(&mut self) -> &mut ChromeRenderer
}
```

**Usage:**
```rust
// Inside your Smithay compositor:
let backend = WaylandBackend::new(renderer, vp_w, vp_h);

// Each frame, deliver input events:
backend.push_key(KeyEvent::plain(KeyCode::Char('j')));

// Then call Terminal::render_frame() to get DrawCmds back
let cmds = terminal.render_frame();

// Pass cmds to your existing ChromeRenderer / TwmChromeElement
```

---

### Backend Trait

A platform backend: owns the window/surface, GL context, and input source.

**Signature:**
```rust
pub trait Backend: Sized {
    fn size(&self) -> (u32, u32);
    fn cell_size(&self) -> (u32, u32);
    fn poll_event<Msg: 'static>(&mut self) -> Option<Event<Msg>>;
    fn render(&mut self, cmds: &[DrawCmd], vp_w: u32, vp_h: u32);
}
```

---

## Input Events

### KeyCode

Represents a key on the keyboard.

**Signature:**
```rust
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum KeyCode {
    Char(char),
    Enter,
    Backspace,
    Delete,
    Esc,
    Tab,
    BackTab,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    F(u8),
    Null,
}
```

---

### KeyEvent

A key event with code and modifiers.

**Signature:**
```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyEvent {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
}

impl KeyEvent {
    pub fn new(code: KeyCode, modifiers: KeyModifiers) -> Self
    pub fn plain(code: KeyCode) -> Self              // No modifiers
}
```

---

### KeyModifiers

Keyboard modifier flags.

**Signature:**
```rust
bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct KeyModifiers: u8 {
        const NONE  = 0b0000;
        const SHIFT = 0b0001;
        const CTRL  = 0b0010;
        const ALT   = 0b0100;
        const SUPER = 0b1000;
    }
}
```

---

### MouseEvent

A mouse event.

**Signature:**
```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MouseEvent {
    pub kind: MouseEventKind,
    pub x: u32,
    pub y: u32,
    pub button: MouseButton,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MouseEventKind {
    Down,
    Up,
    Drag,
    Moved,
    ScrollUp,
    ScrollDown,
}
```

---

### MouseButton

Mouse button.

**Signature:**
```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    None,
}
```

---

## Utility Types

### Style

A text style for widgets.

**Signature:**
```rust
#[derive(Debug, Clone, Copy, Default)]
pub struct Style {
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub bold: bool,
    pub italic: bool,
}
```

**Methods:**
```rust
impl Style {
    pub fn fg(mut self, c: Color) -> Self
    pub fn bg(mut self, c: Color) -> Self
    pub fn bold(mut self) -> Self
    pub fn italic(mut self) -> Self
    pub fn patch(self, other: Style) -> Style
    pub fn to_text_style(self, t: &Theme) -> TextStyle   // Uses content theme slots
    pub fn to_bar_style(self, t: &Theme) -> TextStyle   // Uses bar theme slots
}
```

---

### Borders

Border flags for Block widget.

**Signature:**
```rust
bitflags::bitflags! {
    #[derive(Clone, Copy, Debug, Default)]
    pub struct Borders: u8 {
        const NONE   = 0b0000;
        const TOP    = 0b0001;
        const BOTTOM = 0b0010;
        const LEFT   = 0b0100;
        const RIGHT  = 0b1000;
        const ALL    = /* TOP | BOTTOM | LEFT | RIGHT */;
    }
}
```

---

### Cell

A table cell.

**Signature:**
```rust
pub struct Cell<'a> {
    pub content: &'a str,
    pub style: Style,
}

impl<'a> Cell<'a> {
    pub fn new(content: &'a str) -> Self
    pub fn style(mut self, s: Style) -> Self
}
```

---

### Row

A table row.

**Signature:**
```rust
pub struct Row<'a> {
    pub cells: Vec<Cell<'a>>,
    pub style: Style,
    pub bottom_margin: u32,
}

impl<'a> Row<'a> {
    pub fn new(cells: Vec<Cell<'a>>) -> Self
    pub fn style(mut self, s: Style) -> Self
    pub fn bottom_margin(mut self, px: u32) -> Self
}
```

---

### ListItem

A list item.

**Signature:**
```rust
pub struct ListItem<'a> {
    pub content: &'a str,
    pub style: Style,
}

impl<'a> ListItem<'a> {
    pub fn new(content: &'a str) -> Self
    pub fn style(mut self, s: Style) -> Self
}
```

---

### ListState

State for the List widget.

**Signature:**
```rust
#[derive(Default)]
pub struct ListState {
    selected: Option<usize>,
    pub offset: usize,
}

impl ListState {
    pub fn select(&mut self, i: Option<usize>)
    pub fn selected(&self) -> Option<usize>
}
```

---

### TableState

State for the Table widget.

**Signature:**
```rust
#[derive(Clone, Copy, Debug)]
pub enum ColWidth {
    Fixed(u32),      // Exact pixels
    Cells(u32),      // Number of monospace cells
    Pct(u8),         // Percentage 0-100
    Fill(u32),       // Share of remaining space
}

#[derive(Default)]
pub struct TableState {
    selected: Option<usize>,
    pub offset: usize,
}

impl TableState {
    pub fn select(&mut self, i: Option<usize>)
    pub fn selected(&self) -> Option<usize>
}
```

---

## Quick Start Examples

### Standalone Window

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
        let t = frame.theme().clone();
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

### Smithay Compositor Integration

```rust
use trixui::prelude::*;
use trixui::smithay::SmithayApp;

// 1. Implement App exactly as above.
// 2. Create SmithayApp once after your GL context is current:
let mut ui = SmithayApp::new(font_bytes, 20.0, vp_w, vp_h, MyApp::new())?;

// 3. Each frame (inside your DRM render callback):
ui.push_key(KeyEvent::plain(KeyCode::Char('j')));
ui.render_frame(); // flushes DrawCmds via ChromeRenderer::flush

// 4. On output resize:
ui.resize(new_w, new_h);
```

---

## Feature Flags

- `backend-winit` (default): Enable the winit-based standalone window backend
- `backend-wayland`: Enable the Wayland/Smithay compositor backend
- `backend-smithay`: Enable Smithay integration

```toml
[dependencies]
trixui = { version = "0.1", default-features = false, features = ["backend-winit"] }
trixui = { version = "0.1", features = ["backend-wayland"] }
```

---

## Version

Current: 0.1.0
