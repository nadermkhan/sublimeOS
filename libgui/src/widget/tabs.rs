use crate::event::{EventResult, GuiEvent, MouseButton};
use crate::font::{with_default_font, TextStyle};
use crate::gfx::{Color, Rect, Surface, Theme};

#[derive(Clone, Copy, Debug)]
pub struct TabBarState {
    pub active: usize,
    pub hovered: Option<usize>,
}

impl TabBarState {
    pub const fn new(active: usize) -> Self {
        Self {
            active,
            hovered: None,
        }
    }
}

pub struct TabBar;

impl TabBar {
    fn tab_rect(bounds: Rect, count: usize, idx: usize) -> Rect {
        if count == 0 || bounds.w <= 0 {
            return Rect::default();
        }
        let tab_w = (bounds.w / count as i32).clamp(48, 160);
        let x = bounds.x + idx as i32 * tab_w;
        let rem = (bounds.right() - x).max(0);
        Rect::new(x, bounds.y, tab_w.min(rem), bounds.h)
    }

    pub fn handle_event(
        state: &mut TabBarState,
        bounds: Rect,
        labels: &[&str],
        event: GuiEvent,
    ) -> (EventResult, Option<usize>) {
        match event {
            GuiEvent::MouseMove { x, y } => {
                let mut next = None;
                if bounds.contains(x, y) {
                    for i in 0..labels.len() {
                        if Self::tab_rect(bounds, labels.len(), i).contains(x, y) {
                            next = Some(i);
                            break;
                        }
                    }
                }
                if next != state.hovered {
                    state.hovered = next;
                    return (EventResult::NeedsRedraw, None);
                }
                (EventResult::Ignored, None)
            }
            GuiEvent::MouseDown { x, y, button: MouseButton::Left } => {
                if bounds.contains(x, y) {
                    for i in 0..labels.len() {
                        if Self::tab_rect(bounds, labels.len(), i).contains(x, y) {
                            if state.active != i {
                                state.active = i;
                                return (EventResult::NeedsRedraw, Some(i));
                            }
                            return (EventResult::Handled, None);
                        }
                    }
                }
                (EventResult::Ignored, None)
            }
            _ => (EventResult::Ignored, None),
        }
    }

    pub fn draw(
        surface: &mut Surface,
        bounds: Rect,
        labels: &[&str],
        state: TabBarState,
        theme: &Theme,
    ) {
        if bounds.is_empty() || labels.is_empty() {
            return;
        }
        surface.fill_rect(Rect::new(bounds.x, bounds.bottom() - 1, bounds.w, 1), theme.ctrl_border);
        for (i, &label) in labels.iter().enumerate() {
            let r = Self::tab_rect(bounds, labels.len(), i);
            if r.w < 12 {
                continue;
            }
            let is_active = i == state.active;
            let is_hover = state.hovered == Some(i);
            let tr = if is_active { r } else { Rect::new(r.x, r.y + 2, r.w, (r.h - 2).max(1)) };
            let bg = if is_active {
                theme.face_light
            } else if is_hover {
                Color::lerp(theme.face, theme.accent, 20)
            } else {
                theme.face
            };
            surface.fill_rect(Rect::new(tr.x + 1, tr.y + 1, tr.w - 2, tr.h - 1), bg);
            surface.fill_rect(Rect::new(tr.x, tr.y, tr.w, 1), if is_active { theme.accent } else { theme.border_shadow });
            surface.fill_rect(Rect::new(tr.x, tr.y, 1, tr.h), if is_active { theme.accent } else { theme.border_shadow });
            surface.fill_rect(Rect::new(tr.right() - 1, tr.y, 1, tr.h), if is_active { theme.accent } else { theme.border_shadow });
            if is_active {
                surface.fill_rect(Rect::new(tr.x + 1, tr.y + 1, tr.w - 2, 2), theme.accent);
                surface.fill_rect(Rect::new(tr.x + 1, tr.bottom() - 1, tr.w - 2, 1), theme.face_light);
            }
            surface.push_clip(tr);
            with_default_font(|font| {
                let st = TextStyle {
                    bold: is_active,
                    color: if is_active { theme.text_primary } else { theme.text_secondary },
                    ..TextStyle::ui(theme.text_primary)
                };
                let (tw, lh) = font.measure_text(label, st);
                let tx = tr.x + ((tr.w - tw).max(4) / 2);
                let ty = tr.y + ((tr.h - lh) / 2);
                font.draw_text_ellipsis(surface, tx, ty, (tr.w - 8).max(4), label, st);
            });
            surface.pop_clip();
        }
    }
}
