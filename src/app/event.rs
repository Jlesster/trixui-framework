//! event.rs — unified input event types.

/// All events the app can receive in `update()`.
#[derive(Debug)]
pub enum Event<Msg> {
    /// A key was pressed (fires on both initial press AND key-repeat).
    Key(KeyEvent),
    /// A key was released.
    KeyUp(KeyEvent),
    /// A mouse action occurred.
    Mouse(MouseEvent),
    /// Smooth scroll delta in logical pixels (from touchpad or hi-res mouse).
    Scroll { x: f32, y: f32 },
    /// The viewport was resized.
    Resize(u32, u32),
    /// Regular tick (driven by `App::tick_rate()`).
    Tick,
    /// The window/compositor surface gained keyboard focus.
    FocusGained,
    /// The window/compositor surface lost keyboard focus.
    FocusLost,
    /// A user-defined message delivered via `Cmd::msg()`.
    Message(Msg),
}

// ── KeyEvent ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyEvent {
    pub code:      KeyCode,
    pub modifiers: KeyModifiers,
    /// True when this event is a key-repeat (key held down), false on initial press.
    pub repeat:    bool,
}

impl KeyEvent {
    pub fn new(code: KeyCode, modifiers: KeyModifiers) -> Self {
        Self { code, modifiers, repeat: false }
    }
    pub fn plain(code: KeyCode) -> Self {
        Self { code, modifiers: KeyModifiers::NONE, repeat: false }
    }
    pub fn repeated(code: KeyCode, modifiers: KeyModifiers) -> Self {
        Self { code, modifiers, repeat: true }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum KeyCode {
    Char(char),
    Enter,
    Backspace,
    Delete,
    Esc,
    Tab,
    BackTab,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    F(u8),
    Null,
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct KeyModifiers: u8 {
        const NONE  = 0b0000;
        const SHIFT = 0b0001;
        const CTRL  = 0b0010;
        const ALT   = 0b0100;
        const SUPER = 0b1000;
    }
}

// ── MouseEvent ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct MouseEvent {
    pub kind:   MouseEventKind,
    /// Pixel X from the top-left of the surface.
    pub x:      u32,
    /// Pixel Y from the top-left of the surface.
    pub y:      u32,
    pub button: MouseButton,
}

impl MouseEvent {
    /// Returns true if the event position lies inside `rect`.
    #[inline]
    pub fn in_rect(&self, x: u32, y: u32, w: u32, h: u32) -> bool {
        self.x >= x && self.x < x + w && self.y >= y && self.y < y + h
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MouseEventKind {
    Down,
    Up,
    /// Cursor movement while a button is held.
    Drag,
    /// Cursor movement with no button held.
    Moved,
    ScrollUp,
    ScrollDown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    None,
}
