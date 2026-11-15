use alloc::format;
use alloc::string::String;
use libgui::event::{GuiEvent, KeyCode, Modifiers, MouseButton};
use libgui::gfx::Rect;
use libgui::system::SystemClipboard;
use libgui::widget::{MenuBar, MenuBarState, MenuItemSpec, MenuSpec};
use crate::gui::canvas::Canvas;
use crate::gui::color::Color;
use crate::gui::font::{draw_ui_string, draw_ui_string_centered_v, measure_ui_string};

const CALC_EDIT_ITEMS: &[MenuItemSpec] = &[
    MenuItemSpec::action(1, "Copy", "Ctrl+C"),
    MenuItemSpec::action(2, "Paste", "Ctrl+V"),
    MenuItemSpec::sep(),
    MenuItemSpec::action(3, "Clear", "Esc"),
];

const CALC_VIEW_ITEMS: &[MenuItemSpec] = &[
    MenuItemSpec::action(10, "Standard", ""),
    MenuItemSpec::action(11, "Toggle Theme", ""),
];

const CALC_HELP_ITEMS: &[MenuItemSpec] = &[
    MenuItemSpec::action(20, "About Calculator", ""),
];

const CALC_MENUS: &[MenuSpec] = &[
    MenuSpec { title: "Edit", items: CALC_EDIT_ITEMS },
    MenuSpec { title: "View", items: CALC_VIEW_ITEMS },
    MenuSpec { title: "Help", items: CALC_HELP_ITEMS },
];

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
    pub menu_state: MenuBarState,
    pub context_menu: Option<(i32, i32)>,
    pub ctx_hover: Option<usize>,
    pub pending_toggle_theme: bool,
    pub pending_about: bool,
}

impl CalcApp {
    pub fn new() -> Self {
        Self {
            display: String::from("0"),
            operand: 0.0,
            op: CalcOp::None,
            clear_on_next_digit: false,
            active_btn_idx: None,
            menu_state: MenuBarState::default(),
            context_menu: None,
            ctx_hover: None,
            pending_toggle_theme: false,
            pending_about: false,
        }
    }

    pub fn copy_display(&self) {
        let txt = if self.display.is_empty() { "0" } else { self.display.as_str() };
        SystemClipboard::set_text(txt);
    }

    pub fn paste_clipboard(&mut self) {
        let clip = SystemClipboard::get_text();
        let mut filtered = String::new();
        for ch in clip.trim().chars() {
            if ch.is_ascii_digit() || (ch == '-' && filtered.is_empty()) || (ch == '.' && !filtered.contains('.')) {
                if filtered.len() < 12 {
                    filtered.push(ch);
                }
            }
        }
        if !filtered.is_empty() && filtered != "-" && filtered != "." {
            self.display = filtered;
            self.clear_on_next_digit = false;
        }
    }

    pub fn backspace(&mut self) {
        if self.clear_on_next_digit {
            self.clear();
            return;
        }
        if self.display.len() > 1 {
            self.display.pop();
            if self.display == "-" {
                self.display = String::from("0");
            }
        } else {
            self.display = String::from("0");
        }
    }

    fn apply_menu_cmd(&mut self, cmd: u16) {
        match cmd {
            1 => self.copy_display(),
            2 => self.paste_clipboard(),
            3 | 10 => self.clear(),
            11 => self.pending_toggle_theme = true,
            20 => self.pending_about = true,
            _ => {}
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

    pub fn handle_key(&mut self, key: KeyCode, ch: Option<char>, mods: Modifiers) {
        if mods.ctrl {
            match ch {
                Some('c') | Some('C') => {
                    self.copy_display();
                    return;
                }
                Some('v') | Some('V') => {
                    self.paste_clipboard();
                    return;
                }
                _ => {}
            }
        }
        match key {
            KeyCode::Backspace => self.backspace(),
            KeyCode::Escape => {
                self.context_menu = None;
                self.menu_state.open_menu = None;
                self.clear();
            }
            KeyCode::Enter => self.compute(),
            _ => {
                if let Some(c) = ch {
                    self.handle_char(c);
                }
            }
        }
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
            '\x08' => self.backspace(),
            'c' | 'C' | '\x1B' => self.clear(),
            '%' => self.percent(),
            _ => {}
        }
    }

    pub fn handle_mouse_move(&mut self, px: i32, py: i32, ix: i32, iy: i32, iw: i32, ih: i32) -> bool {
        let mut changed = false;
        if let Some((cx, cy)) = self.context_menu {
            let cw = 132;
            let row_h = 22;
            let ch = row_h * 3 + 4;
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
        let btn_margin = 5;
        let start_y = iy + 64;
        let btn_w = (iw - btn_margin * 5) / 4;
        let btn_h = (ih - 64 - btn_margin * 6) / 5;
        let coords: [(usize, usize, usize); 19] = [
            (0, 0, 1), (1, 0, 1), (2, 0, 1), (3, 0, 1),
            (0, 1, 1), (1, 1, 1), (2, 1, 1), (3, 1, 1),
            (0, 2, 1), (1, 2, 1), (2, 2, 1), (3, 2, 1),
            (0, 3, 1), (1, 3, 1), (2, 3, 1), (3, 3, 1),
            (0, 4, 2), (2, 4, 1), (3, 4, 1),
        ];
        let mut hover_idx = None;
        if btn_w > 0 && btn_h > 0 {
            for (idx, (col, row, span)) in coords.iter().enumerate() {
                let bx = ix + btn_margin + *col as i32 * (btn_w + btn_margin);
                let by = start_y + *row as i32 * (btn_h + btn_margin);
                let bw = if *span == 2 { btn_w * 2 + btn_margin } else { btn_w };
                if px >= bx && px <= bx + bw && py >= by && py <= by + btn_h {
                    hover_idx = Some(idx);
                    break;
                }
            }
        }
        if hover_idx != self.active_btn_idx {
            self.active_btn_idx = hover_idx;
            changed = true;
        }
        let bar_rect = Rect::new(ix, iy, iw, 22);
        let (mres, _) = MenuBar::handle_event(
            &mut self.menu_state,
            bar_rect,
            CALC_MENUS,
            GuiEvent::MouseMove { x: px, y: py },
        );
        changed || mres.needs_redraw()
    }

    pub fn handle_right_click(&mut self, px: i32, py: i32, ix: i32, iy: i32, iw: i32, ih: i32) {
        if px >= ix && px < ix + iw && py >= iy + 22 && py < iy + ih {
            self.menu_state.open_menu = None;
            let cx = px.min(ix + iw - 134).max(ix + 2);
            let cy = py.min(iy + ih - 72).max(iy + 22);
            self.context_menu = Some((cx, cy));
            self.ctx_hover = None;
        } else {
            self.context_menu = None;
        }
    }

    pub fn handle_click(&mut self, px: i32, py: i32, ix: i32, iy: i32, iw: i32, ih: i32) {
        if let Some((cx, cy)) = self.context_menu {
            let cw = 132;
            let row_h = 22;
            let ch = row_h * 3 + 4;
            if px >= cx && px < cx + cw && py >= cy + 2 && py < cy + ch - 2 {
                let idx = ((py - cy - 2) / row_h) as usize;
                match idx {
                    0 => self.copy_display(),
                    1 => self.paste_clipboard(),
                    2 => self.clear(),
                    _ => {}
                }
                self.context_menu = None;
                return;
            }
            self.context_menu = None;
        }

        let bar_rect = Rect::new(ix, iy, iw, 22);
        let (res, cmd) = MenuBar::handle_event(
            &mut self.menu_state,
            bar_rect,
            CALC_MENUS,
            GuiEvent::MouseDown {
                x: px,
                y: py,
                button: MouseButton::Left,
            },
        );
        if let Some(id) = cmd {
            self.apply_menu_cmd(id);
            return;
        }
        if res.consumed() {
            return;
        }

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
        let theme = libgui::gfx::current_theme();
        canvas.fill_rect(ix, iy, iw, ih, Color::from_libgui(theme.face));
        let bar_rect = Rect::new(ix, iy, iw, 22);
        canvas.with_surface(|surf| {
            MenuBar::draw_bar(surf, bar_rect, CALC_MENUS, self.menu_state, &theme);
        });

        // 1. Clean Flat Metro LCD Display
        let disp_box_y = iy + 26;
        canvas.fill_rect(ix + 8, disp_box_y, iw - 16, 32, Color::from_libgui(theme.input_bg));
        canvas.draw_rect(ix + 8, disp_box_y, iw - 16, 32, Color::from_libgui(theme.input_border));

        let disp_text = if self.display.is_empty() { "0" } else { &self.display };
        let disp_w = measure_ui_string(disp_text);
        let disp_x = (ix + iw - 14).saturating_sub(disp_w).max(ix + 12);
        draw_ui_string_centered_v(canvas, disp_x, disp_box_y, 32, disp_text, Color::from_libgui(theme.text_primary));

        // 2. Flat Sharp Windows 8.1 Calculator Keypad
        let btn_margin = 5;
        let start_y = iy + 64;
        let btn_w = (iw - btn_margin * 5) / 4;
        let btn_h = (ih - 64 - btn_margin * 6) / 5;

        let num_bg = Color::from_libgui(theme.face_light);
        let op_bg = Color::from_libgui(theme.face_dark);
        let eq_bg = Color::from_libgui(theme.accent);
        let fg = Color::from_libgui(theme.text_primary);
        let op_fg = Color::from_libgui(theme.text_link);
        let clr_fg = Color::from_libgui(theme.danger);

        let buttons: [(&str, usize, usize, usize, Color, Color); 19] = [
            ("AC", 0, 0, 1, op_bg, clr_fg),
            ("+/-", 1, 0, 1, op_bg, op_fg),
            ("%", 2, 0, 1, op_bg, op_fg),
            ("÷", 3, 0, 1, op_bg, op_fg),

            ("7", 0, 1, 1, num_bg, fg),
            ("8", 1, 1, 1, num_bg, fg),
            ("9", 2, 1, 1, num_bg, fg),
            ("×", 3, 1, 1, op_bg, op_fg),

            ("4", 0, 2, 1, num_bg, fg),
            ("5", 1, 2, 1, num_bg, fg),
            ("6", 2, 2, 1, num_bg, fg),
            ("-", 3, 2, 1, op_bg, op_fg),

            ("1", 0, 3, 1, num_bg, fg),
            ("2", 1, 3, 1, num_bg, fg),
            ("3", 2, 3, 1, num_bg, fg),
            ("+", 3, 3, 1, op_bg, op_fg),

            ("0", 0, 4, 2, num_bg, fg),
            (".", 2, 4, 1, num_bg, fg),
            ("=", 3, 4, 1, eq_bg, Color::WHITE),
        ];

        let bdr = Color::from_libgui(theme.ctrl_border);
        let eq_fg = Color::from_libgui(libgui::gfx::contrast_text_on(theme.accent));
        for (idx, (label, col, row, col_span, bg_col, text_col)) in buttons.iter().enumerate() {
            let bx = ix + btn_margin + *col as i32 * (btn_w + btn_margin);
            let by = start_y + *row as i32 * (btn_h + btn_margin);
            let bw = if *col_span == 2 { btn_w * 2 + btn_margin } else { btn_w };
            let is_hover = self.active_btn_idx == Some(idx);
            let draw_bg = if is_hover {
                Color::lerp(*bg_col, Color::from_libgui(theme.accent), 38)
            } else {
                *bg_col
            };
            let draw_fg = if *label == "=" { eq_fg } else { *text_col };

            canvas.fill_rect(bx, by, bw, btn_h, draw_bg);
            canvas.draw_rect(bx, by, bw, btn_h, if *label == "=" || is_hover { eq_bg } else { bdr });

            let tw = measure_ui_string(label);
            let tx = bx + (bw - tw) / 2;
            draw_ui_string_centered_v(canvas, tx, by, btn_h, label, draw_fg);
        }

        if let Some((cx, cy)) = self.context_menu {
            let cw = 132;
            let row_h = 22;
            let ch = row_h * 3 + 4;
            canvas.fill_rect(cx, cy, cw, ch, Color::from_libgui(theme.face_light));
            canvas.draw_rect(cx, cy, cw, ch, Color::from_libgui(theme.ctrl_border));
            let items = ["Copy Value", "Paste Value", "Clear (AC)"];
            for (i, label) in items.iter().enumerate() {
                let ry = cy + 2 + i as i32 * row_h;
                let hovered = self.ctx_hover == Some(i);
                if hovered {
                    canvas.fill_rect(cx + 2, ry, cw - 4, row_h, Color::from_libgui(theme.selection));
                }
                let tc = Color::from_libgui(if hovered { theme.selection_text } else { theme.text_primary });
                draw_ui_string(canvas, cx + 10, ry + 3, label, tc);
            }
        }

        canvas.with_surface(|surf| {
            MenuBar::draw_popup(surf, bar_rect, CALC_MENUS, self.menu_state, &theme);
        });
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
