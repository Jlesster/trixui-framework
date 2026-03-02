//! wayland.rs — Smithay compositor backend.

use super::Backend;
use crate::app::{event::KeyEvent, Event};
use crate::renderer::{gl::ChromeRenderer, DrawCmd};

pub struct WaylandBackend {
    renderer: ChromeRenderer,
    vp_w: u32,
    vp_h: u32,
    pending: std::collections::VecDeque<RawInput>,
}

enum RawInput {
    Key(KeyEvent),
    Resize(u32, u32),
}

impl WaylandBackend {
    pub fn new(renderer: ChromeRenderer, vp_w: u32, vp_h: u32) -> Self {
        Self {
            renderer,
            vp_w,
            vp_h,
            pending: std::collections::VecDeque::new(),
        }
    }

    pub fn push_key(&mut self, ev: KeyEvent) {
        self.pending.push_back(RawInput::Key(ev));
    }

    pub fn set_size(&mut self, w: u32, h: u32) {
        self.vp_w = w;
        self.vp_h = h;
        self.pending.push_back(RawInput::Resize(w, h));
    }

    pub fn renderer_mut(&mut self) -> &mut ChromeRenderer {
        &mut self.renderer
    }
}

impl Backend for WaylandBackend {
    fn size(&self) -> (u32, u32) {
        (self.vp_w, self.vp_h)
    }

    fn cell_size(&self) -> (u32, u32) {
        (self.renderer.cell_w, self.renderer.cell_h)
    }

    fn natural_h(&self) -> u32 {
        self.renderer.natural_h
    }

    fn poll_event<Msg: 'static>(&mut self) -> Option<Event<Msg>> {
        match self.pending.pop_front()? {
            RawInput::Key(k) => Some(Event::Key(k)),
            RawInput::Resize(w, h) => Some(Event::Resize(w, h)),
        }
    }

    fn render(&mut self, cmds: &[DrawCmd], vp_w: u32, vp_h: u32) {
        self.renderer.flush(cmds, vp_w, vp_h);
    }
}
