use crate::event::{EventResult, GuiEvent, KeyCode, MouseButton};
use crate::font::{with_default_font, TextStyle};
use crate::gfx::{Color, Rect, Surface, Theme};

#[derive(Clone, Copy, Debug)]
pub struct ListViewState {
    pub scroll_y: i32,
    pub selected: Option<usize>,
    pub row_height: i32,
    pub dragging_thumb: bool,
    drag_offset: i32,
}

impl Default for ListViewState {
    fn default() -> Self {
        Self::new(22)
    }
}

impl ListViewState {
    pub const fn new(row_height: i32) -> Self {
        Self {
            scroll_y: 0,
            selected: None,
            row_height: if row_height < 14 { 22 } else { row_height },
            dragging_thumb: false,
            drag_offset: 0,
        }
    }

    pub fn clamp_scroll(&mut self, item_count: usize, view_h: i32) {
        let total_h = (item_count as i32) * self.row_height;
        let max_scroll = (total_h - view_h).max(0);
        self.scroll_y = self.scroll_y.clamp(0, max_scroll);
    }
}

pub struct ListView;

impl ListView {
    pub const SCROLLBAR_W: i32 = 14;

    /// Returns `(EventResult, clicked_or_activated_row)`.
    pub fn handle_event(
        state: &mut ListViewState,
        bounds: Rect,
        item_count: usize,
        event: GuiEvent,
    ) -> (EventResult, Option<usize>) {
        let inner = bounds.inset(crate::gfx::Insets::all(1));
        let total_h = (item_count as i32) * state.row_height;
        let needs_sb = total_h > inner.h;
        let max_scroll = (total_h - inner.h).max(0);
        state.clamp_scroll(item_count, inner.h);

        let sb_rect = Rect::new(inner.right() - Self::SCROLLBAR_W, inner.y, Self::SCROLLBAR_W, inner.h);
        let content_rect = if needs_sb {
            Rect::new(inner.x, inner.y, (inner.w - Self::SCROLLBAR_W).max(0), inner.h)
        } else {
            inner
        };

        match event {
            GuiEvent::MouseWheel { x, y, delta_y } => {
                if bounds.contains(x, y) && max_scroll > 0 {
                    let prev = state.scroll_y;
                    state.scroll_y = (state.scroll_y - delta_y * state.row_height * 2).clamp(0, max_scroll);
                    return (
                        if state.scroll_y != prev {
                            EventResult::NeedsRedraw
                        } else {
                            EventResult::Handled
                        },
                        None,
                    );
                }
                (EventResult::Ignored, None)
            }
            GuiEvent::MouseDown { x, y, button: MouseButton::Left } => {
                if needs_sb && sb_rect.contains(x, y) {
                    let thumb_h = ((inner.h as i64 * inner.h as i64) / total_h.max(1) as i64)
                        .clamp(24, inner.h as i64) as i32;
                    let thumb_travel = (inner.h - thumb_h).max(1);
                    let thumb_y = inner.y + ((state.scroll_y as i64 * thumb_travel as i64) / max_scroll.max(1) as i64) as i32;
                    if y >= thumb_y && y < thumb_y + thumb_h {
                        state.dragging_thumb = true;
                        state.drag_offset = y - thumb_y;
                    } else {
                        let rel = (y - inner.y - thumb_h / 2).clamp(0, thumb_travel);
                        state.scroll_y = ((rel as i64 * max_scroll as i64) / thumb_travel as i64) as i32;
                    }
                    return (EventResult::NeedsRedraw, None);
                }
                if content_rect.contains(x, y) {
                    let idx = ((y - content_rect.y + state.scroll_y) / state.row_height) as usize;
                    if idx < item_count {
                        state.selected = Some(idx);
                        return (EventResult::NeedsRedraw, Some(idx));
                    }
                }
                (EventResult::Ignored, None)
            }
            GuiEvent::MouseMove { y, .. } => {
                if state.dragging_thumb && max_scroll > 0 {
                    let thumb_h = ((inner.h as i64 * inner.h as i64) / total_h.max(1) as i64)
                        .clamp(24, inner.h as i64) as i32;
                    let thumb_travel = (inner.h - thumb_h).max(1);
                    let rel = (y - inner.y - state.drag_offset).clamp(0, thumb_travel);
                    state.scroll_y = ((rel as i64 * max_scroll as i64) / thumb_travel as i64) as i32;
                    return (EventResult::NeedsRedraw, None);
                }
                (EventResult::Ignored, None)
            }
            GuiEvent::MouseUp { button: MouseButton::Left, .. } => {
                if state.dragging_thumb {
                    state.dragging_thumb = false;
                    return (EventResult::NeedsRedraw, None);
                }
                (EventResult::Ignored, None)
            }
            GuiEvent::KeyDown { key, .. } => {
                if item_count == 0 {
                    return (EventResult::Ignored, None);
                }
                match key {
                    KeyCode::Up => {
                        let next = state.selected.map_or(0, |s| s.saturating_sub(1));
                        state.selected = Some(next);
                        let row_top = next as i32 * state.row_height;
                        if row_top < state.scroll_y {
                            state.scroll_y = row_top;
                        }
                        (EventResult::NeedsRedraw, None)
                    }
                    KeyCode::Down => {
                        let next = state.selected.map_or(0, |s| (s + 1).min(item_count - 1));
                        state.selected = Some(next);
                        let row_bot = (next as i32 + 1) * state.row_height;
                        if row_bot > state.scroll_y + inner.h {
                            state.scroll_y = (row_bot - inner.h).min(max_scroll);
                        }
                        (EventResult::NeedsRedraw, None)
                    }
                    _ => (EventResult::Ignored, None),
                }
            }
            _ => (EventResult::Ignored, None),
        }
    }

    pub fn draw<F>(
        surface: &mut Surface,
        bounds: Rect,
        item_count: usize,
        state: &mut ListViewState,
        theme: &Theme,
        mut draw_row: F,
    ) where
        F: FnMut(&mut Surface, usize, Rect, bool),
    {
        if bounds.w < 16 || bounds.h < 16 {
            return;
        }
        let inner = bounds.inset(crate::gfx::Insets::all(1));
        state.clamp_scroll(item_count, inner.h);
        surface.fill_rect(bounds, theme.input_bg);
        surface.draw_rect(bounds, theme.input_border);

        let total_h = (item_count as i32) * state.row_height;
        let needs_sb = total_h > inner.h;
        let content_rect = if needs_sb {
            Rect::new(inner.x, inner.y, (inner.w - Self::SCROLLBAR_W).max(0), inner.h)
        } else {
            inner
        };

        surface.push_clip(content_rect);
        let first_idx = (state.scroll_y / state.row_height).max(0) as usize;
        let visible_rows = (content_rect.h / state.row_height + 2) as usize;
        let end_idx = (first_idx + visible_rows).min(item_count);

        for i in first_idx..end_idx {
            let ry = content_rect.y + (i as i32 * state.row_height) - state.scroll_y;
            let row_rect = Rect::new(content_rect.x, ry, content_rect.w, state.row_height);
            let is_sel = state.selected == Some(i);
            if is_sel {
                surface.fill_rect(row_rect, theme.selection);
            } else if i % 2 == 1 {
                surface.fill_rect(row_rect, Color::lerp(theme.input_bg, theme.face, 28));
            }
            draw_row(surface, i, row_rect, is_sel);
        }
        surface.pop_clip();

        if needs_sb {
            let sb = Rect::new(inner.right() - Self::SCROLLBAR_W, inner.y, Self::SCROLLBAR_W, inner.h);
            surface.fill_rect(sb, theme.face);
            surface.draw_rect(sb, theme.ctrl_border);
            let max_scroll = (total_h - inner.h).max(1);
            let thumb_h = ((inner.h as i64 * inner.h as i64) / total_h.max(1) as i64)
                .clamp(24, inner.h as i64) as i32;
            let travel = (inner.h - thumb_h).max(1);
            let thumb_y = sb.y + ((state.scroll_y as i64 * travel as i64) / max_scroll as i64) as i32;
            let thumb = Rect::new(sb.x + 2, thumb_y + 1, sb.w - 4, (thumb_h - 2).max(12));
            surface.fill_rounded_rect(thumb, 3, if state.dragging_thumb { theme.accent } else { theme.face_dark });
            surface.draw_rounded_rect(thumb, 3, theme.ctrl_border);
        }
    }

    pub fn draw_simple_row(
        surface: &mut Surface,
        row_rect: Rect,
        text: &str,
        selected: bool,
        theme: &Theme,
    ) {
        let fg = if selected { theme.selection_text } else { theme.text_primary };
        surface.push_clip(row_rect);
        with_default_font(|font| {
            let st = TextStyle::ui(fg);
            let (_, lh) = font.line_metrics(st);
            let ty = row_rect.y + ((row_rect.h - lh) / 2);
            font.draw_text_ellipsis(surface, row_rect.x + 6, ty, (row_rect.w - 12).max(4), text, st);
        });
        surface.pop_clip();
    }
}
