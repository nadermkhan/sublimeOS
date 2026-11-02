extern crate alloc;
use alloc::collections::VecDeque;
use crate::arch::x86_64::idt::{register_irq_handler, InterruptContext};
use crate::arch::x86_64::io::inb;
use crate::arch::x86_64::pic::unmask;
use crate::sync::IrqSafeSpinlock;
use crate::kinfo;

const KEYBOARD_DATA_PORT: u16 = 0x60;

pub struct KeyboardDriver {
    shift_pressed: bool,
    caps_lock: bool,
    ctrl_pressed: bool,
    buffer: VecDeque<u8>,
}

impl KeyboardDriver {
    pub const fn new() -> Self {
        Self {
            shift_pressed: false,
            caps_lock: false,
            ctrl_pressed: false,
            buffer: VecDeque::new(),
        }
    }

    pub fn handle_scancode(&mut self, scancode: u8) {
        match scancode {
            // Shift pressed
            0x2A | 0x36 => self.shift_pressed = true,
            // Shift released
            0xAA | 0xB6 => self.shift_pressed = false,
            // Ctrl pressed
            0x1D => self.ctrl_pressed = true,
            // Ctrl released
            0x9D => self.ctrl_pressed = false,
            // CapsLock toggled
            0x3A => self.caps_lock = !self.caps_lock,
            // Key releases (break codes have bit 7 set)
            code if code >= 0x80 => {}
            // Key presses (make codes)
            code => {
                if let Some(ch) = self.scancode_to_char(code) {
                    if self.buffer.len() < 256 {
                        self.buffer.push_back(ch);
                    }
                }
            }
        }
    }

    fn scancode_to_char(&self, code: u8) -> Option<u8> {
        let uppercase = self.shift_pressed ^ self.caps_lock;
        let ch = match code {
            0x01 => return None, // ESC
            0x02 => if self.shift_pressed { b'!' } else { b'1' },
            0x03 => if self.shift_pressed { b'@' } else { b'2' },
            0x04 => if self.shift_pressed { b'#' } else { b'3' },
            0x05 => if self.shift_pressed { b'$' } else { b'4' },
            0x06 => if self.shift_pressed { b'%' } else { b'5' },
            0x07 => if self.shift_pressed { b'^' } else { b'6' },
            0x08 => if self.shift_pressed { b'&' } else { b'7' },
            0x09 => if self.shift_pressed { b'*' } else { b'8' },
            0x0A => if self.shift_pressed { b'(' } else { b'9' },
            0x0B => if self.shift_pressed { b')' } else { b'0' },
            0x0C => if self.shift_pressed { b'_' } else { b'-' },
            0x0D => if self.shift_pressed { b'+' } else { b'=' },
            0x0E => 0x08, // Backspace
            0x0F => b'\t', // Tab

            0x10 => if uppercase { b'Q' } else { b'q' },
            0x11 => if uppercase { b'W' } else { b'w' },
            0x12 => if uppercase { b'E' } else { b'e' },
            0x13 => if uppercase { b'R' } else { b'r' },
            0x14 => if uppercase { b'T' } else { b't' },
            0x15 => if uppercase { b'Y' } else { b'y' },
            0x16 => if uppercase { b'U' } else { b'u' },
            0x17 => if uppercase { b'I' } else { b'i' },
            0x18 => if uppercase { b'O' } else { b'o' },
            0x19 => if uppercase { b'P' } else { b'p' },
            0x1A => if self.shift_pressed { b'{' } else { b'[' },
            0x1B => if self.shift_pressed { b'}' } else { b']' },
            0x1C => b'\n', // Enter

            0x1E => if uppercase { b'A' } else { b'a' },
            0x1F => if uppercase { b'S' } else { b's' },
            0x20 => if uppercase { b'D' } else { b'd' },
            0x21 => if uppercase { b'F' } else { b'f' },
            0x22 => if uppercase { b'G' } else { b'g' },
            0x23 => if uppercase { b'H' } else { b'h' },
            0x24 => if uppercase { b'J' } else { b'j' },
            0x25 => if uppercase { b'K' } else { b'k' },
            0x26 => if uppercase { b'L' } else { b'l' },
            0x27 => if self.shift_pressed { b':' } else { b';' },
            0x28 => if self.shift_pressed { b'"' } else { b'\'' },
            0x29 => if self.shift_pressed { b'~' } else { b'`' },

            0x2B => if self.shift_pressed { b'|' } else { b'\\' },
            0x2C => if uppercase { b'Z' } else { b'z' },
            0x2D => if uppercase { b'X' } else { b'x' },
            0x2E => if uppercase { b'C' } else { b'c' },
            0x2F => if uppercase { b'V' } else { b'v' },
            0x30 => if uppercase { b'B' } else { b'b' },
            0x31 => if uppercase { b'N' } else { b'n' },
            0x32 => if uppercase { b'M' } else { b'm' },
            0x33 => if self.shift_pressed { b'<' } else { b',' },
            0x34 => if self.shift_pressed { b'>' } else { b'.' },
            0x35 => if self.shift_pressed { b'?' } else { b'/' },
            0x39 => b' ', // Space

            _ => return None,
        };

        if self.ctrl_pressed && ch.is_ascii_alphabetic() {
            // Control character (e.g. Ctrl+C = 0x03)
            Some((ch.to_ascii_uppercase() - b'@') as u8)
        } else {
            Some(ch)
        }
    }
}

pub static KEYBOARD: IrqSafeSpinlock<KeyboardDriver> = IrqSafeSpinlock::new(KeyboardDriver::new());

fn keyboard_irq_handler(_ctx: &mut InterruptContext) {
    let scancode = unsafe { inb(KEYBOARD_DATA_PORT) };
    KEYBOARD.lock().handle_scancode(scancode);
}

pub fn init() {
    register_irq_handler(1, keyboard_irq_handler);
    unmask(1);
    kinfo!("PS/2 Keyboard driver initialized (IRQ 1 unmasked)");
}

/// Non-blocking read of the next typed character.
pub fn read_char() -> Option<u8> {
    KEYBOARD.lock().buffer.pop_front()
}

/// Blocking read of the next typed character.
pub fn get_char() -> u8 {
    loop {
        if let Some(ch) = read_char() {
            return ch;
        }
        core::hint::spin_loop();
    }
}
