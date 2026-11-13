use crate::gui::canvas::Canvas;
use crate::gui::color::Color;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CursorType {
    Arrow,
    IBeam,
    ResizeHorizontal,
    ResizeVertical,
    ResizeDiagonal,
    Hand,
}

pub const CURSOR_WIDTH: i32 = 12;
pub const CURSOR_HEIGHT: i32 = 19;

// 0: transparent, 1: border (black), 2: fill (white)
static ARROW_SPRITE: [[u8; CURSOR_WIDTH as usize]; CURSOR_HEIGHT as usize] = [
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
pub fn draw_cursor(canvas: &mut Canvas, x: i32, y: i32, cursor_type: CursorType) {
    match cursor_type {
        CursorType::Arrow => {
            let shadow_color = Color::argb(80, 0, 0, 0);
            let border_color = Color::BLACK;
            let fill_color = Color::WHITE;

            // Shadow pass
            for row in 0..CURSOR_HEIGHT {
                for col in 0..CURSOR_WIDTH {
                    let pixel = ARROW_SPRITE[row as usize][col as usize];
                    if pixel != 0 {
                        canvas.set_pixel_blend(x + col + 1, y + row + 2, shadow_color);
                    }
                }
            }

            // Body
            for row in 0..CURSOR_HEIGHT {
                for col in 0..CURSOR_WIDTH {
                    let pixel = ARROW_SPRITE[row as usize][col as usize];
                    match pixel {
                        1 => canvas.set_pixel(x + col, y + row, border_color),
                        2 => canvas.set_pixel(x + col, y + row, fill_color),
                        _ => {}
                    }
                }
            }
        }
        CursorType::IBeam => {
            // Text cursor (I-beam)
            let col = Color::WHITE;
            let sh = Color::BLACK;
            // Shadow
            canvas.fill_rect(x - 3, y - 7, 7, 1, sh);
            canvas.fill_rect(x - 3, y + 7, 7, 1, sh);
            canvas.fill_rect(x, y - 6, 1, 13, sh);
            // Main beam
            canvas.fill_rect(x - 3, y - 8, 7, 1, col);
            canvas.fill_rect(x - 3, y + 6, 7, 1, col);
            canvas.fill_rect(x, y - 7, 1, 13, col);
        }
        CursorType::ResizeDiagonal => {
            // Double-headed diagonal arrow
            let col = Color::WHITE;
            let sh = Color::BLACK;
            for i in -5..=5 {
                canvas.set_pixel(x + i + 1, y + i + 1, sh);
                canvas.set_pixel(x + i, y + i, col);
            }
            // Arrowhead 1 (top-left)
            canvas.fill_rect(x - 5, y - 5, 4, 1, col);
            canvas.fill_rect(x - 5, y - 5, 1, 4, col);
            // Arrowhead 2 (bottom-right)
            canvas.fill_rect(x + 2, y + 5, 4, 1, col);
            canvas.fill_rect(x + 5, y + 2, 1, 4, col);
        }
        CursorType::ResizeHorizontal => {
            // Horizontal resize double-arrow
            let col = Color::WHITE;
            canvas.fill_rect(x - 6, y, 13, 2, col);
            canvas.set_pixel(x - 5, y - 2, col);
            canvas.set_pixel(x - 5, y + 3, col);
            canvas.set_pixel(x + 5, y - 2, col);
            canvas.set_pixel(x + 5, y + 3, col);
        }
        CursorType::ResizeVertical => {
            // Vertical resize double-arrow
            let col = Color::WHITE;
            canvas.fill_rect(x, y - 6, 2, 13, col);
            canvas.set_pixel(x - 2, y - 5, col);
            canvas.set_pixel(x + 3, y - 5, col);
            canvas.set_pixel(x - 2, y + 5, col);
            canvas.set_pixel(x + 3, y + 5, col);
        }
        CursorType::Hand => {
            // Pointer hand
            draw_cursor(canvas, x, y, CursorType::Arrow);
        }
    }
}
