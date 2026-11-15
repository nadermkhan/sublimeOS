use alloc::string::String;
use alloc::vec::Vec;
use crate::fs::vfs::VFS;
use crate::gui::canvas::Canvas;
use crate::gui::color::Color;
use crate::gui::font::{
    draw_ui_string, draw_ui_string_ellipsis, measure_ui_string, FONT_HEIGHT,
};

use libgui::event::{GuiEvent, MouseButton};
use libgui::gfx::{Insets, Rect};
use libgui::widget::{MenuBar, MenuBarState, MenuItemSpec, MenuSpec, ScrollBar, ScrollBarState, ScrollOrientation};

const EDITOR_FILE_ITEMS: &[MenuItemSpec] = &[
    MenuItemSpec::action(1, "New", "Ctrl+N"),
    MenuItemSpec::action(2, "Open /etc/rc.sh", ""),
    MenuItemSpec::action(3, "Save", "Ctrl+S"),
];

const EDITOR_EDIT_ITEMS: &[MenuItemSpec] = &[
    MenuItemSpec::action(6, "Copy", "Ctrl+C"),
    MenuItemSpec::action(7, "Paste", "Ctrl+V"),
    MenuItemSpec::sep(),
    MenuItemSpec::action(4, "Clear Document", ""),
    MenuItemSpec::action(5, "Insert System Banner", ""),
];

const EDITOR_MENUS: &[MenuSpec] = &[
    MenuSpec { title: "File", items: EDITOR_FILE_ITEMS },
    MenuSpec { title: "Edit", items: EDITOR_EDIT_ITEMS },
];

pub struct TextEditApp {
    pub file_path: String,
    pub lines: Vec<String>,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub scroll_row: usize,
    pub vbar: ScrollBarState,
    pub menu_state: MenuBarState,
    pub status_msg: String,
    pub context_menu: Option<(i32, i32)>,
    pub ctx_hover: Option<usize>,
}

impl TextEditApp {
    pub fn new() -> Self {
        let mut app = Self {
            file_path: String::from("/Users/untitled.txt"),
            lines: Vec::new(),
            cursor_row: 0,
            cursor_col: 0,
            scroll_row: 0,
            vbar: ScrollBarState::new(0, 0, 12),
            menu_state: MenuBarState::default(),
            status_msg: String::from("Ready — Click or type to write"),
            context_menu: None,
            ctx_hover: None,
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
            '\x03' => {
                // Ctrl+C: Copy active line or entire document to System Clipboard
                let text = if self.lines.len() <= 16 {
                    self.lines.join("\n")
                } else {
                    self.lines[self.cursor_row].clone()
                };
                libgui::system::SystemClipboard::set_text(&text);
                self.status_msg = alloc::format!("Copied {} chars to clipboard", text.len());
            }
            '\x0E' => {
                // Ctrl+N: New document
                self.lines.clear();
                self.lines.push(String::new());
                self.cursor_row = 0;
                self.cursor_col = 0;
                self.scroll_row = 0;
                self.file_path = String::from("/Users/untitled.txt");
                self.status_msg = String::from("New document");
            }
            '\x13' => {
                // Ctrl+S: Save document
                self.save_file();
            }
            '\x16' => {
                // Ctrl+V: Paste from System Clipboard
                let clip = libgui::system::SystemClipboard::get_text();
                if !clip.is_empty() {
                    let count = clip.chars().count();
                    for pc in clip.chars() {
                        if pc == '\n' || (pc >= ' ' && pc <= '~') {
                            self.handle_char(pc);
                        }
                    }
                    self.status_msg = alloc::format!("Pasted {} chars", count);
                }
            }
            '\t' => {
                for _ in 0..4 {
                    self.lines[self.cursor_row].insert(self.cursor_col, ' ');
                    self.cursor_col += 1;
                }
            }
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
        self.ensure_cursor_visible(14);
    }

    fn ensure_cursor_visible(&mut self, visible_rows: usize) {
        let rows = visible_rows.max(4);
        if self.cursor_row < self.scroll_row {
            self.scroll_row = self.cursor_row;
        } else if self.cursor_row >= self.scroll_row + rows {
            self.scroll_row = self.cursor_row + 1 - rows;
        }
    }

    pub fn handle_key(&mut self, key: libgui::event::KeyCode, ch: Option<char>) {
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        if self.cursor_row >= self.lines.len() {
            self.cursor_row = self.lines.len() - 1;
        }
        if self.cursor_col > self.lines[self.cursor_row].len() {
            self.cursor_col = self.lines[self.cursor_row].len();
        }
        match key {
            libgui::event::KeyCode::Left => {
                if self.cursor_col > 0 {
                    self.cursor_col -= 1;
                } else if self.cursor_row > 0 {
                    self.cursor_row -= 1;
                    self.cursor_col = self.lines[self.cursor_row].len();
                }
            }
            libgui::event::KeyCode::Right => {
                let len = self.lines[self.cursor_row].len();
                if self.cursor_col < len {
                    self.cursor_col += 1;
                } else if self.cursor_row + 1 < self.lines.len() {
                    self.cursor_row += 1;
                    self.cursor_col = 0;
                }
            }
            libgui::event::KeyCode::Up => {
                if self.cursor_row > 0 {
                    self.cursor_row -= 1;
                    self.cursor_col = self.cursor_col.min(self.lines[self.cursor_row].len());
                }
            }
            libgui::event::KeyCode::Down => {
                if self.cursor_row + 1 < self.lines.len() {
                    self.cursor_row += 1;
                    self.cursor_col = self.cursor_col.min(self.lines[self.cursor_row].len());
                }
            }
            libgui::event::KeyCode::PageUp => {
                self.cursor_row = self.cursor_row.saturating_sub(10);
                self.cursor_col = self.cursor_col.min(self.lines[self.cursor_row].len());
            }
            libgui::event::KeyCode::PageDown => {
                self.cursor_row = (self.cursor_row + 10).min(self.lines.len().saturating_sub(1));
                self.cursor_col = self.cursor_col.min(self.lines[self.cursor_row].len());
            }
            libgui::event::KeyCode::Home => self.cursor_col = 0,
            libgui::event::KeyCode::End => self.cursor_col = self.lines[self.cursor_row].len(),
            libgui::event::KeyCode::Delete => {
                let len = self.lines[self.cursor_row].len();
                if self.cursor_col < len {
                    self.lines[self.cursor_row].remove(self.cursor_col);
                } else if self.cursor_row + 1 < self.lines.len() {
                    let next = self.lines.remove(self.cursor_row + 1);
                    self.lines[self.cursor_row].push_str(&next);
                }
            }
            libgui::event::KeyCode::Enter => self.handle_char('\n'),
            libgui::event::KeyCode::Backspace => self.handle_char('\x08'),
            libgui::event::KeyCode::Tab => self.handle_char('\t'),
            _ => {
                if let Some(c) = ch {
                    self.handle_char(c);
                }
            }
        }
        self.ensure_cursor_visible(14);
    }

    const SB_W: i32 = 14;

    fn scrollbar_metrics(&self, ix: i32, iy: i32, iw: i32, ih: i32) -> (Rect, usize, usize) {
        let doc_y = iy + 50;
        let doc_h = (ih - 50).max(16);
        let line_h = FONT_HEIGHT + 4;
        let vis_rows = ((doc_h - 10) / line_h).max(1) as usize;
        let max_scroll = self.lines.len().saturating_sub(vis_rows);
        let sb_rect = Rect::new(ix + iw - Self::SB_W - 1, doc_y + 1, Self::SB_W, (doc_h - 2).max(12));
        (sb_rect, vis_rows, max_scroll)
    }

    pub fn handle_wheel(&mut self, delta_y: i32) -> bool {
        if delta_y == 0 {
            return false;
        }
        let max_scroll = self.lines.len().saturating_sub(4);
        let prev = self.scroll_row;
        if delta_y > 0 {
            self.scroll_row = self.scroll_row.saturating_sub(delta_y as usize * 2);
        } else {
            self.scroll_row = (self.scroll_row + (-delta_y as usize) * 2).min(max_scroll);
        }
        self.vbar.sync_range(self.scroll_row as i32, max_scroll as i32, 12);
        self.scroll_row != prev
    }

    pub fn handle_mouse_move(&mut self, px: i32, py: i32, ix: i32, iy: i32, iw: i32, ih: i32) -> bool {
        let mut changed = false;
        if let Some((cx, cy)) = self.context_menu {
            let cw = 136;
            let row_h = 22;
            let ch = row_h * 4 + 4;
            let next = if px >= cx && px < cx + cw && py >= cy + 2 && py < cy + ch - 2 {
                Some(((py - cy - 2) / row_h) as usize)
            } else {
                None
            };
            if next != self.ctx_hover {
                self.ctx_hover = next;
                changed = true;
            }
        }
        let menu_bar = Rect::new(ix, iy, iw, 20);
        let (mres, _) = MenuBar::handle_event(
            &mut self.menu_state,
            menu_bar,
            EDITOR_MENUS,
            GuiEvent::MouseMove { x: px, y: py },
        );
        let (sb_rect, vis_rows, max_scroll) = self.scrollbar_metrics(ix, iy, iw, ih);
        self.scroll_row = self.scroll_row.min(max_scroll);
        self.vbar.sync_range(self.scroll_row as i32, max_scroll as i32, vis_rows as i32);
        let (sb_res, sb_ch) = ScrollBar::handle_event(
            &mut self.vbar,
            sb_rect,
            ScrollOrientation::Vertical,
            GuiEvent::MouseMove { x: px, y: py },
        );
        if sb_ch {
            self.scroll_row = (self.vbar.value.max(0) as usize).min(max_scroll);
        }
        changed || mres.needs_redraw() || sb_res.needs_redraw()
    }

    pub fn handle_mouse_up(&mut self, px: i32, py: i32, ix: i32, iy: i32, iw: i32, ih: i32) -> bool {
        let (sb_rect, vis_rows, max_scroll) = self.scrollbar_metrics(ix, iy, iw, ih);
        self.vbar.sync_range(self.scroll_row as i32, max_scroll as i32, vis_rows as i32);
        let (sb_res, sb_ch) = ScrollBar::handle_event(
            &mut self.vbar,
            sb_rect,
            ScrollOrientation::Vertical,
            GuiEvent::MouseUp {
                x: px,
                y: py,
                button: MouseButton::Left,
            },
        );
        if sb_ch {
            self.scroll_row = (self.vbar.value.max(0) as usize).min(max_scroll);
        }
        sb_res.needs_redraw()
    }

    pub fn handle_right_click(&mut self, px: i32, py: i32, ix: i32, iy: i32, iw: i32, ih: i32) {
        if px >= ix && px < ix + iw && py >= iy + 20 && py < iy + ih {
            self.menu_state.open_menu = None;
            let cx = px.min(ix + iw - 138).max(ix + 2);
            let cy = py.min(iy + ih - 94).max(iy + 20);
            self.context_menu = Some((cx, cy));
            self.ctx_hover = None;
        } else {
            self.context_menu = None;
        }
    }

    pub fn handle_click(&mut self, px: i32, py: i32, ix: i32, iy: i32, iw: i32, ih: i32) {
        if let Some((cx, cy)) = self.context_menu {
            let cw = 136;
            let row_h = 22;
            let ch = row_h * 4 + 4;
            if px >= cx && px < cx + cw && py >= cy + 2 && py < cy + ch - 2 {
                let idx = ((py - cy - 2) / row_h) as usize;
                match idx {
                    0 => self.handle_char('\x03'),
                    1 => self.handle_char('\x16'),
                    2 => self.save_file(),
                    3 => {
                        self.lines.clear();
                        self.lines.push(String::new());
                        self.cursor_row = 0;
                        self.cursor_col = 0;
                        self.scroll_row = 0;
                        self.status_msg = String::from("Cleared");
                    }
                    _ => {}
                }
                self.context_menu = None;
                return;
            }
            self.context_menu = None;
        }

        let menu_bar = Rect::new(ix, iy, iw, 20);
        let (mres, action) = MenuBar::handle_event(
            &mut self.menu_state,
            menu_bar,
            EDITOR_MENUS,
            GuiEvent::MouseDown { x: px, y: py, button: MouseButton::Left },
        );
        if let Some(cmd) = action {
            match cmd {
                1 => {
                    self.lines.clear();
                    self.lines.push(String::new());
                    self.cursor_row = 0;
                    self.cursor_col = 0;
                    self.scroll_row = 0;
                    self.file_path = String::from("/Users/untitled.txt");
                    self.status_msg = String::from("New document");
                }
                2 => self.open_file("/etc/rc.sh"),
                3 => self.save_file(),
                4 => {
                    self.lines.clear();
                    self.lines.push(String::new());
                    self.cursor_row = 0;
                    self.cursor_col = 0;
                    self.scroll_row = 0;
                    self.status_msg = String::from("Cleared");
                }
                5 => {
                    self.lines.push(String::from("# Generated by SublimeOS Notepad"));
                    self.status_msg = String::from("Inserted banner");
                }
                6 => self.handle_char('\x03'),
                7 => self.handle_char('\x16'),
                _ => {}
            }
            return;
        }
        if mres.consumed() {
            return;
        }

        // Toolbar buttons at top: [New] [Open rc.sh] [Save]
        if py >= iy + 22 && py <= iy + 48 {
            if px >= ix + 8 && px <= ix + 60 {
                self.lines.clear();
                self.lines.push(String::new());
                self.cursor_row = 0;
                self.cursor_col = 0;
                self.scroll_row = 0;
                self.file_path = String::from("/Users/untitled.txt");
                self.status_msg = String::from("New document");
            } else if px >= ix + 66 && px <= ix + 160 {
                self.open_file("/etc/rc.sh");
            } else if px >= ix + 166 && px <= ix + 220 {
                self.save_file();
            }
        } else if py >= iy + 54 {
            let (sb_rect, vis_rows, max_scroll) = self.scrollbar_metrics(ix, iy, iw, ih);
            self.scroll_row = self.scroll_row.min(max_scroll);
            self.vbar.sync_range(self.scroll_row as i32, max_scroll as i32, vis_rows as i32);
            let (_, sb_ch) = ScrollBar::handle_event(
                &mut self.vbar,
                sb_rect,
                ScrollOrientation::Vertical,
                GuiEvent::MouseDown {
                    x: px,
                    y: py,
                    button: MouseButton::Left,
                },
            );
            if sb_ch {
                self.scroll_row = (self.vbar.value.max(0) as usize).min(max_scroll);
                return;
            }
            if sb_rect.contains(px, py) {
                return;
            }
            let line_h = FONT_HEIGHT + 4;
            let rel_row = ((py - (iy + 56)).max(0) / line_h) as usize;
            let target_row = (self.scroll_row + rel_row).min(self.lines.len().saturating_sub(1));
            self.cursor_row = target_row;
            let rel_x = (px - (ix + 10)).max(0);
            let line = &self.lines[target_row];
            let mut best_col = line.len();
            let mut best_diff = i32::MAX;
            for (byte_idx, _) in line.char_indices().chain(core::iter::once((line.len(), '\0'))) {
                let w = measure_ui_string(&line[..byte_idx]);
                let d = (w - rel_x).abs();
                if d < best_diff {
                    best_diff = d;
                    best_col = byte_idx;
                }
            }
            self.cursor_col = best_col;
        }
    }

    pub fn render(&mut self, canvas: &mut Canvas, ix: i32, iy: i32, iw: i32, ih: i32) {
        let theme = libgui::gfx::current_theme();
        let face = Color::from_libgui(theme.face);
        let text_pri = Color::from_libgui(theme.text_primary);
        let text_sec = Color::from_libgui(theme.text_secondary);
        let input_bg = Color::from_libgui(theme.input_bg);
        let input_border = Color::from_libgui(theme.input_border);
        let shadow = Color::from_libgui(theme.border_shadow);

        // 1. Notepad Menu + Toolbar Strip
        canvas.fill_rect(ix, iy, iw, 50, face);
        let menu_bar = Rect::new(ix, iy, iw, 20);
        canvas.with_surface(|surf| {
            MenuBar::draw_bar(surf, menu_bar, EDITOR_MENUS, self.menu_state, &theme);
        });

        // Action Buttons: [New] [Open rc.sh] [Save]
        let btn_specs = [
            (ix + 8, 52, "New"),
            (ix + 66, 94, "Open rc.sh"),
            (ix + 166, 54, "Save"),
        ];
        for (bx, bw, lbl) in btn_specs {
            canvas.draw_gradient_v(bx, iy + 24, bw, 22, Color::from_libgui(theme.face_light), Color::from_libgui(theme.face_dark));
            canvas.draw_rounded_rect(bx, iy + 24, bw, 22, 3, Color::from_libgui(theme.ctrl_border));
            let lw = measure_ui_string(lbl);
            draw_ui_string(canvas, bx + (bw - lw) / 2, iy + 26, lbl, text_pri);
        }

        draw_ui_string_ellipsis(canvas, ix + 230, iy + 26, &self.status_msg, text_sec, (iw - 238).max(20));
        canvas.fill_rect(ix, iy + 49, iw, 1, shadow);

        // 2. Crisp Notepad Paper Canvas (rendered in Noto Sans TTF)
        let doc_y = iy + 50;
        let doc_h = (ih - 50).max(16);
        canvas.fill_rect(ix, doc_y, iw, doc_h, input_bg);
        canvas.draw_rect(ix, doc_y, iw, doc_h, input_border);

        let mut curr_y = doc_y + 6;
        let left_margin = ix + 10;
        let max_text_w = (iw - 22 - Self::SB_W).max(32);
        let line_h = FONT_HEIGHT + 4;

        for (row_idx, line) in self.lines.iter().enumerate().skip(self.scroll_row) {
            if curr_y + line_h > iy + ih - 4 {
                break;
            }

            draw_ui_string_ellipsis(canvas, left_margin, curr_y, line, text_pri, max_text_w);

            if row_idx == self.cursor_row {
                let col_byte = self.cursor_col.min(line.len());
                let px_off = measure_ui_string(&line[..col_byte]);
                let cursor_x = (left_margin + px_off).min(ix + iw - 14 - Self::SB_W);
                canvas.fill_rect(cursor_x, curr_y + 2, 2, FONT_HEIGHT, text_pri);
            }

            curr_y += line_h;
        }

        let (sb_rect, vis_rows, max_scroll) = self.scrollbar_metrics(ix, iy, iw, ih);
        self.vbar.sync_range(self.scroll_row.min(max_scroll) as i32, max_scroll as i32, vis_rows as i32);
        canvas.with_surface(|surf| {
            ScrollBar::draw(surf, sb_rect, ScrollOrientation::Vertical, self.vbar, &theme);
        });

        if let Some((cx, cy)) = self.context_menu {
            let cw = 136;
            let row_h = 22;
            let ch = row_h * 4 + 4;
            canvas.fill_rect(cx, cy, cw, ch, Color::from_libgui(theme.face_light));
            canvas.draw_rect(cx, cy, cw, ch, Color::from_libgui(theme.ctrl_border));
            let items = ["Copy", "Paste", "Save", "Clear All"];
            for (i, label) in items.iter().enumerate() {
                let ry = cy + 2 + i as i32 * row_h;
                let hovered = self.ctx_hover == Some(i);
                if hovered {
                    canvas.fill_rect(cx + 2, ry, cw - 4, row_h, Color::from_libgui(theme.selection));
                }
                let tc = if hovered { Color::from_libgui(theme.selection_text) } else { text_pri };
                draw_ui_string(canvas, cx + 10, ry + 3, label, tc);
            }
        }

        // Draw open menu dropdown above editor body
        let _ = Insets::all(0);
        canvas.with_surface(|surf| {
            MenuBar::draw_popup(surf, menu_bar, EDITOR_MENUS, self.menu_state, &theme);
        });
    }
}
