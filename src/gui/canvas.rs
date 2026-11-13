use crate::gui::color::Color;
use crate::mm::PAGE_SIZE;
use crate::mm::pmm::PMM;

pub const GUI_BACKBUFFER_VIRT: u64 = 0xFFFFFFFFD8000000;

pub struct Canvas {
    pub width: u32,
    pub height: u32,
    pub buffer: &'static mut [u32],
    allocated_pages: usize,
    contiguous_phys: Option<u64>,
    mapped_virt: bool,
}

impl Canvas {
    pub fn new(width: u32, height: u32) -> Self {
        let total_pixels = (width as usize) * (height as usize);
        let total_bytes = total_pixels * 4;
        let total_pages = (total_bytes + PAGE_SIZE - 1) / PAGE_SIZE;

        // 1. First attempt fast contiguous physical RAM allocation via PMM
        let (virt_ptr, contiguous_phys, mapped_virt) = if let Some(phys) = PMM.lock().alloc_contiguous_frames(total_pages) {
            let virt = crate::mm::phys_to_virt(phys) as *mut u32;
            (virt, Some(phys), false)
        } else {
            // 2. Fallback: map individual 4 KiB frames page-by-page into dedicated virtual window (Write-Back cached)
            let vmm = crate::mm::vmm::VMM.lock();
            let kernel_pml4 = vmm.get_kernel_pml4();
            let flags = crate::mm::vmm::PAGE_PRESENT | crate::mm::vmm::PAGE_WRITABLE;
            for p in 0..total_pages {
                let virt = GUI_BACKBUFFER_VIRT + (p * PAGE_SIZE) as u64;
                let phys = PMM.lock().alloc_frame().unwrap_or(0);
                let _ = vmm.map_page(kernel_pml4, virt, phys, flags);
            }
            (GUI_BACKBUFFER_VIRT as *mut u32, None, true)
        };

        let buffer = unsafe { core::slice::from_raw_parts_mut(virt_ptr, total_pixels) };
        buffer.fill(Color::BLACK.to_u32());

        Self {
            width,
            height,
            buffer,
            allocated_pages: total_pages,
            contiguous_phys,
            mapped_virt,
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

    /// Authentic macOS Frosted Glass: Box-blur backdrop filter across rectangular region.
    pub fn blur_rect(&mut self, x: i32, y: i32, w: i32, h: i32, passes: usize) {
        if w <= 4 || h <= 4 {
            return;
        }
        let x_start = x.max(1);
        let y_start = y.max(1);
        let x_end = (x + w).min(self.width as i32 - 1);
        let y_end = (y + h).min(self.height as i32 - 1);

        if x_start >= x_end || y_start >= y_end {
            return;
        }

        let stride = self.width as usize;

        for _ in 0..passes {
            for py in y_start..y_end {
                let row = py as usize * stride;
                let row_up = (py - 1) as usize * stride;
                let row_dn = (py + 1) as usize * stride;

                for px in x_start..x_end {
                    let idx = row + px as usize;
                    let c = self.buffer[idx];
                    let l = self.buffer[idx - 1];
                    let r = self.buffer[idx + 1];
                    let u = self.buffer[row_up + px as usize];
                    let d = self.buffer[row_dn + px as usize];

                    let red = (((c >> 16) & 0xFF) * 2 + ((l >> 16) & 0xFF) + ((r >> 16) & 0xFF) + ((u >> 16) & 0xFF) + ((d >> 16) & 0xFF)) / 6;
                    let green = (((c >> 8) & 0xFF) * 2 + ((l >> 8) & 0xFF) + ((r >> 8) & 0xFF) + ((u >> 8) & 0xFF) + ((d >> 8) & 0xFF)) / 6;
                    let blue = ((c & 0xFF) * 2 + (l & 0xFF) + (r & 0xFF) + (u & 0xFF) + (d & 0xFF)) / 6;

                    self.buffer[idx] = (0xFF << 24) | (red << 16) | (green << 8) | blue;
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

    /// Cache-friendly vertical gradient with direct contiguous scanline slice fills.
    pub fn draw_gradient_v(&mut self, x: i32, y: i32, w: i32, h: i32, top: Color, bottom: Color) {
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
        let denom = (h - 1).max(1) as u32;
        for py in y_start..y_end {
            let t = ((py - y) as u32 * 255) / denom;
            let val = Color::lerp(top, bottom, t).to_u32();
            let row_offset = py as usize * self.width as usize;
            self.buffer[row_offset + x_start as usize..row_offset + x_end as usize].fill(val);
        }
    }

    /// Cache-friendly row-major horizontal gradient (avoids column-major cache thrashing).
    pub fn draw_gradient_h(&mut self, x: i32, y: i32, w: i32, h: i32, left: Color, right: Color) {
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
        let denom = (w - 1).max(1) as u32;
        for py in y_start..y_end {
            let row_offset = py as usize * self.width as usize;
            for px in x_start..x_end {
                let t = ((px - x) as u32 * 255) / denom;
                self.buffer[row_offset + px as usize] = Color::lerp(left, right, t).to_u32();
            }
        }
    }

    /// Hollow perimeter drop shadow (avoids millions of redundant interior overdraws per frame).
    pub fn draw_shadow(&mut self, x: i32, y: i32, w: i32, h: i32, spread: i32, base_alpha: u8) {
        for i in 1..=spread {
            let alpha = ((base_alpha as i32 * (spread - i + 1)) / (spread * 2)) as u8;
            let shadow_color = Color::argb(alpha, 0, 0, 0);
            self.draw_rounded_rect(
                x - i,
                y - i + 4,
                w + 2 * i,
                h + 2 * i,
                8 + i,
                shadow_color,
            );
        }
    }

    /// Fast copy to the framebuffer (assumes packed 32bpp BGRA with pitch == width * 4).
    pub fn copy_to_framebuffer(&self, fb_ptr: *mut u32) {
        self.blit_to_framebuffer(fb_ptr, self.width * 4, 32, 16, 8, 0);
    }

    /// Blit canvas pixels to an arbitrary UEFI GOP / Multiboot Linear Framebuffer,
    /// honoring scanline byte `pitch`, `bpp` (24 or 32), and RGB/BGR channel shifts.
    pub fn blit_to_framebuffer(
        &self,
        fb_ptr: *mut u32,
        pitch: u32,
        bpp: u32,
        red_pos: u8,
        green_pos: u8,
        blue_pos: u8,
    ) {
        if fb_ptr.is_null() {
            return;
        }
        let w = self.width as usize;
        let h = self.height as usize;
        let stride = (pitch as usize).max(w * 4);

        if bpp == 32 && red_pos == 16 && green_pos == 8 && blue_pos == 0 {
            if stride == w * 4 {
                unsafe {
                    core::ptr::copy_nonoverlapping(self.buffer.as_ptr(), fb_ptr, self.buffer.len());
                }
            } else {
                let dst_bytes = fb_ptr as *mut u8;
                for y in 0..h {
                    unsafe {
                        let dst_row = dst_bytes.add(y * stride) as *mut u32;
                        let src_row = self.buffer.as_ptr().add(y * w);
                        core::ptr::copy_nonoverlapping(src_row, dst_row, w);
                    }
                }
            }
            return;
        }

        let dst_bytes = fb_ptr as *mut u8;
        if bpp == 32 {
            if red_pos == 0 && green_pos == 8 && blue_pos == 16 {
                // High-performance line-buffered conversion using PCIe burst copy
                let mut row_buf = [0u32; 2560];
                let line_len = w.min(2560);
                for y in 0..h {
                    let dst_row = unsafe { dst_bytes.add(y * stride) as *mut u32 };
                    let src_base = y * w;
                    for x in 0..line_len {
                        let argb = self.buffer[src_base + x];
                        let r = (argb >> 16) & 0xFF;
                        let g = (argb >> 8) & 0xFF;
                        let b = argb & 0xFF;
                        row_buf[x] = r | (g << 8) | (b << 16);
                    }
                    unsafe {
                        core::ptr::copy_nonoverlapping(row_buf.as_ptr(), dst_row, line_len);
                    }
                }
            } else {
                for y in 0..h {
                    let dst_row = unsafe { dst_bytes.add(y * stride) as *mut u32 };
                    let src_base = y * w;
                    for x in 0..w {
                        let argb = self.buffer[src_base + x];
                        let r = (argb >> 16) & 0xFF;
                        let g = (argb >> 8) & 0xFF;
                        let b = argb & 0xFF;
                        let packed = (r << red_pos) | (g << green_pos) | (b << blue_pos);
                        unsafe {
                            *dst_row.add(x) = packed;
                        }
                    }
                }
            }
        } else if bpp == 24 {
            let bpp_stride = (pitch as usize).max(w * 3);
            for y in 0..h {
                let dst_row = unsafe { dst_bytes.add(y * bpp_stride) };
                let src_base = y * w;
                for x in 0..w {
                    let argb = self.buffer[src_base + x];
                    let r = ((argb >> 16) & 0xFF) as u8;
                    let g = ((argb >> 8) & 0xFF) as u8;
                    let b = (argb & 0xFF) as u8;
                    unsafe {
                        if red_pos == 16 {
                            *dst_row.add(x * 3) = b;
                            *dst_row.add(x * 3 + 1) = g;
                            *dst_row.add(x * 3 + 2) = r;
                        } else {
                            *dst_row.add(x * 3) = r;
                            *dst_row.add(x * 3 + 1) = g;
                            *dst_row.add(x * 3 + 2) = b;
                        }
                    }
                }
            }
        }
    }
}

impl Drop for Canvas {
    fn drop(&mut self) {
        if let Some(phys) = self.contiguous_phys {
            let mut pmm = PMM.lock();
            for p in 0..self.allocated_pages {
                pmm.free_frame(phys + (p * PAGE_SIZE) as u64);
            }
        } else if self.mapped_virt {
            let vmm = crate::mm::vmm::VMM.lock();
            let kernel_pml4 = vmm.get_kernel_pml4();
            let mut pmm = PMM.lock();
            for p in 0..self.allocated_pages {
                let virt = GUI_BACKBUFFER_VIRT + (p * PAGE_SIZE) as u64;
                if let Some(phys) = vmm.translate(kernel_pml4, virt) {
                    pmm.free_frame(phys);
                }
                vmm.unmap_page(kernel_pml4, virt);
            }
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
