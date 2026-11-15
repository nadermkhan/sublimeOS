extern crate alloc;
use alloc::collections::VecDeque;
use crate::arch::x86_64::idt::{register_irq_handler, InterruptContext};
use crate::arch::x86_64::io::{inb, outb};
use crate::sync::IrqSafeSpinlock;
use crate::kinfo;

const KEYBOARD_DATA_PORT: u16 = 0x60;
const KEYBOARD_STATUS_PORT: u16 = 0x64;
const KEYBOARD_CMD_PORT: u16 = 0x64;

const TYPEMATIC_DELAY_MS: u64 = 450;
const TYPEMATIC_INTERVAL_MS: u64 = 33;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemShortcut {
    ToggleStartMenu,
    AltTab,
    AltF4,
    ToggleSpotlight,
    ShowDesktop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuiInputEvent {
    Shortcut(SystemShortcut),
    FunctionKey {
        num: u8,
        mods: libgui::event::Modifiers,
    },
    KeyDown {
        key: libgui::event::KeyCode,
        ch: Option<char>,
        mods: libgui::event::Modifiers,
    },
    KeyUp {
        key: libgui::event::KeyCode,
        mods: libgui::event::Modifiers,
    },
}

static GUI_EVENT_QUEUE: IrqSafeSpinlock<VecDeque<GuiInputEvent>> = IrqSafeSpinlock::new(VecDeque::new());

pub fn push_gui_event(ev: GuiInputEvent) {
    let mut q = GUI_EVENT_QUEUE.lock();
    if q.len() < 128 {
        q.push_back(ev);
    }
}

pub fn pop_gui_event() -> Option<GuiInputEvent> {
    GUI_EVENT_QUEUE.lock().pop_front()
}

pub struct KeyboardDriver {
    shift_pressed: bool,
    caps_lock: bool,
    ctrl_pressed: bool,
    alt_pressed: bool,
    super_pressed: bool,
    super_used_combo: bool,
    extended: bool,
    held_key: Option<(u8, bool)>,
    next_repeat_ms: u64,
    buffer: VecDeque<u8>,
}

impl KeyboardDriver {
    pub const fn new() -> Self {
        Self {
            shift_pressed: false,
            caps_lock: false,
            ctrl_pressed: false,
            alt_pressed: false,
            super_pressed: false,
            super_used_combo: false,
            extended: false,
            held_key: None,
            next_repeat_ms: 0,
            buffer: VecDeque::new(),
        }
    }

    #[inline]
    fn current_mods(&self) -> libgui::event::Modifiers {
        libgui::event::Modifiers {
            shift: self.shift_pressed,
            ctrl: self.ctrl_pressed,
            alt: self.alt_pressed,
        }
    }

    pub fn handle_scancode(&mut self, scancode: u8) {
        let now_ms = crate::arch::x86_64::pit::get_ticks();
        self.handle_scancode_at(scancode, now_ms);
    }

    pub fn handle_scancode_at(&mut self, scancode: u8, now_ms: u64) {
        if scancode == 0xE0 {
            self.extended = true;
            return;
        }

        let gui_active = crate::drivers::gop_console::is_gui_active();

        if self.extended {
            self.extended = false;
            let base = scancode & 0x7F;
            if (scancode & 0x80) != 0 {
                match base {
                    0x1D => self.ctrl_pressed = false,
                    0x38 => self.alt_pressed = false,
                    0x5B | 0x5C => {
                        let fire_tap = self.super_pressed && !self.super_used_combo;
                        self.super_pressed = false;
                        self.super_used_combo = false;
                        if gui_active && fire_tap {
                            push_gui_event(GuiInputEvent::Shortcut(SystemShortcut::ToggleStartMenu));
                        }
                    }
                    _ => {}
                }
                if self.held_key == Some((base, true)) {
                    self.held_key = None;
                }
                if gui_active {
                    if let Some(kc) = Self::extended_scancode_to_keycode(base) {
                        push_gui_event(GuiInputEvent::KeyUp {
                            key: kc,
                            mods: self.current_mods(),
                        });
                    }
                }
                return;
            }

            match scancode {
                0x1D => {
                    self.ctrl_pressed = true;
                    return;
                }
                0x38 => {
                    self.alt_pressed = true;
                    return;
                }
                0x5B | 0x5C => {
                    self.super_pressed = true;
                    self.super_used_combo = false;
                    return;
                }
                _ => {}
            }

            if self.super_pressed {
                self.super_used_combo = true;
            }
            if gui_active && self.alt_pressed {
                if let Some(kc) = match scancode {
                    0x48 => Some(libgui::event::KeyCode::Up),
                    0x4B => Some(libgui::event::KeyCode::Left),
                    0x4D => Some(libgui::event::KeyCode::Right),
                    _ => None,
                } {
                    self.held_key = None;
                    push_gui_event(GuiInputEvent::KeyDown {
                        key: kc,
                        ch: None,
                        mods: self.current_mods(),
                    });
                    return;
                }
            }
            if self.emit_extended(scancode, gui_active) {
                self.arm_repeat((scancode, true), now_ms);
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

            // Alt+Tab / Alt+F4 / Ctrl+Esc / Win+R / Win+D real-OS global shortcuts in GUI mode
            0x0F if gui_active && self.alt_pressed => {
                self.held_key = None;
                push_gui_event(GuiInputEvent::Shortcut(SystemShortcut::AltTab));
            }
            0x3E if gui_active && self.alt_pressed => {
                self.held_key = None;
                push_gui_event(GuiInputEvent::Shortcut(SystemShortcut::AltF4));
            }
            0x01 if gui_active && self.ctrl_pressed => {
                self.held_key = None;
                push_gui_event(GuiInputEvent::Shortcut(SystemShortcut::ToggleStartMenu));
            }
            0x13 if gui_active && self.super_pressed => {
                self.super_used_combo = true;
                self.held_key = None;
                push_gui_event(GuiInputEvent::Shortcut(SystemShortcut::ToggleSpotlight));
            }
            0x20 if gui_active && self.super_pressed => {
                self.super_used_combo = true;
                self.held_key = None;
                push_gui_event(GuiInputEvent::Shortcut(SystemShortcut::ShowDesktop));
            }

            // Alt + F1..F4 Virtual Console Switching (CLI / text mode)
            0x3B if self.alt_pressed && !gui_active => {
                self.held_key = None;
                crate::tty::switch_tty(1);
            }
            0x3C if self.alt_pressed && !gui_active => {
                self.held_key = None;
                crate::tty::switch_tty(2);
            }
            0x3D if self.alt_pressed && !gui_active => {
                self.held_key = None;
                crate::tty::switch_tty(3);
            }
            0x3E if self.alt_pressed && !gui_active => {
                self.held_key = None;
                crate::tty::switch_tty(4);
            }

            // F1..F12 in GUI mode
            c @ (0x3B..=0x44 | 0x57 | 0x58) if gui_active => {
                self.held_key = None;
                let num = match c {
                    0x3B..=0x44 => (c - 0x3B) + 1,
                    0x57 => 11,
                    _ => 12,
                };
                push_gui_event(GuiInputEvent::FunctionKey {
                    num,
                    mods: self.current_mods(),
                });
            }

            // Ctrl + '=' (0x0D or keypad 0x4E): Zoom in (2x scale / 16x32)
            0x0D | 0x4E if self.ctrl_pressed => crate::drivers::gop_console::set_font_scale(2),
            // Ctrl + '-' (0x0C or keypad 0x4A): Zoom out (1x scale / 8x16)
            0x0C | 0x4A if self.ctrl_pressed => crate::drivers::gop_console::set_font_scale(1),

            // Key releases (break codes have bit 7 set)
            code if (code & 0x80) != 0 => {
                let base = code & 0x7F;
                if self.held_key == Some((base, false)) {
                    self.held_key = None;
                }
                if gui_active {
                    if let Some(kc) = self.normal_scancode_to_keycode(base) {
                        push_gui_event(GuiInputEvent::KeyUp {
                            key: kc,
                            mods: self.current_mods(),
                        });
                    }
                }
            }

            // GUI modifier chords (Ctrl+H, Ctrl+Shift+N, Shift+Tab, Alt+D, Alt+Enter)
            0x23 if gui_active && self.ctrl_pressed => {
                self.held_key = None;
                push_gui_event(GuiInputEvent::KeyDown {
                    key: libgui::event::KeyCode::Char('h'),
                    ch: Some('h'),
                    mods: self.current_mods(),
                });
            }
            0x31 if gui_active && self.ctrl_pressed && self.shift_pressed => {
                self.held_key = None;
                push_gui_event(GuiInputEvent::KeyDown {
                    key: libgui::event::KeyCode::Char('n'),
                    ch: Some('n'),
                    mods: self.current_mods(),
                });
            }
            0x0F if gui_active && self.shift_pressed && !self.ctrl_pressed && !self.alt_pressed => {
                self.held_key = None;
                push_gui_event(GuiInputEvent::KeyDown {
                    key: libgui::event::KeyCode::Tab,
                    ch: None,
                    mods: self.current_mods(),
                });
            }
            0x20 if gui_active && self.alt_pressed => {
                self.held_key = None;
                push_gui_event(GuiInputEvent::KeyDown {
                    key: libgui::event::KeyCode::Char('d'),
                    ch: Some('d'),
                    mods: self.current_mods(),
                });
            }
            0x1C if gui_active && self.alt_pressed => {
                self.held_key = None;
                push_gui_event(GuiInputEvent::KeyDown {
                    key: libgui::event::KeyCode::Enter,
                    ch: None,
                    mods: self.current_mods(),
                });
            }

            // Key presses (make codes)
            code => {
                if self.super_pressed {
                    self.super_used_combo = true;
                }
                if self.emit_normal(code) {
                    self.arm_repeat((code, false), now_ms);
                }
            }
        }
    }

    fn arm_repeat(&mut self, key: (u8, bool), now_ms: u64) {
        if self.held_key == Some(key) {
            self.next_repeat_ms = now_ms.saturating_add(TYPEMATIC_INTERVAL_MS);
        } else {
            self.held_key = Some(key);
            self.next_repeat_ms = now_ms.saturating_add(TYPEMATIC_DELAY_MS);
        }
    }

    fn extended_scancode_to_keycode(base: u8) -> Option<libgui::event::KeyCode> {
        match base {
            0x48 => Some(libgui::event::KeyCode::Up),
            0x50 => Some(libgui::event::KeyCode::Down),
            0x4B => Some(libgui::event::KeyCode::Left),
            0x4D => Some(libgui::event::KeyCode::Right),
            0x47 => Some(libgui::event::KeyCode::Home),
            0x4F => Some(libgui::event::KeyCode::End),
            0x49 => Some(libgui::event::KeyCode::PageUp),
            0x51 => Some(libgui::event::KeyCode::PageDown),
            0x53 => Some(libgui::event::KeyCode::Delete),
            _ => None,
        }
    }

    fn normal_scancode_to_keycode(&self, base: u8) -> Option<libgui::event::KeyCode> {
        self.scancode_to_char(base).map(libgui::event::KeyCode::from_byte)
    }

    fn emit_extended(&mut self, scancode: u8, gui_active: bool) -> bool {
        match scancode {
            0x48 => {
                if self.shift_pressed && !gui_active {
                    crate::drivers::gop_console::scroll_up(1);
                } else {
                    self.push_escape_seq(b"[A");
                }
                true
            }
            0x50 => {
                if self.shift_pressed && !gui_active {
                    crate::drivers::gop_console::scroll_down(1);
                } else {
                    self.push_escape_seq(b"[B");
                }
                true
            }
            0x49 => {
                if gui_active {
                    self.push_escape_seq(b"[5~");
                } else {
                    crate::drivers::gop_console::scroll_up(5);
                }
                true
            }
            0x51 => {
                if gui_active {
                    self.push_escape_seq(b"[6~");
                } else {
                    crate::drivers::gop_console::scroll_down(5);
                }
                true
            }
            0x4D => {
                self.push_escape_seq(b"[C");
                true
            }
            0x4B => {
                self.push_escape_seq(b"[D");
                true
            }
            0x47 => {
                self.push_escape_seq(b"[H");
                true
            }
            0x4F => {
                self.push_escape_seq(b"[F");
                true
            }
            0x53 => {
                self.push_escape_seq(b"[3~");
                true
            }
            _ => false,
        }
    }

    fn emit_normal(&mut self, code: u8) -> bool {
        if let Some(ch) = self.scancode_to_char(code) {
            crate::drivers::gop_console::reset_scroll_view();
            if self.buffer.len() < 256 {
                self.buffer.push_back(ch);
            }
            true
        } else {
            false
        }
    }

    /// Generate software typematic auto-repeat when a key remains held down cleanly.
    pub fn tick_repeat(&mut self, now_ms: u64) -> bool {
        let Some((code, extended)) = self.held_key else {
            return false;
        };
        if now_ms < self.next_repeat_ms {
            return false;
        }
        let gui_active = crate::drivers::gop_console::is_gui_active();
        let emitted = if extended {
            self.emit_extended(code, gui_active)
        } else {
            self.emit_normal(code)
        };
        self.next_repeat_ms = now_ms.saturating_add(TYPEMATIC_INTERVAL_MS);
        emitted
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
            0x39 => if self.ctrl_pressed { 0x00 } else { b' ' }, // Space (Ctrl+Space = NUL byte for Spotlight)

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

            // 4. Configure typematic rate (0xF3 -> 0x22: 500ms initial delay, ~26.7 Hz) and enable scanning (0xF4)
            if ps2_wait_write() {
                outb(KEYBOARD_DATA_PORT, 0xF3);
                if ps2_wait_read() {
                    let _ = inb(KEYBOARD_DATA_PORT);
                }
                if ps2_wait_write() {
                    outb(KEYBOARD_DATA_PORT, 0x22);
                    if ps2_wait_read() {
                        let _ = inb(KEYBOARD_DATA_PORT);
                    }
                }
            }

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
            register_irq_handler(1, keyboard_irq_handler);
            crate::arch::x86_64::ioapic::unmask_isa_irq(1);
            kinfo!("PS/2 Keyboard driver initialized (IRQ 1 routed via I/O APIC -> Vector 33)");
        } else {
            kinfo!("No 8042 PS/2 Keyboard controller detected (0x64=0xFF); keeping IRQ 1 masked (using USB HID)");
        }
    }
}

/// Returns true if any buffered keyboard bytes are waiting to be consumed.
pub fn has_pending_input() -> bool {
    !KEYBOARD.lock().buffer.is_empty() || !GUI_EVENT_QUEUE.lock().is_empty()
}

/// Advance software typematic key repeat for held PS/2 keys (safe when hardware stops repeating).
pub fn poll_typematic() -> bool {
    let now_ms = crate::arch::x86_64::pit::get_ticks();
    KEYBOARD.lock().tick_repeat(now_ms)
}

/// Non-blocking read of the next typed character (checks buffer, then polls PS/2, USB HID, typematic repeat, and UART).
pub fn read_char() -> Option<u8> {
    if let Some(ch) = KEYBOARD.lock().buffer.pop_front() {
        return Some(ch);
    }

    // Poll PS/2 8042 hardware in case OBF was latched or CPU is running with IF=0 inside a syscall
    poll_ps2_controller();

    // Poll USB HID host controllers (works even when LAPIC timer IRQ is masked during syscalls)
    crate::drivers::usb::poll_hid_nonblocking();

    // Tick PS/2 software typematic repeat fallback
    poll_typematic();

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
