use alloc::string::String;
use alloc::vec::Vec;
use crate::fs::vfs::{DirEntry, FileType, VFS};
use crate::gui::canvas::Canvas;
use crate::gui::color::Color;
use crate::gui::font::{draw_char, draw_string, FONT_HEIGHT};

pub struct SidebarItem {
    pub label: &'static str,
    pub path: &'static str,
    pub icon: char,
}

pub struct FileManagerApp {
    pub current_path: String,
    pub entries: Vec<DirEntry>,
    pub selected_index: usize,
    pub preview_text: String,
}

impl FileManagerApp {
    pub fn new() -> Self {
        let mut app = Self {
            current_path: String::from("/Applications"),
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

    pub fn navigate_parent(&mut self) {
        if self.current_path == "/" {
            return;
        }
        if let Some(idx) = self.current_path.rfind('/') {
            if idx == 0 {
                self.current_path = String::from("/");
            } else {
                self.current_path.truncate(idx);
            }
            self.refresh();
        }
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
            self.preview_text = String::from("Folder (click to navigate)");
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
                    self.preview_text = String::from("(Binary object / Executable)");
                }
            }
        }
    }

    pub fn handle_click(&mut self, x: i32, y: i32, inner_x: i32, inner_y: i32, w: i32, _h: i32) {
        let left_w = 136;

        // Check sidebar clicks
        if x >= inner_x && x <= inner_x + left_w {
            let mut dy = inner_y + 24; // start below Favorites label
            let favorites = [
                "/Applications",
                "/Users/sublime/Desktop",
                "/Users/sublime/Documents",
                "/Users/sublime/Downloads",
            ];

            for path in favorites.iter() {
                if y >= dy && y <= dy + 22 {
                    self.navigate_to(path);
                    return;
                }
                dy += 24;
            }

            dy += 20; // skip Locations label
            let locations = [
                "/",
                "/Volumes",
                "/System",
            ];

            for path in locations.iter() {
                if y >= dy && y <= dy + 22 {
                    self.navigate_to(path);
                    return;
                }
                dy += 24;
            }
        }

        // Check items in file list pane
        let right_x = inner_x + left_w + 6;
        let right_w = w - left_w - 12;

        // Check [< Up] button click
        if x >= right_x + 4 && x <= right_x + 48 && y >= inner_y + 6 && y <= inner_y + 30 {
            self.navigate_parent();
            return;
        }

        if x >= right_x && x <= right_x + right_w {
            let mut dy = inner_y + 36;
            for idx in 0..self.entries.len() {
                if y >= dy && y <= dy + 22 {
                    self.select_item(idx);
                    return;
                }
                dy += 24;
            }
        }
    }

    pub fn render(&self, canvas: &mut Canvas, x: i32, y: i32, w: i32, h: i32) {
        canvas.fill_rect(x, y, w, h, Color::argb(255, 30, 32, 44));

        let left_w = 136;
        let mid_x = x + left_w;

        // 1. Finder Sidebar (Vibrant Translucent Dark)
        canvas.fill_rect(x, y, left_w, h, Color::argb(255, 24, 25, 34));
        canvas.fill_rect(mid_x, y, 1, h, Color::argb(50, 255, 255, 255));

        let mut dy = y + 8;

        // Favorites Category
        draw_string(canvas, x + 10, dy, "Favorites", Color::TEXT_MUTED);
        dy += 16;

        let favorites = [
            ("Applications", "/Applications", 'A'),
            ("Desktop", "/Users/sublime/Desktop", 'D'),
            ("Documents", "/Users/sublime/Documents", 'F'),
            ("Downloads", "/Users/sublime/Downloads", 'v'),
        ];

        for (name, path, icon) in favorites.iter() {
            let is_curr = self.current_path == *path;
            if is_curr {
                canvas.fill_rounded_rect(x + 6, dy, left_w - 12, 22, 5, Color::ACCENT_BLUE);
            }
            draw_char(canvas, x + 10, dy + 3, *icon, if is_curr { Color::WHITE } else { Color::ACCENT_CYAN });
            draw_string(
                canvas,
                x + 24,
                dy + 3,
                name,
                if is_curr { Color::WHITE } else { Color::TEXT_PRIMARY },
            );
            dy += 24;
        }

        dy += 8;
        // Locations Category
        draw_string(canvas, x + 10, dy, "Locations", Color::TEXT_MUTED);
        dy += 16;

        let locations = [
            ("Sublime HD", "/", 'C'),
            ("Volumes", "/Volumes", 'U'),
            ("System", "/System", '/'),
        ];

        for (name, path, icon) in locations.iter() {
            let is_curr = self.current_path == *path;
            if is_curr {
                canvas.fill_rounded_rect(x + 6, dy, left_w - 12, 22, 5, Color::ACCENT_BLUE);
            }
            draw_char(canvas, x + 10, dy + 3, *icon, if is_curr { Color::WHITE } else { Color::ACCENT_YELLOW });
            draw_string(
                canvas,
                x + 24,
                dy + 3,
                name,
                if is_curr { Color::WHITE } else { Color::TEXT_PRIMARY },
            );
            dy += 24;
        }

        // 2. Right Pane: Toolbar Breadcrumb & Controls
        let right_x = mid_x + 8;
        let right_w = w - left_w - 16;

        // Finder Toolbar Header
        canvas.fill_rounded_rect(right_x, y + 6, right_w, 24, 4, Color::argb(35, 255, 255, 255));

        // Navigation button [< Up]
        canvas.fill_rounded_rect(right_x + 4, y + 8, 42, 20, 3, Color::argb(60, 255, 255, 255));
        draw_string(canvas, right_x + 8, y + 10, "< Up", Color::WHITE);

        let mut path_bar = String::from("  ");
        path_bar.push_str(&self.current_path);
        draw_string(canvas, right_x + 50, y + 10, &path_bar, Color::ACCENT_CYAN);

        // 3. File List Items
        let mut item_y = y + 36;
        for (idx, entry) in self.entries.iter().enumerate() {
            if item_y + 22 > y + h - 100 {
                break;
            }

            let is_sel = idx == self.selected_index;
            if is_sel {
                canvas.fill_rounded_rect(right_x, item_y, right_w, 22, 4, Color::ACCENT_BLUE);
            } else if idx % 2 == 1 {
                canvas.fill_rect(right_x, item_y, right_w, 22, Color::argb(15, 255, 255, 255));
            }

            let icon = match entry.file_type {
                FileType::Directory => 'D',
                FileType::Regular => if entry.name.ends_with(".app") { 'A' } else if entry.name.ends_with(".plist") { 'P' } else { 'F' },
                FileType::CharDevice => 'C',
                _ => '?',
            };
            let icon_col = match entry.file_type {
                FileType::Directory => Color::ACCENT_CYAN,
                FileType::Regular => if entry.name.ends_with(".app") { Color::ACCENT_YELLOW } else { Color::TEXT_PRIMARY },
                FileType::CharDevice => Color::ACCENT_GREEN,
                _ => Color::TEXT_MUTED,
            };

            draw_char(canvas, right_x + 8, item_y + 3, icon, if is_sel { Color::WHITE } else { icon_col });
            draw_string(
                canvas,
                right_x + 24,
                item_y + 3,
                &entry.name,
                if is_sel { Color::WHITE } else { Color::TEXT_PRIMARY },
            );

            item_y += 24;
        }

        // 4. File Content Preview Pane at bottom
        let prev_y = y + h - 94;
        canvas.fill_rounded_rect(right_x, prev_y, right_w, 86, 6, Color::argb(45, 0, 0, 0));
        canvas.draw_rounded_rect(right_x, prev_y, right_w, 86, 6, Color::argb(50, 255, 255, 255));

        draw_string(canvas, right_x + 8, prev_y + 6, "Quick Look / Info:", Color::ACCENT_YELLOW);

        let mut py = prev_y + 24;
        for line in self.preview_text.lines().take(3) {
            let max_chars = (right_w / 8) as usize;
            let display_line = if line.len() > max_chars {
                &line[..max_chars]
            } else {
                line
            };
            draw_string(canvas, right_x + 8, py, display_line, Color::TEXT_MUTED);
            py += FONT_HEIGHT + 2;
        }
    }
}
