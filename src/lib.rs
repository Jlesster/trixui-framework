//! trixui — Hybrid TUI/OpenGL framework
//!
//! Ratatui-style widgets rendered via OpenGL ES 3 — works inside a Wayland
//! compositor (Smithay) or as a standalone windowed app (winit + glutin).
//!
//! # Quick start
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
//!                 KeyCode::Char('+') => self.count += 1,
//!                 KeyCode::Char('-') => self.count -= 1,
//!                 KeyCode::Char('q') => return Cmd::quit(),
//!                 _ => {}
//!             }
//!         }
//!         Cmd::none()
//!     }
//!     fn view(&self, frame: &mut Frame) {
//!         let area = frame.area();
//!         Block::bordered()
//!             .title(" Counter ")
//!             .render(frame.canvas(), area, frame.cell_w(), frame.cell_h(), frame.theme());
//!     }
//! }
//!
//! fn main() -> trixui::Result<()> {
//!     Terminal::new(WinitBackend::new()?)?.run(Counter { count: 0 })
//! }
//! ```

#![warn(missing_docs)]

pub mod app;
pub mod backend;
pub mod layout;
pub mod renderer;
pub mod widget;

// Flat re-exports for the prelude
pub use app::{App, Cmd, Event, Frame, KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, Terminal};
pub use layout::{CellRect, Rect as PixRect, ScreenLayout};
pub use renderer::{Color as PixColor, DrawCmd, PixelCanvas, TextStyle, Theme};
pub use widget::{
    bar_text_y, center_text_x,
    Block, Borders,
    Constraint, Direction, Flex, Layout,
    Style,
    Widget, StatefulWidget,
    Paragraph,
    List, ListItem, ListState,
    Table, TableState, Row, Cell,
    Tabs,
    Gauge,
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
}

/// Crate-level result type.
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
