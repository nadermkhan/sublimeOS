use alloc::string::String;
use alloc::vec::Vec;
use crate::event::{EventResult, GuiEvent, KeyCode, MouseButton};
use crate::font::{with_default_font, TextStyle};
use crate::gfx::{Insets, Rect, Surface, Theme, ThemePart, ThemeState, UxTheme};

#[derive(Clone, Debug)]
pub struct TreeNode {
    pub id: u32,
    pub label: String,
    pub depth: u8,
    pub parent: Option<usize>,
    pub has_children: bool,
    pub expanded: bool,
}

impl TreeNode {
    pub fn new(id: u32, label: &str, depth: u8, parent: Option<usize>, has_children: bool, expanded: bool) -> Self {
        Self {
            id,
            label: String::from(label),
            depth,
            parent,
            has_children,
            expanded,
        }
    }
}

#[derive(Clone, Debug)]
pub struct TreeViewState {
    pub selected: Option<usize>,
    pub scroll_y: i32,
    pub row_height: i32,
    pub show_lines: bool,
    pub focused: bool,
}

impl Default for TreeViewState {
    fn default() -> Self {
        Self::new()
    }
}

impl TreeViewState {
    pub const fn new() -> Self {
        Self {
            selected: None,
            scroll_y: 0,
            row_height: 22,
            show_lines: true,
            focused: false,
        }
    }
}

/// ComCtl32 `SysTreeView32` hierarchical expandable tree widget.
pub struct TreeView;

impl TreeView {
    const INDENT: i32 = 16;
    const GLYPH_SZ: i32 = 11;

    /// Collect indices of nodes currently visible (all ancestors expanded).
    pub fn visible_indices(nodes: &[TreeNode]) -> Vec<usize> {
        let mut vis = Vec::new();
        for i in 0..nodes.len() {
            let mut shown = true;
            let mut p = nodes[i].parent;
            while let Some(pi) = p {
                if pi >= nodes.len() || !nodes[pi].expanded {
                    shown = false;
                    break;
                }
                p = nodes[pi].parent;
            }
            if shown {
                vis.push(i);
            }
        }
        vis
    }

    /// Returns `(EventResult, activated_node_index)`.
    pub fn handle_event(
        state: &mut TreeViewState,
        bounds: Rect,
        nodes: &mut [TreeNode],
        event: GuiEvent,
    ) -> (EventResult, Option<usize>) {
        let vis = Self::visible_indices(nodes);
        let total_h = vis.len() as i32 * state.row_height;
        let inner_h = (bounds.h - 2).max(1);
        let max_scroll = (total_h - inner_h).max(0);
        state.scroll_y = state.scroll_y.clamp(0, max_scroll);

        match event {
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
                )
            }
            GuiEvent::MouseDown { x, y, button: MouseButton::Left } => {
                let inside = bounds.contains(x, y);
                let fc = state.focused != inside;
                state.focused = inside;
                if !inside {
                    return (if fc { EventResult::NeedsRedraw } else { EventResult::Ignored }, None);
                }
                let rel_y = y - (bounds.y + 1) + state.scroll_y;
                if rel_y >= 0 {
                    let vrow = (rel_y / state.row_height) as usize;
                    if let Some(&node_idx) = vis.get(vrow) {
                        state.selected = Some(node_idx);
                        let gx = bounds.x + 6 + nodes[node_idx].depth as i32 * Self::INDENT;
                        if nodes[node_idx].has_children && x >= gx - 2 && x <= gx + Self::GLYPH_SZ + 4 {
                            nodes[node_idx].expanded = !nodes[node_idx].expanded;
                            return (EventResult::NeedsRedraw, None);
                        }
                        return (EventResult::NeedsRedraw, Some(node_idx));
                    }
                }
                (EventResult::NeedsRedraw, None)
            }
            GuiEvent::KeyDown { key, .. } if state.focused && !vis.is_empty() => {
                let cur_pos = state.selected.and_then(|s| vis.iter().position(|&v| v == s)).unwrap_or(0);
                match key {
                    KeyCode::Up => {
                        let next_pos = cur_pos.saturating_sub(1);
                        state.selected = Some(vis[next_pos]);
                        let row_y = next_pos as i32 * state.row_height;
                        if row_y < state.scroll_y {
                            state.scroll_y = row_y;
                        }
                        (EventResult::NeedsRedraw, state.selected)
                    }
                    KeyCode::Down => {
                        let next_pos = (cur_pos + 1).min(vis.len() - 1);
                        state.selected = Some(vis[next_pos]);
                        let row_bot = (next_pos as i32 + 1) * state.row_height;
                        if row_bot - state.scroll_y > inner_h {
                            state.scroll_y = (row_bot - inner_h).clamp(0, max_scroll);
                        }
                        (EventResult::NeedsRedraw, state.selected)
                    }
                    KeyCode::Right => {
                        let idx = vis[cur_pos];
                        if nodes[idx].has_children && !nodes[idx].expanded {
                            nodes[idx].expanded = true;
                            return (EventResult::NeedsRedraw, None);
                        }
                        (EventResult::Handled, None)
                    }
                    KeyCode::Left => {
                        let idx = vis[cur_pos];
                        if nodes[idx].has_children && nodes[idx].expanded {
                            nodes[idx].expanded = false;
                            return (EventResult::NeedsRedraw, None);
                        } else if let Some(p) = nodes[idx].parent {
                            state.selected = Some(p);
                            return (EventResult::NeedsRedraw, Some(p));
                        }
                        (EventResult::Handled, None)
                    }
                    _ => (EventResult::Ignored, None),
                }
            }
            _ => (EventResult::Ignored, None),
        }
    }

    pub fn draw(surface: &mut Surface, bounds: Rect, nodes: &[TreeNode], state: &TreeViewState, theme: &Theme) {
        if bounds.w < 24 || bounds.h < 24 {
            return;
        }
        UxTheme::draw_background(
            surface,
            ThemePart::Edit,
            if state.focused { ThemeState::Focused } else { ThemeState::Normal },
            bounds,
            theme,
        );
        let inner = bounds.inset(Insets::all(2));
        let vis = Self::visible_indices(nodes);

        surface.push_clip(inner);
        for (vrow, &idx) in vis.iter().enumerate() {
            let ry = inner.y + vrow as i32 * state.row_height - state.scroll_y;
            if ry + state.row_height < inner.y || ry >= inner.bottom() {
                continue;
            }
            let node = &nodes[idx];
            let selected = state.selected == Some(idx);
            let gx = inner.x + 6 + node.depth as i32 * Self::INDENT;
            let gy = ry + (state.row_height - Self::GLYPH_SZ) / 2;

            if state.show_lines && node.depth > 0 {
                let lx = gx - Self::INDENT / 2;
                let cy = ry + state.row_height / 2;
                surface.fill_rect(Rect::new(lx, cy, (gx - lx).max(1), 1), theme.border_shadow);
                surface.fill_rect(Rect::new(lx, ry, 1, state.row_height / 2 + 1), theme.border_shadow);
            }

            if node.has_children {
                let gstate = if node.expanded { ThemeState::Checked } else { ThemeState::Normal };
                UxTheme::draw_background(
                    surface,
                    ThemePart::TreeGlyph,
                    gstate,
                    Rect::new(gx, gy, Self::GLYPH_SZ, Self::GLYPH_SZ),
                    theme,
                );
            }

            let text_x = gx + Self::GLYPH_SZ + 6;
            let row_rect = Rect::new(text_x - 2, ry + 1, (inner.right() - text_x).max(0), state.row_height - 2);
            if selected {
                surface.fill_rect(row_rect, theme.selection);
            }
            let fg = if selected { theme.selection_text } else { theme.text_primary };
            with_default_font(|font| {
                let st = TextStyle::ui(fg);
                let (_, lh) = font.line_metrics(st);
                font.draw_text_ellipsis(surface, text_x + 2, ry + (state.row_height - lh) / 2, (row_rect.w - 4).max(0), &node.label, st);
            });
        }
        surface.pop_clip();
    }
}
