use alloc::string::String;
use crate::event::{EventResult, GuiEvent, KeyCode, MouseButton};
use crate::font::{with_default_font, TextStyle};
use crate::gfx::{Color, Insets, Point, Rect, Surface, Theme, ThemePart, ThemeState, UxTheme};
use crate::widget::{Button, ButtonState, ButtonStyle, MenuItemSpec};

#[derive(Clone, Copy, Debug, Default)]
pub struct ContextMenuState {
    pub open: bool,
    pub pos: Point,
    pub hovered_item: Option<usize>,
}

impl ContextMenuState {
    pub fn show(&mut self, x: i32, y: i32) {
        self.open = true;
        self.pos = Point::new(x, y);
        self.hovered_item = None;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.hovered_item = None;
    }
}

/// Win32 `TrackPopupMenu` standalone right-click popup context menu.
pub struct ContextMenu;

impl ContextMenu {
    const ITEM_H: i32 = 22;
    const SEP_H: i32 = 7;

    pub fn popup_rect(bounds: Rect, pos: Point, items: &[MenuItemSpec]) -> Rect {
        let mut h = 6;
        let mut w = 156;
        for it in items {
            h += if it.separator { Self::SEP_H } else { Self::ITEM_H };
            let need = (it.label.len() + it.shortcut.len()) as i32 * 8 + 34;
            if need > w {
                w = need;
            }
        }
        w = w.min(260);
        let px = pos.x.min((bounds.right() - w - 4).max(bounds.x)).max(bounds.x);
        let py = pos.y.min((bounds.bottom() - h - 4).max(bounds.y)).max(bounds.y);
        Rect::new(px, py, w, h)
    }

    pub fn handle_event(
        state: &mut ContextMenuState,
        bounds: Rect,
        items: &[MenuItemSpec],
        event: GuiEvent,
    ) -> (EventResult, Option<u16>) {
        if !state.open {
            return (EventResult::Ignored, None);
        }
        let pr = Self::popup_rect(bounds, state.pos, items);
        match event {
            GuiEvent::MouseMove { x, y } => {
                let mut next = None;
                if pr.contains(x, y) {
                    let mut iy = pr.y + 3;
                    for (i, it) in items.iter().enumerate() {
                        let h = if it.separator { Self::SEP_H } else { Self::ITEM_H };
                        if !it.separator && y >= iy && y < iy + h {
                            next = Some(i);
                            break;
                        }
                        iy += h;
                    }
                }
                if next != state.hovered_item {
                    state.hovered_item = next;
                    return (EventResult::NeedsRedraw, None);
                }
                (EventResult::Handled, None)
            }
            GuiEvent::MouseDown { x, y, button: MouseButton::Left } => {
                if pr.contains(x, y) {
                    let mut iy = pr.y + 3;
                    for it in items {
                        let h = if it.separator { Self::SEP_H } else { Self::ITEM_H };
                        if !it.separator && y >= iy && y < iy + h {
                            state.close();
                            return (EventResult::NeedsRedraw, Some(it.id));
                        }
                        iy += h;
                    }
                }
                state.close();
                (EventResult::NeedsRedraw, None)
            }
            GuiEvent::KeyDown { key: KeyCode::Escape, .. } => {
                state.close();
                (EventResult::NeedsRedraw, None)
            }
            _ => (EventResult::Ignored, None),
        }
    }

    pub fn draw(surface: &mut Surface, bounds: Rect, items: &[MenuItemSpec], state: ContextMenuState, theme: &Theme) {
        if !state.open || items.is_empty() {
            return;
        }
        let pr = Self::popup_rect(bounds, state.pos, items);
        surface.fill_rect(Rect::new(pr.x + 2, pr.y + 2, pr.w, pr.h), Color::argb(72, 0, 0, 0));
        surface.fill_rect(pr, theme.face_light);
        surface.draw_rect(pr, theme.ctrl_border);

        let mut iy = pr.y + 3;
        for (i, it) in items.iter().enumerate() {
            if it.separator {
                surface.fill_rect(Rect::new(pr.x + 6, iy + 3, pr.w - 12, 1), theme.border_shadow);
                iy += Self::SEP_H;
                continue;
            }
            let ir = Rect::new(pr.x + 2, iy, pr.w - 4, Self::ITEM_H);
            let hov = state.hovered_item == Some(i);
            if hov {
                surface.fill_rect(ir, theme.selection);
            }
            let fg = if hov { theme.selection_text } else { theme.text_primary };
            let sc = if hov { theme.selection_text } else { theme.text_secondary };
            with_default_font(|font| {
                let st = TextStyle::ui(fg);
                let (_, lh) = font.line_metrics(st);
                let ty = ir.y + (ir.h - lh) / 2;
                font.draw_text(surface, ir.x + 8, ty, it.label, st);
                if !it.shortcut.is_empty() {
                    let sst = TextStyle::ui_sized(12, sc);
                    let (sw, _) = font.measure_text(it.shortcut, sst);
                    font.draw_text(surface, ir.right() - sw - 8, ty, it.shortcut, sst);
                }
            });
            iy += Self::ITEM_H;
        }
    }
}

/// ComCtl32 `tooltips_class32` anchored hover tooltip bubble.
pub struct ToolTip;

impl ToolTip {
    pub fn draw(surface: &mut Surface, bounds: Rect, anchor: Point, text: &str, theme: &Theme) {
        if text.is_empty() {
            return;
        }
        with_default_font(|font| {
            let st = TextStyle::ui_sized(12, theme.text_primary);
            let (tw, lh) = font.measure_text(text, st);
            let w = (tw + 14).min(bounds.w - 8).max(32);
            let h = (lh + 8).max(20);
            let x = (anchor.x + 12).min((bounds.right() - w - 4).max(bounds.x)).max(bounds.x);
            let y = (anchor.y + 18).min((bounds.bottom() - h - 4).max(bounds.y)).max(bounds.y);
            let r = Rect::new(x, y, w, h);
            surface.fill_rect(Rect::new(r.x + 2, r.y + 2, r.w, r.h), Color::argb(52, 0, 0, 0));
            UxTheme::draw_background(surface, ThemePart::ToolTip, ThemeState::Normal, r, theme);
            surface.push_clip(r);
            font.draw_text_ellipsis(surface, r.x + 7, r.y + 4, (r.w - 14).max(0), text, st);
            surface.pop_clip();
        });
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MessageBoxIcon {
    Info,
    Warning,
    Error,
    Question,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MessageBoxButtons {
    Ok,
    OkCancel,
    YesNo,
    YesNoCancel,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DialogResult {
    None,
    Ok,
    Cancel,
    Yes,
    No,
}

#[derive(Clone, Debug)]
pub struct MessageBoxState {
    pub visible: bool,
    pub title: String,
    pub message: String,
    pub detail: String,
    pub icon: MessageBoxIcon,
    pub buttons: MessageBoxButtons,
    pub btns: [ButtonState; 3],
}

impl Default for MessageBoxState {
    fn default() -> Self {
        Self::closed()
    }
}

impl MessageBoxState {
    pub const fn closed() -> Self {
        Self {
            visible: false,
            title: String::new(),
            message: String::new(),
            detail: String::new(),
            icon: MessageBoxIcon::Info,
            buttons: MessageBoxButtons::Ok,
            btns: [ButtonState { hovered: false, pressed: false, disabled: false }; 3],
        }
    }

    pub fn open(&mut self, title: &str, message: &str, detail: &str, icon: MessageBoxIcon, buttons: MessageBoxButtons) {
        self.visible = true;
        self.title = String::from(title);
        self.message = String::from(message);
        self.detail = String::from(detail);
        self.icon = icon;
        self.buttons = buttons;
        self.btns = [ButtonState::default(); 3];
    }
}

/// Win32 `MessageBox` / Vista+ `TaskDialog` modal overlay rendered within window `bounds`.
pub struct MessageBox;

impl MessageBox {
    fn modal_rect(bounds: Rect, has_detail: bool) -> Rect {
        let w = (bounds.w - 40).clamp(240, 380);
        let h = if has_detail { 184 } else { 150 };
        Rect::new(bounds.x + (bounds.w - w) / 2, bounds.y + (bounds.h - h) / 2, w, h)
    }

    fn button_specs(buttons: MessageBoxButtons) -> (&'static [(&'static str, DialogResult)], usize) {
        match buttons {
            MessageBoxButtons::Ok => (&[("OK", DialogResult::Ok)], 1),
            MessageBoxButtons::OkCancel => (&[("OK", DialogResult::Ok), ("Cancel", DialogResult::Cancel)], 2),
            MessageBoxButtons::YesNo => (&[("Yes", DialogResult::Yes), ("No", DialogResult::No)], 2),
            MessageBoxButtons::YesNoCancel => (
                &[("Yes", DialogResult::Yes), ("No", DialogResult::No), ("Cancel", DialogResult::Cancel)],
                3,
            ),
        }
    }

    fn button_rects(modal: Rect, count: usize) -> [Rect; 3] {
        let bw = 72;
        let bh = 24;
        let gap = 8;
        let by = modal.bottom() - bh - 10;
        let mut out = [Rect::default(); 3];
        let total_w = count as i32 * bw + (count.saturating_sub(1) as i32) * gap;
        let mut bx = modal.right() - total_w - 12;
        for slot in out.iter_mut().take(count) {
            *slot = Rect::new(bx, by, bw, bh);
            bx += bw + gap;
        }
        out
    }

    pub fn handle_event(state: &mut MessageBoxState, bounds: Rect, event: GuiEvent) -> (EventResult, DialogResult) {
        if !state.visible {
            return (EventResult::Ignored, DialogResult::None);
        }
        let modal = Self::modal_rect(bounds, !state.detail.is_empty());
        let (specs, count) = Self::button_specs(state.buttons);
        let rects = Self::button_rects(modal, count);

        if let GuiEvent::KeyDown { key: KeyCode::Escape, .. } = event {
            state.visible = false;
            return (EventResult::NeedsRedraw, DialogResult::Cancel);
        }
        if let GuiEvent::KeyDown { key: KeyCode::Enter, .. } = event {
            state.visible = false;
            return (EventResult::NeedsRedraw, specs[0].1);
        }

        let mut res = EventResult::Handled;
        for i in 0..count {
            let (br, clicked) = Button::handle_event(&mut state.btns[i], rects[i], event);
            res.merge(br);
            if clicked {
                state.visible = false;
                return (EventResult::NeedsRedraw, specs[i].1);
            }
        }
        (res, DialogResult::None)
    }

    pub fn draw(surface: &mut Surface, bounds: Rect, state: &MessageBoxState, theme: &Theme) {
        if !state.visible {
            return;
        }
        surface.fill_rect(bounds, Color::argb(96, 0, 0, 0));
        let modal = Self::modal_rect(bounds, !state.detail.is_empty());
        surface.fill_rect(Rect::new(modal.x + 3, modal.y + 3, modal.w, modal.h), Color::argb(90, 0, 0, 0));
        surface.fill_rect(modal, theme.face);
        surface.draw_rect(modal, theme.frame_outer);

        let title_r = Rect::new(modal.x + 1, modal.y + 1, modal.w - 2, 24);
        surface.draw_gradient_v(title_r, theme.title_top, theme.title_bot);
        surface.push_clip(title_r);
        with_default_font(|font| {
            let st = TextStyle {
                bold: true,
                size_px: 13,
                color: theme.selection_text,
                ..TextStyle::ui(theme.selection_text)
            };
            font.draw_text_ellipsis(surface, title_r.x + 8, title_r.y + 4, (title_r.w - 16).max(0), &state.title, st);
        });
        surface.pop_clip();

        let (icon_col, badge_ch) = match state.icon {
            MessageBoxIcon::Info => (theme.frame_active, "i"),
            MessageBoxIcon::Warning => (theme.warn, "!"),
            MessageBoxIcon::Error => (theme.danger, "X"),
            MessageBoxIcon::Question => (theme.text_link, "?"),
        };
        let icx = modal.x + 28;
        let icy = title_r.bottom() + 26;
        surface.fill_circle(icx, icy, 14, icon_col);
        with_default_font(|font| {
            let st = TextStyle {
                bold: true,
                size_px: 15,
                color: theme.selection_text,
                ..TextStyle::ui_sized(15, theme.selection_text)
            };
            let (tw, lh) = font.measure_text(badge_ch, st);
            font.draw_text(surface, icx - tw / 2, icy - lh / 2, badge_ch, st);
        });

        let msg_r = Rect::new(modal.x + 54, title_r.bottom() + 12, (modal.w - 66).max(40), modal.h - 76);
        surface.push_clip(msg_r);
        with_default_font(|font| {
            let h = font.draw_text_wrapped(surface, msg_r.x, msg_r.y, msg_r.w, &state.message, TextStyle::ui(theme.text_primary));
            if !state.detail.is_empty() {
                font.draw_text_wrapped(
                    surface,
                    msg_r.x,
                    msg_r.y + h + 6,
                    msg_r.w,
                    &state.detail,
                    TextStyle::ui_sized(12, theme.text_secondary),
                );
            }
        });
        surface.pop_clip();

        let footer_y = modal.bottom() - 44;
        surface.fill_rect(Rect::new(modal.x + 1, footer_y, modal.w - 2, 43), theme.face_dark);
        surface.fill_rect(Rect::new(modal.x + 1, footer_y, modal.w - 2, 1), theme.border_shadow);

        let (specs, count) = Self::button_specs(state.buttons);
        let rects = Self::button_rects(modal, count);
        for i in 0..count {
            let style = if i == 0 { ButtonStyle::Primary } else { ButtonStyle::Normal };
            Button::draw(surface, rects[i], specs[i].0, style, state.btns[i], theme);
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToastSeverity {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Clone, Debug)]
pub struct ToastState {
    pub visible: bool,
    pub title: String,
    pub body: String,
    pub severity: ToastSeverity,
    pub ticks_left: u32,
}

impl Default for ToastState {
    fn default() -> Self {
        Self {
            visible: false,
            title: String::new(),
            body: String::new(),
            severity: ToastSeverity::Info,
            ticks_left: 0,
        }
    }
}

impl ToastState {
    pub fn show(&mut self, title: &str, body: &str, severity: ToastSeverity, ticks: u32) {
        self.visible = true;
        self.title = String::from(title);
        self.body = String::from(body);
        self.severity = severity;
        self.ticks_left = ticks.max(1);
    }

    pub fn tick(&mut self) -> bool {
        if !self.visible {
            return false;
        }
        self.ticks_left = self.ticks_left.saturating_sub(1);
        if self.ticks_left == 0 {
            self.visible = false;
            return true;
        }
        false
    }
}

/// Win32 `NIIF_*` balloon notification toast anchored to bottom-right of `bounds`.
pub struct NotificationToast;

impl NotificationToast {
    pub fn draw(surface: &mut Surface, bounds: Rect, state: &ToastState, theme: &Theme) {
        if !state.visible || bounds.w < 180 || bounds.h < 72 {
            return;
        }
        let w = 248.min(bounds.w - 16);
        let h = 54;
        let r = Rect::new(bounds.right() - w - 10, bounds.bottom() - h - 10, w, h);
        let accent = match state.severity {
            ToastSeverity::Info => theme.frame_active,
            ToastSeverity::Success => theme.good,
            ToastSeverity::Warning => theme.warn,
            ToastSeverity::Error => theme.danger,
        };
        surface.fill_rect(Rect::new(r.x + 2, r.y + 2, r.w, r.h), Color::argb(72, 0, 0, 0));
        surface.fill_rounded_rect(r, 4, theme.face_light);
        surface.draw_rounded_rect(r, 4, theme.ctrl_border);
        surface.fill_rect(Rect::new(r.x + 1, r.y + 4, 4, r.h - 8), accent);

        let inner = r.inset(Insets::new(6, 8, 6, 12));
        surface.push_clip(inner);
        with_default_font(|font| {
            let tst = TextStyle {
                bold: true,
                size_px: 13,
                color: theme.text_primary,
                ..TextStyle::ui(theme.text_primary)
            };
            font.draw_text_ellipsis(surface, inner.x, inner.y, inner.w, &state.title, tst);
            font.draw_text_ellipsis(surface, inner.x, inner.y + 20, inner.w, &state.body, TextStyle::ui_sized(12, theme.text_secondary));
        });
        surface.pop_clip();
    }
}
