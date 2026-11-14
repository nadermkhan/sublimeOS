use alloc::string::String;
use alloc::vec::Vec;
use crate::fs::vfs::VFS;
use crate::gui::canvas::Canvas;
use crate::gui::color::Color;
use crate::gui::font::{draw_string, draw_string_ellipsis, FONT_HEIGHT, FONT_WIDTH};

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
        app.lines.push(String::from("Welcome to SublimeOS Notepad!"));
        app.lines.push(String::from("Type freely or open /etc/rc.sh from the toolbar."));
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
        // 1. Notepad Menu + Toolbar Strip
        canvas.fill_rect(ix, iy, iw, 50, Color::XP_FACE);
        draw_string_ellipsis(canvas, ix + 8, iy + 3, "File   Edit   Format   View   Help", Color::XP_TEXT_BLACK, iw - 16);
        canvas.fill_rect(ix, iy + 20, iw, 1, Color::XP_SHADOW);

        // Action Buttons: [New] [Open rc.sh] [Save]
        canvas.draw_gradient_v(ix + 8, iy + 24, 52, 22, Color::XP_WHITE, Color::XP_FACE_DARK);
        canvas.draw_rounded_rect(ix + 8, iy + 24, 52, 22, 3, Color::XP_CTRL_BORDER);
        draw_string(canvas, ix + 19, iy + 27, "New", Color::XP_TEXT_BLACK);

        canvas.draw_gradient_v(ix + 66, iy + 24, 94, 22, Color::XP_WHITE, Color::XP_FACE_DARK);
        canvas.draw_rounded_rect(ix + 66, iy + 24, 94, 22, 3, Color::XP_CTRL_BORDER);
        draw_string(canvas, ix + 73, iy + 27, "Open rc.sh", Color::XP_TEXT_BLACK);

        canvas.draw_gradient_v(ix + 166, iy + 24, 54, 22, Color::XP_WHITE, Color::XP_FACE_DARK);
        canvas.draw_rounded_rect(ix + 166, iy + 24, 54, 22, 3, Color::XP_CTRL_BORDER);
        draw_string(canvas, ix + 177, iy + 27, "Save", Color::XP_TEXT_BLACK);

        draw_string_ellipsis(canvas, ix + 230, iy + 27, &self.status_msg, Color::XP_TEXT_GRAY, (iw - 238).max(20));
        canvas.fill_rect(ix, iy + 49, iw, 1, Color::XP_SHADOW);

        // 2. Classic Crisp White Notepad Paper Canvas
        let doc_y = iy + 50;
        let doc_h = (ih - 50).max(16);
        canvas.fill_rect(ix, doc_y, iw, doc_h, Color::WHITE);
        canvas.draw_rect(ix, doc_y, iw, doc_h, Color::XP_INPUT_BORDER);

        let mut curr_y = doc_y + 6;
        let left_margin = ix + 10;
        let max_text_w = (iw - 20).max(32);

        for (row_idx, line) in self.lines.iter().enumerate() {
            if curr_y + FONT_HEIGHT > iy + ih - 4 {
                break;
            }

            draw_string_ellipsis(canvas, left_margin, curr_y, line, Color::XP_TEXT_BLACK, max_text_w);

            if row_idx == self.cursor_row {
                let cursor_x = (left_margin + (self.cursor_col as i32 * FONT_WIDTH)).min(ix + iw - 12);
                canvas.fill_rect(cursor_x, curr_y, 2, FONT_HEIGHT, Color::XP_TEXT_BLACK);
            }

            curr_y += FONT_HEIGHT + 2;
        }
    }
}
