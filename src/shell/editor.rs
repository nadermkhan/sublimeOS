extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use crate::print;
use crate::println;

pub struct TextEditor {
    pub file_path: String,
    pub lines: Vec<String>,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub row_offset: usize,
    pub modified: bool,
    pub status_msg: String,
}

impl TextEditor {
    pub fn new(path: &str) -> Self {
        let mut lines = Vec::new();

        // Attempt to load existing file from VFS
        if let Ok(content) = crate::shell::pipeline::read_file_from_vfs(path) {
            for line in content.lines() {
                lines.push(String::from(line));
            }
        }

        if lines.is_empty() {
            lines.push(String::new());
        }

        Self {
            file_path: String::from(path),
            lines,
            cursor_row: 0,
            cursor_col: 0,
            row_offset: 0,
            modified: false,
            status_msg: String::from("Ready - Press ^S to save, ^X to exit"),
        }
    }

    pub fn save(&mut self) -> Result<(), &'static str> {
        let mut full_text = String::new();
        for (i, line) in self.lines.iter().enumerate() {
            full_text.push_str(line);
            if i + 1 < self.lines.len() {
                full_text.push('\n');
            }
        }

        crate::shell::pipeline::write_file_to_vfs(&self.file_path, full_text.as_bytes(), false)?;
        self.modified = false;
        self.status_msg = alloc::format!("Wrote {} lines to {}", self.lines.len(), self.file_path);
        Ok(())
    }

    pub fn insert_char(&mut self, c: char) {
        if self.cursor_row >= self.lines.len() {
            self.lines.push(String::new());
        }

        let line = &mut self.lines[self.cursor_row];
        if self.cursor_col > line.len() {
            self.cursor_col = line.len();
        }

        line.insert(self.cursor_col, c);
        self.cursor_col += 1;
        self.modified = true;
    }

    pub fn insert_newline(&mut self) {
        if self.cursor_row >= self.lines.len() {
            self.lines.push(String::new());
        }

        let line = &mut self.lines[self.cursor_row];
        let remainder = if self.cursor_col < line.len() {
            line.split_off(self.cursor_col)
        } else {
            String::new()
        };

        self.cursor_row += 1;
        self.cursor_col = 0;
        self.lines.insert(self.cursor_row, remainder);
        self.modified = true;
    }

    pub fn backspace(&mut self) {
        if self.cursor_col > 0 {
            let line = &mut self.lines[self.cursor_row];
            self.cursor_col -= 1;
            if self.cursor_col < line.len() {
                line.remove(self.cursor_col);
                self.modified = true;
            }
        } else if self.cursor_row > 0 {
            // Merge current line with previous line
            let current = self.lines.remove(self.cursor_row);
            self.cursor_row -= 1;
            let prev_len = self.lines[self.cursor_row].len();
            self.lines[self.cursor_row].push_str(&current);
            self.cursor_col = prev_len;
            self.modified = true;
        }
    }

    pub fn delete_char(&mut self) {
        if self.cursor_row < self.lines.len() {
            let line_len = self.lines[self.cursor_row].len();
            if self.cursor_col < line_len {
                self.lines[self.cursor_row].remove(self.cursor_col);
                self.modified = true;
            } else if self.cursor_row + 1 < self.lines.len() {
                let next_line = self.lines.remove(self.cursor_row + 1);
                self.lines[self.cursor_row].push_str(&next_line);
                self.modified = true;
            }
        }
    }

    pub fn move_up(&mut self) {
        if self.cursor_row > 0 {
            self.cursor_row -= 1;
            let len = self.lines[self.cursor_row].len();
            if self.cursor_col > len {
                self.cursor_col = len;
            }
        }
    }

    pub fn move_down(&mut self) {
        if self.cursor_row + 1 < self.lines.len() {
            self.cursor_row += 1;
            let len = self.lines[self.cursor_row].len();
            if self.cursor_col > len {
                self.cursor_col = len;
            }
        }
    }

    pub fn move_left(&mut self) {
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        } else if self.cursor_row > 0 {
            self.cursor_row -= 1;
            self.cursor_col = self.lines[self.cursor_row].len();
        }
    }

    pub fn move_right(&mut self) {
        let len = self.lines[self.cursor_row].len();
        if self.cursor_col < len {
            self.cursor_col += 1;
        } else if self.cursor_row + 1 < self.lines.len() {
            self.cursor_row += 1;
            self.cursor_col = 0;
        }
    }

    pub fn render(&self) {
        // Clear screen and position cursor top left
        print!("\x1b[2J\x1b[H");

        // 1. Header Bar
        let mod_tag = if self.modified { " [Modified]" } else { "" };
        print!("\x1b[7m SublimeOS Nano 1.0  File: {}{} \x1b[0m\r\n", self.file_path, mod_tag);

        // 2. Text Area (up to 20 visible lines)
        let visible_lines = 20;
        for i in 0..visible_lines {
            let row = self.row_offset + i;
            if row < self.lines.len() {
                print!("\x1b[36m{:>3} \x1b[0m{}\r\n", row + 1, self.lines[row]);
            } else {
                print!("\x1b[34m~  \x1b[0m\r\n");
            }
        }

        // 3. Status Bar & Shortcut Bar
        print!("\x1b[7m [Status: {}] | Ln {}, Col {} \x1b[0m\r\n", self.status_msg, self.cursor_row + 1, self.cursor_col + 1);
        print!("\x1b[33m ^S: Save | ^X: Exit | ^C: Exit | Tab: 4 spaces\x1b[0m\r\n");

        // Position terminal cursor on the active edit point:
        // row: 2 + (cursor_row - row_offset), col: 5 + cursor_col
        let screen_row = 2 + (self.cursor_row.saturating_sub(self.row_offset));
        let screen_col = 5 + self.cursor_col;
        print!("\x1b[{};{}H", screen_row, screen_col);
    }

    /// Run the interactive text editor loop until exit is requested.
    pub fn run(&mut self) {
        let mut esc_state = 0;

        self.render();

        loop {
            if let Some(b) = crate::shell::get_next_input_char() {
                // Escape sequence state machine
                if esc_state == 1 {
                    if b == b'[' {
                        esc_state = 2;
                        continue;
                    } else {
                        esc_state = 0;
                    }
                } else if esc_state == 2 {
                    esc_state = 0;
                    match b {
                        b'A' => self.move_up(),
                        b'B' => self.move_down(),
                        b'C' => self.move_right(),
                        b'D' => self.move_left(),
                        b'H' => self.cursor_col = 0,
                        b'F' => self.cursor_col = self.lines[self.cursor_row].len(),
                        b'3' => {
                            let _ = crate::shell::get_next_input_char(); // '~'
                            self.delete_char();
                        }
                        _ => {}
                    }
                    self.render();
                    continue;
                }

                match b {
                    0x1B => {
                        esc_state = 1;
                    }
                    0x13 | 0x0F => {
                        // Ctrl+S or Ctrl+O: Save
                        let _ = self.save();
                        self.render();
                    }
                    0x18 | 0x11 | 0x03 => {
                        // Ctrl+X, Ctrl+Q, Ctrl+C: Exit
                        print!("\x1b[2J\x1b[H"); // Clear screen
                        println!("Exited nano.");
                        break;
                    }
                    b'\n' | b'\r' => {
                        self.insert_newline();
                        self.render();
                    }
                    0x08 | 0x7F => {
                        // Backspace
                        self.backspace();
                        self.render();
                    }
                    b'\t' => {
                        for _ in 0..4 {
                            self.insert_char(' ');
                        }
                        self.render();
                    }
                    0x20..=0x7E => {
                        self.insert_char(b as char);
                        self.render();
                    }
                    _ => {}
                }
            } else {
                core::hint::spin_loop();
            }
        }
    }
}

/// Shell entry point: launch editor on given path.
pub fn run_editor(path: &str) {
    let mut editor = TextEditor::new(path);
    editor.run();
}
