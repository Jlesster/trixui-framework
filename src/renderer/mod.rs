//! renderer — GPU primitives, immediate-mode canvas, and theme.
//!
//! All coordinates are pixel-space: top-left origin, X right, Y down.
//! NDC conversion happens ONLY inside GLSL vertex shaders in `gl.rs`.
//! Never do NDC math in Rust.

pub mod gl;

pub use gl::ChromeRenderer;

// ── Color ─────────────────────────────────────────────────────────────────────

/// RGBA8 colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Color(pub u8, pub u8, pub u8, pub u8);

impl Color {
    /// Fully transparent.
    pub const TRANSPARENT: Self = Self(0, 0, 0, 0);

    /// Opaque RGB.
    pub fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self(r, g, b, 255)
    }

    /// Opaque RGB from a `0xRRGGBB` hex literal.
    pub fn hex(v: u32) -> Self {
        Self((v >> 16) as u8, (v >> 8) as u8, v as u8, 255)
    }

    /// With explicit alpha.
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

/// Text rendering style passed to `DrawCmd::Text`.
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

// ── DrawCmd ───────────────────────────────────────────────────────────────────

/// A single GPU draw operation. All coords are pixel-space, top-left origin.
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
    Text {
        x: u32,
        y: u32,
        text: String,
        style: TextStyle,
        max_w: Option<u32>,
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
}

// ── PixRect ───────────────────────────────────────────────────────────────────

/// A pixel-space rectangle. Imported from layout so everything shares one type.
pub use crate::layout::Rect;

// ── PixelCanvas ───────────────────────────────────────────────────────────────

/// Immediate-mode pixel canvas. Collects [`DrawCmd`]s for a single frame.
///
/// All drawing is in pixel space. Clip is enforced on `text` and `fill`;
/// use `child()` to scope a clip region.
pub struct PixelCanvas {
    cmds: Vec<DrawCmd>,
    vp_w: u32,
    vp_h: u32,
    clip: Option<Rect>,
}

impl PixelCanvas {
    pub fn new(vp_w: u32, vp_h: u32) -> Self {
        Self {
            cmds: Vec::with_capacity(512),
            vp_w,
            vp_h,
            clip: None,
        }
    }

    pub fn set_clip(&mut self, clip: Option<Rect>) {
        self.clip = clip;
    }

    /// Create a child canvas that clips all operations to `clip`.
    pub fn child(&mut self, clip: Rect) -> ChildCanvas<'_> {
        ChildCanvas { parent: self, clip }
    }

    /// Consume the canvas and return the collected draw commands.
    pub fn finish(self) -> Vec<DrawCmd> {
        self.cmds
    }

    pub fn fill(&mut self, x: u32, y: u32, w: u32, h: u32, color: Color) {
        if color.is_transparent() || w == 0 || h == 0 {
            return;
        }
        #[cfg(debug_assertions)]
        if x + w > self.vp_w || y + h > self.vp_h {
            tracing::warn!("fill OOB ({x},{y},{w},{h}) vp={}x{}", self.vp_w, self.vp_h);
        }
        if let Some(c) = self.clip {
            if x >= c.x + c.w || y >= c.y + c.h {
                return;
            }
        }
        self.cmds.push(DrawCmd::FillRect { x, y, w, h, color });
    }

    pub fn stroke(&mut self, x: u32, y: u32, w: u32, h: u32, color: Color) {
        if color.is_transparent() || w == 0 || h == 0 {
            return;
        }
        self.cmds.push(DrawCmd::StrokeRect { x, y, w, h, color });
    }

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

    pub fn hline(&mut self, x: u32, y: u32, w: u32, color: Color) {
        if w == 0 || color.is_transparent() {
            return;
        }
        self.cmds.push(DrawCmd::HLine { x, y, w, color });
    }

    pub fn vline(&mut self, x: u32, y: u32, h: u32, color: Color) {
        if h == 0 || color.is_transparent() {
            return;
        }
        self.cmds.push(DrawCmd::VLine { x, y, h, color });
    }
}

/// A clipped sub-view of a [`PixelCanvas`].
pub struct ChildCanvas<'a> {
    parent: &'a mut PixelCanvas,
    clip: Rect,
}

impl<'a> ChildCanvas<'a> {
    pub fn fill(&mut self, x: u32, y: u32, w: u32, h: u32, color: Color) {
        let x = x.max(self.clip.x);
        let y = y.max(self.clip.y);
        let x1 = (x + w).min(self.clip.x + self.clip.w);
        let y1 = (y + h).min(self.clip.y + self.clip.h);
        if x1 > x && y1 > y {
            self.parent.fill(x, y, x1 - x, y1 - y, color);
        }
    }
    pub fn stroke(&mut self, x: u32, y: u32, w: u32, h: u32, color: Color) {
        if x + w > self.clip.x
            && y + h > self.clip.y
            && x < self.clip.x + self.clip.w
            && y < self.clip.y + self.clip.h
        {
            self.parent.stroke(x, y, w, h, color);
        }
    }
    pub fn text(&mut self, x: u32, y: u32, s: &str, style: TextStyle) {
        if x >= self.clip.x + self.clip.w || y >= self.clip.y + self.clip.h {
            return;
        }
        let max_w = self.clip.x + self.clip.w - x;
        self.parent.text_maxw(x, y, s, style, max_w);
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
}

// ── Theme ─────────────────────────────────────────────────────────────────────

/// Colour theme. Defaults to Catppuccin Mocha.
#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub active_border: Color,
    pub inactive_border: Color,
    pub active_title: Color,
    pub inactive_title: Color,
    pub pane_bg: Color,
    pub bar_bg: Color,
    pub bar_fg: Color,
    pub bar_accent: Color,
    pub bar_dim: Color,
    pub ws_active_fg: Color,
    pub ws_active_bg: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            active_border: Color::hex(0xb4befe), // lavender
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
    /// Catppuccin Latte (light variant).
    pub fn latte() -> Self {
        Self {
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
