extern crate alloc;
use alloc::collections::VecDeque;
use crate::arch::x86_64::idt::{register_irq_handler, InterruptContext};
use crate::arch::x86_64::io::{inb, outb};
use crate::sync::IrqSafeSpinlock;
use crate::kinfo;

const KEYBOARD_DATA_PORT: u16 = 0x60;
const KEYBOARD_STATUS_PORT: u16 = 0x64;
const KEYBOARD_CMD_PORT: u16 = 0x64;

pub struct KeyboardDriver {
    shift_pressed: bool,
    caps_lock: bool,
    ctrl_pressed: bool,
    alt_pressed: bool,
    extended: bool,
    buffer: VecDeque<u8>,
}

impl KeyboardDriver {
    pub const fn new() -> Self {
        Self {
            shift_pressed: false,
            caps_lock: false,
            ctrl_pressed: false,
            alt_pressed: false,
            extended: false,
            buffer: VecDeque::new(),
        }
    }

    pub fn handle_scancode(&mut self, scancode: u8) {
        if scancode == 0xE0 {
            self.extended = true;
            return;
        }

        if self.extended {
            self.extended = false;
            // Extended keys (release has bit 7 set)
            if (scancode & 0x80) != 0 {
                return;
            }

            match scancode {
                0x48 => {
                    if self.shift_pressed {
                        crate::drivers::gop_console::scroll_up(1);
                    } else {
                        self.push_escape_seq(b"[A");
                    }
                }
                0x50 => {
                    if self.shift_pressed {
                        crate::drivers::gop_console::scroll_down(1);
                    } else {
                        self.push_escape_seq(b"[B");
                    }
                }
                0x49 => crate::drivers::gop_console::scroll_up(5),   // Page Up
                0x51 => crate::drivers::gop_console::scroll_down(5), // Page Down
                0x4D => self.push_escape_seq(b"[C"),  // Right Arrow
                0x4B => self.push_escape_seq(b"[D"),  // Left Arrow
                0x47 => self.push_escape_seq(b"[H"),  // Home
                0x4F => self.push_escape_seq(b"[F"),  // End
                0x53 => self.push_escape_seq(b"[3~"), // Delete
                _ => {}
            }
            return;
        }

        match scancode {
            // Shift pressed
            0x2A | 0x36 => self.shift_pressed = true,
            // Shift released
            0xAA | 0xB6 => self.shift_pressed = false,
            // Ctrl pressed / released
            0x1D => self.ctrl_pressed = true,
            0x9D => self.ctrl_pressed = false,
            // Alt pressed / released
            0x38 => self.alt_pressed = true,
            0xB8 => self.alt_pressed = false,
            // CapsLock toggled
            0x3A => self.caps_lock = !self.caps_lock,

            // Alt + F1..F4 Virtual Console Switching
            0x3B if self.alt_pressed => crate::tty::switch_tty(1),
            0x3C if self.alt_pressed => crate::tty::switch_tty(2),
            0x3D if self.alt_pressed => crate::tty::switch_tty(3),
            0x3E if self.alt_pressed => crate::tty::switch_tty(4),

            // Ctrl + '=' (0x0D or keypad 0x4E): Zoom in (2x scale / 16x32)
            0x0D | 0x4E if self.ctrl_pressed => crate::drivers::gop_console::set_font_scale(2),
            // Ctrl + '-' (0x0C or keypad 0x4A): Zoom out (1x scale / 8x16)
            0x0C | 0x4A if self.ctrl_pressed => crate::drivers::gop_console::set_font_scale(1),

            // Key releases (break codes have bit 7 set)
            code if (code & 0x80) != 0 => {}

            // Key presses (make codes)
            code => {
                if let Some(ch) = self.scancode_to_char(code) {
                    crate::drivers::gop_console::reset_scroll_view();
                    if self.buffer.len() < 256 {
                        self.buffer.push_back(ch);
                    }
                }
            }
        }
    }

    fn push_escape_seq(&mut self, seq: &[u8]) {
        if self.buffer.len() + seq.len() + 1 < 256 {
            self.buffer.push_back(0x1B);
            for &b in seq {
                self.buffer.push_back(b);
            }
        }
    }

    fn scancode_to_char(&self, code: u8) -> Option<u8> {
        let uppercase = self.shift_pressed ^ self.caps_lock;
        let ch = match code {
            0x01 => 0x1B, // ESC
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

unsafe fn ps2_wait_write() -> bool {
    for _ in 0..100_000 {
        let st = inb(KEYBOARD_STATUS_PORT);
        if st == 0xFF {
            return false;
        }
        if (st & 0x02) == 0 {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

unsafe fn ps2_wait_read() -> bool {
    for _ in 0..100_000 {
        let st = inb(KEYBOARD_STATUS_PORT);
        if st == 0xFF {
            return false;
        }
        if (st & 0x01) != 0 {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

/// Drain all pending bytes from the 8042 output buffer (0x60) and route them
/// to the keyboard or mouse state machine based on the AUXB bit (bit 5 of 0x64).
pub fn poll_ps2_controller() -> bool {
    let mut received_key = false;
    for _ in 0..16 {
        let status = unsafe { inb(KEYBOARD_STATUS_PORT) };
        if status == 0xFF || (status & 0x01) == 0 {
            break;
        }
        let byte = unsafe { inb(KEYBOARD_DATA_PORT) };
        if (status & 0x20) != 0 {
            crate::drivers::mouse::handle_mouse_byte(byte);
        } else if byte != 0xFA && byte != 0xFE && byte != 0xAA {
            KEYBOARD.lock().handle_scancode(byte);
            received_key = true;
        }
    }
    received_key
}

fn keyboard_irq_handler(_ctx: &mut InterruptContext) {
    poll_ps2_controller();
}

pub fn init() {
    unsafe {
        let initial_status = inb(KEYBOARD_STATUS_PORT);
        if initial_status != 0xFF {
            // 1. Flush any stale bytes latched in 0x60 during BIOS/bootloader handoff
            for _ in 0..32 {
                if (inb(KEYBOARD_STATUS_PORT) & 0x01) == 0 {
                    break;
                }
                let _ = inb(KEYBOARD_DATA_PORT);
            }

            // 2. Enable first PS/2 port (0xAE)
            if ps2_wait_write() {
                outb(KEYBOARD_CMD_PORT, 0xAE);
            }

            // 3. Read 8042 Command Byte (0x20), enable IRQ1 (bit 0) + Set 1 translation (bit 6),
            //    and clear Port 1 clock disable (bit 4)
            if ps2_wait_write() {
                outb(KEYBOARD_CMD_PORT, 0x20);
                if ps2_wait_read() {
                    let mut cfg = inb(KEYBOARD_DATA_PORT);
                    cfg = (cfg | 0x41) & !0x10;
                    if ps2_wait_write() {
                        outb(KEYBOARD_CMD_PORT, 0x60);
                        if ps2_wait_write() {
                            outb(KEYBOARD_DATA_PORT, cfg);
                        }
                    }
                }
            }

            // 4. Send 0xF4 (Enable Scanning) to keyboard and drain ACK (0xFA)
            if ps2_wait_write() {
                outb(KEYBOARD_DATA_PORT, 0xF4);
                if ps2_wait_read() {
                    let _ = inb(KEYBOARD_DATA_PORT);
                }
            }

            // 5. Final drain so OBF (bit 0) is guaranteed 0 before unmasking edge-triggered IRQ 1
            for _ in 0..16 {
                if (inb(KEYBOARD_STATUS_PORT) & 0x01) == 0 {
                    break;
                }
                let _ = inb(KEYBOARD_DATA_PORT);
            }
        }
    }

    register_irq_handler(1, keyboard_irq_handler);
    crate::arch::x86_64::ioapic::unmask_isa_irq(1);
    kinfo!("PS/2 Keyboard driver initialized (IRQ 1 routed via I/O APIC -> Vector 33)");
}

/// Non-blocking read of the next typed character (checks buffer, then polls PS/2, USB HID, and UART).
pub fn read_char() -> Option<u8> {
    if let Some(ch) = KEYBOARD.lock().buffer.pop_front() {
        return Some(ch);
    }

    // Poll PS/2 8042 hardware in case OBF was latched or CPU is running with IF=0 inside a syscall
    poll_ps2_controller();

    // Poll USB HID host controllers (works even when LAPIC timer IRQ is masked during syscalls)
    crate::drivers::usb::poll_hid_nonblocking();

    // Check COM1 serial input fallback
    if let Some(ser_ch) = crate::arch::x86_64::serial::read_rx_char() {
        let normalized = if ser_ch == b'\r' { b'\n' } else { ser_ch };
        push_char(normalized);
    }

    KEYBOARD.lock().buffer.pop_front()
}

/// Non-blocking read of the next typed character during panic recovery (avoiding deadlocks).
pub fn read_char_panic() -> Option<u8> {
    let mut kbd = if let Some(guard) = KEYBOARD.try_lock() {
        guard
    } else {
        unsafe { KEYBOARD.force_unlock(); }
        KEYBOARD.lock()
    };
    kbd.buffer.pop_front()
}

/// Push an ASCII character into the unified keyboard input ring buffer (used by USB HID / serial bridges).
pub fn push_char(ch: u8) {
    let mut kbd = KEYBOARD.lock();
    if kbd.buffer.len() < 256 {
        kbd.buffer.push_back(ch);
    }
}

/// Blocking read of the next typed character (safe both with IF=1 and inside IF=0 syscall context).
pub fn get_char() -> u8 {
    loop {
        if let Some(ch) = read_char() {
            return ch;
        }
        if !crate::arch::x86_64::registers::are_interrupts_enabled() {
            crate::arch::x86_64::pit::delay_us(50);
        } else {
            core::hint::spin_loop();
        }
    }
}
