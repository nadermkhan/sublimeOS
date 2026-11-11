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

        // Big stylized macOS title
        draw_string_scaled(
            canvas,
            center_x,
            draw_y,
            "✦ macOS Sequoia",
            Color::WHITE,
            2,
        );
        draw_y += 36;

        draw_string(
            canvas,
            center_x,
            draw_y,
            "Version 15.1 (Build 24B83)",
            Color::ACCENT_CYAN,
        );
        draw_y += FONT_HEIGHT + 4;

        draw_string(
            canvas,
            center_x,
            draw_y,
            "MacBook Pro (16-inch, 2024)",
            Color::TEXT_PRIMARY,
        );
        draw_y += FONT_HEIGHT + 10;

        // Divider
        canvas.fill_rect(center_x, draw_y, w - 32, 1, Color::WINDOW_BORDER);
        draw_y += 10;

        let specs = [
            ("Chip:         ", "Apple M-Series (AMD64 x86_64)", Color::TEXT_MUTED),
            ("Memory:       ", "128 MB Unified Memory", Color::TEXT_MUTED),
            ("Startup Disk: ", "Macintosh HD (APFS Container)", Color::TEXT_MUTED),
            ("Graphics:     ", "Linear Framebuffer (1024x768 32bpp)", Color::ACCENT_GREEN),
            ("Kernel:       ", "Darwin 24.1.0 (xnu-monolithic)", Color::TEXT_MUTED),
            ("Toolchain:    ", "Rust 1.101.0-nightly (#![no_std])", Color::TEXT_MUTED),
            ("Serial Number:", "C02C1234MD6R", Color::TEXT_MUTED),
            ("Firmware:     ", "11215.1.31 (UEFI / BIOS Hybrid)", Color::ACCENT_YELLOW),
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
            "™ and © 1983-2026 Apple Inc. All rights reserved.",
            Color::TEXT_MUTED,
        );
    }
}
