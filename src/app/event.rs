//! event.rs — unified input event types.

/// All events the app can receive in `update()`.
pub enum Event<Msg> {
    /// A key was pressed.
    Key(KeyEvent),
    /// A mouse action occurred.
    Mouse(MouseEvent),
    /// The viewport was resized.
    Resize(u32, u32),
    /// Regular tick (driven by `App::tick_rate()`).
    Tick,
    /// A user-defined message delivered via `Cmd::msg()`.
    Message(Msg),
}

// ── KeyEvent ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyEvent {
    pub code:      KeyCode,
    pub modifiers: KeyModifiers,
}

impl KeyEvent {
    pub fn new(code: KeyCode, modifiers: KeyModifiers) -> Self {
        Self { code, modifiers }
    }
    pub fn plain(code: KeyCode) -> Self {
        Self { code, modifiers: KeyModifiers::NONE }
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MouseEvent {
    pub kind:   MouseEventKind,
    pub x:      u32,
    pub y:      u32,
    pub button: MouseButton,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MouseEventKind {
    Down,
    Up,
    Drag,
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
