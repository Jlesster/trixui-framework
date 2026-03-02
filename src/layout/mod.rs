//! layout — pixel-space geometry primitives.

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

    #[inline]
    pub fn contains_point(self, px: u32, py: u32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }

    pub fn intersect(self, other: Self) -> Option<Self> {
        let x0 = self.x.max(other.x);
        let y0 = self.y.max(other.y);
        let x1 = (self.x + self.w).min(other.x + other.w);
        let y1 = (self.y + self.h).min(other.y + other.h);
        if x1 > x0 && y1 > y0 {
            Some(Self::new(x0, y0, x1 - x0, y1 - y0))
        } else {
            None
        }
    }

    pub fn union(self, other: Self) -> Self {
        let x0 = self.x.min(other.x);
        let y0 = self.y.min(other.y);
        let x1 = (self.x + self.w).max(other.x + other.w);
        let y1 = (self.y + self.h).max(other.y + other.h);
        Self::new(x0, y0, x1 - x0, y1 - y0)
    }

    pub fn inset(self, px: u32) -> Self {
        Self::new(
            self.x + px,
            self.y + px,
            self.w.saturating_sub(px * 2),
            self.h.saturating_sub(px * 2),
        )
    }

    pub fn pad(self, top: u32, right: u32, bottom: u32, left: u32) -> Self {
        Self::new(
            self.x + left,
            self.y + top,
            self.w.saturating_sub(left + right),
            self.h.saturating_sub(top + bottom),
        )
    }

    pub fn split_top(self, top_h: u32) -> (Self, Self) {
        let top_h = top_h.min(self.h);
        (
            Self::new(self.x, self.y, self.w, top_h),
            Self::new(self.x, self.y + top_h, self.w, self.h - top_h),
        )
    }

    pub fn split_left(self, left_w: u32) -> (Self, Self) {
        let left_w = left_w.min(self.w);
        (
            Self::new(self.x, self.y, left_w, self.h),
            Self::new(self.x + left_w, self.y, self.w - left_w, self.h),
        )
    }

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
}

// ── CellRect ─────────────────────────────────────────────────────────────────

/// Cell-grid rectangle — used ONLY for animation interpolation.
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

/// Single layout pass. Produced once per frame. Pure pixel geometry — no cell
/// concepts. The TUI widget layer uses cell_w/cell_h at the Widget::render call
/// site; ScreenLayout has no knowledge of font metrics.
///
/// Invariant: `content.h + bar.h == vp.h` always.
#[derive(Debug, Clone, Copy)]
pub struct ScreenLayout {
    pub vp: Rect,
    pub content: Rect,
    pub bar: Rect,
}

impl ScreenLayout {
    /// `bar_h_px` is the exact pixel height reserved for the status bar at the
    /// bottom. Pass `0` for no bar. No cell-quantisation is applied.
    pub fn new(vp_w: u32, vp_h: u32, bar_h_px: u32) -> Self {
        let vp = Rect::new(0, 0, vp_w, vp_h);
        let bar_h = bar_h_px.min(vp_h);
        let (content, bar) = vp.split_top(vp_h.saturating_sub(bar_h));

        tracing::debug!(
            "ScreenLayout vp={}x{} bar_h_px={} \
             content={}x{}@({},{}) bar={}x{}@({},{})",
            vp_w,
            vp_h,
            bar_h_px,
            content.w,
            content.h,
            content.x,
            content.y,
            bar.w,
            bar.h,
            bar.x,
            bar.y,
        );

        Self { vp, content, bar }
    }

    /// Content width in TUI cells. Only valid when `cell_w` comes from the
    /// renderer — callers in the TUI layer own this conversion.
    pub fn content_cols(&self, cell_w: u32) -> u16 {
        if cell_w == 0 {
            return 1;
        }
        (self.content.w / cell_w).max(1) as u16
    }

    /// Content height in TUI cells.
    pub fn content_rows(&self, cell_h: u32) -> u16 {
        if cell_h == 0 {
            return 1;
        }
        (self.content.h / cell_h).max(1) as u16
    }

    pub fn content_cell_rect(&self, cell_w: u32, cell_h: u32) -> CellRect {
        CellRect::new(0, 0, self.content_cols(cell_w), self.content_rows(cell_h))
    }

    pub fn cell_rect_to_px(&self, cr: CellRect, cell_w: u32, cell_h: u32) -> Rect {
        Rect::new(
            self.content.x + cr.x as u32 * cell_w,
            self.content.y + cr.y as u32 * cell_h,
            cr.w as u32 * cell_w,
            cr.h as u32 * cell_h,
        )
    }
}
