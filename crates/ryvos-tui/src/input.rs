use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Actions produced by key input handling.
pub enum InputAction {
    /// Submit the current input buffer.
    Submit(String),
    /// Insert a newline (Shift+Enter).
    Newline,
    /// Quit the application.
    Quit,
    /// Clear the message history.
    Clear,
    /// Scroll up.
    ScrollUp,
    /// Scroll down.
    ScrollDown,
    /// Approve a pending approval request.
    Approve(String),
    /// Deny a pending approval request with optional reason.
    Deny(String, Option<String>),
    /// Run the soul interview (not supported in TUI).
    Soul,
    /// No-op (key was handled internally).
    None,
}

/// Input handler that manages the input buffer.
pub struct InputHandler {
    pub buffer: String,
    pub cursor: usize,
}

impl InputHandler {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            cursor: 0,
        }
    }

    /// Handle a key event, returning an action.
    pub fn handle_key(&mut self, key: KeyEvent) -> InputAction {
        match key.code {
            KeyCode::Enter => {
                if key.modifiers.contains(KeyModifiers::SHIFT) {
                    self.buffer.insert(self.cursor, '\n');
                    self.cursor += 1;
                    InputAction::Newline
                } else {
                    let text = self.buffer.clone();
                    self.buffer.clear();
                    self.cursor = 0;

                    // Handle commands
                    let trimmed = text.trim();
                    match trimmed {
                        "/quit" | "/exit" | "/q" => InputAction::Quit,
                        "/clear" => InputAction::Clear,
                        "/soul" => InputAction::Soul,
                        _ if trimmed.starts_with("/approve ") => {
                            let id = trimmed.strip_prefix("/approve ").unwrap().trim().to_string();
                            InputAction::Approve(id)
                        }
                        _ if trimmed.starts_with("/deny ") => {
                            let rest = trimmed.strip_prefix("/deny ").unwrap().trim();
                            let (id, reason) = match rest.split_once(' ') {
                                Some((id, reason)) => (id.to_string(), Some(reason.to_string())),
                                None => (rest.to_string(), None),
                            };
                            InputAction::Deny(id, reason)
                        }
                        _ if trimmed.is_empty() => InputAction::None,
                        _ => InputAction::Submit(text),
                    }
                }
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                InputAction::Quit
            }
            KeyCode::Char(c) => {
                self.buffer.insert(self.cursor, c);
                self.cursor += 1;
                InputAction::None
            }
            KeyCode::Backspace => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                    self.buffer.remove(self.cursor);
                }
                InputAction::None
            }
            KeyCode::Delete => {
                if self.cursor < self.buffer.len() {
                    self.buffer.remove(self.cursor);
                }
                InputAction::None
            }
            KeyCode::Left => {
                if self.cursor > 0 {
                    self.cursor -= 1;
                }
                InputAction::None
            }
            KeyCode::Right => {
                if self.cursor < self.buffer.len() {
                    self.cursor += 1;
                }
                InputAction::None
            }
            KeyCode::Home => {
                self.cursor = 0;
                InputAction::None
            }
            KeyCode::End => {
                self.cursor = self.buffer.len();
                InputAction::None
            }
            KeyCode::PageUp => InputAction::ScrollUp,
            KeyCode::PageDown => InputAction::ScrollDown,
            _ => InputAction::None,
        }
    }
}
