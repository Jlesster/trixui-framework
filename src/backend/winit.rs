//! winit.rs — standalone windowed backend using winit + glutin.
//!
//! Feature-gated behind `backend-winit` (default).
//!
//! # Usage
//! ```rust,no_run
//! Terminal::new(WinitBackend::new()?)?.run(MyApp::new())?;
//! ```

use glutin::{
    config::ConfigTemplateBuilder,
    context::{ContextApi, ContextAttributesBuilder, NotCurrentGlContext, PossiblyCurrentContext},
    display::GetGlDisplay,
    prelude::*,
    surface::{Surface, SurfaceAttributesBuilder, WindowSurface},
};
use glutin_winit::GlWindow;
use winit::{
    event::{ElementState, Event as WinitEvent, WindowEvent},
    event_loop::{ControlFlow, EventLoop, EventLoopBuilder},
    window::{Window, WindowBuilder},
};

use super::Backend;
use crate::app::{
    event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind},
    Event,
};
use crate::renderer::{gl::ChromeRenderer, gl::GlyphAtlas, DrawCmd};

/// Default JetBrainsMono Nerd Font — embed at build time.
/// Override by calling `WinitBackend::with_font_data()`.
const DEFAULT_FONT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/IosevkaJlessBrainsNerdFontNerdFont-Regular.ttf"
));

/// Standalone window backend.
pub struct WinitBackend {
    event_loop: Option<EventLoop<()>>,
    window: Window,
    surface: Surface<WindowSurface>,
    context: PossiblyCurrentContext,
    renderer: ChromeRenderer,
    pending: std::collections::VecDeque<RawInput>,
    last_mouse_px: (u32, u32),
}

enum RawInput {
    Key {
        code: KeyCode,
        mods: KeyModifiers,
    },
    Mouse {
        kind: MouseEventKind,
        x: u32,
        y: u32,
        btn: MouseButton,
    },
    Resize(u32, u32),
}

impl WinitBackend {
    /// Create with default font (JetBrainsMono, embedded).
    pub fn new() -> crate::Result<Self> {
        Self::with_font(DEFAULT_FONT, 20.0, "trixui")
    }

    /// Create with a custom font binary and pixel size.
    pub fn with_font(font_data: &[u8], size_px: f32, title: &str) -> crate::Result<Self> {
        use glutin::config::ConfigTemplateBuilder;
        use glutin::context::{ContextApi, ContextAttributesBuilder};
        use glutin::surface::SurfaceAttributesBuilder;
        use glutin_winit::DisplayBuilder;
        use winit::window::WindowBuilder;

        let event_loop = winit::event_loop::EventLoop::new()?;

        let window_builder = WindowBuilder::new()
            .with_title(title)
            .with_inner_size(winit::dpi::LogicalSize::new(1280u32, 800u32))
            .with_resizable(true);

        let template = ConfigTemplateBuilder::new();

        let display_builder = DisplayBuilder::new().with_window_builder(Some(window_builder));

        let (window, gl_config) = display_builder.build(&event_loop, template, |configs| {
            configs
                .reduce(|a, b| {
                    if a.num_samples() > b.num_samples() {
                        a
                    } else {
                        b
                    }
                })
                .unwrap()
        })?;

        let window = window.unwrap();
        let size = window.inner_size();

        // glutin_winit::DisplayBuilder already wired up the window handle internally.
        // Build context and surface using the window reference directly.
        let ctx_attrs = ContextAttributesBuilder::new()
            .with_context_api(ContextApi::Gles(Some(glutin::context::Version::new(3, 0))))
            .build(None); // None = use the display's default window handle

        let display = gl_config.display();
        let not_current = unsafe { display.create_context(&gl_config, &ctx_attrs)? };

        let (width, height) = (
            std::num::NonZeroU32::new(size.width.max(1)).unwrap(),
            std::num::NonZeroU32::new(size.height.max(1)).unwrap(),
        );

        let surf_attrs = window.build_surface_attributes(SurfaceAttributesBuilder::<
            glutin::surface::WindowSurface,
        >::new());
        let surface = unsafe { display.create_window_surface(&gl_config, &surf_attrs)? };
        let context = not_current.make_current(&surface)?;

        gl::load_with(|s| display.get_proc_address(&std::ffi::CString::new(s).unwrap()));

        let font_data_static: &'static [u8] = Box::leak(font_data.to_vec().into_boxed_slice());

        let atlas = GlyphAtlas::new(font_data, None, None, size_px, 1.2)
            .map_err(|e| format!("GlyphAtlas: {e}"))?;

        let shaper = crate::renderer::gl::Shaper::new(font_data_static);

        // JetBrainsMono units_per_em = 1000. If you switch fonts, check the
        // head table. Wrong value only affects ligature advance width, not
        // correctness of char-based glyphs.
        let renderer = ChromeRenderer::new(atlas, shaper, 1000.0, size_px)
            .map_err(|e| format!("ChromeRenderer: {e}"))?;

        Ok(Self {
            event_loop: Some(event_loop),
            window,
            surface,
            context,
            renderer,
            pending: std::collections::VecDeque::new(),
            last_mouse_px: (0, 0),
        })
    }

    /// Pump winit events into `self.pending`. Called from `Terminal::run`.
    pub(crate) fn pump(&mut self) {
        // The event loop is moved into `run` in real usage.
        // This stub covers the polling model used in Terminal::run.
    }
}

impl Backend for WinitBackend {
    fn size(&self) -> (u32, u32) {
        let s = self.window.inner_size();
        (s.width, s.height)
    }

    fn cell_size(&self) -> (u32, u32) {
        (self.renderer.cell_w, self.renderer.cell_h)
    }

    fn poll_event<Msg: 'static>(&mut self) -> Option<Event<Msg>> {
        let raw = self.pending.pop_front()?;
        Some(match raw {
            RawInput::Key { code, mods } => Event::Key(KeyEvent::new(code, mods)),
            RawInput::Mouse { kind, x, y, btn } => Event::Mouse(MouseEvent {
                kind,
                x,
                y,
                button: btn,
            }),
            RawInput::Resize(w, h) => Event::Resize(w, h),
        })
    }

    fn render(&mut self, cmds: &[DrawCmd], vp_w: u32, vp_h: u32) {
        unsafe {
            gl::ClearColor(0.07, 0.07, 0.10, 1.0);
            gl::Clear(gl::COLOR_BUFFER_BIT);
        }
        self.renderer.flush(cmds, vp_w, vp_h);
        self.surface.swap_buffers(&self.context).ok();
    }
}

// ── winit key mapping ─────────────────────────────────────────────────────────

pub(crate) fn map_key(key: &winit::keyboard::Key) -> Option<KeyCode> {
    use winit::keyboard::{Key, NamedKey};
    match key {
        Key::Character(s) => s.chars().next().map(KeyCode::Char),
        Key::Named(n) => Some(match n {
            NamedKey::Enter => KeyCode::Enter,
            NamedKey::Backspace => KeyCode::Backspace,
            NamedKey::Delete => KeyCode::Delete,
            NamedKey::Escape => KeyCode::Esc,
            NamedKey::Tab => KeyCode::Tab,
            NamedKey::ArrowUp => KeyCode::Up,
            NamedKey::ArrowDown => KeyCode::Down,
            NamedKey::ArrowLeft => KeyCode::Left,
            NamedKey::ArrowRight => KeyCode::Right,
            NamedKey::Home => KeyCode::Home,
            NamedKey::End => KeyCode::End,
            NamedKey::PageUp => KeyCode::PageUp,
            NamedKey::PageDown => KeyCode::PageDown,
            NamedKey::Insert => KeyCode::Insert,
            NamedKey::F1 => KeyCode::F(1),
            NamedKey::F2 => KeyCode::F(2),
            NamedKey::F3 => KeyCode::F(3),
            NamedKey::F4 => KeyCode::F(4),
            NamedKey::F5 => KeyCode::F(5),
            NamedKey::F6 => KeyCode::F(6),
            NamedKey::F7 => KeyCode::F(7),
            NamedKey::F8 => KeyCode::F(8),
            NamedKey::F9 => KeyCode::F(9),
            NamedKey::F10 => KeyCode::F(10),
            NamedKey::F11 => KeyCode::F(11),
            NamedKey::F12 => KeyCode::F(12),
            _ => return None,
        }),
        _ => None,
    }
}

pub(crate) fn map_mods(mods: winit::keyboard::ModifiersState) -> KeyModifiers {
    let mut m = KeyModifiers::NONE;
    if mods.shift_key() {
        m |= KeyModifiers::SHIFT;
    }
    if mods.control_key() {
        m |= KeyModifiers::CTRL;
    }
    if mods.alt_key() {
        m |= KeyModifiers::ALT;
    }
    if mods.super_key() {
        m |= KeyModifiers::SUPER;
    }
    m
}
