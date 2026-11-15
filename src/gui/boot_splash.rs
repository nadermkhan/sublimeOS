use alloc::string::String;
use alloc::vec;
use core::sync::atomic::{AtomicBool, Ordering};
use libgui::font::{with_default_font, TextStyle};
use libgui::gfx::{Color as GuiColor, Rect, Surface};
use crate::drivers::vbe::VBE;
use crate::sync::IrqSafeSpinlock;

const CARD_W: i32 = 460;
const CARD_H: i32 = 276;
const PROGRESS_BAND_Y: i32 = 166;
const PROGRESS_BAND_H: i32 = CARD_H - PROGRESS_BAND_Y;

static SPLASH_ACTIVE: AtomicBool = AtomicBool::new(false);

struct SplashState {
    progress_pct: u8,
    stage: [u8; 64],
    stage_len: usize,
    fb_w: u32,
    fb_h: u32,
}

impl SplashState {
    const fn new() -> Self {
        Self {
            progress_pct: 0,
            stage: [0; 64],
            stage_len: 0,
            fb_w: 0,
            fb_h: 0,
        }
    }

    fn set_label(&mut self, label: &str) {
        let bytes = label.as_bytes();
        let n = bytes.len().min(self.stage.len());
        self.stage[..n].copy_from_slice(&bytes[..n]);
        self.stage_len = n;
    }

    fn label_str(&self) -> &str {
        core::str::from_utf8(&self.stage[..self.stage_len]).unwrap_or("")
    }
}

static SPLASH_STATE: IrqSafeSpinlock<SplashState> = IrqSafeSpinlock::new(SplashState::new());

/// Return `true` while the graphical boot splash owns the boot screen.
pub fn is_active() -> bool {
    SPLASH_ACTIVE.load(Ordering::Acquire)
}

/// Snapshot current splash state `(active, progress_pct, stage_label)` for diagnostics/tests.
pub fn snapshot() -> (bool, u8, String) {
    let st = SPLASH_STATE.lock();
    (SPLASH_ACTIVE.load(Ordering::Acquire), st.progress_pct, String::from(st.label_str()))
}

#[inline(always)]
fn pack_pixel(argb: u32, r_pos: u8, g_pos: u8, b_pos: u8) -> u32 {
    let r = ((argb >> 16) & 0xFF) as u32;
    let g = ((argb >> 8) & 0xFF) as u32;
    let b = (argb & 0xFF) as u32;
    (r << (r_pos & 31)) | (g << (g_pos & 31)) | (b << (b_pos & 31))
}

#[inline(always)]
fn lerp_u8(a: u8, b: u8, t: i32, den: i32) -> u8 {
    let d = den.max(1);
    (a as i32 + ((b as i32 - a as i32) * t) / d).clamp(0, 255) as u8
}

#[inline(always)]
fn backdrop_pixel_argb(x: i32, y: i32, w: i32, h: i32) -> u32 {
    let r = lerp_u8(8, 17, y, h);
    let g = lerp_u8(13, 27, y, h);
    let b = lerp_u8(24, 46, y, h);

    // Subtle soft cyan glow behind the centered emblem without heavy floating-point ops
    let cx = w / 2;
    let cy = (h / 2) - 48;
    let dx = (x - cx).abs();
    let dy = (y - cy).abs();
    let dist_sq = dx.saturating_mul(dx) + dy.saturating_mul(dy);
    let max_sq = 280 * 280;
    if dist_sq < max_sq {
        let boost = ((max_sq - dist_sq) * 34) / max_sq;
        let rr = (r as i32 + boost / 4).min(255) as u32;
        let gg = (g as i32 + (boost * 3) / 4).min(255) as u32;
        let bb = (b as i32 + boost).min(255) as u32;
        return 0xFF00_0000 | (rr << 16) | (gg << 8) | bb;
    }
    0xFF00_0000 | ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

fn paint_full_backdrop() {
    let (fb_ptr, w, h, pitch, bpp, rp, gp, bp, text_mode) = {
        let vbe = VBE.lock();
        (
            vbe.framebuffer as *mut u8,
            vbe.width,
            vbe.height,
            vbe.pitch,
            vbe.bpp,
            vbe.red_pos,
            vbe.green_pos,
            vbe.blue_pos,
            vbe.text_mode_fallback,
        )
    };
    if fb_ptr.is_null() || text_mode || w == 0 || h == 0 {
        return;
    }

    let wi = w as i32;
    let hi = h as i32;
    let bytes_pp = (bpp / 8).max(3) as usize;
    let mut row_buf = vec![0u32; w as usize];

    for y in 0..hi {
        for x in 0..wi {
            let argb = backdrop_pixel_argb(x, y, wi, hi);
            row_buf[x as usize] = pack_pixel(argb, rp, gp, bp);
        }
        let dst_row = unsafe { fb_ptr.add((y as usize) * (pitch as usize)) };
        if bpp == 32 && ((dst_row as usize) & 3) == 0 {
            unsafe {
                core::ptr::copy_nonoverlapping(row_buf.as_ptr(), dst_row as *mut u32, w as usize);
            }
        } else {
            for (x, &px) in row_buf.iter().enumerate() {
                unsafe {
                    let p = dst_row.add(x * bytes_pp);
                    *p = (px & 0xFF) as u8;
                    *p.add(1) = ((px >> 8) & 0xFF) as u8;
                    *p.add(2) = ((px >> 16) & 0xFF) as u8;
                    if bytes_pp >= 4 {
                        *p.add(3) = ((px >> 24) & 0xFF) as u8;
                    }
                }
            }
        }
    }
}

fn blit_surface_region_to_lfb(surf: &Surface, src_y: i32, src_h: i32, dst_x: i32, dst_y: i32) {
    let (fb_ptr, fb_w, fb_h, pitch, bpp, rp, gp, bp, text_mode) = {
        let vbe = VBE.lock();
        (
            vbe.framebuffer as *mut u8,
            vbe.width as i32,
            vbe.height as i32,
            vbe.pitch as usize,
            vbe.bpp,
            vbe.red_pos,
            vbe.green_pos,
            vbe.blue_pos,
            vbe.text_mode_fallback,
        )
    };
    if fb_ptr.is_null() || text_mode || fb_w <= 0 || fb_h <= 0 {
        return;
    }

    let sw = surf.width;
    let x0 = dst_x.max(0);
    let x1 = (dst_x + sw).min(fb_w);
    if x1 <= x0 {
        return;
    }
    let copy_w = (x1 - x0) as usize;
    let sx_off = (x0 - dst_x) as usize;
    let bytes_pp = (bpp / 8).max(3) as usize;
    let mut packed_row = vec![0u32; copy_w];

    for dy in 0..src_h {
        let sy = src_y + dy;
        let fy = dst_y + dy;
        if sy < 0 || sy >= surf.height || fy < 0 || fy >= fb_h {
            continue;
        }
        let s_row = &surf.pixels[(sy as usize) * (sw as usize) + sx_off..][..copy_w];
        for (i, &argb) in s_row.iter().enumerate() {
            packed_row[i] = pack_pixel(argb, rp, gp, bp);
        }
        let dst_row = unsafe { fb_ptr.add((fy as usize) * pitch + (x0 as usize) * bytes_pp) };
        if bpp == 32 && ((dst_row as usize) & 3) == 0 {
            unsafe {
                core::ptr::copy_nonoverlapping(packed_row.as_ptr(), dst_row as *mut u32, copy_w);
            }
        } else {
            for (i, &px) in packed_row.iter().enumerate() {
                unsafe {
                    let p = dst_row.add(i * bytes_pp);
                    *p = (px & 0xFF) as u8;
                    *p.add(1) = ((px >> 8) & 0xFF) as u8;
                    *p.add(2) = ((px >> 16) & 0xFF) as u8;
                    if bytes_pp >= 4 {
                        *p.add(3) = 0xFF;
                    }
                }
            }
        }
    }
}

fn fill_backdrop_in_card(surf: &mut Surface, fb_w: i32, fb_h: i32, card_x: i32, card_y: i32) {
    let cw = surf.width;
    let ch = surf.height;
    for ly in 0..ch {
        for lx in 0..cw {
            let argb = backdrop_pixel_argb(card_x + lx, card_y + ly, fb_w, fb_h);
            surf.pixels[(ly as usize) * (cw as usize) + (lx as usize)] = argb;
        }
    }
}

fn draw_sublime_emblem(surf: &mut Surface, cx: i32, cy: i32, radius: i32) {
    // Layered geometric diamond prism in teal, sky cyan, and sapphire blue
    for dy in -radius..=radius {
        let row_span = radius - dy.abs();
        let y = cy + dy;
        for dx in -row_span..=row_span {
            let x = cx + dx;
            let border = dx.abs() + dy.abs() >= radius - 1;
            let col = if border {
                GuiColor::rgb(138, 224, 255)
            } else if dy < 0 && dx <= 0 {
                GuiColor::rgb(52, 206, 248)
            } else if dy < 0 {
                GuiColor::rgb(38, 152, 238)
            } else if dx <= 0 {
                GuiColor::rgb(26, 118, 212)
            } else {
                GuiColor::rgb(18, 84, 172)
            };
            surf.set_pixel(x, y, col);
        }
    }

    // Crisp inner specular diamond highlight
    let inner = (radius / 2).max(6);
    for dy in -inner..=inner {
        let span = inner - dy.abs();
        for dx in -span..=span {
            let alpha_col = if dy <= 0 {
                GuiColor::argb(92, 255, 255, 255)
            } else {
                GuiColor::argb(42, 140, 232, 255)
            };
            surf.blend_pixel(cx + dx, cy + dy, alpha_col);
        }
    }
}

/// Render the centered SublimeOS boot splash hero card into any caller `Surface` (`>= 460x276`).
/// Exposed publicly so kernel self-tests verify emblem & progress rendering even in headless mode.
pub fn render_card_to_surface(surf: &mut Surface, pct: u8, label: &str, fb_w: i32, fb_h: i32) {
    let cw = surf.width;
    let ch = surf.height;
    let card_x = (fb_w - cw) / 2;
    let card_y = (fb_h - ch) / 2 - 12;
    fill_backdrop_in_card(surf, fb_w.max(cw), fb_h.max(ch), card_x.max(0), card_y.max(0));

    // Subtle frosted card outline + inner rim
    let panel = Rect::new(8, 8, cw - 16, ch - 16);
    surf.fill_rounded_rect(panel, 14, GuiColor::argb(210, 14, 22, 38));
    surf.draw_rounded_rect(panel, 14, GuiColor::argb(96, 92, 168, 235));

    // Centered crystalline emblem
    draw_sublime_emblem(surf, cw / 2, 62, 34);

    // Wordmark & version tagline
    with_default_font(|font| {
        let title = "SublimeOS";
        let title_st = TextStyle {
            bold: true,
            ..TextStyle::ui_sized(26, GuiColor::rgb(242, 247, 255))
        };
        let (tw, _) = font.measure_text(title, title_st);
        font.draw_text(surf, (cw - tw) / 2, 108, title, title_st);

        let sub = "Workstation 1.0  |  KiteKernel x86_64";
        let sub_st = TextStyle::ui_sized(11, GuiColor::rgb(138, 174, 214));
        let (sw, _) = font.measure_text(sub, sub_st);
        font.draw_text(surf, (cw - sw) / 2, 142, sub, sub_st);
    });

    // Progress track, accent fill, percentage readout, and live boot stage label
    let clamped = pct.min(100);
    let track = Rect::new(48, 186, cw - 96, 12);
    surf.fill_rounded_rect(track, 6, GuiColor::rgb(20, 32, 52));
    surf.draw_rounded_rect(track, 6, GuiColor::argb(120, 84, 134, 194));

    let inner_max = (track.w - 4).max(0);
    let fill_w = ((inner_max * (clamped as i32)) / 100).max(if clamped > 0 { 10 } else { 0 });
    if fill_w > 0 {
        let fill_r = Rect::new(track.x + 2, track.y + 2, fill_w.min(inner_max), track.h - 4);
        surf.fill_rounded_rect(fill_r, 4, GuiColor::rgb(46, 184, 255));
        let hi_r = Rect::new(fill_r.x, fill_r.y, fill_r.w, (fill_r.h / 2).max(1));
        surf.fill_rounded_rect(hi_r, 2, GuiColor::argb(110, 186, 240, 255));
    }

    with_default_font(|font| {
        let stage_st = TextStyle::ui_sized(11, GuiColor::rgb(198, 218, 242));
        let (lw, _) = font.measure_text(label, stage_st);
        let lx = ((cw - lw) / 2).max(24);
        font.draw_text_ellipsis(surf, lx, 212, cw - 48, label, stage_st);

        let pct_str = alloc::format!("{}%", clamped);
        let pct_st = TextStyle {
            bold: true,
            ..TextStyle::ui_sized(10, GuiColor::rgb(102, 206, 255))
        };
        let (pw, _) = font.measure_text(&pct_str, pct_st);
        font.draw_text(surf, (cw - pw) / 2, 234, &pct_str, pct_st);
    });
}

fn render_footer_to_lfb(fb_w: i32, fb_h: i32) {
    let fw = 360.min(fb_w.max(1));
    let fh = 26;
    let dst_x = (fb_w - fw) / 2;
    let dst_y = (fb_h - 36).max(0);
    let mut pixels = vec![0u32; (fw * fh) as usize];
    for ly in 0..fh {
        for lx in 0..fw {
            pixels[(ly as usize) * (fw as usize) + (lx as usize)] =
                backdrop_pixel_argb(dst_x + lx, dst_y + ly, fb_w, fb_h);
        }
    }
    let mut surf = Surface::from_slice(fw, fh, &mut pixels);
    with_default_font(|font| {
        let txt = "Starting SublimeOS Desktop Environment";
        let st = TextStyle::ui_sized(11, GuiColor::rgb(112, 142, 178));
        let (tw, _) = font.measure_text(txt, st);
        font.draw_text(&mut surf, ((fw - tw) / 2).max(0), 4, txt, st);
    });
    blit_surface_region_to_lfb(&surf, 0, fh, dst_x, dst_y);
}

/// Start graphical boot screen when a UEFI GOP / Multiboot linear framebuffer is available.
/// Returns `false` in headless `-display none` or VGA text mode fallback.
pub fn start() -> bool {
    let (has_lfb, w, h) = {
        let vbe = VBE.lock();
        (!vbe.framebuffer.is_null() && !vbe.text_mode_fallback && vbe.width > 0 && vbe.height > 0, vbe.width, vbe.height)
    };
    if !has_lfb {
        return false;
    }

    crate::drivers::gop_console::set_boot_splash_active(true);
    SPLASH_ACTIVE.store(true, Ordering::Release);
    {
        let mut st = SPLASH_STATE.lock();
        st.progress_pct = 12;
        st.set_label("Initializing Kernel & Framebuffer...");
        st.fb_w = w;
        st.fb_h = h;
    }

    paint_full_backdrop();
    render_footer_to_lfb(w as i32, h as i32);

    let mut card_buf = vec![0u32; (CARD_W * CARD_H) as usize];
    let mut surf = Surface::from_slice(CARD_W, CARD_H, &mut card_buf);
    render_card_to_surface(
        &mut surf,
        12,
        "Initializing Kernel & Framebuffer...",
        w as i32,
        h as i32,
    );
    let card_x = (w as i32 - CARD_W) / 2;
    let card_y = (h as i32 - CARD_H) / 2 - 12;
    blit_surface_region_to_lfb(&surf, 0, CARD_H, card_x, card_y);
    true
}

/// Update milestone progress (`0..=100`) and status label on the active graphical boot screen.
pub fn set_stage(pct: u8, label: &str) {
    let clamped = pct.min(100);
    let (active, fb_w, fb_h) = {
        let mut st = SPLASH_STATE.lock();
        st.progress_pct = clamped;
        st.set_label(label);
        (SPLASH_ACTIVE.load(Ordering::Acquire), st.fb_w as i32, st.fb_h as i32)
    };
    if !active || crate::drivers::gop_console::is_gui_active() || fb_w <= 0 || fb_h <= 0 {
        return;
    }

    let mut card_buf = vec![0u32; (CARD_W * CARD_H) as usize];
    let mut surf = Surface::from_slice(CARD_W, CARD_H, &mut card_buf);
    render_card_to_surface(&mut surf, clamped, label, fb_w, fb_h);
    let card_x = (fb_w - CARD_W) / 2;
    let card_y = (fb_h - CARD_H) / 2 - 12;
    blit_surface_region_to_lfb(
        &surf,
        PROGRESS_BAND_Y,
        PROGRESS_BAND_H,
        card_x,
        card_y + PROGRESS_BAND_Y,
    );
}

/// Exit splash mode (called on compositor takeover, kernel panic, or explicit mode switch).
pub fn stop() {
    SPLASH_ACTIVE.store(false, Ordering::Release);
    crate::drivers::gop_console::set_boot_splash_active(false);
}
