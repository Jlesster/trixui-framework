//! renderer — GPU primitives, immediate-mode canvas, and theme.
//!
//! All coordinates are pixel-space: top-left origin, X right, Y down.

pub mod gl;
pub use gl::ChromeRenderer;

use crate::layout::Rect;

// ── Color ─────────────────────────────────────────────────────────────────────

/// RGBA8 colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Color(pub u8, pub u8, pub u8, pub u8);

impl Color {
    pub const TRANSPARENT: Self = Self(0, 0, 0, 0);

    pub fn rgb(r: u8, g: u8, b: u8) -> Self { Self(r, g, b, 255) }
    pub fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self { Self(r, g, b, a) }

    /// Opaque colour from a 24-bit `0xRRGGBB` hex literal.
    pub fn hex(v: u32) -> Self {
        Self((v >> 16) as u8, (v >> 8) as u8, v as u8, 255)
    }

    /// Return this colour with a new alpha value.
    pub fn alpha(self, a: u8) -> Self { Self(self.0, self.1, self.2, a) }

    /// Lighten by mixing toward white by `factor` (0.0 = no change, 1.0 = white).
    pub fn lighten(self, factor: f32) -> Self {
        let f = factor.clamp(0.0, 1.0);
        Self(
            (self.0 as f32 + (255.0 - self.0 as f32) * f) as u8,
            (self.1 as f32 + (255.0 - self.1 as f32) * f) as u8,
            (self.2 as f32 + (255.0 - self.2 as f32) * f) as u8,
            self.3,
        )
    }

    /// Darken by mixing toward black by `factor` (0.0 = no change, 1.0 = black).
    pub fn darken(self, factor: f32) -> Self {
        let f = factor.clamp(0.0, 1.0);
        Self(
            (self.0 as f32 * (1.0 - f)) as u8,
            (self.1 as f32 * (1.0 - f)) as u8,
            (self.2 as f32 * (1.0 - f)) as u8,
            self.3,
        )
    }

    /// Alpha-blend `self` (foreground) over `bg`.
    pub fn blend_over(self, bg: Color) -> Color {
        let fa = self.3 as f32 / 255.0;
        let ba = bg.3 as f32 / 255.0;
        let out_a = fa + ba * (1.0 - fa);
        if out_a < 1e-6 {
            return Color::TRANSPARENT;
        }
        let r = (self.0 as f32 * fa + bg.0 as f32 * ba * (1.0 - fa)) / out_a;
        let g = (self.1 as f32 * fa + bg.1 as f32 * ba * (1.0 - fa)) / out_a;
        let b = (self.2 as f32 * fa + bg.2 as f32 * ba * (1.0 - fa)) / out_a;
        Color::rgba(r as u8, g as u8, b as u8, (out_a * 255.0) as u8)
    }

    pub fn is_transparent(self) -> bool { self.3 == 0 }

    pub(crate) fn to_f32(self) -> [f32; 4] {
        [
            self.0 as f32 / 255.0,
            self.1 as f32 / 255.0,
            self.2 as f32 / 255.0,
            self.3 as f32 / 255.0,
        ]
    }
}

impl From<(u8, u8, u8)> for Color {
    fn from((r, g, b): (u8, u8, u8)) -> Self { Color::rgb(r, g, b) }
}
impl From<(u8, u8, u8, u8)> for Color {
    fn from((r, g, b, a): (u8, u8, u8, u8)) -> Self { Color::rgba(r, g, b, a) }
}
impl From<u32> for Color {
    /// Same as `Color::hex` — top byte ignored, fully opaque.
    fn from(v: u32) -> Self { Color::hex(v) }
}

// ── TextStyle ─────────────────────────────────────────────────────────────────

/// Text rendering style for `DrawCmd::Text`.
#[derive(Debug, Clone, Copy)]
pub struct TextStyle {
    pub fg:     Color,
    pub bg:     Color,
    pub bold:   bool,
    pub italic: bool,
}

impl TextStyle {
    pub fn fg(color: Color) -> Self {
        Self { fg: color, bg: Color::TRANSPARENT, bold: false, italic: false }
    }
}

// ── BorderSide ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BorderSide(pub u8);

impl BorderSide {
    pub const NONE:   Self = Self(0b0000);
    pub const TOP:    Self = Self(0b0001);
    pub const BOTTOM: Self = Self(0b0010);
    pub const LEFT:   Self = Self(0b0100);
    pub const RIGHT:  Self = Self(0b1000);
    pub const ALL:    Self = Self(0b1111);

    pub fn contains(self, other: Self) -> bool { self.0 & other.0 == other.0 }
    pub fn or(self, other: Self) -> Self { Self(self.0 | other.0) }
}

// ── CornerRadius ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct CornerRadius {
    pub tl: f32, pub tr: f32, pub bl: f32, pub br: f32,
}

impl CornerRadius {
    pub fn all(r: f32) -> Self { Self { tl: r, tr: r, bl: r, br: r } }
    pub fn none() -> Self { Self::default() }
    pub fn top_left(mut self, r: f32) -> Self { self.tl = r; self }
    pub fn top_right(mut self, r: f32) -> Self { self.tr = r; self }
    pub fn bottom_left(mut self, r: f32) -> Self { self.bl = r; self }
    pub fn bottom_right(mut self, r: f32) -> Self { self.br = r; self }
    pub fn is_none(self) -> bool {
        self.tl == 0.0 && self.tr == 0.0 && self.bl == 0.0 && self.br == 0.0
    }
}

// ── PowerlineDir ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PowerlineDir {
    RightFill   = 0,
    LeftFill    = 1,
    RightChevron = 2,
    LeftChevron  = 3,
}

impl PowerlineDir {
    pub(crate) fn as_f32(self) -> f32 { self as u8 as f32 }
    pub(crate) fn is_filled(self) -> bool {
        matches!(self, Self::RightFill | Self::LeftFill)
    }
}

// ── DrawCmd ───────────────────────────────────────────────────────────────────

/// A single GPU draw call. All coords are pixel-space, top-left origin.
#[derive(Debug, Clone)]
pub enum DrawCmd {
    FillRect   { x: u32, y: u32, w: u32, h: u32, color: Color },
    StrokeRect { x: u32, y: u32, w: u32, h: u32, color: Color },
    HLine      { x: u32, y: u32, w: u32, color: Color },
    VLine      { x: u32, y: u32, h: u32, color: Color },

    /// Per-side border lines.
    BorderLine {
        x: u32, y: u32, w: u32, h: u32,
        sides: BorderSide, color: Color, thickness: u32,
    },

    /// SDF rounded-rect.
    RoundRect {
        x: f32, y: f32, w: f32, h: f32,
        radii: CornerRadius,
        fill: Color, stroke: Color, stroke_w: f32,
    },

    /// Powerline glyph geometry.
    PowerlineArrow { x: u32, y: u32, w: u32, h: u32, dir: PowerlineDir, color: Color },

    /// Actual text only.
    Text { x: u32, y: u32, text: String, style: TextStyle, max_w: Option<u32> },
}

// ── PixelCanvas ───────────────────────────────────────────────────────────────

/// Immediate-mode draw list.
///
/// Draw calls pushed to `main` are emitted first; calls pushed to the overlay
/// (via `with_overlay()`) are appended afterwards, ensuring they render on top
/// of all main-layer content regardless of draw order in `view()`.
pub struct PixelCanvas {
    cmds:     Vec<DrawCmd>,
    overlay:  Vec<DrawCmd>,
    clip:     Option<Rect>,
    vp_w:     u32,
    vp_h:     u32,
}

impl PixelCanvas {
    pub fn new(vp_w: u32, vp_h: u32) -> Self {
        Self {
            cmds:    Vec::with_capacity(256),
            overlay: Vec::new(),
            clip:    None,
            vp_w,
            vp_h,
        }
    }

    pub fn set_clip(&mut self, clip: Option<Rect>) { self.clip = clip; }

    /// Write subsequent draw calls to the overlay layer.
    /// Overlay commands are sorted to the end of the draw list in `finish()`.
    pub fn begin_overlay(&mut self) {
        // Signals that subsequent push calls go to `overlay`. We track this via
        // a flag encoded as a special no-op sentinel.
        // Simpler: expose a mutable borrow of the overlay vec directly.
    }

    /// Push directly to the overlay (for Popup/Tooltip widgets).
    pub fn push_overlay(&mut self, cmd: DrawCmd) {
        self.overlay.push(cmd);
    }

    /// Finish and return the merged draw list (main + overlay).
    pub fn finish(mut self) -> Vec<DrawCmd> {
        self.cmds.extend(self.overlay);
        self.cmds
    }

    // ── Drawing primitives ────────────────────────────────────────────────────

    pub fn fill(&mut self, x: u32, y: u32, w: u32, h: u32, color: Color) {
        if color.is_transparent() || w == 0 || h == 0 { return; }
        self.cmds.push(DrawCmd::FillRect { x, y, w, h, color });
    }

    pub fn stroke(&mut self, x: u32, y: u32, w: u32, h: u32, color: Color) {
        if color.is_transparent() || w == 0 || h == 0 { return; }
        self.cmds.push(DrawCmd::StrokeRect { x, y, w, h, color });
    }

    pub fn hline(&mut self, x: u32, y: u32, w: u32, color: Color) {
        if color.is_transparent() || w == 0 { return; }
        self.cmds.push(DrawCmd::HLine { x, y, w, color });
    }

    pub fn vline(&mut self, x: u32, y: u32, h: u32, color: Color) {
        if color.is_transparent() || h == 0 { return; }
        self.cmds.push(DrawCmd::VLine { x, y, h, color });
    }

    pub fn border(
        &mut self, x: u32, y: u32, w: u32, h: u32,
        sides: BorderSide, color: Color, thickness: u32,
    ) {
        if color.is_transparent() { return; }
        self.cmds.push(DrawCmd::BorderLine { x, y, w, h, sides, color, thickness });
    }

    pub fn round_rect(
        &mut self, x: f32, y: f32, w: f32, h: f32,
        radii: CornerRadius, fill: Color, stroke: Color, stroke_w: f32,
    ) {
        if w <= 0.0 || h <= 0.0 { return; }
        self.cmds.push(DrawCmd::RoundRect { x, y, w, h, radii, fill, stroke, stroke_w });
    }

    pub fn round_fill(&mut self, x: f32, y: f32, w: f32, h: f32, radii: CornerRadius, fill: Color) {
        self.round_rect(x, y, w, h, radii, fill, Color::TRANSPARENT, 0.0);
    }

    pub fn round_stroke(
        &mut self, x: f32, y: f32, w: f32, h: f32,
        radii: CornerRadius, stroke: Color, stroke_w: f32,
    ) {
        self.round_rect(x, y, w, h, radii, Color::TRANSPARENT, stroke, stroke_w);
    }

    pub fn powerline(&mut self, x: u32, y: u32, w: u32, h: u32, dir: PowerlineDir, color: Color) {
        self.cmds.push(DrawCmd::PowerlineArrow { x, y, w, h, dir, color });
    }

    pub fn text(&mut self, x: u32, y: u32, s: &str, style: TextStyle) {
        if s.is_empty() { return; }
        self.cmds.push(DrawCmd::Text {
            x, y, text: s.to_string(), style, max_w: None,
        });
    }

    pub fn text_maxw(&mut self, x: u32, y: u32, s: &str, style: TextStyle, max_w: u32) {
        if s.is_empty() || max_w == 0 { return; }
        self.cmds.push(DrawCmd::Text {
            x, y, text: s.to_string(), style, max_w: Some(max_w),
        });
    }

    pub fn vp_w(&self) -> u32 { self.vp_w }
    pub fn vp_h(&self) -> u32 { self.vp_h }
}

// ── ChildCanvas ───────────────────────────────────────────────────────────────

pub struct ChildCanvas<'a> {
    parent: &'a mut PixelCanvas,
    clip:   Rect,
}

impl<'a> ChildCanvas<'a> {
    fn clip_rect(&self, x: u32, y: u32, w: u32, h: u32) -> Option<(u32, u32, u32, u32)> {
        let cx1 = self.clip.x + self.clip.w;
        let cy1 = self.clip.y + self.clip.h;
        let x0 = x.max(self.clip.x);
        let y0 = y.max(self.clip.y);
        let x1 = (x + w).min(cx1);
        let y1 = (y + h).min(cy1);
        if x1 > x0 && y1 > y0 { Some((x0, y0, x1 - x0, y1 - y0)) } else { None }
    }
    fn in_clip(&self, x: u32, y: u32, w: u32, h: u32) -> bool {
        x + w > self.clip.x && y + h > self.clip.y
            && x < self.clip.x + self.clip.w && y < self.clip.y + self.clip.h
    }

    pub fn fill(&mut self, x: u32, y: u32, w: u32, h: u32, color: Color) {
        if let Some((x, y, w, h)) = self.clip_rect(x, y, w, h) { self.parent.fill(x, y, w, h, color); }
    }
    pub fn stroke(&mut self, x: u32, y: u32, w: u32, h: u32, color: Color) {
        if self.in_clip(x, y, w, h) { self.parent.stroke(x, y, w, h, color); }
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
    pub fn border(&mut self, x: u32, y: u32, w: u32, h: u32, sides: BorderSide, color: Color, thickness: u32) {
        if self.in_clip(x, y, w, h) { self.parent.border(x, y, w, h, sides, color, thickness); }
    }
    pub fn round_rect(&mut self, x: f32, y: f32, w: f32, h: f32, radii: CornerRadius, fill: Color, stroke: Color, stroke_w: f32) {
        let cx = self.clip.x as f32;
        let cy = self.clip.y as f32;
        if x < cx + self.clip.w as f32 && y < cy + self.clip.h as f32 && x + w > cx && y + h > cy {
            self.parent.round_rect(x, y, w, h, radii, fill, stroke, stroke_w);
        }
    }
    pub fn round_fill(&mut self, x: f32, y: f32, w: f32, h: f32, radii: CornerRadius, fill: Color) {
        self.round_rect(x, y, w, h, radii, fill, Color::TRANSPARENT, 0.0);
    }
    pub fn round_stroke(&mut self, x: f32, y: f32, w: f32, h: f32, radii: CornerRadius, stroke: Color, stroke_w: f32) {
        self.round_rect(x, y, w, h, radii, Color::TRANSPARENT, stroke, stroke_w);
    }
    pub fn powerline(&mut self, x: u32, y: u32, w: u32, h: u32, dir: PowerlineDir, color: Color) {
        self.parent.powerline(x, y, w, h, dir, color);
    }
    pub fn text(&mut self, x: u32, y: u32, s: &str, style: TextStyle) {
        if x >= self.clip.x + self.clip.w || y >= self.clip.y + self.clip.h { return; }
        let max_w = self.clip.x + self.clip.w - x;
        self.parent.text_maxw(x, y, s, style, max_w);
    }
    pub fn text_maxw(&mut self, x: u32, y: u32, s: &str, style: TextStyle, max_w: u32) {
        if x >= self.clip.x + self.clip.w || y >= self.clip.y + self.clip.h { return; }
        let capped = max_w.min(self.clip.x + self.clip.w - x);
        self.parent.text_maxw(x, y, s, style, capped);
    }

    pub fn x(&self) -> u32 { self.clip.x }
    pub fn y(&self) -> u32 { self.clip.y }
    pub fn width(&self) -> u32 { self.clip.w }
    pub fn height(&self) -> u32 { self.clip.h }
    pub fn rect(&self) -> Rect { self.clip }
}

// ── Theme ─────────────────────────────────────────────────────────────────────

/// Colour theme.
///
/// Defaults to Catppuccin Mocha.
#[derive(Clone, Copy, Debug)]
pub struct Theme {
    // ── content ───────────────────────────────────────────────────────────────
    pub normal_fg:    Color,
    pub normal_bg:    Color,
    pub highlight_fg: Color,
    pub highlight_bg: Color,
    pub dim_fg:       Color,

    // ── chrome ────────────────────────────────────────────────────────────────
    pub active_border:   Color,
    pub inactive_border: Color,
    pub active_title:    Color,
    pub inactive_title:  Color,
    pub pane_bg:         Color,

    // ── status bar ────────────────────────────────────────────────────────────
    pub bar_bg:     Color,
    pub bar_fg:     Color,
    pub bar_accent: Color,
    pub bar_dim:    Color,

    // ── workspace / tab ───────────────────────────────────────────────────────
    pub ws_active_fg: Color,
    pub ws_active_bg: Color,

    // ── semantic status slots ─────────────────────────────────────────────────
    /// Red — errors, destructive actions.
    pub error_fg:   Color,
    /// Yellow — warnings, caution.
    pub warning_fg: Color,
    /// Green — success, online, healthy.
    pub success_fg: Color,

    // ── widget-specific ───────────────────────────────────────────────────────
    /// Cursor colour for `TextInput`.
    pub cursor_color:   Color,
    /// Background for `TextInput` selection highlight.
    pub selection_bg:   Color,
    /// Tooltip background.
    pub tooltip_bg:     Color,
    /// Tooltip foreground text.
    pub tooltip_fg:     Color,
    /// Semi-transparent backdrop for `Popup` / modal overlays.
    pub modal_overlay:  Color,
}

impl Default for Theme {
    /// Catppuccin Mocha.
    fn default() -> Self {
        Self {
            normal_fg:    Color::hex(0xcdd6f4),
            normal_bg:    Color::hex(0x11111b),
            highlight_fg: Color::hex(0x11111b),
            highlight_bg: Color::hex(0xb4befe),
            dim_fg:       Color::hex(0x585b70),

            active_border:   Color::hex(0xb4befe),
            inactive_border: Color::hex(0x45475a),
            active_title:    Color::hex(0xb4befe),
            inactive_title:  Color::hex(0x585b70),
            pane_bg:         Color::hex(0x11111b),

            bar_bg:     Color::hex(0x181825),
            bar_fg:     Color::hex(0xa6adc8),
            bar_accent: Color::hex(0xb4befe),
            bar_dim:    Color::hex(0x585b70),

            ws_active_fg: Color::hex(0x11111b),
            ws_active_bg: Color::hex(0xb4befe),

            error_fg:   Color::hex(0xf38ba8),
            warning_fg: Color::hex(0xf9e2af),
            success_fg: Color::hex(0xa6e3a1),

            cursor_color:  Color::hex(0xb4befe),
            selection_bg:  Color::hex(0x313244),
            tooltip_bg:    Color::hex(0x313244),
            tooltip_fg:    Color::hex(0xcdd6f4),
            modal_overlay: Color::rgba(0, 0, 0, 160),
        }
    }
}

impl Theme {
    /// Catppuccin Latte (light).
    pub fn latte() -> Self {
        Self {
            normal_fg:    Color::hex(0x4c4f69),
            normal_bg:    Color::hex(0xeff1f5),
            highlight_fg: Color::hex(0xeff1f5),
            highlight_bg: Color::hex(0x7287fd),
            dim_fg:       Color::hex(0x8c8fa1),
            active_border:   Color::hex(0x7287fd),
            inactive_border: Color::hex(0xacb0be),
            active_title:    Color::hex(0x7287fd),
            inactive_title:  Color::hex(0x8c8fa1),
            pane_bg:         Color::hex(0xeff1f5),
            bar_bg:     Color::hex(0xe6e9ef),
            bar_fg:     Color::hex(0x4c4f69),
            bar_accent: Color::hex(0x7287fd),
            bar_dim:    Color::hex(0x8c8fa1),
            ws_active_fg: Color::hex(0xeff1f5),
            ws_active_bg: Color::hex(0x7287fd),
            error_fg:   Color::hex(0xd20f39),
            warning_fg: Color::hex(0xdf8e1d),
            success_fg: Color::hex(0x40a02b),
            cursor_color:  Color::hex(0x7287fd),
            selection_bg:  Color::hex(0xccd0da),
            tooltip_bg:    Color::hex(0xccd0da),
            tooltip_fg:    Color::hex(0x4c4f69),
            modal_overlay: Color::rgba(0, 0, 0, 100),
        }
    }

    /// Catppuccin Macchiato.
    pub fn macchiato() -> Self {
        Self {
            normal_fg:    Color::hex(0xcad3f5),
            normal_bg:    Color::hex(0x1e2030),
            highlight_fg: Color::hex(0x1e2030),
            highlight_bg: Color::hex(0xb7bdf8),
            dim_fg:       Color::hex(0x6e738d),
            active_border:   Color::hex(0xb7bdf8),
            inactive_border: Color::hex(0x494d64),
            active_title:    Color::hex(0xb7bdf8),
            inactive_title:  Color::hex(0x6e738d),
            pane_bg:         Color::hex(0x1e2030),
            bar_bg:     Color::hex(0x181926),
            bar_fg:     Color::hex(0xcad3f5),
            bar_accent: Color::hex(0xb7bdf8),
            bar_dim:    Color::hex(0x6e738d),
            ws_active_fg: Color::hex(0x1e2030),
            ws_active_bg: Color::hex(0xb7bdf8),
            error_fg:   Color::hex(0xed8796),
            warning_fg: Color::hex(0xeed49f),
            success_fg: Color::hex(0xa6da95),
            cursor_color:  Color::hex(0xb7bdf8),
            selection_bg:  Color::hex(0x2a2b3c),
            tooltip_bg:    Color::hex(0x2a2b3c),
            tooltip_fg:    Color::hex(0xcad3f5),
            modal_overlay: Color::rgba(0, 0, 0, 150),
        }
    }
}
