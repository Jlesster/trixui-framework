//! widget — ratatui-style widget framework.
//!
//! All geometry is pixel-space (`Rect`). No NDC math here.
//! Widgets call `PixelCanvas` methods directly — no intermediate repr.

use bitflags::bitflags;
use crate::layout::Rect;
use crate::renderer::{Color, PixelCanvas, TextStyle, Theme};

// ── Re-export layout solver so users do `use trixui::widget::Layout` ─────────
mod layout_solver;
pub use layout_solver::{Constraint, Direction, Flex, Layout};

// ── Border thickness ──────────────────────────────────────────────────────────

/// Default border inset in pixels (does not affect drawn line thickness — borders
/// are always 1px. This only changes how much inner rect is returned to callers).
pub const BORDER_PX: u32 = 1;

// ── Style ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Default)]
pub struct Style {
    pub fg:     Option<Color>,
    pub bg:     Option<Color>,
    pub bold:   bool,
    pub italic: bool,
}

impl Style {
    pub fn fg(mut self, c: Color)   -> Self { self.fg = Some(c); self }
    pub fn bg(mut self, c: Color)   -> Self { self.bg = Some(c); self }
    pub fn bold(mut self)           -> Self { self.bold   = true; self }
    pub fn italic(mut self)         -> Self { self.italic = true; self }

    pub fn patch(self, other: Style) -> Style {
        Style {
            fg:     other.fg.or(self.fg),
            bg:     other.bg.or(self.bg),
            bold:   other.bold   || self.bold,
            italic: other.italic || self.italic,
        }
    }

    pub fn to_text_style(self, t: &Theme) -> TextStyle {
        TextStyle {
            fg:     self.fg.unwrap_or(t.bar_fg),
            bg:     self.bg.unwrap_or(t.bar_bg),
            bold:   self.bold,
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

// ── Widget traits ─────────────────────────────────────────────────────────────

pub trait Widget {
    fn render(self, canvas: &mut PixelCanvas, area: Rect, cell_w: u32, cell_h: u32, t: &Theme);
}

pub trait StatefulWidget {
    type State;
    fn render(
        self, canvas: &mut PixelCanvas, area: Rect,
        state: &mut Self::State, cell_w: u32, cell_h: u32, t: &Theme,
    );
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Y pixel position for single-line text vertically centred in `inner`.
#[inline]
pub fn bar_text_y(inner: Rect, cell_h: u32) -> u32 {
    inner.y + inner.h.saturating_sub(cell_h) / 2
}

/// X pixel position for text of `text_w_px` width centred in `inner`.
#[inline]
pub fn center_text_x(inner: Rect, text_w_px: u32) -> u32 {
    inner.x + inner.w.saturating_sub(text_w_px) / 2
}

/// Truncate `s` to at most `max` Unicode scalar values, appending `…` if cut.
pub fn truncate_chars(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max { return s.to_string(); }
    if max <= 1 { return s.chars().take(max).collect(); }
    let mut t: String = s.chars().take(max - 1).collect();
    t.push('…');
    t
}

// ── Block ─────────────────────────────────────────────────────────────────────

pub struct Block<'a> {
    borders:      Borders,
    border_style: Style,
    style:        Style,
    title:        Option<&'a str>,
    title_style:  Style,
    border_px:    u32,
    top_accent:   Option<Color>,
}

impl<'a> Block<'a> {
    pub fn new() -> Self {
        Self {
            borders:      Borders::NONE,
            border_style: Style::default(),
            style:        Style::default(),
            title:        None,
            title_style:  Style::default(),
            border_px:    BORDER_PX,
            top_accent:   None,
        }
    }

    pub fn bordered() -> Self { Self::new().borders(Borders::ALL) }

    pub fn borders(mut self, b: Borders)      -> Self { self.borders      = b; self }
    pub fn border_style(mut self, s: Style)   -> Self { self.border_style = s; self }
    pub fn style(mut self, s: Style)          -> Self { self.style        = s; self }
    pub fn title(mut self, t: &'a str)        -> Self { self.title        = Some(t); self }
    pub fn title_style(mut self, s: Style)    -> Self { self.title_style  = s; self }
    pub fn border_px(mut self, px: u32)       -> Self { self.border_px    = px; self }
    /// 1px accent line at the top — used for bar/zone visual separation.
    pub fn top_accent(mut self, c: Color)     -> Self { self.top_accent   = Some(c); self }

    /// Render and return the inner content rect.
    pub fn render(
        self, canvas: &mut PixelCanvas, area: Rect, cell_w: u32, cell_h: u32, t: &Theme,
    ) -> Rect {
        let bp         = self.border_px;
        let bg         = self.style.bg.unwrap_or(t.pane_bg);
        let border_col = self.border_style.fg.unwrap_or(t.inactive_border);

        canvas.fill(area.x, area.y, area.w, area.h, bg);

        // Borders are always 1px — ratatui-style crisp thin lines.
        // border_px only controls the inner rect inset.
        if self.borders.contains(Borders::TOP) {
            canvas.fill(area.x, area.y, area.w, 1, border_col);
        }
        if self.borders.contains(Borders::BOTTOM) {
            canvas.fill(area.x, area.y + area.h.saturating_sub(1), area.w, 1, border_col);
        }
        if self.borders.contains(Borders::LEFT) {
            canvas.fill(area.x, area.y, 1, area.h, border_col);
        }
        if self.borders.contains(Borders::RIGHT) {
            canvas.fill(area.x + area.w.saturating_sub(1), area.y, 1, area.h, border_col);
        }

        // Accent line over the top border
        if let Some(accent) = self.top_accent {
            canvas.fill(area.x, area.y, area.w, 1, accent);
        }

        // Title on the top border line
        if let Some(title) = self.title {
            if self.borders.contains(Borders::TOP) && area.h >= cell_h && !title.is_empty() {
                let ts = TextStyle {
                    fg:     self.title_style.fg.unwrap_or(t.active_title),
                    bg,
                    bold:   self.title_style.bold,
                    italic: false,
                };
                canvas.text_maxw(
                    area.x + bp + cell_w, area.y, title, ts,
                    area.w.saturating_sub(bp + cell_w * 2),
                );
            }
        }

        let left   = if self.borders.contains(Borders::LEFT)   { bp } else { 0 };
        let right  = if self.borders.contains(Borders::RIGHT)  { bp } else { 0 };
        let top    = if self.borders.contains(Borders::TOP)    { bp } else { 0 };
        let bottom = if self.borders.contains(Borders::BOTTOM) { bp } else { 0 };

        Rect {
            x: area.x + left,
            y: area.y + top,
            w: area.w.saturating_sub(left + right),
            h: area.h.saturating_sub(top + bottom),
        }
    }
}

impl<'a> Default for Block<'a> {
    fn default() -> Self { Self::new() }
}

// ── Paragraph ─────────────────────────────────────────────────────────────────

pub struct Paragraph<'a> {
    text:   &'a str,
    style:  Style,
    wrap:   bool,
    scroll: u32,
}

impl<'a> Paragraph<'a> {
    pub fn new(text: &'a str) -> Self {
        Self { text, style: Style::default(), wrap: false, scroll: 0 }
    }
    pub fn style(mut self, s: Style)    -> Self { self.style  = s; self }
    pub fn wrap(mut self, w: bool)      -> Self { self.wrap   = w; self }
    pub fn scroll(mut self, n: u32)     -> Self { self.scroll = n; self }
}

impl<'a> Widget for Paragraph<'a> {
    fn render(self, canvas: &mut PixelCanvas, area: Rect, cell_w: u32, cell_h: u32, t: &Theme) {
        if area.is_empty() { return; }
        let ts       = self.style.to_text_style(t);
        let max_cols = area.w / cell_w;
        let max_rows = area.h / cell_h;
        canvas.fill(area.x, area.y, area.w, area.h, ts.bg);

        let mut lines: Vec<&str> = Vec::new();
        for raw in self.text.split('\n') {
            if self.wrap && max_cols > 0 {
                let mut rem = raw;
                loop {
                    let bi = char_byte_limit(rem, max_cols as usize);
                    let (chunk, rest) = rem.split_at(bi);
                    lines.push(chunk);
                    rem = rest;
                    if rem.is_empty() { break; }
                }
            } else {
                lines.push(raw);
            }
        }
        for (row, line) in lines.iter().skip(self.scroll as usize).take(max_rows as usize).enumerate() {
            canvas.text_maxw(area.x, area.y + row as u32 * cell_h, line, ts, area.w);
        }
    }
}

// ── List ──────────────────────────────────────────────────────────────────────

pub struct ListItem<'a> {
    pub content: &'a str,
    pub style:   Style,
}
impl<'a> ListItem<'a> {
    pub fn new(content: &'a str) -> Self { Self { content, style: Style::default() } }
    pub fn style(mut self, s: Style) -> Self { self.style = s; self }
}

#[derive(Default)]
pub struct ListState { selected: Option<usize>, pub offset: usize }
impl ListState {
    pub fn select(&mut self, i: Option<usize>) { self.selected = i; }
    pub fn selected(&self) -> Option<usize>    { self.selected }
}

pub struct List<'a> {
    items:            Vec<ListItem<'a>>,
    highlight_style:  Style,
    highlight_symbol: &'a str,
}
impl<'a> List<'a> {
    pub fn new(items: Vec<ListItem<'a>>) -> Self {
        Self { items, highlight_style: Style::default(), highlight_symbol: "" }
    }
    pub fn highlight_style(mut self, s: Style)   -> Self { self.highlight_style  = s; self }
    pub fn highlight_symbol(mut self, s: &'a str)-> Self { self.highlight_symbol = s; self }
}

impl<'a> StatefulWidget for List<'a> {
    type State = ListState;
    fn render(self, canvas: &mut PixelCanvas, area: Rect, state: &mut ListState, cell_w: u32, cell_h: u32, t: &Theme) {
        if area.is_empty() { return; }
        let max_rows = (area.h / cell_h) as usize;
        let sym_w    = self.highlight_symbol.chars().count() as u32 * cell_w;

        if let Some(sel) = state.selected {
            if sel < state.offset { state.offset = sel; }
            else if sel >= state.offset + max_rows {
                state.offset = sel.saturating_sub(max_rows - 1);
            }
        }
        canvas.fill(area.x, area.y, area.w, area.h, t.bar_bg);

        for (row, item) in self.items.iter().skip(state.offset).take(max_rows).enumerate() {
            let abs = state.offset + row;
            let y   = area.y + row as u32 * cell_h;
            let sel = state.selected == Some(abs);
            let base = item.style.patch(Style::default().fg(t.bar_fg).bg(t.bar_bg));
            let rs   = if sel { base.patch(self.highlight_style) } else { base };
            let ts   = rs.to_text_style(t);
            canvas.fill(area.x, y, area.w, cell_h, ts.bg);
            if sel && !self.highlight_symbol.is_empty() {
                canvas.text(area.x, y, self.highlight_symbol, ts);
                canvas.text_maxw(area.x + sym_w, y, item.content, ts, area.w.saturating_sub(sym_w));
            } else {
                let xo = if !self.highlight_symbol.is_empty() { sym_w } else { 0 };
                canvas.text_maxw(area.x + xo, y, item.content, ts, area.w.saturating_sub(xo));
            }
        }
    }
}

// ── Table ─────────────────────────────────────────────────────────────────────

pub struct Cell<'a> { pub content: &'a str, pub style: Style }
impl<'a> Cell<'a> {
    pub fn new(content: &'a str) -> Self { Self { content, style: Style::default() } }
    pub fn style(mut self, s: Style) -> Self { self.style = s; self }
}

pub struct Row<'a> { pub cells: Vec<Cell<'a>>, pub style: Style }
impl<'a> Row<'a> {
    pub fn new(cells: Vec<Cell<'a>>) -> Self { Self { cells, style: Style::default() } }
    pub fn style(mut self, s: Style) -> Self { self.style = s; self }
}

#[derive(Default)]
pub struct TableState { selected: Option<usize>, pub offset: usize }
impl TableState {
    pub fn select(&mut self, i: Option<usize>) { self.selected = i; }
    pub fn selected(&self) -> Option<usize>    { self.selected }
}

pub struct Table<'a> {
    rows:            Vec<Row<'a>>,
    widths:          Vec<Constraint>,
    header:          Option<Row<'a>>,
    highlight_style: Style,
    column_spacing:  u32,
}
impl<'a> Table<'a> {
    pub fn new(rows: Vec<Row<'a>>, widths: Vec<Constraint>) -> Self {
        Self { rows, widths, header: None, highlight_style: Style::default(), column_spacing: 1 }
    }
    pub fn header(mut self, r: Row<'a>)        -> Self { self.header          = Some(r); self }
    pub fn highlight_style(mut self, s: Style) -> Self { self.highlight_style = s; self }
    pub fn column_spacing(mut self, c: u32)    -> Self { self.column_spacing  = c; self }
}

impl<'a> StatefulWidget for Table<'a> {
    type State = TableState;
    fn render(self, canvas: &mut PixelCanvas, area: Rect, state: &mut TableState, cell_w: u32, cell_h: u32, t: &Theme) {
        if area.is_empty() { return; }
        canvas.fill(area.x, area.y, area.w, area.h, t.bar_bg);
        let col_rects = Layout::horizontal(self.widths.clone())
            .spacing(self.column_spacing * cell_w)
            .split(area, cell_w, cell_h);

        let mut cy        = area.y;
        let max_rows      = (area.h / cell_h) as usize;
        let header_rows   = if self.header.is_some() { 1 } else { 0 };

        if let Some(ref hdr) = self.header {
            if cy + cell_h <= area.y + area.h {
                let hs = hdr.style.patch(Style::default().fg(t.bar_accent).bg(t.bar_bg)).to_text_style(t);
                canvas.fill(area.x, cy, area.w, cell_h, hs.bg);
                for (ci, cell) in hdr.cells.iter().enumerate() {
                    if ci >= col_rects.len() { break; }
                    let cr = col_rects[ci];
                    let cs = cell.style.patch(hdr.style).patch(Style::default().fg(t.bar_accent)).to_text_style(t);
                    canvas.text_maxw(cr.x, cy, cell.content, cs, cr.w);
                }
                cy += cell_h;
            }
        }

        let vis = max_rows.saturating_sub(header_rows);
        if let Some(sel) = state.selected {
            if sel < state.offset { state.offset = sel; }
            else if sel >= state.offset + vis { state.offset = sel.saturating_sub(vis.saturating_sub(1)); }
        }

        for (ri, row) in self.rows.iter().skip(state.offset).take(vis).enumerate() {
            if cy + cell_h > area.y + area.h { break; }
            let sel = state.selected == Some(state.offset + ri);
            let rs  = if sel { row.style.patch(self.highlight_style) }
                      else   { row.style.patch(Style::default().fg(t.bar_fg).bg(t.bar_bg)) };
            canvas.fill(area.x, cy, area.w, cell_h, rs.to_text_style(t).bg);
            for (ci, cell) in row.cells.iter().enumerate() {
                if ci >= col_rects.len() { break; }
                let cr = col_rects[ci];
                canvas.text_maxw(cr.x, cy, cell.content, cell.style.patch(rs).to_text_style(t), cr.w);
            }
            cy += cell_h;
        }
    }
}

// ── Tabs ──────────────────────────────────────────────────────────────────────

pub struct Tabs<'a> {
    titles:          Vec<&'a str>,
    selected:        usize,
    style:           Style,
    highlight_style: Style,
    divider:         &'a str,
}
impl<'a> Tabs<'a> {
    pub fn new(titles: Vec<&'a str>) -> Self {
        Self { titles, selected: 0, style: Style::default(),
               highlight_style: Style::default(), divider: "│" }
    }
    pub fn select(mut self, i: usize)           -> Self { self.selected        = i; self }
    pub fn style(mut self, s: Style)            -> Self { self.style           = s; self }
    pub fn highlight_style(mut self, s: Style)  -> Self { self.highlight_style = s; self }
    pub fn divider(mut self, d: &'a str)        -> Self { self.divider         = d; self }
}

impl<'a> Widget for Tabs<'a> {
    fn render(self, canvas: &mut PixelCanvas, area: Rect, cell_w: u32, cell_h: u32, t: &Theme) {
        if area.is_empty() { return; }
        let bg = self.style.bg.unwrap_or(t.bar_bg);
        canvas.fill(area.x, area.y, area.w, area.h, bg);

        let underline_h = 2u32;
        let text_y      = bar_text_y(area, cell_h);
        let underline_y = area.y + area.h.saturating_sub(underline_h);
        let div_w       = self.divider.chars().count() as u32 * cell_w;
        let mut x       = area.x + cell_w; // one cell left margin

        for (i, title) in self.titles.iter().enumerate() {
            if x >= area.x + area.w { break; }
            let pad     = cell_w;
            let label_w = title.chars().count() as u32 * cell_w;
            let tab_w   = pad + label_w + pad;
            if tab_w > (area.x + area.w).saturating_sub(x) { break; }

            if i == self.selected {
                let hi    = self.style.patch(self.highlight_style);
                let hi_bg = hi.bg.unwrap_or(t.ws_active_bg);
                let hi_fg = hi.fg.unwrap_or(t.ws_active_fg);
                canvas.fill(x, area.y, tab_w, area.h, hi_bg);
                canvas.fill(x, underline_y, tab_w, underline_h, t.active_border);
                canvas.text_maxw(x + pad, text_y, title,
                    TextStyle { fg: hi_fg, bg: hi_bg, bold: hi.bold, italic: hi.italic },
                    label_w);
            } else {
                canvas.text_maxw(x + pad, text_y, title,
                    TextStyle { fg: self.style.fg.unwrap_or(t.bar_dim), bg, bold: false, italic: false },
                    label_w);
            }
            x += tab_w;

            if i + 1 < self.titles.len() {
                canvas.text(x, text_y, self.divider,
                    TextStyle { fg: self.style.fg.unwrap_or(t.bar_dim), bg, bold: false, italic: false });
                x += div_w;
            }
        }
    }
}

// ── Gauge ─────────────────────────────────────────────────────────────────────

pub struct Gauge<'a> {
    ratio:       f32,
    label:       Option<&'a str>,
    gauge_style: Style,
    style:       Style,
}
impl<'a> Gauge<'a> {
    pub fn new() -> Self {
        Self { ratio: 0.0, label: None, gauge_style: Style::default(), style: Style::default() }
    }
    pub fn ratio(mut self, r: f32)          -> Self { self.ratio       = r.clamp(0.0, 1.0); self }
    pub fn label(mut self, l: &'a str)      -> Self { self.label       = Some(l); self }
    pub fn gauge_style(mut self, s: Style)  -> Self { self.gauge_style = s; self }
    pub fn style(mut self, s: Style)        -> Self { self.style       = s; self }
}
impl<'a> Default for Gauge<'a> { fn default() -> Self { Self::new() } }

impl<'a> Widget for Gauge<'a> {
    fn render(self, canvas: &mut PixelCanvas, area: Rect, cell_w: u32, cell_h: u32, t: &Theme) {
        if area.is_empty() { return; }
        let bg_col   = self.style.bg.unwrap_or(t.bar_dim);
        let fill_col = self.gauge_style.fg.unwrap_or(t.bar_accent);
        let fg_col   = self.gauge_style.bg.unwrap_or(t.bar_fg);
        let filled_w = (area.w as f32 * self.ratio) as u32;

        canvas.fill(area.x, area.y, area.w, area.h, bg_col);
        if filled_w > 0 { canvas.fill(area.x, area.y, filled_w, area.h, fill_col); }

        if let Some(label) = self.label {
            let lw = label.chars().count() as u32 * cell_w;
            canvas.text_maxw(
                center_text_x(area, lw), bar_text_y(area, cell_h), label,
                TextStyle { fg: fg_col, bg: fill_col, bold: self.gauge_style.bold, italic: false },
                area.w,
            );
        }
    }
}

// ── Internal ──────────────────────────────────────────────────────────────────

fn char_byte_limit(s: &str, max_chars: usize) -> usize {
    s.char_indices().nth(max_chars).map(|(i, _)| i).unwrap_or(s.len())
}
