//! smithay — SmithayApp: compositor-embedded chrome driver.

use crate::app::{
    event::{KeyEvent, MouseEvent},
    App, Event, Frame,
};
use crate::layout::ScreenLayout;
use crate::renderer::{
    gl::{ChromeRenderer, GlyphAtlas, Shaper},
    PixelCanvas,
};

/// A self-contained chrome renderer for use inside a Smithay compositor.
pub struct SmithayApp<A: App> {
    renderer: ChromeRenderer,
    app: A,
    vp_w: u32,
    vp_h: u32,
    /// Bar height in pixels, as configured by the compositor.
    /// Stored as pixels so it stays exact regardless of cell size.
    bar_h_px: u32,
    pending_keys: std::collections::VecDeque<KeyEvent>,
    pending_mouse: std::collections::VecDeque<MouseEvent>,
}

impl<A: App> SmithayApp<A> {
    pub fn new(
        font_bytes: &'static [u8],
        size_px: f32,
        vp_w: u32,
        vp_h: u32,
        mut app: A,
    ) -> crate::Result<Self> {
        let atlas = GlyphAtlas::new(font_bytes, None, None, size_px, 1.2)
            .map_err(|e| format!("GlyphAtlas: {e}"))?;
        let shaper = Shaper::new(font_bytes);
        let renderer = ChromeRenderer::new(atlas, shaper, 1000.0, size_px)
            .map_err(|e| format!("ChromeRenderer: {e}"))?;

        let _ = app.init();
        Ok(Self {
            renderer,
            app,
            vp_w,
            vp_h,
            bar_h_px: 0,
            pending_keys: Default::default(),
            pending_mouse: Default::default(),
        })
    }

    /// Set the bar height in pixels.
    ///
    /// Call this once after `new()` (and again on config reload) with the exact
    /// pixel height from your bar config. `render_frame` converts this to cells
    /// internally, rounding up so the full bar is always visible.
    ///
    /// ```rust,ignore
    /// ui.set_bar_height_px(config.bar.height);
    /// ```
    pub fn set_bar_height_px(&mut self, h: u32) {
        self.bar_h_px = h;
    }

    /// Current bar height in pixels.
    pub fn bar_h_px(&self) -> u32 {
        self.bar_h_px
    }

    /// Queue a key event.
    pub fn push_key(&mut self, ev: KeyEvent) {
        self.pending_keys.push_back(ev);
    }

    /// Queue a mouse event.
    pub fn push_mouse(&mut self, ev: MouseEvent) {
        self.pending_mouse.push_back(ev);
    }

    /// Deliver a typed app message directly (e.g. ChromeMsg::FullSnapshot).
    pub fn send(&mut self, msg: A::Message) {
        self.app.update(Event::Message(msg));
    }

    /// Update the viewport size.
    pub fn resize(&mut self, w: u32, h: u32) {
        self.vp_w = w;
        self.vp_h = h;
        self.app.update(Event::Resize(w, h));
    }

    /// Cell width in pixels.
    pub fn cell_w(&self) -> u32 {
        self.renderer.cell_w
    }

    /// Cell height in pixels.
    pub fn cell_h(&self) -> u32 {
        self.renderer.cell_h
    }

    /// Run one frame: deliver queued events, tick, render, flush to bound FBO.
    pub fn render_frame(&mut self) {
        while let Some(k) = self.pending_keys.pop_front() {
            self.app.update(Event::Key(k));
        }
        while let Some(m) = self.pending_mouse.pop_front() {
            self.app.update(Event::Mouse(m));
        }

        self.app.update(Event::Tick);

        let (vp_w, vp_h) = (self.vp_w, self.vp_h);
        if vp_w == 0 || vp_h == 0 {
            return;
        }

        let (cell_w, cell_h) = (self.renderer.cell_w, self.renderer.cell_h);

        // Convert pixel bar height to cells, rounding UP so the full bar is
        // always covered. A 28px bar with a 16px cell → 2 cells (32px reserved),
        // which is better than 1 cell (16px) clipping the bottom half.
        let bar_h_cells = if cell_h == 0 {
            0
        } else {
            (self.bar_h_px + cell_h - 1) / cell_h
        };

        let theme = self.app.theme();
        let sl = ScreenLayout::new(vp_w, vp_h, cell_w, cell_h, bar_h_cells);
        let mut canvas = PixelCanvas::new(vp_w, vp_h);
        {
            let mut frame = Frame::new(&mut canvas, sl, &theme);
            self.app.view(&mut frame);
        }
        let cmds = canvas.finish();
        self.renderer.flush(&cmds, vp_w, vp_h);
    }

    /// Cell dimensions derived from the font atlas.
    pub fn cell_size(&self) -> (u32, u32) {
        (self.renderer.cell_w, self.renderer.cell_h)
    }
}
