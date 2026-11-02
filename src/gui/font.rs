use crate::gui::canvas::Canvas;
use crate::gui::color::Color;

pub const FONT_WIDTH: i32 = 8;
pub const FONT_HEIGHT: i32 = 16;

include!("font_data.rs");

/// Render a single ASCII character onto the canvas.
pub fn draw_char(canvas: &mut Canvas, x: i32, y: i32, ch: char, fg: Color) {
    let glyph = get_glyph_data(ch);
    for row in 0..16 {
        let bits = glyph[row as usize];
        let py = y + row;
        if py < 0 || py >= canvas.height as i32 {
            continue;
        }

        for col in 0..8 {
            if (bits & (0x80 >> col)) != 0 {
                let px = x + col;
                if px >= 0 && px < canvas.width as i32 {
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
