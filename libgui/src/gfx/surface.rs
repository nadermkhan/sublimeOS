use alloc::vec::Vec;
use crate::gfx::{Color, Rect};

/// Clippable 32bpp ARGB software drawing surface with nested clip-rect stack.
pub struct Surface<'a> {
    pub width: i32,
    pub height: i32,
    pub stride: usize,
    pub pixels: &'a mut [u32],
    clip_stack: Vec<Rect>,
}

impl<'a> Surface<'a> {
    pub fn from_slice(width: i32, height: i32, pixels: &'a mut [u32]) -> Self {
        let w = width.max(0);
        let h = height.max(0);
        Self {
            width: w,
            height: h,
            stride: w as usize,
            pixels,
            clip_stack: Vec::new(),
        }
    }

    #[inline(always)]
    pub fn bounds(&self) -> Rect {
        Rect::new(0, 0, self.width, self.height)
    }

    #[inline(always)]
    pub fn clip_rect(&self) -> Rect {
        self.clip_stack.last().copied().unwrap_or_else(|| self.bounds())
    }

    /// Push a clip rect intersected with the current top clip rect.
    pub fn push_clip(&mut self, rect: Rect) {
        let next = self.clip_rect().intersect(rect);
        self.clip_stack.push(next);
    }

    pub fn pop_clip(&mut self) {
        self.clip_stack.pop();
    }

    #[inline(always)]
    pub fn set_pixel(&mut self, x: i32, y: i32, color: Color) {
        let clip = self.clip_rect();
        if x >= clip.x && x < clip.right() && y >= clip.y && y < clip.bottom() {
            let idx = y as usize * self.stride + x as usize;
            if idx < self.pixels.len() {
                self.pixels[idx] = color.to_u32();
            }
        }
    }

    #[inline(always)]
    pub fn blend_pixel(&mut self, x: i32, y: i32, color: Color) {
        let clip = self.clip_rect();
        if x >= clip.x && x < clip.right() && y >= clip.y && y < clip.bottom() {
            let idx = y as usize * self.stride + x as usize;
            if idx < self.pixels.len() {
                let dst = Color(self.pixels[idx]);
                self.pixels[idx] = Color::blend(color, dst).to_u32();
            }
        }
    }

    #[inline(always)]
    pub fn blend_coverage(&mut self, x: i32, y: i32, color: Color, cov: u8) {
        if cov == 0 {
            return;
        }
        let clip = self.clip_rect();
        if x >= clip.x && x < clip.right() && y >= clip.y && y < clip.bottom() {
            let idx = y as usize * self.stride + x as usize;
            if idx < self.pixels.len() {
                let dst = Color(self.pixels[idx]);
                self.pixels[idx] = Color::blend_coverage(color, dst, cov).to_u32();
            }
        }
    }

    #[inline(always)]
    pub fn blend_subpixel_rgb(&mut self, x: i32, y: i32, color: Color, cov_r: u8, cov_g: u8, cov_b: u8) {
        if (cov_r | cov_g | cov_b) == 0 {
            return;
        }
        let clip = self.clip_rect();
        if x >= clip.x && x < clip.right() && y >= clip.y && y < clip.bottom() {
            let idx = y as usize * self.stride + x as usize;
            if idx < self.pixels.len() {
                let dst = Color(self.pixels[idx]);
                self.pixels[idx] = Color::blend_subpixel_rgb(color, dst, cov_r, cov_g, cov_b).to_u32();
            }
        }
    }

    pub fn fill_rect(&mut self, rect: Rect, color: Color) {
        let clipped = rect.intersect(self.clip_rect());
        if clipped.is_empty() || color.a() == 0 {
            return;
        }
        let is_opaque = color.a() == 255;
        let raw = color.to_u32();
        for py in clipped.y..clipped.bottom() {
            let row = py as usize * self.stride;
            let s = row + clipped.x as usize;
            let e = (row + clipped.right() as usize).min(self.pixels.len());
            if s >= e {
                continue;
            }
            if is_opaque {
                self.pixels[s..e].fill(raw);
            } else {
                for px in s..e {
                    self.pixels[px] = Color::blend(color, Color(self.pixels[px])).to_u32();
                }
            }
        }
    }

    pub fn draw_rect(&mut self, rect: Rect, color: Color) {
        if rect.is_empty() {
            return;
        }
        self.fill_rect(Rect::new(rect.x, rect.y, rect.w, 1), color);
        self.fill_rect(Rect::new(rect.x, rect.bottom() - 1, rect.w, 1), color);
        self.fill_rect(Rect::new(rect.x, rect.y, 1, rect.h), color);
        self.fill_rect(Rect::new(rect.right() - 1, rect.y, 1, rect.h), color);
    }

    pub fn draw_gradient_v(&mut self, rect: Rect, top: Color, bottom: Color) {
        let clipped = rect.intersect(self.clip_rect());
        if clipped.is_empty() {
            return;
        }
        let denom = (rect.h - 1).max(1) as u32;
        for py in clipped.y..clipped.bottom() {
            let t = ((py - rect.y) as u32 * 255) / denom;
            let val = Color::lerp(top, bottom, t).to_u32();
            let row = py as usize * self.stride;
            let s = row + clipped.x as usize;
            let e = (row + clipped.right() as usize).min(self.pixels.len());
            if s < e {
                self.pixels[s..e].fill(val);
            }
        }
    }

    pub fn draw_gradient_h(&mut self, rect: Rect, left: Color, right: Color) {
        let clipped = rect.intersect(self.clip_rect());
        if clipped.is_empty() {
            return;
        }
        let denom = (rect.w - 1).max(1) as u32;
        for py in clipped.y..clipped.bottom() {
            let row = py as usize * self.stride;
            for px in clipped.x..clipped.right() {
                let idx = row + px as usize;
                if idx < self.pixels.len() {
                    let t = ((px - rect.x) as u32 * 255) / denom;
                    self.pixels[idx] = Color::lerp(left, right, t).to_u32();
                }
            }
        }
    }

    pub fn fill_rounded_rect(&mut self, rect: Rect, radius: i32, color: Color) {
        if rect.is_empty() {
            return;
        }
        if radius <= 0 {
            self.fill_rect(rect, color);
            return;
        }
        let r = radius.min(rect.w / 2).min(rect.h / 2);
        self.fill_rect(Rect::new(rect.x, rect.y + r, rect.w, rect.h - 2 * r), color);
        self.fill_rect(Rect::new(rect.x + r, rect.y, rect.w - 2 * r, r), color);
        self.fill_rect(Rect::new(rect.x + r, rect.bottom() - r, rect.w - 2 * r, r), color);

        for dy in 0..r {
            for dx in 0..r {
                let d2 = (r - 1 - dx) * (r - 1 - dx) + (r - 1 - dy) * (r - 1 - dy);
                if d2 <= r * r {
                    self.blend_pixel(rect.x + dx, rect.y + dy, color);
                    self.blend_pixel(rect.right() - 1 - dx, rect.y + dy, color);
                    self.blend_pixel(rect.x + dx, rect.bottom() - 1 - dy, color);
                    self.blend_pixel(rect.right() - 1 - dx, rect.bottom() - 1 - dy, color);
                }
            }
        }
    }

    pub fn draw_rounded_rect(&mut self, rect: Rect, radius: i32, color: Color) {
        if rect.is_empty() {
            return;
        }
        if radius <= 0 {
            self.draw_rect(rect, color);
            return;
        }
        let r = radius.min(rect.w / 2).min(rect.h / 2);
        self.fill_rect(Rect::new(rect.x + r, rect.y, rect.w - 2 * r, 1), color);
        self.fill_rect(Rect::new(rect.x + r, rect.bottom() - 1, rect.w - 2 * r, 1), color);
        self.fill_rect(Rect::new(rect.x, rect.y + r, 1, rect.h - 2 * r), color);
        self.fill_rect(Rect::new(rect.right() - 1, rect.y + r, 1, rect.h - 2 * r), color);

        for dy in 0..r {
            for dx in 0..r {
                let d2 = (r - 1 - dx) * (r - 1 - dx) + (r - 1 - dy) * (r - 1 - dy);
                if d2 <= r * r && d2 > (r - 2) * (r - 2) {
                    self.blend_pixel(rect.x + dx, rect.y + dy, color);
                    self.blend_pixel(rect.right() - 1 - dx, rect.y + dy, color);
                    self.blend_pixel(rect.x + dx, rect.bottom() - 1 - dy, color);
                    self.blend_pixel(rect.right() - 1 - dx, rect.bottom() - 1 - dy, color);
                }
            }
        }
    }

    pub fn draw_line(&mut self, mut x0: i32, mut y0: i32, x1: i32, y1: i32, color: Color) {
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let sx = if x0 < x1 { 1 } else { -1 };
        let sy = if y0 < y1 { 1 } else { -1 };
        let mut err = dx + dy;
        loop {
            self.blend_pixel(x0, y0, color);
            if x0 == x1 && y0 == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 >= dy {
                err += dy;
                x0 += sx;
            }
            if e2 <= dx {
                err += dx;
                y0 += sy;
            }
        }
    }

    pub fn blit_argb(&mut self, dst: Rect, src_w: i32, src_h: i32, src_pixels: &[u32]) {
        if src_w <= 0 || src_h <= 0 || dst.is_empty() {
            return;
        }
        let clipped = dst.intersect(self.clip_rect());
        if clipped.is_empty() {
            return;
        }
        let copy_w = (clipped.w).min(src_w - (clipped.x - dst.x));
        let copy_h = (clipped.h).min(src_h - (clipped.y - dst.y));
        if copy_w <= 0 || copy_h <= 0 {
            return;
        }
        for row in 0..copy_h {
            let sy = (clipped.y - dst.y + row) as usize * src_w as usize + (clipped.x - dst.x) as usize;
            let dy = (clipped.y + row) as usize * self.stride + clipped.x as usize;
            for col in 0..copy_w as usize {
                let sidx = sy + col;
                let didx = dy + col;
                if sidx < src_pixels.len() && didx < self.pixels.len() {
                    let c = Color(src_pixels[sidx]);
                    if c.a() == 255 {
                        self.pixels[didx] = c.to_u32();
                    } else if c.a() > 0 {
                        self.pixels[didx] = Color::blend(c, Color(self.pixels[didx])).to_u32();
                    }
                }
            }
        }
    }

    pub fn fill_circle(&mut self, cx: i32, cy: i32, radius: i32, color: Color) {
        if radius <= 0 || color.a() == 0 {
            return;
        }
        let r2 = radius * radius;
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                if dx * dx + dy * dy <= r2 {
                    self.blend_pixel(cx + dx, cy + dy, color);
                }
            }
        }
    }

    pub fn draw_circle(&mut self, cx: i32, cy: i32, radius: i32, color: Color) {
        if radius <= 0 || color.a() == 0 {
            return;
        }
        let outer = radius * radius;
        let inner = (radius - 1).max(0) * (radius - 1).max(0);
        for dy in -radius..=radius {
            for dx in -radius..=radius {
                let d2 = dx * dx + dy * dy;
                if d2 <= outer && d2 >= inner {
                    self.blend_pixel(cx + dx, cy + dy, color);
                }
            }
        }
    }

    /// Draw a classic 1px top-left / bottom-right 3D bevel inside `rect`.
    pub fn draw_bevel(&mut self, rect: Rect, top_left: Color, bottom_right: Color) {
        if rect.w < 2 || rect.h < 2 {
            return;
        }
        self.fill_rect(Rect::new(rect.x, rect.y, rect.w - 1, 1), top_left);
        self.fill_rect(Rect::new(rect.x, rect.y, 1, rect.h - 1), top_left);
        self.fill_rect(Rect::new(rect.x, rect.bottom() - 1, rect.w, 1), bottom_right);
        self.fill_rect(Rect::new(rect.right() - 1, rect.y, 1, rect.h), bottom_right);
    }

    /// Draw an etched 2px-style 3D border (Win32 `EDGE_ETCHED` / `BS_GROUPBOX` look).
    pub fn draw_etched_rect(&mut self, rect: Rect, shadow: Color, highlight: Color) {
        if rect.w < 4 || rect.h < 4 {
            return;
        }
        self.draw_rect(Rect::new(rect.x + 1, rect.y + 1, rect.w - 1, rect.h - 1), highlight);
        self.draw_rect(Rect::new(rect.x, rect.y, rect.w - 1, rect.h - 1), shadow);
    }

    /// Draw a crisp filled directional arrow triangle centered in `rect`.
    pub fn draw_arrow(&mut self, rect: Rect, dir: crate::gfx::uxtheme::ArrowDir, color: Color) {
        if rect.w < 5 || rect.h < 5 {
            return;
        }
        let cx = rect.x + rect.w / 2;
        let cy = rect.y + rect.h / 2;
        match dir {
            crate::gfx::uxtheme::ArrowDir::Up => {
                for row in 0..4 {
                    let y = cy - 2 + row;
                    self.fill_rect(Rect::new(cx - row, y, row * 2 + 1, 1), color);
                }
            }
            crate::gfx::uxtheme::ArrowDir::Down => {
                for row in 0..4 {
                    let y = cy - 1 + row;
                    let half = 3 - row;
                    self.fill_rect(Rect::new(cx - half, y, half * 2 + 1, 1), color);
                }
            }
            crate::gfx::uxtheme::ArrowDir::Left => {
                for col in 0..4 {
                    let x = cx - 2 + col;
                    self.fill_rect(Rect::new(x, cy - col, 1, col * 2 + 1), color);
                }
            }
            crate::gfx::uxtheme::ArrowDir::Right => {
                for col in 0..4 {
                    let x = cx - 1 + col;
                    let half = 3 - col;
                    self.fill_rect(Rect::new(x, cy - half, 1, half * 2 + 1), color);
                }
            }
        }
    }

    /// Draw a crisp Win32 checkmark centered inside `rect`.
    pub fn draw_checkmark(&mut self, rect: Rect, color: Color) {
        let x = rect.x + (rect.w - 9) / 2;
        let y = rect.y + (rect.h - 7) / 2;
        for t in 0..2 {
            self.draw_line(x + 1, y + 3 + t, x + 3, y + 5 + t, color);
            self.draw_line(x + 3, y + 5 + t, x + 7, y + 1 + t, color);
        }
    }
}
