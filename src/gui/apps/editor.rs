use alloc::string::String;
use alloc::vec::Vec;
use crate::fs::vfs::VFS;
use crate::gui::canvas::Canvas;
use crate::gui::color::Color;
use crate::gui::font::{draw_string, FONT_HEIGHT, FONT_WIDTH};

pub struct TextEditApp {
    pub file_path: String,
    pub lines: Vec<String>,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub status_msg: String,
}

impl TextEditApp {
    pub fn new() -> Self {
        let mut app = Self {
            file_path: String::from("/Users/untitled.txt"),
            lines: Vec::new(),
            cursor_row: 0,
            cursor_col: 0,
            status_msg: String::from("Ready — Click or type to write"),
        };
        app.lines.push(String::from("Welcome to TextEdit for SublimeOS!"));
        app.lines.push(String::from("A lightweight document editor styled after macOS."));
        app.lines.push(String::new());
        app.cursor_row = 2;
        app.cursor_col = 0;
        app
    }

    pub fn open_file(&mut self, path: &str) {
        let vfs = VFS.lock();
        if let Ok(file) = vfs.open(path, 0) {
            drop(vfs);
            let mut buf = alloc::vec![0u8; 16384];
            if let Ok(n) = file.read(&mut buf) {
                if let Ok(s) = core::str::from_utf8(&buf[..n]) {
                    self.lines = s.lines().map(String::from).collect();
                    if self.lines.is_empty() {
                        self.lines.push(String::new());
                    }
                    self.file_path = String::from(path);
                    self.cursor_row = 0;
                    self.cursor_col = 0;
                    self.status_msg = alloc::format!("Opened {} ({} bytes)", path, n);
                    return;
                }
            }
        }
        self.status_msg = alloc::format!("Failed to open {}", path);
    }

    pub fn save_file(&mut self) {
        let mut content = String::new();
        for (i, line) in self.lines.iter().enumerate() {
            content.push_str(line);
            if i + 1 < self.lines.len() {
                content.push('\n');
            }
        }
        let bytes = content.as_bytes();
        let vfs = VFS.lock();
        if let Ok(file) = vfs.open(&self.file_path, crate::user::libc::O_CREAT | crate::user::libc::O_WRONLY | crate::user::libc::O_TRUNC) {
            drop(vfs);
            if let Ok(n) = file.write(bytes) {
                self.status_msg = alloc::format!("Saved to {} ({} bytes)", self.file_path, n);
                return;
            }
        }
        self.status_msg = alloc::format!("Error saving {}", self.file_path);
    }

    pub fn handle_char(&mut self, ch: char) {
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        if self.cursor_row >= self.lines.len() {
            self.cursor_row = self.lines.len() - 1;
        }

        match ch {
            '\n' | '\r' => {
                let tail = self.lines[self.cursor_row].split_off(self.cursor_col);
                self.cursor_row += 1;
                self.lines.insert(self.cursor_row, tail);
                self.cursor_col = 0;
            }
            '\x08' => {
                // Backspace
                if self.cursor_col > 0 {
                    self.lines[self.cursor_row].remove(self.cursor_col - 1);
                    self.cursor_col -= 1;
                } else if self.cursor_row > 0 {
                    let prev_len = self.lines[self.cursor_row - 1].len();
                    let curr = self.lines.remove(self.cursor_row);
                    self.cursor_row -= 1;
                    self.lines[self.cursor_row].push_str(&curr);
                    self.cursor_col = prev_len;
                }
            }
            c if c >= ' ' && c <= '~' => {
                self.lines[self.cursor_row].insert(self.cursor_col, c);
                self.cursor_col += 1;
            }
            _ => {}
        }
    }

    pub fn handle_click(&mut self, px: i32, py: i32, ix: i32, iy: i32, _iw: i32, _ih: i32) {
        // Toolbar buttons at top: [New] [Open /etc/rc.sh] [Save]
        if py >= iy + 4 && py <= iy + 26 {
            if px >= ix + 8 && px <= ix + 60 {
                // New
                self.lines.clear();
                self.lines.push(String::new());
                self.cursor_row = 0;
                self.cursor_col = 0;
                self.file_path = String::from("/Users/untitled.txt");
                self.status_msg = String::from("New document");
            } else if px >= ix + 66 && px <= ix + 160 {
                // Open /etc/rc.sh
                self.open_file("/etc/rc.sh");
            } else if px >= ix + 166 && px <= ix + 220 {
                // Save
                self.save_file();
            }
        }
    }

    pub fn render(&self, canvas: &mut Canvas, ix: i32, iy: i32, iw: i32, ih: i32) {
        // 1. Window Document Canvas (Dark/Light macOS document theme)
        canvas.fill_rect(ix, iy, iw, ih, Color::argb(255, 24, 25, 34));

        // 2. Toolbar at Top
        canvas.fill_rect(ix, iy, iw, 30, Color::argb(255, 32, 34, 46));
        canvas.fill_rect(ix, iy + 29, iw, 1, Color::WINDOW_BORDER);

        // Action Buttons: [New] [Open /etc/rc.sh] [Save]
        canvas.fill_rounded_rect(ix + 8, iy + 4, 52, 22, 4, Color::argb(60, 255, 255, 255));
        draw_string(canvas, ix + 18, iy + 7, "New", Color::WHITE);

        canvas.fill_rounded_rect(ix + 66, iy + 4, 94, 22, 4, Color::argb(60, 255, 255, 255));
        draw_string(canvas, ix + 74, iy + 7, "Open rc.sh", Color::TEXT_PRIMARY);

        canvas.fill_rounded_rect(ix + 166, iy + 4, 54, 22, 4, Color::ACCENT_BLUE);
        draw_string(canvas, ix + 176, iy + 7, "Save", Color::WHITE);

        // Status message right-aligned
        draw_string(canvas, ix + 230, iy + 7, &self.status_msg, Color::TEXT_MUTED);

        // 3. Document Content Area
        let doc_y = iy + 36;
        let mut curr_y = doc_y;
        let left_margin = ix + 12;

        for (row_idx, line) in self.lines.iter().enumerate() {
            if curr_y + FONT_HEIGHT as i32 > iy + ih - 6 {
                break;
            }

            draw_string(canvas, left_margin, curr_y, line, Color::WHITE);

            // Draw text cursor
            if row_idx == self.cursor_row {
                let cursor_x = left_margin + (self.cursor_col as i32 * FONT_WIDTH as i32);
                canvas.fill_rect(cursor_x, curr_y, 2, FONT_HEIGHT as i32, Color::ACCENT_CYAN);
            }

            curr_y += FONT_HEIGHT as i32 + 2;
        }
    }
}
