use crate::font::{with_default_font, TextStyle};
use crate::gfx::{Color, Rect, Surface, Theme};

pub struct Label;

impl Label {
    pub fn draw(surface: &mut Surface, rect: Rect, text: &str, style: TextStyle) {
        if rect.is_empty() {
            return;
        }
        surface.push_clip(rect);
        with_default_font(|font| {
            let (_, lh) = font.line_metrics(style);
            let ty = rect.y + ((rect.h - lh) / 2).max(0);
            font.draw_text_ellipsis(surface, rect.x, ty, rect.w, text, style);
        });
        surface.pop_clip();
    }

    pub fn draw_multiline(surface: &mut Surface, rect: Rect, text: &str, style: TextStyle) {
        if rect.is_empty() {
            return;
        }
        surface.push_clip(rect);
        with_default_font(|font| {
            font.draw_text_wrapped(surface, rect.x, rect.y, rect.w, text, style);
        });
        surface.pop_clip();
    }
}

pub struct Card;

impl Card {
    pub fn draw(surface: &mut Surface, rect: Rect, title: Option<&str>, theme: &Theme) -> Rect {
        if rect.w < 8 || rect.h < 8 {
            return Rect::default();
        }
        surface.fill_rounded_rect(rect, 4, theme.face_light);
        surface.draw_rounded_rect(rect, 4, theme.ctrl_border);
        if let Some(t) = title {
            let (hdr, body) = rect.split_top(24, 2);
            surface.fill_rect(Rect::new(hdr.x + 1, hdr.y + 1, hdr.w - 2, hdr.h), theme.face);
            surface.fill_rect(Rect::new(hdr.x + 1, hdr.bottom() - 1, hdr.w - 2, 1), theme.border_shadow);
            Label::draw(
                surface,
                Rect::new(hdr.x + 8, hdr.y, hdr.w - 16, hdr.h),
                t,
                TextStyle {
                    bold: true,
                    color: theme.text_primary,
                    ..TextStyle::ui(theme.text_primary)
                },
            );
            body.inset(crate::gfx::Insets::all(8))
        } else {
            rect.inset(crate::gfx::Insets::all(8))
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProgressState {
    Normal,
    Paused,
    Error,
}

pub struct ProgressBar;

impl ProgressBar {
    /// Draw a horizontal progress bar (`percent` in `0..=100`).
    pub fn draw(surface: &mut Surface, rect: Rect, percent: u8, fill_color: Color, theme: &Theme) {
        if rect.w < 6 || rect.h < 6 {
            return;
        }
        surface.fill_rect(rect, theme.input_bg);
        surface.draw_rect(rect, theme.input_border);
        let inner = rect.inset(crate::gfx::Insets::all(2));
        let p = percent.min(100) as i32;
        let fill_w = (inner.w * p) / 100;
        if fill_w > 0 {
            // Segmented v6-style highlight chunks cleanly clipped inside filled region
            let bar = Rect::new(inner.x, inner.y, fill_w, inner.h);
            surface.draw_gradient_v(bar, Color::lerp(fill_color, Color::WHITE, 56), fill_color);
            let mut sx = inner.x + 8;
            while sx < inner.x + fill_w - 2 {
                surface.fill_rect(Rect::new(sx, inner.y, 1, inner.h), theme.input_bg);
                sx += 10;
            }
        }
    }

    /// Draw a `PBST_*` state-colored progress bar (`Normal`, `Paused`, `Error`).
    pub fn draw_with_state(
        surface: &mut Surface,
        rect: Rect,
        percent: u8,
        state: ProgressState,
        theme: &Theme,
    ) {
        let col = match state {
            ProgressState::Normal => theme.good,
            ProgressState::Paused => theme.warn,
            ProgressState::Error => theme.danger,
        };
        Self::draw(surface, rect, percent, col, theme);
    }

    /// Draw an indeterminate `PBS_MARQUEE` progress bar (`phase` ticks continuously).
    pub fn draw_marquee(surface: &mut Surface, rect: Rect, phase: i32, theme: &Theme) {
        if rect.w < 12 || rect.h < 6 {
            return;
        }
        surface.fill_rect(rect, theme.input_bg);
        surface.draw_rect(rect, theme.input_border);
        let inner = rect.inset(crate::gfx::Insets::all(2));
        let seg_w = (inner.w / 3).clamp(16, 84);
        let travel = inner.w + seg_w;
        let offset = (phase.rem_euclid(travel)) - seg_w;
        let seg = Rect::new(inner.x + offset, inner.y, seg_w, inner.h).intersect(inner);
        if !seg.is_empty() {
            surface.draw_gradient_v(seg, Color::lerp(theme.good, Color::WHITE, 64), theme.good);
        }
    }
}
