use crate::gui::canvas::Canvas;
use crate::gui::color::Color;

pub const CURSOR_WIDTH: i32 = 12;
pub const CURSOR_HEIGHT: i32 = 19;

// 0: transparent, 1: border (black), 2: fill (white), 3: shadow (translucent black)
static CURSOR_SPRITE: [[u8; CURSOR_WIDTH as usize]; CURSOR_HEIGHT as usize] = [
    [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [1, 2, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0],
    [1, 2, 2, 1, 0, 0, 0, 0, 0, 0, 0, 0],
    [1, 2, 2, 2, 1, 0, 0, 0, 0, 0, 0, 0],
    [1, 2, 2, 2, 2, 1, 0, 0, 0, 0, 0, 0],
    [1, 2, 2, 2, 2, 2, 1, 0, 0, 0, 0, 0],
    [1, 2, 2, 2, 2, 2, 2, 1, 0, 0, 0, 0],
    [1, 2, 2, 2, 2, 2, 2, 2, 1, 0, 0, 0],
    [1, 2, 2, 2, 2, 2, 2, 2, 2, 1, 0, 0],
    [1, 2, 2, 2, 2, 2, 1, 1, 1, 1, 1, 0],
    [1, 2, 2, 1, 2, 2, 1, 0, 0, 0, 0, 0],
    [1, 2, 1, 0, 1, 2, 2, 1, 0, 0, 0, 0],
    [1, 1, 0, 0, 1, 2, 2, 1, 0, 0, 0, 0],
    [1, 0, 0, 0, 0, 1, 2, 2, 1, 0, 0, 0],
    [0, 0, 0, 0, 0, 1, 2, 2, 1, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 1, 2, 2, 1, 0, 0],
    [0, 0, 0, 0, 0, 0, 1, 1, 1, 0, 0, 0],
    [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
];

/// Render hardware mouse cursor onto canvas with soft shadow.
pub fn draw_cursor(canvas: &mut Canvas, x: i32, y: i32) {
    let shadow_color = Color::argb(80, 0, 0, 0);
    let border_color = Color::BLACK;
    let fill_color = Color::WHITE;

    // Draw shadow pass (offset by +1, +2)
    for row in 0..CURSOR_HEIGHT {
        for col in 0..CURSOR_WIDTH {
            let pixel = CURSOR_SPRITE[row as usize][col as usize];
            if pixel != 0 {
                canvas.set_pixel_blend(x + col + 1, y + row + 2, shadow_color);
            }
        }
    }

    // Draw cursor body
    for row in 0..CURSOR_HEIGHT {
        for col in 0..CURSOR_WIDTH {
            let pixel = CURSOR_SPRITE[row as usize][col as usize];
            match pixel {
                1 => canvas.set_pixel(x + col, y + row, border_color),
                2 => canvas.set_pixel(x + col, y + row, fill_color),
                _ => {}
            }
        }
    }
}
