use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::gui::canvas::Canvas;
use crate::gui::color::Color;
use crate::gui::desktop::DesktopAppId;
use crate::gui::font::{
    draw_ui_string_centered_v, draw_ui_string_ellipsis, draw_ui_string_sized, measure_ui_string,
};

#[derive(Clone)]
pub enum SpotlightTarget {
    LaunchApp(DesktopAppId),
    LaunchPath(String),
    Calculation(String),
}

pub struct SpotlightItem {
    pub label: String,
    pub category: &'static str,
    pub icon_char: char,
    pub target: SpotlightTarget,
}

pub struct Spotlight {
    pub is_open: bool,
    pub query: String,
    pub selected_index: usize,
    cursor_timer: u64,
}

impl Spotlight {
    pub fn new() -> Self {
        Self {
            is_open: false,
            query: String::new(),
            selected_index: 0,
            cursor_timer: 0,
        }
    }

    pub fn toggle(&mut self) {
        self.is_open = !self.is_open;
        if self.is_open {
            self.query.clear();
            self.selected_index = 0;
        }
    }

    pub fn handle_key(&mut self, key: libgui::event::KeyCode) -> Option<SpotlightTarget> {
        let count = self.get_results().len().min(5);
        match key {
            libgui::event::KeyCode::Up => {
                if count > 0 {
                    self.selected_index = if self.selected_index == 0 { count - 1 } else { self.selected_index - 1 };
                }
                None
            }
            libgui::event::KeyCode::Down => {
                if count > 0 {
                    self.selected_index = (self.selected_index + 1) % count;
                }
                None
            }
            libgui::event::KeyCode::Enter => self.handle_char('\n'),
            libgui::event::KeyCode::Backspace => self.handle_char('\x08'),
            _ => None,
        }
    }

    pub fn handle_char(&mut self, ch: char) -> Option<SpotlightTarget> {
        match ch {
            '\x1B' => {
                // Escape closes Spotlight
                self.is_open = false;
                None
            }
            '\n' | '\r' => {
                let results = self.get_results();
                if let Some(item) = results.get(self.selected_index) {
                    let target = item.target.clone();
                    self.is_open = false;
                    Some(target)
                } else {
                    None
                }
            }
            '\x08' => {
                // Backspace
                self.query.pop();
                self.selected_index = 0;
                None
            }
            c if c >= ' ' && c <= '~' => {
                self.query.push(c);
                self.selected_index = 0;
                None
            }
            _ => None,
        }
    }

    pub fn get_results(&self) -> Vec<SpotlightItem> {
        let mut results = Vec::new();
        let q = self.query.trim().to_ascii_lowercase();

        if q.is_empty() {
            // Show top default SublimeOS programs
            results.push(SpotlightItem {
                label: String::from("SublimeOS Terminal (sh / cmd)"),
                category: "Application",
                icon_char: '>',
                target: SpotlightTarget::LaunchApp(DesktopAppId::Terminal),
            });
            results.push(SpotlightItem {
                label: String::from("My Computer / Explorer (files)"),
                category: "Application",
                icon_char: 'E',
                target: SpotlightTarget::LaunchApp(DesktopAppId::FileManager),
            });
            results.push(SpotlightItem {
                label: String::from("Notepad (notepad)"),
                category: "Application",
                icon_char: 'N',
                target: SpotlightTarget::LaunchApp(DesktopAppId::TextEdit),
            });
            results.push(SpotlightItem {
                label: String::from("Calculator (calc)"),
                category: "Application",
                icon_char: '+',
                target: SpotlightTarget::LaunchApp(DesktopAppId::Calculator),
            });
            results.push(SpotlightItem {
                label: String::from("Task Manager (sysmon)"),
                category: "Application",
                icon_char: '#',
                target: SpotlightTarget::LaunchApp(DesktopAppId::Sysmon),
            });
            return results;
        }

        // 1. Math calculation check (e.g. "12 * 8", "100 / 4", "50 + 25")
        if let Some(calc_res) = evaluate_arithmetic(&q) {
            results.push(SpotlightItem {
                label: format!("= {}", calc_res),
                category: "Calculation",
                icon_char: '=',
                target: SpotlightTarget::Calculation(format!("{}", calc_res)),
            });
        }

        // 2. Apps match
        let apps: [(&str, &str, DesktopAppId, char); 8] = [
            ("sublimeos terminal", "cmd terminal sh", DesktopAppId::Terminal, '>'),
            ("widget studio", "studio libgui ring3 app", DesktopAppId::Studio, 'W'),
            ("calculator", "calc", DesktopAppId::Calculator, '+'),
            ("notepad", "textedit editor", DesktopAppId::TextEdit, 'N'),
            ("my computer explorer", "explorer files folders", DesktopAppId::FileManager, 'E'),
            ("task manager", "sysmon activity processes", DesktopAppId::Sysmon, '#'),
            ("control panel", "settings wifi network display", DesktopAppId::Settings, 'C'),
            ("about sublimeos", "version sysinfo about", DesktopAppId::About, 'i'),
        ];

        for (name, aliases, app_id, icon) in apps.iter() {
            if name.contains(&q) || aliases.contains(&q) {
                let mut title = String::from(*name);
                if let Some(first) = title.get_mut(0..1) {
                    first.make_ascii_uppercase();
                }
                results.push(SpotlightItem {
                    label: title,
                    category: "Program",
                    icon_char: *icon,
                    target: SpotlightTarget::LaunchApp(*app_id),
                });
            }
        }

        // 3. Discovered .app bundles & Ring-3 ELF64 binaries
        for app in crate::user::apps::discover_apps() {
            let name_l = app.name.to_ascii_lowercase();
            let exec_l = app.exec.to_ascii_lowercase();
            let comment_l = app.comment.to_ascii_lowercase();
            if name_l.contains(&q) || exec_l.contains(&q) || comment_l.contains(&q) {
                let already_builtin = app.builtin.is_some()
                    && results.iter().any(|r| {
                        matches!(&r.target, SpotlightTarget::LaunchApp(id) if Some(*id) == app.builtin)
                    });
                if already_builtin {
                    continue;
                }
                let cat = if app.terminal { "CLI Binary" } else { "User App" };
                let icon_ch = app.name.chars().next().unwrap_or('A').to_ascii_uppercase();
                results.push(SpotlightItem {
                    label: app.name,
                    category: cat,
                    icon_char: icon_ch,
                    target: SpotlightTarget::LaunchPath(app.bundle_path),
                });
            }
        }

        results
    }

    pub fn handle_click(&mut self, x: i32, y: i32, screen_w: i32, screen_h: i32) -> (bool, Option<SpotlightTarget>) {
        if !self.is_open {
            return (false, None);
        }
        let panel_w = 340;
        let panel_x = screen_w - panel_w;
        let panel_h = (screen_h - 36).max(200);
        if x < panel_x || x >= screen_w || y < 0 || y >= panel_h {
            self.is_open = false;
            return (false, None);
        }
        let close_x = screen_w - 36;
        if x >= close_x && x <= close_x + 24 && y >= 18 && y <= 42 {
            self.is_open = false;
            return (true, None);
        }
        let results = self.get_results();
        let list_y = 120;
        let item_h = 40;
        let box_x = panel_x + 20;
        let box_w = panel_w - 40;
        if x >= box_x && x <= box_x + box_w && y >= list_y {
            let idx = ((y - list_y) / item_h) as usize;
            if idx < 6 {
                if let Some(item) = results.get(idx) {
                    let target = item.target.clone();
                    self.is_open = false;
                    return (true, Some(target));
                }
            }
        }
        (true, None)
    }

    pub fn render(&mut self, canvas: &mut Canvas, screen_w: i32, screen_h: i32) {
        if !self.is_open {
            return;
        }

        let results = self.get_results();
        let panel_w = 340;
        let panel_x = screen_w - panel_w;
        let panel_h = (screen_h - 36).max(200);

        // Windows 8.1 Search Charm right-side docked matte panel
        canvas.fill_rect(panel_x - 4, 0, 4, panel_h, Color::argb(65, 0, 0, 0));
        canvas.fill_rect(panel_x, 0, panel_w, panel_h, Color::rgb(27, 27, 30));
        canvas.fill_rect(panel_x, 0, 1, panel_h, Color::rgb(62, 62, 68));

        // Large Metro Header "Search" + Close button
        draw_ui_string_sized(canvas, panel_x + 20, 18, 22, true, "Search", Color::WHITE);
        let close_x = screen_w - 36;
        for i in 0..9 {
            canvas.fill_rect(close_x + 7 + i, 24 + i, 2, 2, Color::rgb(190, 190, 198));
            canvas.fill_rect(close_x + 15 - i, 24 + i, 2, 2, Color::rgb(190, 190, 198));
        }

        // Sharp Cyan-Bordered Search Box
        let box_x = panel_x + 20;
        let box_y = 64;
        let box_w = panel_w - 40;
        canvas.fill_rect(box_x, box_y, box_w, 30, Color::WHITE);
        canvas.draw_rect(box_x, box_y, box_w, 30, Color::METRO_TILE_BLUE);
        canvas.draw_rect(box_x + 1, box_y + 1, box_w - 2, 28, Color::METRO_TILE_BLUE);

        if self.query.is_empty() {
            draw_ui_string_ellipsis(canvas, box_x + 8, box_y + 5, "Search apps, settings, or math...", Color::rgb(130, 130, 140), box_w - 36);
        } else {
            draw_ui_string_ellipsis(canvas, box_x + 8, box_y + 5, &self.query, Color::rgb(20, 20, 24), box_w - 36);
        }

        // Blinking caret inside search field
        self.cursor_timer = self.cursor_timer.wrapping_add(1);
        if (self.cursor_timer % 30) < 15 {
            let cursor_x = (box_x + 8 + measure_ui_string(&self.query)).min(box_x + box_w - 28);
            canvas.fill_rect(cursor_x, box_y + 6, 2, 18, Color::METRO_TILE_BLUE);
        }

        // Cyan magnifier accent badge on right end of search box
        canvas.fill_rect(box_x + box_w - 28, box_y + 2, 26, 26, Color::METRO_TILE_BLUE);
        canvas.draw_circle(box_x + box_w - 16, box_y + 13, 5, Color::WHITE);
        canvas.draw_line(box_x + box_w - 12, box_y + 17, box_x + box_w - 8, box_y + 21, Color::WHITE);

        // Flat Metro Search Result Rows
        if !results.is_empty() {
            let item_h = 40;
            let mut ry = 120;
            for (idx, item) in results.iter().take(6).enumerate() {
                let is_sel = idx == self.selected_index;
                if is_sel {
                    canvas.fill_rect(box_x, ry, box_w, item_h - 2, Color::METRO_TILE_BLUE);
                } else {
                    canvas.fill_rect(box_x, ry, box_w, item_h - 2, Color::rgb(38, 38, 43));
                }
                let badge_col = if is_sel { Color::rgb(0, 90, 165) } else { Color::METRO_TILE_TEAL };
                canvas.fill_rect(box_x + 6, ry + 6, 26, 26, badge_col);
                let mut glyph_buf = [0u8; 4];
                let glyph_str = item.icon_char.encode_utf8(&mut glyph_buf);
                let gw = measure_ui_string(glyph_str);
                draw_ui_string_centered_v(canvas, box_x + 6 + (26 - gw) / 2, ry + 6, 26, glyph_str, Color::WHITE);

                draw_ui_string_ellipsis(canvas, box_x + 40, ry + 2, &item.label, Color::WHITE, box_w - 48);
                draw_ui_string_sized(canvas, box_x + 40, ry + 19, 12, false, item.category, Color::rgb(195, 215, 238));
                ry += item_h;
            }
        }

        draw_ui_string_sized(
            canvas,
            box_x,
            panel_h - 30,
            12,
            false,
            "Enter to launch  •  Esc to close Search Charm",
            Color::rgb(160, 162, 170),
        );
    }
}

fn evaluate_arithmetic(s: &str) -> Option<f64> {
    let ops = ['+', '-', '*', '/'];
    let op_idx = s.find(|c| ops.contains(&c))?;

    let op = s.as_bytes()[op_idx] as char;
    let left_str = s[..op_idx].trim();
    let right_str = s[op_idx + 1..].trim();

    let a: f64 = parse_float(left_str)?;
    let b: f64 = parse_float(right_str)?;

    match op {
        '+' => Some(a + b),
        '-' => Some(a - b),
        '*' => Some(a * b),
        '/' => if b != 0.0 { Some(a / b) } else { None },
        _ => None,
    }
}

fn parse_float(s: &str) -> Option<f64> {
    if s.is_empty() {
        return None;
    }
    let mut val = 0.0;
    let mut sign = 1.0;
    let mut chars = s.chars().peekable();

    if let Some(&'-') = chars.peek() {
        sign = -1.0;
        chars.next();
    }

    let mut in_frac = false;
    let mut frac_div = 1.0;
    let mut has_digit = false;

    for c in chars {
        if c == '.' {
            in_frac = true;
            continue;
        }
        if let Some(digit) = c.to_digit(10) {
            has_digit = true;
            if in_frac {
                frac_div *= 10.0;
                val += (digit as f64) / frac_div;
            } else {
                val = val * 10.0 + (digit as f64);
            }
        } else {
            return None;
        }
    }

    if has_digit {
        Some(sign * val)
    } else {
        None
    }
}
