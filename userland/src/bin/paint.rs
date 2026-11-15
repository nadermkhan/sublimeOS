#![no_std]
#![no_main]

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use libgui::app::{WindowClient, WindowSpec};
use libgui::component::ComponentContext;
use libgui::event::{EventResult, GuiEvent, KeyCode, MouseButton};
use libgui::font::{with_default_font, TextStyle};
use libgui::gfx::{contrast_text_on, current_theme, Color, Rect, Surface, Theme, ThemeKind};
use sublime_rt::{getpid, println, write_file, SublimeWmOs};

#[derive(Clone, Copy, PartialEq, Eq)]
enum PaintTool {
    Brush,
    Eraser,
    Line,
    Box,
}

struct PaintApp {
    ctx: ComponentContext,
    tool: PaintTool,
    brush_radius: i32,
    active_color: u32,
    canvas_w: usize,
    canvas_h: usize,
    pixels: Vec<u32>,
    drawing: bool,
    start_px: (i32, i32),
    last_px: (i32, i32),
    save_seq: u32,
    status: String,
    dirty_all: bool,
}

impl PaintApp {
    fn new() -> Self {
        Self {
            ctx: ComponentContext::new(),
            tool: PaintTool::Brush,
            brush_radius: 4,
            active_color: 0xFF00_78D7,
            canvas_w: 520,
            canvas_h: 300,
            pixels: vec![0xFFFF_FFFF; 520 * 300],
            drawing: false,
            start_px: (0, 0),
            last_px: (0, 0),
            save_seq: 1,
            status: String::from("Ready — Left drag to paint, Save BMP writes /Users/sublime/Pictures/"),
            dirty_all: true,
        }
    }

    fn ensure_canvas_size(&mut self, w: i32, h: i32) {
        let nw = (w - 16).max(64) as usize;
        let nh = (h - 74).max(64) as usize;
        if nw == self.canvas_w && nh == self.canvas_h {
            return;
        }
        let mut next = vec![0xFFFF_FFFFu32; nw * nh];
        let cw = nw.min(self.canvas_w);
        let ch = nh.min(self.canvas_h);
        for y in 0..ch {
            let src = y * self.canvas_w;
            let dst = y * nw;
            next[dst..dst + cw].copy_from_slice(&self.pixels[src..src + cw]);
        }
        self.canvas_w = nw;
        self.canvas_h = nh;
        self.pixels = next;
    }

    fn clear_canvas(&mut self) {
        for p in &mut self.pixels {
            *p = 0xFFFF_FFFF;
        }
        self.status = String::from("Canvas cleared");
    }

    fn plot_brush(&mut self, cx: i32, cy: i32, col: u32, r: i32) {
        let r2 = r * r;
        let w = self.canvas_w as i32;
        let h = self.canvas_h as i32;
        for dy in -r..=r {
            let py = cy + dy;
            if py < 0 || py >= h {
                continue;
            }
            for dx in -r..=r {
                if dx * dx + dy * dy > r2 {
                    continue;
                }
                let px = cx + dx;
                if px < 0 || px >= w {
                    continue;
                }
                self.pixels[py as usize * self.canvas_w + px as usize] = col;
            }
        }
    }

    fn draw_stroke(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, col: u32, r: i32) {
        let mut x = x0;
        let mut y = y0;
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        loop {
            self.plot_brush(x, y, col, r);
            if x == x1 && y == y1 {
                break;
            }
            let e2 = err * 2;
            if e2 >= dy {
                err += dy;
                x += sx;
            }
            if e2 <= dx {
                err += dx;
                y += sy;
            }
        }
    }

    fn draw_rect_outline(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, col: u32, thick: i32) {
        let lx = x0.min(x1);
        let rx = x0.max(x1);
        let ty = y0.min(y1);
        let by = y0.max(y1);
        let r = (thick / 2).max(1);
        self.draw_stroke(lx, ty, rx, ty, col, r);
        self.draw_stroke(lx, by, rx, by, col, r);
        self.draw_stroke(lx, ty, lx, by, col, r);
        self.draw_stroke(rx, ty, rx, by, col, r);
    }

    fn save_bmp(&mut self) {
        let w = self.canvas_w;
        let h = self.canvas_h;
        let row_bytes = w * 3;
        let pad = (4 - (row_bytes % 4)) % 4;
        let stride = row_bytes + pad;
        let pix_bytes = stride * h;
        let total_size = 54 + pix_bytes;
        let mut out = Vec::with_capacity(total_size);
        // BMP File Header (14 bytes)
        out.extend_from_slice(b"BM");
        out.extend_from_slice(&(total_size as u32).to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&54u32.to_le_bytes());
        // DIB Header (40 bytes)
        out.extend_from_slice(&40u32.to_le_bytes());
        out.extend_from_slice(&(w as i32).to_le_bytes());
        out.extend_from_slice(&(h as i32).to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&24u16.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&(pix_bytes as u32).to_le_bytes());
        out.extend_from_slice(&2835u32.to_le_bytes());
        out.extend_from_slice(&2835u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());

        for y_rev in 0..h {
            let y = h - 1 - y_rev;
            let row_start = y * w;
            for x in 0..w {
                let p = self.pixels[row_start + x];
                out.push((p & 0xFF) as u8);
                out.push(((p >> 8) & 0xFF) as u8);
                out.push(((p >> 16) & 0xFF) as u8);
            }
            for _ in 0..pad {
                out.push(0);
            }
        }

        let path = format!("/Users/sublime/Pictures/Painting_{}.bmp", self.save_seq);
        self.save_seq += 1;
        match write_file(&path, &out) {
            Ok(()) => {
                self.status = format!("Saved {} ({}x{}, {} KB)", path, w, h, out.len() / 1024);
            }
            Err(e) => {
                self.status = format!("Save error ({}): {}", e, path);
            }
        }
    }
}

impl WindowClient for PaintApp {
    fn spec(&self) -> WindowSpec {
        WindowSpec {
            title: String::from("Sublime Paint (Ring-3 App)"),
            width: 560,
            height: 390,
            min_width: 380,
            min_height: 280,
            resizable: true,
        }
    }

    fn on_mount(&mut self, bounds: Rect) {
        self.active_color = current_theme().accent.to_u32();
        self.ensure_canvas_size(bounds.w, bounds.h);
        self.dirty_all = true;
    }

    fn on_theme_changed(&mut self, _theme: ThemeKind) {
        self.dirty_all = true;
    }

    fn on_event(&mut self, event: GuiEvent, bounds: Rect) -> EventResult {
        self.ensure_canvas_size(bounds.w, bounds.h);
        let tb_h = 44;
        let canvas_rect = Rect::new(
            bounds.x + 8,
            bounds.y + tb_h + 4,
            self.canvas_w as i32,
            self.canvas_h as i32,
        );

        match event {
            GuiEvent::Resize { width, height } => {
                self.ensure_canvas_size(width, height);
                self.ctx.damage.invalidate(bounds);
                return EventResult::NeedsRedraw;
            }
            GuiEvent::KeyDown { key, ch, .. } => {
                if matches!(key, KeyCode::Char('c') | KeyCode::Char('C')) || ch == Some('c') {
                    self.clear_canvas();
                    self.ctx.damage.invalidate(bounds);
                    return EventResult::NeedsRedraw;
                }
                if matches!(key, KeyCode::Char('s') | KeyCode::Char('S')) || ch == Some('s') {
                    self.save_bmp();
                    self.ctx.damage.invalidate(bounds);
                    return EventResult::NeedsRedraw;
                }
                return EventResult::Ignored;
            }
            GuiEvent::MouseDown {
                x,
                y,
                button: MouseButton::Left,
            } => {
                // Toolbar row buttons
                if y >= bounds.y + 6 && y < bounds.y + 30 {
                    let mut bx = bounds.x + 8;
                    let tools = [
                        ("Brush", PaintTool::Brush, 46),
                        ("Eraser", PaintTool::Eraser, 50),
                        ("Line", PaintTool::Line, 40),
                        ("Box", PaintTool::Box, 38),
                    ];
                    for &(_, t, w) in &tools {
                        if x >= bx && x < bx + w {
                            self.tool = t;
                            self.ctx.damage.invalidate(bounds);
                            return EventResult::NeedsRedraw;
                        }
                        bx += w + 4;
                    }
                    bx += 4;
                    for &r in &[2i32, 5, 10] {
                        if x >= bx && x < bx + 26 {
                            self.brush_radius = r;
                            self.ctx.damage.invalidate(bounds);
                            return EventResult::NeedsRedraw;
                        }
                        bx += 28;
                    }
                    bx += 4;
                    let theme_acc = current_theme().accent.to_u32();
                    let palette = [
                        theme_acc,
                        0xFF1A_1C22,
                        0xFFD8_3B01,
                        0xFF10_7C10,
                        0xFF00_78D7,
                        0xFFFF_B900,
                        0xFF88_1798,
                    ];
                    for &col in &palette {
                        if x >= bx && x < bx + 16 && y >= bounds.y + 10 && y < bounds.y + 26 {
                            self.active_color = col;
                            if self.tool == PaintTool::Eraser {
                                self.tool = PaintTool::Brush;
                            }
                            self.ctx.damage.invalidate(bounds);
                            return EventResult::NeedsRedraw;
                        }
                        bx += 18;
                    }

                    // Right-side Clear & Save BMP buttons
                    let save_x = bounds.x + bounds.w - 74;
                    let clr_x = save_x - 48;
                    if x >= clr_x && x < clr_x + 44 {
                        self.clear_canvas();
                        self.ctx.damage.invalidate(bounds);
                        return EventResult::NeedsRedraw;
                    }
                    if x >= save_x && x < save_x + 66 {
                        self.save_bmp();
                        self.ctx.damage.invalidate(bounds);
                        return EventResult::NeedsRedraw;
                    }
                }

                if canvas_rect.contains(x, y) {
                    let px = x - canvas_rect.x;
                    let py = y - canvas_rect.y;
                    self.drawing = true;
                    self.start_px = (px, py);
                    self.last_px = (px, py);
                    if self.tool == PaintTool::Brush || self.tool == PaintTool::Eraser {
                        let col = if self.tool == PaintTool::Eraser {
                            0xFFFF_FFFF
                        } else {
                            self.active_color
                        };
                        self.plot_brush(px, py, col, self.brush_radius);
                    }
                    self.ctx.damage.invalidate(canvas_rect);
                    return EventResult::NeedsRedraw;
                }
            }
            GuiEvent::MouseMove { x, y } => {
                if self.drawing {
                    let px = (x - canvas_rect.x).clamp(0, self.canvas_w as i32 - 1);
                    let py = (y - canvas_rect.y).clamp(0, self.canvas_h as i32 - 1);
                    if self.tool == PaintTool::Brush || self.tool == PaintTool::Eraser {
                        let col = if self.tool == PaintTool::Eraser {
                            0xFFFF_FFFF
                        } else {
                            self.active_color
                        };
                        self.draw_stroke(self.last_px.0, self.last_px.1, px, py, col, self.brush_radius);
                        self.last_px = (px, py);
                        self.ctx.damage.invalidate(canvas_rect);
                        return EventResult::NeedsRedraw;
                    } else {
                        self.last_px = (px, py);
                        self.ctx.damage.invalidate(canvas_rect);
                        return EventResult::NeedsRedraw;
                    }
                }
            }
            GuiEvent::MouseUp {
                x,
                y,
                button: MouseButton::Left,
            } => {
                if self.drawing {
                    let px = (x - canvas_rect.x).clamp(0, self.canvas_w as i32 - 1);
                    let py = (y - canvas_rect.y).clamp(0, self.canvas_h as i32 - 1);
                    if self.tool == PaintTool::Line {
                        self.draw_stroke(
                            self.start_px.0,
                            self.start_px.1,
                            px,
                            py,
                            self.active_color,
                            (self.brush_radius / 2).max(1),
                        );
                    } else if self.tool == PaintTool::Box {
                        self.draw_rect_outline(
                            self.start_px.0,
                            self.start_px.1,
                            px,
                            py,
                            self.active_color,
                            self.brush_radius,
                        );
                    }
                    self.drawing = false;
                    self.ctx.damage.invalidate(canvas_rect);
                    return EventResult::NeedsRedraw;
                }
            }
            _ => {}
        }
        EventResult::Ignored
    }

    fn take_damage(&mut self, bounds: Rect) -> Option<Rect> {
        if self.dirty_all {
            self.dirty_all = false;
            self.ctx.damage.clear();
            return Some(bounds);
        }
        self.ctx.damage.take_bounding(bounds)
    }

    fn draw(&mut self, surf: &mut Surface, bounds: Rect, theme: &Theme) {
        surf.fill_rect(bounds, theme.face);
        let tb_h = 44;
        surf.fill_rect(Rect::new(bounds.x, bounds.y, bounds.w, tb_h), theme.face_light);
        surf.fill_rect(Rect::new(bounds.x, bounds.y + tb_h - 1, bounds.w, 1), theme.ctrl_border);

        with_default_font(|font| {
            let st = TextStyle::ui_sized(12, contrast_text_on(theme.face_light));
            let st_bold = TextStyle {
                bold: true,
                ..st
            };

            let mut bx = bounds.x + 8;
            let tools = [
                ("Brush", PaintTool::Brush, 46),
                ("Eraser", PaintTool::Eraser, 50),
                ("Line", PaintTool::Line, 40),
                ("Box", PaintTool::Box, 38),
            ];
            for &(label, t, w) in &tools {
                let active = self.tool == t;
                let bg = if active { theme.accent } else { theme.face };
                surf.fill_rect(Rect::new(bx, bounds.y + 8, w, 24), bg);
                surf.draw_rect(Rect::new(bx, bounds.y + 8, w, 24), theme.ctrl_border);
                let tc = contrast_text_on(bg);
                font.draw_text(
                    surf,
                    bx + 6,
                    bounds.y + 13,
                    label,
                    TextStyle {
                        bold: active,
                        ..TextStyle::ui_sized(11, tc)
                    },
                );
                bx += w + 4;
            }

            bx += 4;
            for &(lbl, r) in &[("S", 2i32), ("M", 5), ("L", 10)] {
                let active = self.brush_radius == r;
                let bg = if active { theme.selection } else { theme.face };
                surf.fill_rect(Rect::new(bx, bounds.y + 8, 26, 24), bg);
                surf.draw_rect(Rect::new(bx, bounds.y + 8, 26, 24), theme.ctrl_border);
                font.draw_text(
                    surf,
                    bx + 9,
                    bounds.y + 13,
                    lbl,
                    TextStyle {
                        bold: active,
                        ..TextStyle::ui_sized(11, contrast_text_on(bg))
                    },
                );
                bx += 28;
            }

            bx += 4;
            let palette = [
                theme.accent.to_u32(),
                0xFF1A_1C22,
                0xFFD8_3B01,
                0xFF10_7C10,
                0xFF00_78D7,
                0xFFFF_B900,
                0xFF88_1798,
            ];
            for &col in &palette {
                surf.fill_rect(Rect::new(bx, bounds.y + 10, 16, 16), Color::from_hex(col));
                let border = if self.active_color == col && self.tool != PaintTool::Eraser {
                    Color::BLACK
                } else {
                    theme.ctrl_border
                };
                surf.draw_rect(Rect::new(bx, bounds.y + 10, 16, 16), border);
                bx += 18;
            }

            let save_x = bounds.x + bounds.w - 74;
            let clr_x = save_x - 48;
            surf.fill_rect(Rect::new(clr_x, bounds.y + 8, 44, 24), theme.face);
            surf.draw_rect(Rect::new(clr_x, bounds.y + 8, 44, 24), theme.ctrl_border);
            font.draw_text(surf, clr_x + 7, bounds.y + 13, "Clear", st);

            surf.fill_rect(Rect::new(save_x, bounds.y + 8, 66, 24), theme.accent);
            surf.draw_rect(Rect::new(save_x, bounds.y + 8, 66, 24), theme.frame_outer);
            font.draw_text(
                surf,
                save_x + 7,
                bounds.y + 13,
                "Save BMP",
                TextStyle {
                    color: contrast_text_on(theme.accent),
                    ..st_bold
                },
            );

            // Canvas pixel surface
            let cx = bounds.x + 8;
            let cy = bounds.y + tb_h + 4;
            let cw = self.canvas_w as i32;
            let ch = self.canvas_h as i32;
            surf.draw_rect(Rect::new(cx - 1, cy - 1, cw + 2, ch + 2), theme.ctrl_border);
            for row in 0..self.canvas_h {
                let py = cy + row as i32;
                let src = row * self.canvas_w;
                for col in 0..self.canvas_w {
                    surf.set_pixel(cx + col as i32, py, Color::from_hex(self.pixels[src + col]));
                }
            }

            // Status bar
            let sy = bounds.y + bounds.h - 20;
            surf.fill_rect(Rect::new(bounds.x, sy, bounds.w, 20), theme.face_dark);
            font.draw_text(
                surf,
                bounds.x + 8,
                sy + 3,
                &self.status,
                TextStyle::ui_sized(11, contrast_text_on(theme.face_dark)),
            );
        });
    }
}

#[no_mangle]
pub fn main() -> i32 {
    let mut os = SublimeWmOs;
    let mut app = PaintApp::new();
    match libgui::ipc::run_app(&mut app, &mut os) {
        Ok(()) => 0,
        Err(code) => {
            println!("[paint pid={}] WM error: {}", getpid(), code);
            1
        }
    }
}
