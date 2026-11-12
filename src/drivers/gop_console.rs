//! Early UEFI GOP / Multiboot Linear Framebuffer Text Console
//!
//! Mirrors kernel logs (`kinfo!`, `kwarn!`, `kerror!`), CosmicSH shell output,
//! virtual TTY switches, and `#[panic_handler]` crash diagnostics directly to the
//! physical linear framebuffer using the built-in 8x16 bitmap font (`FONT_8X16`),
//! ensuring visibility on EFI laptops without legacy VGA `0xB8000` or `COM1` headers.

use core::fmt::{self, Write};
use core::sync::atomic::{AtomicBool, Ordering};
use crate::arch::x86_64::vga::ScreenChar;
use crate::gui::font::get_glyph_data;
use crate::sync::IrqSafeSpinlock;
use crate::tty::ansi::{AnsiCommand, AnsiParser};
use crate::tty::console::VirtualConsole;

pub const BASE_GLYPH_W: usize = 8;
pub const BASE_GLYPH_H: usize = 16;
const EARLY_LOG_CAP: usize = 4096;
const MAX_GRID_COLS: usize = 160;
const MAX_GRID_ROWS: usize = 64;
const SCROLLBACK_CAP: usize = 256;

const EMPTY_CELL: ScreenChar = ScreenChar {
    ascii_character: b' ',
    color_code: crate::arch::x86_64::vga::ColorCode::new(
        crate::arch::x86_64::vga::Color::LightGray,
        crate::arch::x86_64::vga::Color::Black,
    ),
};

static GOP_CONSOLE_ACTIVE: AtomicBool = AtomicBool::new(false);
static GUI_ACTIVE: AtomicBool = AtomicBool::new(false);

struct EarlyLogBuffer {
    data: [u8; EARLY_LOG_CAP],
    len: usize,
}

impl EarlyLogBuffer {
    const fn new() -> Self {
        Self {
            data: [0; EARLY_LOG_CAP],
            len: 0,
        }
    }

    fn push_bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            if self.len >= EARLY_LOG_CAP {
                break;
            }
            self.data[self.len] = b;
            self.len += 1;
        }
    }
}

static EARLY_LOG: IrqSafeSpinlock<EarlyLogBuffer> = IrqSafeSpinlock::new(EarlyLogBuffer::new());

pub struct GopConsole {
    pub fb_ptr: *mut u8,
    pub shadow_fb: *mut u8,
    pub shadow_frame: u64,
    pub shadow_pages: usize,
    pub width: usize,
    pub height: usize,
    pub pitch: usize,
    pub bpp: u32,
    pub red_pos: u8,
    pub green_pos: u8,
    pub blue_pos: u8,
    pub cols: usize,
    pub rows: usize,
    pub col: usize,
    pub row: usize,
    pub margin_x: usize,
    pub margin_y: usize,
    pub font_scale: usize,
    pub anti_alias: bool,
    pub fg_rgb: (u8, u8, u8),
    pub bg_rgb: (u8, u8, u8),
    grid_ptr: *mut ScreenChar,
    grid_frame: u64,
    grid_pages: usize,
    scrollback_ptr: *mut ScreenChar,
    scrollback_frame: u64,
    scrollback_pages: usize,
    scrollback_head: usize,
    scrollback_count: usize,
    pub view_offset: usize,
    current_color_code: crate::arch::x86_64::vga::ColorCode,
    ansi: AnsiParser,
    utf8_buf: [u8; 4],
    utf8_len: u8,
    utf8_expected: u8,
    pub initialized: bool,
}

unsafe impl Send for GopConsole {}
unsafe impl Sync for GopConsole {}

impl GopConsole {
    pub const fn new() -> Self {
        Self {
            fb_ptr: core::ptr::null_mut(),
            shadow_fb: core::ptr::null_mut(),
            shadow_frame: 0,
            shadow_pages: 0,
            width: 0,
            height: 0,
            pitch: 0,
            bpp: 32,
            red_pos: 16,
            green_pos: 8,
            blue_pos: 0,
            cols: 80,
            rows: 25,
            col: 0,
            row: 0,
            margin_x: 0,
            margin_y: 0,
            font_scale: 1,
            anti_alias: true,
            fg_rgb: (220, 223, 228),
            bg_rgb: (16, 20, 26),
            grid_ptr: core::ptr::null_mut(),
            grid_frame: 0,
            grid_pages: 0,
            scrollback_ptr: core::ptr::null_mut(),
            scrollback_frame: 0,
            scrollback_pages: 0,
            scrollback_head: 0,
            scrollback_count: 0,
            view_offset: 0,
            current_color_code: crate::arch::x86_64::vga::ColorCode::new(
                crate::arch::x86_64::vga::Color::LightGray,
                crate::arch::x86_64::vga::Color::Black,
            ),
            ansi: AnsiParser::new(),
            utf8_buf: [0; 4],
            utf8_len: 0,
            utf8_expected: 0,
            initialized: false,
        }
    }

    #[inline(always)]
    fn get_active_cell(&self, r: usize, c: usize) -> ScreenChar {
        if self.grid_ptr.is_null() || r >= MAX_GRID_ROWS || c >= MAX_GRID_COLS {
            EMPTY_CELL
        } else {
            unsafe { *self.grid_ptr.add(r * MAX_GRID_COLS + c) }
        }
    }

    #[inline(always)]
    fn set_active_cell(&mut self, r: usize, c: usize, cell: ScreenChar) {
        if !self.grid_ptr.is_null() && r < MAX_GRID_ROWS && c < MAX_GRID_COLS {
            unsafe {
                *self.grid_ptr.add(r * MAX_GRID_COLS + c) = cell;
            }
        }
    }

    #[inline(always)]
    fn get_scrollback_cell(&self, line: usize, c: usize) -> ScreenChar {
        if self.scrollback_ptr.is_null() || line >= SCROLLBACK_CAP || c >= MAX_GRID_COLS {
            EMPTY_CELL
        } else {
            unsafe { *self.scrollback_ptr.add(line * MAX_GRID_COLS + c) }
        }
    }

    #[inline(always)]
    fn set_scrollback_cell(&mut self, line: usize, c: usize, cell: ScreenChar) {
        if !self.scrollback_ptr.is_null() && line < SCROLLBACK_CAP && c < MAX_GRID_COLS {
            unsafe {
                *self.scrollback_ptr.add(line * MAX_GRID_COLS + c) = cell;
            }
        }
    }

    #[inline(always)]
    fn pack_rgb(&self, r: u8, g: u8, b: u8) -> u32 {
        ((r as u32) << (self.red_pos & 31))
            | ((g as u32) << (self.green_pos & 31))
            | ((b as u32) << (self.blue_pos & 31))
    }

    #[inline(always)]
    fn vga_color_to_rgb(idx: u8) -> (u8, u8, u8) {
        match idx & 0x0F {
            0 => (16, 20, 26),    // Black (slate background)
            1 => (38, 79, 120),   // Blue
            2 => (152, 195, 121), // Green
            3 => (86, 182, 194),  // Cyan
            4 => (224, 108, 117), // Red
            5 => (198, 120, 221), // Magenta
            6 => (229, 192, 123), // Brown / Yellow
            7 => (220, 223, 228), // LightGray
            8 => (92, 99, 112),   // DarkGray
            9 => (97, 175, 239),  // LightBlue
            10 => (166, 226, 46), // LightGreen
            11 => (102, 217, 239),// LightCyan
            12 => (255, 102, 102),// LightRed
            13 => (249, 117, 220),// Pink
            14 => (235, 203, 139),// Yellow
            _ => (255, 255, 255), // White
        }
    }

    pub fn configure(
        &mut self,
        fb_ptr: *mut u8,
        width: u32,
        height: u32,
        pitch: u32,
        bpp: u32,
        red_pos: u8,
        green_pos: u8,
        blue_pos: u8,
    ) {
        // Free previous shadow buffer frames if allocated
        self.free_shadow_buffer();

        self.fb_ptr = fb_ptr;
        self.width = width as usize;
        self.height = height as usize;
        self.pitch = pitch as usize;
        self.bpp = bpp;
        self.red_pos = red_pos;
        self.green_pos = green_pos;
        self.blue_pos = blue_pos;
        self.margin_x = if self.width >= 640 { 16 } else { 0 };
        self.margin_y = if self.height >= 480 { 12 } else { 0 };
        // Default to 2x (16x32) on high-resolution displays (>= 1280x800) for comfortable reading.
        self.font_scale = if self.width >= 1280 && self.height >= 800 { 2 } else { 1 };
        self.anti_alias = true;
        self.recalc_dimensions();
        self.col = 0;
        self.row = 0;
        self.view_offset = 0;
        self.scrollback_head = 0;
        self.scrollback_count = 0;
        self.fg_rgb = (220, 223, 228);
        self.bg_rgb = (16, 20, 26);
        self.current_color_code = crate::arch::x86_64::vga::ColorCode::new(
            crate::arch::x86_64::vga::Color::LightGray,
            crate::arch::x86_64::vga::Color::Black,
        );
        self.utf8_buf = [0; 4];
        self.utf8_len = 0;
        self.utf8_expected = 0;
        self.initialized = !fb_ptr.is_null() && self.width >= 16 && self.height >= 16;

        if self.initialized {
            // Allocate contiguous shadow buffer in fast cached system RAM
            let total_bytes = self.pitch * self.height;
            let total_pages = (total_bytes + crate::mm::PAGE_SIZE - 1) / crate::mm::PAGE_SIZE;
            if total_pages > 0 {
                if let Some(mut pmm) = crate::mm::pmm::PMM.try_lock() {
                    if let Some(frame) = pmm.alloc_contiguous_frames(total_pages) {
                        let virt = crate::mm::phys_to_virt(frame) as *mut u8;
                        unsafe {
                            core::ptr::write_bytes(virt, 0, total_pages * crate::mm::PAGE_SIZE);
                        }
                        self.shadow_fb = virt;
                        self.shadow_frame = frame;
                        self.shadow_pages = total_pages;
                    }
                }
            }

            // Allocate active grid buffer from fast cached system RAM (5 pages)
            const GRID_BYTES: usize = MAX_GRID_ROWS * MAX_GRID_COLS * core::mem::size_of::<ScreenChar>();
            let grid_pages = (GRID_BYTES + crate::mm::PAGE_SIZE - 1) / crate::mm::PAGE_SIZE;
            if let Some(mut pmm) = crate::mm::pmm::PMM.try_lock() {
                if let Some(frame) = pmm.alloc_contiguous_frames(grid_pages) {
                    let virt = crate::mm::phys_to_virt(frame) as *mut ScreenChar;
                    for i in 0..(MAX_GRID_ROWS * MAX_GRID_COLS) {
                        unsafe { *virt.add(i) = EMPTY_CELL; }
                    }
                    self.grid_ptr = virt;
                    self.grid_frame = frame;
                    self.grid_pages = grid_pages;
                }
            }

            // Allocate scrollback history buffer from fast cached system RAM (20 pages = 256 lines)
            const SB_BYTES: usize = SCROLLBACK_CAP * MAX_GRID_COLS * core::mem::size_of::<ScreenChar>();
            let sb_pages = (SB_BYTES + crate::mm::PAGE_SIZE - 1) / crate::mm::PAGE_SIZE;
            if let Some(mut pmm) = crate::mm::pmm::PMM.try_lock() {
                if let Some(frame) = pmm.alloc_contiguous_frames(sb_pages) {
                    let virt = crate::mm::phys_to_virt(frame) as *mut ScreenChar;
                    for i in 0..(SCROLLBACK_CAP * MAX_GRID_COLS) {
                        unsafe { *virt.add(i) = EMPTY_CELL; }
                    }
                    self.scrollback_ptr = virt;
                    self.scrollback_frame = frame;
                    self.scrollback_pages = sb_pages;
                }
            }

            self.clear_screen();
        }
    }

    pub fn free_shadow_buffer(&mut self) {
        if !self.shadow_fb.is_null() && self.shadow_pages > 0 {
            if let Some(mut pmm) = crate::mm::pmm::PMM.try_lock() {
                for i in 0..self.shadow_pages {
                    pmm.free_frame(self.shadow_frame + (i * crate::mm::PAGE_SIZE) as u64);
                }
            }
            self.shadow_fb = core::ptr::null_mut();
            self.shadow_frame = 0;
            self.shadow_pages = 0;
        }

        if !self.grid_ptr.is_null() && self.grid_pages > 0 {
            if let Some(mut pmm) = crate::mm::pmm::PMM.try_lock() {
                for i in 0..self.grid_pages {
                    pmm.free_frame(self.grid_frame + (i * crate::mm::PAGE_SIZE) as u64);
                }
            }
            self.grid_ptr = core::ptr::null_mut();
            self.grid_frame = 0;
            self.grid_pages = 0;
        }

        if !self.scrollback_ptr.is_null() && self.scrollback_pages > 0 {
            if let Some(mut pmm) = crate::mm::pmm::PMM.try_lock() {
                for i in 0..self.scrollback_pages {
                    pmm.free_frame(self.scrollback_frame + (i * crate::mm::PAGE_SIZE) as u64);
                }
            }
            self.scrollback_ptr = core::ptr::null_mut();
            self.scrollback_frame = 0;
            self.scrollback_pages = 0;
        }
    }

    pub fn fill_rect_rgb(&self, x: usize, y: usize, w: usize, h: usize, rgb: (u8, u8, u8)) {
        if !self.initialized {
            return;
        }
        let max_x = (x + w).min(self.width);
        let max_y = (y + h).min(self.height);
        let packed = self.pack_rgb(rgb.0, rgb.1, rgb.2);

        // 1. Fill fast cached system RAM shadow buffer if present
        if !self.shadow_fb.is_null() {
            if self.bpp == 32 {
                for py in y..max_y {
                    let row_ptr = unsafe { self.shadow_fb.add(py * self.pitch) as *mut u32 };
                    for px in x..max_x {
                        unsafe {
                            *row_ptr.add(px) = packed;
                        }
                    }
                }
            } else {
                let bytes = [(packed & 0xFF) as u8, ((packed >> 8) & 0xFF) as u8, ((packed >> 16) & 0xFF) as u8];
                let step = (self.bpp as usize / 8).max(3);
                for py in y..max_y {
                    let row_base = unsafe { self.shadow_fb.add(py * self.pitch) };
                    for px in x..max_x {
                        let p = unsafe { row_base.add(px * step) };
                        unsafe {
                            *p = bytes[0];
                            *p.add(1) = bytes[1];
                            *p.add(2) = bytes[2];
                        }
                    }
                }
            }
        }

        // 2. Burst write to Write-Combining VRAM
        if !self.fb_ptr.is_null() {
            if self.bpp == 32 {
                for py in y..max_y {
                    let row_ptr = unsafe { self.fb_ptr.add(py * self.pitch) as *mut u32 };
                    for px in x..max_x {
                        unsafe {
                            core::ptr::write_volatile(row_ptr.add(px), packed);
                        }
                    }
                }
            } else {
                let bytes = [(packed & 0xFF) as u8, ((packed >> 8) & 0xFF) as u8, ((packed >> 16) & 0xFF) as u8];
                let step = (self.bpp as usize / 8).max(3);
                for py in y..max_y {
                    let row_base = unsafe { self.fb_ptr.add(py * self.pitch) };
                    for px in x..max_x {
                        let p = unsafe { row_base.add(px * step) };
                        unsafe {
                            core::ptr::write_volatile(p, bytes[0]);
                            core::ptr::write_volatile(p.add(1), bytes[1]);
                            core::ptr::write_volatile(p.add(2), bytes[2]);
                        }
                    }
                }
            }
        }
    }

    fn fill_rect_shadow_only(&self, x: usize, y: usize, w: usize, h: usize, rgb: (u8, u8, u8)) {
        if self.shadow_fb.is_null() {
            return;
        }
        let max_x = (x + w).min(self.width);
        let max_y = (y + h).min(self.height);
        let packed = self.pack_rgb(rgb.0, rgb.1, rgb.2);

        if self.bpp == 32 {
            for py in y..max_y {
                let row_ptr = unsafe { self.shadow_fb.add(py * self.pitch) as *mut u32 };
                for px in x..max_x {
                    unsafe {
                        *row_ptr.add(px) = packed;
                    }
                }
            }
        } else {
            let bytes = [(packed & 0xFF) as u8, ((packed >> 8) & 0xFF) as u8, ((packed >> 16) & 0xFF) as u8];
            let step = (self.bpp as usize / 8).max(3);
            for py in y..max_y {
                let row_base = unsafe { self.shadow_fb.add(py * self.pitch) };
                for px in x..max_x {
                    let p = unsafe { row_base.add(px * step) };
                    unsafe {
                        *p = bytes[0];
                        *p.add(1) = bytes[1];
                        *p.add(2) = bytes[2];
                    }
                }
            }
        }
    }

    #[inline(always)]
    pub fn cell_w(&self) -> usize {
        8 * self.font_scale
    }

    #[inline(always)]
    pub fn cell_h(&self) -> usize {
        16 * self.font_scale
    }

    pub fn recalc_dimensions(&mut self) {
        let cw = self.cell_w();
        let ch = self.cell_h();
        self.cols = (self.width.saturating_sub(self.margin_x * 2) / cw).min(MAX_GRID_COLS).max(1);
        self.rows = (self.height.saturating_sub(self.margin_y * 2) / ch).min(MAX_GRID_ROWS).max(1);
    }

    pub fn set_font_scale(&mut self, scale: usize) {
        let scale = scale.clamp(1, 2);
        if self.font_scale != scale {
            self.font_scale = scale;
            self.recalc_dimensions();
            self.clear_screen();
        }
    }

    pub fn set_anti_alias(&mut self, enabled: bool) {
        if self.anti_alias != enabled {
            self.anti_alias = enabled;
            self.render_view();
        }
    }

    pub fn clear_screen(&mut self) {
        self.fill_rect_rgb(0, 0, self.width, self.height, self.bg_rgb);
        self.col = 0;
        self.row = 0;
        self.view_offset = 0;
        for r in 0..MAX_GRID_ROWS {
            for c in 0..MAX_GRID_COLS {
                self.set_active_cell(r, c, EMPTY_CELL);
            }
        }
    }

    fn draw_cell_direct(&self, col: usize, row: usize, ch: char, fg: (u8, u8, u8), bg: (u8, u8, u8)) {
        if !self.initialized || col >= self.cols || row >= self.rows {
            return;
        }
        let glyph = get_glyph_data(ch);
        let cw = self.cell_w();
        let ch_h = self.cell_h();
        let x0 = self.margin_x + col * cw;
        let y0 = self.margin_y + row * ch_h;
        let fg_px = self.pack_rgb(fg.0, fg.1, fg.2);
        let bg_px = self.pack_rgb(bg.0, bg.1, bg.2);
        let blend_50 = self.pack_rgb(
            ((fg.0 as u16 + bg.0 as u16) / 2) as u8,
            ((fg.1 as u16 + bg.1 as u16) / 2) as u8,
            ((fg.2 as u16 + bg.2 as u16) / 2) as u8,
        );
        let blend_25 = self.pack_rgb(
            ((fg.0 as u16 * 25 + bg.0 as u16 * 75) / 100) as u8,
            ((fg.1 as u16 * 25 + bg.1 as u16 * 75) / 100) as u8,
            ((fg.2 as u16 * 25 + bg.2 as u16 * 75) / 100) as u8,
        );

        let get_bit = |gx: isize, gy: isize| -> bool {
            if gx < 0 || gx >= 8 || gy < 0 || gy >= 16 {
                false
            } else {
                (glyph[gy as usize] & (0x80 >> (gx as usize))) != 0
            }
        };

        if self.font_scale == 2 {
            // 16x32 Cell: 2x expansion with subpixel morphological anti-aliasing
            if !self.shadow_fb.is_null() && self.bpp == 32 {
                for dy in 0..16 {
                    let py0 = y0 + dy * 2;
                    let py1 = py0 + 1;
                    if py0 >= self.height { break; }

                    let s_row0 = unsafe { self.shadow_fb.add(py0 * self.pitch + x0 * 4) as *mut u32 };
                    let fb_row0 = unsafe { self.fb_ptr.add(py0 * self.pitch + x0 * 4) as *mut u64 };
                    let s_row1 = if py1 < self.height {
                        unsafe { self.shadow_fb.add(py1 * self.pitch + x0 * 4) as *mut u32 }
                    } else {
                        core::ptr::null_mut()
                    };
                    let fb_row1 = if py1 < self.height {
                        unsafe { self.fb_ptr.add(py1 * self.pitch + x0 * 4) as *mut u64 }
                    } else {
                        core::ptr::null_mut()
                    };

                    for dx in 0..8 {
                        let is_fg = get_bit(dx as isize, dy as isize);
                        let (c_tl, c_tr, c_bl, c_br) = if is_fg {
                            (fg_px, fg_px, fg_px, fg_px)
                        } else if self.anti_alias {
                            let up = get_bit(dx as isize, dy as isize - 1);
                            let down = get_bit(dx as isize, dy as isize + 1);
                            let left = get_bit(dx as isize - 1, dy as isize);
                            let right = get_bit(dx as isize + 1, dy as isize);

                            (
                                if up && left { blend_50 } else { bg_px },
                                if up && right { blend_50 } else { bg_px },
                                if down && left { blend_50 } else { bg_px },
                                if down && right { blend_50 } else { bg_px },
                            )
                        } else {
                            (bg_px, bg_px, bg_px, bg_px)
                        };

                        unsafe {
                            *s_row0.add(dx * 2) = c_tl;
                            *s_row0.add(dx * 2 + 1) = c_tr;
                            if !s_row1.is_null() {
                                *s_row1.add(dx * 2) = c_bl;
                                *s_row1.add(dx * 2 + 1) = c_br;
                            }
                        }
                    }

                    // Flush 16 pixels (64 bytes = 8 u64) per row to WC VRAM
                    unsafe {
                        let s0 = s_row0 as *const u64;
                        core::ptr::write_volatile(fb_row0, *s0);
                        core::ptr::write_volatile(fb_row0.add(1), *s0.add(1));
                        core::ptr::write_volatile(fb_row0.add(2), *s0.add(2));
                        core::ptr::write_volatile(fb_row0.add(3), *s0.add(3));
                        core::ptr::write_volatile(fb_row0.add(4), *s0.add(4));
                        core::ptr::write_volatile(fb_row0.add(5), *s0.add(5));
                        core::ptr::write_volatile(fb_row0.add(6), *s0.add(6));
                        core::ptr::write_volatile(fb_row0.add(7), *s0.add(7));

                        if !fb_row1.is_null() && !s_row1.is_null() {
                            let s1 = s_row1 as *const u64;
                            core::ptr::write_volatile(fb_row1, *s1);
                            core::ptr::write_volatile(fb_row1.add(1), *s1.add(1));
                            core::ptr::write_volatile(fb_row1.add(2), *s1.add(2));
                            core::ptr::write_volatile(fb_row1.add(3), *s1.add(3));
                            core::ptr::write_volatile(fb_row1.add(4), *s1.add(4));
                            core::ptr::write_volatile(fb_row1.add(5), *s1.add(5));
                            core::ptr::write_volatile(fb_row1.add(6), *s1.add(6));
                            core::ptr::write_volatile(fb_row1.add(7), *s1.add(7));
                        }
                    }
                }
            } else if self.bpp == 32 {
                for dy in 0..16 {
                    let py0 = y0 + dy * 2;
                    let py1 = py0 + 1;
                    if py0 >= self.height { break; }
                    let row_ptr0 = unsafe { self.fb_ptr.add(py0 * self.pitch) as *mut u32 };
                    let row_ptr1 = if py1 < self.height {
                        unsafe { self.fb_ptr.add(py1 * self.pitch) as *mut u32 }
                    } else {
                        core::ptr::null_mut()
                    };
                    for dx in 0..8 {
                        let is_fg = get_bit(dx as isize, dy as isize);
                        let c = if is_fg { fg_px } else { bg_px };
                        unsafe {
                            core::ptr::write_volatile(row_ptr0.add(x0 + dx * 2), c);
                            core::ptr::write_volatile(row_ptr0.add(x0 + dx * 2 + 1), c);
                            if !row_ptr1.is_null() {
                                core::ptr::write_volatile(row_ptr1.add(x0 + dx * 2), c);
                                core::ptr::write_volatile(row_ptr1.add(x0 + dx * 2 + 1), c);
                            }
                        }
                    }
                }
            }
        } else {
            // Scale 1 (8x16) Cell
            if !self.shadow_fb.is_null() && self.bpp == 32 {
                for dy in 0..16 {
                    let py = y0 + dy;
                    if py >= self.height { break; }
                    let shadow_row = unsafe { self.shadow_fb.add(py * self.pitch + x0 * 4) as *mut u32 };
                    let fb_row = unsafe { self.fb_ptr.add(py * self.pitch + x0 * 4) as *mut u64 };

                    for dx in 0..8 {
                        let is_fg = get_bit(dx as isize, dy as isize);
                        let color = if is_fg {
                            fg_px
                        } else if self.anti_alias {
                            let up = get_bit(dx as isize, dy as isize - 1);
                            let down = get_bit(dx as isize, dy as isize + 1);
                            let left = get_bit(dx as isize - 1, dy as isize);
                            let right = get_bit(dx as isize + 1, dy as isize);
                            if (up && left) || (up && right) || (down && left) || (down && right) {
                                blend_25
                            } else {
                                bg_px
                            }
                        } else {
                            bg_px
                        };
                        unsafe {
                            *shadow_row.add(dx) = color;
                        }
                    }
                    // Flush 8 pixels (32 bytes = 4 u64)
                    unsafe {
                        let s = shadow_row as *const u64;
                        core::ptr::write_volatile(fb_row, *s);
                        core::ptr::write_volatile(fb_row.add(1), *s.add(1));
                        core::ptr::write_volatile(fb_row.add(2), *s.add(2));
                        core::ptr::write_volatile(fb_row.add(3), *s.add(3));
                    }
                }
            } else if self.bpp == 32 {
                for dy in 0..16 {
                    let bits = glyph[dy];
                    let py = y0 + dy;
                    if py >= self.height { break; }
                    let row_ptr = unsafe { self.fb_ptr.add(py * self.pitch) as *mut u32 };
                    for dx in 0..8 {
                        let color = if (bits & (0x80 >> dx)) != 0 { fg_px } else { bg_px };
                        unsafe {
                            core::ptr::write_volatile(row_ptr.add(x0 + dx), color);
                        }
                    }
                }
            } else {
                let step = (self.bpp as usize / 8).max(3);
                let fg_b = [(fg_px & 0xFF) as u8, ((fg_px >> 8) & 0xFF) as u8, ((fg_px >> 16) & 0xFF) as u8];
                let bg_b = [(bg_px & 0xFF) as u8, ((bg_px >> 8) & 0xFF) as u8, ((bg_px >> 16) & 0xFF) as u8];
                for dy in 0..16 {
                    let bits = glyph[dy];
                    let py = y0 + dy;
                    if py >= self.height { break; }
                    let row_base = unsafe { self.fb_ptr.add(py * self.pitch) };
                    for dx in 0..8 {
                        let b = if (bits & (0x80 >> dx)) != 0 { fg_b } else { bg_b };
                        let p = unsafe { row_base.add((x0 + dx) * step) };
                        unsafe {
                            core::ptr::write_volatile(p, b[0]);
                            core::ptr::write_volatile(p.add(1), b[1]);
                            core::ptr::write_volatile(p.add(2), b[2]);
                        }
                    }
                }
            }
        }
    }

    fn draw_cell(&mut self, col: usize, row: usize, ch: char, fg: (u8, u8, u8), bg: (u8, u8, u8)) {
        if !self.initialized || col >= self.cols || row >= self.rows {
            return;
        }
        if row < MAX_GRID_ROWS && col < MAX_GRID_COLS {
            let ascii_byte = if (ch as u32) < 128 { ch as u8 } else { b'?' };
            self.set_active_cell(row, col, ScreenChar {
                ascii_character: ascii_byte,
                color_code: self.current_color_code,
            });
        }
        self.draw_cell_direct(col, row, ch, fg, bg);
    }

    fn new_line(&mut self) {
        if self.view_offset > 0 {
            self.view_offset = 0;
            self.render_view();
        }
        if self.row + 1 < self.rows {
            self.row += 1;
        } else {
            // Push top row of active grid to scrollback ring buffer
            if self.cols <= MAX_GRID_COLS {
                for c in 0..self.cols {
                    let cell = self.get_active_cell(0, c);
                    self.set_scrollback_cell(self.scrollback_head, c, cell);
                }
                for c in self.cols..MAX_GRID_COLS {
                    self.set_scrollback_cell(self.scrollback_head, c, EMPTY_CELL);
                }
            }
            self.scrollback_head = (self.scrollback_head + 1) % SCROLLBACK_CAP;
            if self.scrollback_count < SCROLLBACK_CAP {
                self.scrollback_count += 1;
            }

            // Shift active grid rows up by 1
            for r in 1..self.rows.min(MAX_GRID_ROWS) {
                for c in 0..self.cols.min(MAX_GRID_COLS) {
                    let cell = self.get_active_cell(r, c);
                    self.set_active_cell(r - 1, c, cell);
                }
            }
            if self.rows > 0 && self.rows <= MAX_GRID_ROWS {
                for c in 0..self.cols.min(MAX_GRID_COLS) {
                    self.set_active_cell(self.rows - 1, c, EMPTY_CELL);
                }
            }

            let cw = self.cell_w();
            let ch = self.cell_h();
            let line_bytes = self.pitch * ch;
            let total_rows = self.rows.saturating_sub(1);
            if total_rows > 0 && self.pitch > 0 {
                if !self.shadow_fb.is_null() {
                    unsafe {
                        let src_y = self.margin_y + ch;
                        let dst_y = self.margin_y;
                        let src = self.shadow_fb.add(src_y * self.pitch);
                        let dst = self.shadow_fb.add(dst_y * self.pitch);
                        core::ptr::copy(src, dst, total_rows * line_bytes);
                    }
                    self.fill_rect_shadow_only(
                        self.margin_x,
                        self.margin_y + total_rows * ch,
                        self.cols * cw,
                        ch,
                        self.bg_rgb,
                    );
                    self.flush_shadow_to_vram();
                } else {
                    unsafe {
                        let src_y = self.margin_y + ch;
                        let dst_y = self.margin_y;
                        let src = self.fb_ptr.add(src_y * self.pitch);
                        let dst = self.fb_ptr.add(dst_y * self.pitch);
                        core::ptr::copy(src, dst, total_rows * line_bytes);
                    }
                    self.fill_rect_rgb(
                        self.margin_x,
                        self.margin_y + total_rows * ch,
                        self.cols * cw,
                        ch,
                        self.bg_rgb,
                    );
                }
            }
        }
        self.col = 0;
    }

    pub fn scroll_up(&mut self, lines: usize) {
        if self.scrollback_count == 0 {
            return;
        }
        let new_offset = (self.view_offset + lines).min(self.scrollback_count);
        if new_offset != self.view_offset {
            self.view_offset = new_offset;
            self.render_view();
        }
    }

    pub fn scroll_down(&mut self, lines: usize) {
        if self.view_offset == 0 {
            return;
        }
        let new_offset = self.view_offset.saturating_sub(lines);
        if new_offset != self.view_offset {
            self.view_offset = new_offset;
            self.render_view();
        }
    }

    pub fn reset_scroll_view(&mut self) {
        if self.view_offset > 0 {
            self.view_offset = 0;
            self.render_view();
        }
    }

    pub fn render_view(&mut self) {
        if !self.initialized {
            return;
        }
        for r in 0..self.rows {
            let target_line = (r as isize) - (self.view_offset as isize);
            for c in 0..self.cols {
                let cell = if target_line >= 0 {
                    self.get_active_cell(target_line as usize, c)
                } else {
                    let back = (-target_line) as usize;
                    if back <= self.scrollback_count {
                        let sb_idx = (self.scrollback_head + SCROLLBACK_CAP - back) % SCROLLBACK_CAP;
                        self.get_scrollback_cell(sb_idx, c)
                    } else {
                        EMPTY_CELL
                    }
                };
                let (fg_idx, bg_idx) = cell.color_code.indices();
                let fg = Self::vga_color_to_rgb(fg_idx);
                let bg = Self::vga_color_to_rgb(bg_idx);
                let ch = match cell.ascii_character {
                    0x20..=0x7E => cell.ascii_character as char,
                    0x80..=0xFF => Self::cp437_byte_to_char(cell.ascii_character),
                    _ => ' ',
                };
                self.draw_cell_direct(c, r, ch, fg, bg);
            }
        }
        self.flush_shadow_to_vram();
    }

    /// Burst write active text lines from cached RAM shadow buffer to Write-Combining VRAM.
    /// Operates via 64-bit (`u64`) stores to maximize PCIe write-combining FIFO bursts.
    pub fn flush_shadow_to_vram(&self) {
        if self.shadow_fb.is_null() || self.fb_ptr.is_null() {
            return;
        }
        let total_bytes = self.pitch * self.height;
        let u64_count = total_bytes / 8;
        let src = self.shadow_fb as *const u64;
        let dst = self.fb_ptr as *mut u64;
        unsafe {
            for i in 0..u64_count {
                core::ptr::write_volatile(dst.add(i), *src.add(i));
            }
            let rem_start = u64_count * 8;
            if rem_start < total_bytes {
                let src_u8 = self.shadow_fb.add(rem_start);
                let dst_u8 = self.fb_ptr.add(rem_start);
                for i in 0..(total_bytes - rem_start) {
                    core::ptr::write_volatile(dst_u8.add(i), *src_u8.add(i));
                }
            }
        }
    }

    fn decode_utf8_byte(&mut self, b: u8) -> Option<char> {
        if self.utf8_len == 0 {
            if b < 0x80 {
                Some(b as char)
            } else if (b & 0xE0) == 0xC0 && (b & 0x1E) != 0 {
                self.utf8_buf[0] = b;
                self.utf8_len = 1;
                self.utf8_expected = 2;
                None
            } else if (b & 0xF0) == 0xE0 {
                self.utf8_buf[0] = b;
                self.utf8_len = 1;
                self.utf8_expected = 3;
                None
            } else if (b & 0xF8) == 0xF0 && b <= 0xF4 {
                self.utf8_buf[0] = b;
                self.utf8_len = 1;
                self.utf8_expected = 4;
                None
            } else {
                Some(Self::cp437_byte_to_char(b))
            }
        } else {
            if (b & 0xC0) == 0x80 {
                self.utf8_buf[self.utf8_len as usize] = b;
                self.utf8_len += 1;
                if self.utf8_len == self.utf8_expected {
                    let len = self.utf8_len as usize;
                    self.utf8_len = 0;
                    self.utf8_expected = 0;
                    match core::str::from_utf8(&self.utf8_buf[..len]) {
                        Ok(s) => s.chars().next(),
                        Err(_) => Some('?'),
                    }
                } else {
                    None
                }
            } else {
                self.utf8_len = 0;
                self.utf8_expected = 0;
                self.decode_utf8_byte(b)
            }
        }
    }

    pub fn cp437_byte_to_char(b: u8) -> char {
        match b {
            0x20..=0x7E => b as char,
            0xDB => '█',
            0xC9 => '╔',
            0xCD => '═',
            0xBB => '╗',
            0xBA => '║',
            0xC8 => '╚',
            0xBC => '╝',
            0xCC => '╠',
            0xB9 => '╣',
            0xCB => '╦',
            0xCA => '╩',
            0xCE => '╬',
            0xB0 => '░',
            0xB1 => '▒',
            0xB2 => '▓',
            0xDF => '▀',
            0xDC => '▄',
            0xC4 => '─',
            0xB3 => '│',
            0xDA => '┌',
            0xBF => '┐',
            0xC0 => '└',
            0xD9 => '┘',
            0xC3 => '├',
            0xB4 => '┤',
            0xC2 => '┬',
            0xC1 => '┴',
            0xC5 => '┼',
            _ => '?',
        }
    }

    pub fn write_byte(&mut self, byte: u8) {
        if !self.initialized {
            return;
        }

        let (consumed, command) = self.ansi.process_byte(byte);
        if let Some(cmd) = command {
            self.utf8_len = 0;
            self.utf8_expected = 0;
            match cmd {
                AnsiCommand::SetColor { fg, bg } => {
                    self.current_color_code = crate::arch::x86_64::vga::ColorCode::new(fg, bg);
                    self.fg_rgb = Self::vga_color_to_rgb(fg as u8);
                    self.bg_rgb = Self::vga_color_to_rgb(bg as u8);
                }
                AnsiCommand::ClearScreen => self.clear_screen(),
                AnsiCommand::ClearLine => {
                    let cw = self.cell_w();
                    let ch = self.cell_h();
                    let start_x = self.margin_x + self.col * cw;
                    let width_to_clear = (self.cols.saturating_sub(self.col)) * cw;
                    self.fill_rect_rgb(
                        start_x,
                        self.margin_y + self.row * ch,
                        width_to_clear,
                        ch,
                        self.bg_rgb,
                    );
                    if self.row < MAX_GRID_ROWS {
                        for c in self.col..self.cols.min(MAX_GRID_COLS) {
                            self.set_active_cell(self.row, c, EMPTY_CELL);
                        }
                    }
                }
                AnsiCommand::CursorPosition { row, col } => {
                    self.row = row.min(self.rows - 1);
                    self.col = col.min(self.cols - 1);
                }
                AnsiCommand::CursorUp(n) => self.row = self.row.saturating_sub(n),
                AnsiCommand::CursorDown(n) => self.row = (self.row + n).min(self.rows - 1),
                AnsiCommand::CursorForward(n) => self.col = (self.col + n).min(self.cols - 1),
                AnsiCommand::CursorBack(n) => self.col = self.col.saturating_sub(n),
                _ => {}
            }
            return;
        }
        if consumed {
            self.utf8_len = 0;
            self.utf8_expected = 0;
            return;
        }

        let ch = match self.decode_utf8_byte(byte) {
            Some(c) => c,
            None => return,
        };

        match ch {
            '\n' => self.new_line(),
            '\r' => self.col = 0,
            '\x08' | '\x7F' => {
                if self.col > 0 {
                    self.col -= 1;
                    self.draw_cell(self.col, self.row, ' ', self.fg_rgb, self.bg_rgb);
                }
            }
            '\t' => {
                for _ in 0..4 {
                    if self.col >= self.cols {
                        self.new_line();
                    }
                    self.draw_cell(self.col, self.row, ' ', self.fg_rgb, self.bg_rgb);
                    self.col += 1;
                }
            }
            c => {
                if self.col >= self.cols {
                    self.new_line();
                }
                self.draw_cell(self.col, self.row, c, self.fg_rgb, self.bg_rgb);
                self.col += 1;
            }
        }
    }

    pub fn write_bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.write_byte(b);
        }
    }

    pub fn flush_virtual_console(&mut self, console: &VirtualConsole) {
        if !self.initialized {
            return;
        }
        self.view_offset = 0;
        let max_rows = self.rows.min(crate::arch::x86_64::vga::BUFFER_HEIGHT);
        let max_cols = self.cols.min(crate::arch::x86_64::vga::BUFFER_WIDTH);

        for r in 0..max_rows {
            for c in 0..max_cols {
                let cell: ScreenChar = console.buffer[r * crate::arch::x86_64::vga::BUFFER_WIDTH + c];
                let (fg_idx, bg_idx) = cell.color_code.indices();
                let ch = match cell.ascii_character {
                    0x20..=0x7E => cell.ascii_character as char,
                    0x80..=0xFF => Self::cp437_byte_to_char(cell.ascii_character),
                    _ => ' ',
                };
                self.current_color_code = cell.color_code;
                self.draw_cell(
                    c,
                    r,
                    ch,
                    Self::vga_color_to_rgb(fg_idx),
                    Self::vga_color_to_rgb(bg_idx),
                );
            }
        }
        self.row = console.cursor_row.min(self.rows - 1);
        self.col = console.cursor_col.min(self.cols - 1);
    }
}

impl Drop for GopConsole {
    fn drop(&mut self) {
        self.free_shadow_buffer();
    }
}

pub static GOP_CONSOLE: IrqSafeSpinlock<GopConsole> = IrqSafeSpinlock::new(GopConsole::new());

/// True when a physical UEFI GOP / Multiboot LFB text console is mapped and active.
#[inline(always)]
pub fn is_active() -> bool {
    GOP_CONSOLE_ACTIVE.load(Ordering::Relaxed)
}

/// Suppress/resume GOP text output while `gui::launch_desktop()` owns the compositor.
pub fn set_gui_active(active: bool) {
    GUI_ACTIVE.store(active, Ordering::Release);
}

/// Bind `GopConsole` to a mapped GOP framebuffer and replay buffered early boot logs.
pub fn init_from_framebuffer(
    fb_ptr: *mut u8,
    width: u32,
    height: u32,
    pitch: u32,
    bpp: u32,
    red_pos: u8,
    green_pos: u8,
    blue_pos: u8,
) {
    {
        let mut con = GOP_CONSOLE.lock();
        con.configure(fb_ptr, width, height, pitch, bpp, red_pos, green_pos, blue_pos);
        if con.initialized {
            let early = EARLY_LOG.lock();
            if early.len > 0 {
                con.write_bytes(&early.data[..early.len]);
            }
        }
    }
    GOP_CONSOLE_ACTIVE.store(!fb_ptr.is_null(), Ordering::Release);
}

/// Record early logs prior to GOP mapping or stream live text onto active GOP display.
pub fn write_fmt(args: fmt::Arguments) {
    if !is_active() {
        struct EarlyWriter;
        impl Write for EarlyWriter {
            fn write_str(&mut self, s: &str) -> fmt::Result {
                if let Some(mut buf) = EARLY_LOG.try_lock() {
                    buf.push_bytes(s.as_bytes());
                }
                Ok(())
            }
        }
        let mut w = EarlyWriter;
        let _ = w.write_fmt(args);
        return;
    }

    if GUI_ACTIVE.load(Ordering::Relaxed) {
        return;
    }

    struct LiveWriter;
    impl Write for LiveWriter {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            if let Some(mut con) = GOP_CONSOLE.try_lock() {
                con.write_bytes(s.as_bytes());
            }
            Ok(())
        }
    }
    let mut w = LiveWriter;
    let _ = w.write_fmt(args);
}

/// Stream a single slice/byte from `TtyManager` to GOP console if active.
pub fn tty_write_bytes(bytes: &[u8]) {
    if !is_active() || GUI_ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    if let Some(mut con) = GOP_CONSOLE.try_lock() {
        con.write_bytes(bytes);
    }
}

/// Flush an entire virtual TTY screen buffer onto the GOP console during TTY switches.
pub fn flush_tty_console(console: &VirtualConsole) {
    if !is_active() || GUI_ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    if let Some(mut con) = GOP_CONSOLE.try_lock() {
        con.flush_virtual_console(console);
    }
}

/// Switch GOP console out of GUI compositor suppression into visible high-contrast panic output.
pub fn enter_panic_mode() {
    GUI_ACTIVE.store(false, Ordering::Release);
    if !is_active() {
        return;
    }
    unsafe {
        GOP_CONSOLE.force_unlock();
    }
    if let Some(mut con) = GOP_CONSOLE.try_lock() {
        con.bg_rgb = (16, 20, 26);
        con.fg_rgb = (255, 102, 102);
        con.write_bytes(b"\n");
    }
}

/// Scroll console view up by `lines` into historical buffer (e.g., Shift+PageUp or Shift+Up)
pub fn scroll_up(lines: usize) {
    if !is_active() || GUI_ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    if let Some(mut con) = GOP_CONSOLE.try_lock() {
        con.scroll_up(lines);
    }
}

/// Scroll console view down by `lines` towards live screen (e.g., Shift+PageDown or Shift+Down)
pub fn scroll_down(lines: usize) {
    if !is_active() || GUI_ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    if let Some(mut con) = GOP_CONSOLE.try_lock() {
        con.scroll_down(lines);
    }
}

/// Reset console view back to live output (offset 0)
pub fn reset_scroll_view() {
    if !is_active() || GUI_ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    if let Some(mut con) = GOP_CONSOLE.try_lock() {
        con.reset_scroll_view();
    }
}

/// Change font scale (1 = 8x16, 2 = 16x32)
pub fn set_font_scale(scale: usize) {
    if let Some(mut con) = GOP_CONSOLE.try_lock() {
        con.set_font_scale(scale);
    }
}

/// Enable or disable subpixel anti-aliasing
pub fn set_anti_alias(enabled: bool) {
    if let Some(mut con) = GOP_CONSOLE.try_lock() {
        con.set_anti_alias(enabled);
    }
}
