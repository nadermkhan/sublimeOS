pub mod ansi;
pub mod console;
pub mod pty;

use core::fmt;
use crate::arch::x86_64::io::outb;
use crate::arch::x86_64::serial::SERIAL1;
use crate::arch::x86_64::vga::{BUFFER_WIDTH, VGA_BUFFER_ADDR, ScreenChar};
use crate::sync::IrqSafeSpinlock;
use crate::tty::console::VirtualConsole;

pub const MAX_TTYS: usize = 4;

pub struct TtyManager {
    pub consoles: [VirtualConsole; MAX_TTYS],
    pub active_index: usize,
}

impl TtyManager {
    pub const fn new() -> Self {
        Self {
            consoles: [
                VirtualConsole::new(1, "ttys000"),
                VirtualConsole::new(2, "ttys001"),
                VirtualConsole::new(3, "ttys002"),
                VirtualConsole::new(4, "ttys003"),
            ],
            active_index: 0,
        }
    }

    pub fn active_console(&mut self) -> &mut VirtualConsole {
        &mut self.consoles[self.active_index]
    }

    pub fn switch_to(&mut self, index: usize) {
        if index < MAX_TTYS && index != self.active_index {
            self.active_index = index;
            let vga_ptr = VGA_BUFFER_ADDR as *mut ScreenChar;
            self.consoles[self.active_index].flush_to_vga(vga_ptr);
            crate::drivers::gop_console::flush_tty_console(&self.consoles[self.active_index]);
            self.update_hardware_cursor();
        }
    }

    pub fn write_byte(&mut self, byte: u8) {
        let active = self.active_index;
        self.consoles[active].write_byte(byte);

        // Update physical screen
        let vga_ptr = VGA_BUFFER_ADDR as *mut ScreenChar;
        self.consoles[active].flush_to_vga(vga_ptr);
        crate::drivers::gop_console::tty_write_bytes(&[byte]);
        self.update_hardware_cursor();
    }

    pub fn write_string(&mut self, s: &str) {
        let active = self.active_index;
        self.consoles[active].write_string(s);

        let vga_ptr = VGA_BUFFER_ADDR as *mut ScreenChar;
        self.consoles[active].flush_to_vga(vga_ptr);
        crate::drivers::gop_console::tty_write_bytes(s.as_bytes());
        self.update_hardware_cursor();
    }

    pub fn update_hardware_cursor(&self) {
        let console = &self.consoles[self.active_index];
        let pos = (console.cursor_row * BUFFER_WIDTH + console.cursor_col) as u16;

        unsafe {
            outb(0x3D4, 0x0F);
            outb(0x3D5, (pos & 0xFF) as u8);
            outb(0x3D4, 0x0E);
            outb(0x3D5, ((pos >> 8) & 0xFF) as u8);
        }
    }
}

pub static TTY: IrqSafeSpinlock<TtyManager> = IrqSafeSpinlock::new(TtyManager::new());

/// Switch active virtual console (1..=4).
pub fn switch_tty(tty_num: usize) {
    if tty_num >= 1 && tty_num <= MAX_TTYS {
        TTY.lock().switch_to(tty_num - 1);
    }
}

/// Print formatted text to the active TTY and mirror to serial COM1.
pub fn tty_print(args: fmt::Arguments) {
    use core::fmt::Write;

    // 1. Mirror to COM1 Serial
    {
        let mut serial = SERIAL1.lock();
        let _ = serial.write_fmt(args);
    }

    // 2. Format into TTY
    struct TtyWriter;
    impl fmt::Write for TtyWriter {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            TTY.lock().write_string(s);
            Ok(())
        }
    }

    let mut writer = TtyWriter;
    let _ = writer.write_fmt(args);
}

/// Print formatted text with a newline.
#[macro_export]
macro_rules! tty_println {
    () => ($crate::tty::tty_print(format_args!("\n")));
    ($($arg:tt)*) => ($crate::tty::tty_print(format_args!("{}\n", format_args!($($arg)*))));
}

/// Initialize the TTY subsystem and set up virtual consoles.
pub fn init() {
    let mut tty = TTY.lock();
    let vga_ptr = VGA_BUFFER_ADDR as *mut ScreenChar;

    // Initialize ttys001 with a secondary shell banner
    tty.consoles[1].write_string("\x1b[36m=== SublimeOS Virtual Console [ttys001] ===\x1b[0m\n");
    tty.consoles[1].write_string("Secondary sublimesh session ready.\n\n");

    // Initialize ttys002 as Console.app / dmesg
    tty.consoles[2].write_string("\x1b[33m=== SublimeOS Console [ttys002 - log stream] ===\x1b[0m\n");
    tty.consoles[2].write_string("Streaming live KiteKernel diagnostics and hardware IRQ logs...\n\n");

    // Initialize ttys003 as Activity Monitor / top
    tty.consoles[3].write_string("\x1b[35m=== SublimeOS Activity Monitor [ttys003 - top] ===\x1b[0m\n");
    tty.consoles[3].write_string("KiteKernel tasks and CPU thread telemetry active.\n\n");

    // TTY 1 is active by default
    tty.consoles[0].flush_to_vga(vga_ptr);
    tty.update_hardware_cursor();
}

/// Inject an input character from USB HID or external input devices into the active TTY/shell input stream.
pub fn push_input_char(ch: char) {
    crate::drivers::keyboard::push_char(ch as u8);
}
