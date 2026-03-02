//! app — App trait, event loop, Frame, Event, Cmd.

use std::sync::Arc;

use crate::layout::{Rect, ScreenLayout};
use crate::renderer::{PixelCanvas, Theme};
use crate::widget::{
    chrome::{draw_pane, BarBuilder, PaneOpts},
    Block, StatefulWidget, Widget,
};

pub mod event;
pub use event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent};

// ── Cmd ───────────────────────────────────────────────────────────────────────

pub enum Cmd<Msg: 'static> {
    None,
    Quit,
    Msg(Msg),
    Batch(Vec<Cmd<Msg>>),
    Spawn(Box<dyn FnOnce() -> Msg + Send + 'static>),
}

impl<Msg: 'static> Cmd<Msg> {
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
    pub fn spawn<F: FnOnce() -> Msg + Send + 'static>(f: F) -> Self {
        Self::Spawn(Box::new(f))
    }
}

pub(crate) type SpawnQueue<Msg> = Arc<std::sync::Mutex<std::collections::VecDeque<Msg>>>;

// ── Frame ─────────────────────────────────────────────────────────────────────

/// Render target passed to `App::view`.
pub struct Frame<'a> {
    canvas: &'a mut PixelCanvas,
    layout: ScreenLayout,
    theme: &'a Theme,
    /// Nominal glyph advance width in pixels (from the renderer font atlas).
    pub glyph_w: u32,
    /// Font line height in pixels (cell_h, includes atlas padding).
    pub line_h: u32,
    /// Actual ink height of glyphs (ascent + |descent| + line_gap, no padding).
    /// Use this for vertical centering — it avoids the off-by-a-few-pixels
    /// shift that cell_h causes in bar_text_y.
    pub natural_h: u32,
    /// TUI cell width — passed to Widget::render.
    cell_w: u32,
    /// TUI cell height — passed to Widget::render.
    cell_h: u32,
    regions: Vec<(String, Rect)>,
}

impl<'a> Frame<'a> {
    /// Full constructor — supply all font metrics explicitly.
    pub fn new_with_metrics(
        canvas: &'a mut PixelCanvas,
        layout: ScreenLayout,
        theme: &'a Theme,
        glyph_w: u32,
        line_h: u32,
        natural_h: u32,
    ) -> Self {
        Self {
            canvas,
            layout,
            theme,
            glyph_w,
            line_h,
            natural_h,
            cell_w: glyph_w,
            cell_h: line_h,
            regions: Vec::new(),
        }
    }

    /// Convenience constructor with metrics inferred as `(0, 0)`.
    ///
    /// Prefer [`Frame::new_with_metrics`] when font metrics are available.
    pub fn new(canvas: &'a mut PixelCanvas, layout: ScreenLayout, theme: &'a Theme) -> Self {
        Self::new_with_metrics(canvas, layout, theme, 0, 0, 0)
    }

    // ── Area accessors ────────────────────────────────────────────────────────

    pub fn area(&self) -> Rect {
        self.layout.vp
    }
    pub fn content_area(&self) -> Rect {
        self.layout.content
    }
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
        self.cell_w
    }
    pub fn cell_h(&self) -> u32 {
        self.cell_h
    }
    pub fn canvas(&mut self) -> &mut PixelCanvas {
        self.canvas
    }

    // ── TUI widget render helpers ─────────────────────────────────────────────

    pub fn render(&mut self, widget: impl Widget, area: Rect) {
        widget.render(self.canvas, area, self.cell_w, self.cell_h, self.theme);
    }

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
            self.cell_w,
            self.cell_h,
            self.theme,
        );
    }

    pub fn render_block(&mut self, block: Block<'_>, area: Rect) -> Rect {
        block.render(self.canvas, area, self.cell_w, self.cell_h, self.theme)
    }

    // ── Chrome helpers ────────────────────────────────────────────────────────

    pub fn draw_pane(&mut self, area: Rect, opts: PaneOpts) {
        draw_pane(
            self.canvas,
            area,
            &opts,
            self.glyph_w,
            self.line_h,
            self.theme,
        );
    }

    /// Begin building a status bar for `area`.
    ///
    /// Passes `natural_h` (not `line_h`) so that `bar_text_y` centers text
    /// correctly against the actual ink height of glyphs.
    pub fn bar(&mut self, area: Rect) -> BarBuilder<'_> {
        BarBuilder::new(
            self.canvas,
            area,
            self.theme,
            self.glyph_w,
            self.line_h,
            self.natural_h,
        )
    }

    // ── Hit region API ────────────────────────────────────────────────────────

    pub fn register_region(&mut self, name: impl Into<String>, area: Rect) {
        self.regions.push((name.into(), area));
    }

    pub fn into_regions(self) -> Vec<(String, Rect)> {
        self.regions
    }

    pub fn hit_test_regions(regions: &[(String, Rect)], x: u32, y: u32) -> Option<&str> {
        regions.iter().rev().find_map(|(name, rect)| {
            if x >= rect.x && x < rect.x + rect.w && y >= rect.y && y < rect.y + rect.h {
                Some(name.as_str())
            } else {
                None
            }
        })
    }
}

// ── App trait ─────────────────────────────────────────────────────────────────

pub trait App: Sized + 'static {
    type Message: Send + 'static;

    fn update(&mut self, event: Event<Self::Message>) -> Cmd<Self::Message>;
    fn view(&self, frame: &mut Frame);

    fn init(&mut self) -> Cmd<Self::Message> {
        Cmd::none()
    }
    fn theme(&self) -> Theme {
        Theme::default()
    }
    fn tick_rate(&self) -> u64 {
        60
    }
}

// ── Terminal ──────────────────────────────────────────────────────────────────

pub struct Terminal<B: crate::backend::Backend> {
    backend: B,
    spawn_queue: SpawnQueue<()>,
}

impl<B: crate::backend::Backend> Terminal<B> {
    pub fn new(backend: B) -> crate::Result<Self> {
        Ok(Self {
            backend,
            spawn_queue: Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new())),
        })
    }

    pub fn run<A: App>(mut self, mut app: A) -> crate::Result<()> {
        let spawn_queue: SpawnQueue<A::Message> =
            Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new()));

        let init_cmd = app.init();
        if process_cmd_tree(init_cmd, &mut app, &spawn_queue) {
            return Ok(());
        }

        let mut last_tick = std::time::Instant::now();

        'main: loop {
            drain_spawn(&spawn_queue, &mut app);

            while let Some(ev) = self.backend.poll_event::<A::Message>() {
                if process_cmd_tree(app.update(ev), &mut app, &spawn_queue) {
                    break 'main;
                }
            }

            let tick_ns = 1_000_000_000u64 / app.tick_rate();
            let now = std::time::Instant::now();
            if now.duration_since(last_tick).as_nanos() as u64 >= tick_ns {
                last_tick = now;
                if process_cmd_tree(app.update(Event::Tick), &mut app, &spawn_queue) {
                    break 'main;
                }
            }

            let (vp_w, vp_h) = self.backend.size();
            let (cell_w, cell_h) = self.backend.cell_size();
            let natural_h = self.backend.natural_h();
            let theme = app.theme();
            let sl = ScreenLayout::new(vp_w, vp_h, 0);
            let mut canvas = PixelCanvas::new(vp_w, vp_h);
            canvas.set_clip(Some(sl.vp));
            {
                let mut frame =
                    Frame::new_with_metrics(&mut canvas, sl, &theme, cell_w, cell_h, natural_h);
                app.view(&mut frame);
            }
            let cmds = canvas.finish();
            self.backend.render(&cmds, vp_w, vp_h);
        }

        Ok(())
    }
}

// ── Cmd processing helpers ────────────────────────────────────────────────────

pub(crate) fn process_cmd_tree<A: App>(
    root: Cmd<A::Message>,
    app: &mut A,
    spawn_queue: &SpawnQueue<A::Message>,
) -> bool {
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
            Cmd::Spawn(f) => {
                let q = Arc::clone(spawn_queue);
                std::thread::spawn(move || {
                    let msg = f();
                    if let Ok(mut guard) = q.lock() {
                        guard.push_back(msg);
                    }
                });
            }
        }
    }
    false
}

pub(crate) fn drain_spawn<A: App>(spawn_queue: &SpawnQueue<A::Message>, app: &mut A) {
    let msgs: Vec<A::Message> = {
        let mut guard = spawn_queue.lock().unwrap();
        guard.drain(..).collect()
    };
    for m in msgs {
        app.update(Event::Message(m));
    }
}
