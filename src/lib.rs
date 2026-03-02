//! trixui — Hybrid TUI/OpenGL framework
//!
//! Ratatui-style widgets rendered via OpenGL ES 3.  Works as a standalone
//! windowed app (winit + glutin) or as the chrome layer inside a Wayland
//! compositor (Smithay).  Same widget API, two backends.
//!
//! # Quick start — standalone window
//!
//! ```rust,no_run
//! use trixui::prelude::*;
//!
//! struct Counter { count: i32 }
//!
//! impl App for Counter {
//!     type Message = ();
//!     fn update(&mut self, event: Event<()>) -> Cmd<()> {
//!         if let Event::Key(k) = event {
//!             match k.code {
//!                 KeyCode::Char('+') | KeyCode::Up   => self.count += 1,
//!                 KeyCode::Char('-') | KeyCode::Down => self.count -= 1,
//!                 KeyCode::Char('q') | KeyCode::Esc  => return Cmd::quit(),
//!                 _ => {}
//!             }
//!         }
//!         Cmd::none()
//!     }
//!     fn view(&self, frame: &mut Frame) {
//!         let area = frame.area();
//!         let inner = frame.render_block(
//!             Block::bordered().title(" Counter "),
//!             area,
//!         );
//!         frame.render(
//!             Paragraph::new(&format!("count: {}", self.count)),
//!             inner,
//!         );
//!     }
//! }
//!
//! fn main() -> trixui::Result<()> {
//!     // ✓ Correct — WinitBackend owns the event loop.
//!     WinitBackend::new()?.run_app(Counter { count: 0 })
//! }
//! ```
//!
//! # Quick start — Smithay compositor
//!
//! ```rust,no_run
//! use trixui::prelude::*;
//! use trixui::smithay::SmithayApp;
//!
//! // 1. Implement App exactly as above.
//! // 2. Create SmithayApp once after your GL context is current:
//! let mut ui = SmithayApp::new(font_bytes, 20.0, vp_w, vp_h, MyApp::new())?;
//!
//! // 3. Each frame (inside your DRM render callback):
//! ui.push_key(KeyEvent::plain(KeyCode::Char('j')));
//! ui.render_frame(); // flushes DrawCmds via ChromeRenderer::flush
//!
//! // 4. On output resize:
//! ui.resize(new_w, new_h);
//! ```
//!
//! See [`smithay`] for the full Smithay integration guide.

#![warn(missing_docs)]

pub mod app;
pub mod backend;
pub mod layout;
pub mod renderer;
pub mod widget;

#[cfg(feature = "backend-smithay")]
pub mod smithay;

// Flat re-exports
pub use app::{App, Cmd, Event, Frame, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent};
pub use layout::{CellRect, Rect as PixRect, ScreenLayout};
pub use renderer::{Color as PixColor, DrawCmd, PixelCanvas, TextStyle, Theme};
pub use widget::{
    bar_text_y,
    center_text_x,
    draw_bar,
    draw_pane,
    // Chrome drawing
    BarBuilder,
    BarItem,
    Block,
    Borders,
    Cell,
    Constraint,
    Direction,
    Flex,
    Gauge,
    Layout,
    List,
    ListItem,
    ListState,
    PaneOpts,
    Paragraph,
    Row,
    SectionBuilder,
    StatefulWidget,
    Style,
    Table,
    TableState,
    Tabs,
    Widget,
};

#[cfg(feature = "backend-winit")]
pub use backend::winit::WinitBackend;

#[cfg(feature = "backend-wayland")]
pub use backend::wayland::WaylandBackend;

/// Everything you need for typical usage.
pub mod prelude {
    pub use crate::app::{App, Cmd, Event, Frame, KeyCode, KeyEvent, KeyModifiers, Terminal};
    pub use crate::layout::{Rect as PixRect, ScreenLayout};
    pub use crate::renderer::{Color as PixColor, PixelCanvas, Theme};
    pub use crate::widget::*;

    #[cfg(feature = "backend-winit")]
    pub use crate::backend::winit::WinitBackend;

    #[cfg(feature = "backend-wayland")]
    pub use crate::backend::wayland::WaylandBackend;

    #[cfg(feature = "backend-smithay")]
    pub use crate::smithay::SmithayApp;
}

/// Crate-level result type.
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
