//! spinner.rs — animated throbber / loading indicator.
//!
//! # Usage
//!
//! ```rust,ignore
//! // In App state:
//! spinner: SpinnerState::new(),
//!
//! // In update():
//! if let Event::Tick = event {
//!     self.spinner.tick();
//! }
//!
//! // In view():
//! frame.render_stateful(
//!     Spinner::new().style(Style::default().fg(t.bar_accent)),
//!     spinner_area,
//!     &mut self.spinner,
//! );
//! ```

use crate::layout::Rect;
use crate::renderer::{PixelCanvas, TextStyle, Theme};
use crate::widget::{Style, StatefulWidget};

// ── SpinnerState ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct SpinnerState {
    frame: usize,
}

impl SpinnerState {
    pub fn new() -> Self { Self::default() }

    /// Advance one animation frame. Call once per `Event::Tick`.
    pub fn tick(&mut self) {
        self.frame = self.frame.wrapping_add(1);
    }

    /// Reset to the first frame.
    pub fn reset(&mut self) { self.frame = 0; }
}

// ── Spinner widget ────────────────────────────────────────────────────────────

/// Spinner styles.
#[derive(Debug, Clone, Copy, Default)]
pub enum SpinnerStyle {
    /// ⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏  (braille)
    #[default]
    Braille,
    /// ◐◓◑◒ (quarter circles)
    Quarters,
    /// ◜◝◞◟ (small arc)
    Arc,
    /// |/-\
    Ascii,
    /// ▁▂▃▄▅▆▇█▇▆▅▄▃▂ (bar growth)
    Bar,
}

impl SpinnerStyle {
    fn frames(self) -> &'static [&'static str] {
        match self {
            Self::Braille  => &["⠋","⠙","⠹","⠸","⠼","⠴","⠦","⠧","⠇","⠏"],
            Self::Quarters => &["◐","◓","◑","◒"],
            Self::Arc      => &["◜","◝","◞","◟"],
            Self::Ascii    => &["|","/","-","\\"],
            Self::Bar      => &["▁","▂","▃","▄","▅","▆","▇","█","▇","▆","▅","▄","▃","▂"],
        }
    }
}

/// Animated loading spinner.
pub struct Spinner {
    pub style:    Style,
    pub kind:     SpinnerStyle,
    pub label:    Option<String>,
}

impl Spinner {
    pub fn new() -> Self {
        Self { style: Style::default(), kind: SpinnerStyle::default(), label: None }
    }
    pub fn style(mut self, s: Style) -> Self { self.style = s; self }
    pub fn kind(mut self, k: SpinnerStyle) -> Self { self.kind = k; self }
    pub fn label(mut self, l: impl Into<String>) -> Self { self.label = Some(l.into()); self }
}

impl Default for Spinner { fn default() -> Self { Self::new() } }

impl StatefulWidget for Spinner {
    type State = SpinnerState;

    fn render(
        self,
        canvas: &mut PixelCanvas,
        area: Rect,
        state: &mut SpinnerState,
        cell_w: u32,
        cell_h: u32,
        t: &Theme,
    ) {
        if area.is_empty() { return; }
        let frames = self.kind.frames();
        let frame = &frames[state.frame % frames.len()];

        let fg = self.style.fg.unwrap_or(t.bar_accent);
        let bg = self.style.bg.unwrap_or(t.normal_bg);
        let ts = TextStyle { fg, bg, bold: self.style.bold, italic: false };

        let y = area.y + area.h.saturating_sub(cell_h) / 2;
        canvas.text(area.x, y, frame, ts);

        if let Some(label) = &self.label {
            let lx = area.x + cell_w + cell_w / 2;
            if lx < area.x + area.w {
                let label_ts = TextStyle { fg: t.dim_fg, bg, bold: false, italic: false };
                canvas.text_maxw(lx, y, label, label_ts, area.w.saturating_sub(lx - area.x));
            }
        }
    }
}
