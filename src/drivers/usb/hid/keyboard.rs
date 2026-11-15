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

const USB_TYPEMATIC_DELAY_MS: u64 = 450;
const USB_TYPEMATIC_INTERVAL_MS: u64 = 33;

/// Stateful USB HID Boot Keyboard driver with rollover guard, modifiers, typematic repeat, and TTY forwarding.
#[derive(Debug, Clone)]
pub struct UsbKeyboardDriver {
    prev_keys: [u8; 6],
    prev_modifiers: u8,
    super_used_combo: bool,
    caps_lock: bool,
    held_key: Option<u8>,
    held_shift: bool,
    held_ctrl: bool,
    next_repeat_ms: u64,
}

impl UsbKeyboardDriver {
    pub const fn new() -> Self {
        Self {
            prev_keys: [0; 6],
            prev_modifiers: 0,
            super_used_combo: false,
            caps_lock: false,
            held_key: None,
            held_shift: false,
            held_ctrl: false,
            next_repeat_ms: 0,
        }
    }

    pub fn reset(&mut self) {
        self.prev_keys = [0; 6];
        self.prev_modifiers = 0;
        self.super_used_combo = false;
        self.held_key = None;
        self.next_repeat_ms = 0;
    }

    #[inline]
    fn is_repeatable_usage(key: u8) -> bool {
        key > 0x03 && key != 0x39 && !(0x3A..=0x45).contains(&key)
    }

    fn emit_usage(&self, key: u8, shift: bool, ctrl: bool) {
        let gui_active = crate::drivers::gop_console::is_gui_active();
        if shift && key == 0x52 && !gui_active {
            crate::drivers::gop_console::scroll_up(1);
            return;
        }
        if shift && key == 0x51 && !gui_active {
            crate::drivers::gop_console::scroll_down(1);
            return;
        }
        if key == 0x4B {
            if gui_active {
                for &b in b"\x1b[5~" {
                    crate::tty::push_input_char(b as char);
                }
            } else {
                crate::drivers::gop_console::scroll_up(5);
            }
            return;
        }
        if key == 0x4E {
            if gui_active {
                for &b in b"\x1b[6~" {
                    crate::tty::push_input_char(b as char);
                }
            } else {
                crate::drivers::gop_console::scroll_down(5);
            }
            return;
        }

        if ctrl && (key == 0x2E || key == 0x57) {
            crate::drivers::gop_console::set_font_scale(2);
            return;
        }
        if ctrl && (key == 0x2D || key == 0x56) {
            crate::drivers::gop_console::set_font_scale(1);
            return;
        }

        if let Some(ch) = Self::map_usb_to_ascii_with_caps(key, shift, ctrl, self.caps_lock) {
            crate::drivers::gop_console::reset_scroll_view();
            crate::tty::push_input_char(ch);
        } else {
            Self::emit_navigation_sequence(key);
        }
    }

    fn usage_to_keycode(code: u8) -> Option<libgui::event::KeyCode> {
        if let Some(ch) = Self::map_usb_to_ascii_with_caps(code, false, false, false) {
            return Some(libgui::event::KeyCode::from_byte(ch as u8));
        }
        match code {
            0x4A => Some(libgui::event::KeyCode::Home),
            0x4B => Some(libgui::event::KeyCode::PageUp),
            0x4C => Some(libgui::event::KeyCode::Delete),
            0x4D => Some(libgui::event::KeyCode::End),
            0x4E => Some(libgui::event::KeyCode::PageDown),
            0x4F => Some(libgui::event::KeyCode::Right),
            0x50 => Some(libgui::event::KeyCode::Left),
            0x51 => Some(libgui::event::KeyCode::Down),
            0x52 => Some(libgui::event::KeyCode::Up),
            _ => None,
        }
    }

    /// Decode a Boot Keyboard input report, pushing newly pressed keys into SublimeOS input.
    pub fn process_report(&mut self, report: &UsbKeyboardReport) {
        let now_ms = crate::arch::x86_64::pit::get_ticks();
        self.process_report_at(report, now_ms);
    }

    pub fn process_report_at(&mut self, report: &UsbKeyboardReport, now_ms: u64) {
        // Ignore HID ErrorRollOver (0x01), POSTFail (0x02), ErrorUndefined (0x03) phantom frames.
        if report.keycodes[0] >= 0x01 && report.keycodes[0] <= 0x03 {
            return;
        }

        let shift = (report.modifiers & 0x22) != 0; // Left Shift (bit 1) | Right Shift (bit 5)
        let ctrl = (report.modifiers & 0x11) != 0;  // Left Ctrl (bit 0) | Right Ctrl (bit 4)
        let alt = (report.modifiers & 0x44) != 0;   // Left Alt (bit 2) | Right Alt (bit 6)
        let super_down = (report.modifiers & 0x88) != 0; // Left GUI (bit 3) | Right GUI (bit 7)
        let prev_super_down = (self.prev_modifiers & 0x88) != 0;
        let gui_active = crate::drivers::gop_console::is_gui_active();
        let mods = libgui::event::Modifiers { shift, ctrl, alt };

        if prev_super_down && !super_down {
            if gui_active && !self.super_used_combo {
                crate::drivers::keyboard::push_gui_event(
                    crate::drivers::keyboard::GuiInputEvent::Shortcut(
                        crate::drivers::keyboard::SystemShortcut::ToggleStartMenu,
                    ),
                );
            }
            self.super_used_combo = false;
        } else if !prev_super_down && super_down {
            self.super_used_combo = false;
        }

        if let Some(held) = self.held_key {
            if !report.keycodes.contains(&held) {
                self.held_key = None;
            } else {
                self.held_shift = shift;
                self.held_ctrl = ctrl;
            }
        }

        if gui_active {
            for &old_k in &self.prev_keys {
                if old_k > 0x03 && old_k != 0x39 && !report.keycodes.contains(&old_k) {
                    if let Some(kc) = Self::usage_to_keycode(old_k) {
                        crate::drivers::keyboard::push_gui_event(
                            crate::drivers::keyboard::GuiInputEvent::KeyUp { key: kc, mods },
                        );
                    }
                }
            }
        }

        for &key in &report.keycodes {
            if key <= 0x03 || self.prev_keys.contains(&key) {
                continue;
            }
            if key == 0x39 {
                self.caps_lock = !self.caps_lock;
                continue;
            }

            if super_down {
                self.super_used_combo = true;
                if gui_active && key == 0x15 {
                    crate::drivers::keyboard::push_gui_event(
                        crate::drivers::keyboard::GuiInputEvent::Shortcut(
                            crate::drivers::keyboard::SystemShortcut::ToggleSpotlight,
                        ),
                    );
                    continue;
                }
                if gui_active && key == 0x07 {
                    crate::drivers::keyboard::push_gui_event(
                        crate::drivers::keyboard::GuiInputEvent::Shortcut(
                            crate::drivers::keyboard::SystemShortcut::ShowDesktop,
                        ),
                    );
                    continue;
                }
            }

            if gui_active {
                if alt && key == 0x2B {
                    crate::drivers::keyboard::push_gui_event(
                        crate::drivers::keyboard::GuiInputEvent::Shortcut(
                            crate::drivers::keyboard::SystemShortcut::AltTab,
                        ),
                    );
                    continue;
                }
                if alt && key == 0x3D {
                    crate::drivers::keyboard::push_gui_event(
                        crate::drivers::keyboard::GuiInputEvent::Shortcut(
                            crate::drivers::keyboard::SystemShortcut::AltF4,
                        ),
                    );
                    continue;
                }
                if ctrl && key == 0x29 {
                    crate::drivers::keyboard::push_gui_event(
                        crate::drivers::keyboard::GuiInputEvent::Shortcut(
                            crate::drivers::keyboard::SystemShortcut::ToggleStartMenu,
                        ),
                    );
                    continue;
                }
                if (0x3A..=0x45).contains(&key) {
                    let fnum = (key - 0x3A) + 1;
                    crate::drivers::keyboard::push_gui_event(
                        crate::drivers::keyboard::GuiInputEvent::FunctionKey { num: fnum, mods },
                    );
                    continue;
                }
                if let Some((kc, ch_opt)) = match key {
                    0x0B if ctrl => Some((libgui::event::KeyCode::Char('h'), Some('h'))),
                    0x11 if ctrl && shift => Some((libgui::event::KeyCode::Char('n'), Some('n'))),
                    0x2B if shift && !ctrl && !alt => Some((libgui::event::KeyCode::Tab, None)),
                    0x07 if alt => Some((libgui::event::KeyCode::Char('d'), Some('d'))),
                    0x28 | 0x58 if alt => Some((libgui::event::KeyCode::Enter, None)),
                    0x4F if alt => Some((libgui::event::KeyCode::Right, None)),
                    0x50 if alt => Some((libgui::event::KeyCode::Left, None)),
                    0x52 if alt => Some((libgui::event::KeyCode::Up, None)),
                    _ => None,
                } {
                    crate::drivers::keyboard::push_gui_event(
                        crate::drivers::keyboard::GuiInputEvent::KeyDown {
                            key: kc,
                            ch: ch_opt,
                            mods,
                        },
                    );
                    continue;
                }
            }

            self.emit_usage(key, shift, ctrl);

            if Self::is_repeatable_usage(key) && !(ctrl && matches!(key, 0x2E | 0x57 | 0x2D | 0x56)) {
                self.held_key = Some(key);
                self.held_shift = shift;
                self.held_ctrl = ctrl;
                self.next_repeat_ms = now_ms.saturating_add(USB_TYPEMATIC_DELAY_MS);
            }
        }

        self.prev_keys = report.keycodes;
        self.prev_modifiers = report.modifiers;
    }

    /// Emit typematic auto-repeat events while a USB HID key stays pressed (`SET_IDLE(0)` compatible).
    pub fn tick_repeat(&mut self, now_ms: u64) -> bool {
        let Some(key) = self.held_key else {
            return false;
        };
        if now_ms < self.next_repeat_ms {
            return false;
        }
        self.emit_usage(key, self.held_shift, self.held_ctrl);
        self.next_repeat_ms = now_ms.saturating_add(USB_TYPEMATIC_INTERVAL_MS);
        true
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
