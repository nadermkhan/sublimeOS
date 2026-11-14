use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use crate::gui::canvas::Canvas;
use crate::gui::color::Color;

pub const FONT_WIDTH: i32 = 8;
pub const FONT_HEIGHT: i32 = 16;

/// Global font style configuration (0 = Classic 8x16, 1 = Crisp Bold UI, 2 = Condensed Compact).
static UI_FONT_STYLE: AtomicU8 = AtomicU8::new(0);
static TERMINAL_BOLD: AtomicBool = AtomicBool::new(false);

pub fn get_ui_font_style() -> u8 {
    UI_FONT_STYLE.load(Ordering::Relaxed)
}

pub fn set_ui_font_style(style: u8) {
    UI_FONT_STYLE.store(style % 3, Ordering::Relaxed);
}

pub fn ui_font_style_name(style: u8) -> &'static str {
    match style % 3 {
        0 => "Sublime Mono 8x16 (Regular)",
        1 => "Sublime UI Sans-Bold (High Contrast)",
        _ => "Sublime ClearType Mono",
    }
}

pub fn get_terminal_bold() -> bool {
    TERMINAL_BOLD.load(Ordering::Relaxed)
}

pub fn set_terminal_bold(bold: bool) {
    TERMINAL_BOLD.store(bold, Ordering::Relaxed);
}

include!("font_data.rs");

/// Render a single character onto the canvas honoring clip bounds.
pub fn draw_char(canvas: &mut Canvas, x: i32, y: i32, ch: char, fg: Color) {
    let (cx0, cy0, cx1, cy1) = canvas.clip_bounds();
    if x + FONT_WIDTH <= cx0 || x >= cx1 || y + FONT_HEIGHT <= cy0 || y >= cy1 {
        return;
    }

    let glyph = get_glyph_data(ch);
    let bold_ui = UI_FONT_STYLE.load(Ordering::Relaxed) == 1;
    for row in 0..16 {
        let bits = glyph[row as usize];
        if bits == 0 {
            continue;
        }
        let py = y + row;
        if py < cy0 || py >= cy1 {
            continue;
        }

        let draw_bits = if bold_ui { bits | (bits >> 1) } else { bits };
        for col in 0..8 {
            if (draw_bits & (0x80 >> col)) != 0 {
                let px = x + col;
                if px >= cx0 && px < cx1 {
                    canvas.set_pixel_blend(px, py, fg);
                }
            }
        }
    }
}

/// Render a bold synthesized character (1px horizontal smear).
pub fn draw_char_bold(canvas: &mut Canvas, x: i32, y: i32, ch: char, fg: Color) {
    let (cx0, cy0, cx1, cy1) = canvas.clip_bounds();
    if x + FONT_WIDTH <= cx0 || x >= cx1 || y + FONT_HEIGHT <= cy0 || y >= cy1 {
        return;
    }
    let glyph = get_glyph_data(ch);
    for row in 0..16 {
        let bits = glyph[row as usize];
        if bits == 0 {
            continue;
        }
        let py = y + row;
        if py < cy0 || py >= cy1 {
            continue;
        }
        let bold_bits = bits | (bits >> 1);
        for col in 0..8 {
            if (bold_bits & (0x80 >> col)) != 0 {
                let px = x + col;
                if px >= cx0 && px < cx1 {
                    canvas.set_pixel_blend(px, py, fg);
                }
            }
        }
    }
}

/// Render a string onto the canvas.
pub fn draw_string(canvas: &mut Canvas, mut x: i32, y: i32, text: &str, fg: Color) {
    for ch in text.chars() {
        if ch == '\n' {
            continue;
        }
        draw_char(canvas, x, y, ch, fg);
        x += FONT_WIDTH;
    }
}

/// Render a bold string onto the canvas.
pub fn draw_string_bold(canvas: &mut Canvas, mut x: i32, y: i32, text: &str, fg: Color) {
    for ch in text.chars() {
        if ch == '\n' {
            continue;
        }
        draw_char_bold(canvas, x, y, ch, fg);
        x += FONT_WIDTH;
    }
}

/// Render a string truncated cleanly with "..." if it exceeds `max_w` pixels (UTF-8 safe).
pub fn draw_string_ellipsis(canvas: &mut Canvas, mut x: i32, y: i32, text: &str, fg: Color, max_w: i32) {
    if max_w < FONT_WIDTH {
        return;
    }
    let max_chars = (max_w / FONT_WIDTH) as usize;
    let total_chars = text.chars().filter(|&c| c != '\n').count();
    if total_chars <= max_chars {
        draw_string(canvas, x, y, text, fg);
        return;
    }
    if max_chars <= 3 {
        for _ in 0..max_chars {
            draw_char(canvas, x, y, '.', fg);
            x += FONT_WIDTH;
        }
        return;
    }
    for ch in text.chars().filter(|&c| c != '\n').take(max_chars - 3) {
        draw_char(canvas, x, y, ch, fg);
        x += FONT_WIDTH;
    }
    for _ in 0..3 {
        draw_char(canvas, x, y, '.', fg);
        x += FONT_WIDTH;
    }
}

/// Render a bold string truncated cleanly with "..." if it exceeds `max_w` pixels (UTF-8 safe).
pub fn draw_string_bold_ellipsis(canvas: &mut Canvas, mut x: i32, y: i32, text: &str, fg: Color, max_w: i32) {
    if max_w < FONT_WIDTH {
        return;
    }
    let max_chars = (max_w / FONT_WIDTH) as usize;
    let total_chars = text.chars().filter(|&c| c != '\n').count();
    if total_chars <= max_chars {
        draw_string_bold(canvas, x, y, text, fg);
        return;
    }
    if max_chars <= 3 {
        for _ in 0..max_chars {
            draw_char_bold(canvas, x, y, '.', fg);
            x += FONT_WIDTH;
        }
        return;
    }
    for ch in text.chars().filter(|&c| c != '\n').take(max_chars - 3) {
        draw_char_bold(canvas, x, y, ch, fg);
        x += FONT_WIDTH;
    }
    for _ in 0..3 {
        draw_char_bold(canvas, x, y, '.', fg);
        x += FONT_WIDTH;
    }
}

/// Render word-wrapped text within `max_w` and up to `max_lines`. Returns next Y coordinate.
pub fn draw_string_wrapped(
    canvas: &mut Canvas,
    x: i32,
    mut y: i32,
    max_w: i32,
    line_height: i32,
    max_lines: usize,
    text: &str,
    fg: Color,
) -> i32 {
    let cols = (max_w / FONT_WIDTH).max(4) as usize;
    let mut drawn = 0usize;
    let mut cur = alloc::string::String::new();

    for word in text.split_whitespace() {
        let word_len = word.chars().count();
        if cur.is_empty() {
            if word_len <= cols {
                cur.push_str(word);
            } else {
                for chunk_start in (0..word_len).step_by(cols) {
                    if drawn >= max_lines {
                        return y;
                    }
                    let slice: alloc::string::String = word.chars().skip(chunk_start).take(cols).collect();
                    draw_string(canvas, x, y, &slice, fg);
                    y += line_height;
                    drawn += 1;
                }
            }
        } else if cur.chars().count() + 1 + word_len <= cols {
            cur.push(' ');
            cur.push_str(word);
        } else {
            if drawn >= max_lines {
                return y;
            }
            draw_string(canvas, x, y, &cur, fg);
            y += line_height;
            drawn += 1;
            cur.clear();
            cur.push_str(word);
        }
    }

    if !cur.is_empty() && drawn < max_lines {
        draw_string_ellipsis(canvas, x, y, &cur, fg, max_w);
        y += line_height;
    }
    y
}

/// Render a string with a 1px drop shadow.
pub fn draw_string_shadow(canvas: &mut Canvas, x: i32, y: i32, text: &str, fg: Color, shadow: Color) {
    draw_string(canvas, x + 1, y + 1, text, shadow);
    draw_string(canvas, x, y, text, fg);
}

/// Render a string scaled by an integer factor (e.g. 2x for large banners).
pub fn draw_string_scaled(canvas: &mut Canvas, mut x: i32, y: i32, text: &str, fg: Color, scale: i32) {
    if scale <= 1 {
        draw_string(canvas, x, y, text, fg);
        return;
    }

    for ch in text.chars() {
        let glyph = get_glyph_data(ch);
        for row in 0..16 {
            let bits = glyph[row as usize];
            for col in 0..8 {
                if (bits & (0x80 >> col)) != 0 {
                    let px = x + col * scale;
                    let py = y + row * scale;
                    canvas.fill_rect(px, py, scale, scale, fg);
                }
            }
        }
        x += FONT_WIDTH * scale;
    }
}

/// Calculate the pixel width of a rendered string.
#[inline(always)]
pub fn measure_string(text: &str) -> i32 {
    text.chars().count() as i32 * FONT_WIDTH
}
