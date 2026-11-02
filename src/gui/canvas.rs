use alloc::vec::Vec;
use crate::gui::color::Color;

pub struct Canvas {
    pub width: u32,
    pub height: u32,
    pub buffer: Vec<u32>,
}

impl Canvas {
    pub fn new(width: u32, height: u32) -> Self {
        let size = (width * height) as usize;
        let mut buffer = Vec::with_capacity(size);
        buffer.resize(size, Color::BLACK.to_u32());
        Self {
            width,
            height,
            buffer,
        }
    }

    #[inline(always)]
    pub fn set_pixel(&mut self, x: i32, y: i32, color: Color) {
        if x >= 0 && x < self.width as i32 && y >= 0 && y < self.height as i32 {
            let idx = (y as usize * self.width as usize) + x as usize;
            self.buffer[idx] = color.to_u32();
        }
    }

    #[inline(always)]
    pub fn set_pixel_blend(&mut self, x: i32, y: i32, color: Color) {
        if x >= 0 && x < self.width as i32 && y >= 0 && y < self.height as i32 {
            let idx = (y as usize * self.width as usize) + x as usize;
            let current = Color(self.buffer[idx]);
            self.buffer[idx] = Color::blend(color, current).to_u32();
        }
    }

    #[inline(always)]
    pub fn get_pixel(&self, x: i32, y: i32) -> Color {
        if x >= 0 && x < self.width as i32 && y >= 0 && y < self.height as i32 {
            let idx = (y as usize * self.width as usize) + x as usize;
            Color(self.buffer[idx])
        } else {
            Color::BLACK
        }
    }

    pub fn clear(&mut self, color: Color) {
        let val = color.to_u32();
        for pixel in self.buffer.iter_mut() {
            *pixel = val;
        }
    }

    pub fn fill_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: Color) {
        if w <= 0 || h <= 0 {
            return;
        }

        let x_start = x.max(0);
        let y_start = y.max(0);
        let x_end = (x + w).min(self.width as i32);
        let y_end = (y + h).min(self.height as i32);

        if x_start >= x_end || y_start >= y_end {
            return;
        }

        let is_opaque = color.a() == 255;
        let c_val = color.to_u32();

        for py in y_start..y_end {
            let row_offset = py as usize * self.width as usize;
            for px in x_start..x_end {
                let idx = row_offset + px as usize;
                if is_opaque {
                    self.buffer[idx] = c_val;
                } else {
                    let current = Color(self.buffer[idx]);
                    self.buffer[idx] = Color::blend(color, current).to_u32();
                }
            }
        }
    }

    pub fn draw_rect(&mut self, x: i32, y: i32, w: i32, h: i32, color: Color) {
        if w <= 0 || h <= 0 {
            return;
        }
        self.fill_rect(x, y, w, 1, color);
        self.fill_rect(x, y + h - 1, w, 1, color);
        self.fill_rect(x, y, 1, h, color);
        self.fill_rect(x + w - 1, y, 1, h, color);
    }

    pub fn fill_rounded_rect(&mut self, x: i32, y: i32, w: i32, h: i32, r: i32, color: Color) {
        if r <= 0 {
            self.fill_rect(x, y, w, h, color);
            return;
        }
        let r = r.min(w / 2).min(h / 2);

        // Center body
        self.fill_rect(x, y + r, w, h - 2 * r, color);
        // Top and bottom slabs
        self.fill_rect(x + r, y, w - 2 * r, r, color);
        self.fill_rect(x + r, y + h - r, w - 2 * r, r, color);

        // Four corner discs
        for dy in 0..r {
            for dx in 0..r {
                let dist_sq = (r - 1 - dx) * (r - 1 - dx) + (r - 1 - dy) * (r - 1 - dy);
                if dist_sq <= r * r {
                    // Top-Left
                    self.set_pixel_blend(x + dx, y + dy, color);
                    // Top-Right
                    self.set_pixel_blend(x + w - 1 - dx, y + dy, color);
                    // Bottom-Left
                    self.set_pixel_blend(x + dx, y + h - 1 - dy, color);
                    // Bottom-Right
                    self.set_pixel_blend(x + w - 1 - dx, y + h - 1 - dy, color);
                }
            }
        }
    }

    pub fn draw_rounded_rect(&mut self, x: i32, y: i32, w: i32, h: i32, r: i32, color: Color) {
        if r <= 0 {
            self.draw_rect(x, y, w, h, color);
            return;
        }
        let r = r.min(w / 2).min(h / 2);

        // Top, bottom, left, right straight segments
        self.fill_rect(x + r, y, w - 2 * r, 1, color);
        self.fill_rect(x + r, y + h - 1, w - 2 * r, 1, color);
        self.fill_rect(x, y + r, 1, h - 2 * r, color);
        self.fill_rect(x + w - 1, y + r, 1, h - 2 * r, color);

        // Four corners
        for dy in 0..r {
            for dx in 0..r {
                let dist_sq = (r - 1 - dx) * (r - 1 - dx) + (r - 1 - dy) * (r - 1 - dy);
                if dist_sq <= r * r && dist_sq > (r - 2) * (r - 2) {
                    self.set_pixel_blend(x + dx, y + dy, color);
                    self.set_pixel_blend(x + w - 1 - dx, y + dy, color);
                    self.set_pixel_blend(x + dx, y + h - 1 - dy, color);
                    self.set_pixel_blend(x + w - 1 - dx, y + h - 1 - dy, color);
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
            self.set_pixel_blend(x0, y0, color);
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

    pub fn draw_circle(&mut self, cx: i32, cy: i32, radius: i32, color: Color) {
        let mut x = radius;
        let mut y = 0;
        let mut err = 0;

        while x >= y {
            self.set_pixel_blend(cx + x, cy + y, color);
            self.set_pixel_blend(cx + y, cy + x, color);
            self.set_pixel_blend(cx - y, cy + x, color);
            self.set_pixel_blend(cx - x, cy + y, color);
            self.set_pixel_blend(cx - x, cy - y, color);
            self.set_pixel_blend(cx - y, cy - x, color);
            self.set_pixel_blend(cx + y, cy - x, color);
            self.set_pixel_blend(cx + x, cy - y, color);

            y += 1;
            err += 1 + 2 * y;
            if 2 * (err - x) + 1 > 0 {
                x -= 1;
                err += 1 - 2 * x;
            }
        }
    }

    pub fn fill_circle(&mut self, cx: i32, cy: i32, radius: i32, color: Color) {
        let isqrt = f32_sqrt();
        for dy in -radius..=radius {
            let dx_max = isqrt(radius * radius - dy * dy);
            self.fill_rect(cx - dx_max, cy + dy, 2 * dx_max + 1, 1, color);
        }
    }

    pub fn draw_gradient_v(&mut self, x: i32, y: i32, w: i32, h: i32, top: Color, bottom: Color) {
        if w <= 0 || h <= 0 {
            return;
        }
        for dy in 0..h {
            let t = (dy * 255) / (h - 1).max(1);
            let color = Color::lerp(top, bottom, t as u32);
            self.fill_rect(x, y + dy, w, 1, color);
        }
    }

    pub fn draw_gradient_h(&mut self, x: i32, y: i32, w: i32, h: i32, left: Color, right: Color) {
        if w <= 0 || h <= 0 {
            return;
        }
        for dx in 0..w {
            let t = (dx * 255) / (w - 1).max(1);
            let color = Color::lerp(left, right, t as u32);
            self.fill_rect(x + dx, y, 1, h, color);
        }
    }

    /// Render a smooth drop shadow around a rectangular window.
    pub fn draw_shadow(&mut self, x: i32, y: i32, w: i32, h: i32, spread: i32, base_alpha: u8) {
        for i in 1..=spread {
            let alpha = ((base_alpha as i32 * (spread - i + 1)) / (spread * 2)) as u8;
            let shadow_color = Color::argb(alpha, 0, 0, 0);
            self.fill_rounded_rect(
                x - i,
                y - i + 4,
                w + 2 * i,
                h + 2 * i,
                8 + i,
                shadow_color,
            );
        }
    }

    /// Fast copy to the VBE linear framebuffer.
    pub fn copy_to_framebuffer(&self, fb_ptr: *mut u32) {
        if fb_ptr.is_null() {
            return;
        }
        unsafe {
            core::ptr::copy_nonoverlapping(self.buffer.as_ptr(), fb_ptr, self.buffer.len());
        }
    }
}

/// Helper integer square root for circle rasterization without floating point hardware.
fn f32_sqrt() -> impl Fn(i32) -> i32 {
    |val: i32| -> i32 {
        if val <= 0 {
            return 0;
        }
        let mut x = val;
        let mut y = (x + 1) / 2;
        while y < x {
            x = y;
            y = (x + val / x) / 2;
        }
        x
    }
}
