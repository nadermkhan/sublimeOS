use crate::gui::canvas::Canvas;
use crate::gui::color::Color;
use crate::gui::font::{draw_string, draw_string_scaled, FONT_HEIGHT};

pub struct AboutApp;

impl AboutApp {
    pub fn new() -> Self {
        Self
    }

    pub fn render(&self, canvas: &mut Canvas, x: i32, y: i32, w: i32, h: i32) {
        canvas.fill_rect(x, y, w, h, Color::BG_INPUT);

        let center_x = x + 16;
        let mut draw_y = y + 14;

        // Big stylized CosmicOS title
        draw_string_scaled(
            canvas,
            center_x,
            draw_y,
            "✦ CosmicOS",
            Color::WHITE,
            2,
        );
        draw_y += 36;

        draw_string(
            canvas,
            center_x,
            draw_y,
            "Version 1.0 (Build 1A01)",
            Color::ACCENT_CYAN,
        );
        draw_y += FONT_HEIGHT + 4;

        draw_string(
            canvas,
            center_x,
            draw_y,
            "CosmicStation Pro (x86_64)",
            Color::TEXT_PRIMARY,
        );
        draw_y += FONT_HEIGHT + 10;

        // Divider
        canvas.fill_rect(center_x, draw_y, w - 32, 1, Color::WINDOW_BORDER);
        draw_y += 10;

        let specs = [
            ("Chip:         ", "Sublime Architecture (AMD64 x86_64)", Color::TEXT_MUTED),
            ("Memory:       ", "128 MB Unified Architecture", Color::TEXT_MUTED),
            ("Startup Disk: ", "Cosmic HD (CosmicFS)", Color::TEXT_MUTED),
            ("Graphics:     ", "Linear Framebuffer (1024x768 32bpp)", Color::ACCENT_GREEN),
            ("Kernel:       ", "Sublime Kernel 1.0.0 (Monolithic)", Color::TEXT_MUTED),
            ("Toolchain:    ", "Rust nightly (#![no_std])", Color::TEXT_MUTED),
            ("Serial Number:", "CS1000SUBX64", Color::TEXT_MUTED),
            ("Firmware:     ", "1.0.0 (UEFI / BIOS Hybrid)", Color::ACCENT_YELLOW),
        ];

        for (label, val, col) in specs.iter() {
            draw_string(canvas, center_x, draw_y, label, Color::TEXT_PRIMARY);
            draw_string(canvas, center_x + 128, draw_y, val, *col);
            draw_y += FONT_HEIGHT + 2;
        }

        draw_y += 8;
        draw_string(
            canvas,
            center_x,
            draw_y,
            "© 2026 CosmicOS Project. All rights reserved.",
            Color::TEXT_MUTED,
        );
    }
}
