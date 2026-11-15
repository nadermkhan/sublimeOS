#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum MouseButton {
    Left = 1,
    Right = 2,
    Middle = 3,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum KeyCode {
    Char(char),
    Enter,
    Backspace,
    Delete,
    Tab,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Function(u8),
    Unknown(u16),
}

impl KeyCode {
    /// Convert a standard PC/SublimeOS key byte (`\n`, `\x08`, ASCII, etc.) into `KeyCode`.
    pub fn from_byte(b: u8) -> Self {
        match b {
            b'\n' | b'\r' => KeyCode::Enter,
            0x08 | 0x7F => KeyCode::Backspace,
            b'\t' => KeyCode::Tab,
            0x1B => KeyCode::Escape,
            0x20..=0x7E => KeyCode::Char(b as char),
            _ => KeyCode::Unknown(b as u16),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GuiEvent {
    MouseMove { x: i32, y: i32 },
    MouseDown { x: i32, y: i32, button: MouseButton },
    MouseUp { x: i32, y: i32, button: MouseButton },
    MouseWheel { x: i32, y: i32, delta_y: i32 },
    KeyDown { key: KeyCode, ch: Option<char>, mods: Modifiers },
    KeyUp { key: KeyCode, mods: Modifiers },
    FocusChanged { focused: bool },
    Resize { width: i32, height: i32 },
    Tick { now_ms: u64 },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EventResult {
    Ignored,
    Handled,
    NeedsRedraw,
    CloseRequested,
}

impl EventResult {
    #[inline]
    pub fn needs_redraw(self) -> bool {
        matches!(self, EventResult::NeedsRedraw)
    }

    #[inline]
    pub fn consumed(self) -> bool {
        !matches!(self, EventResult::Ignored)
    }

    #[inline]
    pub fn merge(&mut self, other: EventResult) {
        *self = match (*self, other) {
            (EventResult::CloseRequested, _) | (_, EventResult::CloseRequested) => {
                EventResult::CloseRequested
            }
            (EventResult::NeedsRedraw, _) | (_, EventResult::NeedsRedraw) => {
                EventResult::NeedsRedraw
            }
            (EventResult::Handled, _) | (_, EventResult::Handled) => EventResult::Handled,
            _ => EventResult::Ignored,
        };
    }
}
