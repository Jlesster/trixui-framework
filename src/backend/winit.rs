//! winit.rs — standalone windowed backend using winit 0.30 + glutin.

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

// ── GL surface + window ───────────────────────────────────────────────────────

struct GlState {
    window: Window,
    surface: Surface<WindowSurface>,
    context: PossiblyCurrentContext,
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
    held_key: Option<(
        KeyCode,
        KeyModifiers,
        std::time::Instant,
        std::time::Instant,
    )>,
    held_mouse_btn: Option<MouseButton>,
    win_title: String,
    win_size: (u32, u32),
    win_resizable: bool,
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
            Cmd::Spawn(f) => {
                std::thread::spawn(f);
                false
            }
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
        let natural_h = r.natural_h;
        let theme = self.app.theme();
        let sl = ScreenLayout::new(vw, vh, 0);
        let mut canvas = PixelCanvas::new(vw, vh);
        canvas.set_clip(Some(sl.vp));
        {
            let mut frame = Frame::new_with_metrics(&mut canvas, sl, &theme, cw, ch, natural_h);
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
            .with_title(self.win_title.as_str())
            .with_inner_size(winit::dpi::LogicalSize::new(
                self.win_size.0,
                self.win_size.1,
            ))
            .with_resizable(self.win_resizable);

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

            WindowEvent::Focused(gained) => {
                let ev = if gained {
                    Event::FocusGained
                } else {
                    Event::FocusLost
                };
                let cmd = self.app.update(ev);
                if self.do_cmd(cmd) {
                    self.quit = true;
                    el.exit();
                }
                if !gained {
                    self.held_key = None;
                    self.held_mouse_btn = None;
                }
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
                let cmd = self.app.update(Event::Resize(sz.width, sz.height));
                if self.do_cmd(cmd) {
                    self.quit = true;
                    el.exit();
                }
            }

            WindowEvent::ModifiersChanged(mods) => {
                self.cur_mods = map_mods(mods.state());
                self.held_key = None;
            }

            WindowEvent::KeyboardInput { event: ke, .. } => match ke.state {
                ElementState::Pressed => {
                    if let Some(code) = map_key(&ke.logical_key) {
                        let now = std::time::Instant::now();
                        self.held_key = Some((code.clone(), self.cur_mods, now, now));
                        let ev = Event::Key(KeyEvent::new(code, self.cur_mods));
                        let cmd = self.app.update(ev);
                        if self.do_cmd(cmd) {
                            self.quit = true;
                            el.exit();
                        }
                    }
                }
                ElementState::Released => {
                    if let Some(code) = map_key(&ke.logical_key) {
                        if matches!(&self.held_key, Some((hk, _, _, _)) if *hk == code) {
                            self.held_key = None;
                        }
                        let ev = Event::KeyUp(KeyEvent::new(code, self.cur_mods));
                        self.app.update(ev);
                    }
                }
                _ => {}
            },

            WindowEvent::CursorMoved { position, .. } => {
                self.last_mouse = (position.x as u32, position.y as u32);
                let (x, y) = self.last_mouse;
                let (kind, button) = match &self.held_mouse_btn {
                    Some(btn) => (MouseEventKind::Drag, btn.clone()),
                    None => (MouseEventKind::Moved, MouseButton::None),
                };
                self.app
                    .update(Event::Mouse(MouseEvent { kind, x, y, button }));
            }

            WindowEvent::MouseInput { state, button, .. } => {
                let (x, y) = self.last_mouse;
                let btn = map_mouse_btn(button);
                if state == ElementState::Pressed {
                    self.held_mouse_btn = Some(btn.clone());
                    let cmd = self.app.update(Event::Mouse(MouseEvent {
                        kind: MouseEventKind::Down,
                        x,
                        y,
                        button: btn,
                    }));
                    if self.do_cmd(cmd) {
                        self.quit = true;
                        el.exit();
                    }
                } else {
                    self.held_mouse_btn = None;
                    let cmd = self.app.update(Event::Mouse(MouseEvent {
                        kind: MouseEventKind::Up,
                        x,
                        y,
                        button: btn,
                    }));
                    if self.do_cmd(cmd) {
                        self.quit = true;
                        el.exit();
                    }
                }
            }

            WindowEvent::MouseWheel { delta, .. } => {
                let (x, y) = self.last_mouse;
                match delta {
                    MouseScrollDelta::LineDelta(dx, dy) => {
                        let kind = if dy > 0.0 {
                            MouseEventKind::ScrollUp
                        } else {
                            MouseEventKind::ScrollDown
                        };
                        self.app.update(Event::Mouse(MouseEvent {
                            kind,
                            x,
                            y,
                            button: MouseButton::None,
                        }));
                        self.app.update(Event::Scroll { x: dx, y: dy });
                    }
                    MouseScrollDelta::PixelDelta(p) => {
                        let kind = if p.y > 0.0 {
                            MouseEventKind::ScrollUp
                        } else {
                            MouseEventKind::ScrollDown
                        };
                        self.app.update(Event::Mouse(MouseEvent {
                            kind,
                            x,
                            y,
                            button: MouseButton::None,
                        }));
                        self.app.update(Event::Scroll {
                            x: p.x as f32,
                            y: p.y as f32,
                        });
                    }
                    _ => {}
                }
            }

            WindowEvent::RedrawRequested => {
                self.render();
            }

            _ => {}
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if self.quit {
            el.exit();
            return;
        }

        let now = std::time::Instant::now();

        const REPEAT_DELAY_MS: u128 = 300;
        const REPEAT_INTERVAL_MS: u128 = 30;

        if let Some((ref code, mods, press_t, ref mut last_r)) = self.held_key {
            let elapsed_press = now.duration_since(press_t).as_millis();
            let elapsed_last = now.duration_since(*last_r).as_millis();
            if elapsed_press >= REPEAT_DELAY_MS && elapsed_last >= REPEAT_INTERVAL_MS {
                *last_r = now;
                let ev = Event::Key(KeyEvent::repeated(code.clone(), mods));
                let cmd = self.app.update(ev);
                if self.do_cmd(cmd) {
                    self.quit = true;
                    el.exit();
                    return;
                }
            }
        }

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

// ── Public API ────────────────────────────────────────────────────────────────

pub struct WinitBackend {
    font_data: Vec<u8>,
    size_px: f32,
    pending: std::collections::VecDeque<RawInput>,
    title: String,
    size: (u32, u32),
    resizable: bool,
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
            title: "trixui".into(),
            size: (1280, 800),
            resizable: true,
        })
    }

    pub fn title(mut self, t: impl Into<String>) -> Self {
        self.title = t.into();
        self
    }
    pub fn window_size(mut self, w: u32, h: u32) -> Self {
        self.size = (w, h);
        self
    }
    pub fn resizable(mut self, r: bool) -> Self {
        self.resizable = r;
        self
    }

    pub fn run_app<A: App>(self, mut app: A) -> crate::Result<()> {
        let atlas = GlyphAtlas::new(&self.font_data, None, None, self.size_px, 1.2)
            .map_err(|e| format!("GlyphAtlas: {e}"))?;
        let shaper = Shaper::new(&self.font_data);
        let init_cmd = app.init();

        let event_loop = EventLoop::new()?;
        event_loop.set_control_flow(ControlFlow::Poll);

        let mut handler = HandlerBuilder {
            app,
            font_data: self.font_data,
            size_px: self.size_px,
            atlas: Some(atlas),
            shaper: Some(shaper),
            gl_state: None,
            renderer: None,
            tick_ns: 1_000_000_000u64 / 60,
            last_tick: std::time::Instant::now(),
            last_mouse: (0, 0),
            cur_mods: KeyModifiers::NONE,
            quit: false,
            init_cmd: Some(init_cmd),
            held_key: None,
            held_mouse_btn: None,
            win_title: self.title,
            win_size: self.size,
            win_resizable: self.resizable,
        };

        event_loop.run_app(&mut handler)?;
        Ok(())
    }
}

// ── Backend trait impl ────────────────────────────────────────────────────────

impl Backend for WinitBackend {
    fn size(&self) -> (u32, u32) {
        (0, 0)
    }
    fn cell_size(&self) -> (u32, u32) {
        (6, 17)
    }
    fn natural_h(&self) -> u32 {
        0
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

    fn render(&mut self, _cmds: &[DrawCmd], _vp_w: u32, _vp_h: u32) {}
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
