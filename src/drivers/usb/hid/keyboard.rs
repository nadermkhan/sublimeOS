//! USB HID Boot Keyboard Report Parser (HID 1.11 Boot Protocol).

/// Standard 8-byte USB HID Boot Keyboard Input Report.
#[repr(C, packed)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UsbKeyboardReport {
    pub modifiers: u8,
    pub reserved: u8,
    pub keycodes: [u8; 6],
}

impl UsbKeyboardReport {
    /// Parse a raw interrupt IN buffer into a `UsbKeyboardReport`.
    /// Handles both standard 8-byte HID Boot reports and Apple 9/10-byte Report-ID prefixed reports.
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < 3 {
            return None;
        }

        if bytes.len() >= 9 && (bytes[0] <= 0x02 || bytes.len() == 10) {
            let modifiers = bytes[1];
            let reserved = bytes[2];
            let mut keycodes = [0u8; 6];
            for i in 0..6 {
                if 3 + i < bytes.len() {
                    keycodes[i] = bytes[3 + i];
                }
            }
            Some(Self {
                modifiers,
                reserved,
                keycodes,
            })
        } else {
            let modifiers = bytes[0];
            let reserved = if bytes.len() > 1 { bytes[1] } else { 0 };
            let mut keycodes = [0u8; 6];
            for i in 0..6 {
                if 2 + i < bytes.len() {
                    keycodes[i] = bytes[2 + i];
                }
            }
            Some(Self {
                modifiers,
                reserved,
                keycodes,
            })
        }
    }
}

/// Stateful USB HID Boot Keyboard driver with rollover guard, modifiers, and TTY forwarding.
#[derive(Debug, Clone)]
pub struct UsbKeyboardDriver {
    prev_keys: [u8; 6],
    caps_lock: bool,
}

impl UsbKeyboardDriver {
    pub const fn new() -> Self {
        Self {
            prev_keys: [0; 6],
            caps_lock: false,
        }
    }

    pub fn reset(&mut self) {
        self.prev_keys = [0; 6];
    }

    /// Decode a Boot Keyboard input report, pushing newly pressed keys into CosmicOS TTY input.
    pub fn process_report(&mut self, report: &UsbKeyboardReport) {
        // Ignore HID ErrorRollOver (0x01), POSTFail (0x02), ErrorUndefined (0x03) phantom frames.
        if report.keycodes[0] >= 0x01 && report.keycodes[0] <= 0x03 {
            return;
        }

        let shift = (report.modifiers & 0x22) != 0; // Left Shift (bit 1) | Right Shift (bit 5)
        let ctrl = (report.modifiers & 0x11) != 0;  // Left Ctrl (bit 0) | Right Ctrl (bit 4)

        for &key in &report.keycodes {
            if key <= 0x03 || self.prev_keys.contains(&key) {
                continue;
            }
            if key == 0x39 {
                self.caps_lock = !self.caps_lock;
                continue;
            }

            if shift && key == 0x52 {
                crate::drivers::gop_console::scroll_up(1);
                continue;
            }
            if shift && key == 0x51 {
                crate::drivers::gop_console::scroll_down(1);
                continue;
            }
            if key == 0x4B {
                crate::drivers::gop_console::scroll_up(5);
                continue;
            }
            if key == 0x4E {
                crate::drivers::gop_console::scroll_down(5);
                continue;
            }

            if ctrl && (key == 0x2E || key == 0x57) { // '=' or Keypad '+'
                crate::drivers::gop_console::set_font_scale(2);
                continue;
            }
            if ctrl && (key == 0x2D || key == 0x56) { // '-' or Keypad '-'
                crate::drivers::gop_console::set_font_scale(1);
                continue;
            }

            if let Some(ch) = Self::map_usb_to_ascii_with_caps(key, shift, ctrl, self.caps_lock) {
                crate::drivers::gop_console::reset_scroll_view();
                crate::tty::push_input_char(ch);
            } else {
                Self::emit_navigation_sequence(key);
            }
        }

        self.prev_keys = report.keycodes;
    }

    /// Map a USB HID Usage ID (Page 0x07) to an ASCII character.
    pub fn map_usb_to_ascii(code: u8, shift: bool, ctrl: bool) -> Option<char> {
        Self::map_usb_to_ascii_with_caps(code, shift, ctrl, false)
    }

    fn map_usb_to_ascii_with_caps(
        code: u8,
        shift: bool,
        ctrl: bool,
        caps_lock: bool,
    ) -> Option<char> {
        if ctrl {
            if (0x04..=0x1D).contains(&code) {
                let ctrl_byte = (code - 0x04) + 1; // Ctrl+A (0x01) .. Ctrl+Z (0x1A)
                return Some(ctrl_byte as char);
            }
            if code == 0x2F {
                return Some('\x1b'); // Ctrl+[
            }
        }

        match code {
            0x04..=0x1D => {
                let base = b'a' + (code - 0x04);
                let upper = shift ^ caps_lock;
                Some(if upper { base.to_ascii_uppercase() } else { base } as char)
            }
            0x1E..=0x27 => {
                const NORMAL: [u8; 10] = *b"1234567890";
                const SHIFTED: [u8; 10] = *b"!@#$%^&*()";
                let idx = (code - 0x1E) as usize;
                Some((if shift { SHIFTED[idx] } else { NORMAL[idx] }) as char)
            }
            0x28 | 0x58 => Some('\n'), // Enter / Keypad Enter
            0x29 => Some('\x1b'),      // Escape
            0x2A => Some('\x08'),      // Backspace
            0x2B => Some('\t'),        // Tab
            0x2C => Some(' '),         // Space
            0x2D => Some(if shift { '_' } else { '-' }),
            0x2E => Some(if shift { '+' } else { '=' }),
            0x2F => Some(if shift { '{' } else { '[' }),
            0x30 => Some(if shift { '}' } else { ']' }),
            0x31 => Some(if shift { '|' } else { '\\' }),
            0x32 => Some(if shift { '~' } else { '#' }), // Non-US #/~
            0x33 => Some(if shift { ':' } else { ';' }),
            0x34 => Some(if shift { '"' } else { '\'' }),
            0x35 => Some(if shift { '~' } else { '`' }),
            0x36 => Some(if shift { '<' } else { ',' }),
            0x37 => Some(if shift { '>' } else { '.' }),
            0x38 => Some(if shift { '?' } else { '/' }),
            // Keypad digits & operators
            0x54 => Some('/'),
            0x55 => Some('*'),
            0x56 => Some('-'),
            0x57 => Some('+'),
            0x59..=0x61 => Some((b'1' + (code - 0x59)) as char),
            0x62 => Some('0'),
            0x63 => Some('.'),
            _ => None,
        }
    }

    fn emit_navigation_sequence(code: u8) {
        let seq: Option<&[u8]> = match code {
            0x4A => Some(b"\x1b[H"),  // Home
            0x4C => Some(b"\x1b[3~"), // Delete Forward
            0x4D => Some(b"\x1b[F"),  // End
            0x4F => Some(b"\x1b[C"),  // Right Arrow
            0x50 => Some(b"\x1b[D"),  // Left Arrow
            0x51 => Some(b"\x1b[B"),  // Down Arrow
            0x52 => Some(b"\x1b[A"),  // Up Arrow
            _ => None,
        };
        if let Some(bytes) = seq {
            for &b in bytes {
                crate::tty::push_input_char(b as char);
            }
        }
    }
}
