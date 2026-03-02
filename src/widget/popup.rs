//! popup.rs — floating modal overlay with dimmed backdrop.
//!
//! # Usage
//!
//! ```rust,ignore
//! // In view(), AFTER rendering the main UI:
//! if self.show_modal {
//!     let popup_rect = Popup::centered(frame.area(), 60, 20, frame.cell_w(), frame.cell_h());
//!     let inner = frame.render_block(
//!         Popup::block(t).title(" Confirm "),
//!         popup_rect,
//!     );
//!     frame.render(Paragraph::new("Are you sure?"), inner);
//! }
//! ```

use crate::layout::Rect;
use crate::renderer::{Color, CornerRadius, PixelCanvas, Theme};
use crate::widget::Block;

// ── Popup geometry helpers ────────────────────────────────────────────────────

/// Geometry and backdrop rendering for modal popups.
pub struct Popup;

impl Popup {
    /// Compute a centered rect of `cols` × `rows` cells inside `parent`.
    ///
    /// Falls back gracefully if the parent is too small.
    pub fn centered(parent: Rect, cols: u32, rows: u32, cell_w: u32, cell_h: u32) -> Rect {
        let w = (cols * cell_w).min(parent.w);
        let h = (rows * cell_h).min(parent.h);
        let x = parent.x + parent.w.saturating_sub(w) / 2;
        let y = parent.y + parent.h.saturating_sub(h) / 2;
        Rect::new(x, y, w, h)
    }

    /// Compute a centered rect given an explicit pixel size.
    pub fn centered_px(parent: Rect, w: u32, h: u32) -> Rect {
        let w = w.min(parent.w);
        let h = h.min(parent.h);
        let x = parent.x + parent.w.saturating_sub(w) / 2;
        let y = parent.y + parent.h.saturating_sub(h) / 2;
        Rect::new(x, y, w, h)
    }

    /// Render a full-viewport semi-transparent backdrop.
    ///
    /// Call this before rendering popup content so the backdrop is behind it.
    ///
    /// ```rust,ignore
    /// Popup::render_backdrop(frame.canvas(), frame.area(), frame.theme());
    /// let inner = frame.render_block(Popup::block(t), popup_rect);
    /// ```
    pub fn render_backdrop(canvas: &mut PixelCanvas, viewport: Rect, t: &Theme) {
        canvas.fill(viewport.x, viewport.y, viewport.w, viewport.h, t.modal_overlay);
    }

    /// Return a pre-styled [`Block`] suitable for popup content.
    ///
    /// Rounded corners, active border, pane background.
    pub fn block(t: &Theme) -> Block<'static> {
        use crate::widget::{Borders, Style};
        Block::new()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(t.active_border))
            .style(Style::default().bg(t.pane_bg))
            .rounded(6.0)
    }

    /// Render a backdrop AND a popup block in one call, returning the inner
    /// content rect.
    ///
    /// ```rust,ignore
    /// let inner = Popup::render(
    ///     frame.canvas(), frame.area(), popup_rect,
    ///     frame.cell_w(), frame.cell_h(), frame.theme(),
    /// );
    /// frame.render(Paragraph::new("popup content"), inner);
    /// ```
    pub fn render(
        canvas: &mut PixelCanvas,
        viewport: Rect,
        popup_rect: Rect,
        cell_w: u32,
        cell_h: u32,
        t: &Theme,
    ) -> Rect {
        Self::render_backdrop(canvas, viewport, t);
        Self::block(t).render(canvas, popup_rect, cell_w, cell_h, t)
    }
}
