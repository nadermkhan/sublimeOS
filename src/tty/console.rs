use alloc::collections::VecDeque;
use core::ptr::write_volatile;
use crate::arch::x86_64::vga::{BUFFER_HEIGHT, BUFFER_WIDTH, Color, ColorCode, ScreenChar};
use crate::tty::ansi::{AnsiCommand, AnsiParser};

pub struct VirtualConsole {
    pub id: usize,
    pub name: &'static str,
    pub buffer: [ScreenChar; BUFFER_WIDTH * BUFFER_HEIGHT],
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub color_code: ColorCode,
    pub ansi: AnsiParser,
    pub input_queue: VecDeque<u8>,
}

impl VirtualConsole {
    pub const fn new(id: usize, name: &'static str) -> Self {
        let blank = ScreenChar {
            ascii_character: b' ',
            color_code: ColorCode::new(Color::LightGray, Color::Black),
        };
        Self {
            id,
            name,
            buffer: [blank; BUFFER_WIDTH * BUFFER_HEIGHT],
            cursor_row: 0,
            cursor_col: 0,
            color_code: ColorCode::new(Color::LightGray, Color::Black),
            ansi: AnsiParser::new(),
            input_queue: VecDeque::new(),
        }
    }

    pub fn write_byte(&mut self, byte: u8) {
        let (consumed, command) = self.ansi.process_byte(byte);

        if let Some(cmd) = command {
            self.execute_ansi_command(cmd);
            return;
        }

        if consumed {
            return;
        }

        // Printable / control character
        match byte {
            b'\n' => self.new_line(),
            b'\r' => self.cursor_col = 0,
            0x08 | 0x7F => self.backspace(),
            0x09 => {
                // Tab: advance to next 4-space tabstop
                let next_tab = (self.cursor_col + 4) & !3;
                while self.cursor_col < next_tab && self.cursor_col < BUFFER_WIDTH {
                    self.write_char_at_cursor(b' ');
                }
            }
            c => {
                self.write_char_at_cursor(c);
            }
        }
    }

    pub fn write_string(&mut self, s: &str) {
        for b in s.bytes() {
            self.write_byte(b);
        }
    }

    fn write_char_at_cursor(&mut self, ch: u8) {
        if self.cursor_col >= BUFFER_WIDTH {
            self.new_line();
        }

        let idx = self.cursor_row * BUFFER_WIDTH + self.cursor_col;
        self.buffer[idx] = ScreenChar {
            ascii_character: ch,
            color_code: self.color_code,
        };
        self.cursor_col += 1;
    }

    pub fn backspace(&mut self) {
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
            let idx = self.cursor_row * BUFFER_WIDTH + self.cursor_col;
            self.buffer[idx] = ScreenChar {
                ascii_character: b' ',
                color_code: self.color_code,
            };
        }
    }

    fn new_line(&mut self) {
        self.cursor_col = 0;
        if self.cursor_row < BUFFER_HEIGHT - 1 {
            self.cursor_row += 1;
        } else {
            // Scroll buffer up by 1 line
            for row in 1..BUFFER_HEIGHT {
                for col in 0..BUFFER_WIDTH {
                    let src = row * BUFFER_WIDTH + col;
                    let dst = (row - 1) * BUFFER_WIDTH + col;
                    self.buffer[dst] = self.buffer[src];
                }
            }
            // Clear last row
            let blank = ScreenChar {
                ascii_character: b' ',
                color_code: self.color_code,
            };
            for col in 0..BUFFER_WIDTH {
                self.buffer[(BUFFER_HEIGHT - 1) * BUFFER_WIDTH + col] = blank;
            }
        }
    }

    pub fn clear(&mut self) {
        let blank = ScreenChar {
            ascii_character: b' ',
            color_code: self.color_code,
        };
        for cell in self.buffer.iter_mut() {
            *cell = blank;
        }
        self.cursor_row = 0;
        self.cursor_col = 0;
    }

    fn execute_ansi_command(&mut self, cmd: AnsiCommand) {
        match cmd {
            AnsiCommand::SetColor { fg, bg } => {
                self.color_code = ColorCode::new(fg, bg);
            }
            AnsiCommand::ClearScreen => {
                self.clear();
            }
            AnsiCommand::ClearLine => {
                let start = self.cursor_row * BUFFER_WIDTH + self.cursor_col;
                let end = (self.cursor_row + 1) * BUFFER_WIDTH;
                let blank = ScreenChar {
                    ascii_character: b' ',
                    color_code: self.color_code,
                };
                for i in start..end {
                    self.buffer[i] = blank;
                }
            }
            AnsiCommand::CursorPosition { row, col } => {
                self.cursor_row = row.min(BUFFER_HEIGHT - 1);
                self.cursor_col = col.min(BUFFER_WIDTH - 1);
            }
            AnsiCommand::CursorUp(count) => {
                self.cursor_row = self.cursor_row.saturating_sub(count);
            }
            AnsiCommand::CursorDown(count) => {
                self.cursor_row = (self.cursor_row + count).min(BUFFER_HEIGHT - 1);
            }
            AnsiCommand::CursorForward(count) => {
                self.cursor_col = (self.cursor_col + count).min(BUFFER_WIDTH - 1);
            }
            AnsiCommand::CursorBack(count) => {
                self.cursor_col = self.cursor_col.saturating_sub(count);
            }
            AnsiCommand::ShowCursor(_show) => {}
            AnsiCommand::None => {}
        }
    }

    /// Render this virtual console's buffer directly to the physical VGA text buffer.
    pub fn flush_to_vga(&self, vga_ptr: *mut ScreenChar) {
        if vga_ptr.is_null() {
            return;
        }
        for (i, cell) in self.buffer.iter().enumerate() {
            unsafe {
                write_volatile(vga_ptr.add(i), *cell);
            }
        }
    }
}
