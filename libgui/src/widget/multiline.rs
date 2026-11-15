use alloc::string::String;
use alloc::vec::Vec;
use crate::event::{EventResult, GuiEvent, KeyCode, MouseButton};
use crate::font::{with_default_font, TextStyle};
use crate::gfx::{Insets, Rect, Surface, Theme, ThemePart, ThemeState, UxTheme};

#[derive(Clone, Debug)]
pub struct MultiLineEditState {
    pub lines: Vec<String>,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub scroll_row: usize,
    pub focused: bool,
}

impl Default for MultiLineEditState {
    fn default() -> Self {
        Self::new("")
    }
}

impl MultiLineEditState {
    pub fn new(initial: &str) -> Self {
        let mut lines: Vec<String> = initial.split('\n').map(String::from).collect();
        if lines.is_empty() {
            lines.push(String::new());
        }
        Self {
            lines,
            cursor_row: 0,
            cursor_col: 0,
            scroll_row: 0,
            focused: false,
        }
    }

    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    fn clamp_cursor(&mut self) {
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.cursor_row = self.cursor_row.min(self.lines.len() - 1);
        let len = self.lines[self.cursor_row].chars().count();
        self.cursor_col = self.cursor_col.min(len);
    }

    fn byte_offset_for_char_col(line: &str, col: usize) -> usize {
        line.char_indices().nth(col).map(|(b, _)| b).unwrap_or(line.len())
    }
}

/// Win32 `EDIT` (`ES_MULTILINE`) scrollable multi-line text editor control.
pub struct MultiLineEdit;

impl MultiLineEdit {
    const LINE_H: i32 = 18;
    const SCROLLBAR_W: i32 = 12;

    pub fn handle_event(state: &mut MultiLineEditState, bounds: Rect, event: GuiEvent) -> (EventResult, bool) {
        state.clamp_cursor();
        let inner = bounds.inset(Insets::all(4));
        let vis_rows = (inner.h / Self::LINE_H).max(1) as usize;
        let max_scroll = state.lines.len().saturating_sub(vis_rows);
        state.scroll_row = state.scroll_row.min(max_scroll);

        match event {
            GuiEvent::MouseWheel { x, y, delta_y } if bounds.contains(x, y) => {
                let prev = state.scroll_row;
                if delta_y > 0 {
                    state.scroll_row = state.scroll_row.saturating_sub(1);
                } else if delta_y < 0 {
                    state.scroll_row = (state.scroll_row + 1).min(max_scroll);
                }
                (
                    if state.scroll_row != prev { EventResult::NeedsRedraw } else { EventResult::Handled },
                    false,
                )
            }
            GuiEvent::MouseDown { x, y, button: MouseButton::Left } => {
                let inside = bounds.contains(x, y);
                let fc = state.focused != inside;
                state.focused = inside;
                if inside {
                    let row = (state.scroll_row + ((y - inner.y).max(0) / Self::LINE_H) as usize)
                        .min(state.lines.len() - 1);
                    state.cursor_row = row;
                    state.cursor_col = (((x - inner.x).max(0) + 3) / 7) as usize;
                    state.clamp_cursor();
                    return (EventResult::NeedsRedraw, false);
                }
                (if fc { EventResult::NeedsRedraw } else { EventResult::Ignored }, false)
            }
            GuiEvent::KeyDown { key, ch, .. } if state.focused => {
                let mut changed = false;
                match key {
                    KeyCode::Left => {
                        if state.cursor_col > 0 {
                            state.cursor_col -= 1;
                        } else if state.cursor_row > 0 {
                            state.cursor_row -= 1;
                            state.cursor_col = state.lines[state.cursor_row].chars().count();
                        }
                    }
                    KeyCode::Right => {
                        let len = state.lines[state.cursor_row].chars().count();
                        if state.cursor_col < len {
                            state.cursor_col += 1;
                        } else if state.cursor_row + 1 < state.lines.len() {
                            state.cursor_row += 1;
                            state.cursor_col = 0;
                        }
                    }
                    KeyCode::Up => {
                        state.cursor_row = state.cursor_row.saturating_sub(1);
                    }
                    KeyCode::Down => {
                        if state.cursor_row + 1 < state.lines.len() {
                            state.cursor_row += 1;
                        }
                    }
                    KeyCode::Home => state.cursor_col = 0,
                    KeyCode::End => state.cursor_col = state.lines[state.cursor_row].chars().count(),
                    KeyCode::Enter => {
                        let row = state.cursor_row;
                        let byte = MultiLineEditState::byte_offset_for_char_col(&state.lines[row], state.cursor_col);
                        let tail = state.lines[row].split_off(byte);
                        state.lines.insert(row + 1, tail);
                        state.cursor_row += 1;
                        state.cursor_col = 0;
                        changed = true;
                    }
                    KeyCode::Backspace => {
                        if state.cursor_col > 0 {
                            let row = state.cursor_row;
                            let prev_byte = MultiLineEditState::byte_offset_for_char_col(&state.lines[row], state.cursor_col - 1);
                            state.lines[row].remove(prev_byte);
                            state.cursor_col -= 1;
                            changed = true;
                        } else if state.cursor_row > 0 {
                            let row = state.cursor_row;
                            let tail = state.lines.remove(row);
                            state.cursor_row -= 1;
                            state.cursor_col = state.lines[state.cursor_row].chars().count();
                            state.lines[state.cursor_row].push_str(&tail);
                            changed = true;
                        }
                    }
                    KeyCode::Delete => {
                        let row = state.cursor_row;
                        let len = state.lines[row].chars().count();
                        if state.cursor_col < len {
                            let b = MultiLineEditState::byte_offset_for_char_col(&state.lines[row], state.cursor_col);
                            state.lines[row].remove(b);
                            changed = true;
                        } else if row + 1 < state.lines.len() {
                            let tail = state.lines.remove(row + 1);
                            state.lines[row].push_str(&tail);
                            changed = true;
                        }
                    }
                    KeyCode::Char(c) if !c.is_control() => {
                        let row = state.cursor_row;
                        let b = MultiLineEditState::byte_offset_for_char_col(&state.lines[row], state.cursor_col);
                        state.lines[row].insert(b, c);
                        state.cursor_col += 1;
                        changed = true;
                    }
                    _ => {
                        if let Some(c) = ch {
                            if !c.is_control() {
                                let row = state.cursor_row;
                                let b = MultiLineEditState::byte_offset_for_char_col(&state.lines[row], state.cursor_col);
                                state.lines[row].insert(b, c);
                                state.cursor_col += 1;
                                changed = true;
                            }
                        } else {
                            return (EventResult::Ignored, false);
                        }
                    }
                }
                state.clamp_cursor();
                if state.cursor_row < state.scroll_row {
                    state.scroll_row = state.cursor_row;
                } else if state.cursor_row >= state.scroll_row + vis_rows {
                    state.scroll_row = state.cursor_row + 1 - vis_rows;
                }
                (EventResult::NeedsRedraw, changed)
            }
            _ => (EventResult::Ignored, false),
        }
    }

    pub fn draw(
        surface: &mut Surface,
        bounds: Rect,
        state: &mut MultiLineEditState,
        cursor_visible: bool,
        theme: &Theme,
    ) {
        if bounds.w < 24 || bounds.h < 24 {
            return;
        }
        state.clamp_cursor();
        UxTheme::draw_background(
            surface,
            ThemePart::Edit,
            if state.focused { ThemeState::Focused } else { ThemeState::Normal },
            bounds,
            theme,
        );

        let inner = bounds.inset(Insets::all(4));
        let vis_rows = (inner.h / Self::LINE_H).max(1) as usize;
        let has_sb = state.lines.len() > vis_rows;
        let text_w = if has_sb { (inner.w - Self::SCROLLBAR_W - 2).max(0) } else { inner.w };
        let clip_r = Rect::new(inner.x, inner.y, text_w, inner.h);

        surface.push_clip(clip_r);
        let end = (state.scroll_row + vis_rows + 1).min(state.lines.len());
        for row in state.scroll_row..end {
            let ry = inner.y + (row - state.scroll_row) as i32 * Self::LINE_H;
            let line = &state.lines[row];
            with_default_font(|font| {
                let st = TextStyle::ui(theme.text_primary);
                font.draw_text(surface, inner.x + 2, ry + 1, line, st);
                if state.focused && cursor_visible && row == state.cursor_row {
                    let byte = MultiLineEditState::byte_offset_for_char_col(line, state.cursor_col);
                    let (cx_off, _) = font.measure_text(&line[..byte], st);
                    surface.fill_rect(Rect::new(inner.x + 2 + cx_off, ry + 1, 1, Self::LINE_H - 2), theme.text_primary);
                }
            });
        }
        surface.pop_clip();

        if has_sb {
            let sb = Rect::new(inner.x + text_w + 2, inner.y, Self::SCROLLBAR_W, inner.h);
            UxTheme::draw_background(surface, ThemePart::ScrollTrack, ThemeState::Normal, sb, theme);
            let max_scroll = (state.lines.len() - vis_rows).max(1);
            let th = ((vis_rows as i64 * sb.h as i64) / state.lines.len().max(1) as i64).clamp(16, sb.h as i64) as i32;
            let ty = sb.y + ((state.scroll_row as i64 * (sb.h - th) as i64) / max_scroll as i64) as i32;
            UxTheme::draw_background(
                surface,
                ThemePart::ScrollThumb,
                ThemeState::Normal,
                Rect::new(sb.x + 2, ty + 1, sb.w - 4, (th - 2).max(10)),
                theme,
            );
        }
    }
}
