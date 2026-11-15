use alloc::format;
use alloc::string::String;
use crate::event::{EventResult, GuiEvent, KeyCode, Modifiers, MouseButton};
use crate::font::{with_default_font, TextStyle};
use crate::gfx::{Rect, Surface, Theme, ThemePart, ThemeState, UxTheme};

#[derive(Clone, Copy, Debug)]
pub struct SpinBoxState {
    pub value: i32,
    pub min: i32,
    pub max: i32,
    pub step: i32,
    pub focused: bool,
    pub hover_up: bool,
    pub hover_down: bool,
    pub press_up: bool,
    pub press_down: bool,
    pub disabled: bool,
}

impl SpinBoxState {
    pub const fn new(value: i32, min: i32, max: i32, step: i32) -> Self {
        let hi = if max < min { min } else { max };
        Self {
            value: if value < min { min } else if value > hi { hi } else { value },
            min,
            max: hi,
            step: if step <= 0 { 1 } else { step },
            focused: false,
            hover_up: false,
            hover_down: false,
            press_up: false,
            press_down: false,
            disabled: false,
        }
    }
}

/// ComCtl32 `msctls_updown32` numeric spinner with integrated ▲ / ▼ buddy buttons.
pub struct SpinBox;

impl SpinBox {
    const BTN_W: i32 = 18;

    fn rects(bounds: Rect) -> (Rect, Rect, Rect) {
        let bw = Self::BTN_W.min(bounds.w / 3);
        let half_h = (bounds.h / 2).max(4);
        let edit_r = Rect::new(bounds.x, bounds.y, (bounds.w - bw).max(0), bounds.h);
        let up_r = Rect::new(edit_r.right(), bounds.y, bw, half_h);
        let down_r = Rect::new(edit_r.right(), bounds.y + half_h, bw, bounds.h - half_h);
        (edit_r, up_r, down_r)
    }

    pub fn handle_event(state: &mut SpinBoxState, bounds: Rect, event: GuiEvent) -> (EventResult, bool) {
        if state.disabled {
            return (EventResult::Ignored, false);
        }
        let (_, up_r, down_r) = Self::rects(bounds);
        match event {
            GuiEvent::MouseMove { x, y } => {
                let hu = up_r.contains(x, y);
                let hd = down_r.contains(x, y);
                if hu != state.hover_up || hd != state.hover_down {
                    state.hover_up = hu;
                    state.hover_down = hd;
                    return (EventResult::NeedsRedraw, false);
                }
                (EventResult::Ignored, false)
            }
            GuiEvent::MouseDown { x, y, button: MouseButton::Left } => {
                let inside = bounds.contains(x, y);
                let fc = state.focused != inside;
                state.focused = inside;
                if up_r.contains(x, y) {
                    state.press_up = true;
                    return (EventResult::NeedsRedraw, false);
                }
                if down_r.contains(x, y) {
                    state.press_down = true;
                    return (EventResult::NeedsRedraw, false);
                }
                if inside || fc {
                    return (EventResult::NeedsRedraw, false);
                }
                (EventResult::Ignored, false)
            }
            GuiEvent::MouseUp { x, y, button: MouseButton::Left } => {
                let pu = state.press_up;
                let pd = state.press_down;
                state.press_up = false;
                state.press_down = false;
                let old = state.value;
                if pu && up_r.contains(x, y) {
                    state.value = (state.value + state.step).clamp(state.min, state.max);
                    return (EventResult::NeedsRedraw, state.value != old);
                }
                if pd && down_r.contains(x, y) {
                    state.value = (state.value - state.step).clamp(state.min, state.max);
                    return (EventResult::NeedsRedraw, state.value != old);
                }
                if pu || pd {
                    return (EventResult::NeedsRedraw, false);
                }
                (EventResult::Ignored, false)
            }
            GuiEvent::MouseWheel { x, y, delta_y } if bounds.contains(x, y) => {
                let old = state.value;
                let dir = if delta_y > 0 { 1 } else { -1 };
                state.value = (state.value + dir * state.step).clamp(state.min, state.max);
                (EventResult::NeedsRedraw, state.value != old)
            }
            GuiEvent::KeyDown { key, .. } if state.focused => {
                let old = state.value;
                match key {
                    KeyCode::Up => {
                        state.value = (state.value + state.step).clamp(state.min, state.max);
                        (EventResult::NeedsRedraw, state.value != old)
                    }
                    KeyCode::Down => {
                        state.value = (state.value - state.step).clamp(state.min, state.max);
                        (EventResult::NeedsRedraw, state.value != old)
                    }
                    KeyCode::Char(c) if c.is_ascii_digit() => {
                        let d = (c as u8 - b'0') as i32;
                        let candidate = state.value.saturating_mul(10).saturating_add(d);
                        state.value = candidate.clamp(state.min, state.max);
                        (EventResult::NeedsRedraw, state.value != old)
                    }
                    KeyCode::Backspace => {
                        state.value = (state.value / 10).clamp(state.min, state.max);
                        (EventResult::NeedsRedraw, state.value != old)
                    }
                    _ => (EventResult::Ignored, false),
                }
            }
            _ => (EventResult::Ignored, false),
        }
    }

    pub fn draw(surface: &mut Surface, bounds: Rect, state: SpinBoxState, theme: &Theme) {
        if bounds.w < 32 || bounds.h < 16 {
            return;
        }
        let (edit_r, up_r, down_r) = Self::rects(bounds);
        let est = if state.disabled {
            ThemeState::Disabled
        } else if state.focused {
            ThemeState::Focused
        } else {
            ThemeState::Normal
        };
        UxTheme::draw_background(surface, ThemePart::Edit, est, bounds, theme);
        UxTheme::draw_background(
            surface,
            ThemePart::SpinUp,
            ThemeState::from_flags(state.hover_up, state.press_up, state.disabled, false),
            up_r,
            theme,
        );
        UxTheme::draw_background(
            surface,
            ThemePart::SpinDown,
            ThemeState::from_flags(state.hover_down, state.press_down, state.disabled, false),
            down_r,
            theme,
        );

        let text = format!("{}", state.value);
        surface.push_clip(edit_r);
        with_default_font(|font| {
            let fg = if state.disabled { theme.text_secondary } else { theme.text_primary };
            let st = TextStyle::ui(fg);
            let (_, lh) = font.line_metrics(st);
            font.draw_text(surface, edit_r.x + 6, edit_r.y + (edit_r.h - lh) / 2, &text, st);
        });
        surface.pop_clip();
    }
}

#[derive(Clone, Copy, Debug)]
pub struct IpAddressState {
    pub octets: [u8; 4],
    pub active_field: Option<usize>,
    pub edit_accum: u16,
    pub edit_digits: u8,
}

impl Default for IpAddressState {
    fn default() -> Self {
        Self::new([192, 168, 1, 1])
    }
}

impl IpAddressState {
    pub const fn new(octets: [u8; 4]) -> Self {
        Self {
            octets,
            active_field: None,
            edit_accum: 0,
            edit_digits: 0,
        }
    }
}

/// ComCtl32 `SysIPAddress32` 4-octet dotted IPv4 address input control.
pub struct IpAddressInput;

impl IpAddressInput {
    pub fn handle_event(state: &mut IpAddressState, bounds: Rect, event: GuiEvent) -> (EventResult, bool) {
        match event {
            GuiEvent::MouseDown { x, y, button: MouseButton::Left } => {
                if bounds.contains(x, y) {
                    let field_w = (bounds.w / 4).max(1);
                    let idx = ((x - bounds.x) / field_w).clamp(0, 3) as usize;
                    state.active_field = Some(idx);
                    state.edit_digits = 0;
                    state.edit_accum = 0;
                    return (EventResult::NeedsRedraw, false);
                } else if state.active_field.is_some() {
                    state.active_field = None;
                    return (EventResult::NeedsRedraw, false);
                }
                (EventResult::Ignored, false)
            }
            GuiEvent::KeyDown { key, .. } => {
                let Some(idx) = state.active_field else {
                    return (EventResult::Ignored, false);
                };
                match key {
                    KeyCode::Char('.') | KeyCode::Right | KeyCode::Tab => {
                        state.active_field = Some((idx + 1) % 4);
                        state.edit_digits = 0;
                        state.edit_accum = 0;
                        (EventResult::NeedsRedraw, false)
                    }
                    KeyCode::Left => {
                        state.active_field = Some((idx + 3) % 4);
                        state.edit_digits = 0;
                        state.edit_accum = 0;
                        (EventResult::NeedsRedraw, false)
                    }
                    KeyCode::Backspace => {
                        state.octets[idx] /= 10;
                        state.edit_digits = state.edit_digits.saturating_sub(1);
                        state.edit_accum = state.octets[idx] as u16;
                        (EventResult::NeedsRedraw, true)
                    }
                    KeyCode::Char(c) if c.is_ascii_digit() => {
                        let d = (c as u8 - b'0') as u16;
                        let next = if state.edit_digits == 0 { d } else { state.edit_accum * 10 + d };
                        state.octets[idx] = next.min(255) as u8;
                        state.edit_accum = next.min(255);
                        state.edit_digits += 1;
                        if state.edit_digits >= 3 {
                            state.active_field = Some((idx + 1).min(3));
                            state.edit_digits = 0;
                            state.edit_accum = 0;
                        }
                        (EventResult::NeedsRedraw, true)
                    }
                    _ => (EventResult::Ignored, false),
                }
            }
            _ => (EventResult::Ignored, false),
        }
    }

    pub fn draw(surface: &mut Surface, bounds: Rect, state: IpAddressState, theme: &Theme) {
        if bounds.w < 64 || bounds.h < 18 {
            return;
        }
        let st = if state.active_field.is_some() { ThemeState::Focused } else { ThemeState::Normal };
        UxTheme::draw_background(surface, ThemePart::Edit, st, bounds, theme);

        let fw = bounds.w / 4;
        surface.push_clip(bounds);
        with_default_font(|font| {
            let tstyle = TextStyle::ui(theme.text_primary);
            let (_, lh) = font.line_metrics(tstyle);
            let ty = bounds.y + (bounds.h - lh) / 2;
            for i in 0..4 {
                let fx = bounds.x + i as i32 * fw;
                if state.active_field == Some(i) {
                    surface.fill_rect(Rect::new(fx + 3, bounds.y + 3, (fw - 8).max(4), (bounds.h - 6).max(4)), theme.selection.with_alpha(52));
                }
                let s = format!("{}", state.octets[i]);
                let (tw, _) = font.measure_text(&s, tstyle);
                font.draw_text(surface, fx + ((fw - tw - 4) / 2).max(2), ty, &s, tstyle);
                if i < 3 {
                    font.draw_text(surface, fx + fw - 4, ty, ".", tstyle);
                }
            }
        });
        surface.pop_clip();
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct HotKeyState {
    pub focused: bool,
    pub key: Option<KeyCode>,
    pub mods: Modifiers,
}

impl HotKeyState {
    pub fn format_combo(&self) -> String {
        let Some(k) = self.key else {
            return String::from("None");
        };
        let mut s = String::new();
        if self.mods.ctrl { s.push_str("Ctrl+"); }
        if self.mods.alt { s.push_str("Alt+"); }
        if self.mods.shift { s.push_str("Shift+"); }
        match k {
            KeyCode::Char(' ') => s.push_str("Space"),
            KeyCode::Char(c) => s.push(c.to_ascii_uppercase()),
            KeyCode::Enter => s.push_str("Enter"),
            KeyCode::Tab => s.push_str("Tab"),
            KeyCode::Escape => s.push_str("Esc"),
            KeyCode::Function(n) => s.push_str(&format!("F{}", n)),
            _ => s.push_str("Key"),
        }
        s
    }
}

/// ComCtl32 `msctls_hotkey32` shortcut capture input box.
pub struct HotKeyBox;

impl HotKeyBox {
    pub fn handle_event(state: &mut HotKeyState, bounds: Rect, event: GuiEvent) -> (EventResult, bool) {
        match event {
            GuiEvent::MouseDown { x, y, button: MouseButton::Left } => {
                let inside = bounds.contains(x, y);
                if state.focused != inside {
                    state.focused = inside;
                    return (EventResult::NeedsRedraw, false);
                }
                (EventResult::Ignored, false)
            }
            GuiEvent::KeyDown { key, mods, .. } if state.focused => {
                if matches!(key, KeyCode::Backspace | KeyCode::Delete) {
                    state.key = None;
                    state.mods = Modifiers::default();
                } else {
                    state.key = Some(key);
                    state.mods = mods;
                }
                (EventResult::NeedsRedraw, true)
            }
            _ => (EventResult::Ignored, false),
        }
    }

    pub fn draw(surface: &mut Surface, bounds: Rect, state: &HotKeyState, theme: &Theme) {
        if bounds.w < 32 || bounds.h < 18 {
            return;
        }
        UxTheme::draw_background(
            surface,
            ThemePart::Edit,
            if state.focused { ThemeState::Focused } else { ThemeState::Normal },
            bounds,
            theme,
        );
        let label = state.format_combo();
        surface.push_clip(bounds);
        with_default_font(|font| {
            let col = if state.key.is_none() { theme.text_secondary } else { theme.text_primary };
            let st = TextStyle::ui(col);
            let (_, lh) = font.line_metrics(st);
            font.draw_text_ellipsis(surface, bounds.x + 6, bounds.y + (bounds.h - lh) / 2, (bounds.w - 12).max(0), &label, st);
        });
        surface.pop_clip();
    }
}
