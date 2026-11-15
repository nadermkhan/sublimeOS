use crate::event::{EventResult, GuiEvent, MouseButton};
use crate::font::{with_default_font, TextStyle};
use crate::gfx::{Color, Rect, Surface, Theme};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ButtonStyle {
    Normal,
    Primary,
    Ghost,
    Subtle,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ButtonState {
    pub hovered: bool,
    pub pressed: bool,
    pub disabled: bool,
}

pub struct Button;

impl Button {
    pub fn handle_event(state: &mut ButtonState, rect: Rect, event: GuiEvent) -> (EventResult, bool) {
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
                if rect.contains(x, y) {
                    state.pressed = true;
                    state.hovered = true;
                    return (EventResult::NeedsRedraw, false);
                }
                (EventResult::Ignored, false)
            }
            GuiEvent::MouseUp { x, y, button: MouseButton::Left } => {
                let was_pressed = state.pressed;
                state.pressed = false;
                let inside = rect.contains(x, y);
                if was_pressed {
                    return (EventResult::NeedsRedraw, inside);
                }
                (EventResult::Ignored, false)
            }
            _ => (EventResult::Ignored, false),
        }
    }

    pub fn draw(
        surface: &mut Surface,
        rect: Rect,
        label: &str,
        style: ButtonStyle,
        state: ButtonState,
        theme: &Theme,
    ) {
        if rect.w < 4 || rect.h < 4 {
            return;
        }
        let (bg, border, fg) = match style {
            ButtonStyle::Primary => {
                if state.pressed {
                    (theme.frame_outer, theme.frame_outer, theme.selection_text)
                } else if state.hovered {
                    (Color::lerp(theme.accent, Color::WHITE, 32), theme.accent, theme.selection_text)
                } else {
                    (theme.accent, theme.frame_outer, theme.selection_text)
                }
            }
            ButtonStyle::Normal => {
                if state.disabled {
                    (theme.face_dark, theme.border_shadow, theme.text_secondary)
                } else if state.pressed {
                    (Color::lerp(theme.face_light, theme.accent, 64), theme.frame_outer, theme.text_primary)
                } else if state.hovered {
                    (Color::lerp(theme.face_light, theme.accent, 30), theme.accent, theme.text_primary)
                } else {
                    (theme.face_dark, theme.ctrl_border, theme.text_primary)
                }
            }
            ButtonStyle::Subtle => {
                if state.pressed {
                    (Color::lerp(theme.face, theme.accent, 52), theme.accent, theme.text_primary)
                } else if state.hovered {
                    (Color::lerp(theme.face_light, theme.accent, 24), theme.accent, theme.text_primary)
                } else {
                    (theme.face, theme.border_shadow, theme.text_primary)
                }
            }
            ButtonStyle::Ghost => {
                let bg = if state.pressed {
                    theme.selection.with_alpha(96)
                } else if state.hovered {
                    theme.selection.with_alpha(48)
                } else {
                    Color::TRANSPARENT
                };
                if bg.a() > 0 {
                    surface.fill_rect(rect, bg);
                }
                Self::draw_centered_label(surface, rect, label, theme.text_primary);
                return;
            }
        };

        surface.fill_rect(rect, bg);
        surface.draw_rect(rect, border);
        Self::draw_centered_label(surface, rect, label, fg);
    }

    fn draw_centered_label(surface: &mut Surface, rect: Rect, label: &str, fg: Color) {
        surface.push_clip(rect);
        with_default_font(|font| {
            let st = TextStyle::ui(fg);
            let (tw, lh) = font.measure_text(label, st);
            let tx = rect.x + ((rect.w - tw).max(4) / 2);
            let ty = rect.y + ((rect.h - lh) / 2);
            font.draw_text_ellipsis(surface, tx, ty, (rect.w - 8).max(4), label, st);
        });
        surface.pop_clip();
    }
}
