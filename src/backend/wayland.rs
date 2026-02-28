//! wayland.rs — Smithay compositor backend.
//!
//! Feature-gated behind `backend-wayland`.
//!
//! This backend is designed to be used from inside a Smithay compositor:
//! the compositor creates a `WaylandBackend`, installs it into trixui's
//! `Terminal`, and trixui drives the chrome render pass each frame.
//!
//! The compositor retains full control of Wayland protocol handling,
//! XDG shell, DRM/KMS etc. — trixui only owns the chrome DrawCmd layer.
//!
//! # Usage (from your compositor)
//! ```rust,ignore
//! let backend = WaylandBackend::new(renderer, cell_w, cell_h);
//! // Each frame, deliver input events:
//! backend.push_key(KeyEvent::plain(KeyCode::Char('j')));
//! // Then call Terminal::render_frame() to get DrawCmds back.
//! let cmds = terminal.render_frame();
//! // Pass cmds to your existing ChromeRenderer / TwmChromeElement.
//! ```

use crate::app::{event::KeyEvent, Event};
use crate::renderer::{DrawCmd, gl::ChromeRenderer};
use super::Backend;

/// Smithay compositor backend.
///
/// Unlike `WinitBackend`, this does not own a window or event loop.
/// The compositor pushes events in and pulls draw commands out.
pub struct WaylandBackend {
    renderer:  ChromeRenderer,
    vp_w:      u32,
    vp_h:      u32,
    pending:   std::collections::VecDeque<RawInput>,
}

enum RawInput {
    Key(KeyEvent),
    Resize(u32, u32),
}

impl WaylandBackend {
    /// Create from an already-initialised `ChromeRenderer`.
    ///
    /// `vp_w` / `vp_h` are the initial viewport dimensions; update them
    /// via `set_size()` when the output is resized.
    pub fn new(renderer: ChromeRenderer, vp_w: u32, vp_h: u32) -> Self {
        Self { renderer, vp_w, vp_h, pending: std::collections::VecDeque::new() }
    }

    /// Deliver a key event from the compositor's input handler.
    pub fn push_key(&mut self, ev: KeyEvent) {
        self.pending.push_back(RawInput::Key(ev));
    }

    /// Notify of a viewport resize (call from your output resize handler).
    pub fn set_size(&mut self, w: u32, h: u32) {
        self.vp_w = w;
        self.vp_h = h;
        self.pending.push_back(RawInput::Resize(w, h));
    }

    /// Borrow the renderer for direct use in `TwmChromeElement::draw()`.
    pub fn renderer_mut(&mut self) -> &mut ChromeRenderer {
        &mut self.renderer
    }
}

impl Backend for WaylandBackend {
    fn size(&self) -> (u32, u32) { (self.vp_w, self.vp_h) }

    fn cell_size(&self) -> (u32, u32) {
        (self.renderer.cell_w, self.renderer.cell_h)
    }

    fn poll_event<Msg: 'static>(&mut self) -> Option<Event<Msg>> {
        match self.pending.pop_front()? {
            RawInput::Key(k)       => Some(Event::Key(k)),
            RawInput::Resize(w, h) => Some(Event::Resize(w, h)),
        }
    }

    fn render(&mut self, cmds: &[DrawCmd], vp_w: u32, vp_h: u32) {
        // In the Wayland path the compositor calls renderer.flush() itself
        // inside TwmChromeElement::draw(). We just store the cmds for retrieval.
        // Terminal::run() should NOT be used with this backend — use
        // Terminal::render_frame() instead (see below).
        self.renderer.flush(cmds, vp_w, vp_h);
    }
}
