use alloc::format;
use alloc::string::String;
use crate::gui::canvas::Canvas;
use crate::gui::color::Color;
use crate::gui::font::{draw_string, draw_string_scaled, measure_string};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CalcOp {
    None,
    Add,
    Sub,
    Mul,
    Div,
}

pub struct CalcApp {
    pub display: String,
    pub operand: f64,
    pub op: CalcOp,
    pub clear_on_next_digit: bool,
    pub active_btn_idx: Option<usize>,
}

impl CalcApp {
    pub fn new() -> Self {
        Self {
            display: String::from("0"),
            operand: 0.0,
            op: CalcOp::None,
            clear_on_next_digit: false,
            active_btn_idx: None,
        }
    }

    pub fn input_digit(&mut self, digit: char) {
        if self.clear_on_next_digit || self.display == "0" {
            self.display.clear();
            self.clear_on_next_digit = false;
        }
        if self.display.len() < 12 {
            self.display.push(digit);
        }
    }

    pub fn input_dot(&mut self) {
        if self.clear_on_next_digit {
            self.display = String::from("0");
            self.clear_on_next_digit = false;
        }
        if !self.display.contains('.') {
            self.display.push('.');
        }
    }

    pub fn clear(&mut self) {
        self.display = String::from("0");
        self.operand = 0.0;
        self.op = CalcOp::None;
        self.clear_on_next_digit = false;
    }

    pub fn negate(&mut self) {
        if self.display != "0" {
            if self.display.starts_with('-') {
                self.display.remove(0);
            } else {
                self.display.insert(0, '-');
            }
        }
    }

    pub fn percent(&mut self) {
        let val: f64 = parse_f64(&self.display);
        let res = val / 100.0;
        self.display = format_f64(res);
    }

    pub fn set_op(&mut self, op: CalcOp) {
        self.operand = parse_f64(&self.display);
        self.op = op;
        self.clear_on_next_digit = true;
    }

    pub fn compute(&mut self) {
        if self.op == CalcOp::None {
            return;
        }
        let current: f64 = parse_f64(&self.display);
        let result = match self.op {
            CalcOp::Add => self.operand + current,
            CalcOp::Sub => self.operand - current,
            CalcOp::Mul => self.operand * current,
            CalcOp::Div => {
                if current == 0.0 {
                    0.0
                } else {
                    self.operand / current
                }
            }
            CalcOp::None => current,
        };
        self.display = format_f64(result);
        self.op = CalcOp::None;
        self.clear_on_next_digit = true;
    }

    pub fn handle_char(&mut self, ch: char) {
        match ch {
            '0'..='9' => self.input_digit(ch),
            '.' => self.input_dot(),
            '+' => self.set_op(CalcOp::Add),
            '-' => self.set_op(CalcOp::Sub),
            '*' => self.set_op(CalcOp::Mul),
            '/' => self.set_op(CalcOp::Div),
            '=' | '\n' | '\r' => self.compute(),
            'c' | 'C' | '\x1B' => self.clear(),
            '%' => self.percent(),
            _ => {}
        }
    }

    pub fn handle_click(&mut self, px: i32, py: i32, ix: i32, iy: i32, iw: i32, ih: i32) {
        // Layout 4x5 buttons inside inner window
        let btn_margin = 6;
        let start_y = iy + 64; // Under display register
        let btn_w = (iw - btn_margin * 5) / 4;
        let btn_h = (ih - 64 - btn_margin * 6) / 5;

        let buttons: [(&str, usize, usize, usize); 19] = [
            ("AC", 0, 0, 1), ("+/-", 1, 0, 1), ("%", 2, 0, 1), ("÷", 3, 0, 1),
            ("7", 0, 1, 1),  ("8", 1, 1, 1),   ("9", 2, 1, 1), ("×", 3, 1, 1),
            ("4", 0, 2, 1),  ("5", 1, 2, 1),   ("6", 2, 2, 1), ("-", 3, 2, 1),
            ("1", 0, 3, 1),  ("2", 1, 3, 1),   ("3", 2, 3, 1), ("+", 3, 3, 1),
            ("0", 0, 4, 2),  (".", 2, 4, 1),   ("=", 3, 4, 1),
        ];

        for (label, col, row, col_span) in buttons.iter() {
            let bx = ix + btn_margin + *col as i32 * (btn_w + btn_margin);
            let by = start_y + *row as i32 * (btn_h + btn_margin);
            let bw = if *col_span == 2 { btn_w * 2 + btn_margin } else { btn_w };

            if px >= bx && px <= bx + bw && py >= by && py <= by + btn_h {
                match *label {
                    "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" => {
                        let ch = label.chars().next().unwrap();
                        self.input_digit(ch);
                    }
                    "." => self.input_dot(),
                    "AC" => self.clear(),
                    "+/-" => self.negate(),
                    "%" => self.percent(),
                    "+" => self.set_op(CalcOp::Add),
                    "-" => self.set_op(CalcOp::Sub),
                    "×" => self.set_op(CalcOp::Mul),
                    "÷" => self.set_op(CalcOp::Div),
                    "=" => self.compute(),
                    _ => {}
                }
                break;
            }
        }
    }

    pub fn render(&self, canvas: &mut Canvas, ix: i32, iy: i32, iw: i32, ih: i32) {
        // macOS Calculator dark body
        canvas.fill_rect(ix, iy, iw, ih, Color::argb(255, 30, 32, 40));

        // 1. Digital Display Register
        let disp_y = iy + 14;
        let disp_text = if self.display.is_empty() { "0" } else { &self.display };
        let disp_w = measure_string(disp_text) * 2;
        let disp_x = (ix + iw - 16).saturating_sub(disp_w).max(ix + 16);

        draw_string_scaled(canvas, disp_x, disp_y, disp_text, Color::WHITE, 2);

        // 2. Buttons Grid
        let btn_margin = 6;
        let start_y = iy + 64;
        let btn_w = (iw - btn_margin * 5) / 4;
        let btn_h = (ih - 64 - btn_margin * 6) / 5;

        let buttons: [(&str, usize, usize, usize, Color, Color); 19] = [
            ("AC", 0, 0, 1, Color::argb(255, 165, 165, 165), Color::BLACK),
            ("+/-", 1, 0, 1, Color::argb(255, 165, 165, 165), Color::BLACK),
            ("%", 2, 0, 1, Color::argb(255, 165, 165, 165), Color::BLACK),
            ("÷", 3, 0, 1, Color::argb(255, 255, 159, 10), Color::WHITE),

            ("7", 0, 1, 1, Color::argb(255, 55, 58, 70), Color::WHITE),
            ("8", 1, 1, 1, Color::argb(255, 55, 58, 70), Color::WHITE),
            ("9", 2, 1, 1, Color::argb(255, 55, 58, 70), Color::WHITE),
            ("×", 3, 1, 1, Color::argb(255, 255, 159, 10), Color::WHITE),

            ("4", 0, 2, 1, Color::argb(255, 55, 58, 70), Color::WHITE),
            ("5", 1, 2, 1, Color::argb(255, 55, 58, 70), Color::WHITE),
            ("6", 2, 2, 1, Color::argb(255, 55, 58, 70), Color::WHITE),
            ("-", 3, 2, 1, Color::argb(255, 255, 159, 10), Color::WHITE),

            ("1", 0, 3, 1, Color::argb(255, 55, 58, 70), Color::WHITE),
            ("2", 1, 3, 1, Color::argb(255, 55, 58, 70), Color::WHITE),
            ("3", 2, 3, 1, Color::argb(255, 55, 58, 70), Color::WHITE),
            ("+", 3, 3, 1, Color::argb(255, 255, 159, 10), Color::WHITE),

            ("0", 0, 4, 2, Color::argb(255, 55, 58, 70), Color::WHITE),
            (".", 2, 4, 1, Color::argb(255, 55, 58, 70), Color::WHITE),
            ("=", 3, 4, 1, Color::argb(255, 255, 159, 10), Color::WHITE),
        ];

        for (label, col, row, col_span, bg_col, text_col) in buttons.iter() {
            let bx = ix + btn_margin + *col as i32 * (btn_w + btn_margin);
            let by = start_y + *row as i32 * (btn_h + btn_margin);
            let bw = if *col_span == 2 { btn_w * 2 + btn_margin } else { btn_w };

            // Rounded button pill
            canvas.fill_rounded_rect(bx, by, bw, btn_h, 8, *bg_col);
            canvas.draw_rounded_rect(bx, by, bw, btn_h, 8, Color::argb(30, 255, 255, 255));

            // Centered label text
            let tw = measure_string(label);
            let tx = bx + (bw - tw) / 2;
            let ty = by + (btn_h - 16) / 2;
            draw_string(canvas, tx, ty, label, *text_col);
        }
    }
}

fn parse_f64(s: &str) -> f64 {
    let mut val = 0.0;
    let mut sign = 1.0;
    let mut chars = s.chars().peekable();

    if let Some(&'-') = chars.peek() {
        sign = -1.0;
        chars.next();
    }

    let mut in_frac = false;
    let mut frac_div = 1.0;

    for c in chars {
        if c == '.' {
            in_frac = true;
            continue;
        }
        if let Some(digit) = c.to_digit(10) {
            if in_frac {
                frac_div *= 10.0;
                val += (digit as f64) / frac_div;
            } else {
                val = val * 10.0 + (digit as f64);
            }
        }
    }

    sign * val
}

fn format_f64(val: f64) -> String {
    if val.is_nan() || val.is_infinite() {
        return String::from("Error");
    }
    let integer_part = val as i64;
    let frac = (val - integer_part as f64).abs();
    if frac < 0.00001 {
        format!("{}", integer_part)
    } else {
        let frac_int = (frac * 10000.0) as i64;
        format!("{}.{:04}", integer_part, frac_int)
    }
}
