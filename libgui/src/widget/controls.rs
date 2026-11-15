use crate::event::{EventResult, GuiEvent, KeyCode, MouseButton};
use crate::font::{with_default_font, TextStyle};
use crate::gfx::{ArrowDir, Color, Insets, Rect, Surface, Theme, ThemePart, ThemeState, UxTheme};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CheckState {
    Unchecked,
    Checked,
    Indeterminate,
}

impl CheckState {
    pub fn next(self, three_state: bool) -> Self {
        if three_state {
            match self {
                CheckState::Unchecked => CheckState::Checked,
                CheckState::Checked => CheckState::Indeterminate,
                CheckState::Indeterminate => CheckState::Unchecked,
            }
        } else if self == CheckState::Checked {
            CheckState::Unchecked
        } else {
            CheckState::Checked
        }
    }

    pub fn is_checked(self) -> bool {
        self == CheckState::Checked
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CheckBoxState {
    pub check: CheckState,
    pub three_state: bool,
    pub hovered: bool,
    pub pressed: bool,
    pub focused: bool,
    pub disabled: bool,
}

impl Default for CheckBoxState {
    fn default() -> Self {
        Self::new(false)
    }
}

impl CheckBoxState {
    pub const fn new(checked: bool) -> Self {
        Self {
            check: if checked { CheckState::Checked } else { CheckState::Unchecked },
            three_state: false,
            hovered: false,
            pressed: false,
            focused: false,
            disabled: false,
        }
    }

    pub const fn tristate(check: CheckState) -> Self {
        Self {
            check,
            three_state: true,
            hovered: false,
            pressed: false,
            focused: false,
            disabled: false,
        }
    }
}

pub struct CheckBox;

impl CheckBox {
    /// Returns `(EventResult, toggled)`.
    pub fn handle_event(state: &mut CheckBoxState, rect: Rect, event: GuiEvent) -> (EventResult, bool) {
        if state.disabled {
            state.hovered = false;
            state.pressed = false;
            return (EventResult::Ignored, false);
        }
        match event {
            GuiEvent::MouseMove { x, y } => {
                let h = rect.contains(x, y);
                if h != state.hovered {
                    state.hovered = h;
                    if !h {
                        state.pressed = false;
                    }
                    return (EventResult::NeedsRedraw, false);
                }
                (EventResult::Ignored, false)
            }
            GuiEvent::MouseDown { x, y, button: MouseButton::Left } => {
                let inside = rect.contains(x, y);
                let fc = state.focused != inside;
                state.focused = inside;
                if inside {
                    state.pressed = true;
                    state.hovered = true;
                    return (EventResult::NeedsRedraw, false);
                }
                if fc {
                    return (EventResult::NeedsRedraw, false);
                }
                (EventResult::Ignored, false)
            }
            GuiEvent::MouseUp { x, y, button: MouseButton::Left } => {
                let was_pressed = state.pressed;
                state.pressed = false;
                if was_pressed {
                    if rect.contains(x, y) {
                        state.check = state.check.next(state.three_state);
                        return (EventResult::NeedsRedraw, true);
                    }
                    return (EventResult::NeedsRedraw, false);
                }
                (EventResult::Ignored, false)
            }
            GuiEvent::KeyDown { key: KeyCode::Char(' '), .. } if state.focused => {
                state.check = state.check.next(state.three_state);
                (EventResult::NeedsRedraw, true)
            }
            _ => (EventResult::Ignored, false),
        }
    }

    pub fn draw(surface: &mut Surface, rect: Rect, label: &str, state: CheckBoxState, theme: &Theme) {
        if rect.w < 14 || rect.h < 14 {
            return;
        }
        let box_sz = 14;
        let bx = rect.x + 2;
        let by = rect.y + (rect.h - box_sz) / 2;
        let tstate = match state.check {
            CheckState::Indeterminate if !state.disabled => ThemeState::Mixed,
            _ => ThemeState::from_flags(state.hovered, state.pressed, state.disabled, state.check.is_checked()),
        };
        UxTheme::draw_background(surface, ThemePart::CheckBox, tstate, Rect::new(bx, by, box_sz, box_sz), theme);

        let tx = bx + box_sz + 7;
        let fg = if state.disabled { theme.text_secondary } else { theme.text_primary };
        let text_r = Rect::new(tx, rect.y, (rect.right() - tx).max(0), rect.h);
        if !text_r.is_empty() {
            surface.push_clip(text_r);
            with_default_font(|font| {
                let st = TextStyle::ui(fg);
                let (_, lh) = font.line_metrics(st);
                let ty = rect.y + (rect.h - lh) / 2;
                font.draw_text_ellipsis(surface, tx, ty, text_r.w, label, st);
            });
            surface.pop_clip();
        }
        if state.focused && !state.disabled {
            UxTheme::draw_focus_rect(surface, Rect::new(tx - 2, rect.y + 1, (rect.right() - tx + 1).max(4), (rect.h - 2).max(4)), theme);
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RadioGroupState {
    pub selected: usize,
    pub hovered: Option<usize>,
    pub pressed: Option<usize>,
    pub disabled: bool,
}

impl Default for RadioGroupState {
    fn default() -> Self {
        Self::new(0)
    }
}

impl RadioGroupState {
    pub const fn new(selected: usize) -> Self {
        Self {
            selected,
            hovered: None,
            pressed: None,
            disabled: false,
        }
    }
}

pub struct RadioGroup;

impl RadioGroup {
    pub const ROW_H: i32 = 22;

    fn option_rect(bounds: Rect, idx: usize) -> Rect {
        Rect::new(bounds.x, bounds.y + idx as i32 * Self::ROW_H, bounds.w, Self::ROW_H)
    }

    pub fn handle_event(
        state: &mut RadioGroupState,
        bounds: Rect,
        labels: &[&str],
        event: GuiEvent,
    ) -> (EventResult, Option<usize>) {
        if state.disabled {
            return (EventResult::Ignored, None);
        }
        let count = labels.len();
        match event {
            GuiEvent::MouseMove { x, y } => {
                let mut next = None;
                for i in 0..count {
                    if Self::option_rect(bounds, i).contains(x, y) {
                        next = Some(i);
                        break;
                    }
                }
                if next != state.hovered {
                    state.hovered = next;
                    return (EventResult::NeedsRedraw, None);
                }
                (EventResult::Ignored, None)
            }
            GuiEvent::MouseDown { x, y, button: MouseButton::Left } => {
                for i in 0..count {
                    if Self::option_rect(bounds, i).contains(x, y) {
                        state.pressed = Some(i);
                        return (EventResult::NeedsRedraw, None);
                    }
                }
                (EventResult::Ignored, None)
            }
            GuiEvent::MouseUp { x, y, button: MouseButton::Left } => {
                if let Some(idx) = state.pressed.take() {
                    if idx < count && Self::option_rect(bounds, idx).contains(x, y) {
                        let changed = state.selected != idx;
                        state.selected = idx;
                        return (EventResult::NeedsRedraw, if changed { Some(idx) } else { None });
                    }
                    return (EventResult::NeedsRedraw, None);
                }
                (EventResult::Ignored, None)
            }
            _ => (EventResult::Ignored, None),
        }
    }

    pub fn draw(surface: &mut Surface, bounds: Rect, labels: &[&str], state: RadioGroupState, theme: &Theme) {
        for (i, &label) in labels.iter().enumerate() {
            let r = Self::option_rect(bounds, i);
            if r.bottom() > bounds.bottom() + 2 {
                break;
            }
            let checked = state.selected == i;
            let hovered = state.hovered == Some(i);
            let pressed = state.pressed == Some(i);
            let tstate = ThemeState::from_flags(hovered, pressed, state.disabled, checked);
            let radio_r = Rect::new(r.x + 2, r.y + (r.h - 14) / 2, 14, 14);
            UxTheme::draw_background(surface, ThemePart::RadioButton, tstate, radio_r, theme);

            let tx = r.x + 22;
            let fg = if state.disabled { theme.text_secondary } else { theme.text_primary };
            let text_r = Rect::new(tx, r.y, (r.right() - tx).max(0), r.h);
            surface.push_clip(text_r);
            with_default_font(|font| {
                let st = TextStyle::ui(fg);
                let (_, lh) = font.line_metrics(st);
                font.draw_text_ellipsis(surface, tx, r.y + (r.h - lh) / 2, text_r.w, label, st);
            });
            surface.pop_clip();
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ToggleState {
    pub on: bool,
    pub hovered: bool,
    pub pressed: bool,
    pub disabled: bool,
}

impl ToggleState {
    pub const fn new(on: bool) -> Self {
        Self {
            on,
            hovered: false,
            pressed: false,
            disabled: false,
        }
    }
}

pub struct ToggleSwitch;

impl ToggleSwitch {
    pub fn handle_event(state: &mut ToggleState, rect: Rect, event: GuiEvent) -> (EventResult, bool) {
        if state.disabled {
            return (EventResult::Ignored, false);
        }
        match event {
            GuiEvent::MouseMove { x, y } => {
                let h = rect.contains(x, y);
                if h != state.hovered {
                    state.hovered = h;
                    if !h {
                        state.pressed = false;
                    }
                    return (EventResult::NeedsRedraw, false);
                }
                (EventResult::Ignored, false)
            }
            GuiEvent::MouseDown { x, y, button: MouseButton::Left } => {
                if rect.contains(x, y) {
                    state.pressed = true;
                    return (EventResult::NeedsRedraw, false);
                }
                (EventResult::Ignored, false)
            }
            GuiEvent::MouseUp { x, y, button: MouseButton::Left } => {
                let was = state.pressed;
                state.pressed = false;
                if was {
                    if rect.contains(x, y) {
                        state.on = !state.on;
                        return (EventResult::NeedsRedraw, true);
                    }
                    return (EventResult::NeedsRedraw, false);
                }
                (EventResult::Ignored, false)
            }
            _ => (EventResult::Ignored, false),
        }
    }

    pub fn draw(surface: &mut Surface, rect: Rect, label: &str, state: ToggleState, theme: &Theme) {
        if rect.w < 36 || rect.h < 18 {
            return;
        }
        let sw_w = 38;
        let sw_h = 20;
        let sw_r = Rect::new(rect.x, rect.y + (rect.h - sw_h) / 2, sw_w, sw_h);
        let tstate = ThemeState::from_flags(state.hovered, state.pressed, state.disabled, state.on);
        UxTheme::draw_background(surface, ThemePart::ToggleSwitch, tstate, sw_r, theme);
        if !label.is_empty() {
            let tx = sw_r.right() + 8;
            let tr = Rect::new(tx, rect.y, (rect.right() - tx).max(0), rect.h);
            surface.push_clip(tr);
            with_default_font(|font| {
                let fg = if state.disabled { theme.text_secondary } else { theme.text_primary };
                let st = TextStyle::ui(fg);
                let (_, lh) = font.line_metrics(st);
                font.draw_text_ellipsis(surface, tx, rect.y + (rect.h - lh) / 2, tr.w, label, st);
            });
            surface.pop_clip();
        }
    }
}

/// Win32 etched `BS_GROUPBOX` frame that returns the inner usable content `Rect`.
pub struct GroupBox;

impl GroupBox {
    pub fn draw(surface: &mut Surface, rect: Rect, title: &str, bg_fill: Option<Color>, theme: &Theme) -> Rect {
        if rect.w < 16 || rect.h < 24 {
            return Rect::default();
        }
        if let Some(bg) = bg_fill {
            surface.fill_rect(rect, bg);
        }
        let border_r = Rect::new(rect.x, rect.y + 7, rect.w, rect.h - 7);
        UxTheme::draw_background(surface, ThemePart::GroupBox, ThemeState::Normal, border_r, theme);

        if !title.is_empty() {
            with_default_font(|font| {
                let st = TextStyle {
                    bold: true,
                    size_px: 13,
                    color: theme.text_link,
                    ..TextStyle::ui(theme.text_link)
                };
                let (tw, lh) = font.measure_text(title, st);
                let cap_w = (tw + 8).min(rect.w - 20).max(0);
                let cap_bg = bg_fill.unwrap_or(theme.face);
                surface.fill_rect(Rect::new(rect.x + 10, rect.y + 1, cap_w, lh.max(14)), cap_bg);
                surface.push_clip(Rect::new(rect.x + 14, rect.y, (cap_w - 8).max(0), 18));
                font.draw_text(surface, rect.x + 14, rect.y + 1, title, st);
                surface.pop_clip();
            });
        }

        Rect::new(rect.x + 10, rect.y + 20, (rect.w - 20).max(0), (rect.h - 28).max(0))
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CommandLinkState {
    pub hovered: bool,
    pub pressed: bool,
    pub focused: bool,
    pub disabled: bool,
}

/// ComCtl32 v6 `BS_COMMANDLINK` multi-line task button with leading chevron arrow.
pub struct CommandLink;

impl CommandLink {
    pub fn handle_event(state: &mut CommandLinkState, rect: Rect, event: GuiEvent) -> (EventResult, bool) {
        if state.disabled {
            return (EventResult::Ignored, false);
        }
        match event {
            GuiEvent::MouseMove { x, y } => {
                let h = rect.contains(x, y);
                if h != state.hovered {
                    state.hovered = h;
                    if !h {
                        state.pressed = false;
                    }
                    return (EventResult::NeedsRedraw, false);
                }
                (EventResult::Ignored, false)
            }
            GuiEvent::MouseDown { x, y, button: MouseButton::Left } => {
                let inside = rect.contains(x, y);
                state.focused = inside;
                if inside {
                    state.pressed = true;
                    return (EventResult::NeedsRedraw, false);
                }
                (EventResult::Ignored, false)
            }
            GuiEvent::MouseUp { x, y, button: MouseButton::Left } => {
                let was = state.pressed;
                state.pressed = false;
                if was {
                    return (EventResult::NeedsRedraw, rect.contains(x, y));
                }
                (EventResult::Ignored, false)
            }
            _ => (EventResult::Ignored, false),
        }
    }

    pub fn draw(
        surface: &mut Surface,
        rect: Rect,
        title: &str,
        note: &str,
        state: CommandLinkState,
        theme: &Theme,
    ) {
        if rect.w < 32 || rect.h < 28 {
            return;
        }
        let tstate = if state.disabled {
            ThemeState::Disabled
        } else if state.pressed {
            ThemeState::Pressed
        } else if state.hovered {
            ThemeState::Hot
        } else if state.focused {
            ThemeState::Focused
        } else {
            ThemeState::Normal
        };
        UxTheme::draw_background(surface, ThemePart::CommandLink, tstate, rect, theme);

        let icon_r = Rect::new(rect.x + 10, rect.y + 10, 18, 18);
        surface.fill_circle(icon_r.x + 9, icon_r.y + 9, 8, if state.hovered { theme.good } else { theme.frame_active });
        surface.draw_arrow(icon_r, ArrowDir::Right, theme.selection_text);

        let tx = icon_r.right() + 10;
        let tw = (rect.right() - tx - 8).max(0);
        surface.push_clip(Rect::new(tx, rect.y + 4, tw, (rect.h - 8).max(0)));
        with_default_font(|font| {
            let tstyle = TextStyle {
                bold: true,
                size_px: 14,
                color: if state.disabled { theme.text_secondary } else { theme.text_link },
                ..TextStyle::ui(theme.text_link)
            };
            font.draw_text_ellipsis(surface, tx, rect.y + 6, tw, title, tstyle);
            if !note.is_empty() && rect.h >= 38 {
                let nstyle = TextStyle::ui_sized(12, theme.text_secondary);
                font.draw_text_ellipsis(surface, tx, rect.y + 24, tw, note, nstyle);
            }
        });
        surface.pop_clip();
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SplitAction {
    None,
    Primary,
    DropDown,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SplitButtonState {
    pub hover_main: bool,
    pub hover_drop: bool,
    pub press_main: bool,
    pub press_drop: bool,
    pub disabled: bool,
}

/// ComCtl32 v6 `BS_SPLITBUTTON` combining a primary button and a side dropdown arrow.
pub struct SplitButton;

impl SplitButton {
    const DROP_W: i32 = 20;

    fn rects(rect: Rect) -> (Rect, Rect) {
        let drop_w = Self::DROP_W.min(rect.w / 3).max(14);
        let main_r = Rect::new(rect.x, rect.y, (rect.w - drop_w).max(0), rect.h);
        let drop_r = Rect::new(main_r.right(), rect.y, drop_w, rect.h);
        (main_r, drop_r)
    }

    pub fn handle_event(state: &mut SplitButtonState, rect: Rect, event: GuiEvent) -> (EventResult, SplitAction) {
        if state.disabled {
            return (EventResult::Ignored, SplitAction::None);
        }
        let (main_r, drop_r) = Self::rects(rect);
        match event {
            GuiEvent::MouseMove { x, y } => {
                let hm = main_r.contains(x, y);
                let hd = drop_r.contains(x, y);
                if hm != state.hover_main || hd != state.hover_drop {
                    state.hover_main = hm;
                    state.hover_drop = hd;
                    return (EventResult::NeedsRedraw, SplitAction::None);
                }
                (EventResult::Ignored, SplitAction::None)
            }
            GuiEvent::MouseDown { x, y, button: MouseButton::Left } => {
                if drop_r.contains(x, y) {
                    state.press_drop = true;
                    return (EventResult::NeedsRedraw, SplitAction::None);
                }
                if main_r.contains(x, y) {
                    state.press_main = true;
                    return (EventResult::NeedsRedraw, SplitAction::None);
                }
                (EventResult::Ignored, SplitAction::None)
            }
            GuiEvent::MouseUp { x, y, button: MouseButton::Left } => {
                let pm = state.press_main;
                let pd = state.press_drop;
                state.press_main = false;
                state.press_drop = false;
                if pd {
                    let act = if drop_r.contains(x, y) { SplitAction::DropDown } else { SplitAction::None };
                    return (EventResult::NeedsRedraw, act);
                }
                if pm {
                    let act = if main_r.contains(x, y) { SplitAction::Primary } else { SplitAction::None };
                    return (EventResult::NeedsRedraw, act);
                }
                (EventResult::Ignored, SplitAction::None)
            }
            _ => (EventResult::Ignored, SplitAction::None),
        }
    }

    pub fn draw(surface: &mut Surface, rect: Rect, label: &str, state: SplitButtonState, theme: &Theme) {
        if rect.w < 24 || rect.h < 16 {
            return;
        }
        let (main_r, drop_r) = Self::rects(rect);
        let tstate = ThemeState::from_flags(
            state.hover_main || state.hover_drop,
            state.press_main || state.press_drop,
            state.disabled,
            false,
        );
        UxTheme::draw_background(surface, ThemePart::PushButton, tstate, rect, theme);
        // Vertical separator between main label and drop zone
        surface.fill_rect(
            Rect::new(drop_r.x, rect.y + 4, 1, (rect.h - 8).max(1)),
            theme.border_shadow,
        );
        if state.hover_drop || state.press_drop {
            surface.fill_rect(drop_r.inset(Insets::all(2)), theme.accent.with_alpha(52));
        }
        surface.draw_arrow(drop_r, ArrowDir::Down, if state.disabled { theme.text_secondary } else { theme.text_primary });
        surface.push_clip(main_r.inset(Insets::hv(4, 2)));
        with_default_font(|font| {
            let fg = if state.disabled { theme.text_secondary } else { theme.text_primary };
            let st = TextStyle::ui(fg);
            let (tw, lh) = font.measure_text(label, st);
            let tx = main_r.x + ((main_r.w - tw) / 2).max(4);
            let ty = main_r.y + (main_r.h - lh) / 2;
            font.draw_text_ellipsis(surface, tx, ty, (main_r.w - 8).max(0), label, st);
        });
        surface.pop_clip();
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct LinkLabelState {
    pub hovered: bool,
    pub pressed: bool,
    pub visited: bool,
}

/// ComCtl32 `WC_LINK` (`SysLink`) hyperlinked action label.
pub struct LinkLabel;

impl LinkLabel {
    pub fn handle_event(state: &mut LinkLabelState, rect: Rect, event: GuiEvent) -> (EventResult, bool) {
        match event {
            GuiEvent::MouseMove { x, y } => {
                let h = rect.contains(x, y);
                if h != state.hovered {
                    state.hovered = h;
                    if !h {
                        state.pressed = false;
                    }
                    return (EventResult::NeedsRedraw, false);
                }
                (EventResult::Ignored, false)
            }
            GuiEvent::MouseDown { x, y, button: MouseButton::Left } => {
                if rect.contains(x, y) {
                    state.pressed = true;
                    return (EventResult::NeedsRedraw, false);
                }
                (EventResult::Ignored, false)
            }
            GuiEvent::MouseUp { x, y, button: MouseButton::Left } => {
                let was = state.pressed;
                state.pressed = false;
                if was && rect.contains(x, y) {
                    state.visited = true;
                    return (EventResult::NeedsRedraw, true);
                }
                if was {
                    return (EventResult::NeedsRedraw, false);
                }
                (EventResult::Ignored, false)
            }
            _ => (EventResult::Ignored, false),
        }
    }

    pub fn draw(surface: &mut Surface, rect: Rect, text: &str, state: LinkLabelState, theme: &Theme) {
        if rect.is_empty() {
            return;
        }
        let col = if state.pressed {
            theme.danger
        } else if state.hovered {
            theme.accent
        } else if state.visited {
            Color::lerp(theme.text_link, theme.accent, 64)
        } else {
            theme.text_link
        };
        surface.push_clip(rect);
        with_default_font(|font| {
            let st = TextStyle::ui(col);
            let (tw, lh) = font.measure_text(text, st);
            let ty = rect.y + (rect.h - lh) / 2;
            font.draw_text_ellipsis(surface, rect.x, ty, rect.w, text, st);
            let underline_w = tw.min(rect.w);
            if state.hovered || underline_w > 0 {
                surface.fill_rect(Rect::new(rect.x, ty + lh - 1, underline_w, 1), col);
            }
        });
        surface.pop_clip();
    }
}
