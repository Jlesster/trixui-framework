//! app — App trait, event loop, Frame, Event, Cmd.
//!
//! The hybrid ratatui/bubbletea model:
//!   - `App::view()` renders into `&mut Frame` (ratatui style)
//!   - `App::update()` receives `Event<Msg>` and returns `Cmd<Msg>` (bubbletea style)
//!
//! # Rendering widgets
//!
//! The ergonomic path is `frame.render(widget, area)` which automatically
//! threads `cell_w`, `cell_h`, and `theme` through for you:
//!
//! ```rust,ignore
//! fn view(&self, frame: &mut Frame) {
//!     let inner = frame.render_block(
//!         Block::bordered().title(" My App "),
//!         frame.area(),
//!     );
//!     frame.render(Paragraph::new("hello world"), inner);
//!     frame.render_stateful(
//!         List::new(self.items.clone()),
//!         inner,
//!         &mut self.list_state,
//!     );
//! }
//! ```

use crate::backend::Backend;
use crate::layout::{Rect, ScreenLayout};
use crate::renderer::{PixelCanvas, Theme};
use crate::widget::{Block, StatefulWidget, Widget};

pub mod event;
pub use event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent};

// ── Cmd ───────────────────────────────────────────────────────────────────────

/// An effect returned from `App::update`.
pub enum Cmd<Msg> {
    None,
    Quit,
    Msg(Msg),
    Batch(Vec<Cmd<Msg>>),
}

impl<Msg> Cmd<Msg> {
    pub fn none() -> Self {
        Self::None
    }
    pub fn quit() -> Self {
        Self::Quit
    }
    pub fn msg(m: Msg) -> Self {
        Self::Msg(m)
    }
    pub fn batch(v: Vec<Cmd<Msg>>) -> Self {
        Self::Batch(v)
    }
}

// ── Frame ─────────────────────────────────────────────────────────────────────

/// Render target passed to `App::view`.
///
/// Use [`Frame::render`] / [`Frame::render_stateful`] for the cleanest API.
/// The raw [`Frame::canvas`] accessor is still available for custom drawing.
pub struct Frame<'a> {
    canvas: &'a mut PixelCanvas,
    layout: ScreenLayout,
    theme: &'a Theme,
}

impl<'a> Frame<'a> {
    pub fn new(canvas: &'a mut PixelCanvas, layout: ScreenLayout, theme: &'a Theme) -> Self {
        Self {
            canvas,
            layout,
            theme,
        }
    }

    // ── Area accessors ────────────────────────────────────────────────────────

    /// Full viewport rect.
    pub fn area(&self) -> Rect {
        self.layout.vp
    }

    /// Content area (everything above the status bar).
    pub fn content_area(&self) -> Rect {
        self.layout.content
    }

    /// Status bar rect.
    pub fn bar_area(&self) -> Rect {
        self.layout.bar
    }

    pub fn layout(&self) -> &ScreenLayout {
        &self.layout
    }
    pub fn theme(&self) -> &Theme {
        self.theme
    }
    pub fn cell_w(&self) -> u32 {
        self.layout.cell_w
    }
    pub fn cell_h(&self) -> u32 {
        self.layout.cell_h
    }

    /// Raw canvas — use when you need direct `DrawCmd` control.
    pub fn canvas(&mut self) -> &mut PixelCanvas {
        self.canvas
    }

    // ── Ergonomic render helpers ──────────────────────────────────────────────

    /// Render any [`Widget`] into `area`, threading cell metrics and theme
    /// automatically.
    ///
    /// ```rust,ignore
    /// frame.render(Paragraph::new("hello"), inner);
    /// frame.render(Tabs::new(vec!["A","B"]).select(0), tab_area);
    /// frame.render(Gauge::new().ratio(0.6), gauge_area);
    /// ```
    pub fn render(&mut self, widget: impl Widget, area: Rect) {
        widget.render(
            self.canvas,
            area,
            self.layout.cell_w,
            self.layout.cell_h,
            self.theme,
        );
    }

    /// Render a [`StatefulWidget`] into `area`.
    ///
    /// ```rust,ignore
    /// frame.render_stateful(
    ///     List::new(items).highlight_style(Style::default().bg(t.highlight_bg)),
    ///     list_area,
    ///     &mut self.list_state,
    /// );
    /// ```
    pub fn render_stateful<W: StatefulWidget>(
        &mut self,
        widget: W,
        area: Rect,
        state: &mut W::State,
    ) {
        widget.render(
            self.canvas,
            area,
            state,
            self.layout.cell_w,
            self.layout.cell_h,
            self.theme,
        );
    }

    /// Render a [`Block`], returning the inner content [`Rect`].
    ///
    /// This is the idiomatic way to render a bordered panel:
    ///
    /// ```rust,ignore
    /// let inner = frame.render_block(Block::bordered().title(" Pane "), area);
    /// frame.render(Paragraph::new("content"), inner);
    /// ```
    pub fn render_block(&mut self, block: Block<'_>, area: Rect) -> Rect {
        block.render(
            self.canvas,
            area,
            self.layout.cell_w,
            self.layout.cell_h,
            self.theme,
        )
    }
}

// ── App trait ─────────────────────────────────────────────────────────────────

/// Implement this to build a trixui application.
///
/// # Minimal example
///
/// ```rust,ignore
/// use trixui::prelude::*;
///
/// struct MyApp { count: i32 }
///
/// impl App for MyApp {
///     type Message = ();
///
///     fn update(&mut self, event: Event<()>) -> Cmd<()> {
///         if let Event::Key(k) = event {
///             match k.code {
///                 KeyCode::Char('q') | KeyCode::Esc => return Cmd::quit(),
///                 KeyCode::Up   => self.count += 1,
///                 KeyCode::Down => self.count -= 1,
///                 _ => {}
///             }
///         }
///         Cmd::none()
///     }
///
///     fn view(&self, frame: &mut Frame) {
///         let inner = frame.render_block(
///             Block::bordered().title(format!(" count: {} ", self.count).as_str()),
///             frame.area(),
///         );
///         frame.render(
///             Paragraph::new("↑/↓ to change, q to quit"),
///             inner,
///         );
///     }
/// }
///
/// fn main() -> trixui::Result<()> {
///     WinitBackend::new()?.run_app(MyApp { count: 0 })
/// }
/// ```
pub trait App: Sized + 'static {
    type Message: 'static;

    /// Process an event, return a command.
    fn update(&mut self, event: Event<Self::Message>) -> Cmd<Self::Message>;

    /// Render the current state into `frame`.
    fn view(&self, frame: &mut Frame);

    /// Called once before the event loop starts. Override for async init.
    fn init(&mut self) -> Cmd<Self::Message> {
        Cmd::none()
    }

    /// Override to supply a custom theme. Called each frame.
    fn theme(&self) -> Theme {
        Theme::default()
    }

    /// Target frame rate in Hz. Default 60.
    fn tick_rate(&self) -> u64 {
        60
    }
}

// ── Terminal ──────────────────────────────────────────────────────────────────

/// Drives the event loop for backends that are not `WinitBackend`.
///
/// For `WinitBackend` (standalone windows) use `WinitBackend::run_app(app)`
/// instead — winit owns its own event loop.
///
/// `Terminal` is the right choice for:
/// - [`WaylandBackend`](crate::backend::wayland::WaylandBackend) inside a Smithay compositor
/// - Custom test/headless backends
pub struct Terminal<B: Backend> {
    backend: B,
}

impl<B: Backend> Terminal<B> {
    pub fn new(backend: B) -> crate::Result<Self> {
        Ok(Self { backend })
    }

    /// Run the app. Blocks until `Cmd::Quit` is returned.
    ///
    /// **Do not call this with `WinitBackend`** — use `WinitBackend::run_app()`
    /// instead. The winit event loop cannot be driven from here.
    pub fn run<A: App>(mut self, mut app: A) -> crate::Result<()> {
        let init_cmd = app.init();
        if self.process_cmd(init_cmd, &mut app) {
            return Ok(());
        }

        let mut last = std::time::Instant::now();

        'main: loop {
            // Drain backend events
            while let Some(ev) = self.backend.poll_event::<A::Message>() {
                if self.process_cmd_from_event(ev, &mut app) {
                    break 'main;
                }
            }

            // Tick
            let tick_ns = 1_000_000_000u64 / app.tick_rate();
            let now = std::time::Instant::now();
            if now.duration_since(last).as_nanos() as u64 >= tick_ns {
                last = now;
                let cmd = app.update(Event::Tick);
                if self.process_cmd(cmd, &mut app) {
                    break 'main;
                }
            }

            // Render
            let (vp_w, vp_h) = self.backend.size();
            let (cell_w, cell_h) = self.backend.cell_size();
            let theme = app.theme();
            let sl = ScreenLayout::new(vp_w, vp_h, cell_w, cell_h, 0);
            let mut canvas = PixelCanvas::new(vp_w, vp_h);
            canvas.set_clip(Some(sl.vp));
            {
                let mut frame = Frame::new(&mut canvas, sl, &theme);
                app.view(&mut frame);
            }
            let cmds = canvas.finish();
            self.backend.render(&cmds, vp_w, vp_h);
        }

        Ok(())
    }

    fn process_cmd_from_event<A: App>(&mut self, ev: Event<A::Message>, app: &mut A) -> bool {
        let cmd = app.update(ev);
        self.process_cmd(cmd, app)
    }

    /// Process a `Cmd` tree iteratively (no stack growth for deep batches).
    fn process_cmd<A: App>(&mut self, root: Cmd<A::Message>, app: &mut A) -> bool {
        let mut stack = vec![root];
        while let Some(cmd) = stack.pop() {
            match cmd {
                Cmd::None => {}
                Cmd::Quit => return true,
                Cmd::Msg(m) => {
                    let next = app.update(Event::Message(m));
                    stack.push(next);
                }
                Cmd::Batch(v) => stack.extend(v),
            }
        }
        false
    }
}
