//! backend — platform abstraction trait + implementations.

use crate::app::Event;
use crate::renderer::DrawCmd;

/// A platform backend: owns the window/surface, GL context, and input source.
pub trait Backend: Sized {
    /// Physical pixel size of the drawable surface.
    fn size(&self) -> (u32, u32);

    /// Cell dimensions in pixels (from the font atlas).
    fn cell_size(&self) -> (u32, u32);

    /// Actual ink height of glyphs (ascent + |descent| + line_gap).
    /// Used by Frame to pass natural_h to BarBuilder for correct vertical centering.
    /// Return 0 if not available (Frame will fall back to cell_h behaviour).
    fn natural_h(&self) -> u32;

    /// Poll one pending input event, or return None if the queue is empty.
    fn poll_event<Msg: 'static>(&mut self) -> Option<Event<Msg>>;

    /// Flush draw commands to the screen.
    fn render(&mut self, cmds: &[DrawCmd], vp_w: u32, vp_h: u32);
}

#[cfg(feature = "backend-winit")]
pub mod winit;

#[cfg(feature = "backend-wayland")]
pub mod wayland;
