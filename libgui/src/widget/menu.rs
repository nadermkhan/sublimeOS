use crate::event::{EventResult, GuiEvent, KeyCode, MouseButton};
use crate::font::{with_default_font, TextStyle};
use crate::gfx::{Rect, Surface, Theme};

#[derive(Clone, Copy, Debug)]
pub struct MenuItemSpec {
    pub id: u16,
    pub label: &'static str,
    pub shortcut: &'static str,
    pub separator: bool,
}

impl MenuItemSpec {
    pub const fn action(id: u16, label: &'static str, shortcut: &'static str) -> Self {
        Self {
            id,
            label,
            shortcut,
            separator: false,
        }
    }

    pub const fn sep() -> Self {
        Self {
            id: 0,
            label: "",
            shortcut: "",
            separator: true,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct MenuSpec {
    pub title: &'static str,
    pub items: &'static [MenuItemSpec],
}

#[derive(Clone, Copy, Debug, Default)]
pub struct MenuBarState {
    pub open_menu: Option<usize>,
    pub hovered_header: Option<usize>,
    pub hovered_item: Option<usize>,
}

pub struct MenuBar;

impl MenuBar {
    const MENU_ITEM_H: i32 = 22;
    const MENU_SEP_H: i32 = 7;

    fn header_rect(bar: Rect, menus: &[MenuSpec], idx: usize) -> Rect {
        let mut x = bar.x + 4;
        for (i, m) in menus.iter().enumerate() {
            let w = (m.title.len() as i32 * 8 + 16).max(38);
            if i == idx {
                return Rect::new(x, bar.y + 1, w, (bar.h - 2).max(1));
            }
            x += w + 2;
        }
        Rect::default()
    }

    pub fn popup_rect(bar: Rect, menus: &[MenuSpec], idx: usize) -> Rect {
        let hdr = Self::header_rect(bar, menus, idx);
        let m = &menus[idx];
        let mut h = 6i32;
        let mut max_chars = 10usize;
        for it in m.items {
            h += if it.separator { Self::MENU_SEP_H } else { Self::MENU_ITEM_H };
            if !it.separator {
                let sc_len = if it.shortcut.is_empty() { 0 } else { it.shortcut.len() + 2 };
                max_chars = max_chars.max(it.label.len() + sc_len);
            }
        }
        let max_w = (bar.w - 4).max(150);
        let w = (max_chars as i32 * 8 + 24).max(180).min(max_w);
        let px = hdr.x.min((bar.right() - w - 2).max(bar.x)).max(bar.x);
        Rect::new(px, bar.bottom(), w, h)
    }

    /// Handle bar + dropdown popup events. Returns `(EventResult, triggered_item_id)`.
    pub fn handle_event(
        state: &mut MenuBarState,
        bar: Rect,
        menus: &[MenuSpec],
        event: GuiEvent,
    ) -> (EventResult, Option<u16>) {
        match event {
            GuiEvent::KeyDown { key: KeyCode::Escape, .. } => {
                if state.open_menu.is_some() {
                    state.open_menu = None;
                    state.hovered_item = None;
                    return (EventResult::NeedsRedraw, None);
                }
                (EventResult::Ignored, None)
            }
            GuiEvent::MouseMove { x, y } => {
                let mut changed = false;
                let mut next_hdr = None;
                if bar.contains(x, y) {
                    for i in 0..menus.len() {
                        if Self::header_rect(bar, menus, i).contains(x, y) {
                            next_hdr = Some(i);
                            break;
                        }
                    }
                }
                if next_hdr != state.hovered_header {
                    state.hovered_header = next_hdr;
                    changed = true;
                }
                if state.open_menu.is_some() && next_hdr.is_some() && next_hdr != state.open_menu {
                    state.open_menu = next_hdr;
                    state.hovered_item = None;
                    changed = true;
                }
                if let Some(open_idx) = state.open_menu {
                    let popup = Self::popup_rect(bar, menus, open_idx);
                    let mut next_item = None;
                    if popup.contains(x, y) {
                        let mut iy = popup.y + 3;
                        for (ii, it) in menus[open_idx].items.iter().enumerate() {
                            let h = if it.separator { Self::MENU_SEP_H } else { Self::MENU_ITEM_H };
                            if !it.separator && y >= iy && y < iy + h {
                                next_item = Some(ii);
                                break;
                            }
                            iy += h;
                        }
                    }
                    if next_item != state.hovered_item {
                        state.hovered_item = next_item;
                        changed = true;
                    }
                }
                if changed {
                    (EventResult::NeedsRedraw, None)
                } else {
                    (EventResult::Ignored, None)
                }
            }
            GuiEvent::MouseDown { x, y, button: MouseButton::Left } => {
                if bar.contains(x, y) {
                    for i in 0..menus.len() {
                        if Self::header_rect(bar, menus, i).contains(x, y) {
                            state.open_menu = if state.open_menu == Some(i) { None } else { Some(i) };
                            state.hovered_item = None;
                            return (EventResult::NeedsRedraw, None);
                        }
                    }
                }
                if let Some(open_idx) = state.open_menu {
                    let popup = Self::popup_rect(bar, menus, open_idx);
                    if popup.contains(x, y) {
                        let mut iy = popup.y + 3;
                        for it in menus[open_idx].items {
                            let h = if it.separator { Self::MENU_SEP_H } else { Self::MENU_ITEM_H };
                            if !it.separator && y >= iy && y < iy + h {
                                state.open_menu = None;
                                state.hovered_item = None;
                                return (EventResult::NeedsRedraw, Some(it.id));
                            }
                            iy += h;
                        }
                    }
                    // Click outside closes open menu and absorbs click
                    state.open_menu = None;
                    state.hovered_item = None;
                    return (EventResult::NeedsRedraw, None);
                }
                (EventResult::Ignored, None)
            }
            _ => (EventResult::Ignored, None),
        }
    }

    pub fn draw_bar(surface: &mut Surface, bar: Rect, menus: &[MenuSpec], state: MenuBarState, theme: &Theme) {
        if bar.is_empty() {
            return;
        }
        surface.fill_rect(bar, theme.face);
        surface.fill_rect(Rect::new(bar.x, bar.bottom() - 1, bar.w, 1), theme.border_shadow);

        for (i, m) in menus.iter().enumerate() {
            let hr = Self::header_rect(bar, menus, i);
            let is_open = state.open_menu == Some(i);
            let is_hover = state.hovered_header == Some(i);
            if is_open {
                surface.fill_rect(hr, theme.selection);
            } else if is_hover {
                surface.fill_rect(hr, theme.face_dark);
            }
            let fg = if is_open { theme.selection_text } else { theme.text_primary };
            with_default_font(|font| {
                let st = TextStyle::ui(fg);
                let (tw, lh) = font.measure_text(m.title, st);
                let tx = hr.x + ((hr.w - tw) / 2);
                let ty = hr.y + ((hr.h - lh) / 2);
                font.draw_text(surface, tx, ty, m.title, st);
            });
        }
    }

    /// Render dropdown overlay AFTER window body so it draws cleanly above client widgets.
    pub fn draw_popup(surface: &mut Surface, bar: Rect, menus: &[MenuSpec], state: MenuBarState, theme: &Theme) {
        let Some(open_idx) = state.open_menu else {
            return;
        };
        if open_idx >= menus.len() {
            return;
        }
        let popup = Self::popup_rect(bar, menus, open_idx);
        surface.fill_rect(Rect::new(popup.x + 2, popup.y + 2, popup.w, popup.h), crate::gfx::Color::argb(72, 0, 0, 0));
        surface.fill_rect(popup, theme.face_light);
        surface.draw_rect(popup, theme.ctrl_border);

        let mut iy = popup.y + 3;
        for (ii, it) in menus[open_idx].items.iter().enumerate() {
            if it.separator {
                surface.fill_rect(Rect::new(popup.x + 6, iy + 3, popup.w - 12, 1), theme.border_shadow);
                iy += Self::MENU_SEP_H;
                continue;
            }
            let item_r = Rect::new(popup.x + 2, iy, popup.w - 4, Self::MENU_ITEM_H);
            let hovered = state.hovered_item == Some(ii);
            if hovered {
                surface.fill_rect(item_r, theme.selection);
            }
            let fg = if hovered { theme.selection_text } else { theme.text_primary };
            let sc_fg = if hovered { theme.selection_text } else { theme.text_secondary };
            with_default_font(|font| {
                let st = TextStyle::ui(fg);
                let (_, lh) = font.line_metrics(st);
                let ty = item_r.y + ((item_r.h - lh) / 2);
                font.draw_text(surface, item_r.x + 8, ty, it.label, st);
                if !it.shortcut.is_empty() {
                    let sst = TextStyle::ui_sized(12, sc_fg);
                    let (sw, _) = font.measure_text(it.shortcut, sst);
                    font.draw_text(surface, item_r.right() - sw - 8, ty, it.shortcut, sst);
                }
            });
            iy += Self::MENU_ITEM_H;
        }
    }
}
