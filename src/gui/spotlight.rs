use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::gui::canvas::Canvas;
use crate::gui::color::Color;
use crate::gui::desktop::DesktopAppId;
use crate::gui::font::{draw_char, draw_string, measure_string};

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
            // Show top default apps
            results.push(SpotlightItem {
                label: String::from("Terminal"),
                category: "Application",
                icon_char: '>',
                target: SpotlightTarget::LaunchApp(DesktopAppId::Terminal),
            });
            results.push(SpotlightItem {
                label: String::from("Finder"),
                category: "Application",
                icon_char: '@',
                target: SpotlightTarget::LaunchApp(DesktopAppId::FileManager),
            });
            results.push(SpotlightItem {
                label: String::from("TextEdit"),
                category: "Application",
                icon_char: 'T',
                target: SpotlightTarget::LaunchApp(DesktopAppId::TextEdit),
            });
            results.push(SpotlightItem {
                label: String::from("Calculator"),
                category: "Application",
                icon_char: '+',
                target: SpotlightTarget::LaunchApp(DesktopAppId::Calculator),
            });
            results.push(SpotlightItem {
                label: String::from("System Settings"),
                category: "Application",
                icon_char: 'S',
                target: SpotlightTarget::LaunchApp(DesktopAppId::Settings),
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
        let apps: [(&str, DesktopAppId, char); 7] = [
            ("terminal", DesktopAppId::Terminal, '>'),
            ("calculator", DesktopAppId::Calculator, '+'),
            ("textedit", DesktopAppId::TextEdit, 'T'),
            ("finder", DesktopAppId::FileManager, '@'),
            ("activity monitor", DesktopAppId::Sysmon, '#'),
            ("system settings", DesktopAppId::Settings, 'S'),
            ("about sublimeos", DesktopAppId::About, '✦'),
        ];

        for (name, app_id, icon) in apps.iter() {
            if name.contains(&q) {
                let mut title = String::from(*name);
                if let Some(first) = title.get_mut(0..1) {
                    first.make_ascii_uppercase();
                }
                results.push(SpotlightItem {
                    label: title,
                    category: "Application",
                    icon_char: *icon,
                    target: SpotlightTarget::LaunchApp(*app_id),
                });
            }
        }

        results
    }

    pub fn render(&mut self, canvas: &mut Canvas, screen_w: i32, _screen_h: i32) {
        if !self.is_open {
            return;
        }

        let results = self.get_results();
        let modal_w = 540;
        let item_h = 32;
        let modal_h = 56 + (results.len().min(5) as i32 * item_h);
        let modal_x = (screen_w - modal_w) / 2;
        let modal_y = 100;

        // 1. Frosted Glass Box Blur + Shadow
        canvas.blur_rect(modal_x, modal_y, modal_w, modal_h, 2);
        canvas.draw_shadow(modal_x, modal_y, modal_w, modal_h, 14, 110);
        canvas.fill_rounded_rect(modal_x, modal_y, modal_w, modal_h, 12, Color::argb(235, 26, 28, 38));
        canvas.draw_rounded_rect(modal_x, modal_y, modal_w, modal_h, 12, Color::argb(80, 255, 255, 255));

        // 2. Search Bar Input Area
        let input_y = modal_y + 14;
        draw_char(canvas, modal_x + 16, input_y + 2, '?', Color::ACCENT_CYAN); // Search icon

        if self.query.is_empty() {
            draw_string(canvas, modal_x + 38, input_y + 2, "Spotlight Search (apps, math, commands)...", Color::TEXT_MUTED);
        } else {
            draw_string(canvas, modal_x + 38, input_y + 2, &self.query, Color::WHITE);
        }

        // Blinking cursor
        self.cursor_timer = self.cursor_timer.wrapping_add(1);
        if (self.cursor_timer % 30) < 15 {
            let cursor_x = modal_x + 38 + measure_string(&self.query);
            canvas.fill_rect(cursor_x, input_y + 2, 2, 16, Color::ACCENT_BLUE);
        }

        // 3. Results List
        if !results.is_empty() {
            canvas.fill_rect(modal_x + 8, modal_y + 48, modal_w - 16, 1, Color::WINDOW_BORDER);

            let mut ry = modal_y + 54;
            for (idx, item) in results.iter().take(5).enumerate() {
                let is_sel = idx == self.selected_index;
                if is_sel {
                    canvas.fill_rounded_rect(modal_x + 8, ry - 2, modal_w - 16, item_h - 4, 6, Color::ACCENT_BLUE);
                }

                // Item icon
                draw_char(canvas, modal_x + 18, ry + 4, item.icon_char, if is_sel { Color::WHITE } else { Color::ACCENT_CYAN });

                // Item label
                draw_string(canvas, modal_x + 38, ry + 4, &item.label, if is_sel { Color::WHITE } else { Color::TEXT_PRIMARY });

                // Category tag on right
                let cat_w = measure_string(item.category);
                draw_string(canvas, modal_x + modal_w - cat_w - 20, ry + 4, item.category, if is_sel { Color::argb(220, 255, 255, 255) } else { Color::TEXT_MUTED });

                ry += item_h;
            }
        }
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
