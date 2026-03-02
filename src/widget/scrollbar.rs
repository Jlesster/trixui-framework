//! scrollbar.rs — vertical/horizontal scrollbar.
//!
//! # Usage
//!
//! ```rust,ignore
//! // In view(), after rendering your list:
//! if items.len() > visible_rows {
//!     let sb_rect = Rect::new(area.x + area.w - 1, area.y, 1, area.h);
//!     frame.render(
//!         Scrollbar::vertical()
//!             .total(items.len())
//!             .visible(visible_rows)
//!             .position(list_state.offset),
//!         sb_rect,
//!     );
//! }
//! ```

use crate::layout::Rect;
use crate::renderer::{Color, PixelCanvas, Theme};
use crate::widget::{Style, Widget};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollbarOrientation {
    Vertical,
    Horizontal,
}

/// Non-interactive scrollbar indicator.
///
/// For a 1-pixel-wide vertical bar on the right edge of a pane,
/// use `Scrollbar::vertical()` and pass a 1-wide `Rect`.
pub struct Scrollbar {
    orientation: ScrollbarOrientation,
    total:       usize,
    visible:     usize,
    position:    usize,
    track_color: Option<Color>,
    thumb_color: Option<Color>,
}

impl Scrollbar {
    pub fn vertical() -> Self {
        Self {
            orientation: ScrollbarOrientation::Vertical,
            total: 1, visible: 1, position: 0,
            track_color: None, thumb_color: None,
        }
    }
    pub fn horizontal() -> Self {
        Self {
            orientation: ScrollbarOrientation::Horizontal,
            total: 1, visible: 1, position: 0,
            track_color: None, thumb_color: None,
        }
    }

    /// Total number of items (rows/cols) in the scrollable content.
    pub fn total(mut self, n: usize) -> Self { self.total = n.max(1); self }
    /// Number of items currently visible in the viewport.
    pub fn visible(mut self, n: usize) -> Self { self.visible = n.max(1); self }
    /// Current scroll offset (first visible item index).
    pub fn position(mut self, p: usize) -> Self { self.position = p; self }
    pub fn track_color(mut self, c: Color) -> Self { self.track_color = Some(c); self }
    pub fn thumb_color(mut self, c: Color) -> Self { self.thumb_color = Some(c); self }
}

impl Widget for Scrollbar {
    fn render(self, canvas: &mut PixelCanvas, area: Rect, _cell_w: u32, _cell_h: u32, t: &Theme) {
        if area.is_empty() || self.total <= self.visible { return; }

        let track = self.track_color.unwrap_or(t.inactive_border.alpha(80));
        let thumb = self.thumb_color.unwrap_or(t.active_border);

        let is_vert = self.orientation == ScrollbarOrientation::Vertical;
        let track_len = if is_vert { area.h } else { area.w };

        // Draw track
        if is_vert {
            canvas.fill(area.x, area.y, area.w, area.h, track);
        } else {
            canvas.fill(area.x, area.y, area.w, area.h, track);
        }

        if track_len == 0 { return; }

        // Thumb size and position
        let thumb_len = ((track_len as f64 * self.visible as f64 / self.total as f64) as u32)
            .max(2)
            .min(track_len);
        let max_scroll = self.total.saturating_sub(self.visible);
        let thumb_off = if max_scroll == 0 {
            0
        } else {
            ((track_len - thumb_len) as f64 * self.position as f64 / max_scroll as f64) as u32
        };

        if is_vert {
            canvas.fill(area.x, area.y + thumb_off, area.w, thumb_len, thumb);
        } else {
            canvas.fill(area.x + thumb_off, area.y, thumb_len, area.h, thumb);
        }
    }
}
