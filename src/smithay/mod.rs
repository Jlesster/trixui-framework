//! smithay — plug-and-play compositor chrome driver.
//!
//! # Minimal integration (5 lines)
//!
//! ```rust,no_run
//! use trixui::smithay::SmithayApp;
//! use trixui::prelude::*;
//!
//! struct MyChrome;
//! impl App for MyChrome {
//!     type Message = ();
//!     fn update(&mut self, _: Event<()>) -> Cmd<()> { Cmd::none() }
//!     fn view(&self, _: &mut Frame) {}
//! }
//!
//! // After your GL context is current:
//! let mut ui = SmithayApp::builder(MyChrome)
//!     .viewport(1920, 1080)
//!     .build()?;
//!
//! // Each DRM frame:
//! ui.render(); // → flushes DrawCmds into the bound FBO
//!
//! // Input:
//! ui.push_key(KeyEvent::plain(KeyCode::Char('j')));
//!
//! // Resize:
//! ui.resize(2560, 1440);
//!
//! // Hit-test for compositor routing:
//! if let Some(region) = ui.hit_test(x, y) {
//!     // "titlebar", "close_btn", etc.
//! }
//! ```
//!
//! ## Two-phase workflow (explicit-sync / multi-GPU)
//!
//! ```rust,no_run
//! // CPU phase — safe before FBO bind:
//! let cmds = ui.collect();
//!
//! // GPU phase — inside DRM render callback:
//! if ui.needs_flush() {
//!     ui.flush_collected(cmds);
//! }
//! ```
//!
//! ## Compositor input routing
//!
//! ```rust,no_run
//! // Deliver keyboard events only when chrome has focus:
//! match your_compositor.focused_surface() {
//!     FocusTarget::Chrome => ui.push_key(key),
//!     FocusTarget::Client(id) => client_map[id].push_key(key),
//! }
//!
//! // Mouse hit-test:
//! if let Some(region) = ui.hit_test(ptr_x, ptr_y) {
//!     match region {
//!         "titlebar" => /* start move */,
//!         "close_btn" => /* close window */,
//!         _ => {}
//!     }
//! }
//! ```

use std::sync::Arc;

use crate::app::{
    drain_spawn,
    event::{KeyEvent, MouseEvent},
    process_cmd_tree, App, Cmd, Event, Frame, SpawnQueue,
};
use crate::layout::{Rect, ScreenLayout};
use crate::renderer::{
    gl::{ChromeRenderer, GlyphAtlas, Shaper},
    DrawCmd, PixelCanvas,
};

// ── Default embedded font ─────────────────────────────────────────────────────

const DEFAULT_FONT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/IosevkaJlessBrainsNerdFontNerdFont-Regular.ttf"
));
const DEFAULT_FONT_SIZE: f32 = 20.0;

// ── FontConfig ────────────────────────────────────────────────────────────────

/// Font configuration for [`SmithayApp`].
#[derive(Clone)]
pub struct FontConfig {
    pub regular: Arc<[u8]>,
    pub bold: Option<Arc<[u8]>>,
    pub italic: Option<Arc<[u8]>>,
    pub size_px: f32,
}

impl FontConfig {
    pub fn new(regular: impl Into<Arc<[u8]>>, size_px: f32) -> Self {
        Self {
            regular: regular.into(),
            bold: None,
            italic: None,
            size_px,
        }
    }
    pub fn with_bold(mut self, data: impl Into<Arc<[u8]>>) -> Self {
        self.bold = Some(data.into());
        self
    }
    pub fn with_italic(mut self, data: impl Into<Arc<[u8]>>) -> Self {
        self.italic = Some(data.into());
        self
    }
}

impl Default for FontConfig {
    fn default() -> Self {
        Self::new(Arc::<[u8]>::from(DEFAULT_FONT), DEFAULT_FONT_SIZE)
    }
}

// ── Builder ───────────────────────────────────────────────────────────────────

/// Builder for [`SmithayApp`]. Obtain via [`SmithayApp::builder`].
pub struct SmithayAppBuilder<A: App> {
    app: A,
    font: FontConfig,
    vp_w: u32,
    vp_h: u32,
    bar_h_px: u32,
}

impl<A: App> SmithayAppBuilder<A> {
    fn new(app: A) -> Self {
        Self {
            app,
            font: FontConfig::default(),
            vp_w: 1920,
            vp_h: 1080,
            bar_h_px: 0,
        }
    }

    /// Use a custom font file.
    pub fn font(mut self, data: impl Into<Arc<[u8]>>, size_px: f32) -> Self {
        self.font = FontConfig::new(data, size_px);
        self
    }
    /// Supply a full [`FontConfig`] (bold/italic variants + size).
    pub fn font_config(mut self, cfg: FontConfig) -> Self {
        self.font = cfg;
        self
    }
    /// Initial viewport dimensions in physical pixels.
    pub fn viewport(mut self, w: u32, h: u32) -> Self {
        self.vp_w = w;
        self.vp_h = h;
        self
    }
    /// Reserve this many **exact pixels** at the bottom for a status bar.
    pub fn bar_height_px(mut self, h: u32) -> Self {
        self.bar_h_px = h;
        self
    }
    /// Build. Requires a current GL context.
    pub fn build(self) -> crate::Result<SmithayApp<A>> {
        SmithayApp::from_builder(self)
    }
}

// ── SmithayApp ────────────────────────────────────────────────────────────────

/// Self-contained chrome renderer for a Smithay compositor.
///
/// See the [module docs](self) for a minimal integration example.
pub struct SmithayApp<A: App> {
    renderer: ChromeRenderer,
    app: A,
    vp_w: u32,
    vp_h: u32,
    bar_h_px: u32,
    pending_keys: std::collections::VecDeque<KeyEvent>,
    pending_mouse: std::collections::VecDeque<MouseEvent>,
    dirty: bool,
    last_cmds: Vec<DrawCmd>,
    /// Hit regions from the last rendered frame.
    last_regions: Vec<(String, Rect)>,
    /// Spawn queue for background tasks (Cmd::Spawn).
    spawn_queue: SpawnQueue<A::Message>,
}

impl<A: App> SmithayApp<A> {
    // ── Construction ──────────────────────────────────────────────────────────

    /// Start building. Call `.build()` once the GL context is current.
    pub fn builder(app: A) -> SmithayAppBuilder<A> {
        SmithayAppBuilder::new(app)
    }

    /// Convenience constructor using the default embedded font.
    pub fn new(app: A, vp_w: u32, vp_h: u32) -> crate::Result<Self> {
        SmithayApp::builder(app).viewport(vp_w, vp_h).build()
    }

    fn from_builder(b: SmithayAppBuilder<A>) -> crate::Result<Self> {
        let font = &b.font;

        let atlas = GlyphAtlas::new(
            &font.regular,
            font.bold.as_deref(),
            font.italic.as_deref(),
            font.size_px,
            1.2,
        )
        .map_err(|e| format!("GlyphAtlas: {e}"))?;

        let shaper = Shaper::new(&font.regular);
        let renderer = ChromeRenderer::new(atlas, shaper, 1000.0, font.size_px)
            .map_err(|e| format!("ChromeRenderer: {e}"))?;

        let spawn_queue: SpawnQueue<A::Message> =
            Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new()));

        let mut app = b.app;
        // Correctly process init Cmd — including Quit and Msg variants.
        let init_cmd = app.init();
        process_cmd_tree(init_cmd, &mut app, &spawn_queue);

        Ok(Self {
            renderer,
            app,
            vp_w: b.vp_w,
            vp_h: b.vp_h,
            bar_h_px: b.bar_h_px,
            pending_keys: Default::default(),
            pending_mouse: Default::default(),
            dirty: true,
            last_cmds: Vec::new(),
            last_regions: Vec::new(),
            spawn_queue,
        })
    }

    // ── Input ─────────────────────────────────────────────────────────────────

    /// Enqueue a key event to be processed on the next frame.
    pub fn push_key(&mut self, ev: KeyEvent) {
        self.pending_keys.push_back(ev);
        self.dirty = true;
    }

    /// Enqueue a mouse event to be processed on the next frame.
    pub fn push_mouse(&mut self, ev: MouseEvent) {
        self.pending_mouse.push_back(ev);
        self.dirty = true;
    }

    /// Notify that the compositor surface gained keyboard focus.
    pub fn focus_gained(&mut self) {
        let cmd = self.app.update(Event::FocusGained);
        process_cmd_tree(cmd, &mut self.app, &self.spawn_queue.clone());
        self.dirty = true;
    }

    /// Notify that the compositor surface lost keyboard focus.
    pub fn focus_lost(&mut self) {
        let cmd = self.app.update(Event::FocusLost);
        process_cmd_tree(cmd, &mut self.app, &self.spawn_queue.clone());
        self.dirty = true;
    }

    /// Deliver a scroll delta (logical pixels). Positive Y = scroll down.
    pub fn push_scroll(&mut self, x: f32, y: f32) {
        let cmd = self.app.update(Event::Scroll { x, y });
        process_cmd_tree(cmd, &mut self.app, &self.spawn_queue.clone());
        self.dirty = true;
    }

    /// Deliver a typed application message directly (bypasses the event queue).
    pub fn send(&mut self, msg: A::Message) {
        let cmd = self.app.update(Event::Message(msg));
        process_cmd_tree(cmd, &mut self.app, &self.spawn_queue.clone());
        self.dirty = true;
    }

    // ── Geometry ──────────────────────────────────────────────────────────────

    /// Notify of a viewport resize.
    pub fn resize(&mut self, w: u32, h: u32) {
        if self.vp_w == w && self.vp_h == h {
            return;
        }
        self.vp_w = w;
        self.vp_h = h;
        let cmd = self.app.update(Event::Resize(w, h));
        process_cmd_tree(cmd, &mut self.app, &self.spawn_queue.clone());
        self.dirty = true;
    }

    /// Update the status-bar height (exact pixels, no cell-quantisation).
    pub fn set_bar_height_px(&mut self, h: u32) {
        if self.bar_h_px == h {
            return;
        }
        self.bar_h_px = h;
        self.dirty = true;
    }

    /// Returns true if there's pending work (dirty state, queued events, or
    /// pending spawn results).
    pub fn needs_flush(&self) -> bool {
        self.dirty
            || !self.pending_keys.is_empty()
            || !self.pending_mouse.is_empty()
            || self.spawn_queue.lock().map_or(false, |g| !g.is_empty())
    }

    /// The current [`ScreenLayout`] (recalculated without re-rendering).
    pub fn layout(&self) -> ScreenLayout {
        ScreenLayout::new(self.vp_w, self.vp_h, self.bar_h_px)
    }

    /// Raw font glyph advance width from the renderer (pixels).
    pub fn glyph_w(&self) -> u32 {
        self.renderer.cell_w
    }
    /// Raw font line height from the renderer (pixels).
    pub fn line_h(&self) -> u32 {
        self.renderer.cell_h
    }

    // ── Hit-test ──────────────────────────────────────────────────────────────

    /// Test a physical-pixel coordinate against regions registered during the
    /// last `render()` / `collect()` call.
    ///
    /// Returns the name of the hit region (e.g. `"titlebar"`, `"close_btn"`),
    /// or `None` if outside all registered regions.
    ///
    /// ```rust,ignore
    /// // In your compositor pointer-motion handler:
    /// match ui.hit_test(ptr_x, ptr_y) {
    ///     Some("titlebar")  => start_interactive_move(),
    ///     Some("close_btn") => close_window(),
    ///     _                 => forward_to_client(),
    /// }
    /// ```
    pub fn hit_test(&self, x: u32, y: u32) -> Option<&str> {
        Frame::hit_test_regions(&self.last_regions, x, y)
    }

    /// Access the last rendered hit regions directly (for custom routing).
    pub fn regions(&self) -> &[(String, Rect)] {
        &self.last_regions
    }

    // ── Render API ────────────────────────────────────────────────────────────

    /// **Primary entry-point.**
    ///
    /// Processes all pending input and spawn results, ticks the app, renders
    /// the view, and flushes draw commands into the currently-bound FBO.
    ///
    /// Skips the GL flush if nothing changed (damage tracking).
    /// Returns `true` if a GPU flush was performed.
    pub fn render(&mut self) -> bool {
        let (cmds, regions) = self.collect_inner();
        if cmds_equal(&self.last_cmds, &cmds) && !self.dirty {
            return false;
        }
        self.flush_inner(&cmds);
        self.last_cmds = cmds;
        self.last_regions = regions;
        self.dirty = false;
        true
    }

    // ── Two-phase API ─────────────────────────────────────────────────────────

    /// **Phase 1 — CPU only.** Process events + tick + build draw list.
    ///
    /// Safe to call before the DRM render pass / FBO bind. Does not touch GL.
    /// After this call, [`needs_flush`](Self::needs_flush) indicates whether
    /// the draw list changed.
    pub fn collect(&mut self) -> Vec<DrawCmd> {
        let (cmds, regions) = self.collect_inner();
        if cmds_equal(&self.last_cmds, &cmds) {
            self.dirty = false;
        }
        self.last_regions = regions;
        cmds
    }

    /// **Phase 2 — GL.** Flush previously collected commands into the bound FBO.
    pub fn flush_collected(&mut self, cmds: Vec<DrawCmd>) {
        self.flush_inner(&cmds);
        self.last_cmds = cmds;
        self.dirty = false;
    }

    // ── Internal ──────────────────────────────────────────────────────────────

    fn collect_inner(&mut self) -> (Vec<DrawCmd>, Vec<(String, Rect)>) {
        // Drain spawn queue first (results from background tasks).
        let sq = self.spawn_queue.clone();
        drain_spawn(&sq, &mut self.app);

        // Drain pending events.
        while let Some(k) = self.pending_keys.pop_front() {
            let cmd = self.app.update(Event::Key(k));
            process_cmd_tree(cmd, &mut self.app, &self.spawn_queue);
        }
        while let Some(m) = self.pending_mouse.pop_front() {
            let cmd = self.app.update(Event::Mouse(m));
            process_cmd_tree(cmd, &mut self.app, &self.spawn_queue);
        }

        // Tick.
        let cmd = self.app.update(Event::Tick);
        process_cmd_tree(cmd, &mut self.app, &self.spawn_queue);

        if self.vp_w == 0 || self.vp_h == 0 {
            return (Vec::new(), Vec::new());
        }

        let theme = self.app.theme();
        let sl = ScreenLayout::new(self.vp_w, self.vp_h, self.bar_h_px);
        let mut canvas = PixelCanvas::new(self.vp_w, self.vp_h);

        let regions = {
            let mut frame = Frame::new_with_metrics(
                &mut canvas,
                sl,
                &theme,
                self.renderer.cell_w, // glyph_w
                self.renderer.cell_h, // line_h
            );
            self.app.view(&mut frame);
            frame.into_regions()
        };

        (canvas.finish(), regions)
    }

    fn flush_inner(&mut self, cmds: &[DrawCmd]) {
        tracing::debug!(
            vp_w = self.vp_w,
            vp_h = self.vp_h,
            cmds = cmds.len(),
            "SmithayApp::flush"
        );
        self.renderer.flush(cmds, self.vp_w, self.vp_h);
    }
}

// ── Damage detection ──────────────────────────────────────────────────────────

/// Returns `true` when the two draw-command lists are structurally identical.
/// Named correctly: `cmds_equal` (previously the naming was inverted).
fn cmds_equal(prev: &[DrawCmd], next: &[DrawCmd]) -> bool {
    if prev.len() != next.len() {
        return false;
    }
    prev.iter().zip(next.iter()).all(|(a, b)| drawcmd_eq(a, b))
}

fn drawcmd_eq(a: &DrawCmd, b: &DrawCmd) -> bool {
    use DrawCmd::*;
    match (a, b) {
        (
            FillRect {
                x: x1,
                y: y1,
                w: w1,
                h: h1,
                color: c1,
            },
            FillRect {
                x: x2,
                y: y2,
                w: w2,
                h: h2,
                color: c2,
            },
        ) => x1 == x2 && y1 == y2 && w1 == w2 && h1 == h2 && c1 == c2,
        (
            StrokeRect {
                x: x1,
                y: y1,
                w: w1,
                h: h1,
                color: c1,
            },
            StrokeRect {
                x: x2,
                y: y2,
                w: w2,
                h: h2,
                color: c2,
            },
        ) => x1 == x2 && y1 == y2 && w1 == w2 && h1 == h2 && c1 == c2,
        (
            HLine {
                x: x1,
                y: y1,
                w: w1,
                color: c1,
            },
            HLine {
                x: x2,
                y: y2,
                w: w2,
                color: c2,
            },
        ) => x1 == x2 && y1 == y2 && w1 == w2 && c1 == c2,
        (
            VLine {
                x: x1,
                y: y1,
                h: h1,
                color: c1,
            },
            VLine {
                x: x2,
                y: y2,
                h: h2,
                color: c2,
            },
        ) => x1 == x2 && y1 == y2 && h1 == h2 && c1 == c2,
        (
            BorderLine {
                x: x1,
                y: y1,
                w: w1,
                h: h1,
                sides: s1,
                color: c1,
                thickness: t1,
            },
            BorderLine {
                x: x2,
                y: y2,
                w: w2,
                h: h2,
                sides: s2,
                color: c2,
                thickness: t2,
            },
        ) => x1 == x2 && y1 == y2 && w1 == w2 && h1 == h2 && s1 == s2 && c1 == c2 && t1 == t2,
        (
            RoundRect {
                x: x1,
                y: y1,
                w: w1,
                h: h1,
                radii: r1,
                fill: f1,
                stroke: s1,
                stroke_w: sw1,
            },
            RoundRect {
                x: x2,
                y: y2,
                w: w2,
                h: h2,
                radii: r2,
                fill: f2,
                stroke: s2,
                stroke_w: sw2,
            },
        ) => {
            x1 == x2
                && y1 == y2
                && w1 == w2
                && h1 == h2
                && r1 == r2
                && f1 == f2
                && s1 == s2
                && sw1 == sw2
        }
        (
            PowerlineArrow {
                x: x1,
                y: y1,
                w: w1,
                h: h1,
                dir: d1,
                color: c1,
            },
            PowerlineArrow {
                x: x2,
                y: y2,
                w: w2,
                h: h2,
                dir: d2,
                color: c2,
            },
        ) => x1 == x2 && y1 == y2 && w1 == w2 && h1 == h2 && d1 == d2 && c1 == c2,
        (
            Text {
                x: x1,
                y: y1,
                text: t1,
                style: s1,
                max_w: m1,
            },
            Text {
                x: x2,
                y: y2,
                text: t2,
                style: s2,
                max_w: m2,
            },
        ) => {
            x1 == x2
                && y1 == y2
                && t1 == t2
                && s1.fg == s2.fg
                && s1.bg == s2.bg
                && s1.bold == s2.bold
                && s1.italic == s2.italic
                && m1 == m2
        }
        _ => false,
    }
}
