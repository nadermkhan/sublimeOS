use alloc::format;
use crate::event::{EventResult, GuiEvent, MouseButton};
use crate::font::{with_default_font, TextStyle};
use crate::gfx::{ArrowDir, Color, Rect, Surface, Theme, ThemePart, ThemeState, UxTheme};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Date {
    pub year: i32,
    pub month: u8,
    pub day: u8,
}

impl Date {
    pub const fn new(year: i32, month: u8, day: u8) -> Self {
        Self { year, month, day }
    }

    pub fn is_leap_year(year: i32) -> bool {
        (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
    }

    pub fn days_in_month(year: i32, month: u8) -> u8 {
        match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 => if Self::is_leap_year(year) { 29 } else { 28 },
            _ => 30,
        }
    }

    /// 0 = Sunday .. 6 = Saturday (Sakamoto's algorithm).
    pub fn weekday(year: i32, month: u8, day: u8) -> u8 {
        const T: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
        let m = month.clamp(1, 12) as i32;
        let y = if m < 3 { year - 1 } else { year };
        ((y + y / 4 - y / 100 + y / 400 + T[(m - 1) as usize] + day as i32).rem_euclid(7)) as u8
    }

    pub fn month_name(month: u8) -> &'static str {
        match month {
            1 => "January",
            2 => "February",
            3 => "March",
            4 => "April",
            5 => "May",
            6 => "June",
            7 => "July",
            8 => "August",
            9 => "September",
            10 => "October",
            11 => "November",
            _ => "December",
        }
    }

    pub fn format_iso(&self) -> alloc::string::String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct MonthCalendarState {
    pub view_year: i32,
    pub view_month: u8,
    pub selected: Date,
    pub today: Date,
    pub hovered_day: Option<u8>,
}

impl Default for MonthCalendarState {
    fn default() -> Self {
        let d = Date::new(2026, 11, 14);
        Self {
            view_year: d.year,
            view_month: d.month,
            selected: d,
            today: d,
            hovered_day: None,
        }
    }
}

/// ComCtl32 `SysMonthCal32` interactive month grid calendar.
pub struct MonthCalendar;

impl MonthCalendar {
    const HDR_H: i32 = 26;
    const DOW_H: i32 = 18;

    fn rects(bounds: Rect) -> (Rect, Rect, Rect, Rect) {
        let hdr = Rect::new(bounds.x + 2, bounds.y + 2, (bounds.w - 4).max(0), Self::HDR_H);
        let prev_r = Rect::new(hdr.x + 2, hdr.y + 3, 20, hdr.h - 6);
        let next_r = Rect::new(hdr.right() - 22, hdr.y + 3, 20, hdr.h - 6);
        let grid = Rect::new(
            bounds.x + 4,
            hdr.bottom() + Self::DOW_H,
            (bounds.w - 8).max(7),
            (bounds.bottom() - hdr.bottom() - Self::DOW_H - 4).max(6),
        );
        (hdr, prev_r, next_r, grid)
    }

    fn shift_month(state: &mut MonthCalendarState, delta: i32) {
        let total = state.view_year * 12 + (state.view_month as i32 - 1) + delta;
        state.view_year = total.div_euclid(12);
        state.view_month = (total.rem_euclid(12) + 1) as u8;
    }

    pub fn handle_event(state: &mut MonthCalendarState, bounds: Rect, event: GuiEvent) -> (EventResult, Option<Date>) {
        let (_, prev_r, next_r, grid) = Self::rects(bounds);
        let cell_w = (grid.w / 7).max(1);
        let cell_h = (grid.h / 6).max(1);
        let first_dow = Date::weekday(state.view_year, state.view_month, 1) as i32;
        let max_day = Date::days_in_month(state.view_year, state.view_month);

        match event {
            GuiEvent::MouseMove { x, y } => {
                let mut hday = None;
                if grid.contains(x, y) {
                    let col = ((x - grid.x) / cell_w).clamp(0, 6);
                    let row = ((y - grid.y) / cell_h).clamp(0, 5);
                    let cell_idx = row * 7 + col - first_dow + 1;
                    if cell_idx >= 1 && cell_idx <= max_day as i32 {
                        hday = Some(cell_idx as u8);
                    }
                }
                if hday != state.hovered_day {
                    state.hovered_day = hday;
                    return (EventResult::NeedsRedraw, None);
                }
                (EventResult::Ignored, None)
            }
            GuiEvent::MouseDown { x, y, button: MouseButton::Left } => {
                if prev_r.contains(x, y) {
                    Self::shift_month(state, -1);
                    return (EventResult::NeedsRedraw, None);
                }
                if next_r.contains(x, y) {
                    Self::shift_month(state, 1);
                    return (EventResult::NeedsRedraw, None);
                }
                if grid.contains(x, y) {
                    let col = ((x - grid.x) / cell_w).clamp(0, 6);
                    let row = ((y - grid.y) / cell_h).clamp(0, 5);
                    let cell_idx = row * 7 + col - first_dow + 1;
                    if cell_idx >= 1 && cell_idx <= max_day as i32 {
                        state.selected = Date::new(state.view_year, state.view_month, cell_idx as u8);
                        return (EventResult::NeedsRedraw, Some(state.selected));
                    }
                }
                (EventResult::Ignored, None)
            }
            _ => (EventResult::Ignored, None),
        }
    }

    pub fn draw(surface: &mut Surface, bounds: Rect, state: MonthCalendarState, theme: &Theme) {
        if bounds.w < 140 || bounds.h < 130 {
            return;
        }
        surface.fill_rect(bounds, theme.input_bg);
        surface.draw_rect(bounds, theme.ctrl_border);

        let (hdr, prev_r, next_r, grid) = Self::rects(bounds);
        surface.draw_gradient_v(hdr, theme.title_top, theme.title_bot);
        surface.fill_rounded_rect(prev_r, 2, theme.face);
        surface.draw_arrow(prev_r, ArrowDir::Left, theme.text_primary);
        surface.fill_rounded_rect(next_r, 2, theme.face);
        surface.draw_arrow(next_r, ArrowDir::Right, theme.text_primary);

        let title = format!("{} {}", Date::month_name(state.view_month), state.view_year);
        with_default_font(|font| {
            let st = TextStyle {
                bold: true,
                size_px: 13,
                color: theme.selection_text,
                ..TextStyle::ui(theme.selection_text)
            };
            let (tw, lh) = font.measure_text(&title, st);
            font.draw_text(surface, hdr.x + (hdr.w - tw) / 2, hdr.y + (hdr.h - lh) / 2, &title, st);
        });

        let dows = ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"];
        let cell_w = (grid.w / 7).max(1);
        let cell_h = (grid.h / 6).max(1);
        let dow_y = hdr.bottom() + 2;
        surface.fill_rect(Rect::new(grid.x, hdr.bottom() + Self::DOW_H - 1, grid.w, 1), theme.border_shadow);
        for (c, &dw) in dows.iter().enumerate() {
            let cx = grid.x + c as i32 * cell_w;
            with_default_font(|font| {
                let st = TextStyle::ui_sized(12, theme.text_link);
                let (tw, _) = font.measure_text(dw, st);
                font.draw_text(surface, cx + (cell_w - tw) / 2, dow_y, dw, st);
            });
        }

        let first_dow = Date::weekday(state.view_year, state.view_month, 1) as i32;
        let max_day = Date::days_in_month(state.view_year, state.view_month);
        for d in 1..=max_day {
            let cell = first_dow + (d as i32 - 1);
            let row = cell / 7;
            let col = cell % 7;
            let cr = Rect::new(grid.x + col * cell_w + 1, grid.y + row * cell_h + 1, (cell_w - 2).max(1), (cell_h - 2).max(1));
            let is_sel = state.selected == Date::new(state.view_year, state.view_month, d);
            let is_today = state.today == Date::new(state.view_year, state.view_month, d);
            let is_hover = state.hovered_day == Some(d);
            if is_sel {
                surface.fill_rounded_rect(cr, 3, theme.selection);
            } else if is_hover {
                surface.fill_rounded_rect(cr, 3, Color::lerp(theme.input_bg, theme.selection, 45));
            }
            if is_today {
                surface.draw_rounded_rect(cr, 3, theme.danger);
            }
            let fg = if is_sel { theme.selection_text } else { theme.text_primary };
            let txt = format!("{}", d);
            with_default_font(|font| {
                let st = TextStyle::ui_sized(12, fg);
                let (tw, lh) = font.measure_text(&txt, st);
                font.draw_text(surface, cr.x + (cr.w - tw) / 2, cr.y + (cr.h - lh) / 2, &txt, st);
            });
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct DatePickerState {
    pub calendar: MonthCalendarState,
    pub open: bool,
    pub hovered: bool,
}

/// ComCtl32 `SysDateTimePick32` compact date input with dropdown `MonthCalendar`.
pub struct DateTimePicker;

impl DateTimePicker {
    fn popup_rect(bounds: Rect) -> Rect {
        Rect::new(bounds.x, bounds.bottom(), bounds.w.max(210), 178)
    }

    pub fn handle_event(state: &mut DatePickerState, bounds: Rect, event: GuiEvent) -> (EventResult, Option<Date>) {
        match event {
            GuiEvent::MouseMove { x, y } => {
                let h = bounds.contains(x, y);
                let mut res = EventResult::Ignored;
                if h != state.hovered {
                    state.hovered = h;
                    res = EventResult::NeedsRedraw;
                }
                if state.open {
                    let (cr, _) = MonthCalendar::handle_event(&mut state.calendar, Self::popup_rect(bounds), event);
                    res.merge(cr);
                }
                (res, None)
            }
            GuiEvent::MouseDown { x, y, button: MouseButton::Left } => {
                if bounds.contains(x, y) {
                    state.open = !state.open;
                    return (EventResult::NeedsRedraw, None);
                }
                if state.open {
                    let pr = Self::popup_rect(bounds);
                    if pr.contains(x, y) {
                        let (cr, picked) = MonthCalendar::handle_event(&mut state.calendar, pr, event);
                        if picked.is_some() {
                            state.open = false;
                            return (EventResult::NeedsRedraw, picked);
                        }
                        return (cr, None);
                    }
                    state.open = false;
                    return (EventResult::NeedsRedraw, None);
                }
                (EventResult::Ignored, None)
            }
            _ => (EventResult::Ignored, None),
        }
    }

    pub fn draw(surface: &mut Surface, bounds: Rect, state: DatePickerState, theme: &Theme) {
        if bounds.w < 64 || bounds.h < 18 {
            return;
        }
        UxTheme::draw_background(
            surface,
            ThemePart::ComboBox,
            if state.open { ThemeState::Focused } else if state.hovered { ThemeState::Hot } else { ThemeState::Normal },
            bounds,
            theme,
        );
        let bw = 20.min(bounds.w / 3);
        let btn_r = Rect::new(bounds.right() - bw - 1, bounds.y + 1, bw, (bounds.h - 2).max(2));
        UxTheme::draw_background(surface, ThemePart::ComboDropButton, ThemeState::from_flags(state.hovered, state.open, false, false), btn_r, theme);

        let text = state.calendar.selected.format_iso();
        surface.push_clip(Rect::new(bounds.x + 6, bounds.y, (bounds.w - bw - 10).max(0), bounds.h));
        with_default_font(|font| {
            let st = TextStyle::ui(theme.text_primary);
            let (_, lh) = font.line_metrics(st);
            font.draw_text(surface, bounds.x + 6, bounds.y + (bounds.h - lh) / 2, &text, st);
        });
        surface.pop_clip();
    }

    pub fn draw_popup(surface: &mut Surface, bounds: Rect, state: DatePickerState, theme: &Theme) {
        if state.open {
            let pr = Self::popup_rect(bounds);
            surface.fill_rect(Rect::new(pr.x + 2, pr.y + 2, pr.w, pr.h), Color::argb(64, 0, 0, 0));
            MonthCalendar::draw(surface, pr, state.calendar, theme);
        }
    }
}
