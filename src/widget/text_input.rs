//! text_input.rs — single-line editable text widget.
//!
//! # Usage
//!
//! ```rust,ignore
//! // In your App state:
//! text_input: TextInputState::new(),
//!
//! // In update():
//! if let Event::Key(k) = &event {
//!     self.text_input.handle_key(k);
//! }
//!
//! // In view():
//! frame.render_stateful(
//!     TextInput::new().placeholder("Search…"),
//!     search_area,
//!     &mut self.text_input,
//! );
//! ```

use crate::layout::Rect;
use crate::renderer::{Color, PixelCanvas, TextStyle, Theme};
use crate::widget::{Style, StatefulWidget};
use crate::app::event::{KeyCode, KeyEvent, KeyModifiers};

// ── TextInputState ────────────────────────────────────────────────────────────

/// Persistent state for [`TextInput`].
#[derive(Debug, Clone, Default)]
pub struct TextInputState {
    /// Current content.
    pub value: String,
    /// Insertion cursor as a byte offset into `value`.
    cursor: usize,
    /// Horizontal scroll offset in characters (number of leading chars hidden).
    scroll: usize,
}

impl TextInputState {
    pub fn new() -> Self { Self::default() }

    /// Current content.
    pub fn value(&self) -> &str { &self.value }

    /// Set content programmatically, moving cursor to end.
    pub fn set_value(&mut self, s: impl Into<String>) {
        self.value = s.into();
        self.cursor = self.value.len();
        self.scroll = 0;
    }

    /// Clear content and reset cursor.
    pub fn clear(&mut self) {
        self.value.clear();
        self.cursor = 0;
        self.scroll = 0;
    }

    /// Process a key event. Returns `true` if the state changed.
    ///
    /// Call this from `App::update` for the focused input:
    /// ```rust,ignore
    /// if let Event::Key(k) = &event {
    ///     if self.input_focused {
    ///         self.text_input.handle_key(k);
    ///     }
    /// }
    /// ```
    pub fn handle_key(&mut self, k: &KeyEvent) -> bool {
        match &k.code {
            KeyCode::Char(c) => {
                // Ctrl+A — select all (move to end for now)
                if k.modifiers.contains(KeyModifiers::CTRL) {
                    match c {
                        'a' => { self.cursor = 0; return true; }
                        'e' => { self.cursor = self.value.len(); return true; }
                        'k' => { // kill to end of line
                            self.value.truncate(self.cursor);
                            return true;
                        }
                        'u' => { // kill to beginning of line
                            let rest = self.value[self.cursor..].to_string();
                            self.value = rest;
                            self.cursor = 0;
                            self.scroll = 0;
                            return true;
                        }
                        'w' => { // delete word backwards
                            return self.delete_word_back();
                        }
                        _ => return false,
                    }
                }
                self.insert_char(*c);
                true
            }
            KeyCode::Backspace => self.delete_back(),
            KeyCode::Delete    => self.delete_forward(),
            KeyCode::Left      => self.move_left(k.modifiers.contains(KeyModifiers::CTRL)),
            KeyCode::Right     => self.move_right(k.modifiers.contains(KeyModifiers::CTRL)),
            KeyCode::Home      => {
                let moved = self.cursor != 0;
                self.cursor = 0;
                self.scroll = 0;
                moved
            }
            KeyCode::End => {
                let target = self.value.len();
                let moved = self.cursor != target;
                self.cursor = target;
                moved
            }
            _ => false,
        }
    }

    // ── Cursor movement ───────────────────────────────────────────────────────

    fn insert_char(&mut self, c: char) {
        self.value.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    fn delete_back(&mut self) -> bool {
        if self.cursor == 0 { return false; }
        let prev = self.prev_char_boundary();
        self.value.drain(prev..self.cursor);
        self.cursor = prev;
        if self.scroll > 0 { self.scroll -= 1; }
        true
    }

    fn delete_forward(&mut self) -> bool {
        if self.cursor >= self.value.len() { return false; }
        let next = self.next_char_boundary();
        self.value.drain(self.cursor..next);
        true
    }

    fn delete_word_back(&mut self) -> bool {
        if self.cursor == 0 { return false; }
        let mut pos = self.cursor;
        // Skip trailing spaces
        while pos > 0 {
            let prev = self.char_before(pos);
            if self.value[prev..pos].chars().next() != Some(' ') { break; }
            pos = prev;
        }
        // Skip word chars
        while pos > 0 {
            let prev = self.char_before(pos);
            if self.value[prev..pos].chars().next() == Some(' ') { break; }
            pos = prev;
        }
        self.value.drain(pos..self.cursor);
        let deleted = self.cursor - pos;
        self.cursor = pos;
        self.scroll = self.scroll.saturating_sub(deleted);
        true
    }

    fn move_left(&mut self, by_word: bool) -> bool {
        if self.cursor == 0 { return false; }
        if by_word {
            let orig = self.cursor;
            while self.cursor > 0 {
                let prev = self.prev_char_boundary();
                self.cursor = prev;
                if self.cursor == 0 { break; }
                let p = self.prev_char_boundary();
                if self.value[p..self.cursor].chars().next() == Some(' ') { break; }
            }
            orig != self.cursor
        } else {
            self.cursor = self.prev_char_boundary();
            if self.cursor < self.scroll { self.scroll = self.cursor; }
            true
        }
    }

    fn move_right(&mut self, by_word: bool) -> bool {
        if self.cursor >= self.value.len() { return false; }
        if by_word {
            let orig = self.cursor;
            while self.cursor < self.value.len() {
                let next = self.next_char_boundary();
                self.cursor = next;
                if self.cursor >= self.value.len() { break; }
                let nn = self.next_char_boundary();
                if self.value[self.cursor..nn].chars().next() == Some(' ') { break; }
            }
            orig != self.cursor
        } else {
            self.cursor = self.next_char_boundary();
            true
        }
    }

    // ── Byte-index helpers ────────────────────────────────────────────────────

    fn prev_char_boundary(&self) -> usize {
        let mut p = self.cursor;
        loop {
            if p == 0 { return 0; }
            p -= 1;
            if self.value.is_char_boundary(p) { return p; }
        }
    }
    fn next_char_boundary(&self) -> usize {
        let mut p = self.cursor + 1;
        while p <= self.value.len() {
            if self.value.is_char_boundary(p) { return p; }
            p += 1;
        }
        self.value.len()
    }
    fn char_before(&self, pos: usize) -> usize {
        let mut p = pos;
        loop {
            if p == 0 { return 0; }
            p -= 1;
            if self.value.is_char_boundary(p) { return p; }
        }
    }

    /// Cursor position as a character index (for rendering).
    pub fn cursor_char_idx(&self) -> usize {
        self.value[..self.cursor].chars().count()
    }
}

// ── TextInput widget ──────────────────────────────────────────────────────────

/// Single-line editable text field.
pub struct TextInput<'a> {
    placeholder: &'a str,
    style:        Style,
    focused:      bool,
    max_len:      Option<usize>,
}

impl<'a> TextInput<'a> {
    pub fn new() -> Self {
        Self {
            placeholder: "",
            style: Style::default(),
            focused: false,
            max_len: None,
        }
    }

    pub fn placeholder(mut self, s: &'a str) -> Self {
        self.placeholder = s;
        self
    }
    pub fn style(mut self, s: Style) -> Self {
        self.style = s;
        self
    }
    pub fn focused(mut self, f: bool) -> Self {
        self.focused = f;
        self
    }
    pub fn max_len(mut self, n: usize) -> Self {
        self.max_len = Some(n);
        self
    }
}

impl<'a> Default for TextInput<'a> {
    fn default() -> Self { Self::new() }
}

impl<'a> StatefulWidget for TextInput<'a> {
    type State = TextInputState;

    fn render(
        self,
        canvas: &mut PixelCanvas,
        area: Rect,
        state: &mut TextInputState,
        cell_w: u32,
        cell_h: u32,
        t: &Theme,
    ) {
        if area.is_empty() { return; }

        let bg = self.style.bg.unwrap_or(t.normal_bg);
        let fg = self.style.fg.unwrap_or(t.normal_fg);

        // Background
        canvas.fill(area.x, area.y, area.w, area.h, bg);

        // Border — active/inactive
        let border_col = if self.focused { t.cursor_color } else { t.inactive_border };
        canvas.border(area.x, area.y, area.w, area.h,
            crate::renderer::BorderSide::ALL, border_col, 1);

        let inner = area.inset(1);
        if inner.is_empty() { return; }

        let max_cols = (inner.w / cell_w).max(1) as usize;

        // Compute visible window of text
        let char_count = state.value.chars().count();
        let cursor_char = state.cursor_char_idx();

        // Scroll so cursor is visible
        if cursor_char < state.scroll {
            state.scroll = cursor_char;
        }
        if cursor_char >= state.scroll + max_cols {
            state.scroll = cursor_char.saturating_sub(max_cols - 1);
        }

        let y_text = inner.y + inner.h.saturating_sub(cell_h) / 2;

        if state.value.is_empty() && !self.placeholder.is_empty() {
            // Placeholder
            let ph_ts = TextStyle {
                fg: t.dim_fg, bg, bold: false, italic: true,
            };
            canvas.text_maxw(inner.x, y_text, self.placeholder, ph_ts, inner.w);
        } else {
            // Visible slice of text
            let visible: String = state.value
                .chars()
                .skip(state.scroll)
                .take(max_cols)
                .collect();
            let ts = TextStyle { fg, bg: Color::TRANSPARENT, bold: self.style.bold, italic: self.style.italic };
            canvas.text_maxw(inner.x, y_text, &visible, ts, inner.w);
        }

        // Cursor bar
        if self.focused {
            let cursor_rel = cursor_char.saturating_sub(state.scroll);
            if cursor_rel <= max_cols {
                let cx = inner.x + cursor_rel as u32 * cell_w;
                canvas.fill(cx, inner.y, 2, inner.h, t.cursor_color);
            }
        }
    }
}
