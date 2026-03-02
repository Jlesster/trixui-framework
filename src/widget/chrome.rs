//! chrome — High-level compositor chrome drawing.
//!
//! This module is the **backing implementation** for the ergonomic surface on
//! [`Frame`](crate::app::Frame):
//!
//! ```rust,ignore
//! fn view(&self, frame: &mut Frame) {
//!     // Draw a pane border + title — all metrics implicit from frame
//!     frame.draw_pane(pane_rect, PaneOpts::new("nvim").focused(true));
//!
//!     // Build a status bar fluently
//!     frame.bar(frame.bar_area())
//!         .left(|b| b
//!             .workspace_state(1, true,  false)
//!             .workspace_state(2, false, true)
//!             .workspace_state(3, false, false))
//!         .center(|b| b.layout("BSP"))
//!         .right(|b| b.clock("14:32"))
//!         .finish();
//! }
//! ```
//!
//! The free functions (`draw_pane`, `draw_bar`) are also public for callers
//! that already have a raw [`PixelCanvas`].

use crate::layout::Rect;
use crate::renderer::{BorderSide, Color, CornerRadius, PixelCanvas, TextStyle, Theme};
use crate::widget::{bar_text_y, truncate_chars};

// ═══════════════════════════════════════════════════════════════════════════════
// PaneOpts
// ═══════════════════════════════════════════════════════════════════════════════

/// Options for drawing a pane decoration — border + title.
///
/// Created via [`PaneOpts::new`].  Colour fields default to `TRANSPARENT`,
/// which causes [`draw_pane`] to fall back to the supplied [`Theme`].
///
/// ```rust,ignore
/// // Colours from theme
/// frame.draw_pane(rect, PaneOpts::new("nvim").focused(true));
///
/// // With icon + rounded corners + explicit colour override
/// frame.draw_pane(rect,
///     PaneOpts::new("term")
///         .icon("󰖟 ")
///         .focused(true)
///         .border_w(2)
///         .corner_radius(6.0)
///         .active_border(Color::hex(0xcba6f7)));
/// ```
#[derive(Clone, Default)]
pub struct PaneOpts {
    pub title: String,
    pub icon: Option<String>,
    pub focused: bool,
    pub border_w: u32,
    pub corner_radius: f32,
    /// `TRANSPARENT` → `theme.active_border`.
    pub active_border: Color,
    /// `TRANSPARENT` → `theme.inactive_border`.
    pub inactive_border: Color,
    /// `TRANSPARENT` → `theme.pane_bg`.  Used to erase behind the title text.
    pub bg: Color,
}

impl PaneOpts {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            border_w: 1,
            ..Default::default()
        }
    }

    pub fn icon(mut self, s: impl Into<String>) -> Self {
        self.icon = Some(s.into());
        self
    }
    pub fn focused(mut self, f: bool) -> Self {
        self.focused = f;
        self
    }
    pub fn border_w(mut self, px: u32) -> Self {
        self.border_w = px;
        self
    }
    pub fn corner_radius(mut self, r: f32) -> Self {
        self.corner_radius = r;
        self
    }
    pub fn active_border(mut self, c: Color) -> Self {
        self.active_border = c;
        self
    }
    pub fn inactive_border(mut self, c: Color) -> Self {
        self.inactive_border = c;
        self
    }
    pub fn bg(mut self, c: Color) -> Self {
        self.bg = c;
        self
    }

    fn resolved_active(&self, t: &Theme) -> Color {
        if self.active_border.is_transparent() {
            t.active_border
        } else {
            self.active_border
        }
    }
    fn resolved_inactive(&self, t: &Theme) -> Color {
        if self.inactive_border.is_transparent() {
            t.inactive_border
        } else {
            self.inactive_border
        }
    }
    fn resolved_bg(&self, t: &Theme) -> Color {
        if self.bg.is_transparent() {
            t.pane_bg
        } else {
            self.bg
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// draw_pane
// ═══════════════════════════════════════════════════════════════════════════════

/// Draw a pane border (optionally rounded) + title in the top border strip.
///
/// Use [`Frame::draw_pane`] for the ergonomic path where `glyph_w / line_h /
/// theme` are supplied automatically.
pub fn draw_pane(
    canvas: &mut PixelCanvas,
    rect: Rect,
    opts: &PaneOpts,
    glyph_w: u32,
    line_h: u32,
    theme: &Theme,
) {
    let bw = opts.border_w.max(1).min(8);
    if rect.w < bw * 2 + 1 || rect.h < bw * 2 + 1 {
        return;
    }

    let border_col = if opts.focused {
        opts.resolved_active(theme)
    } else {
        opts.resolved_inactive(theme)
    };
    let bg = opts.resolved_bg(theme);

    // ── Border ────────────────────────────────────────────────────────────────
    if opts.corner_radius > 0.0 {
        canvas.round_stroke(
            rect.x as f32,
            rect.y as f32,
            rect.w as f32,
            rect.h as f32,
            CornerRadius::all(opts.corner_radius),
            border_col,
            bw as f32,
        );
    } else {
        canvas.border(
            rect.x,
            rect.y,
            rect.w,
            rect.h,
            BorderSide::ALL,
            border_col,
            bw,
        );
    }

    // ── Title ─────────────────────────────────────────────────────────────────
    if glyph_w > 0 && line_h > 0 && !opts.title.is_empty() {
        let prefix = opts.icon.as_deref().unwrap_or("");
        let raw = format!("{}{}", prefix, opts.title);
        let avail_w = rect.w.saturating_sub(glyph_w * 4);
        let max_ch = (avail_w / glyph_w.max(1)) as usize;
        let label = truncate_chars(&raw, max_ch);

        if !label.is_empty() {
            let tx = rect.x + glyph_w * 2;
            let title_w = label.chars().count() as u32 * glyph_w;
            // Vertically straddle the top border line
            let ty = rect.y.saturating_sub(line_h / 2 + bw / 2);
            let erase_h = (rect.y + bw).saturating_sub(ty);

            canvas.fill(tx, ty, title_w, erase_h, bg);
            let ts = TextStyle {
                fg: border_col,
                bg: Color::TRANSPARENT,
                bold: opts.focused,
                italic: false,
            };
            canvas.text_maxw(tx, ty, &label, ts, avail_w);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// BarItem
// ═══════════════════════════════════════════════════════════════════════════════

/// A single rendered item in a status bar section.
///
/// Usually created through [`SectionBuilder`] helpers on [`BarBuilder`].
/// Also constructable directly for custom layouts.
#[derive(Clone)]
pub struct BarItem {
    pub text: String,
    /// `TRANSPARENT` → use the bar's default foreground.
    pub fg: Color,
    /// `TRANSPARENT` → no fill, text on bar background.
    pub bg: Color,
    pub padding: u32,
    pub bold: bool,
    /// Draw a 1-px vertical separator line *before* this item.
    pub separator: bool,
    /// `TRANSPARENT` → use `theme.inactive_border`.
    pub sep_color: Color,
}

impl BarItem {
    /// Plain unstyled text.
    pub fn text(s: impl Into<String>) -> Self {
        Self {
            text: s.into(),
            fg: Color::TRANSPARENT,
            bg: Color::TRANSPARENT,
            padding: 0,
            bold: false,
            separator: false,
            sep_color: Color::TRANSPARENT,
        }
    }
    /// Bold text in `fg` colour.
    pub fn accent(s: impl Into<String>, fg: Color) -> Self {
        Self {
            text: s.into(),
            fg,
            bg: Color::TRANSPARENT,
            padding: 0,
            bold: true,
            separator: false,
            sep_color: Color::TRANSPARENT,
        }
    }
    /// Solid background pill.
    pub fn pill(s: impl Into<String>, fg: Color, bg: Color, padding: u32) -> Self {
        Self {
            text: s.into(),
            fg,
            bg,
            padding,
            bold: false,
            separator: false,
            sep_color: Color::TRANSPARENT,
        }
    }

    pub fn fg(mut self, c: Color) -> Self {
        self.fg = c;
        self
    }
    pub fn bg(mut self, c: Color) -> Self {
        self.bg = c;
        self
    }
    pub fn padding(mut self, px: u32) -> Self {
        self.padding = px;
        self
    }
    pub fn bold(mut self, b: bool) -> Self {
        self.bold = b;
        self
    }
    /// Prepend a 1-px separator before this item.
    pub fn sep(mut self) -> Self {
        self.separator = true;
        self
    }
    pub fn sep_color(mut self, c: Color) -> Self {
        self.sep_color = c;
        self
    }

    /// Pixel width this item occupies.
    pub fn width(&self, glyph_w: u32) -> u32 {
        self.text.chars().count() as u32 * glyph_w
            + self.padding * 2
            + if self.separator { 1 } else { 0 }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// SectionBuilder
// ═══════════════════════════════════════════════════════════════════════════════

/// Fluent helper for one horizontal zone of the bar (left / center / right).
///
/// Obtained via [`BarBuilder::left`], [`BarBuilder::center`], or [`BarBuilder::right`].
pub struct SectionBuilder<'a> {
    pub(crate) items: Vec<BarItem>,
    theme: &'a Theme,
    next_sep: bool,
}

impl<'a> SectionBuilder<'a> {
    pub(crate) fn new(theme: &'a Theme) -> Self {
        Self {
            items: Vec::new(),
            theme,
            next_sep: false,
        }
    }

    /// Push a pre-built [`BarItem`] directly.
    pub fn item(mut self, item: BarItem) -> Self {
        let item = if self.next_sep { item.sep() } else { item };
        self.next_sep = false;
        self.items.push(item);
        self
    }

    /// Force a separator before the *next* item added.
    pub fn separator(mut self) -> Self {
        self.next_sep = true;
        self
    }

    /// Plain text in `theme.bar_fg`.
    pub fn text(self, s: impl Into<String>) -> Self {
        let fg = self.theme.bar_fg;
        self.item(BarItem::text(s).fg(fg))
    }

    /// Bold text in an explicit foreground colour.
    pub fn accent(self, s: impl Into<String>, fg: Color) -> Self {
        self.item(BarItem::accent(s, fg))
    }

    /// Filled pill.
    pub fn pill(self, s: impl Into<String>, fg: Color, bg: Color, padding: u32) -> Self {
        self.item(BarItem::pill(s, fg, bg, padding))
    }

    /// Workspace number pill using theme colours.
    /// `active` → filled accent pill; `!active` → dim bare text.
    pub fn workspace(self, number: u8, active: bool) -> Self {
        self.workspace_state(number, active, false)
    }

    /// Workspace with explicit occupied flag.
    /// `active` → filled; `occupied` → accent-coloured bare text; else → dim.
    pub fn workspace_state(self, number: u8, active: bool, occupied: bool) -> Self {
        let t = self.theme;
        let label = format!(" {} ", number);
        let item = if active {
            BarItem::pill(label, t.ws_active_fg, t.ws_active_bg, 4).bold(true)
        } else if occupied {
            BarItem::text(label).fg(t.active_border).padding(4)
        } else {
            BarItem::text(label).fg(t.bar_dim).padding(4)
        };
        self.item(item)
    }

    /// Clock as a bold accent-filled pill (right-section default).
    pub fn clock(self, time: impl Into<String>) -> Self {
        let t = self.theme;
        let text = format!("  {} ", time.into());
        self.item(BarItem::pill(text, t.ws_active_fg, t.ws_active_bg, 0).bold(true))
    }

    /// Clock as plain dim text (no fill).
    pub fn clock_plain(self, time: impl Into<String>) -> Self {
        let fg = self.theme.bar_dim;
        let text = format!("  {} ", time.into());
        self.item(BarItem::text(text).fg(fg))
    }

    /// Layout mode label with Nerd Font icon.  `"BSP"` → `"󰙀 BSP"` in accent colour.
    pub fn layout(self, name: impl AsRef<str>) -> Self {
        let icon = match name.as_ref() {
            "BSP" => "󰙀 ",
            "Columns" => "󰕘 ",
            "Rows" => "󰕛 ",
            "ThreeCol" => "󱗼 ",
            "Monocle" => "󱕻 ",
            _ => "  ",
        };
        let text = format!("{}{}", icon, name.as_ref());
        let fg = self.theme.bar_accent;
        self.item(BarItem::accent(text, fg))
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// BarBuilder
// ═══════════════════════════════════════════════════════════════════════════════

/// Fluent status bar builder — obtained from [`Frame::bar`].
///
/// Fill the three layout zones with `.left()`, `.center()`, `.right()`,
/// then call `.finish()` to render everything in one pass.
///
/// ```rust,ignore
/// frame.bar(frame.bar_area())
///     .left(|b| b
///         .workspace_state(1, true,  false)
///         .workspace_state(2, false, true)
///         .separator()
///         .text("extra"))
///     .center(|b| b.layout("BSP"))
///     .right(|b| b.clock("14:32"))
///     .finish();
/// ```
pub struct BarBuilder<'frame> {
    canvas: &'frame mut PixelCanvas,
    rect: Rect,
    theme: &'frame Theme,
    glyph_w: u32,
    line_h: u32,
    /// `TRANSPARENT` → `theme.bar_bg`.
    bg: Color,
    /// `TRANSPARENT` → `theme.inactive_border`.
    sep_col: Color,
    sep_top: bool,
    left: Vec<BarItem>,
    center: Vec<BarItem>,
    right: Vec<BarItem>,
    natural_h: u32,
}

impl<'frame> BarBuilder<'frame> {
    /// Internal constructor — call via [`Frame::bar`].
    pub(crate) fn new(
        canvas: &'frame mut PixelCanvas,
        rect: Rect,
        theme: &'frame Theme,
        glyph_w: u32,
        line_h: u32,
        natural_h: u32,
    ) -> Self {
        Self {
            canvas,
            rect,
            theme,
            glyph_w,
            line_h,
            bg: Color::TRANSPARENT,
            sep_col: Color::TRANSPARENT,
            sep_top: true,
            left: Vec::new(),
            center: Vec::new(),
            right: Vec::new(),
            natural_h,
        }
    }

    /// Override bar background colour.
    pub fn bg(mut self, c: Color) -> Self {
        self.bg = c;
        self
    }
    /// Override separator line colour.
    pub fn separator_color(mut self, c: Color) -> Self {
        self.sep_col = c;
        self
    }
    /// Draw the 1-px separator on the **bottom** edge (default: top).
    pub fn separator_bottom(mut self) -> Self {
        self.sep_top = false;
        self
    }

    /// Fill the left zone.
    pub fn left(
        mut self,
        f: impl FnOnce(SectionBuilder<'frame>) -> SectionBuilder<'frame>,
    ) -> Self {
        self.left = f(SectionBuilder::new(self.theme)).items;
        self
    }

    /// Fill the centre zone.
    pub fn center(
        mut self,
        f: impl FnOnce(SectionBuilder<'frame>) -> SectionBuilder<'frame>,
    ) -> Self {
        self.center = f(SectionBuilder::new(self.theme)).items;
        self
    }

    /// Fill the right zone.
    pub fn right(
        mut self,
        f: impl FnOnce(SectionBuilder<'frame>) -> SectionBuilder<'frame>,
    ) -> Self {
        self.right = f(SectionBuilder::new(self.theme)).items;
        self
    }

    /// Flush all items to the canvas. **Must be called explicitly.**
    pub fn finish(self) {
        draw_bar(
            self.canvas,
            self.rect,
            self.glyph_w,
            self.line_h,
            self.theme,
            self.natural_h,
            self.bg,
            self.sep_col,
            self.sep_top,
            &self.left,
            &self.center,
            &self.right,
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// draw_bar — free function for raw PixelCanvas callers
// ═══════════════════════════════════════════════════════════════════════════════

/// Draw a three-zone status bar from pre-built item slices.
///
/// For most callers the [`BarBuilder`] via [`Frame::bar`] is simpler.
/// This free function is for code that owns a [`PixelCanvas`] directly.
///
/// `bg_override` / `sep_override`: pass `Color::TRANSPARENT` to use theme defaults.
pub fn draw_bar(
    canvas: &mut PixelCanvas,
    rect: Rect,
    glyph_w: u32,
    line_h: u32,
    theme: &Theme,
    natural_h: u32,
    bg_override: Color,
    sep_override: Color,
    sep_on_top: bool,
    left: &[BarItem],
    center: &[BarItem],
    right: &[BarItem],
) {
    if rect.is_empty() {
        return;
    }

    let bg = if bg_override.is_transparent() {
        theme.bar_bg
    } else {
        bg_override
    };
    let sep_col = if sep_override.is_transparent() {
        theme.inactive_border
    } else {
        sep_override
    };
    let def_fg = theme.bar_fg;
    let def_sep = theme.inactive_border;

    canvas.fill(rect.x, rect.y, rect.w, rect.h, bg);
    if !sep_col.is_transparent() {
        let line_y = if sep_on_top {
            rect.y
        } else {
            rect.y + rect.h - 1
        };
        canvas.hline(rect.x, line_y, rect.w, sep_col);
    }

    let right_w: u32 = right.iter().map(|i| i.width(glyph_w)).sum();
    let center_w: u32 = center.iter().map(|i| i.width(glyph_w)).sum();

    let mut x = rect.x;
    for item in left {
        x = flush_item(canvas, item, rect, x, glyph_w, natural_h, def_fg, def_sep);
    }

    let mut x = rect.x + rect.w.saturating_sub(center_w) / 2;
    for item in center {
        x = flush_item(canvas, item, rect, x, glyph_w, natural_h, def_fg, def_sep);
    }

    let mut x = rect.x + rect.w.saturating_sub(right_w);
    for item in right {
        x = flush_item(canvas, item, rect, x, glyph_w, natural_h, def_fg, def_sep);
    }
}

/// Render one [`BarItem`]. Returns `x` after the item.
fn flush_item(
    canvas: &mut PixelCanvas,
    item: &BarItem,
    bar: Rect,
    mut x: u32,
    glyph_w: u32,
    natural_h: u32,
    def_fg: Color,
    def_sep: Color,
) -> u32 {
    if item.text.is_empty() {
        return x + item.padding * 2;
    }

    if item.separator {
        let sc = if item.sep_color.is_transparent() {
            def_sep
        } else {
            item.sep_color
        };
        canvas.vline(x, bar_text_y(bar, natural_h), natural_h, sc); // ← both uses
        x += 1;
    }

    let text_w = item.text.chars().count() as u32 * glyph_w;
    let item_w = text_w + item.padding * 2;
    if x + item_w > bar.x + bar.w {
        return x;
    }

    let fg = if item.fg.is_transparent() {
        def_fg
    } else {
        item.fg
    };
    let ty = bar_text_y(bar, natural_h);
    eprintln!(
        "flush_item: bar.y={} bar.h={} natural_h={} ty={}",
        bar.y, bar.h, natural_h, ty
    );

    if !item.bg.is_transparent() {
        canvas.fill(x, bar.y, item_w, bar.h, item.bg);
    }
    let ts = TextStyle {
        fg,
        bg: Color::TRANSPARENT,
        bold: item.bold,
        italic: false,
    };
    canvas.text_maxw(x + item.padding, ty, &item.text, ts, text_w + 2);

    x + item_w
}
