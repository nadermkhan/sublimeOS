use crate::event::{EventResult, GuiEvent, KeyCode, MouseButton};
use crate::font::{with_default_font, TextStyle};
use crate::gfx::{Color, Insets, Rect, Surface, Theme, ThemePart, ThemeState, UxTheme};

#[derive(Clone, Copy, Debug)]
pub struct ComboBoxState {
    pub selected: usize,
    pub open: bool,
    pub hovered_header: bool,
    pub hovered_item: Option<usize>,
    pub scroll_offset: usize,
    pub disabled: bool,
}

impl Default for ComboBoxState {
    fn default() -> Self {
        Self::new(0)
    }
}

impl ComboBoxState {
    pub const fn new(selected: usize) -> Self {
        Self {
            selected,
            open: false,
            hovered_header: false,
            hovered_item: None,
            scroll_offset: 0,
            disabled: false,
        }
    }
}

/// Win32 `COMBOBOX` (`CBS_DROPDOWNLIST`) with hover/keyboard navigation & floating popup.
pub struct ComboBox;

impl ComboBox {
    const BTN_W: i32 = 20;
    const ITEM_H: i32 = 22;
    const MAX_VISIBLE_ITEMS: usize = 6;

    pub fn popup_rect(bounds: Rect, item_count: usize) -> Rect {
        let visible = item_count.min(Self::MAX_VISIBLE_ITEMS).max(1) as i32;
        Rect::new(bounds.x, bounds.bottom(), bounds.w, visible * Self::ITEM_H + 4)
    }

    pub fn handle_event(
        state: &mut ComboBoxState,
        bounds: Rect,
        items: &[&str],
        event: GuiEvent,
    ) -> (EventResult, Option<usize>) {
        if state.disabled || items.is_empty() {
            state.open = false;
            return (EventResult::Ignored, None);
        }
        state.selected = state.selected.min(items.len() - 1);
        let max_scroll = items.len().saturating_sub(Self::MAX_VISIBLE_ITEMS);
        state.scroll_offset = state.scroll_offset.min(max_scroll);

        match event {
            GuiEvent::MouseMove { x, y } => {
                let hh = bounds.contains(x, y);
                let mut hi = None;
                if state.open {
                    let pr = Self::popup_rect(bounds, items.len());
                    if pr.contains(x, y) {
                        let rel = ((y - pr.y - 2) / Self::ITEM_H).max(0) as usize;
                        let idx = state.scroll_offset + rel;
                        if idx < items.len() {
                            hi = Some(idx);
                        }
                    }
                }
                if hh != state.hovered_header || hi != state.hovered_item {
                    state.hovered_header = hh;
                    state.hovered_item = hi;
                    return (EventResult::NeedsRedraw, None);
                }
                (EventResult::Ignored, None)
            }
            GuiEvent::MouseWheel { x, y, delta_y } if state.open => {
                let pr = Self::popup_rect(bounds, items.len());
                if pr.contains(x, y) && max_scroll > 0 {
                    let next = if delta_y > 0 {
                        state.scroll_offset.saturating_sub(1)
                    } else {
                        (state.scroll_offset + 1).min(max_scroll)
                    };
                    if next != state.scroll_offset {
                        state.scroll_offset = next;
                        return (EventResult::NeedsRedraw, None);
                    }
                }
                (EventResult::Ignored, None)
            }
            GuiEvent::MouseDown { x, y, button: MouseButton::Left } => {
                if bounds.contains(x, y) {
                    state.open = !state.open;
                    state.hovered_item = Some(state.selected);
                    return (EventResult::NeedsRedraw, None);
                }
                if state.open {
                    let pr = Self::popup_rect(bounds, items.len());
                    if pr.contains(x, y) {
                        let rel = ((y - pr.y - 2) / Self::ITEM_H).max(0) as usize;
                        let idx = state.scroll_offset + rel;
                        state.open = false;
                        if idx < items.len() {
                            let changed = state.selected != idx;
                            state.selected = idx;
                            return (EventResult::NeedsRedraw, if changed { Some(idx) } else { None });
                        }
                    }
                    state.open = false;
                    return (EventResult::NeedsRedraw, None);
                }
                (EventResult::Ignored, None)
            }
            GuiEvent::KeyDown { key, .. } if state.open => match key {
                KeyCode::Up => {
                    if state.selected > 0 {
                        state.selected -= 1;
                        if state.selected < state.scroll_offset {
                            state.scroll_offset = state.selected;
                        }
                        return (EventResult::NeedsRedraw, Some(state.selected));
                    }
                    (EventResult::Handled, None)
                }
                KeyCode::Down => {
                    if state.selected + 1 < items.len() {
                        state.selected += 1;
                        if state.selected >= state.scroll_offset + Self::MAX_VISIBLE_ITEMS {
                            state.scroll_offset = state.selected + 1 - Self::MAX_VISIBLE_ITEMS;
                        }
                        return (EventResult::NeedsRedraw, Some(state.selected));
                    }
                    (EventResult::Handled, None)
                }
                KeyCode::Enter | KeyCode::Escape => {
                    state.open = false;
                    (EventResult::NeedsRedraw, None)
                }
                _ => (EventResult::Ignored, None),
            },
            _ => (EventResult::Ignored, None),
        }
    }

    pub fn draw(surface: &mut Surface, bounds: Rect, items: &[&str], state: ComboBoxState, theme: &Theme) {
        if bounds.w < 28 || bounds.h < 16 {
            return;
        }
        let tstate = if state.disabled {
            ThemeState::Disabled
        } else if state.open {
            ThemeState::Focused
        } else if state.hovered_header {
            ThemeState::Hot
        } else {
            ThemeState::Normal
        };
        UxTheme::draw_background(surface, ThemePart::ComboBox, tstate, bounds, theme);

        let btn_w = Self::BTN_W.min(bounds.w / 3);
        let btn_r = Rect::new(bounds.right() - btn_w - 1, bounds.y + 1, btn_w, (bounds.h - 2).max(2));
        let btn_state = ThemeState::from_flags(state.hovered_header, state.open, state.disabled, false);
        UxTheme::draw_background(surface, ThemePart::ComboDropButton, btn_state, btn_r, theme);

        let text_r = Rect::new(bounds.x + 6, bounds.y, (bounds.w - btn_w - 10).max(0), bounds.h);
        if !text_r.is_empty() {
            let label = items.get(state.selected).copied().unwrap_or("");
            surface.push_clip(text_r);
            with_default_font(|font| {
                let fg = if state.disabled { theme.text_secondary } else { theme.text_primary };
                let st = TextStyle::ui(fg);
                let (_, lh) = font.line_metrics(st);
                font.draw_text_ellipsis(surface, text_r.x, bounds.y + (bounds.h - lh) / 2, text_r.w, label, st);
            });
            surface.pop_clip();
        }
    }

    /// Render the floating dropdown popup AFTER client body so it paints cleanly over widgets below.
    pub fn draw_popup(surface: &mut Surface, bounds: Rect, items: &[&str], state: ComboBoxState, theme: &Theme) {
        if !state.open || items.is_empty() {
            return;
        }
        let pr = Self::popup_rect(bounds, items.len());
        surface.fill_rect(Rect::new(pr.x + 2, pr.y + 2, pr.w, pr.h), Color::argb(64, 0, 0, 0));
        surface.fill_rect(pr, theme.input_bg);
        surface.draw_rect(pr, theme.ctrl_border);

        let visible = items.len().min(Self::MAX_VISIBLE_ITEMS);
        let inner = pr.inset(Insets::all(2));
        surface.push_clip(inner);
        for row in 0..visible {
            let idx = state.scroll_offset + row;
            let Some(&label) = items.get(idx) else {
                break;
            };
            let rr = Rect::new(inner.x, inner.y + row as i32 * Self::ITEM_H, inner.w, Self::ITEM_H);
            let active = state.hovered_item == Some(idx) || (state.hovered_item.is_none() && state.selected == idx);
            if active {
                surface.fill_rect(rr, theme.selection);
            }
            let fg = if active { theme.selection_text } else { theme.text_primary };
            with_default_font(|font| {
                let st = TextStyle::ui(fg);
                let (_, lh) = font.line_metrics(st);
                font.draw_text_ellipsis(surface, rr.x + 5, rr.y + (rr.h - lh) / 2, (rr.w - 10).max(0), label, st);
            });
        }
        surface.pop_clip();
    }
}
