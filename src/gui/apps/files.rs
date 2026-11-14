use alloc::string::String;
use alloc::vec::Vec;
use crate::fs::vfs::{DirEntry, FileType, VFS};
use crate::gui::canvas::Canvas;
use crate::gui::color::Color;
use crate::gui::font::{draw_string, FONT_HEIGHT};

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
        let entries_res = {
            let vfs = VFS.lock();
            vfs.resolve_path(&self.current_path)
                .and_then(|node| node.lock().readdir())
        };
        if let Ok(entries) = entries_res {
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
        let tb_y = inner_y + 23;
        // 1. Check [< Back] toolbar button
        if x >= inner_x + 8 && x <= inner_x + 66 && y >= tb_y && y <= tb_y + 22 {
            self.navigate_parent();
            return;
        }

        let body_y = inner_y + 52;
        let left_w = 148;

        // 2. Check XP Blue Common Tasks sidebar clicks
        if x >= inner_x && x <= inner_x + left_w {
            let mut dy = body_y + 32;
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
                dy += 23;
            }

            let mut ly = body_y + 136 + 24;
            let locations = [
                "/",
                "/Volumes",
                "/System",
            ];

            for path in locations.iter() {
                if y >= ly && y <= ly + 22 {
                    self.navigate_to(path);
                    return;
                }
                ly += 23;
            }
        }

        // 3. Check items in white Explorer file pane
        let right_x = inner_x + left_w;
        let right_w = w - left_w;

        if x >= right_x && x <= right_x + right_w {
            let mut dy = body_y + 8;
            for idx in 0..self.entries.len() {
                if y >= dy && y <= dy + 22 {
                    self.select_item(idx);
                    return;
                }
                dy += 23;
            }
        }
    }

    pub fn render(&self, canvas: &mut Canvas, x: i32, y: i32, w: i32, h: i32) {
        // 0. Explorer Top Menu & Toolbar
        canvas.fill_rect(x, y, w, 52, Color::XP_FACE);
        crate::gui::font::draw_string_ellipsis(canvas, x + 8, y + 2, "File   Edit   View   Places   Tools   Help", Color::XP_TEXT_BLACK, w - 16);
        canvas.fill_rect(x, y + 20, w, 1, Color::XP_SHADOW);

        // Navigation Toolbar Row
        let tb_y = y + 23;
        canvas.draw_gradient_v(x + 8, tb_y, 58, 22, Color::XP_WHITE, Color::XP_FACE_DARK);
        canvas.draw_rounded_rect(x + 8, tb_y, 58, 22, 3, Color::XP_CTRL_BORDER);
        canvas.fill_circle(x + 19, tb_y + 11, 7, Color::XP_START_MID);
        draw_string(canvas, x + 28, tb_y + 3, "Back", Color::XP_TEXT_BLACK);

        // Address Bar
        draw_string(canvas, x + 76, tb_y + 3, "Address", Color::XP_TEXT_GRAY);
        let addr_x = x + 138;
        let addr_w = (w - 146).max(80);
        canvas.fill_rect(addr_x, tb_y, addr_w, 22, Color::WHITE);
        canvas.draw_rect(addr_x, tb_y, addr_w, 22, Color::XP_INPUT_BORDER);
        crate::gui::font::draw_string_ellipsis(canvas, addr_x + 6, tb_y + 3, &self.current_path, Color::XP_TEXT_BLACK, addr_w - 12);
        canvas.fill_rect(x, y + 51, w, 1, Color::XP_SHADOW);

        let body_y = y + 52;
        let body_h = (h - 52 - 20).max(60);
        let left_w = 148;

        // 1. Classic Blue "Common Tasks" Left Sidebar
        canvas.draw_gradient_v(x, body_y, left_w, body_h, Color::rgb(123, 162, 231), Color::rgb(99, 117, 214));

        // Task Group Card 1: Favorite Places
        let card1_x = x + 8;
        let card1_w = left_w - 16;
        canvas.draw_gradient_h(card1_x, body_y + 8, card1_w, 20, Color::WHITE, Color::rgb(198, 214, 245));
        draw_string(canvas, card1_x + 8, body_y + 10, "Places", Color::XP_TEXT_LINK);
        canvas.fill_rect(card1_x, body_y + 28, card1_w, 98, Color::rgb(214, 223, 247));

        let favorites = [
            ("Applications", "/Applications"),
            ("Desktop", "/Users/sublime/Desktop"),
            ("Documents", "/Users/sublime/Documents"),
            ("Downloads", "/Users/sublime/Downloads"),
        ];

        let mut dy = body_y + 32;
        for (name, path) in favorites.iter() {
            let is_curr = self.current_path == *path;
            if is_curr {
                canvas.fill_rect(card1_x + 2, dy, card1_w - 4, 20, Color::XP_SELECTION);
            }
            draw_string(
                canvas,
                card1_x + 8,
                dy + 2,
                name,
                if is_curr { Color::WHITE } else { Color::rgb(33, 93, 198) },
            );
            dy += 23;
        }

        // Task Group Card 2: Drives / System
        let card2_y = body_y + 136;
        canvas.draw_gradient_h(card1_x, card2_y, card1_w, 20, Color::WHITE, Color::rgb(198, 214, 245));
        draw_string(canvas, card1_x + 8, card2_y + 2, "System", Color::XP_TEXT_LINK);
        canvas.fill_rect(card1_x, card2_y + 20, card1_w, 76, Color::rgb(214, 223, 247));

        let locations = [
            ("Local Disk (C:)", "/"),
            ("Volumes", "/Volumes"),
            ("System Root", "/System"),
        ];

        let mut ly = card2_y + 24;
        for (name, path) in locations.iter() {
            let is_curr = self.current_path == *path;
            if is_curr {
                canvas.fill_rect(card1_x + 2, ly, card1_w - 4, 20, Color::XP_SELECTION);
            }
            draw_string(
                canvas,
                card1_x + 8,
                ly + 2,
                name,
                if is_curr { Color::WHITE } else { Color::rgb(33, 93, 198) },
            );
            ly += 23;
        }

        // 2. Right White Explorer File Pane
        let right_x = x + left_w;
        let right_w = (w - left_w).max(80);
        canvas.fill_rect(right_x, body_y, right_w, body_h, Color::WHITE);

        let mut item_y = body_y + 8;
        for (idx, entry) in self.entries.iter().enumerate() {
            if item_y + 22 > body_y + body_h - 76 {
                break;
            }

            let is_sel = idx == self.selected_index;
            if is_sel {
                canvas.fill_rect(right_x + 6, item_y, right_w - 12, 22, Color::XP_SELECTION);
            }

            // Mini folder or document glyph
            match entry.file_type {
                FileType::Directory => {
                    canvas.fill_rect(right_x + 12, item_y + 4, 14, 12, Color::rgb(248, 204, 72));
                    canvas.draw_rect(right_x + 12, item_y + 4, 14, 12, Color::rgb(176, 128, 18));
                }
                _ => {
                    canvas.fill_rect(right_x + 13, item_y + 3, 12, 14, Color::XP_FACE_LIGHT);
                    canvas.draw_rect(right_x + 13, item_y + 3, 12, 14, Color::XP_INPUT_BORDER);
                }
            }

            draw_string(
                canvas,
                right_x + 34,
                item_y + 3,
                &entry.name,
                if is_sel { Color::WHITE } else { Color::XP_TEXT_BLACK },
            );

            item_y += 23;
        }

        // 3. Details / File Preview Pane
        let prev_y = body_y + body_h - 72;
        canvas.fill_rect(right_x, prev_y, right_w, 72, Color::XP_FACE_LIGHT);
        canvas.fill_rect(right_x, prev_y, right_w, 1, Color::XP_SHADOW);
        draw_string(canvas, right_x + 10, prev_y + 6, "Details / Preview:", Color::XP_TEXT_LINK);

        let mut py = prev_y + 24;
        for line in self.preview_text.lines().take(2) {
            let max_chars = ((right_w - 20) / 8).max(4) as usize;
            let cut = line
                .char_indices()
                .nth(max_chars)
                .map(|(idx, _)| idx)
                .unwrap_or(line.len());
            let display_line = &line[..cut];
            draw_string(canvas, right_x + 10, py, display_line, Color::XP_TEXT_GRAY);
            py += FONT_HEIGHT + 2;
        }

        // 4. Bottom Explorer Status Bar (#ECE9D8)
        let stat_y = y + h - 20;
        canvas.fill_rect(x, stat_y, w, 20, Color::XP_FACE);
        canvas.fill_rect(x, stat_y, w, 1, Color::XP_SHADOW);
        let obj_count = alloc::format!("{} object(s)", self.entries.len());
        draw_string(canvas, x + 8, stat_y + 3, &obj_count, Color::XP_TEXT_BLACK);
        crate::gui::font::draw_string_ellipsis(canvas, x + w - 150, stat_y + 3, "SublimeOS Explorer", Color::XP_TEXT_GRAY, 144);
    }
}

impl FileManagerApp {
    pub fn handle_char(&mut self, ch: char) {
        if ch == '\x08' {
            // Backspace navigates up one folder level safely
            if self.current_path != "/" {
                if let Some(pos) = self.current_path.rfind('/') {
                    if pos == 0 {
                        self.current_path = alloc::string::String::from("/");
                    } else {
                        self.current_path = alloc::string::String::from(&self.current_path[..pos]);
                    }
                    self.selected_index = 0;
                    self.refresh();
                }
            }
        }
    }
}
