use crate::event::{EventResult, GuiEvent, MouseButton};
use crate::gfx::{ArrowDir, Rect, Surface, Theme, ThemePart, ThemeState, UxTheme};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScrollOrientation {
    Vertical,
    Horizontal,
}

#[derive(Clone, Copy, Debug)]
pub struct ScrollBarState {
    pub value: i32,
    pub max: i32,
    pub page: i32,
    pub dragging: bool,
    pub drag_offset: i32,
    pub hover_thumb: bool,
}

impl Default for ScrollBarState {
    fn default() -> Self {
        Self::new(0, 100, 20)
    }
}

impl ScrollBarState {
    pub const fn new(value: i32, max: i32, page: i32) -> Self {
        let m = if max < 0 { 0 } else { max };
        Self {
            value: if value > m { m } else if value < 0 { 0 } else { value },
            max: m,
            page: if page < 1 { 1 } else { page },
            dragging: false,
            drag_offset: 0,
            hover_thumb: false,
        }
    }

    /// Update scrollbar `(value, max, page)` while preserving active drag/hover state unless range collapses.
    pub fn sync_range(&mut self, value: i32, max: i32, page: i32) {
        self.max = max.max(0);
        self.page = page.max(1);
        self.value = value.clamp(0, self.max);
        if self.max == 0 {
            self.dragging = false;
        }
    }
}

/// Standalone Win32 `SCROLLBAR` (`SBS_VERT` / `SBS_HORZ`) with stepper buttons & proportional thumb.
pub struct ScrollBar;

impl ScrollBar {
    const BTN: i32 = 16;

    fn geometry(bounds: Rect, orient: ScrollOrientation, state: ScrollBarState) -> (Rect, Rect, Rect, Rect) {
        match orient {
            ScrollOrientation::Vertical => {
                let b = Self::BTN.min(bounds.h / 3).max(8);
                let up_r = Rect::new(bounds.x, bounds.y, bounds.w, b);
                let dn_r = Rect::new(bounds.x, bounds.bottom() - b, bounds.w, b);
                let track = Rect::new(bounds.x, bounds.y + b, bounds.w, (bounds.h - b * 2).max(1));
                let total = (state.max + state.page).max(1);
                let thumb_h = ((track.h as i64 * state.page as i64) / total as i64).clamp(18, track.h as i64) as i32;
                let travel = (track.h - thumb_h).max(1);
                let ty = track.y + ((state.value as i64 * travel as i64) / state.max.max(1) as i64) as i32;
                let thumb = Rect::new(track.x + 2, ty, (track.w - 4).max(4), thumb_h);
                (up_r, dn_r, track, thumb)
            }
            ScrollOrientation::Horizontal => {
                let b = Self::BTN.min(bounds.w / 3).max(8);
                let lf_r = Rect::new(bounds.x, bounds.y, b, bounds.h);
                let rt_r = Rect::new(bounds.right() - b, bounds.y, b, bounds.h);
                let track = Rect::new(bounds.x + b, bounds.y, (bounds.w - b * 2).max(1), bounds.h);
                let total = (state.max + state.page).max(1);
                let thumb_w = ((track.w as i64 * state.page as i64) / total as i64).clamp(18, track.w as i64) as i32;
                let travel = (track.w - thumb_w).max(1);
                let tx = track.x + ((state.value as i64 * travel as i64) / state.max.max(1) as i64) as i32;
                let thumb = Rect::new(tx, track.y + 2, thumb_w, (track.h - 4).max(4));
                (lf_r, rt_r, track, thumb)
            }
        }
    }

    pub fn handle_event(
        state: &mut ScrollBarState,
        bounds: Rect,
        orient: ScrollOrientation,
        event: GuiEvent,
    ) -> (EventResult, bool) {
        if state.max <= 0 {
            state.dragging = false;
            return (EventResult::Ignored, false);
        }
        let (b1, b2, track, thumb) = Self::geometry(bounds, orient, *state);
        let old = state.value;
        match event {
            GuiEvent::MouseMove { x, y } => {
                if state.dragging {
                    match orient {
                        ScrollOrientation::Vertical => {
                            let travel = (track.h - thumb.h).max(1);
                            let rel = (y - state.drag_offset - track.y).clamp(0, travel);
                            state.value = ((rel as i64 * state.max as i64) / travel as i64) as i32;
                        }
                        ScrollOrientation::Horizontal => {
                            let travel = (track.w - thumb.w).max(1);
                            let rel = (x - state.drag_offset - track.x).clamp(0, travel);
                            state.value = ((rel as i64 * state.max as i64) / travel as i64) as i32;
                        }
                    }
                    return (EventResult::NeedsRedraw, state.value != old);
                }
                let ht = thumb.contains(x, y);
                if ht != state.hover_thumb {
                    state.hover_thumb = ht;
                    return (EventResult::NeedsRedraw, false);
                }
                (EventResult::Ignored, false)
            }
            GuiEvent::MouseDown { x, y, button: MouseButton::Left } => {
                if b1.contains(x, y) {
                    state.value = (state.value - 1).clamp(0, state.max);
                    return (EventResult::NeedsRedraw, state.value != old);
                }
                if b2.contains(x, y) {
                    state.value = (state.value + 1).clamp(0, state.max);
                    return (EventResult::NeedsRedraw, state.value != old);
                }
                if thumb.contains(x, y) {
                    state.dragging = true;
                    state.drag_offset = match orient {
                        ScrollOrientation::Vertical => y - thumb.y,
                        ScrollOrientation::Horizontal => x - thumb.x,
                    };
                    return (EventResult::NeedsRedraw, false);
                }
                if track.contains(x, y) {
                    let forward = match orient {
                        ScrollOrientation::Vertical => y >= thumb.bottom(),
                        ScrollOrientation::Horizontal => x >= thumb.right(),
                    };
                    state.value = (state.value + if forward { state.page } else { -state.page }).clamp(0, state.max);
                    return (EventResult::NeedsRedraw, state.value != old);
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
            GuiEvent::MouseWheel { x, y, delta_y } if bounds.contains(x, y) => {
                let step = 3.min(state.page).max(1);
                state.value = (state.value - delta_y * step).clamp(0, state.max);
                (EventResult::NeedsRedraw, state.value != old)
            }
            _ => (EventResult::Ignored, false),
        }
    }

    pub fn draw(surface: &mut Surface, bounds: Rect, orient: ScrollOrientation, state: ScrollBarState, theme: &Theme) {
        if bounds.w < 10 || bounds.h < 10 {
            return;
        }
        let (b1, b2, track, thumb) = Self::geometry(bounds, orient, state);
        UxTheme::draw_background(surface, ThemePart::ScrollTrack, ThemeState::Normal, track, theme);
        UxTheme::draw_background(surface, ThemePart::ScrollButton, ThemeState::Normal, b1, theme);
        UxTheme::draw_background(surface, ThemePart::ScrollButton, ThemeState::Normal, b2, theme);
        match orient {
            ScrollOrientation::Vertical => {
                surface.fill_rect(b1, theme.face);
                surface.draw_rect(b1, theme.ctrl_border);
                surface.draw_arrow(b1, ArrowDir::Up, theme.text_primary);
                surface.fill_rect(b2, theme.face);
                surface.draw_rect(b2, theme.ctrl_border);
                surface.draw_arrow(b2, ArrowDir::Down, theme.text_primary);
            }
            ScrollOrientation::Horizontal => {
                surface.fill_rect(b1, theme.face);
                surface.draw_rect(b1, theme.ctrl_border);
                surface.draw_arrow(b1, ArrowDir::Left, theme.text_primary);
                surface.fill_rect(b2, theme.face);
                surface.draw_rect(b2, theme.ctrl_border);
                surface.draw_arrow(b2, ArrowDir::Right, theme.text_primary);
            }
        }
        if state.max > 0 {
            let tst = if state.dragging {
                ThemeState::Pressed
            } else if state.hover_thumb {
                ThemeState::Hot
            } else {
                ThemeState::Normal
            };
            UxTheme::draw_background(surface, ThemePart::ScrollThumb, tst, thumb, theme);
        }
    }
}
