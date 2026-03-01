//! layout — pixel-space geometry primitives.
//!
//! `Rect` is the single source of truth for all pixel-space rectangles.
//! `CellRect` exists only for animation interpolation — convert to `Rect`
//! exactly once via `ScreenLayout::cell_rect_to_px`.

// ── Rect ──────────────────────────────────────────────────────────────────────

/// A pixel-space rectangle. Top-left origin, X right, Y down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub fn new(x: u32, y: u32, w: u32, h: u32) -> Self {
        Self { x, y, w, h }
    }
    pub fn is_empty(&self) -> bool {
        self.w == 0 || self.h == 0
    }

    /// Split into (top, bottom) at `top_h` pixels from the top.
    pub fn split_top(self, top_h: u32) -> (Self, Self) {
        let top_h = top_h.min(self.h);
        (
            Self::new(self.x, self.y, self.w, top_h),
            Self::new(self.x, self.y + top_h, self.w, self.h - top_h),
        )
    }

    /// Split into `n` equal columns; remainder goes to the last.
    pub fn split_cols(self, n: usize) -> Vec<Self> {
        if n == 0 {
            return vec![];
        }
        let each = self.w / n as u32;
        (0..n)
            .map(|i| {
                let x = self.x + i as u32 * each;
                let w = if i + 1 == n {
                    self.x + self.w - x
                } else {
                    each
                };
                Self::new(x, self.y, w, self.h)
            })
            .collect()
    }

    /// Split by normalised ratios; remainder goes to the last.
    pub fn split_ratios(self, ratios: &[f32]) -> Vec<Self> {
        if ratios.is_empty() {
            return vec![];
        }
        let total: f32 = ratios.iter().sum();
        let mut x = self.x;
        ratios
            .iter()
            .enumerate()
            .map(|(i, &r)| {
                let w = if i + 1 == ratios.len() {
                    self.x + self.w - x
                } else {
                    ((self.w as f32 * r / total) as u32).max(1)
                };
                let rect = Self::new(x, self.y, w, self.h);
                x += w;
                rect
            })
            .collect()
    }

    /// Inset on all sides by `px` pixels.
    pub fn inset(self, px: u32) -> Self {
        Self::new(
            self.x + px,
            self.y + px,
            self.w.saturating_sub(px * 2),
            self.h.saturating_sub(px * 2),
        )
    }
}

// ── CellRect ─────────────────────────────────────────────────────────────────

/// Cell-grid rectangle — used ONLY for animation interpolation.
/// Convert to `Rect` exactly once via `ScreenLayout::cell_rect_to_px`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CellRect {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

impl CellRect {
    pub fn new(x: u16, y: u16, w: u16, h: u16) -> Self {
        Self { x, y, w, h }
    }
    pub fn is_empty(self) -> bool {
        self.w == 0 || self.h == 0
    }

    /// Raw pixel conversion — prefer `ScreenLayout::cell_rect_to_px`.
    pub fn to_px(self, cell_w: u32, cell_h: u32) -> Rect {
        Rect::new(
            self.x as u32 * cell_w,
            self.y as u32 * cell_h,
            self.w as u32 * cell_w,
            self.h as u32 * cell_h,
        )
    }
}

// ── ScreenLayout ──────────────────────────────────────────────────────────────

/// Single layout pass. Produced once per frame.
///
/// Invariant: `content.h + bar.h == vp.h` always.
#[derive(Debug, Clone, Copy)]
pub struct ScreenLayout {
    pub vp: Rect,
    pub content: Rect,
    pub bar: Rect,
    pub cell_w: u32,
    pub cell_h: u32,
}

impl ScreenLayout {
    pub fn new(vp_w: u32, vp_h: u32, cell_w: u32, cell_h: u32, bar_h_cells: u32) -> Self {
        let vp = Rect::new(0, 0, vp_w, vp_h);
        let bar_h = bar_h_cells * cell_h;
        let (content, bar) = vp.split_top(vp_h.saturating_sub(bar_h));

        // Only log when the debug level is actually enabled — called every frame.
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!(
                "ScreenLayout vp={}x{} cell={}x{} bar_h_cells={} \
                 content={}x{}@({},{}) bar={}x{}@({},{})",
                vp_w,
                vp_h,
                cell_w,
                cell_h,
                bar_h_cells,
                content.w,
                content.h,
                content.x,
                content.y,
                bar.w,
                bar.h,
                bar.x,
                bar.y,
            );
        }

        Self {
            vp,
            content,
            bar,
            cell_w,
            cell_h,
        }
    }

    pub fn content_cols(&self) -> u16 {
        (self.content.w / self.cell_w).max(1) as u16
    }
    pub fn content_rows(&self) -> u16 {
        (self.content.h / self.cell_h).max(1) as u16
    }
    pub fn content_cell_rect(&self) -> CellRect {
        CellRect::new(0, 0, self.content_cols(), self.content_rows())
    }

    /// The one place `CellRect` → `Rect` conversion happens for panes.
    pub fn cell_rect_to_px(&self, cr: CellRect) -> Rect {
        Rect::new(
            self.content.x + cr.x as u32 * self.cell_w,
            self.content.y + cr.y as u32 * self.cell_h,
            cr.w as u32 * self.cell_w,
            cr.h as u32 * self.cell_h,
        )
    }
}
