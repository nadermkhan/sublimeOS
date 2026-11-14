use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::gui::canvas::Canvas;
use crate::gui::color::Color;
use crate::gui::desktop::DesktopAppId;
use crate::gui::font::{draw_char, draw_string, draw_string_ellipsis, measure_string};

pub enum SpotlightTarget {
    LaunchApp(DesktopAppId),
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
                    let target = match &item.target {
                        SpotlightTarget::LaunchApp(app_id) => SpotlightTarget::LaunchApp(*app_id),
                        SpotlightTarget::Calculation(calc) => SpotlightTarget::Calculation(calc.clone()),
                    };
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
        let apps: [(&str, &str, DesktopAppId, char); 7] = [
            ("sublimeos terminal", "cmd terminal sh", DesktopAppId::Terminal, '>'),
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

        results
    }

    pub fn render(&mut self, canvas: &mut Canvas, screen_w: i32, screen_h: i32) {
        if !self.is_open {
            return;
        }

        let results = self.get_results();
        let modal_w = 460;
        let item_h = 26;
        let list_h = (results.len().min(5) as i32) * item_h;
        let modal_h = 132 + list_h;
        let modal_x = (screen_w - modal_w) / 2;
        let modal_y = ((screen_h - 30 - modal_h) / 2).max(40);

        // 1. Run Dialog Frame
        canvas.fill_rect(modal_x + 3, modal_y + 3, modal_w, modal_h, Color::argb(55, 0, 0, 0));
        canvas.fill_rect(modal_x, modal_y, modal_w, modal_h, Color::XP_FRAME_ACTIVE);
        canvas.draw_rect(modal_x, modal_y, modal_w, modal_h, Color::XP_FRAME_OUTER);

        // Titlebar
        canvas.draw_gradient_v(modal_x + 1, modal_y + 1, modal_w - 2, 26, Color::XP_TITLE_HIGHLIGHT, Color::XP_TITLE_BOTTOM);
        draw_string(canvas, modal_x + 10, modal_y + 6, "Run Command - SublimeOS", Color::WHITE);
        // Red Close Button [X]
        let cx = modal_x + modal_w - 24;
        let cy = modal_y + 4;
        canvas.draw_gradient_v(cx + 1, cy + 1, 18, 17, Color::XP_CAP_RED_TOP, Color::XP_CAP_RED_BOT);
        canvas.draw_rounded_rect(cx, cy, 20, 19, 3, Color::WHITE);
        for i in 0..8 {
            canvas.fill_rect(cx + 6 + i, cy + 5 + i, 2, 2, Color::WHITE);
            canvas.fill_rect(cx + 13 - i, cy + 5 + i, 2, 2, Color::WHITE);
        }

        // Dialog Client Body
        let body_x = modal_x + 3;
        let body_y = modal_y + 27;
        let body_w = modal_w - 6;
        let body_h = modal_h - 30;
        canvas.fill_rect(body_x, body_y, body_w, body_h, Color::XP_FACE);

        // Prompt line
        draw_string_ellipsis(canvas, body_x + 14, body_y + 10, "Type a program name, command, or math expression:", Color::XP_TEXT_BLACK, body_w - 28);

        // "Open:" Label + Sunken White Text Field
        draw_string(canvas, body_x + 14, body_y + 36, "Open:", Color::XP_TEXT_BLACK);
        let box_x = body_x + 62;
        let box_y = body_y + 32;
        let box_w = body_w - 76;
        canvas.fill_rect(box_x, box_y, box_w, 24, Color::WHITE);
        canvas.draw_rect(box_x, box_y, box_w, 24, Color::XP_INPUT_BORDER);

        if self.query.is_empty() {
            draw_string_ellipsis(canvas, box_x + 6, box_y + 4, "terminal, notepad, calc, files...", Color::XP_TEXT_GRAY, box_w - 12);
        } else {
            draw_string_ellipsis(canvas, box_x + 6, box_y + 4, &self.query, Color::XP_TEXT_BLACK, box_w - 12);
        }

        // Blinking cursor
        self.cursor_timer = self.cursor_timer.wrapping_add(1);
        if (self.cursor_timer % 30) < 15 {
            let cursor_x = (box_x + 6 + measure_string(&self.query)).min(box_x + box_w - 6);
            canvas.fill_rect(cursor_x, box_y + 4, 2, 16, Color::XP_TEXT_BLACK);
        }

        // Suggestions Drop List
        if !results.is_empty() {
            let list_y = body_y + 62;
            canvas.fill_rect(box_x, list_y, box_w, list_h + 2, Color::WHITE);
            canvas.draw_rect(box_x, list_y, box_w, list_h + 2, Color::XP_INPUT_BORDER);

            let mut ry = list_y + 1;
            for (idx, item) in results.iter().take(5).enumerate() {
                let is_sel = idx == self.selected_index;
                if is_sel {
                    canvas.fill_rect(box_x + 1, ry, box_w - 2, item_h, Color::XP_SELECTION);
                }
                draw_char(canvas, box_x + 6, ry + 5, item.icon_char, if is_sel { Color::WHITE } else { Color::XP_TEXT_LINK });
                draw_string_ellipsis(canvas, box_x + 24, ry + 5, &item.label, if is_sel { Color::WHITE } else { Color::XP_TEXT_BLACK }, box_w - 30);
                ry += item_h;
            }
        }

        // Classic [OK] and [Cancel] buttons at bottom right
        let btn_y = modal_y + modal_h - 34;
        let ok_x = modal_x + modal_w - 172;
        let cancel_x = modal_x + modal_w - 90;

        canvas.draw_gradient_v(ok_x, btn_y, 74, 24, Color::XP_WHITE, Color::XP_FACE_DARK);
        canvas.draw_rounded_rect(ok_x, btn_y, 74, 24, 3, Color::XP_CTRL_BORDER);
        draw_string(canvas, ok_x + 28, btn_y + 4, "OK", Color::XP_TEXT_BLACK);

        canvas.draw_gradient_v(cancel_x, btn_y, 74, 24, Color::XP_WHITE, Color::XP_FACE_DARK);
        canvas.draw_rounded_rect(cancel_x, btn_y, 74, 24, 3, Color::XP_CTRL_BORDER);
        draw_string(canvas, cancel_x + 13, btn_y + 4, "Cancel", Color::XP_TEXT_BLACK);
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
