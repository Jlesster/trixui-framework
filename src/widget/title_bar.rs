//! title_bar.rs — compositor window titlebar decoration widget.
//!
//! Renders a styled titlebar with optional close/minimize/maximize buttons
//! and registers hit regions so the compositor can route mouse events.
//!
//! # Usage (in a TWM compositor)
//!
//! ```rust,ignore
//! let bar_rect = Rect::new(win_x, win_y, win_w, cell_h + 4);
//!
//! let hits = TitleBar::new("My Window")
//!     .focused(is_focused)
//!     .buttons(TitleBarButtons::ALL)
//!     .render_with_regions(canvas, bar_rect, cell_w, cell_h, theme);
//!
//! // hits is a Vec<(TitleBarHit, Rect)>
//! // Pass to compositor hit-test on mouse events.
//! ```

use crate::layout::Rect;
use crate::renderer::{Color, CornerRadius, PixelCanvas, TextStyle, Theme};
use crate::widget::{Style, Widget, bar_text_y};

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, Default)]
    pub struct TitleBarButtons: u8 {
        const CLOSE    = 0b001;
        const MINIMIZE = 0b010;
        const MAXIMIZE = 0b100;
        const ALL      = 0b111;
    }
}

/// Which part of a titlebar was hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitleBarHit {
    /// The drag / move region (empty area of the bar).
    Drag,
    Close,
    Minimize,
    Maximize,
}

pub struct TitleBar<'a> {
    title:    &'a str,
    focused:  bool,
    buttons:  TitleBarButtons,
    style:    Style,
    /// Radius of the window-button circles.
    btn_r:    u32,
}

impl<'a> TitleBar<'a> {
    pub fn new(title: &'a str) -> Self {
        Self {
            title,
            focused: false,
            buttons: TitleBarButtons::ALL,
            style: Style::default(),
            btn_r: 5,
        }
    }

    pub fn focused(mut self, f: bool) -> Self { self.focused = f; self }
    pub fn buttons(mut self, b: TitleBarButtons) -> Self { self.buttons = b; self }
    pub fn style(mut self, s: Style) -> Self { self.style = s; self }
    pub fn button_radius(mut self, r: u32) -> Self { self.btn_r = r; self }

    /// Render and return hit-test regions for each interactive area.
    ///
    /// Returns `Vec<(TitleBarHit, Rect)>` — use these for compositor mouse routing.
    pub fn render_with_regions(
        self,
        canvas: &mut PixelCanvas,
        area: Rect,
        cell_w: u32,
        cell_h: u32,
        t: &Theme,
    ) -> Vec<(TitleBarHit, Rect)> {
        if area.is_empty() { return vec![]; }

        let bg = self.style.bg.unwrap_or(t.bar_bg);
        let fg = if self.focused { t.active_title } else { t.inactive_title };

        canvas.fill(area.x, area.y, area.w, area.h, bg);

        // Bottom border line
        let border_col = if self.focused { t.active_border } else { t.inactive_border };
        canvas.hline(area.x, area.y + area.h - 1, area.w, border_col);

        let mut regions: Vec<(TitleBarHit, Rect)> = Vec::new();

        // ── Buttons (right side) ──────────────────────────────────────────────
        let btn_d = self.btn_r * 2;
        let btn_pad = cell_w / 2;
        let mut bx = area.x + area.w;
        let by = area.y + area.h.saturating_sub(btn_d) / 2;

        if self.buttons.contains(TitleBarButtons::CLOSE) && bx > btn_d + btn_pad {
            bx -= btn_d + btn_pad;
            let rect = Rect::new(bx, by, btn_d, btn_d);
            let col = if self.focused { Color::hex(0xf38ba8) } else { t.inactive_border };
            canvas.round_fill(rect.x as f32, rect.y as f32, rect.w as f32, rect.h as f32,
                CornerRadius::all(self.btn_r as f32), col);
            regions.push((TitleBarHit::Close, rect));
        }
        if self.buttons.contains(TitleBarButtons::MAXIMIZE) && bx > btn_d + btn_pad {
            bx -= btn_d + btn_pad;
            let rect = Rect::new(bx, by, btn_d, btn_d);
            let col = if self.focused { Color::hex(0xa6e3a1) } else { t.inactive_border };
            canvas.round_fill(rect.x as f32, rect.y as f32, rect.w as f32, rect.h as f32,
                CornerRadius::all(self.btn_r as f32), col);
            regions.push((TitleBarHit::Maximize, rect));
        }
        if self.buttons.contains(TitleBarButtons::MINIMIZE) && bx > btn_d + btn_pad {
            bx -= btn_d + btn_pad;
            let rect = Rect::new(bx, by, btn_d, btn_d);
            let col = if self.focused { Color::hex(0xf9e2af) } else { t.inactive_border };
            canvas.round_fill(rect.x as f32, rect.y as f32, rect.w as f32, rect.h as f32,
                CornerRadius::all(self.btn_r as f32), col);
            regions.push((TitleBarHit::Minimize, rect));
        }

        // ── Title (centred) ───────────────────────────────────────────────────
        let title_area_w = bx.saturating_sub(area.x);
        let title_char_w = self.title.chars().count() as u32 * cell_w;
        let tx = if title_char_w < title_area_w {
            area.x + (title_area_w - title_char_w) / 2
        } else {
            area.x + cell_w
        };
        let ty = bar_text_y(area, cell_h);
        let ts = TextStyle { fg, bg, bold: self.focused, italic: false };
        canvas.text_maxw(tx, ty, self.title, ts, title_area_w.saturating_sub(cell_w));

        // ── Drag region (everything not covered by buttons) ───────────────────
        let drag_rect = Rect::new(area.x, area.y, title_area_w, area.h);
        regions.push((TitleBarHit::Drag, drag_rect));

        regions
    }
}

impl<'a> Widget for TitleBar<'a> {
    fn render(self, canvas: &mut PixelCanvas, area: Rect, cell_w: u32, cell_h: u32, t: &Theme) {
        self.render_with_regions(canvas, area, cell_w, cell_h, t);
    }
}
