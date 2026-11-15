use crate::event::{EventResult, GuiEvent, MouseButton};
use crate::gfx::{Rect, Surface, Theme};

#[derive(Clone, Copy, Debug)]
pub struct SliderState {
    pub value: i32,
    pub min: i32,
    pub max: i32,
    pub dragging: bool,
}

impl SliderState {
    pub const fn new(value: i32, min: i32, max: i32) -> Self {
        Self {
            value,
            min,
            max: if max <= min { min + 1 } else { max },
            dragging: false,
        }
    }
}

pub struct Slider;

impl Slider {
    fn value_from_x(bounds: Rect, min: i32, max: i32, x: i32) -> i32 {
        let track_w = (bounds.w - 14).max(1);
        let rel = (x - (bounds.x + 7)).clamp(0, track_w);
        min + ((rel as i64 * (max - min) as i64) / track_w as i64) as i32
    }

    /// Returns `(EventResult, value_changed)`.
    pub fn handle_event(state: &mut SliderState, bounds: Rect, event: GuiEvent) -> (EventResult, bool) {
        match event {
            GuiEvent::MouseDown { x, y, button: MouseButton::Left } => {
                if bounds.contains(x, y) {
                    state.dragging = true;
                    let nv = Self::value_from_x(bounds, state.min, state.max, x);
                    let ch = nv != state.value;
                    state.value = nv;
                    return (EventResult::NeedsRedraw, ch);
                }
                (EventResult::Ignored, false)
            }
            GuiEvent::MouseMove { x, .. } => {
                if state.dragging {
                    let nv = Self::value_from_x(bounds, state.min, state.max, x);
                    let ch = nv != state.value;
                    state.value = nv;
                    return (EventResult::NeedsRedraw, ch);
                }
                (EventResult::Ignored, false)
            }
            GuiEvent::MouseUp { button: MouseButton::Left, .. } => {
                if state.dragging {
                    state.dragging = false;
                    return (EventResult::NeedsRedraw, false);
                }
                (EventResult::Ignored, false)
            }
            _ => (EventResult::Ignored, false),
        }
    }

    pub fn draw(surface: &mut Surface, bounds: Rect, state: SliderState, theme: &Theme) {
        if bounds.w < 24 || bounds.h < 12 {
            return;
        }
        let cy = bounds.y + bounds.h / 2;
        let track_w = (bounds.w - 14).max(1);
        let track = Rect::new(bounds.x + 7, cy - 2, track_w, 4);
        surface.fill_rect(track, theme.face_dark);
        surface.draw_rect(track, theme.ctrl_border);

        let span = (state.max - state.min).max(1);
        let pos = ((state.value.clamp(state.min, state.max) - state.min) as i64 * track_w as i64 / span as i64) as i32;
        if pos > 0 {
            surface.fill_rect(Rect::new(track.x, track.y, pos, track.h), theme.accent);
        }

        let thumb = Rect::new(track.x + pos - 6, cy - 8, 12, 16);
        surface.fill_rounded_rect(thumb, 3, if state.dragging { theme.accent } else { theme.face_light });
        surface.draw_rounded_rect(thumb, 3, theme.frame_active);
    }
}
