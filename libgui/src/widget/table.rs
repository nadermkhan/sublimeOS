use alloc::string::String;
use crate::event::{EventResult, GuiEvent, MouseButton};
use crate::font::{with_default_font, TextStyle};
use crate::gfx::{ArrowDir, Color, Rect, Surface, Theme, ThemePart, ThemeState, UxTheme};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SortOrder {
    None,
    Ascending,
    Descending,
}

#[derive(Clone, Debug)]
pub struct ColumnSpec {
    pub title: String,
    pub width: i32,
    pub min_width: i32,
}

impl ColumnSpec {
    pub fn new(title: &str, width: i32) -> Self {
        Self {
            title: String::from(title),
            width: width.max(36),
            min_width: 36,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ReportListState {
    pub scroll_y: i32,
    pub selected: Option<usize>,
    pub sort_col: Option<usize>,
    pub sort_order: SortOrder,
    pub hovered_col: Option<usize>,
    pub resizing_col: Option<usize>,
    pub resize_start_x: i32,
    pub resize_orig_w: i32,
    pub row_height: i32,
}

impl Default for ReportListState {
    fn default() -> Self {
        Self::new()
    }
}

impl ReportListState {
    pub const fn new() -> Self {
        Self {
            scroll_y: 0,
            selected: None,
            sort_col: None,
            sort_order: SortOrder::None,
            hovered_col: None,
            resizing_col: None,
            resize_start_x: 0,
            resize_orig_w: 0,
            row_height: 22,
        }
    }
}

/// ComCtl32 `SysHeader32` + `SysListView32` (`LVS_REPORT`) sortable, column-resizable report table.
pub struct ReportListView;

impl ReportListView {
    const HEADER_H: i32 = 24;
    const SCROLLBAR_W: i32 = 14;

    fn header_rect(bounds: Rect) -> Rect {
        Rect::new(bounds.x + 1, bounds.y + 1, (bounds.w - 2).max(0), Self::HEADER_H)
    }

    fn body_rect(bounds: Rect) -> Rect {
        Rect::new(bounds.x + 1, bounds.y + 1 + Self::HEADER_H, (bounds.w - 2).max(0), (bounds.h - 2 - Self::HEADER_H).max(0))
    }

    /// Returns `(EventResult, clicked_row, clicked_sort_column)`.
    pub fn handle_event(
        state: &mut ReportListState,
        bounds: Rect,
        cols: &mut [ColumnSpec],
        row_count: usize,
        event: GuiEvent,
    ) -> (EventResult, Option<usize>, Option<usize>) {
        let hdr = Self::header_rect(bounds);
        let body = Self::body_rect(bounds);
        let total_h = row_count as i32 * state.row_height;
        let max_scroll = (total_h - body.h).max(0);
        state.scroll_y = state.scroll_y.clamp(0, max_scroll);

        match event {
            GuiEvent::MouseMove { x, y } => {
                if let Some(ci) = state.resizing_col {
                    if ci < cols.len() {
                        cols[ci].width = (state.resize_orig_w + (x - state.resize_start_x)).max(cols[ci].min_width);
                        return (EventResult::NeedsRedraw, None, None);
                    }
                }
                let mut hcol = None;
                if hdr.contains(x, y) {
                    let mut cx = hdr.x;
                    for (i, c) in cols.iter().enumerate() {
                        if x >= cx && x < cx + c.width {
                            hcol = Some(i);
                            break;
                        }
                        cx += c.width;
                    }
                }
                if hcol != state.hovered_col {
                    state.hovered_col = hcol;
                    return (EventResult::NeedsRedraw, None, None);
                }
                (EventResult::Ignored, None, None)
            }
            GuiEvent::MouseUp { button: MouseButton::Left, .. } => {
                if state.resizing_col.is_some() {
                    state.resizing_col = None;
                    return (EventResult::NeedsRedraw, None, None);
                }
                (EventResult::Ignored, None, None)
            }
            GuiEvent::MouseDown { x, y, button: MouseButton::Left } => {
                if hdr.contains(x, y) {
                    let mut cx = hdr.x;
                    for (i, c) in cols.iter().enumerate() {
                        let right_edge = cx + c.width;
                        if (x - right_edge).abs() <= 4 {
                            state.resizing_col = Some(i);
                            state.resize_start_x = x;
                            state.resize_orig_w = c.width;
                            return (EventResult::NeedsRedraw, None, None);
                        }
                        if x >= cx && x < right_edge {
                            if state.sort_col == Some(i) {
                                state.sort_order = match state.sort_order {
                                    SortOrder::Ascending => SortOrder::Descending,
                                    _ => SortOrder::Ascending,
                                };
                            } else {
                                state.sort_col = Some(i);
                                state.sort_order = SortOrder::Ascending;
                            }
                            return (EventResult::NeedsRedraw, None, Some(i));
                        }
                        cx = right_edge;
                    }
                    return (EventResult::Handled, None, None);
                }
                if body.contains(x, y) {
                    let has_sb = total_h > body.h;
                    let content_w = if has_sb { body.w - Self::SCROLLBAR_W } else { body.w };
                    if x < body.x + content_w {
                        let row = ((y - body.y + state.scroll_y) / state.row_height) as usize;
                        if row < row_count {
                            state.selected = Some(row);
                            return (EventResult::NeedsRedraw, Some(row), None);
                        }
                    }
                }
                (EventResult::Ignored, None, None)
            }
            GuiEvent::MouseWheel { x, y, delta_y } if bounds.contains(x, y) && max_scroll > 0 => {
                let prev = state.scroll_y;
                state.scroll_y = (state.scroll_y - delta_y * state.row_height * 2).clamp(0, max_scroll);
                (
                    if state.scroll_y != prev {
                        EventResult::NeedsRedraw
                    } else {
                        EventResult::Handled
                    },
                    None,
                    None,
                )
            }
            _ => (EventResult::Ignored, None, None),
        }
    }

    pub fn draw<F>(
        surface: &mut Surface,
        bounds: Rect,
        cols: &[ColumnSpec],
        row_count: usize,
        state: &ReportListState,
        theme: &Theme,
        mut cell_text: F,
    ) where
        F: FnMut(usize, usize) -> String,
    {
        if bounds.w < 32 || bounds.h < 32 {
            return;
        }
        surface.fill_rect(bounds, theme.input_bg);
        surface.draw_rect(bounds, theme.input_border);

        let hdr = Self::header_rect(bounds);
        let body = Self::body_rect(bounds);
        let total_h = row_count as i32 * state.row_height;
        let has_sb = total_h > body.h;
        let content_w = if has_sb { (body.w - Self::SCROLLBAR_W).max(0) } else { body.w };

        // Draw SysHeader32 columns
        surface.push_clip(hdr);
        surface.fill_rect(hdr, theme.face);
        let mut cx = hdr.x;
        for (ci, col) in cols.iter().enumerate() {
            if cx >= hdr.right() {
                break;
            }
            let hr = Rect::new(cx, hdr.y, col.width, hdr.h);
            let hstate = if state.resizing_col == Some(ci) {
                ThemeState::Pressed
            } else if state.hovered_col == Some(ci) {
                ThemeState::Hot
            } else {
                ThemeState::Normal
            };
            UxTheme::draw_background(surface, ThemePart::HeaderSection, hstate, hr, theme);
            let sort_space = if state.sort_col == Some(ci) { 14 } else { 0 };
            let text_w = (hr.w - 10 - sort_space).max(0);
            surface.push_clip(hr);
            with_default_font(|font| {
                let st = TextStyle::ui(theme.text_primary);
                let (_, lh) = font.line_metrics(st);
                font.draw_text_ellipsis(surface, hr.x + 6, hr.y + (hr.h - lh) / 2, text_w, &col.title, st);
            });
            if state.sort_col == Some(ci) && state.sort_order != SortOrder::None && hr.w >= 20 {
                let dir = match state.sort_order {
                    SortOrder::Descending => ArrowDir::Down,
                    _ => ArrowDir::Up,
                };
                surface.draw_arrow(Rect::new(hr.right() - 14, hr.y + 4, 10, hr.h - 8), dir, theme.text_secondary);
            }
            surface.pop_clip();
            cx += col.width;
        }
        surface.pop_clip();

        // Draw rows
        let body_clip = Rect::new(body.x, body.y, content_w, body.h);
        surface.push_clip(body_clip);
        let first = (state.scroll_y / state.row_height).max(0) as usize;
        let vis_rows = (body.h / state.row_height + 2) as usize;
        let end = (first + vis_rows).min(row_count);
        for r in first..end {
            let ry = body.y + r as i32 * state.row_height - state.scroll_y;
            let row_r = Rect::new(body.x, ry, content_w, state.row_height);
            let selected = state.selected == Some(r);
            if selected {
                surface.fill_rect(row_r, theme.selection);
            } else if r % 2 == 1 {
                surface.fill_rect(row_r, Color::lerp(theme.input_bg, theme.face, 28));
            }
            let fg = if selected { theme.selection_text } else { theme.text_primary };
            let mut rx = body.x;
            for (ci, col) in cols.iter().enumerate() {
                if rx >= body_clip.right() {
                    break;
                }
                let cell_r = Rect::new(rx, ry, col.width, state.row_height);
                let txt = cell_text(r, ci);
                surface.push_clip(cell_r);
                with_default_font(|font| {
                    let st = TextStyle::ui(fg);
                    let (_, lh) = font.line_metrics(st);
                    font.draw_text_ellipsis(surface, rx + 6, ry + (state.row_height - lh) / 2, (col.width - 10).max(0), &txt, st);
                });
                surface.pop_clip();
                rx += col.width;
            }
        }
        surface.pop_clip();

        if has_sb {
            let sb = Rect::new(body.x + content_w, body.y, Self::SCROLLBAR_W, body.h);
            UxTheme::draw_background(surface, ThemePart::ScrollTrack, ThemeState::Normal, sb, theme);
            let max_scroll = (total_h - body.h).max(1);
            let th = ((body.h as i64 * body.h as i64) / total_h.max(1) as i64).clamp(20, body.h as i64) as i32;
            let travel = (body.h - th).max(1);
            let ty = sb.y + ((state.scroll_y as i64 * travel as i64) / max_scroll as i64) as i32;
            UxTheme::draw_background(
                surface,
                ThemePart::ScrollThumb,
                ThemeState::Normal,
                Rect::new(sb.x + 2, ty + 1, sb.w - 4, (th - 2).max(12)),
                theme,
            );
        }
    }
}
