use crate::event::{EventResult, GuiEvent, MouseButton};
use crate::font::{with_default_font, TextStyle};
use crate::gfx::{ArrowDir, Rect, Surface, Theme, ThemePart, ThemeState, UxTheme};
use crate::layout::Length;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToolBarItemKind {
    Button,
    Toggle { checked: bool },
    Separator,
}

#[derive(Clone, Copy, Debug)]
pub struct ToolBarItem {
    pub id: u16,
    pub label: &'static str,
    pub kind: ToolBarItemKind,
    pub disabled: bool,
}

impl ToolBarItem {
    pub const fn button(id: u16, label: &'static str) -> Self {
        Self {
            id,
            label,
            kind: ToolBarItemKind::Button,
            disabled: false,
        }
    }

    pub const fn toggle(id: u16, label: &'static str, checked: bool) -> Self {
        Self {
            id,
            label,
            kind: ToolBarItemKind::Toggle { checked },
            disabled: false,
        }
    }

    pub const fn sep() -> Self {
        Self {
            id: 0,
            label: "",
            kind: ToolBarItemKind::Separator,
            disabled: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ToolBarState {
    pub hovered: Option<usize>,
    pub pressed: Option<usize>,
}

/// ComCtl32 `ToolbarWindow32` flat command toolbar with separators and toggle buttons.
pub struct ToolBar;

impl ToolBar {
    fn item_rect(bar: Rect, items: &[ToolBarItem], idx: usize) -> Rect {
        let mut x = bar.x + 4;
        for (i, it) in items.iter().enumerate() {
            let w = match it.kind {
                ToolBarItemKind::Separator => 7,
                _ => (it.label.len() as i32 * 8 + 16).max(32),
            };
            if i == idx {
                return Rect::new(x, bar.y + 3, w, (bar.h - 6).max(16));
            }
            x += w + 2;
        }
        Rect::default()
    }

    pub fn handle_event(
        state: &mut ToolBarState,
        bar: Rect,
        items: &mut [ToolBarItem],
        event: GuiEvent,
    ) -> (EventResult, Option<u16>) {
        match event {
            GuiEvent::MouseMove { x, y } => {
                let mut nh = None;
                if bar.contains(x, y) {
                    for (i, it) in items.iter().enumerate() {
                        if !it.disabled
                            && !matches!(it.kind, ToolBarItemKind::Separator)
                            && Self::item_rect(bar, items, i).contains(x, y)
                        {
                            nh = Some(i);
                            break;
                        }
                    }
                }
                if nh != state.hovered {
                    state.hovered = nh;
                    return (EventResult::NeedsRedraw, None);
                }
                (EventResult::Ignored, None)
            }
            GuiEvent::MouseDown { x, y, button: MouseButton::Left } => {
                if bar.contains(x, y) {
                    for (i, it) in items.iter().enumerate() {
                        if !it.disabled
                            && !matches!(it.kind, ToolBarItemKind::Separator)
                            && Self::item_rect(bar, items, i).contains(x, y)
                        {
                            state.pressed = Some(i);
                            return (EventResult::NeedsRedraw, None);
                        }
                    }
                }
                (EventResult::Ignored, None)
            }
            GuiEvent::MouseUp { x, y, button: MouseButton::Left } => {
                if let Some(i) = state.pressed.take() {
                    if i < items.len() && Self::item_rect(bar, items, i).contains(x, y) {
                        if let ToolBarItemKind::Toggle { ref mut checked } = items[i].kind {
                            *checked = !*checked;
                        }
                        return (EventResult::NeedsRedraw, Some(items[i].id));
                    }
                    return (EventResult::NeedsRedraw, None);
                }
                (EventResult::Ignored, None)
            }
            _ => (EventResult::Ignored, None),
        }
    }

    pub fn draw(surface: &mut Surface, bar: Rect, items: &[ToolBarItem], state: ToolBarState, theme: &Theme) {
        if bar.is_empty() {
            return;
        }
        surface.draw_gradient_v(bar, theme.face_light, theme.face);
        surface.fill_rect(Rect::new(bar.x, bar.bottom() - 1, bar.w, 1), theme.border_shadow);
        surface.push_clip(bar);

        for (i, it) in items.iter().enumerate() {
            let r = Self::item_rect(bar, items, i);
            if r.right() > bar.right() - 2 {
                break;
            }
            if matches!(it.kind, ToolBarItemKind::Separator) {
                let sx = r.x + r.w / 2;
                surface.fill_rect(Rect::new(sx, r.y + 2, 1, (r.h - 4).max(1)), theme.border_shadow);
                surface.fill_rect(Rect::new(sx + 1, r.y + 2, 1, (r.h - 4).max(1)), theme.face_light);
                continue;
            }
            let checked = matches!(it.kind, ToolBarItemKind::Toggle { checked: true });
            let tstate = ThemeState::from_flags(
                state.hovered == Some(i),
                state.pressed == Some(i),
                it.disabled,
                checked,
            );
            UxTheme::draw_background(surface, ThemePart::ToolBarButton, tstate, r, theme);
            let fg = if it.disabled { theme.text_secondary } else { theme.text_primary };
            with_default_font(|font| {
                let st = TextStyle::ui(fg);
                let (tw, lh) = font.measure_text(it.label, st);
                let tx = r.x + (r.w - tw) / 2;
                let ty = r.y + (r.h - lh) / 2;
                font.draw_text(surface, tx, ty, it.label, st);
            });
        }
        surface.pop_clip();
    }
}

#[derive(Clone, Copy, Debug)]
pub struct StatusPane<'a> {
    pub text: &'a str,
    pub width: Length,
}

/// ComCtl32 `msctls_statusbar32` multi-pane bottom status bar with sunken bevels.
pub struct StatusBar;

impl StatusBar {
    pub fn draw(surface: &mut Surface, rect: Rect, panes: &[StatusPane<'_>], theme: &Theme) {
        if rect.w < 16 || rect.h < 14 {
            return;
        }
        surface.fill_rect(rect, theme.face);
        surface.fill_rect(Rect::new(rect.x, rect.y, rect.w, 1), theme.border_shadow);

        let gap = 4;
        let inner_w = (rect.w - 8 - gap * panes.len().saturating_sub(1) as i32).max(0);
        let mut fixed = 0i32;
        let mut flex = 0i32;
        for p in panes {
            match p.width {
                Length::Px(px) => fixed += px.max(0),
                Length::Flex(w) => flex += w as i32,
            }
        }
        let flex_rem = (inner_w - fixed).max(0);
        let mut x = rect.x + 4;
        for p in panes {
            let pw = match p.width {
                Length::Px(px) => px.max(0),
                Length::Flex(w) => {
                    if flex > 0 {
                        (flex_rem * w as i32) / flex
                    } else {
                        0
                    }
                }
            };
            let pr = Rect::new(x, rect.y + 3, pw.min(rect.right() - x - 2).max(0), (rect.h - 5).max(8));
            if pr.w >= 8 {
                UxTheme::draw_background(surface, ThemePart::StatusPane, ThemeState::Normal, pr, theme);
                surface.push_clip(Rect::new(pr.x + 2, pr.y + 1, (pr.w - 4).max(0), (pr.h - 2).max(0)));
                with_default_font(|font| {
                    let st = TextStyle::ui_sized(12, theme.text_primary);
                    let (_, lh) = font.line_metrics(st);
                    font.draw_text_ellipsis(surface, pr.x + 6, pr.y + (pr.h - lh) / 2, (pr.w - 10).max(0), p.text, st);
                });
                surface.pop_clip();
            }
            x += pw + gap;
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct AccordionState {
    pub expanded: bool,
    pub hovered: bool,
}

impl Default for AccordionState {
    fn default() -> Self {
        Self::new(true)
    }
}

impl AccordionState {
    pub const fn new(expanded: bool) -> Self {
        Self {
            expanded,
            hovered: false,
        }
    }
}

/// Explorer-style collapsible `Accordion` section header (`Expando`).
pub struct Accordion;

impl Accordion {
    pub const HEADER_H: i32 = 24;

    pub fn handle_event(state: &mut AccordionState, rect: Rect, event: GuiEvent) -> (EventResult, bool) {
        let hdr = Rect::new(rect.x, rect.y, rect.w, Self::HEADER_H.min(rect.h));
        match event {
            GuiEvent::MouseMove { x, y } => {
                let h = hdr.contains(x, y);
                if h != state.hovered {
                    state.hovered = h;
                    return (EventResult::NeedsRedraw, false);
                }
                (EventResult::Ignored, false)
            }
            GuiEvent::MouseDown { x, y, button: MouseButton::Left } => {
                if hdr.contains(x, y) {
                    state.expanded = !state.expanded;
                    return (EventResult::NeedsRedraw, true);
                }
                (EventResult::Ignored, false)
            }
            _ => (EventResult::Ignored, false),
        }
    }

    pub fn draw(surface: &mut Surface, rect: Rect, title: &str, state: AccordionState, theme: &Theme) -> Option<Rect> {
        if rect.w < 24 || rect.h < Self::HEADER_H {
            return None;
        }
        let hdr = Rect::new(rect.x, rect.y, rect.w, Self::HEADER_H);
        UxTheme::draw_background(
            surface,
            ThemePart::AccordionHeader,
            if state.hovered { ThemeState::Hot } else { ThemeState::Normal },
            hdr,
            theme,
        );
        let fg = if state.hovered { theme.selection_text } else { theme.text_primary };
        let arrow_r = Rect::new(hdr.right() - 20, hdr.y + 4, 14, hdr.h - 8);
        surface.draw_arrow(arrow_r, if state.expanded { ArrowDir::Up } else { ArrowDir::Down }, fg);
        surface.push_clip(hdr);
        with_default_font(|font| {
            let st = TextStyle {
                bold: true,
                size_px: 13,
                color: fg,
                ..TextStyle::ui(fg)
            };
            let (_, lh) = font.line_metrics(st);
            font.draw_text_ellipsis(surface, hdr.x + 8, hdr.y + (hdr.h - lh) / 2, (hdr.w - 30).max(0), title, st);
        });
        surface.pop_clip();
        if state.expanded && rect.h > Self::HEADER_H + 8 {
            let body = Rect::new(rect.x, hdr.bottom(), rect.w, rect.h - Self::HEADER_H);
            surface.fill_rect(body, theme.face_light);
            surface.draw_rect(body, theme.ctrl_border);
            Some(Rect::new(body.x + 6, body.y + 6, (body.w - 12).max(0), (body.h - 12).max(0)))
        } else {
            None
        }
    }
}
