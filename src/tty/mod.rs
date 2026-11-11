pub mod ansi;
pub mod console;

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
                VirtualConsole::new(1, "tty1"),
                VirtualConsole::new(2, "tty2"),
                VirtualConsole::new(3, "tty3"),
                VirtualConsole::new(4, "tty4"),
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
            self.update_hardware_cursor();
        }
    }

    pub fn write_byte(&mut self, byte: u8) {
        let active = self.active_index;
        self.consoles[active].write_byte(byte);

        // Update physical screen
        let vga_ptr = VGA_BUFFER_ADDR as *mut ScreenChar;
        self.consoles[active].flush_to_vga(vga_ptr);
        self.update_hardware_cursor();
    }

    pub fn write_string(&mut self, s: &str) {
        let active = self.active_index;
        self.consoles[active].write_string(s);

        let vga_ptr = VGA_BUFFER_ADDR as *mut ScreenChar;
        self.consoles[active].flush_to_vga(vga_ptr);
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

    // Initialize TTY 2 with a secondary shell banner
    tty.consoles[1].write_string("\x1b[36m=== CosmosKernel Virtual Console [TTY 2] ===\x1b[0m\n");
    tty.consoles[1].write_string("Secondary interactive session ready.\n\n");

    // Initialize TTY 3 as Kernel Logs / dmesg
    tty.consoles[2].write_string("\x1b[33m=== CosmosKernel System Event Log [TTY 3 - dmesg] ===\x1b[0m\n");
    tty.consoles[2].write_string("Streaming live kernel diagnostics and hardware IRQ logs...\n\n");

    // Initialize TTY 4 as Task Monitor
    tty.consoles[3].write_string("\x1b[35m=== CosmosKernel Task Manager [TTY 4 - top] ===\x1b[0m\n");
    tty.consoles[3].write_string("Preemptive multitasking scheduler telemetry active.\n\n");

    // TTY 1 is active by default
    tty.consoles[0].flush_to_vga(vga_ptr);
    tty.update_hardware_cursor();
}
