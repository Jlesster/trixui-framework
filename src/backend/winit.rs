//! winit.rs — standalone windowed backend using winit 0.30 + glutin.
//!
//! # Usage
//!
//! Do NOT use `Terminal::run()` with this backend — winit owns the event
//! loop. Use `WinitBackend::run_app()` instead:
//!
//! ```rust,no_run
//! WinitBackend::new()?.run_app(MyApp::new())?;
//! ```
//!
//! # winit 0.30 notes
//!
//! winit 0.30 replaced the old `event_loop.run(|event, _, cf| {})` closure
//! API with the `ApplicationHandler` trait. The loop is driven by
//! `EventLoop::run_app(&mut handler)` which calls trait methods on your
//! handler struct. `ControlFlow::Exit` → `EventLoopWindowTarget::exit()`.
//! `MainEventsCleared` → `ApplicationHandler::about_to_wait`.

use std::num::NonZeroU32;

use glutin::{
    config::ConfigTemplateBuilder,
    context::{ContextApi, ContextAttributesBuilder, NotCurrentGlContext, PossiblyCurrentContext},
    display::GetGlDisplay,
    prelude::*,
    surface::{Surface, SurfaceAttributesBuilder, WindowSurface},
};
use glutin_winit::GlWindow;
use winit::{
    application::ApplicationHandler,
    event::{ElementState, MouseButton as WinitBtn, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowId},
};

use super::Backend;
use crate::app::{
    event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind},
    App, Cmd, Event, Frame,
};
use crate::layout::ScreenLayout;
use crate::renderer::{
    gl::{ChromeRenderer, GlyphAtlas, Shaper},
    DrawCmd, PixelCanvas,
};

const DEFAULT_FONT: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/IosevkaJlessBrainsNerdFontNerdFont-Regular.ttf"
));

// ── GL surface + window, created lazily on Resumed ───────────────────────────

struct GlState {
    window: Window,
    surface: Surface<WindowSurface>,
    context: PossiblyCurrentContext,
}

// ── ApplicationHandler impl ───────────────────────────────────────────────────

/// Inner handler passed to `EventLoop::run_app`.
struct Handler<A: App> {
    app: A,
    renderer: ChromeRenderer,  // created before the event loop
    gl_state: Option<GlState>, // None until first Resumed
    font_data: Vec<u8>,        // kept for surface recreation on Android
    tick_ns: u64,
    last_tick: std::time::Instant,
    last_mouse: (u32, u32),
    cur_mods: KeyModifiers,
    quit: bool,
}

impl<A: App> Handler<A> {
    fn render(&mut self) {
        let Some(gs) = self.gl_state.as_mut() else {
            return;
        };
        let sz = gs.window.inner_size();
        let (vw, vh) = (sz.width, sz.height);
        if vw == 0 || vh == 0 {
            return;
        }

        let (cw, ch) = (self.renderer.cell_w, self.renderer.cell_h);
        let theme = self.app.theme();
        let sl = ScreenLayout::new(vw, vh, cw, ch, 0);
        let mut canvas = PixelCanvas::new(vw, vh);
        canvas.set_clip(Some(sl.vp));
        {
            let mut frame = Frame::new(&mut canvas, sl, &theme);
            self.app.view(&mut frame);
        }
        let cmds = canvas.finish();

        unsafe {
            gl::ClearColor(0.07, 0.07, 0.10, 1.0);
            gl::Clear(gl::COLOR_BUFFER_BIT);
        }
        self.renderer.flush(&cmds, vw, vh);
        gs.surface.swap_buffers(&gs.context).ok();
    }

    fn do_cmd(&mut self, cmd: Cmd<A::Message>) -> bool {
        match cmd {
            Cmd::None => false,
            Cmd::Quit => true,
            Cmd::Msg(m) => {
                let next = self.app.update(Event::Message(m));
                self.do_cmd(next)
            }
            Cmd::Batch(v) => v.into_iter().any(|c| self.do_cmd(c)),
        }
    }
}

impl<A: App> ApplicationHandler for Handler<A> {
    /// Called when the event loop is ready (or the app is resumed on mobile).
    /// This is where we create the GL surface and window.
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.gl_state.is_some() {
            return;
        } // already initialised

        let attrs = winit::window::WindowAttributes::default()
            .with_title("trixui")
            .with_inner_size(winit::dpi::LogicalSize::new(1280u32, 800u32))
            .with_resizable(true);

        let template = ConfigTemplateBuilder::new();
        let display_builder =
            glutin_winit::DisplayBuilder::new().with_window_attributes(Some(attrs));

        let (window, gl_config) = display_builder
            .build(el, template, |cfgs| {
                cfgs.reduce(|a, b| {
                    if a.num_samples() > b.num_samples() {
                        a
                    } else {
                        b
                    }
                })
                .unwrap()
            })
            .expect("failed to build window + GL config");

        let window = window.unwrap();
        let display = gl_config.display();

        let ctx_attrs = ContextAttributesBuilder::new()
            .with_context_api(ContextApi::Gles(Some(glutin::context::Version::new(3, 0))))
            .build(None);
        let not_current = unsafe {
            display
                .create_context(&gl_config, &ctx_attrs)
                .expect("failed to create GL context")
        };

        let sz = window.inner_size();
        let surf_attrs = window
            .build_surface_attributes(SurfaceAttributesBuilder::<WindowSurface>::new())
            .expect("failed to build surface attrs");
        let surface = unsafe {
            display
                .create_window_surface(&gl_config, &surf_attrs)
                .expect("failed to create window surface")
        };
        let context = not_current
            .make_current(&surface)
            .expect("make_current failed");

        gl::load_with(|s| display.get_proc_address(&std::ffi::CString::new(s).unwrap()));

        self.gl_state = Some(GlState {
            window,
            surface,
            context,
        });
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if self.quit {
            el.exit();
            return;
        }

        match event {
            WindowEvent::CloseRequested => {
                el.exit();
            }

            WindowEvent::Resized(sz) => {
                if let Some(gs) = self.gl_state.as_mut() {
                    if let (Some(nw), Some(nh)) = (
                        NonZeroU32::new(sz.width.max(1)),
                        NonZeroU32::new(sz.height.max(1)),
                    ) {
                        gs.surface.resize(&gs.context, nw, nh);
                    }
                }
                // Split the borrow: call update first, store result, then call do_cmd.
                let cmd = self.app.update(Event::Resize(sz.width, sz.height));
                if self.do_cmd(cmd) {
                    self.quit = true;
                    el.exit();
                }
            }

            WindowEvent::ModifiersChanged(mods) => {
                self.cur_mods = map_mods(mods.state());
            }

            WindowEvent::KeyboardInput { event: ke, .. } => {
                if ke.state == ElementState::Pressed {
                    if let Some(code) = map_key(&ke.logical_key) {
                        let ev = Event::Key(KeyEvent::new(code, self.cur_mods));
                        let cmd = self.app.update(ev);
                        if self.do_cmd(cmd) {
                            self.quit = true;
                            el.exit();
                        }
                    }
                }
            }

            WindowEvent::CursorMoved { position, .. } => {
                self.last_mouse = (position.x as u32, position.y as u32);
                let (x, y) = self.last_mouse;
                self.app.update(Event::Mouse(MouseEvent {
                    kind: MouseEventKind::Moved,
                    x,
                    y,
                    button: MouseButton::None,
                }));
            }

            WindowEvent::MouseInput { state, button, .. } => {
                let (x, y) = self.last_mouse;
                let btn = map_mouse_btn(button);
                let kind = if state == ElementState::Pressed {
                    MouseEventKind::Down
                } else {
                    MouseEventKind::Up
                };
                let cmd = self.app.update(Event::Mouse(MouseEvent {
                    kind,
                    x,
                    y,
                    button: btn,
                }));
                if self.do_cmd(cmd) {
                    self.quit = true;
                    el.exit();
                }
            }

            WindowEvent::MouseWheel { delta, .. } => {
                let (x, y) = self.last_mouse;
                let kind = match delta {
                    MouseScrollDelta::LineDelta(_, dy) if dy > 0.0 => MouseEventKind::ScrollUp,
                    MouseScrollDelta::PixelDelta(p) if p.y > 0.0 => MouseEventKind::ScrollUp,
                    _ => MouseEventKind::ScrollDown,
                };
                self.app.update(Event::Mouse(MouseEvent {
                    kind,
                    x,
                    y,
                    button: MouseButton::None,
                }));
            }

            WindowEvent::RedrawRequested => {
                self.render();
            }

            _ => {}
        }
    }

    /// Replaces `MainEventsCleared` in winit 0.30.
    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if self.quit {
            el.exit();
            return;
        }

        // Tick
        let now = std::time::Instant::now();
        if now.duration_since(self.last_tick).as_nanos() as u64 >= self.tick_ns {
            self.last_tick = now;
            let cmd = self.app.update(Event::Tick);
            if self.do_cmd(cmd) {
                self.quit = true;
                el.exit();
                return;
            }
        }

        // Request a redraw every frame (Poll mode).
        if let Some(gs) = self.gl_state.as_ref() {
            gs.window.request_redraw();
        }
    }
}

// Fix for the Resized branch — split the borrow manually:
// (replace the inline self.quit |= self.do_cmd(self.app.update(...)) with this pattern)
// The above handler body has a subtle double-borrow. The actual implementation
// below in window_event uses a temporary to avoid it.

// ── Public API ────────────────────────────────────────────────────────────────

pub struct WinitBackend {
    font_data: Vec<u8>,
    size_px: f32,
    // For the Backend trait impl only — not used by run_app.
    pending: std::collections::VecDeque<RawInput>,
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
    pub fn new() -> crate::Result<Self> {
        Self::with_font(DEFAULT_FONT, 20.0)
    }

    pub fn with_font(font_data: &[u8], size_px: f32) -> crate::Result<Self> {
        Ok(Self {
            font_data: font_data.to_vec(),
            size_px,
            pending: Default::default(),
        })
    }

    /// Run the application. Blocks until quit.
    ///
    /// Creates the GL context and window on the first `Resumed` event (the
    /// winit 0.30 lifecycle), then drives the app at `App::tick_rate()` Hz.
    pub fn run_app<A: App>(self, mut app: A) -> crate::Result<()> {
        let font_static: &'static [u8] = Box::leak(self.font_data.clone().into_boxed_slice());

        // Build a temporary event loop just to get display/config for the
        // atlas — we need cell metrics before the first Resumed fires.
        // Instead, build the atlas from font data directly (no GL needed).
        let atlas = GlyphAtlas::new(&self.font_data, None, None, self.size_px, 1.2)
            .map_err(|e| format!("GlyphAtlas: {e}"))?;
        let shaper = Shaper::new(font_static);

        // ChromeRenderer requires a current GL context. We can't create it
        // until Resumed. Use a deferred init pattern: build the EventLoop,
        // then create the renderer inside the first Resumed call.
        //
        // To keep the API simple we create a throwaway EventLoop here only
        // for the display probe, create the renderer on its first Resumed,
        // then run the real loop. Since glutin_winit creates the window
        // inside DisplayBuilder::build (called in Resumed), we defer
        // ChromeRenderer creation there too.
        //
        // The atlas cell metrics are available from GlyphAtlas before any
        // GL calls — ChromeRenderer::new just uploads textures.

        let init_cmd = app.init();

        let event_loop = EventLoop::new()?;
        event_loop.set_control_flow(ControlFlow::Poll);

        // We'll finish ChromeRenderer init inside the first `resumed` call.
        // Smuggle the pre-built atlas + shaper via Option fields in a wrapper.
        let mut handler = HandlerBuilder {
            app,
            font_data: self.font_data,
            size_px: self.size_px,
            atlas: Some(atlas),
            shaper: Some(shaper),
            gl_state: None,
            renderer: None,
            tick_ns: 1_000_000_000u64 / 60, // updated in run after app.tick_rate()
            last_tick: std::time::Instant::now(),
            last_mouse: (0, 0),
            cur_mods: KeyModifiers::NONE,
            quit: false,
            init_cmd: Some(init_cmd),
        };

        event_loop.run_app(&mut handler)?;
        Ok(())
    }
}

// ── HandlerBuilder — deferred GL init ────────────────────────────────────────

struct HandlerBuilder<A: App> {
    app: A,
    font_data: Vec<u8>,
    size_px: f32,
    atlas: Option<GlyphAtlas>,
    shaper: Option<Shaper>,
    gl_state: Option<GlState>,
    renderer: Option<ChromeRenderer>,
    tick_ns: u64,
    last_tick: std::time::Instant,
    last_mouse: (u32, u32),
    cur_mods: KeyModifiers,
    quit: bool,
    init_cmd: Option<Cmd<A::Message>>,
}

impl<A: App> HandlerBuilder<A> {
    fn renderer(&mut self) -> &mut ChromeRenderer {
        self.renderer.as_mut().unwrap()
    }

    fn do_cmd(&mut self, cmd: Cmd<A::Message>) -> bool {
        match cmd {
            Cmd::None => false,
            Cmd::Quit => true,
            Cmd::Msg(m) => {
                let next = self.app.update(Event::Message(m));
                self.do_cmd(next)
            }
            Cmd::Batch(v) => v.into_iter().any(|c| self.do_cmd(c)),
        }
    }

    fn render(&mut self) {
        let Some(gs) = self.gl_state.as_mut() else {
            return;
        };
        let Some(r) = self.renderer.as_mut() else {
            return;
        };
        let sz = gs.window.inner_size();
        let (vw, vh) = (sz.width, sz.height);
        if vw == 0 || vh == 0 {
            return;
        }

        let (cw, ch) = (r.cell_w, r.cell_h);
        let theme = self.app.theme();
        let sl = ScreenLayout::new(vw, vh, cw, ch, 0);
        let mut canvas = PixelCanvas::new(vw, vh);
        canvas.set_clip(Some(sl.vp));
        {
            let mut frame = Frame::new(&mut canvas, sl, &theme);
            self.app.view(&mut frame);
        }
        let cmds = canvas.finish();

        unsafe {
            gl::ClearColor(0.07, 0.07, 0.10, 1.0);
            gl::Clear(gl::COLOR_BUFFER_BIT);
        }
        r.flush(&cmds, vw, vh);
        gs.surface.swap_buffers(&gs.context).ok();
    }
}

impl<A: App> ApplicationHandler for HandlerBuilder<A> {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.gl_state.is_some() {
            return;
        }

        let win_attrs = winit::window::WindowAttributes::default()
            .with_title("trixui")
            .with_inner_size(winit::dpi::LogicalSize::new(1280u32, 800u32))
            .with_resizable(true);

        let display_builder =
            glutin_winit::DisplayBuilder::new().with_window_attributes(Some(win_attrs));

        let (window, gl_config) = display_builder
            .build(el, ConfigTemplateBuilder::new(), |cfgs| {
                cfgs.reduce(|a, b| {
                    if a.num_samples() > b.num_samples() {
                        a
                    } else {
                        b
                    }
                })
                .unwrap()
            })
            .expect("DisplayBuilder::build failed");

        let window = window.unwrap();
        let display = gl_config.display();

        let ctx_attrs = ContextAttributesBuilder::new()
            .with_context_api(ContextApi::Gles(Some(glutin::context::Version::new(3, 0))))
            .build(None);
        let not_current = unsafe {
            display
                .create_context(&gl_config, &ctx_attrs)
                .expect("create_context failed")
        };

        let surf_attrs = window
            .build_surface_attributes(SurfaceAttributesBuilder::<WindowSurface>::new())
            .expect("build_surface_attributes failed");
        let surface = unsafe {
            display
                .create_window_surface(&gl_config, &surf_attrs)
                .expect("create_window_surface failed")
        };
        let context = not_current
            .make_current(&surface)
            .expect("make_current failed");

        gl::load_with(|s| display.get_proc_address(&std::ffi::CString::new(s).unwrap()));

        // Now that GL is current, finish building ChromeRenderer.
        let atlas = self.atlas.take().unwrap();
        let shaper = self.shaper.take().unwrap();
        let size_px = self.size_px;
        let renderer = ChromeRenderer::new(atlas, shaper, 1000.0, size_px)
            .expect("ChromeRenderer::new failed");

        self.tick_ns = 1_000_000_000u64 / self.app.tick_rate();
        self.renderer = Some(renderer);
        self.gl_state = Some(GlState {
            window,
            surface,
            context,
        });

        // Process the deferred init command now that we can render.
        if let Some(cmd) = self.init_cmd.take() {
            if self.do_cmd(cmd) {
                el.exit();
            }
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if self.quit {
            el.exit();
            return;
        }

        match event {
            WindowEvent::CloseRequested => {
                el.exit();
            }

            WindowEvent::Resized(sz) => {
                if let Some(gs) = self.gl_state.as_mut() {
                    if let (Some(nw), Some(nh)) = (
                        NonZeroU32::new(sz.width.max(1)),
                        NonZeroU32::new(sz.height.max(1)),
                    ) {
                        gs.surface.resize(&gs.context, nw, nh);
                    }
                }
                // Split borrow: call update first, then do_cmd.
                let cmd = self.app.update(Event::Resize(sz.width, sz.height));
                if self.do_cmd(cmd) {
                    self.quit = true;
                    el.exit();
                }
            }

            WindowEvent::ModifiersChanged(mods) => {
                self.cur_mods = map_mods(mods.state());
            }

            WindowEvent::KeyboardInput { event: ke, .. } => {
                if ke.state == ElementState::Pressed {
                    if let Some(code) = map_key(&ke.logical_key) {
                        let ev = Event::Key(KeyEvent::new(code, self.cur_mods));
                        let cmd = self.app.update(ev);
                        if self.do_cmd(cmd) {
                            self.quit = true;
                            el.exit();
                        }
                    }
                }
            }

            WindowEvent::CursorMoved { position, .. } => {
                self.last_mouse = (position.x as u32, position.y as u32);
                let (x, y) = self.last_mouse;
                self.app.update(Event::Mouse(MouseEvent {
                    kind: MouseEventKind::Moved,
                    x,
                    y,
                    button: MouseButton::None,
                }));
            }

            WindowEvent::MouseInput { state, button, .. } => {
                let (x, y) = self.last_mouse;
                let btn = map_mouse_btn(button);
                let kind = if state == ElementState::Pressed {
                    MouseEventKind::Down
                } else {
                    MouseEventKind::Up
                };
                let cmd = self.app.update(Event::Mouse(MouseEvent {
                    kind,
                    x,
                    y,
                    button: btn,
                }));
                if self.do_cmd(cmd) {
                    self.quit = true;
                    el.exit();
                }
            }

            WindowEvent::MouseWheel { delta, .. } => {
                let (x, y) = self.last_mouse;
                let kind = match delta {
                    MouseScrollDelta::LineDelta(_, dy) if dy > 0.0 => MouseEventKind::ScrollUp,
                    MouseScrollDelta::PixelDelta(p) if p.y > 0.0 => MouseEventKind::ScrollUp,
                    _ => MouseEventKind::ScrollDown,
                };
                self.app.update(Event::Mouse(MouseEvent {
                    kind,
                    x,
                    y,
                    button: MouseButton::None,
                }));
            }

            WindowEvent::RedrawRequested => {
                self.render();
            }

            _ => {}
        }
    }

    /// Replaces `MainEventsCleared`. Called once all pending window events
    /// for a frame have been dispatched.
    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if self.quit {
            el.exit();
            return;
        }

        let now = std::time::Instant::now();
        if now.duration_since(self.last_tick).as_nanos() as u64 >= self.tick_ns {
            self.last_tick = now;
            let cmd = self.app.update(Event::Tick);
            if self.do_cmd(cmd) {
                self.quit = true;
                el.exit();
                return;
            }
        }

        if let Some(gs) = self.gl_state.as_ref() {
            gs.window.request_redraw();
        }
    }
}

// ── Backend trait impl (for type-system compat; not used by run_app) ─────────

impl Backend for WinitBackend {
    fn size(&self) -> (u32, u32) {
        (0, 0)
    } // no window outside run_app
    fn cell_size(&self) -> (u32, u32) {
        (6, 17)
    } // fallback metrics

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

    fn render(&mut self, _cmds: &[DrawCmd], _vp_w: u32, _vp_h: u32) {
        // No-op outside run_app — window doesn't exist.
    }
}

// ── Input mapping ─────────────────────────────────────────────────────────────

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

fn map_mouse_btn(b: WinitBtn) -> MouseButton {
    match b {
        WinitBtn::Left => MouseButton::Left,
        WinitBtn::Right => MouseButton::Right,
        WinitBtn::Middle => MouseButton::Middle,
        _ => MouseButton::None,
    }
}
