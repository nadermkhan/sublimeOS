use core::fmt;
use core::ptr::write_volatile;
use crate::arch::x86_64::io::outb;
use crate::sync::IrqSafeSpinlock;

pub const VGA_BUFFER_ADDR: usize = 0xFFFFFFFF800B8000;
pub const BUFFER_HEIGHT: usize = 25;
pub const BUFFER_WIDTH: usize = 80;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Color {
    Black = 0,
    Blue = 1,
    Green = 2,
    Cyan = 3,
    Red = 4,
    Magenta = 5,
    Brown = 6,
    LightGray = 7,
    DarkGray = 8,
    LightBlue = 9,
    LightGreen = 10,
    LightCyan = 11,
    LightRed = 12,
    Pink = 13,
    Yellow = 14,
    White = 15,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct ColorCode(u8);

impl ColorCode {
    pub const fn new(foreground: Color, background: Color) -> Self {
        ColorCode((background as u8) << 4 | (foreground as u8))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
struct ScreenChar {
    ascii_character: u8,
    color_code: ColorCode,
}

pub struct VgaWriter {
    row_position: usize,
    column_position: usize,
    color_code: ColorCode,
    buffer: *mut ScreenChar,
}

unsafe impl Send for VgaWriter {}
unsafe impl Sync for VgaWriter {}

impl VgaWriter {
    pub const fn new() -> Self {
        Self {
            row_position: 0,
            column_position: 0,
            color_code: ColorCode::new(Color::LightGray, Color::Black),
            buffer: VGA_BUFFER_ADDR as *mut ScreenChar,
        }
    }

    pub fn set_color(&mut self, fg: Color, bg: Color) {
        self.color_code = ColorCode::new(fg, bg);
    }

    pub fn write_byte(&mut self, byte: u8) {
        match byte {
            b'\n' => self.new_line(),
            b'\r' => self.column_position = 0,
            0x08 => self.backspace(), // Backspace
            byte => {
                if self.column_position >= BUFFER_WIDTH {
                    self.new_line();
                }

                let row = self.row_position;
                let col = self.column_position;
                let color_code = self.color_code;

                unsafe {
                    let offset = row * BUFFER_WIDTH + col;
                    write_volatile(
                        self.buffer.add(offset),
                        ScreenChar {
                            ascii_character: byte,
                            color_code,
                        },
                    );
                }
                self.column_position += 1;
            }
        }
        self.update_hardware_cursor();
    }

    pub fn write_string(&mut self, s: &str) {
        for byte in s.bytes() {
            match byte {
                0x20..=0x7e | b'\n' | b'\r' | 0x08 => self.write_byte(byte),
                _ => self.write_byte(0xfe), // printable block for non-ascii
            }
        }
    }

    pub fn backspace(&mut self) {
        if self.column_position > 0 {
            self.column_position -= 1;
            let offset = self.row_position * BUFFER_WIDTH + self.column_position;
            unsafe {
                write_volatile(
                    self.buffer.add(offset),
                    ScreenChar {
                        ascii_character: b' ',
                        color_code: self.color_code,
                    },
                );
            }
        }
    }

    pub fn clear_screen(&mut self) {
        let blank = ScreenChar {
            ascii_character: b' ',
            color_code: self.color_code,
        };
        for i in 0..(BUFFER_WIDTH * BUFFER_HEIGHT) {
            unsafe {
                write_volatile(self.buffer.add(i), blank);
            }
        }
        self.row_position = 0;
        self.column_position = 0;
        self.update_hardware_cursor();
    }

    fn new_line(&mut self) {
        if self.row_position < BUFFER_HEIGHT - 1 {
            self.row_position += 1;
        } else {
            // Scroll screen up by 1 row
            for row in 1..BUFFER_HEIGHT {
                for col in 0..BUFFER_WIDTH {
                    unsafe {
                        let src = self.buffer.add(row * BUFFER_WIDTH + col);
                        let dst = self.buffer.add((row - 1) * BUFFER_WIDTH + col);
                        write_volatile(dst, core::ptr::read_volatile(src));
                    }
                }
            }
            // Clear last row
            let blank = ScreenChar {
                ascii_character: b' ',
                color_code: self.color_code,
            };
            for col in 0..BUFFER_WIDTH {
                unsafe {
                    write_volatile(self.buffer.add((BUFFER_HEIGHT - 1) * BUFFER_WIDTH + col), blank);
                }
            }
        }
        self.column_position = 0;
    }

    fn update_hardware_cursor(&self) {
        let pos = (self.row_position * BUFFER_WIDTH + self.column_position) as u16;
        unsafe {
            outb(0x3D4, 0x0F);
            outb(0x3D5, (pos & 0xFF) as u8);
            outb(0x3D4, 0x0E);
            outb(0x3D5, ((pos >> 8) & 0xFF) as u8);
        }
    }
}

impl fmt::Write for VgaWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write_string(s);
        Ok(())
    }
}

pub static VGA_WRITER: IrqSafeSpinlock<VgaWriter> = IrqSafeSpinlock::new(VgaWriter::new());

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;
    let mut writer = VGA_WRITER.lock();
    let _ = writer.write_fmt(args);
    let _ = crate::arch::x86_64::serial::SERIAL1.lock().write_fmt(args);
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::arch::x86_64::vga::_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}

#[macro_export]
macro_rules! kinfo {
    ($($arg:tt)*) => {
        $crate::println!("\x1b[32m[INFO] \x1b[0m{}", format_args!($($arg)*));
    };
}

#[macro_export]
macro_rules! kwarn {
    ($($arg:tt)*) => {
        $crate::println!("\x1b[33m[WARN] \x1b[0m{}", format_args!($($arg)*));
    };
}

#[macro_export]
macro_rules! kerror {
    ($($arg:tt)*) => {
        $crate::println!("\x1b[31m[ERROR]\x1b[0m {}", format_args!($($arg)*));
    };
}

#[macro_export]
macro_rules! kdebug {
    ($($arg:tt)*) => {
        $crate::println!("\x1b[36m[DEBUG]\x1b[0m {}", format_args!($($arg)*));
    };
}
