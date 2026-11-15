use alloc::string::String;
use crate::event::{EventResult, GuiEvent, KeyCode, MouseButton};
use crate::font::{with_default_font, TextStyle};
use crate::gfx::{Rect, Surface, Theme};

#[derive(Clone, Debug, Default)]
pub struct TextInputState {
    pub text: String,
    pub cursor_bytes: usize,
    pub focused: bool,
    pub scroll_x: i32,
}

impl TextInputState {
    pub fn new(initial: &str) -> Self {
        let s = String::from(initial);
        let len = s.len();
        Self {
            text: s,
            cursor_bytes: len,
            focused: false,
            scroll_x: 0,
        }
    }

    pub fn set_text(&mut self, text: &str) {
        self.text.clear();
        self.text.push_str(text);
        self.cursor_bytes = self.text.len();
        self.scroll_x = 0;
    }

    fn clamp_cursor(&mut self) {
        self.cursor_bytes = self.cursor_bytes.min(self.text.len());
        while self.cursor_bytes > 0 && !self.text.is_char_boundary(self.cursor_bytes) {
            self.cursor_bytes -= 1;
        }
    }

    fn prev_char_boundary(&self, pos: usize) -> usize {
        let mut p = pos.saturating_sub(1);
        while p > 0 && !self.text.is_char_boundary(p) {
            p -= 1;
        }
        p
    }

    fn next_char_boundary(&self, pos: usize) -> usize {
        if pos >= self.text.len() {
            return self.text.len();
        }
        let mut p = pos + 1;
        while p < self.text.len() && !self.text.is_char_boundary(p) {
            p += 1;
        }
        p
    }
}

pub struct TextInput;

impl TextInput {
    /// Returns `(result, submitted_enter)`.
    pub fn handle_event(
        state: &mut TextInputState,
        bounds: Rect,
        event: GuiEvent,
    ) -> (EventResult, bool) {
        state.clamp_cursor();
        match event {
            GuiEvent::MouseDown { x, y, button: MouseButton::Left } => {
                let inside = bounds.contains(x, y);
                let changed = state.focused != inside;
                state.focused = inside;
                if inside {
                    // Approximate caret placement by measuring prefix width
                    let rel_x = (x - (bounds.x + 6) + state.scroll_x).max(0);
                    let mut best = state.text.len();
                    with_default_font(|font| {
                        let st = TextStyle::ui(crate::gfx::Color::BLACK);
                        for (idx, ch) in state.text.char_indices() {
                            let (pw, _) = font.measure_text(&state.text[..idx + ch.len_utf8()], st);
                            if pw >= rel_x {
                                best = idx;
                                return;
                            }
                        }
                    });
                    state.cursor_bytes = best;
                    return (EventResult::NeedsRedraw, false);
                }
                if changed {
                    (EventResult::NeedsRedraw, false)
                } else {
                    (EventResult::Ignored, false)
                }
            }
            GuiEvent::KeyDown { key, ch, mods } => {
                if !state.focused || mods.ctrl || mods.alt {
                    return (EventResult::Ignored, false);
                }
                match key {
                    KeyCode::Enter => return (EventResult::NeedsRedraw, true),
                    KeyCode::Backspace => {
                        if state.cursor_bytes > 0 {
                            let prev = state.prev_char_boundary(state.cursor_bytes);
                            state.text.drain(prev..state.cursor_bytes);
                            state.cursor_bytes = prev;
                            return (EventResult::NeedsRedraw, false);
                        }
                        return (EventResult::Handled, false);
                    }
                    KeyCode::Delete => {
                        if state.cursor_bytes < state.text.len() {
                            let next = state.next_char_boundary(state.cursor_bytes);
                            state.text.drain(state.cursor_bytes..next);
                            return (EventResult::NeedsRedraw, false);
                        }
                        return (EventResult::Handled, false);
                    }
                    KeyCode::Left => {
                        state.cursor_bytes = state.prev_char_boundary(state.cursor_bytes);
                        return (EventResult::NeedsRedraw, false);
                    }
                    KeyCode::Right => {
                        state.cursor_bytes = state.next_char_boundary(state.cursor_bytes);
                        return (EventResult::NeedsRedraw, false);
                    }
                    KeyCode::Home => {
                        state.cursor_bytes = 0;
                        return (EventResult::NeedsRedraw, false);
                    }
                    KeyCode::End => {
                        state.cursor_bytes = state.text.len();
                        return (EventResult::NeedsRedraw, false);
                    }
                    KeyCode::Char(c) if !c.is_control() => {
                        let mut buf = [0u8; 4];
                        let s = c.encode_utf8(&mut buf);
                        state.text.insert_str(state.cursor_bytes, s);
                        state.cursor_bytes += s.len();
                        return (EventResult::NeedsRedraw, false);
                    }
                    _ => {
                        if let Some(c) = ch {
                            if !c.is_control() {
                                let mut buf = [0u8; 4];
                                let s = c.encode_utf8(&mut buf);
                                state.text.insert_str(state.cursor_bytes, s);
                                state.cursor_bytes += s.len();
                                return (EventResult::NeedsRedraw, false);
                            }
                        }
                    }
                }
                (EventResult::Ignored, false)
            }
            _ => (EventResult::Ignored, false),
        }
    }

    pub fn draw(
        surface: &mut Surface,
        bounds: Rect,
        state: &mut TextInputState,
        placeholder: &str,
        cursor_visible: bool,
        theme: &Theme,
    ) {
        if bounds.w < 12 || bounds.h < 12 {
            return;
        }
        state.clamp_cursor();
        surface.fill_rect(bounds, theme.input_bg);
        let border = if state.focused { theme.frame_active } else { theme.input_border };
        surface.draw_rect(bounds, border);

        let inner = bounds.inset(crate::gfx::Insets::hv(6, 2));
        surface.push_clip(inner);
        with_default_font(|font| {
            let st = TextStyle::ui(theme.text_primary);
            let (_, lh) = font.line_metrics(st);
            let ty = inner.y + ((inner.h - lh) / 2);

            if state.text.is_empty() && !placeholder.is_empty() {
                let pst = TextStyle::ui(theme.text_secondary);
                font.draw_text(surface, inner.x, ty, placeholder, pst);
            } else {
                let (caret_x, _) = font.measure_text(&state.text[..state.cursor_bytes], st);
                if caret_x - state.scroll_x > inner.w - 4 {
                    state.scroll_x = (caret_x - inner.w + 8).max(0);
                } else if caret_x - state.scroll_x < 0 {
                    state.scroll_x = caret_x.max(0);
                }
                font.draw_text(surface, inner.x - state.scroll_x, ty, &state.text, st);
                if state.focused && cursor_visible {
                    let cx = inner.x + caret_x - state.scroll_x;
                    surface.fill_rect(Rect::new(cx, inner.y + 2, 1, (inner.h - 4).max(8)), theme.text_primary);
                }
            }
        });
        surface.pop_clip();
    }
}
