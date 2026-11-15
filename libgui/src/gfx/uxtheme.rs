use crate::gfx::{Color, Rect, Surface, Theme};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArrowDir {
    Up,
    Down,
    Left,
    Right,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThemePart {
    PushButton,
    DefaultButton,
    CommandLink,
    CheckBox,
    RadioButton,
    ToggleSwitch,
    GroupBox,
    Edit,
    ComboBox,
    ComboDropButton,
    ScrollTrack,
    ScrollThumb,
    ScrollButton,
    SpinUp,
    SpinDown,
    HeaderSection,
    StatusPane,
    ToolBarButton,
    ToolTip,
    TreeGlyph,
    AccordionHeader,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThemeState {
    Normal,
    Hot,
    Pressed,
    Disabled,
    Focused,
    Checked,
    CheckedHot,
    CheckedPressed,
    Mixed,
}

impl ThemeState {
    #[inline]
    pub fn from_flags(hovered: bool, pressed: bool, disabled: bool, checked: bool) -> Self {
        if disabled {
            ThemeState::Disabled
        } else if checked {
            if pressed {
                ThemeState::CheckedPressed
            } else if hovered {
                ThemeState::CheckedHot
            } else {
                ThemeState::Checked
            }
        } else if pressed {
            ThemeState::Pressed
        } else if hovered {
            ThemeState::Hot
        } else {
            ThemeState::Normal
        }
    }
}

/// UxTheme-compatible part/state visual style renderer bound to the active `Theme`.
pub struct UxTheme;

impl UxTheme {
    pub fn draw_background(
        surface: &mut Surface,
        part: ThemePart,
        state: ThemeState,
        rect: Rect,
        theme: &Theme,
    ) {
        if rect.w < 2 || rect.h < 2 {
            return;
        }
        match part {
            ThemePart::PushButton | ThemePart::DefaultButton => {
                let is_def = matches!(part, ThemePart::DefaultButton);
                let (bg, border) = match state {
                    ThemeState::Disabled => (theme.face_dark, theme.border_shadow),
                    ThemeState::Pressed | ThemeState::CheckedPressed => (
                        Color::lerp(theme.face_light, theme.accent, 62),
                        theme.frame_outer,
                    ),
                    ThemeState::Hot | ThemeState::CheckedHot => (
                        Color::lerp(theme.face_light, theme.accent, 30),
                        theme.accent,
                    ),
                    ThemeState::Checked => (
                        Color::lerp(theme.face_light, theme.accent, 42),
                        theme.accent,
                    ),
                    _ if is_def => (theme.face_light, theme.accent),
                    _ => (theme.face_dark, theme.ctrl_border),
                };
                surface.fill_rect(rect, bg);
                surface.draw_rect(rect, border);
                if is_def && rect.w > 4 && rect.h > 4 {
                    surface.draw_rect(
                        Rect::new(rect.x + 1, rect.y + 1, rect.w - 2, rect.h - 2),
                        border,
                    );
                }
                if matches!(state, ThemeState::Focused) {
                    Self::draw_focus_rect(
                        surface,
                        Rect::new(rect.x + 3, rect.y + 3, (rect.w - 6).max(0), (rect.h - 6).max(0)),
                        theme,
                    );
                }
            }
            ThemePart::CommandLink => {
                let (bg, border) = match state {
                    ThemeState::Pressed => (
                        Color::lerp(theme.face_light, theme.selection, 56),
                        theme.accent,
                    ),
                    ThemeState::Hot => (
                        Color::lerp(theme.face_light, theme.selection, 24),
                        theme.accent,
                    ),
                    ThemeState::Focused => (theme.face_light, theme.ctrl_border),
                    _ => (theme.face_light, Color::TRANSPARENT),
                };
                surface.fill_rect(rect, bg);
                if border.a() > 0 {
                    surface.draw_rect(rect, border);
                }
            }
            ThemePart::CheckBox => {
                let bg = match state {
                    ThemeState::Disabled => theme.face,
                    ThemeState::Pressed | ThemeState::CheckedPressed => theme.face_dark,
                    ThemeState::Hot | ThemeState::CheckedHot => Color::lerp(theme.input_bg, theme.accent, 22),
                    _ => theme.input_bg,
                };
                let border = match state {
                    ThemeState::Disabled => theme.border_shadow,
                    ThemeState::Hot | ThemeState::CheckedHot => theme.accent,
                    ThemeState::Checked | ThemeState::CheckedPressed | ThemeState::Mixed => theme.frame_active,
                    _ => theme.ctrl_border,
                };
                surface.fill_rect(rect, bg);
                surface.draw_rect(rect, border);
                if matches!(
                    state,
                    ThemeState::Checked | ThemeState::CheckedHot | ThemeState::CheckedPressed
                ) {
                    surface.draw_checkmark(rect, theme.good);
                } else if matches!(state, ThemeState::Mixed) {
                    let bx = Rect::new(rect.x + 3, rect.y + 3, (rect.w - 6).max(2), (rect.h - 6).max(2));
                    surface.fill_rect(bx, theme.frame_active);
                }
            }
            ThemePart::RadioButton => {
                let cx = rect.x + rect.w / 2;
                let cy = rect.y + rect.h / 2;
                let r = (rect.w.min(rect.h) / 2).max(4);
                let bg = match state {
                    ThemeState::Disabled => theme.face,
                    ThemeState::Pressed | ThemeState::CheckedPressed => theme.face_dark,
                    ThemeState::Hot | ThemeState::CheckedHot => Color::lerp(theme.input_bg, theme.accent, 22),
                    _ => theme.input_bg,
                };
                let border = match state {
                    ThemeState::Disabled => theme.border_shadow,
                    ThemeState::Hot | ThemeState::CheckedHot => theme.accent,
                    ThemeState::Checked | ThemeState::CheckedPressed => theme.frame_active,
                    _ => theme.ctrl_border,
                };
                surface.fill_circle(cx, cy, r, bg);
                surface.draw_circle(cx, cy, r, border);
                if matches!(
                    state,
                    ThemeState::Checked | ThemeState::CheckedHot | ThemeState::CheckedPressed
                ) {
                    surface.fill_circle(cx, cy, (r - 3).max(2), theme.good);
                }
            }
            ThemePart::ToggleSwitch => {
                // Authentic Windows 8.1 rectangular bar toggle switch
                let on = matches!(
                    state,
                    ThemeState::Checked | ThemeState::CheckedHot | ThemeState::CheckedPressed
                );
                let track_bg = if matches!(state, ThemeState::Disabled) {
                    theme.face_dark
                } else if on {
                    theme.accent
                } else if matches!(state, ThemeState::Hot) {
                    Color::lerp(theme.face_dark, theme.ctrl_border, 45)
                } else {
                    Color::lerp(theme.face_dark, theme.ctrl_border, 25)
                };
                surface.fill_rect(rect, theme.face_light);
                surface.draw_rect(rect, if on { theme.accent } else { theme.ctrl_border });
                let inner_track = Rect::new(
                    rect.x + 2,
                    rect.y + 2,
                    (rect.w - 4).max(2),
                    (rect.h - 4).max(2),
                );
                surface.fill_rect(inner_track, track_bg);
                let knob_w = (rect.h / 2).max(9);
                let kx = if on {
                    rect.right() - knob_w
                } else {
                    rect.x
                };
                let knob = Rect::new(kx, rect.y, knob_w, rect.h);
                surface.fill_rect(knob, theme.text_primary);
            }
            ThemePart::GroupBox => {
                surface.draw_etched_rect(rect, theme.border_shadow, theme.face_light);
            }
            ThemePart::Edit | ThemePart::ComboBox => {
                let bg = if matches!(state, ThemeState::Disabled) {
                    theme.face
                } else {
                    theme.input_bg
                };
                let border = match state {
                    ThemeState::Focused | ThemeState::Pressed => theme.frame_active,
                    ThemeState::Hot => theme.accent,
                    ThemeState::Disabled => theme.border_shadow,
                    _ => theme.input_border,
                };
                surface.fill_rect(rect, bg);
                surface.draw_rect(rect, border);
            }
            ThemePart::ComboDropButton
            | ThemePart::ScrollButton
            | ThemePart::SpinUp
            | ThemePart::SpinDown => {
                let (top, bot, border) = match state {
                    ThemeState::Pressed => (theme.face_dark, theme.face_light, theme.frame_active),
                    ThemeState::Hot => (
                        Color::lerp(theme.face_light, theme.selection, 24),
                        Color::lerp(theme.face, theme.selection, 32),
                        theme.frame_active,
                    ),
                    ThemeState::Disabled => (theme.face, theme.face, theme.border_shadow),
                    _ => (
                        Color::lerp(theme.face_light, theme.selection, 12),
                        theme.face_dark,
                        theme.ctrl_border,
                    ),
                };
                surface.draw_gradient_v(rect, top, bot);
                surface.draw_rect(rect, border);
                let dir = match part {
                    ThemePart::SpinUp => ArrowDir::Up,
                    _ => ArrowDir::Down,
                };
                surface.draw_arrow(rect, dir, if matches!(state, ThemeState::Disabled) { theme.text_secondary } else { theme.text_primary });
            }
            ThemePart::ScrollTrack => {
                surface.fill_rect(rect, theme.face);
                surface.draw_rect(rect, theme.border_shadow);
            }
            ThemePart::ScrollThumb => {
                let fill = match state {
                    ThemeState::Pressed => theme.accent,
                    ThemeState::Hot => Color::lerp(theme.ctrl_border, theme.accent, 64),
                    _ => theme.ctrl_border,
                };
                surface.fill_rect(rect, fill);
            }
            ThemePart::HeaderSection => {
                let bg = match state {
                    ThemeState::Pressed => Color::lerp(theme.face_light, theme.accent, 36),
                    ThemeState::Hot => Color::lerp(theme.face_light, theme.accent, 18),
                    _ => theme.face_light,
                };
                surface.fill_rect(rect, bg);
                surface.fill_rect(Rect::new(rect.x, rect.bottom() - 1, rect.w, 1), theme.border_shadow);
                surface.fill_rect(Rect::new(rect.right() - 1, rect.y + 2, 1, (rect.h - 4).max(1)), theme.border_shadow);
            }
            ThemePart::StatusPane => {
                surface.draw_rect(rect, theme.border_shadow);
            }
            ThemePart::ToolBarButton => {
                if matches!(
                    state,
                    ThemeState::Hot | ThemeState::Pressed | ThemeState::Checked | ThemeState::CheckedHot | ThemeState::CheckedPressed
                ) {
                    let (bg, border) = match state {
                        ThemeState::Pressed | ThemeState::CheckedPressed => (
                            Color::lerp(theme.face_light, theme.selection, 56),
                            theme.accent,
                        ),
                        ThemeState::Checked | ThemeState::CheckedHot => (
                            Color::lerp(theme.face_light, theme.selection, 36),
                            theme.accent,
                        ),
                        _ => (
                            Color::lerp(theme.face_light, theme.selection, 22),
                            theme.accent,
                        ),
                    };
                    surface.fill_rect(rect, bg);
                    surface.draw_rect(rect, border);
                }
            }
            ThemePart::ToolTip => {
                surface.fill_rect(rect, theme.face_light);
                surface.draw_rect(rect, theme.ctrl_border);
            }
            ThemePart::TreeGlyph => {
                surface.fill_rect(rect, theme.input_bg);
                surface.draw_rect(rect, theme.border_shadow);
                let cy = rect.y + rect.h / 2;
                let cx = rect.x + rect.w / 2;
                surface.fill_rect(Rect::new(rect.x + 2, cy, (rect.w - 4).max(1), 1), theme.text_primary);
                if !matches!(state, ThemeState::Checked) {
                    surface.fill_rect(Rect::new(cx, rect.y + 2, 1, (rect.h - 4).max(1)), theme.text_primary);
                }
            }
            ThemePart::AccordionHeader => {
                let bg = match state {
                    ThemeState::Hot => Color::lerp(theme.face_light, theme.accent, 28),
                    _ => theme.face_dark,
                };
                surface.fill_rect(rect, bg);
                surface.draw_rect(rect, theme.ctrl_border);
            }
        }
    }

    /// Draw a classic dotted Win32 focus rectangle.
    pub fn draw_focus_rect(surface: &mut Surface, rect: Rect, theme: &Theme) {
        if rect.w < 4 || rect.h < 4 {
            return;
        }
        let col = theme.text_secondary;
        let mut x = rect.x;
        while x < rect.right() {
            surface.set_pixel(x, rect.y, col);
            surface.set_pixel(x, rect.bottom() - 1, col);
            x += 2;
        }
        let mut y = rect.y;
        while y < rect.bottom() {
            surface.set_pixel(rect.x, y, col);
            surface.set_pixel(rect.right() - 1, y, col);
            y += 2;
        }
    }
}
