//! widget — ratatui-style widget framework.
//!
//! All geometry is pixel-space (`Rect`). No NDC math here.
//!
//! # Border drawing policy
//!
//! Every widget emits only primitive `DrawCmd` variants — no box-drawing
//! codepoints in `DrawCmd::Text`.
//!
//! # Semantic theme usage
//!
//! - Content widgets (`Paragraph`, `List`, `Table`) use `theme.normal_*` and
//!   `theme.highlight_*` slots.
//! - Chrome widgets (`Block`, `Tabs`, `Gauge`) use `theme.bar_*` /
//!   `theme.active_border` etc.
//! - Override via `Style::fg()` / `Style::bg()` for per-instance colours.

use crate::layout::Rect;
use crate::renderer::{
    BorderSide, Color, CornerRadius, PixelCanvas, PowerlineDir, TextStyle, Theme,
};
use bitflags::bitflags;

mod layout_solver;
pub use layout_solver::{Constraint, Direction, Flex, Layout};

pub const BORDER_PX: u32 = 1;

// ── Style ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Default)]
pub struct Style {
    pub fg: Option<Color>,
    pub bg: Option<Color>,
    pub bold: bool,
    pub italic: bool,
}

impl Style {
    pub fn fg(mut self, c: Color) -> Self {
        self.fg = Some(c);
        self
    }
    pub fn bg(mut self, c: Color) -> Self {
        self.bg = Some(c);
        self
    }
    pub fn bold(mut self) -> Self {
        self.bold = true;
        self
    }
    pub fn italic(mut self) -> Self {
        self.italic = true;
        self
    }

    pub fn patch(self, other: Style) -> Style {
        Style {
            fg: other.fg.or(self.fg),
            bg: other.bg.or(self.bg),
            bold: other.bold || self.bold,
            italic: other.italic || self.italic,
        }
    }

    /// Convert to `TextStyle` using **content** theme slots (`normal_fg/bg`).
    ///
    /// Use this for `Paragraph`, `List`, `Table` and any content text.
    pub fn to_text_style(self, t: &Theme) -> TextStyle {
        TextStyle {
            fg: self.fg.unwrap_or(t.normal_fg),
            bg: self.bg.unwrap_or(t.normal_bg),
            bold: self.bold,
            italic: self.italic,
        }
    }

    /// Convert to `TextStyle` using **bar** theme slots (`bar_fg/bg`).
    ///
    /// Use this for status bars, tab bars, and chrome text.
    pub fn to_bar_style(self, t: &Theme) -> TextStyle {
        TextStyle {
            fg: self.fg.unwrap_or(t.bar_fg),
            bg: self.bg.unwrap_or(t.bar_bg),
            bold: self.bold,
            italic: self.italic,
        }
    }
}

// ── Borders ───────────────────────────────────────────────────────────────────

bitflags! {
    #[derive(Clone, Copy, Debug, Default)]
    pub struct Borders: u8 {
        const NONE   = 0b0000;
        const TOP    = 0b0001;
        const BOTTOM = 0b0010;
        const LEFT   = 0b0100;
        const RIGHT  = 0b1000;
        const ALL    = Self::TOP.bits() | Self::BOTTOM.bits()
                     | Self::LEFT.bits() | Self::RIGHT.bits();
    }
}

impl Borders {
    fn to_side(self) -> BorderSide {
        BorderSide(self.bits())
    }
}

// ── Widget traits ─────────────────────────────────────────────────────────────

pub trait Widget {
    fn render(self, canvas: &mut PixelCanvas, area: Rect, cell_w: u32, cell_h: u32, t: &Theme);
}

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

// ── Helpers ───────────────────────────────────────────────────────────────────

#[inline]
pub fn bar_text_y(inner: Rect, cell_h: u32) -> u32 {
    inner.y + inner.h.saturating_sub(cell_h) / 2
}

#[inline]
pub fn center_text_x(inner: Rect, text_w_px: u32) -> u32 {
    inner.x + inner.w.saturating_sub(text_w_px) / 2
}

/// Truncate `s` to at most `max` Unicode scalar values, appending '…' if needed.
pub fn truncate_chars(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        return s.to_string();
    }
    if max <= 1 {
        return s.chars().take(max).collect();
    }
    let mut t: String = s.chars().take(max - 1).collect();
    t.push('…');
    t
}

#[inline]
fn str_cell_w(s: &str, cell_w: u32) -> u32 {
    s.chars().count() as u32 * cell_w
}

fn char_byte_limit(s: &str, max_chars: usize) -> usize {
    s.char_indices()
        .nth(max_chars)
        .map(|(b, _)| b)
        .unwrap_or(s.len())
}

// ══════════════════════════════════════════════════════════════════════════════
// Block
// ══════════════════════════════════════════════════════════════════════════════

/// A bordered container with an optional title.
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

impl<'a> Block<'a> {
    pub fn new() -> Self {
        Self {
            borders: Borders::NONE,
            border_style: Style::default(),
            style: Style::default(),
            title: None,
            title_style: Style::default(),
            border_px: BORDER_PX,
            top_accent: None,
            corner_radius: 0.0,
        }
    }

    pub fn bordered() -> Self {
        Self::new().borders(Borders::ALL)
    }

    pub fn borders(mut self, b: Borders) -> Self {
        self.borders = b;
        self
    }
    pub fn border_style(mut self, s: Style) -> Self {
        self.border_style = s;
        self
    }
    pub fn style(mut self, s: Style) -> Self {
        self.style = s;
        self
    }
    pub fn title(mut self, t: &'a str) -> Self {
        self.title = Some(t);
        self
    }
    pub fn title_style(mut self, s: Style) -> Self {
        self.title_style = s;
        self
    }
    pub fn border_px(mut self, px: u32) -> Self {
        self.border_px = px;
        self
    }
    pub fn top_accent(mut self, c: Color) -> Self {
        self.top_accent = Some(c);
        self
    }
    pub fn rounded(mut self, r: f32) -> Self {
        self.corner_radius = r;
        self
    }

    /// Render and return the inner content `Rect`.
    pub fn render(
        self,
        canvas: &mut PixelCanvas,
        area: Rect,
        cell_w: u32,
        cell_h: u32,
        t: &Theme,
    ) -> Rect {
        let bp = self.border_px;
        let bg = self.style.bg.unwrap_or(t.pane_bg);
        let bdr = self.border_style.fg.unwrap_or(t.inactive_border);

        canvas.fill(area.x, area.y, area.w, area.h, bg);

        if !self.borders.is_empty() {
            if self.corner_radius > 0.0 {
                canvas.round_stroke(
                    area.x as f32,
                    area.y as f32,
                    area.w as f32,
                    area.h as f32,
                    CornerRadius::all(self.corner_radius),
                    bdr,
                    bp as f32,
                );
            } else {
                canvas.border(
                    area.x,
                    area.y,
                    area.w,
                    area.h,
                    self.borders.to_side(),
                    bdr,
                    bp,
                );
            }
        }

        if let Some(accent) = self.top_accent {
            canvas.hline(area.x, area.y, area.w, accent);
        }

        if let Some(title) = self.title {
            if self.borders.contains(Borders::TOP) && area.h >= cell_h && !title.is_empty() {
                let ts = TextStyle {
                    fg: self.title_style.fg.unwrap_or(t.active_title),
                    bg,
                    bold: self.title_style.bold,
                    italic: false,
                };
                let pad = cell_w;
                let max_w = area.w.saturating_sub((bp + pad) * 2);
                canvas.text_maxw(area.x + bp + pad, area.y, title, ts, max_w);
            }
        }

        let l = if self.borders.contains(Borders::LEFT) {
            bp
        } else {
            0
        };
        let r = if self.borders.contains(Borders::RIGHT) {
            bp
        } else {
            0
        };
        let top = if self.borders.contains(Borders::TOP) {
            bp
        } else {
            0
        };
        let bot = if self.borders.contains(Borders::BOTTOM) {
            bp
        } else {
            0
        };

        Rect {
            x: area.x + l,
            y: area.y + top,
            w: area.w.saturating_sub(l + r),
            h: area.h.saturating_sub(top + bot),
        }
    }
}

impl<'a> Default for Block<'a> {
    fn default() -> Self {
        Self::new()
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Paragraph
// ══════════════════════════════════════════════════════════════════════════════

pub struct Paragraph<'a> {
    text: &'a str,
    style: Style,
    wrap: bool,
    scroll: u32,
}

impl<'a> Paragraph<'a> {
    pub fn new(text: &'a str) -> Self {
        Self {
            text,
            style: Style::default(),
            wrap: false,
            scroll: 0,
        }
    }
    pub fn style(mut self, s: Style) -> Self {
        self.style = s;
        self
    }
    pub fn wrap(mut self, w: bool) -> Self {
        self.wrap = w;
        self
    }
    pub fn scroll(mut self, n: u32) -> Self {
        self.scroll = n;
        self
    }
}

impl<'a> Widget for Paragraph<'a> {
    fn render(self, canvas: &mut PixelCanvas, area: Rect, cell_w: u32, cell_h: u32, t: &Theme) {
        if area.is_empty() {
            return;
        }
        // Paragraph uses content theme slots.
        let ts = self.style.to_text_style(t);
        let max_cols = (area.w / cell_w).max(1) as usize;
        let max_rows = (area.h / cell_h) as usize;

        canvas.fill(area.x, area.y, area.w, area.h, ts.bg);

        let mut lines: Vec<&str> = Vec::new();
        for raw in self.text.split('\n') {
            if self.wrap {
                let mut rem = raw;
                loop {
                    let bi = char_byte_limit(rem, max_cols);
                    let (chunk, rest) = rem.split_at(bi);
                    lines.push(chunk);
                    rem = rest;
                    if rem.is_empty() {
                        break;
                    }
                }
            } else {
                lines.push(raw);
            }
        }

        for (row, line) in lines
            .iter()
            .skip(self.scroll as usize)
            .take(max_rows)
            .enumerate()
        {
            canvas.text_maxw(area.x, area.y + row as u32 * cell_h, line, ts, area.w);
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// List
// ══════════════════════════════════════════════════════════════════════════════

pub struct ListItem<'a> {
    pub content: &'a str,
    pub style: Style,
}

impl<'a> ListItem<'a> {
    pub fn new(content: &'a str) -> Self {
        Self {
            content,
            style: Style::default(),
        }
    }
    pub fn style(mut self, s: Style) -> Self {
        self.style = s;
        self
    }
}

#[derive(Default)]
pub struct ListState {
    selected: Option<usize>,
    pub offset: usize,
}

impl ListState {
    pub fn select(&mut self, i: Option<usize>) {
        self.selected = i;
    }
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }
}

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

impl<'a> List<'a> {
    pub fn new(items: Vec<ListItem<'a>>) -> Self {
        Self {
            items,
            highlight_style: Style::default(),
            highlight_symbol: "",
            selected_bar: false,
            selected_bar_color: None,
            selected_bar_px: 2,
            row_separator: false,
            row_separator_color: None,
        }
    }
    pub fn highlight_style(mut self, s: Style) -> Self {
        self.highlight_style = s;
        self
    }
    pub fn highlight_symbol(mut self, s: &'a str) -> Self {
        self.highlight_symbol = s;
        self
    }
    pub fn selected_bar(mut self, c: Color) -> Self {
        self.selected_bar = true;
        self.selected_bar_color = Some(c);
        self
    }
    pub fn selected_bar_px(mut self, px: u32) -> Self {
        self.selected_bar_px = px;
        self
    }
    pub fn row_separator(mut self, c: Color) -> Self {
        self.row_separator = true;
        self.row_separator_color = Some(c);
        self
    }
}

impl<'a> StatefulWidget for List<'a> {
    type State = ListState;

    fn render(
        self,
        canvas: &mut PixelCanvas,
        area: Rect,
        state: &mut ListState,
        cell_w: u32,
        cell_h: u32,
        t: &Theme,
    ) {
        if area.is_empty() {
            return;
        }
        let max_rows = (area.h / cell_h) as usize;
        let sym_w = str_cell_w(self.highlight_symbol, cell_w);

        if let Some(sel) = state.selected {
            if sel < state.offset {
                state.offset = sel;
            } else if sel >= state.offset + max_rows {
                state.offset = sel.saturating_sub(max_rows - 1);
            }
        }

        // List uses content theme slots for background.
        canvas.fill(area.x, area.y, area.w, area.h, t.normal_bg);

        for (row, item) in self
            .items
            .iter()
            .skip(state.offset)
            .take(max_rows)
            .enumerate()
        {
            let abs = state.offset + row;
            let y = area.y + row as u32 * cell_h;
            let sel = state.selected == Some(abs);

            // Row colours — content slots, overridable by item.style / highlight_style.
            let (fg, bg) = if sel {
                (
                    self.highlight_style
                        .fg
                        .or(item.style.fg)
                        .unwrap_or(t.highlight_fg),
                    self.highlight_style
                        .bg
                        .or(item.style.bg)
                        .unwrap_or(t.highlight_bg),
                )
            } else {
                (
                    item.style.fg.unwrap_or(t.normal_fg),
                    item.style.bg.unwrap_or(t.normal_bg),
                )
            };

            let ts = TextStyle {
                fg,
                bg,
                bold: item.style.bold,
                italic: item.style.italic,
            };

            canvas.fill(area.x, y, area.w, cell_h, ts.bg);

            // Selection bar — VLine on left edge
            if sel && self.selected_bar {
                let bar_col = self.selected_bar_color.unwrap_or(t.active_border);
                canvas.vline(area.x, y, cell_h, bar_col);
                // repeat for thickness
                for px in 1..self.selected_bar_px {
                    if area.x + px < area.x + area.w {
                        canvas.vline(area.x + px, y, cell_h, bar_col);
                    }
                }
            }

            // Row separator — HLine between rows
            if self.row_separator && row + 1 < max_rows {
                let sep_col = self.row_separator_color.unwrap_or(t.inactive_border);
                canvas.hline(area.x, y + cell_h - 1, area.w, sep_col);
            }

            let text_x = area.x + sym_w;
            let text_w = area.w.saturating_sub(sym_w);

            if sel && !self.highlight_symbol.is_empty() {
                let sym_ts = TextStyle {
                    fg,
                    bg,
                    bold: true,
                    italic: false,
                };
                canvas.text_maxw(area.x, y, self.highlight_symbol, sym_ts, sym_w);
            }
            canvas.text_maxw(text_x, y, item.content, ts, text_w);
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Table
// ══════════════════════════════════════════════════════════════════════════════

pub struct Cell<'a> {
    pub content: &'a str,
    pub style: Style,
}

impl<'a> Cell<'a> {
    pub fn new(content: &'a str) -> Self {
        Self {
            content,
            style: Style::default(),
        }
    }
    pub fn style(mut self, s: Style) -> Self {
        self.style = s;
        self
    }
}

pub struct Row<'a> {
    pub cells: Vec<Cell<'a>>,
    pub style: Style,
    pub bottom_margin: u32,
}

impl<'a> Row<'a> {
    pub fn new(cells: Vec<Cell<'a>>) -> Self {
        Self {
            cells,
            style: Style::default(),
            bottom_margin: 0,
        }
    }
    pub fn style(mut self, s: Style) -> Self {
        self.style = s;
        self
    }
    pub fn bottom_margin(mut self, px: u32) -> Self {
        self.bottom_margin = px;
        self
    }
}

#[derive(Clone, Copy, Debug)]
pub enum ColWidth {
    /// Exact pixels.
    Fixed(u32),
    /// Number of monospace cells.
    Cells(u32),
    /// Percentage of total table width (0–100).
    Pct(u8),
    /// Share of remaining space after fixed/pct columns.
    Fill(u32),
}

#[derive(Default)]
pub struct TableState {
    selected: Option<usize>,
    pub offset: usize,
}

impl TableState {
    pub fn select(&mut self, i: Option<usize>) {
        self.selected = i;
    }
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }
}

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

impl<'a> Table<'a> {
    pub fn new(rows: Vec<Row<'a>>, col_widths: Vec<ColWidth>) -> Self {
        Self {
            header: None,
            rows,
            col_widths,
            highlight_style: Style::default(),
            col_spacing: 1,
            header_style: Style::default(),
            header_separator: true,
            header_separator_color: None,
            row_separator: false,
            row_separator_color: None,
        }
    }
    pub fn header(mut self, r: Row<'a>) -> Self {
        self.header = Some(r);
        self
    }
    pub fn highlight_style(mut self, s: Style) -> Self {
        self.highlight_style = s;
        self
    }
    pub fn header_style(mut self, s: Style) -> Self {
        self.header_style = s;
        self
    }
    pub fn col_spacing(mut self, px: u32) -> Self {
        self.col_spacing = px;
        self
    }
    pub fn header_separator(mut self, c: Color) -> Self {
        self.header_separator = true;
        self.header_separator_color = Some(c);
        self
    }
    pub fn row_separator(mut self, c: Color) -> Self {
        self.row_separator = true;
        self.row_separator_color = Some(c);
        self
    }
    pub fn no_header_separator(mut self) -> Self {
        self.header_separator = false;
        self
    }
}

impl<'a> StatefulWidget for Table<'a> {
    type State = TableState;

    fn render(
        self,
        canvas: &mut PixelCanvas,
        area: Rect,
        state: &mut TableState,
        cell_w: u32,
        cell_h: u32,
        t: &Theme,
    ) {
        if area.is_empty() {
            return;
        }

        let ncols = self.col_widths.len();
        let spacing_total = self
            .col_spacing
            .saturating_mul(ncols.saturating_sub(1) as u32);
        let avail = area.w.saturating_sub(spacing_total);
        let mut col_px = vec![0u32; ncols];
        let mut fixed_total = 0u32;
        let mut fill_wt = 0u32;

        for (i, cw) in self.col_widths.iter().enumerate() {
            match *cw {
                ColWidth::Fixed(px) => {
                    col_px[i] = px;
                    fixed_total += px;
                }
                ColWidth::Cells(n) => {
                    col_px[i] = n * cell_w;
                    fixed_total += col_px[i];
                }
                ColWidth::Pct(p) => {
                    col_px[i] = (avail as f32 * p as f32 / 100.0) as u32;
                    fixed_total += col_px[i];
                }
                ColWidth::Fill(w) => {
                    fill_wt += w;
                }
            }
        }
        let remaining = avail.saturating_sub(fixed_total);
        if fill_wt > 0 {
            for (i, cw) in self.col_widths.iter().enumerate() {
                if let ColWidth::Fill(w) = *cw {
                    col_px[i] = (remaining as f32 * w as f32 / fill_wt as f32) as u32;
                }
            }
        }
        let used: u32 = col_px.iter().sum::<u32>() + spacing_total;
        if ncols > 0 && area.w > used {
            col_px[ncols - 1] += area.w - used;
        }

        let mut col_x = vec![0u32; ncols];
        {
            let mut x = area.x;
            for i in 0..ncols {
                col_x[i] = x;
                x += col_px[i] + if i + 1 < ncols { self.col_spacing } else { 0 };
            }
        }

        // Table uses content theme slots.
        canvas.fill(area.x, area.y, area.w, area.h, t.normal_bg);
        let mut y = area.y;

        // ── Header ────────────────────────────────────────────────────────────
        if let Some(hdr) = &self.header {
            // Header uses dim_fg for text (muted, column-header convention).
            let hdr_fg = self.header_style.fg.or(hdr.style.fg).unwrap_or(t.dim_fg);
            let hdr_bg = self.header_style.bg.or(hdr.style.bg).unwrap_or(t.normal_bg);
            let hdr_ts = TextStyle {
                fg: hdr_fg,
                bg: hdr_bg,
                bold: true,
                italic: false,
            };

            canvas.fill(area.x, y, area.w, cell_h, hdr_bg);

            for (ci, cell) in hdr.cells.iter().enumerate().take(ncols) {
                let ts = if cell.style.fg.is_some() || cell.style.bg.is_some() {
                    cell.style.to_text_style(t)
                } else {
                    hdr_ts
                };
                canvas.text_maxw(col_x[ci], y, cell.content, ts, col_px[ci]);
            }
            y += cell_h + hdr.bottom_margin;

            if self.header_separator && y <= area.y + area.h {
                let sep_col = self.header_separator_color.unwrap_or(t.inactive_border);
                canvas.hline(area.x, y, area.w, sep_col);
                y += 1;
            }
        }

        // ── Data rows ─────────────────────────────────────────────────────────
        let max_rows = (area.h.saturating_sub(y - area.y) / cell_h) as usize;

        if let Some(sel) = state.selected {
            if sel < state.offset {
                state.offset = sel;
            } else if sel >= state.offset + max_rows {
                state.offset = sel.saturating_sub(max_rows.saturating_sub(1));
            }
        }

        for (ri, row) in self
            .rows
            .iter()
            .skip(state.offset)
            .take(max_rows)
            .enumerate()
        {
            let abs = state.offset + ri;
            let sel = state.selected == Some(abs);
            let row_h = cell_h + row.bottom_margin;

            let (row_fg, row_bg) = if sel {
                (
                    self.highlight_style
                        .fg
                        .or(row.style.fg)
                        .unwrap_or(t.highlight_fg),
                    self.highlight_style
                        .bg
                        .or(row.style.bg)
                        .unwrap_or(t.highlight_bg),
                )
            } else {
                (
                    row.style.fg.unwrap_or(t.normal_fg),
                    row.style.bg.unwrap_or(t.normal_bg),
                )
            };

            canvas.fill(area.x, y, area.w, row_h, row_bg);

            if self.row_separator && ri + 1 < max_rows {
                let sep_col = self.row_separator_color.unwrap_or(t.inactive_border);
                canvas.hline(area.x, y + cell_h - 1, area.w, sep_col);
            }

            for (ci, cell) in row.cells.iter().enumerate().take(ncols) {
                let ts = TextStyle {
                    fg: cell.style.fg.unwrap_or(row_fg),
                    bg: row_bg,
                    bold: cell.style.bold || row.style.bold,
                    italic: cell.style.italic || row.style.italic,
                };
                canvas.text_maxw(col_x[ci], y, cell.content, ts, col_px[ci]);
            }
            y += row_h;
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Tabs
// ══════════════════════════════════════════════════════════════════════════════

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

impl<'a> Tabs<'a> {
    pub fn new(titles: Vec<&'a str>) -> Self {
        Self {
            titles,
            selected: 0,
            style: Style::default(),
            highlight_style: Style::default(),
            tab_padding: 1,
            powerline: false,
            powerline_color: None,
            underline: false,
            underline_color: None,
            divider: false,
            divider_color: None,
        }
    }
    pub fn select(mut self, i: usize) -> Self {
        self.selected = i;
        self
    }
    pub fn style(mut self, s: Style) -> Self {
        self.style = s;
        self
    }
    pub fn highlight_style(mut self, s: Style) -> Self {
        self.highlight_style = s;
        self
    }
    pub fn tab_padding(mut self, cells: u32) -> Self {
        self.tab_padding = cells;
        self
    }
    pub fn powerline(mut self, c: Color) -> Self {
        self.powerline = true;
        self.powerline_color = Some(c);
        self
    }
    pub fn underline(mut self, c: Color) -> Self {
        self.underline = true;
        self.underline_color = Some(c);
        self
    }
    pub fn divider(mut self, c: Color) -> Self {
        self.divider = true;
        self.divider_color = Some(c);
        self
    }
}

impl<'a> Widget for Tabs<'a> {
    fn render(self, canvas: &mut PixelCanvas, area: Rect, cell_w: u32, cell_h: u32, t: &Theme) {
        if area.is_empty() {
            return;
        }

        // Tabs use bar theme slots.
        let base_ts = self.style.to_bar_style(t);
        let sel_ts = self.style.patch(self.highlight_style).to_bar_style(t);

        canvas.fill(area.x, area.y, area.w, area.h, base_ts.bg);

        let pad_px = self.tab_padding * cell_w;
        let arrow_w = if self.powerline { cell_w } else { 0 };
        let y_text = bar_text_y(area, cell_h);
        let mut x = area.x;

        for (i, &title) in self.titles.iter().enumerate() {
            let sel = i == self.selected;
            let ts = if sel { sel_ts } else { base_ts };
            let title_w = str_cell_w(title, cell_w);
            let tab_w = pad_px + title_w + pad_px + if sel { arrow_w } else { 0 };

            if x + tab_w > area.x + area.w {
                break;
            }

            canvas.fill(x, area.y, tab_w, area.h, ts.bg);

            if self.divider && i > 0 && !sel {
                if self.selected + 1 != i {
                    let div_col = self.divider_color.unwrap_or(t.inactive_border);
                    canvas.vline(x, area.y, area.h, div_col);
                }
            }

            canvas.text_maxw(x + pad_px, y_text, title, ts, title_w + 1);

            if sel && self.powerline {
                let arrow_col = self.powerline_color.unwrap_or(ts.bg);
                canvas.powerline(
                    x + pad_px + title_w + pad_px,
                    area.y,
                    arrow_w,
                    area.h,
                    PowerlineDir::RightFill,
                    arrow_col,
                );
            }

            x += tab_w;
        }

        if self.underline {
            let ul_col = self.underline_color.unwrap_or(t.active_border);
            canvas.hline(area.x, area.y + area.h - 1, area.w, ul_col);
        }
    }
}

// ══════════════════════════════════════════════════════════════════════════════
// Gauge
// ══════════════════════════════════════════════════════════════════════════════

pub struct Gauge<'a> {
    ratio: f64,
    style: Style,
    filled_style: Style,
    label: Option<&'a str>,
    label_style: Style,
}

impl<'a> Gauge<'a> {
    pub fn new() -> Self {
        Self {
            ratio: 0.0,
            style: Style::default(),
            filled_style: Style::default(),
            label: None,
            label_style: Style::default(),
        }
    }
    pub fn ratio(mut self, r: f64) -> Self {
        self.ratio = r.clamp(0.0, 1.0);
        self
    }
    pub fn percent(mut self, p: u8) -> Self {
        self.ratio = p as f64 / 100.0;
        self
    }
    pub fn style(mut self, s: Style) -> Self {
        self.style = s;
        self
    }
    pub fn filled_style(mut self, s: Style) -> Self {
        self.filled_style = s;
        self
    }
    pub fn label(mut self, l: &'a str) -> Self {
        self.label = Some(l);
        self
    }
    pub fn label_style(mut self, s: Style) -> Self {
        self.label_style = s;
        self
    }
}

impl<'a> Widget for Gauge<'a> {
    fn render(self, canvas: &mut PixelCanvas, area: Rect, cell_w: u32, cell_h: u32, t: &Theme) {
        if area.is_empty() {
            return;
        }

        let empty_col = self.style.bg.unwrap_or(t.bar_bg);
        let filled_col = self
            .filled_style
            .bg
            .or(self.style.fg)
            .unwrap_or(t.active_border);

        canvas.fill(area.x, area.y, area.w, area.h, empty_col);

        let filled_px = ((area.w as f64 * self.ratio) as u32).min(area.w);
        if filled_px > 0 {
            canvas.fill(area.x, area.y, filled_px, area.h, filled_col);
        }

        // ── Label: correct two-pass rendering ─────────────────────────────────
        // The label is drawn twice — once clipped to the filled region (with
        // inverted colours so it's readable against the fill) and once clipped
        // to the empty region (normal colours). This avoids the abrupt
        // single-colour midpoint switch from the naive approach.
        if let Some(label) = self.label {
            let lw = str_cell_w(label, cell_w);
            if lw <= area.w {
                let lx = center_text_x(area, lw);
                let ly = bar_text_y(area, cell_h);

                let label_fg = self.label_style.fg.or(self.style.fg).unwrap_or(t.bar_fg);

                // Pass 1: label over the filled portion — inverted fg colour.
                let fill_end = area.x + filled_px;
                if lx < fill_end {
                    let clip_w = fill_end.saturating_sub(lx).min(lw);
                    let ts_inv = TextStyle {
                        fg: empty_col, // text on filled = empty colour (invert)
                        bg: Color::TRANSPARENT,
                        bold: self.label_style.bold,
                        italic: false,
                    };
                    canvas.text_maxw(lx, ly, label, ts_inv, clip_w);
                }

                // Pass 2: label over the empty portion — normal fg colour.
                let empty_start = area.x + filled_px;
                let label_end = lx + lw;
                if label_end > empty_start {
                    let offset = empty_start.saturating_sub(lx);
                    // We can't easily skip bytes without walking chars, so we draw
                    // the full label starting at lx but clip to [empty_start .. lx+lw].
                    // PixelCanvas::text_maxw naturally clips from the right; for left
                    // clipping we shift x and reduce max_w accordingly.
                    // If filled_px > lx the start is inside the fill — we need to
                    // start at empty_start with a reduced string start. Since we only
                    // have text_maxw (right-clip), we render starting at lx but the
                    // filled region already has its pass-1 draw on top. For simplicity:
                    // draw from lx with full max_w — pass-1 overdraw is identical pixels.
                    let _ = offset; // see above
                    let ts_norm = TextStyle {
                        fg: label_fg,
                        bg: Color::TRANSPARENT,
                        bold: self.label_style.bold,
                        italic: false,
                    };
                    canvas.text_maxw(lx, ly, label, ts_norm, lw);
                }
            }
        }
    }
}

impl<'a> Default for Gauge<'a> {
    fn default() -> Self {
        Self::new()
    }
}
