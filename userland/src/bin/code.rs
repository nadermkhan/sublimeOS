#![no_std]
#![no_main]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use libgui::app::{WindowClient, WindowSpec};
use libgui::code_highlight::{detect_language, token_color, tokenize_line, CodeLanguage};
use libgui::component::ComponentContext;
use libgui::event::{EventResult, GuiEvent, KeyCode, Modifiers, MouseButton};
use libgui::font::{with_default_font, TextStyle};
use libgui::gfx::{contrast_text_on, Color, Rect, Surface, Theme, ThemeKind};
use libgui::widget::{ScrollBar, ScrollBarState, ScrollOrientation};
use sublime_rt::{
    args, getpid, println, read_dir, read_to_string, syscommand, write_file, SublimeWmOs,
};

#[derive(Clone)]
struct UndoSnapshot {
    lines: Vec<String>,
    row: usize,
    col: usize,
}

#[derive(Clone)]
struct EditorTab {
    path: String,
    title: String,
    lang: CodeLanguage,
    lines: Vec<String>,
    cursor_row: usize,
    cursor_col: usize,
    scroll_row: usize,
    dirty: bool,
    undo: Vec<UndoSnapshot>,
}

impl EditorTab {
    fn from_content(path: &str, text: &str) -> Self {
        let mut lines: Vec<String> = text.lines().map(String::from).collect();
        if text.ends_with('\n') {
            lines.push(String::new());
        }
        if lines.is_empty() {
            lines.push(String::new());
        }
        let title = String::from(path.rsplit('/').next().unwrap_or(path));
        Self {
            path: String::from(path),
            title,
            lang: detect_language(path),
            lines,
            cursor_row: 0,
            cursor_col: 0,
            scroll_row: 0,
            dirty: false,
            undo: Vec::new(),
        }
    }

    fn push_undo(&mut self) {
        if self.undo.len() >= 32 {
            self.undo.remove(0);
        }
        self.undo.push(UndoSnapshot {
            lines: self.lines.clone(),
            row: self.cursor_row,
            col: self.cursor_col,
        });
    }

    fn undo(&mut self) -> bool {
        if let Some(snap) = self.undo.pop() {
            self.lines = snap.lines;
            self.cursor_row = snap.row.min(self.lines.len().saturating_sub(1));
            self.cursor_col = snap.col;
            self.dirty = true;
            true
        } else {
            false
        }
    }

    fn serialized(&self) -> String {
        let mut out = String::new();
        for (i, line) in self.lines.iter().enumerate() {
            out.push_str(line);
            if i + 1 < self.lines.len() {
                out.push('\n');
            }
        }
        out
    }
}

#[derive(Clone)]
struct WorkspaceFile {
    label: String,
    path: String,
}

struct CodeEditorApp {
    ctx: ComponentContext,
    tabs: Vec<EditorTab>,
    active_tab: usize,
    workspace_files: Vec<WorkspaceFile>,
    sidebar_open: bool,
    sidebar_scroll: usize,
    sidebar_vbar: ScrollBarState,
    editor_vbar: ScrollBarState,
    dark_editor: bool,
    find_open: bool,
    find_query: String,
    output_open: bool,
    output_lines: Vec<String>,
    status: String,
    new_seq: usize,
    dirty_all: bool,
}

impl CodeEditorApp {
    fn new(initial_path: Option<&str>) -> Self {
        let mut app = Self {
            ctx: ComponentContext::new(),
            tabs: Vec::new(),
            active_tab: 0,
            workspace_files: Vec::new(),
            sidebar_open: true,
            sidebar_scroll: 0,
            sidebar_vbar: ScrollBarState::new(0, 0, 12),
            editor_vbar: ScrollBarState::new(0, 0, 16),
            dark_editor: true,
            find_open: false,
            find_query: String::new(),
            output_open: false,
            output_lines: Vec::new(),
            status: String::from("Ctrl+S Save • Ctrl+F Find • Ctrl+Z Undo • F5 Run"),
            new_seq: 1,
            dirty_all: true,
        };
        app.scan_workspace();
        if let Some(p) = initial_path {
            app.open_path(p);
        } else if !app.workspace_files.is_empty() {
            let p = app.workspace_files[0].path.clone();
            app.open_path(&p);
        } else {
            app.tabs.push(EditorTab::from_content(
                "/Users/sublime/Documents/main.rs",
                "// Welcome to Sublime Code Editor\nfn main() {\n    let msg = \"Hello SublimeOS\";\n    println!(\"{}\", msg);\n}\n",
            ));
        }
        app
    }

    fn scan_workspace(&mut self) {
        self.workspace_files.clear();
        let dirs = ["/Users/sublime/Documents", "/etc", "/Users/sublime"];
        for dir in dirs {
            if let Ok(entries) = read_dir(dir) {
                for e in entries {
                    if e.is_dir {
                        continue;
                    }
                    let path = format!("{}/{}", dir, e.name);
                    if e.name.ends_with(".rs")
                        || e.name.ends_with(".c")
                        || e.name.ends_with(".py")
                        || e.name.ends_with(".js")
                        || e.name.ends_with(".json")
                        || e.name.ends_with(".sh")
                        || e.name.ends_with(".md")
                        || e.name.ends_with(".txt")
                        || e.name.ends_with(".sublimerc")
                        || e.name.ends_with(".toml")
                    {
                        self.workspace_files.push(WorkspaceFile {
                            label: e.name,
                            path,
                        });
                    }
                }
            }
        }
    }

    fn open_path(&mut self, path: &str) {
        if let Some(idx) = self.tabs.iter().position(|t| t.path == path) {
            self.active_tab = idx;
            self.status = format!("Switched to {}", path);
            self.dirty_all = true;
            return;
        }
        match read_to_string(path) {
            Ok(txt) => {
                self.tabs.push(EditorTab::from_content(path, &txt));
                self.active_tab = self.tabs.len() - 1;
                self.status = format!("Opened {} ({} bytes)", path, txt.len());
            }
            Err(e) => {
                // Create editable buffer for new path
                self.tabs.push(EditorTab::from_content(path, ""));
                self.active_tab = self.tabs.len() - 1;
                self.status = format!("New buffer {} (read code {})", path, e);
            }
        }
        self.dirty_all = true;
    }

    fn new_untitled(&mut self) {
        let p = format!("/Users/sublime/Documents/script_{}.rs", self.new_seq);
        self.new_seq += 1;
        self.tabs.push(EditorTab::from_content(
            &p,
            "// SublimeOS Rust Snippet\nfn run() -> u32 {\n    42\n}\n",
        ));
        self.active_tab = self.tabs.len() - 1;
        self.status = format!("Created {}", p);
    }

    fn save_active(&mut self) {
        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
            let content = tab.serialized();
            match write_file(&tab.path, content.as_bytes()) {
                Ok(()) => {
                    tab.dirty = false;
                    self.status = format!("Saved {} ({} bytes)", tab.path, content.len());
                    self.scan_workspace();
                }
                Err(e) => {
                    self.status = format!("Save failed ({}): {}", e, tab.path);
                }
            }
        }
    }

    fn find_next(&mut self) {
        if self.find_query.is_empty() {
            return;
        }
        let Some(tab) = self.tabs.get_mut(self.active_tab) else {
            return;
        };
        let n = tab.lines.len();
        for offset in 0..n {
            let r = (tab.cursor_row + offset) % n;
            let start_c = if offset == 0 {
                (tab.cursor_col + 1).min(tab.lines[r].len())
            } else {
                0
            };
            if let Some(pos) = tab.lines[r][start_c..].find(&self.find_query) {
                tab.cursor_row = r;
                tab.cursor_col = start_c + pos;
                if tab.cursor_row < tab.scroll_row {
                    tab.scroll_row = tab.cursor_row;
                }
                self.status = format!("Match at Ln {}, Col {}", r + 1, tab.cursor_col + 1);
                return;
            }
        }
        self.status = format!("No match for \"{}\"", self.find_query);
    }

    fn run_active_file(&mut self) {
        self.save_active();
        self.output_open = true;
        self.output_lines.clear();
        let Some(tab) = self.tabs.get(self.active_tab) else {
            return;
        };
        self.output_lines.push(format!("[Sublime Code Runner] Target: {} ({})", tab.path, tab.lang.name()));

        match tab.lang {
            CodeLanguage::Shell => {
                let mut ran = 0usize;
                for line in &tab.lines {
                    let trimmed = line.trim();
                    if trimmed.is_empty() || trimmed.starts_with('#') {
                        continue;
                    }
                    let rc = syscommand(trimmed);
                    self.output_lines.push(format!("$ {}  -> exit {}", trimmed, rc));
                    ran += 1;
                }
                if ran == 0 {
                    self.output_lines.push(String::from("(No executable shell lines)"));
                }
            }
            CodeLanguage::Json => {
                let text = tab.serialized();
                let mut depth = 0i32;
                let mut ok = true;
                for ch in text.chars() {
                    match ch {
                        '{' | '[' => depth += 1,
                        '}' | ']' => {
                            depth -= 1;
                            if depth < 0 {
                                ok = false;
                            }
                        }
                        _ => {}
                    }
                }
                if ok && depth == 0 {
                    self.output_lines.push(format!("JSON Validated OK ({} lines, balanced braces)", tab.lines.len()));
                } else {
                    self.output_lines.push(format!("JSON Syntax Warning: unbalanced brackets (depth={})", depth));
                }
            }
            _ => {
                let line_cnt = tab.lines.len();
                let char_cnt: usize = tab.lines.iter().map(|l| l.len()).sum();
                let mut brace_bal = 0i32;
                for line in &tab.lines {
                    for ch in line.chars() {
                        if ch == '{' { brace_bal += 1; } else if ch == '}' { brace_bal -= 1; }
                    }
                }
                self.output_lines.push(format!(
                    "Static Check OK: {} lines, {} chars, brace balance {}",
                    line_cnt, char_cnt, brace_bal
                ));
                let _ = syscommand(&format!("echo [code] Checked {}", tab.title));
            }
        }
        self.status = String::from("Run completed — see Output drawer");
    }

    fn ensure_cursor_visible(&mut self, visible_rows: usize) {
        let Some(tab) = self.tabs.get_mut(self.active_tab) else {
            return;
        };
        if tab.cursor_row < tab.scroll_row {
            tab.scroll_row = tab.cursor_row;
        } else if visible_rows > 0 && tab.cursor_row >= tab.scroll_row + visible_rows {
            tab.scroll_row = tab.cursor_row + 1 - visible_rows;
        }
    }

    fn editor_area(&self, bounds: Rect) -> Rect {
        let side_w = if self.sidebar_open { 164 } else { 0 };
        let top_h = 56 + if self.find_open { 28 } else { 0 };
        let out_h = if self.output_open { 96 } else { 0 };
        let status_h = 20;
        Rect::new(
            bounds.x + side_w,
            bounds.y + top_h,
            (bounds.w - side_w).max(160),
            (bounds.h - top_h - out_h - status_h).max(60),
        )
    }

    fn edit_key(&mut self, key: KeyCode, ch: Option<char>, mods: Modifiers, bounds: Rect) -> bool {
        if self.find_open && !mods.ctrl {
            match key {
                KeyCode::Escape => {
                    self.find_open = false;
                    return true;
                }
                KeyCode::Enter => {
                    self.find_next();
                    return true;
                }
                KeyCode::Backspace => {
                    self.find_query.pop();
                    return true;
                }
                _ => {
                    if let Some(c) = ch {
                        if !c.is_control() {
                            self.find_query.push(c);
                            return true;
                        }
                    }
                }
            }
        }

        if mods.ctrl {
            let lower = ch.map(|c| c.to_ascii_lowercase());
            if lower == Some('s') || matches!(key, KeyCode::Char('s') | KeyCode::Char('S')) {
                self.save_active();
                return true;
            }
            if lower == Some('f') || matches!(key, KeyCode::Char('f') | KeyCode::Char('F')) {
                self.find_open = !self.find_open;
                return true;
            }
            if lower == Some('z') || matches!(key, KeyCode::Char('z') | KeyCode::Char('Z')) {
                if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                    if tab.undo() {
                        self.status = String::from("Undid last change");
                    }
                }
                return true;
            }
            if lower == Some('n') || matches!(key, KeyCode::Char('n') | KeyCode::Char('N')) {
                self.new_untitled();
                return true;
            }
            if lower == Some('b') || matches!(key, KeyCode::Char('b') | KeyCode::Char('B')) {
                self.sidebar_open = !self.sidebar_open;
                return true;
            }
        }

        if matches!(key, KeyCode::Function(5)) {
            self.run_active_file();
            return true;
        }

        let ed = self.editor_area(bounds);
        let vis_rows = ((ed.h - 8) / 18).max(1) as usize;

        let Some(tab) = self.tabs.get_mut(self.active_tab) else {
            return false;
        };

        let mut edited = true;
        match key {
            KeyCode::Left => {
                if tab.cursor_col > 0 {
                    tab.cursor_col -= 1;
                } else if tab.cursor_row > 0 {
                    tab.cursor_row -= 1;
                    tab.cursor_col = tab.lines[tab.cursor_row].len();
                }
            }
            KeyCode::Right => {
                let line_len = tab.lines[tab.cursor_row].len();
                if tab.cursor_col < line_len {
                    tab.cursor_col += 1;
                } else if tab.cursor_row + 1 < tab.lines.len() {
                    tab.cursor_row += 1;
                    tab.cursor_col = 0;
                }
            }
            KeyCode::Up => {
                if tab.cursor_row > 0 {
                    tab.cursor_row -= 1;
                    tab.cursor_col = tab.cursor_col.min(tab.lines[tab.cursor_row].len());
                }
            }
            KeyCode::Down => {
                if tab.cursor_row + 1 < tab.lines.len() {
                    tab.cursor_row += 1;
                    tab.cursor_col = tab.cursor_col.min(tab.lines[tab.cursor_row].len());
                }
            }
            KeyCode::Home => tab.cursor_col = 0,
            KeyCode::End => tab.cursor_col = tab.lines[tab.cursor_row].len(),
            KeyCode::PageUp => {
                tab.cursor_row = tab.cursor_row.saturating_sub(vis_rows);
                tab.cursor_col = tab.cursor_col.min(tab.lines[tab.cursor_row].len());
            }
            KeyCode::PageDown => {
                tab.cursor_row = (tab.cursor_row + vis_rows).min(tab.lines.len() - 1);
                tab.cursor_col = tab.cursor_col.min(tab.lines[tab.cursor_row].len());
            }
            KeyCode::Enter => {
                tab.push_undo();
                let cur = tab.lines[tab.cursor_row].clone();
                let split_col = tab.cursor_col.min(cur.len());
                let (left, right) = cur.split_at(split_col);
                let base_indent: usize = left.chars().take_while(|c| *c == ' ').count();
                let extra = if left.trim_end().ends_with('{') || left.trim_end().ends_with(':') {
                    4
                } else {
                    0
                };
                let mut next_line = String::new();
                for _ in 0..(base_indent + extra) {
                    next_line.push(' ');
                }
                next_line.push_str(right);
                tab.lines[tab.cursor_row] = String::from(left);
                tab.cursor_row += 1;
                tab.lines.insert(tab.cursor_row, next_line);
                tab.cursor_col = base_indent + extra;
                tab.dirty = true;
            }
            KeyCode::Backspace => {
                tab.push_undo();
                if tab.cursor_col > 0 {
                    let pos = tab.cursor_col.min(tab.lines[tab.cursor_row].len());
                    if pos > 0 {
                        tab.lines[tab.cursor_row].remove(pos - 1);
                        tab.cursor_col = pos - 1;
                        tab.dirty = true;
                    }
                } else if tab.cursor_row > 0 {
                    let cur_line = tab.lines.remove(tab.cursor_row);
                    tab.cursor_row -= 1;
                    let prev_len = tab.lines[tab.cursor_row].len();
                    tab.lines[tab.cursor_row].push_str(&cur_line);
                    tab.cursor_col = prev_len;
                    tab.dirty = true;
                }
            }
            KeyCode::Delete => {
                tab.push_undo();
                let len = tab.lines[tab.cursor_row].len();
                if tab.cursor_col < len {
                    tab.lines[tab.cursor_row].remove(tab.cursor_col);
                    tab.dirty = true;
                } else if tab.cursor_row + 1 < tab.lines.len() {
                    let next = tab.lines.remove(tab.cursor_row + 1);
                    tab.lines[tab.cursor_row].push_str(&next);
                    tab.dirty = true;
                }
            }
            KeyCode::Tab => {
                tab.push_undo();
                let pos = tab.cursor_col.min(tab.lines[tab.cursor_row].len());
                tab.lines[tab.cursor_row].insert_str(pos, "    ");
                tab.cursor_col = pos + 4;
                tab.dirty = true;
            }
            _ => {
                if let Some(c) = ch {
                    if c.is_ascii() && !c.is_control() {
                        tab.push_undo();
                        let pos = tab.cursor_col.min(tab.lines[tab.cursor_row].len());
                        tab.lines[tab.cursor_row].insert(pos, c);
                        tab.cursor_col = pos + 1;
                        tab.dirty = true;
                    } else {
                        edited = false;
                    }
                } else {
                    edited = false;
                }
            }
        }
        self.ensure_cursor_visible(vis_rows);
        edited
    }
}

impl WindowClient for CodeEditorApp {
    fn spec(&self) -> WindowSpec {
        WindowSpec {
            title: String::from("Sublime Code Editor (Ring-3 App)"),
            width: 720,
            height: 480,
            min_width: 500,
            min_height: 320,
            resizable: true,
        }
    }

    fn on_mount(&mut self, _bounds: Rect) {
        self.dirty_all = true;
    }

    fn on_theme_changed(&mut self, _theme: ThemeKind) {
        self.dirty_all = true;
    }

    fn on_event(&mut self, event: GuiEvent, bounds: Rect) -> EventResult {
        match event {
            GuiEvent::Resize { .. } => {
                self.dirty_all = true;
                self.ctx.damage.invalidate(bounds);
                EventResult::NeedsRedraw
            }
            GuiEvent::KeyDown { key, ch, mods } => {
                if self.edit_key(key, ch, mods, bounds) {
                    self.ctx.damage.invalidate(bounds);
                    EventResult::NeedsRedraw
                } else {
                    EventResult::Ignored
                }
            }
            GuiEvent::MouseWheel { x, y, delta_y } => {
                let side_w = if self.sidebar_open { 164 } else { 0 };
                if self.sidebar_open && x < bounds.x + side_w {
                    let side_h = (bounds.h - 76).max(16);
                    let vis = (side_h / 22).max(1) as usize;
                    let max_s = self.workspace_files.len().saturating_sub(vis);
                    if delta_y > 0 {
                        self.sidebar_scroll = self.sidebar_scroll.saturating_sub(delta_y as usize * 2);
                    } else if delta_y < 0 {
                        self.sidebar_scroll = (self.sidebar_scroll + (-delta_y as usize) * 2).min(max_s);
                    }
                    self.sidebar_vbar.sync_range(self.sidebar_scroll as i32, max_s as i32, vis as i32);
                } else {
                    let ed = self.editor_area(bounds);
                    let vis_rows = ((ed.h - 8) / 18).max(1) as usize;
                    if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                        let max_s = tab.lines.len().saturating_sub(vis_rows);
                        if delta_y > 0 {
                            tab.scroll_row = tab.scroll_row.saturating_sub(delta_y as usize * 3);
                        } else if delta_y < 0 {
                            tab.scroll_row = (tab.scroll_row + (-delta_y as usize) * 3).min(max_s);
                        }
                        self.editor_vbar.sync_range(tab.scroll_row as i32, max_s as i32, vis_rows as i32);
                    }
                    let _ = y;
                }
                self.ctx.damage.invalidate(bounds);
                EventResult::NeedsRedraw
            }
            GuiEvent::MouseMove { x, y } => {
                let mut redraw = false;
                if self.sidebar_open {
                    let side_r = Rect::new(bounds.x + 150, bounds.y + 56, 14, (bounds.h - 76).max(16));
                    let vis = (side_r.h / 22).max(1) as usize;
                    let max_s = self.workspace_files.len().saturating_sub(vis);
                    self.sidebar_vbar.sync_range(self.sidebar_scroll.min(max_s) as i32, max_s as i32, vis as i32);
                    let (res, ch) = ScrollBar::handle_event(&mut self.sidebar_vbar, side_r, ScrollOrientation::Vertical, GuiEvent::MouseMove { x, y });
                    if ch {
                        self.sidebar_scroll = (self.sidebar_vbar.value.max(0) as usize).min(max_s);
                    }
                    redraw |= ch || res.needs_redraw();
                }
                let ed = self.editor_area(bounds);
                let ed_sb = Rect::new(ed.right() - 14, ed.y, 14, ed.h.max(16));
                let vis_rows = ((ed.h - 8) / 18).max(1) as usize;
                if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                    let max_s = tab.lines.len().saturating_sub(vis_rows);
                    self.editor_vbar.sync_range(tab.scroll_row.min(max_s) as i32, max_s as i32, vis_rows as i32);
                    let (res, ch) = ScrollBar::handle_event(&mut self.editor_vbar, ed_sb, ScrollOrientation::Vertical, GuiEvent::MouseMove { x, y });
                    if ch {
                        tab.scroll_row = (self.editor_vbar.value.max(0) as usize).min(max_s);
                    }
                    redraw |= ch || res.needs_redraw();
                }
                if redraw {
                    self.ctx.damage.invalidate(bounds);
                    EventResult::NeedsRedraw
                } else {
                    EventResult::Ignored
                }
            }
            GuiEvent::MouseUp { x, y, button: MouseButton::Left } => {
                let mut redraw = false;
                if self.sidebar_open {
                    let side_r = Rect::new(bounds.x + 150, bounds.y + 56, 14, (bounds.h - 76).max(16));
                    let (res, _) = ScrollBar::handle_event(&mut self.sidebar_vbar, side_r, ScrollOrientation::Vertical, GuiEvent::MouseUp { x, y, button: MouseButton::Left });
                    redraw |= res.needs_redraw();
                }
                let ed = self.editor_area(bounds);
                let ed_sb = Rect::new(ed.right() - 14, ed.y, 14, ed.h.max(16));
                let (res, _) = ScrollBar::handle_event(&mut self.editor_vbar, ed_sb, ScrollOrientation::Vertical, GuiEvent::MouseUp { x, y, button: MouseButton::Left });
                redraw |= res.needs_redraw();
                if redraw {
                    self.ctx.damage.invalidate(bounds);
                    EventResult::NeedsRedraw
                } else {
                    EventResult::Ignored
                }
            }
            GuiEvent::MouseDown {
                x,
                y,
                button: MouseButton::Left,
            } => {
                // Toolbar row (y .. y+32)
                if y >= bounds.y && y < bounds.y + 32 {
                    let mut bx = bounds.x + 8;
                    let btns = [
                        (48, "Files"),
                        (44, "New"),
                        (46, "Save"),
                        (48, "Undo"),
                        (46, "Find"),
                        (46, "Run"),
                        (56, if self.dark_editor { "Light" } else { "Dark" }),
                    ];
                    for (idx, (bw, _)) in btns.iter().enumerate() {
                        if x >= bx && x < bx + *bw {
                            match idx {
                                0 => self.sidebar_open = !self.sidebar_open,
                                1 => self.new_untitled(),
                                2 => self.save_active(),
                                3 => {
                                    if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                                        let _ = tab.undo();
                                    }
                                }
                                4 => self.find_open = !self.find_open,
                                5 => self.run_active_file(),
                                6 => self.dark_editor = !self.dark_editor,
                                _ => {}
                            }
                            self.ctx.damage.invalidate(bounds);
                            return EventResult::NeedsRedraw;
                        }
                        bx += *bw + 4;
                    }
                }

                // Tab bar row (y+32 .. y+56)
                let side_w = if self.sidebar_open { 164 } else { 0 };
                if y >= bounds.y + 32 && y < bounds.y + 56 && x >= bounds.x + side_w {
                    let mut tx = bounds.x + side_w + 4;
                    for i in 0..self.tabs.len() {
                        let tw = 112;
                        if x >= tx && x < tx + tw {
                            self.active_tab = i;
                            self.ctx.damage.invalidate(bounds);
                            return EventResult::NeedsRedraw;
                        }
                        tx += tw + 2;
                    }
                }

                // Sidebar scrollbar + item click
                if self.sidebar_open && x < bounds.x + side_w && y >= bounds.y + 56 && y < bounds.y + bounds.h - 20 {
                    let side_r = Rect::new(bounds.x + 150, bounds.y + 56, 14, (bounds.h - 76).max(16));
                    let vis = (side_r.h / 22).max(1) as usize;
                    let max_s = self.workspace_files.len().saturating_sub(vis);
                    self.sidebar_vbar.sync_range(self.sidebar_scroll.min(max_s) as i32, max_s as i32, vis as i32);
                    if side_r.contains(x, y) {
                        let (_, ch) = ScrollBar::handle_event(&mut self.sidebar_vbar, side_r, ScrollOrientation::Vertical, GuiEvent::MouseDown { x, y, button: MouseButton::Left });
                        if ch {
                            self.sidebar_scroll = (self.sidebar_vbar.value.max(0) as usize).min(max_s);
                        }
                        self.ctx.damage.invalidate(bounds);
                        return EventResult::NeedsRedraw;
                    }
                    let row = ((y - (bounds.y + 58)) / 22) as usize + self.sidebar_scroll;
                    if row < self.workspace_files.len() {
                        let p = self.workspace_files[row].path.clone();
                        self.open_path(&p);
                        self.ctx.damage.invalidate(bounds);
                        return EventResult::NeedsRedraw;
                    }
                }

                // Find Next button click
                if self.find_open && y >= bounds.y + 56 && y < bounds.y + 84 && x >= bounds.x + bounds.w - 72 {
                    self.find_next();
                    self.ctx.damage.invalidate(bounds);
                    return EventResult::NeedsRedraw;
                }

                // Editor scrollbar + cursor placement
                let ed = self.editor_area(bounds);
                if ed.contains(x, y) {
                    let ed_sb = Rect::new(ed.right() - 14, ed.y, 14, ed.h.max(16));
                    let vis_rows = ((ed.h - 8) / 18).max(1) as usize;
                    if ed_sb.contains(x, y) {
                        if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                            let max_s = tab.lines.len().saturating_sub(vis_rows);
                            self.editor_vbar.sync_range(tab.scroll_row.min(max_s) as i32, max_s as i32, vis_rows as i32);
                            let (_, ch) = ScrollBar::handle_event(&mut self.editor_vbar, ed_sb, ScrollOrientation::Vertical, GuiEvent::MouseDown { x, y, button: MouseButton::Left });
                            if ch {
                                tab.scroll_row = (self.editor_vbar.value.max(0) as usize).min(max_s);
                            }
                        }
                        self.ctx.damage.invalidate(bounds);
                        return EventResult::NeedsRedraw;
                    }
                    let gutter_w = 42;
                    let clicked_row = ((y - (ed.y + 6)).max(0) / 18) as usize;
                    if let Some(tab) = self.tabs.get_mut(self.active_tab) {
                        let target_row = (tab.scroll_row + clicked_row).min(tab.lines.len().saturating_sub(1));
                        tab.cursor_row = target_row;
                        let rel_x = (x - (ed.x + gutter_w + 6)).max(0);
                        let approx_col = (rel_x / 7) as usize;
                        tab.cursor_col = approx_col.min(tab.lines[target_row].len());
                    }
                    self.ctx.damage.invalidate(bounds);
                    return EventResult::NeedsRedraw;
                }

                EventResult::Ignored
            }
            _ => EventResult::Ignored,
        }
    }

    fn take_damage(&mut self, bounds: Rect) -> Option<Rect> {
        if self.dirty_all {
            self.dirty_all = false;
            return Some(bounds);
        }
        self.ctx.damage.take_bounding(bounds)
    }

    fn draw(&mut self, surf: &mut Surface, bounds: Rect, theme: &Theme) {
        let side_w = if self.sidebar_open { 164 } else { 0 };
        let status_h = 20;

        let ed_bg = if self.dark_editor {
            Color::rgb(24, 28, 36)
        } else {
            Color::rgb(252, 252, 253)
        };
        let gutter_bg = if self.dark_editor {
            Color::rgb(20, 23, 30)
        } else {
            Color::rgb(242, 244, 248)
        };
        let active_line_bg = if self.dark_editor {
            Color::rgb(34, 40, 52)
        } else {
            Color::rgb(235, 242, 252)
        };

        // Top toolbar
        surf.fill_rect(Rect::new(bounds.x, bounds.y, bounds.w, 32), theme.face_dark);
        // Sidebar
        if self.sidebar_open {
            surf.fill_rect(
                Rect::new(bounds.x, bounds.y + 32, side_w, bounds.h - 32 - status_h),
                theme.face,
            );
        }
        // Tab bar
        surf.fill_rect(
            Rect::new(bounds.x + side_w, bounds.y + 32, (bounds.w - side_w).max(80), 24),
            theme.face,
        );

        let ed = self.editor_area(bounds);
        surf.fill_rect(ed, ed_bg);
        let gutter_w = 42;
        surf.fill_rect(Rect::new(ed.x, ed.y, gutter_w, ed.h), gutter_bg);

        with_default_font(|font| {
            // Toolbar buttons
            let mut bx = bounds.x + 8;
            let btns = [
                (48, "Files", self.sidebar_open),
                (44, "New", false),
                (46, "Save", true),
                (48, "Undo", false),
                (46, "Find", self.find_open),
                (46, "Run", false),
                (56, if self.dark_editor { "Light" } else { "Dark" }, false),
            ];
            for (bw, label, active) in btns {
                let bg = if active { theme.accent } else { theme.face_light };
                surf.fill_rect(Rect::new(bx, bounds.y + 4, bw, 24), bg);
                surf.draw_rect(Rect::new(bx, bounds.y + 4, bw, 24), theme.ctrl_border);
                font.draw_text(
                    surf,
                    bx + 8,
                    bounds.y + 9,
                    label,
                    TextStyle::ui_sized(11, contrast_text_on(bg)),
                );
                bx += bw + 4;
            }

            // Sidebar header + file list
            if self.sidebar_open {
                font.draw_text(
                    surf,
                    bounds.x + 10,
                    bounds.y + 37,
                    "WORKSPACE",
                    TextStyle::ui_sized(10, theme.text_secondary),
                );
                let mut fy = bounds.y + 58;
                let cur_path = self.tabs.get(self.active_tab).map(|t| t.path.as_str()).unwrap_or("");
                for f in self.workspace_files.iter().skip(self.sidebar_scroll) {
                    if fy + 22 > bounds.y + bounds.h - status_h {
                        break;
                    }
                    let sel = f.path == cur_path;
                    let r = Rect::new(bounds.x + 4, fy, side_w - 20, 20);
                    let bg = if sel { theme.accent } else { theme.face };
                    surf.fill_rect(r, bg);
                    font.draw_text(
                        surf,
                        r.x + 6,
                        r.y + 3,
                        &f.label,
                        TextStyle::ui_sized(11, contrast_text_on(bg)),
                    );
                    fy += 22;
                }
                let side_r = Rect::new(bounds.x + 150, bounds.y + 56, 14, (bounds.h - 76).max(16));
                let vis = (side_r.h / 22).max(1) as usize;
                let max_s = self.workspace_files.len().saturating_sub(vis);
                self.sidebar_vbar.sync_range(self.sidebar_scroll.min(max_s) as i32, max_s as i32, vis as i32);
                ScrollBar::draw(surf, side_r, ScrollOrientation::Vertical, self.sidebar_vbar, theme);
            }

            // Tabs
            let mut tx = bounds.x + side_w + 4;
            for (i, t) in self.tabs.iter().enumerate() {
                if tx + 112 > bounds.x + bounds.w {
                    break;
                }
                let active = i == self.active_tab;
                let bg = if active { ed_bg } else { theme.face_dark };
                surf.fill_rect(Rect::new(tx, bounds.y + 33, 112, 23), bg);
                let title = if t.dirty {
                    format!("* {}", t.title)
                } else {
                    t.title.clone()
                };
                let fg = if active && self.dark_editor {
                    Color::rgb(235, 240, 248)
                } else {
                    contrast_text_on(bg)
                };
                font.draw_text(surf, tx + 8, bounds.y + 38, &title, TextStyle::ui_sized(11, fg));
                tx += 114;
            }

            // Find strip
            if self.find_open {
                let fr = Rect::new(bounds.x + side_w, bounds.y + 56, bounds.w - side_w, 28);
                surf.fill_rect(fr, theme.face);
                font.draw_text(
                    surf,
                    fr.x + 10,
                    fr.y + 7,
                    &format!("Find: {}_", self.find_query),
                    TextStyle::ui_sized(11, contrast_text_on(theme.face)),
                );
                let nx = bounds.x + bounds.w - 72;
                surf.fill_rect(Rect::new(nx, fr.y + 3, 64, 22), theme.accent);
                font.draw_text(surf, nx + 16, fr.y + 7, "Next", TextStyle::ui_sized(11, contrast_text_on(theme.accent)));
            }

            // Syntax-highlighted editor viewport
            if let Some(tab) = self.tabs.get(self.active_tab) {
                let row_h = 18;
                let max_rows = ((ed.h - 8) / row_h).max(1) as usize;
                let mut in_block = false;
                for i in 0..tab.scroll_row {
                    let _ = tokenize_line(&tab.lines[i], tab.lang, &mut in_block);
                }

                for vis_i in 0..max_rows {
                    let line_idx = tab.scroll_row + vis_i;
                    if line_idx >= tab.lines.len() {
                        break;
                    }
                    let y = ed.y + 6 + vis_i as i32 * row_h;
                    let is_cur = line_idx == tab.cursor_row;
                    if is_cur {
                        surf.fill_rect(Rect::new(ed.x + gutter_w, y - 1, ed.w - gutter_w, row_h), active_line_bg);
                    }

                    // Line number in gutter
                    let num_col = if is_cur {
                        theme.accent
                    } else if self.dark_editor {
                        Color::rgb(96, 108, 128)
                    } else {
                        Color::rgb(130, 140, 155)
                    };
                    font.draw_text(
                        surf,
                        ed.x + 6,
                        y + 2,
                        &format!("{:>3}", line_idx + 1),
                        TextStyle::ui_sized(11, num_col),
                    );

                    // Highlighted code tokens
                    let line = &tab.lines[line_idx];
                    let spans = tokenize_line(line, tab.lang, &mut in_block);
                    let mut x = ed.x + gutter_w + 6;
                    let measure_style = TextStyle::ui_sized(12, Color::rgb(255, 255, 255));

                    // Find query highlight
                    if self.find_open && !self.find_query.is_empty() {
                        if let Some(mpos) = line.find(&self.find_query) {
                            let prefix_w = font.measure_text(&line[..mpos], measure_style).0;
                            let match_w = font.measure_text(&self.find_query, measure_style).0.max(6);
                            surf.fill_rect(Rect::new(x + prefix_w, y, match_w, 16), Color::rgb(98, 72, 22));
                        }
                    }

                    for sp in spans {
                        if x >= ed.x + ed.w - 4 {
                            break;
                        }
                        let seg = &line[sp.start..sp.end];
                        let col = token_color(sp.kind, self.dark_editor);
                        font.draw_text(surf, x, y + 1, seg, TextStyle::ui_sized(12, col));
                        x += font.measure_text(seg, measure_style).0;
                    }

                    // Cursor caret
                    if is_cur {
                        let col_byte = tab.cursor_col.min(line.len());
                        let prefix = &line[..col_byte];
                        let cx = ed.x + gutter_w + 6 + font.measure_text(prefix, measure_style).0;
                        if cx < ed.x + ed.w - 16 {
                            surf.fill_rect(Rect::new(cx, y + 1, 2, 15), theme.accent);
                        }
                    }
                }
                let ed_sb = Rect::new(ed.right() - 14, ed.y, 14, ed.h.max(16));
                let max_s = tab.lines.len().saturating_sub(max_rows);
                self.editor_vbar.sync_range(tab.scroll_row.min(max_s) as i32, max_s as i32, max_rows as i32);
                ScrollBar::draw(surf, ed_sb, ScrollOrientation::Vertical, self.editor_vbar, theme);
            }

            // Output drawer
            if self.output_open {
                let oy = bounds.y + bounds.h - status_h - 96;
                surf.fill_rect(Rect::new(bounds.x + side_w, oy, bounds.w - side_w, 96), Color::rgb(14, 18, 24));
                surf.draw_rect(Rect::new(bounds.x + side_w, oy, bounds.w - side_w, 96), theme.accent);
                font.draw_text(
                    surf,
                    bounds.x + side_w + 8,
                    oy + 4,
                    "OUTPUT PANEL (F5 Run)",
                    TextStyle::ui_sized(10, theme.accent),
                );
                for (oi, oline) in self.output_lines.iter().take(4).enumerate() {
                    font.draw_text(
                        surf,
                        bounds.x + side_w + 8,
                        oy + 20 + oi as i32 * 17,
                        oline,
                        TextStyle::ui_sized(11, Color::rgb(212, 224, 238)),
                    );
                }
            }

            // Status bar
            let sy = bounds.y + bounds.h - status_h;
            surf.fill_rect(Rect::new(bounds.x, sy, bounds.w, status_h), theme.face_dark);
            let (ln, col, lang_name) = if let Some(t) = self.tabs.get(self.active_tab) {
                (t.cursor_row + 1, t.cursor_col + 1, t.lang.name())
            } else {
                (1, 1, "Text")
            };
            font.draw_text(
                surf,
                bounds.x + 8,
                sy + 3,
                &format!("{}  |  Ln {}, Col {}  |  {}", lang_name, ln, col, self.status),
                TextStyle::ui_sized(11, contrast_text_on(theme.face_dark)),
            );
        });
    }
}

#[no_mangle]
pub fn main() -> i32 {
    let argv = args();
    let initial = argv.get(1).copied();
    let mut os = SublimeWmOs;
    let mut app = CodeEditorApp::new(initial);
    match libgui::ipc::run_app(&mut app, &mut os) {
        Ok(()) => 0,
        Err(code) => {
            println!("[code pid={}] WM error: {}", getpid(), code);
            1
        }
    }
}
