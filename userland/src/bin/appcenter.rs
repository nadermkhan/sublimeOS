#![no_std]
#![no_main]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use libgui::app::{WindowClient, WindowSpec};
use libgui::component::ComponentContext;
use libgui::event::{EventResult, GuiEvent, KeyCode, MouseButton};
use libgui::font::{with_default_font, TextStyle};
use libgui::gfx::{contrast_text_on, Rect, Surface, Theme, ThemeKind};
use libgui::widget::{ScrollBar, ScrollBarState, ScrollOrientation};
use sublime_rt::{
    getenv, getpid, println, read_bytes, read_dir, read_to_string, spawn_bg, syscommand,
    write_file, SublimeWmOs,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum AppCategory {
    All,
    Gui,
    Cli,
    Desktop,
}

#[derive(Clone)]
struct AppItem {
    name: String,
    bundle_path: String,
    exec_path: String,
    builtin_name: String,
    category: String,
    comment: String,
    is_gui: bool,
    is_desktop: bool,
    elf_summary: String,
}

fn inspect_elf(path: &str) -> String {
    match read_bytes(path, 64) {
        Ok(hdr) if hdr.len() >= 58 && &hdr[0..4] == b"\x7FELF" => {
            let class = if hdr[4] == 2 { "ELF64" } else { "ELF32" };
            let mach = u16::from_le_bytes([hdr[18], hdr[19]]);
            let arch = if mach == 0x3E { "x86_64" } else { "other" };
            let phnum = u16::from_le_bytes([hdr[56], hdr[57]]);
            format!("{} {} ({} segs)", class, arch, phnum)
        }
        Ok(hdr) if hdr.len() >= 4 && &hdr[0..4] == b"\x7FELF" => String::from("ELF64"),
        Ok(_) => String::from("Script / Bundle"),
        Err(_) => String::from("System Builtin"),
    }
}

fn parse_app_file(path: &str, fname: &str, is_desktop: bool) -> Option<AppItem> {
    let text = read_to_string(path).ok()?;
    let mut name = String::from(fname.strip_suffix(".app").unwrap_or(fname));
    let mut exec_path = String::new();
    let mut builtin_name = String::new();
    let mut category = String::from(if is_desktop { "Desktop" } else { "Application" });
    let mut comment = String::new();
    let mut terminal = false;

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('[') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let key = k.trim();
            let val = v.trim();
            match key {
                "Name" => name = String::from(val),
                "Exec" => exec_path = String::from(val),
                "Builtin" => builtin_name = String::from(val),
                "Category" => category = String::from(val),
                "Comment" => comment = String::from(val),
                "Terminal" => terminal = val.eq_ignore_ascii_case("true") || val == "1",
                _ => {}
            }
        }
    }

    if exec_path.is_empty() && builtin_name.is_empty() {
        return None;
    }

    let elf_summary = if !exec_path.is_empty() {
        inspect_elf(&exec_path)
    } else {
        String::from("KiteKernel Builtin")
    };
    let is_gui = !terminal && (builtin_name.is_empty() || builtin_name != "Terminal");

    Some(AppItem {
        name,
        bundle_path: String::from(path),
        exec_path,
        builtin_name,
        category,
        comment,
        is_gui,
        is_desktop,
        elf_summary,
    })
}

struct AppCenter {
    ctx: ComponentContext,
    pid: usize,
    tab: AppCategory,
    all_items: Vec<AppItem>,
    selected: usize,
    scroll: usize,
    list_vbar: ScrollBarState,
    status: String,
    dirty_all: bool,
}

impl AppCenter {
    fn new() -> Self {
        let mut s = Self {
            ctx: ComponentContext::new(),
            pid: getpid(),
            tab: AppCategory::All,
            all_items: Vec::new(),
            selected: 0,
            scroll: 0,
            list_vbar: ScrollBarState::new(0, 0, 8),
            status: String::new(),
            dirty_all: true,
        };
        s.rescan();
        s
    }

    fn rescan(&mut self) {
        self.all_items.clear();
        for &(dir, is_desk) in &[
            ("/Applications", false),
            ("/Applications/Utilities", false),
            ("/Users/sublime/Desktop", true),
        ] {
            if let Ok(entries) = read_dir(dir) {
                for e in entries {
                    if !e.is_dir && e.name.ends_with(".app") {
                        let p = format!("{}/{}", dir, e.name);
                        if let Some(item) = parse_app_file(&p, &e.name, is_desk) {
                            self.all_items.push(item);
                        }
                    }
                }
            }
        }

        if let Ok(bins) = read_dir("/bin") {
            for e in bins {
                if e.is_dir || e.name == "init" {
                    continue;
                }
                let full = format!("/bin/{}", e.name);
                if self.all_items.iter().any(|a| a.exec_path == full) {
                    continue;
                }
                let elf_info = inspect_elf(&full);
                if elf_info.starts_with("ELF") {
                    self.all_items.push(AppItem {
                        name: e.name,
                        bundle_path: full.clone(),
                        exec_path: full,
                        builtin_name: String::new(),
                        category: String::from("CLI Tool"),
                        comment: String::from("Ring-3 ELF64 Executable in /bin"),
                        is_gui: false,
                        is_desktop: false,
                        elf_summary: elf_info,
                    });
                }
            }
        }

        if self.selected >= self.filtered().len() {
            self.selected = 0;
        }
        let home = getenv("HOME").unwrap_or("/Users/sublime");
        self.status = format!(
            "PID {} • HOME={} • {} user apps/tools indexed",
            self.pid,
            home,
            self.all_items.len()
        );
    }

    fn filtered(&self) -> Vec<&AppItem> {
        self.all_items
            .iter()
            .filter(|it| match self.tab {
                AppCategory::All => true,
                AppCategory::Gui => it.is_gui && !it.exec_path.is_empty(),
                AppCategory::Cli => !it.is_gui,
                AppCategory::Desktop => it.is_desktop,
            })
            .collect()
    }

    fn launch_selected(&mut self) {
        let list = self.filtered();
        let Some(item) = list.get(self.selected).copied().cloned() else {
            return;
        };
        if !item.exec_path.is_empty() && item.is_gui {
            let pid = spawn_bg(&item.exec_path);
            if pid > 0 {
                self.status = format!("Launched {} (Ring-3 PID {})", item.name, pid);
            } else {
                self.status = format!("Failed to spawn {} ({})", item.exec_path, pid);
            }
        } else {
            let cmd = format!("open {}", item.bundle_path);
            let rc = syscommand(&cmd);
            self.status = if rc >= 0 {
                format!("Opened {} via OS launcher", item.name)
            } else {
                format!("Open failed ({})", rc)
            };
        }
    }

    fn pin_to_desktop(&mut self) {
        let list = self.filtered();
        let Some(item) = list.get(self.selected).copied().cloned() else {
            return;
        };
        let fname: String = item
            .name
            .chars()
            .filter(|c| *c != '/' && *c != '\\')
            .collect();
        let dst = format!("/Users/sublime/Desktop/{}.app", fname);
        let mut body = format!("[SublimeApp]\nName={}\n", item.name);
        if !item.exec_path.is_empty() {
            body.push_str(&format!("Exec={}\n", item.exec_path));
        }
        if !item.builtin_name.is_empty() {
            body.push_str(&format!("Builtin={}\n", item.builtin_name));
        }
        body.push_str(&format!(
            "Category={}\nTerminal={}\nComment={}\n",
            item.category, !item.is_gui, item.comment
        ));
        match write_file(&dst, body.as_bytes()) {
            Ok(()) => {
                self.rescan();
                self.status = format!("Created shortcut {}", dst);
            }
            Err(e) => {
                self.status = format!("Pin failed ({})", e);
            }
        }
    }

    fn create_sample_user_app(&mut self) {
        let path = "/Users/sublime/Desktop/HelloUser.app";
        let manifest = b"[SublimeApp]\nName=Hello User App\nExec=/bin/hello\nCategory=User\nTerminal=true\nComment=Custom Ring-3 ELF64 User App Bundle\n";
        match write_file(path, manifest) {
            Ok(()) => {
                self.rescan();
                self.status = String::from("Installed HelloUser.app -> /Users/sublime/Desktop/");
            }
            Err(e) => {
                self.status = format!("Install failed ({})", e);
            }
        }
    }
}

impl WindowClient for AppCenter {
    fn spec(&self) -> WindowSpec {
        WindowSpec {
            title: String::from("App Center & User Launcher"),
            width: 560,
            height: 380,
            min_width: 420,
            min_height: 290,
            resizable: true,
        }
    }

    fn on_mount(&mut self, _bounds: Rect) {
        self.rescan();
        self.dirty_all = true;
    }

    fn on_theme_changed(&mut self, _theme: ThemeKind) {
        self.dirty_all = true;
    }

    fn on_event(&mut self, event: GuiEvent, bounds: Rect) -> EventResult {
        match event {
            GuiEvent::Resize { .. } => {
                self.ctx.damage.invalidate(bounds);
                return EventResult::NeedsRedraw;
            }
            GuiEvent::KeyDown { key, .. } => {
                let count = self.filtered().len();
                match key {
                    KeyCode::Up if self.selected > 0 => {
                        self.selected -= 1;
                        if self.selected < self.scroll {
                            self.scroll = self.selected;
                        }
                        self.ctx.damage.invalidate(bounds);
                        return EventResult::NeedsRedraw;
                    }
                    KeyCode::Down if self.selected + 1 < count => {
                        self.selected += 1;
                        self.ctx.damage.invalidate(bounds);
                        return EventResult::NeedsRedraw;
                    }
                    KeyCode::Enter => {
                        self.launch_selected();
                        self.ctx.damage.invalidate(bounds);
                        return EventResult::NeedsRedraw;
                    }
                    _ => {}
                }
            }
            GuiEvent::MouseWheel { delta_y, .. } => {
                let list_h = (bounds.h - 70).max(80);
                let max_rows = (list_h / 26).max(1) as usize;
                let count = self.filtered().len();
                let max_s = count.saturating_sub(max_rows);
                if delta_y > 0 {
                    self.scroll = self.scroll.saturating_sub(delta_y as usize * 2);
                } else if delta_y < 0 {
                    self.scroll = (self.scroll + (-delta_y as usize) * 2).min(max_s);
                }
                self.list_vbar.sync_range(self.scroll as i32, max_s as i32, max_rows as i32);
                self.ctx.damage.invalidate(bounds);
                return EventResult::NeedsRedraw;
            }
            GuiEvent::MouseMove { x, y } => {
                let list_x = bounds.x + 10;
                let list_w = (bounds.w * 54) / 100;
                let list_y = bounds.y + 42;
                let list_h = (bounds.h - 70).max(80);
                let sb_r = Rect::new(list_x + list_w - 15, list_y + 1, 14, (list_h - 2).max(16));
                let max_rows = (list_h / 26).max(1) as usize;
                let max_s = self.filtered().len().saturating_sub(max_rows);
                self.list_vbar.sync_range(self.scroll.min(max_s) as i32, max_s as i32, max_rows as i32);
                let (res, ch) = ScrollBar::handle_event(&mut self.list_vbar, sb_r, ScrollOrientation::Vertical, GuiEvent::MouseMove { x, y });
                if ch {
                    self.scroll = (self.list_vbar.value.max(0) as usize).min(max_s);
                }
                if ch || res.needs_redraw() {
                    self.ctx.damage.invalidate(bounds);
                    return EventResult::NeedsRedraw;
                }
            }
            GuiEvent::MouseUp { x, y, button: MouseButton::Left } => {
                let list_x = bounds.x + 10;
                let list_w = (bounds.w * 54) / 100;
                let list_y = bounds.y + 42;
                let list_h = (bounds.h - 70).max(80);
                let sb_r = Rect::new(list_x + list_w - 15, list_y + 1, 14, (list_h - 2).max(16));
                let (res, _) = ScrollBar::handle_event(&mut self.list_vbar, sb_r, ScrollOrientation::Vertical, GuiEvent::MouseUp { x, y, button: MouseButton::Left });
                if res.needs_redraw() {
                    self.ctx.damage.invalidate(bounds);
                    return EventResult::NeedsRedraw;
                }
            }
            GuiEvent::MouseDown {
                x,
                y,
                button: MouseButton::Left,
            } => {
                // Tab bar
                if y >= bounds.y + 8 && y < bounds.y + 34 {
                    let tabs = [
                        ("All Apps", AppCategory::All, 72),
                        ("GUI Apps", AppCategory::Gui, 74),
                        ("CLI Tools", AppCategory::Cli, 74),
                        ("Desktop", AppCategory::Desktop, 72),
                    ];
                    let mut tx = bounds.x + 10;
                    for &(_, tab, tw) in &tabs {
                        if x >= tx && x < tx + tw {
                            self.tab = tab;
                            self.selected = 0;
                            self.scroll = 0;
                            self.ctx.damage.invalidate(bounds);
                            return EventResult::NeedsRedraw;
                        }
                        tx += tw + 4;
                    }
                    if x >= bounds.x + bounds.w - 86 && x < bounds.x + bounds.w - 10 {
                        self.rescan();
                        self.ctx.damage.invalidate(bounds);
                        return EventResult::NeedsRedraw;
                    }
                }

                // List rows & scrollbar
                let list_x = bounds.x + 10;
                let list_w = (bounds.w * 54) / 100;
                let list_y = bounds.y + 42;
                let list_h = (bounds.h - 70).max(80);
                let sb_r = Rect::new(list_x + list_w - 15, list_y + 1, 14, (list_h - 2).max(16));
                let max_rows = (list_h / 26).max(1) as usize;
                let count = self.filtered().len();
                let max_s = count.saturating_sub(max_rows);
                self.list_vbar.sync_range(self.scroll.min(max_s) as i32, max_s as i32, max_rows as i32);
                if sb_r.contains(x, y) {
                    let (_, ch) = ScrollBar::handle_event(&mut self.list_vbar, sb_r, ScrollOrientation::Vertical, GuiEvent::MouseDown { x, y, button: MouseButton::Left });
                    if ch {
                        self.scroll = (self.list_vbar.value.max(0) as usize).min(max_s);
                    }
                    self.ctx.damage.invalidate(bounds);
                    return EventResult::NeedsRedraw;
                }
                if x >= list_x && x < list_x + list_w - 15 && y >= list_y && y < list_y + list_h {
                    let row = ((y - list_y) / 26) as usize + self.scroll;
                    if row < count {
                        self.selected = row;
                        self.ctx.damage.invalidate(bounds);
                        return EventResult::NeedsRedraw;
                    }
                }

                // Right panel action buttons
                let card_x = list_x + list_w + 10;
                let card_w = (bounds.x + bounds.w - 10 - card_x).max(120);
                let btn_y = bounds.y + 188;
                if x >= card_x + 10 && x < card_x + card_w - 10 {
                    if y >= btn_y && y < btn_y + 28 {
                        self.launch_selected();
                        self.ctx.damage.invalidate(bounds);
                        return EventResult::NeedsRedraw;
                    }
                    if y >= btn_y + 36 && y < btn_y + 62 {
                        self.pin_to_desktop();
                        self.ctx.damage.invalidate(bounds);
                        return EventResult::NeedsRedraw;
                    }
                    if y >= btn_y + 70 && y < btn_y + 96 {
                        self.create_sample_user_app();
                        self.ctx.damage.invalidate(bounds);
                        return EventResult::NeedsRedraw;
                    }
                }
            }
            _ => {}
        }
        EventResult::Ignored
    }

    fn take_damage(&mut self, bounds: Rect) -> Option<Rect> {
        if self.dirty_all {
            self.dirty_all = false;
            self.ctx.damage.clear();
            return Some(bounds);
        }
        self.ctx.damage.take_bounding(bounds)
    }

    fn draw(&mut self, surf: &mut Surface, bounds: Rect, theme: &Theme) {
        surf.fill_rect(bounds, theme.face);

        with_default_font(|font| {
            let fg_face = contrast_text_on(theme.face);
            let fg_light = contrast_text_on(theme.face_light);

            // Tabs & Refresh
            let tabs = [
                ("All Apps", AppCategory::All, 72),
                ("GUI Apps", AppCategory::Gui, 74),
                ("CLI Tools", AppCategory::Cli, 74),
                ("Desktop", AppCategory::Desktop, 72),
            ];
            let mut tx = bounds.x + 10;
            for &(lbl, tab, tw) in &tabs {
                let active = self.tab == tab;
                let bg = if active { theme.accent } else { theme.face_light };
                surf.fill_rect(Rect::new(tx, bounds.y + 8, tw, 24), bg);
                surf.draw_rect(Rect::new(tx, bounds.y + 8, tw, 24), theme.ctrl_border);
                font.draw_text(
                    surf,
                    tx + 8,
                    bounds.y + 13,
                    lbl,
                    TextStyle {
                        bold: active,
                        ..TextStyle::ui_sized(11, contrast_text_on(bg))
                    },
                );
                tx += tw + 4;
            }

            let rx = bounds.x + bounds.w - 86;
            surf.fill_rect(Rect::new(rx, bounds.y + 8, 76, 24), theme.face_light);
            surf.draw_rect(Rect::new(rx, bounds.y + 8, 76, 24), theme.ctrl_border);
            font.draw_text(
                surf,
                rx + 12,
                bounds.y + 13,
                "Rescan",
                TextStyle::ui_sized(11, fg_light),
            );

            // Left App List
            let list_x = bounds.x + 10;
            let list_w = (bounds.w * 54) / 100;
            let list_y = bounds.y + 42;
            let list_h = (bounds.h - 70).max(80);
            surf.fill_rect(Rect::new(list_x, list_y, list_w, list_h), theme.face_light);
            surf.draw_rect(Rect::new(list_x, list_y, list_w, list_h), theme.ctrl_border);

            let items = self.filtered();
            let max_rows = (list_h / 26) as usize;
            for (vis_idx, item) in items.iter().skip(self.scroll).take(max_rows).enumerate() {
                let idx = self.scroll + vis_idx;
                let ry = list_y + 2 + (vis_idx as i32) * 26;
                let is_sel = idx == self.selected;
                if is_sel {
                    surf.fill_rect(Rect::new(list_x + 2, ry, list_w - 18, 24), theme.selection);
                }
                let row_fg = if is_sel {
                    contrast_text_on(theme.selection)
                } else {
                    fg_light
                };
                let badge = if item.is_gui { "[GUI]" } else { "[CLI]" };
                let badge_bg = if item.is_gui { theme.accent } else { theme.face_dark };
                surf.fill_rect(Rect::new(list_x + 6, ry + 4, 34, 16), badge_bg);
                font.draw_text(
                    surf,
                    list_x + 8,
                    ry + 5,
                    badge,
                    TextStyle {
                        bold: true,
                        ..TextStyle::ui_sized(10, contrast_text_on(badge_bg))
                    },
                );
                font.draw_text(
                    surf,
                    list_x + 46,
                    ry + 4,
                    &item.name,
                    TextStyle {
                        bold: is_sel,
                        ..TextStyle::ui_sized(12, row_fg)
                    },
                );
            }
            let sb_r = Rect::new(list_x + list_w - 15, list_y + 1, 14, (list_h - 2).max(16));
            let max_s = items.len().saturating_sub(max_rows.max(1));
            let mut sb_copy = self.list_vbar;
            sb_copy.sync_range(self.scroll.min(max_s) as i32, max_s as i32, max_rows.max(1) as i32);
            ScrollBar::draw(surf, sb_r, ScrollOrientation::Vertical, sb_copy, theme);

            // Right Inspector Card
            let card_x = list_x + list_w + 10;
            let card_w = (bounds.x + bounds.w - 10 - card_x).max(120);
            surf.fill_rect(Rect::new(card_x, list_y, card_w, list_h), theme.face_light);
            surf.draw_rect(Rect::new(card_x, list_y, card_w, list_h), theme.ctrl_border);

            if let Some(sel) = items.get(self.selected).copied() {
                font.draw_text(
                    surf,
                    card_x + 10,
                    list_y + 10,
                    &sel.name,
                    TextStyle {
                        bold: true,
                        ..TextStyle::ui_sized(15, fg_light)
                    },
                );
                let target_line = if !sel.exec_path.is_empty() {
                    format!("Exec: {}", sel.exec_path)
                } else {
                    format!("Builtin: {}", sel.builtin_name)
                };
                let info_lines = [
                    format!("Cat: {}", sel.category),
                    format!("Format: {}", sel.elf_summary),
                    target_line,
                    format!("File: {}", sel.bundle_path),
                    sel.comment.clone(),
                ];
                for (i, line) in info_lines.iter().enumerate() {
                    font.draw_text(
                        surf,
                        card_x + 10,
                        list_y + 38 + (i as i32) * 18,
                        line,
                        TextStyle::ui_sized(11, fg_light),
                    );
                }

                let btn_y = bounds.y + 188;
                surf.fill_rect(Rect::new(card_x + 10, btn_y, card_w - 20, 28), theme.accent);
                font.draw_text(
                    surf,
                    card_x + 20,
                    btn_y + 7,
                    "Launch User App",
                    TextStyle {
                        bold: true,
                        ..TextStyle::ui_sized(12, contrast_text_on(theme.accent))
                    },
                );

                surf.fill_rect(Rect::new(card_x + 10, btn_y + 36, card_w - 20, 26), theme.face);
                surf.draw_rect(Rect::new(card_x + 10, btn_y + 36, card_w - 20, 26), theme.ctrl_border);
                font.draw_text(
                    surf,
                    card_x + 20,
                    btn_y + 42,
                    "Pin Shortcut to Desktop",
                    TextStyle::ui_sized(11, fg_face),
                );

                surf.fill_rect(Rect::new(card_x + 10, btn_y + 70, card_w - 20, 26), theme.face);
                surf.draw_rect(Rect::new(card_x + 10, btn_y + 70, card_w - 20, 26), theme.ctrl_border);
                font.draw_text(
                    surf,
                    card_x + 20,
                    btn_y + 76,
                    "Install Sample App (.app)",
                    TextStyle::ui_sized(11, fg_face),
                );
            }

            // Footer status bar
            let sy = bounds.y + bounds.h - 20;
            surf.fill_rect(Rect::new(bounds.x, sy, bounds.w, 20), theme.face_dark);
            font.draw_text(
                surf,
                bounds.x + 8,
                sy + 3,
                &self.status,
                TextStyle::ui_sized(11, contrast_text_on(theme.face_dark)),
            );
        });
    }
}

#[no_mangle]
pub fn main() -> i32 {
    let mut os = SublimeWmOs;
    let mut app = AppCenter::new();
    match libgui::ipc::run_app(&mut app, &mut os) {
        Ok(()) => 0,
        Err(code) => {
            println!("[appcenter pid={}] WM error: {}", getpid(), code);
            1
        }
    }
}
