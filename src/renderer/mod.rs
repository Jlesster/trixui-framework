//! renderer — GPU primitives, immediate-mode canvas, and theme.
//!
//! All coordinates are pixel-space: top-left origin, X right, Y down.
//! NDC conversion happens ONLY inside GLSL vertex shaders in `gl.rs`.
//! Never do NDC math in Rust.
//!
//! # Primitive hierarchy
//!
//! ```text
//! PixelCanvas methods          (ergonomic widget API)
//!        │
//!        ▼
//!    DrawCmd enum               (GPU contract — serialisable, Clone)
//!        │
//!        ▼
//! ChromeRenderer::flush          (4 instanced GL passes)
//!   Pass 1 — BgInst             FillRect, StrokeRect, HLine, VLine,
//!                                BorderLine (→ per-side rects)
//!   Pass 2 — RRectInst          RoundRect (SDF path)
//!   Pass 3 — GlyphInst          Text (HarfBuzz + glyph atlas)
//!   Pass 4 — TriInst            PowerlineArrow
//! ```
//!
//! # Widget code contract
//!
//! ```text
//! canvas.fill()        → DrawCmd::FillRect
//! canvas.stroke()      → DrawCmd::StrokeRect
//! canvas.hline()       → DrawCmd::HLine
//! canvas.vline()       → DrawCmd::VLine
//! canvas.border()      → DrawCmd::BorderLine
//! canvas.round_rect()  → DrawCmd::RoundRect  (SDF, anti-aliased)
//! canvas.round_fill()  → DrawCmd::RoundRect  (fill-only convenience)
//! canvas.round_stroke()→ DrawCmd::RoundRect  (stroke-only convenience)
//! canvas.powerline()   → DrawCmd::PowerlineArrow
//! canvas.text()        → DrawCmd::Text        ← actual text ONLY
//! canvas.text_maxw()   → DrawCmd::Text        ← actual text ONLY
//! ```

pub mod gl;
pub use gl::ChromeRenderer;

use crate::layout::Rect;

// ── Color ─────────────────────────────────────────────────────────────────────

/// RGBA8 colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Color(pub u8, pub u8, pub u8, pub u8);

impl Color {
    pub const TRANSPARENT: Self = Self(0, 0, 0, 0);

    /// Opaque colour from RGB components.
    pub fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self(r, g, b, 255)
    }

    /// Opaque colour from a 24-bit `0xRRGGBB` hex literal.
    ///
    /// The top byte is **ignored** — this is always fully opaque.
    /// Use [`Color::rgba`] if you need transparency.
    ///
    /// ```rust
    /// # use trixui::PixColor as Color;
    /// let lavender = Color::hex(0xb4befe); // correct
    /// // Color::hex(0xFF_b4befe) also works (top byte ignored)
    /// ```
    pub fn hex(v: u32) -> Self {
        Self((v >> 16) as u8, (v >> 8) as u8, v as u8, 255)
    }

    /// Colour from RGBA components.
    pub fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self(r, g, b, a)
    }

    pub fn is_transparent(self) -> bool {
        self.3 == 0
    }

    pub(crate) fn to_f32(self) -> [f32; 4] {
        [
            self.0 as f32 / 255.0,
            self.1 as f32 / 255.0,
            self.2 as f32 / 255.0,
            self.3 as f32 / 255.0,
        ]
    }
}

// ── TextStyle ─────────────────────────────────────────────────────────────────

/// Text rendering style for `DrawCmd::Text`.
#[derive(Debug, Clone, Copy)]
pub struct TextStyle {
    pub fg: Color,
    pub bg: Color,
    pub bold: bool,
    pub italic: bool,
}

impl TextStyle {
    pub fn fg(color: Color) -> Self {
        Self {
            fg: color,
            bg: Color::TRANSPARENT,
            bold: false,
            italic: false,
        }
    }
}

// ── BorderSide ────────────────────────────────────────────────────────────────

/// Which sides to draw for `DrawCmd::BorderLine`.
///
/// `u8` bitmask — same bit layout as `widget::Borders` so the cast is free.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BorderSide(pub u8);

impl BorderSide {
    pub const NONE: Self = Self(0b0000);
    pub const TOP: Self = Self(0b0001);
    pub const BOTTOM: Self = Self(0b0010);
    pub const LEFT: Self = Self(0b0100);
    pub const RIGHT: Self = Self(0b1000);
    pub const ALL: Self = Self(0b1111);

    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
    pub fn or(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

// ── CornerRadius ─────────────────────────────────────────────────────────────

/// Per-corner pixel radii for `DrawCmd::RoundRect`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CornerRadius {
    pub tl: f32,
    pub tr: f32,
    pub bl: f32,
    pub br: f32,
}

impl CornerRadius {
    pub fn all(r: f32) -> Self {
        Self {
            tl: r,
            tr: r,
            bl: r,
            br: r,
        }
    }
    pub fn none() -> Self {
        Self::default()
    }

    pub fn top_left(mut self, r: f32) -> Self {
        self.tl = r;
        self
    }
    pub fn top_right(mut self, r: f32) -> Self {
        self.tr = r;
        self
    }
    pub fn bottom_left(mut self, r: f32) -> Self {
        self.bl = r;
        self
    }
    pub fn bottom_right(mut self, r: f32) -> Self {
        self.br = r;
        self
    }

    pub fn is_none(self) -> bool {
        self.tl == 0.0 && self.tr == 0.0 && self.bl == 0.0 && self.br == 0.0
    }
}

// ── PowerlineDir ──────────────────────────────────────────────────────────────

/// Arrow style for `DrawCmd::PowerlineArrow`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PowerlineDir {
    RightFill = 0,
    LeftFill = 1,
    RightChevron = 2,
    LeftChevron = 3,
}

impl PowerlineDir {
    pub(crate) fn as_f32(self) -> f32 {
        self as u8 as f32
    }
    pub(crate) fn is_filled(self) -> bool {
        matches!(self, Self::RightFill | Self::LeftFill)
    }
}

// ── DrawCmd ───────────────────────────────────────────────────────────────────

/// A single GPU draw call. All coords are pixel-space, top-left origin.
#[derive(Debug, Clone)]
pub enum DrawCmd {
    FillRect {
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        color: Color,
    },
    StrokeRect {
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        color: Color,
    },
    HLine {
        x: u32,
        y: u32,
        w: u32,
        color: Color,
    },
    VLine {
        x: u32,
        y: u32,
        h: u32,
        color: Color,
    },

    /// Per-side border lines — use `canvas.border()`.
    BorderLine {
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        sides: BorderSide,
        color: Color,
        thickness: u32,
    },

    /// SDF rounded-rect — fill and/or stroke in a single pass.
    RoundRect {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radii: CornerRadius,
        fill: Color,
        stroke: Color,
        stroke_w: f32,
    },

    /// Powerline glyph geometry — use `canvas.powerline()`.
    PowerlineArrow {
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        dir: PowerlineDir,
        color: Color,
    },

    /// Actual text only. Never embed box-drawing or Powerline codepoints here.
    Text {
        x: u32,
        y: u32,
        text: String,
        style: TextStyle,
        max_w: Option<u32>,
    },
}

// ── PixelCanvas ───────────────────────────────────────────────────────────────

/// Immediate-mode draw list. All widget rendering goes through here.
///
/// ```rust,no_run
/// # use trixui::renderer::{PixelCanvas, TextStyle, Color as PixColor};
/// # let (vp_w, vp_h) = (800u32, 600u32);
/// let mut canvas = PixelCanvas::new(vp_w, vp_h);
/// canvas.fill(10, 10, 100, 50, PixColor::hex(0xff0000));
/// canvas.text(10, 10, "hello", TextStyle::fg(PixColor::hex(0xffffff)));
/// let cmds = canvas.finish();
/// ```
pub struct PixelCanvas {
    cmds: Vec<DrawCmd>,
    clip: Option<Rect>,
    #[allow(dead_code)]
    vp_w: u32,
    #[allow(dead_code)]
    vp_h: u32,
}

impl PixelCanvas {
    pub fn new(vp_w: u32, vp_h: u32) -> Self {
        Self {
            cmds: Vec::with_capacity(256),
            clip: None,
            vp_w,
            vp_h,
        }
    }

    pub fn set_clip(&mut self, r: Option<Rect>) {
        self.clip = r;
    }

    /// Consume the canvas and return the collected draw commands.
    pub fn finish(self) -> Vec<DrawCmd> {
        self.cmds
    }

    /// Create a clip-scoped child canvas constrained to `clip`.
    pub fn child(&mut self, clip: Rect) -> ChildCanvas<'_> {
        ChildCanvas { parent: self, clip }
    }

    // ── Primitives ────────────────────────────────────────────────────────────

    pub fn fill(&mut self, x: u32, y: u32, w: u32, h: u32, color: Color) {
        if color.is_transparent() || w == 0 || h == 0 {
            return;
        }
        self.cmds.push(DrawCmd::FillRect { x, y, w, h, color });
    }

    pub fn stroke(&mut self, x: u32, y: u32, w: u32, h: u32, color: Color) {
        if color.is_transparent() {
            return;
        }
        self.cmds.push(DrawCmd::StrokeRect { x, y, w, h, color });
    }

    pub fn hline(&mut self, x: u32, y: u32, w: u32, color: Color) {
        if color.is_transparent() || w == 0 {
            return;
        }
        self.cmds.push(DrawCmd::HLine { x, y, w, color });
    }

    pub fn vline(&mut self, x: u32, y: u32, h: u32, color: Color) {
        if color.is_transparent() || h == 0 {
            return;
        }
        self.cmds.push(DrawCmd::VLine { x, y, h, color });
    }

    pub fn border(
        &mut self,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        sides: BorderSide,
        color: Color,
        thickness: u32,
    ) {
        if sides == BorderSide::NONE || color.is_transparent() {
            return;
        }
        self.cmds.push(DrawCmd::BorderLine {
            x,
            y,
            w,
            h,
            sides,
            color,
            thickness,
        });
    }

    pub fn round_rect(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radii: CornerRadius,
        fill: Color,
        stroke: Color,
        stroke_w: f32,
    ) {
        if fill.is_transparent() && (stroke.is_transparent() || stroke_w == 0.0) {
            return;
        }
        self.cmds.push(DrawCmd::RoundRect {
            x,
            y,
            w,
            h,
            radii,
            fill,
            stroke,
            stroke_w,
        });
    }

    pub fn round_fill(&mut self, x: f32, y: f32, w: f32, h: f32, radii: CornerRadius, fill: Color) {
        self.round_rect(x, y, w, h, radii, fill, Color::TRANSPARENT, 0.0);
    }

    pub fn round_stroke(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radii: CornerRadius,
        stroke: Color,
        stroke_w: f32,
    ) {
        self.round_rect(x, y, w, h, radii, Color::TRANSPARENT, stroke, stroke_w);
    }

    pub fn powerline(&mut self, x: u32, y: u32, w: u32, h: u32, dir: PowerlineDir, color: Color) {
        if color.is_transparent() {
            return;
        }
        self.cmds.push(DrawCmd::PowerlineArrow {
            x,
            y,
            w,
            h,
            dir,
            color,
        });
    }

    /// Render `s` as shaped text.  **Actual text only** — no box-draw codepoints.
    pub fn text(&mut self, x: u32, y: u32, s: &str, style: TextStyle) {
        if s.is_empty() {
            return;
        }
        if let Some(c) = self.clip {
            if x >= c.x + c.w || y >= c.y + c.h {
                return;
            }
        }
        self.cmds.push(DrawCmd::Text {
            x,
            y,
            text: s.to_string(),
            style,
            max_w: self.clip.map(|c| c.x + c.w - x),
        });
    }

    /// Like [`text`](Self::text) but with an explicit pixel width cap.
    pub fn text_maxw(&mut self, x: u32, y: u32, s: &str, style: TextStyle, max_w: u32) {
        if s.is_empty() || max_w == 0 {
            return;
        }
        self.cmds.push(DrawCmd::Text {
            x,
            y,
            text: s.to_string(),
            style,
            max_w: Some(max_w),
        });
    }
}

// ── ChildCanvas ───────────────────────────────────────────────────────────────

/// A clip-scoped view of a [`PixelCanvas`].
///
/// All draw calls are constrained to the `clip` rect — coordinates outside it
/// are silently discarded or clamped.
pub struct ChildCanvas<'a> {
    parent: &'a mut PixelCanvas,
    clip: Rect,
}

impl<'a> ChildCanvas<'a> {
    fn clip_rect(&self, x: u32, y: u32, w: u32, h: u32) -> Option<(u32, u32, u32, u32)> {
        let cx1 = self.clip.x + self.clip.w;
        let cy1 = self.clip.y + self.clip.h;
        let x0 = x.max(self.clip.x);
        let y0 = y.max(self.clip.y);
        let x1 = (x + w).min(cx1);
        let y1 = (y + h).min(cy1);
        if x1 > x0 && y1 > y0 {
            Some((x0, y0, x1 - x0, y1 - y0))
        } else {
            None
        }
    }

    fn in_clip(&self, x: u32, y: u32, w: u32, h: u32) -> bool {
        x + w > self.clip.x
            && y + h > self.clip.y
            && x < self.clip.x + self.clip.w
            && y < self.clip.y + self.clip.h
    }

    pub fn fill(&mut self, x: u32, y: u32, w: u32, h: u32, color: Color) {
        if let Some((x, y, w, h)) = self.clip_rect(x, y, w, h) {
            self.parent.fill(x, y, w, h, color);
        }
    }

    pub fn stroke(&mut self, x: u32, y: u32, w: u32, h: u32, color: Color) {
        if self.in_clip(x, y, w, h) {
            self.parent.stroke(x, y, w, h, color);
        }
    }

    pub fn hline(&mut self, x: u32, y: u32, w: u32, color: Color) {
        let x1 = (x + w).min(self.clip.x + self.clip.w);
        if x1 > x && y >= self.clip.y && y < self.clip.y + self.clip.h {
            self.parent.hline(x, y, x1 - x, color);
        }
    }

    pub fn vline(&mut self, x: u32, y: u32, h: u32, color: Color) {
        let y1 = (y + h).min(self.clip.y + self.clip.h);
        if y1 > y && x >= self.clip.x && x < self.clip.x + self.clip.w {
            self.parent.vline(x, y, y1 - y, color);
        }
    }

    pub fn border(
        &mut self,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        sides: BorderSide,
        color: Color,
        thickness: u32,
    ) {
        if self.in_clip(x, y, w, h) {
            self.parent.border(x, y, w, h, sides, color, thickness);
        }
    }

    pub fn round_rect(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radii: CornerRadius,
        fill: Color,
        stroke: Color,
        stroke_w: f32,
    ) {
        let cx = self.clip.x as f32;
        let cy = self.clip.y as f32;
        if x < cx + self.clip.w as f32 && y < cy + self.clip.h as f32 && x + w > cx && y + h > cy {
            self.parent
                .round_rect(x, y, w, h, radii, fill, stroke, stroke_w);
        }
    }

    pub fn round_fill(&mut self, x: f32, y: f32, w: f32, h: f32, radii: CornerRadius, fill: Color) {
        self.round_rect(x, y, w, h, radii, fill, Color::TRANSPARENT, 0.0);
    }

    pub fn round_stroke(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radii: CornerRadius,
        stroke: Color,
        stroke_w: f32,
    ) {
        self.round_rect(x, y, w, h, radii, Color::TRANSPARENT, stroke, stroke_w);
    }

    pub fn powerline(&mut self, x: u32, y: u32, w: u32, h: u32, dir: PowerlineDir, color: Color) {
        self.parent.powerline(x, y, w, h, dir, color);
    }

    pub fn text(&mut self, x: u32, y: u32, s: &str, style: TextStyle) {
        if x >= self.clip.x + self.clip.w || y >= self.clip.y + self.clip.h {
            return;
        }
        let max_w = self.clip.x + self.clip.w - x;
        self.parent.text_maxw(x, y, s, style, max_w);
    }

    pub fn text_maxw(&mut self, x: u32, y: u32, s: &str, style: TextStyle, max_w: u32) {
        if x >= self.clip.x + self.clip.w || y >= self.clip.y + self.clip.h {
            return;
        }
        let capped = max_w.min(self.clip.x + self.clip.w - x);
        self.parent.text_maxw(x, y, s, style, capped);
    }

    pub fn x(&self) -> u32 {
        self.clip.x
    }
    pub fn y(&self) -> u32 {
        self.clip.y
    }
    pub fn width(&self) -> u32 {
        self.clip.w
    }
    pub fn height(&self) -> u32 {
        self.clip.h
    }
    pub fn rect(&self) -> Rect {
        self.clip
    }
}

// ── Theme ─────────────────────────────────────────────────────────────────────

/// Colour theme.
///
/// Two categories of slots:
/// - **Content** (`normal_*`, `highlight_*`, `dim_fg`) — used by `Paragraph`,
///   `List`, `Table`, and any widget that renders content text.
/// - **Chrome** (`active_border`, `bar_*`, `ws_*`) — used by `Block`, `Tabs`,
///   the status bar, and compositor decorations.
///
/// Defaults to Catppuccin Mocha.
#[derive(Clone, Copy, Debug)]
pub struct Theme {
    // ── content slots ─────────────────────────────────────────────────────────
    /// Default foreground for content text (Paragraph, List items, Table cells).
    pub normal_fg: Color,
    /// Default background for content areas.
    pub normal_bg: Color,
    /// Foreground used for selected / highlighted rows.
    pub highlight_fg: Color,
    /// Background used for selected / highlighted rows.
    pub highlight_bg: Color,
    /// Muted foreground — column headers, secondary text.
    pub dim_fg: Color,

    // ── chrome / border slots ─────────────────────────────────────────────────
    pub active_border: Color,
    pub inactive_border: Color,
    pub active_title: Color,
    pub inactive_title: Color,
    pub pane_bg: Color,

    // ── status bar slots ──────────────────────────────────────────────────────
    pub bar_bg: Color,
    pub bar_fg: Color,
    pub bar_accent: Color,
    pub bar_dim: Color,

    // ── workspace / tab pill slots ────────────────────────────────────────────
    pub ws_active_fg: Color,
    pub ws_active_bg: Color,
}

impl Default for Theme {
    /// Catppuccin Mocha.
    fn default() -> Self {
        Self {
            normal_fg: Color::hex(0xcdd6f4),
            normal_bg: Color::hex(0x11111b),
            highlight_fg: Color::hex(0x11111b),
            highlight_bg: Color::hex(0xb4befe),
            dim_fg: Color::hex(0x585b70),

            active_border: Color::hex(0xb4befe),
            inactive_border: Color::hex(0x45475a),
            active_title: Color::hex(0xb4befe),
            inactive_title: Color::hex(0x585b70),
            pane_bg: Color::hex(0x11111b),

            bar_bg: Color::hex(0x181825),
            bar_fg: Color::hex(0xa6adc8),
            bar_accent: Color::hex(0xb4befe),
            bar_dim: Color::hex(0x585b70),

            ws_active_fg: Color::hex(0x11111b),
            ws_active_bg: Color::hex(0xb4befe),
        }
    }
}

impl Theme {
    /// Catppuccin Latte (light).
    pub fn latte() -> Self {
        Self {
            normal_fg: Color::hex(0x4c4f69),
            normal_bg: Color::hex(0xeff1f5),
            highlight_fg: Color::hex(0xeff1f5),
            highlight_bg: Color::hex(0x7287fd),
            dim_fg: Color::hex(0x8c8fa1),

            active_border: Color::hex(0x7287fd),
            inactive_border: Color::hex(0xacb0be),
            active_title: Color::hex(0x7287fd),
            inactive_title: Color::hex(0x8c8fa1),
            pane_bg: Color::hex(0xeff1f5),

            bar_bg: Color::hex(0xe6e9ef),
            bar_fg: Color::hex(0x4c4f69),
            bar_accent: Color::hex(0x7287fd),
            bar_dim: Color::hex(0x8c8fa1),

            ws_active_fg: Color::hex(0xeff1f5),
            ws_active_bg: Color::hex(0x7287fd),
        }
    }

    /// Catppuccin Macchiato.
    pub fn macchiato() -> Self {
        Self {
            normal_fg: Color::hex(0xcad3f5),
            normal_bg: Color::hex(0x1e2030),
            highlight_fg: Color::hex(0x1e2030),
            highlight_bg: Color::hex(0xb7bdf8),
            dim_fg: Color::hex(0x6e738d),

            active_border: Color::hex(0xb7bdf8),
            inactive_border: Color::hex(0x494d64),
            active_title: Color::hex(0xb7bdf8),
            inactive_title: Color::hex(0x6e738d),
            pane_bg: Color::hex(0x1e2030),

            bar_bg: Color::hex(0x181926),
            bar_fg: Color::hex(0xcad3f5),
            bar_accent: Color::hex(0xb7bdf8),
            bar_dim: Color::hex(0x6e738d),

            ws_active_fg: Color::hex(0x1e2030),
            ws_active_bg: Color::hex(0xb7bdf8),
        }
    }
}
