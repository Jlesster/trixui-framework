//! app — App trait, event loop, Frame, Event, Cmd.
//!
//! The hybrid ratatui/bubbletea model:
//!   - `App::view()` renders directly into `&mut Frame` (ratatui style)
//!   - `App::update()` receives `Event<Msg>` and returns `Cmd<Msg>` (bubbletea style)
//!   - `Terminal::run()` owns the event loop

use crate::backend::Backend;
use crate::layout::{Rect, ScreenLayout};
use crate::renderer::{PixelCanvas, Theme};

pub mod event;
pub use event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent};

// ── Cmd ───────────────────────────────────────────────────────────────────────

/// An effect returned from `App::update`.
///
/// Inspired by Elm/bubbletea: the app returns what it *wants* to happen,
/// the runtime decides how to fulfil it.
pub enum Cmd<Msg> {
    /// Do nothing.
    None,
    /// Quit the event loop cleanly.
    Quit,
    /// Schedule a message to be delivered on the next tick.
    Msg(Msg),
    /// Batch multiple commands.
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

/// Render target passed to `App::view`. Wraps `PixelCanvas` + layout info.
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

    /// Full viewport rect.
    pub fn area(&self) -> Rect {
        self.layout.vp
    }

    /// Content area (everything above the bar).
    pub fn content_area(&self) -> Rect {
        self.layout.content
    }

    /// Status bar rect.
    pub fn bar_area(&self) -> Rect {
        self.layout.bar
    }

    /// The underlying canvas — pass to widget `.render()` calls.
    pub fn canvas(&mut self) -> &mut PixelCanvas {
        self.canvas
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
}

// ── App trait ─────────────────────────────────────────────────────────────────

/// Implement this to build a trixui application.
///
/// ```rust,ignore
/// impl App for MyApp {
///     type Message = MyMsg;
///
///     fn update(&mut self, event: Event<MyMsg>) -> Cmd<MyMsg> {
///         match event {
///             Event::Key(k) if k.code == KeyCode::Char('q') => Cmd::quit(),
///             Event::Message(msg) => { /* handle app message */ Cmd::none() }
///             _ => Cmd::none(),
///         }
///     }
///
///     fn view(&self, frame: &mut Frame) {
///         let area = frame.content_area();
///         Block::bordered()
///             .title(" My App ")
///             .render(frame.canvas(), area, frame.cell_w(), frame.cell_h(), frame.theme());
///     }
/// }
/// ```
pub trait App: Sized + 'static {
    type Message: 'static;

    /// Process an event and return a command.
    fn update(&mut self, event: Event<Self::Message>) -> Cmd<Self::Message>;

    /// Render the current state into `frame`.
    fn view(&self, frame: &mut Frame);

    /// Called once before the event loop starts. Override for init logic.
    fn init(&mut self) -> Cmd<Self::Message> {
        Cmd::none()
    }

    /// Override to customise the theme.
    fn theme(&self) -> Theme {
        Theme::default()
    }

    /// Target frame rate in Hz. Default 60.
    fn tick_rate(&self) -> u64 {
        60
    }
}

// ── Terminal ──────────────────────────────────────────────────────────────────

/// Owns the backend and event loop. Created once, then drives `App::run`.
pub struct Terminal<B: Backend> {
    backend: B,
}

impl<B: Backend> Terminal<B> {
    pub fn new(backend: B) -> crate::Result<Self> {
        Ok(Self { backend })
    }

    /// Run the app. Blocks until `Cmd::Quit` is returned.
    pub fn run<A: App>(mut self, mut app: A) -> crate::Result<()> {
        // Init
        let init_cmd = app.init();
        let theme = app.theme();
        self.process_cmd(init_cmd, &mut app);

        let tick_ns = 1_000_000_000u64 / app.tick_rate();
        let mut last = std::time::Instant::now();

        'main: loop {
            // Drain backend events
            while let Some(ev) = self.backend.poll_event() {
                let cmd = app.update(ev);
                if self.process_cmd(cmd, &mut app) {
                    break 'main;
                }
            }

            // Tick
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

    /// Returns true if the app should quit.
    fn process_cmd<A: App>(&mut self, cmd: Cmd<A::Message>, app: &mut A) -> bool {
        match cmd {
            Cmd::None => false,
            Cmd::Quit => true,
            Cmd::Msg(m) => {
                let next = app.update(Event::Message(m));
                self.process_cmd(next, app)
            }
            Cmd::Batch(v) => v.into_iter().any(|c| self.process_cmd(c, app)),
        }
    }
}
