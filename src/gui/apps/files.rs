use alloc::string::String;
use alloc::vec::Vec;
use crate::fs::vfs::{DirEntry, FileType, VFS};
use crate::gui::canvas::Canvas;
use crate::gui::color::Color;
use crate::gui::font::{draw_char, draw_string, FONT_HEIGHT};

pub struct FileManagerApp {
    pub current_path: String,
    pub entries: Vec<DirEntry>,
    pub selected_index: usize,
    pub preview_text: String,
}

impl FileManagerApp {
    pub fn new() -> Self {
        let mut app = Self {
            current_path: String::from("/etc"),
            entries: Vec::new(),
            selected_index: 0,
            preview_text: String::new(),
        };
        app.refresh();
        app
    }

    pub fn refresh(&mut self) {
        let vfs = VFS.lock();
        if let Ok(entries) = vfs.resolve_path(&self.current_path).and_then(|node| node.lock().readdir()) {
            self.entries = entries;
        } else {
            self.entries.clear();
        }
        self.selected_index = 0;
        self.update_preview();
    }

    pub fn navigate_to(&mut self, path: &str) {
        self.current_path = String::from(path);
        self.refresh();
    }

    pub fn select_item(&mut self, index: usize) {
        if index < self.entries.len() {
            self.selected_index = index;
            let entry = &self.entries[index];
            if entry.file_type == FileType::Directory {
                let mut new_path = self.current_path.clone();
                if !new_path.ends_with('/') {
                    new_path.push('/');
                }
                new_path.push_str(&entry.name);
                self.navigate_to(&new_path);
            } else {
                self.update_preview();
            }
        }
    }

    fn update_preview(&mut self) {
        self.preview_text.clear();
        if self.selected_index >= self.entries.len() {
            return;
        }

        let entry = &self.entries[self.selected_index];
        if entry.file_type == FileType::Directory {
            self.preview_text = String::from("(Directory: double click or select to open)");
            return;
        }

        let mut full_path = self.current_path.clone();
        if !full_path.ends_with('/') {
            full_path.push('/');
        }
        full_path.push_str(&entry.name);

        let vfs = VFS.lock();
        if let Ok(file) = vfs.open(&full_path, 0) {
            let mut buf = [0u8; 512];
            if let Ok(bytes_read) = file.read(&mut buf) {
                if let Ok(s) = core::str::from_utf8(&buf[..bytes_read]) {
                    self.preview_text = String::from(s);
                } else {
                    self.preview_text = String::from("(Binary content)");
                }
            }
        }
    }

    pub fn handle_click(&mut self, x: i32, y: i32, inner_x: i32, inner_y: i32, w: i32, _h: i32) {
        let left_w = 110;

        // Check directory tree buttons on left
        if x >= inner_x && x <= inner_x + left_w {
            let dirs = ["/", "/etc", "/proc", "/dev", "/bin", "/tmp"];
            let mut dy = inner_y + 8;
            for dir in dirs.iter() {
                if y >= dy && y <= dy + 24 {
                    self.navigate_to(dir);
                    return;
                }
                dy += 26;
            }
        }

        // Check items in file list pane
        let right_x = inner_x + left_w + 6;
        let right_w = w - left_w - 12;
        if x >= right_x && x <= right_x + right_w {
            let mut dy = inner_y + 30;
            for idx in 0..self.entries.len() {
                if y >= dy && y <= dy + 20 {
                    self.select_item(idx);
                    return;
                }
                dy += 22;
            }
        }
    }

    pub fn render(&self, canvas: &mut Canvas, x: i32, y: i32, w: i32, h: i32) {
        canvas.fill_rect(x, y, w, h, Color::BG_INPUT);

        let left_w = 110;
        let mid_x = x + left_w;

        // 1. Left Sidebar: Places / Bookmarks
        canvas.fill_rect(x, y, left_w, h, Color::BG_CARD);
        canvas.fill_rect(mid_x, y, 1, h, Color::WINDOW_BORDER);

        let dirs = ["/", "/etc", "/proc", "/dev", "/bin", "/tmp"];
        let mut dy = y + 8;

        for &dir in dirs.iter() {
            let is_curr = self.current_path == dir;
            if is_curr {
                canvas.fill_rounded_rect(x + 4, dy, left_w - 8, 22, 4, Color::TITLEBAR_ACTIVE_START);
            }
            draw_char(canvas, x + 8, dy + 3, '@', Color::ACCENT_YELLOW);
            draw_string(
                canvas,
                x + 24,
                dy + 3,
                dir,
                if is_curr { Color::WHITE } else { Color::TEXT_PRIMARY },
            );
            dy += 26;
        }

        // 2. Right Pane: Current Path Bar
        let right_x = mid_x + 6;
        let right_w = w - left_w - 12;

        let mut path_bar = String::from("Path: ");
        path_bar.push_str(&self.current_path);
        draw_string(canvas, right_x, y + 8, &path_bar, Color::ACCENT_CYAN);
        canvas.fill_rect(right_x, y + 26, right_w, 1, Color::WINDOW_BORDER);

        // 3. File List
        let mut item_y = y + 32;
        for (idx, entry) in self.entries.iter().enumerate() {
            if item_y + 20 > y + h - 110 {
                break;
            }

            let is_sel = idx == self.selected_index;
            if is_sel {
                canvas.fill_rounded_rect(right_x, item_y, right_w, 20, 3, Color::TITLEBAR_ACTIVE_START);
            }

            let icon = match entry.file_type {
                FileType::Directory => '@',
                FileType::Regular => '#',
                FileType::CharDevice => '!',
                _ => '?',
            };
            let icon_col = match entry.file_type {
                FileType::Directory => Color::ACCENT_YELLOW,
                FileType::Regular => Color::ACCENT_BLUE,
                FileType::CharDevice => Color::ACCENT_GREEN,
                _ => Color::TEXT_MUTED,
            };

            draw_char(canvas, right_x + 4, item_y + 2, icon, icon_col);
            draw_string(
                canvas,
                right_x + 20,
                item_y + 2,
                &entry.name,
                if is_sel { Color::WHITE } else { Color::TEXT_PRIMARY },
            );

            item_y += 22;
        }

        // 4. File Content Preview Pane at bottom
        let prev_y = y + h - 100;
        canvas.fill_rect(right_x, prev_y - 4, right_w, 1, Color::WINDOW_BORDER);
        draw_string(canvas, right_x, prev_y, "Preview:", Color::ACCENT_YELLOW);

        let mut py = prev_y + FONT_HEIGHT + 2;
        for line in self.preview_text.lines().take(4) {
            let max_chars = (right_w / 8) as usize;
            let display_line = if line.len() > max_chars {
                &line[..max_chars]
            } else {
                line
            };
            draw_string(canvas, right_x + 4, py, display_line, Color::TEXT_MUTED);
            py += FONT_HEIGHT;
        }
    }
}
