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

        // Big stylized title
        draw_string_scaled(
            canvas,
            center_x,
            draw_y,
            "✦ CosmosKernel",
            Color::ACCENT_PURPLE,
            2,
        );
        draw_y += 36;

        draw_string(
            canvas,
            center_x,
            draw_y,
            "Version 0.1.0-monolithic (x86_64)",
            Color::ACCENT_CYAN,
        );
        draw_y += FONT_HEIGHT + 4;

        draw_string(
            canvas,
            center_x,
            draw_y,
            "Pure Rust Monolithic Kernel - Clean, Neat & Mature",
            Color::TEXT_PRIMARY,
        );
        draw_y += FONT_HEIGHT + 10;

        // Divider
        canvas.fill_rect(center_x, draw_y, w - 32, 1, Color::WINDOW_BORDER);
        draw_y += 10;

        let specs = [
            ("Architecture: ", "x86_64-unknown-none (64-Bit AMD64)", Color::TEXT_MUTED),
            ("Toolchain:    ", "Rust 1.101.0-nightly (#![no_std])", Color::TEXT_MUTED),
            ("Memory Mgmt:  ", "Bitmap Frame Allocator + Coalescing Heap", Color::TEXT_MUTED),
            ("Virtual Mem:  ", "4-Level Paging (PML4, PDPT, PD, PT)", Color::TEXT_MUTED),
            ("Multitasking: ", "Preemptive Context Switcher (1000 Hz)", Color::TEXT_MUTED),
            ("Filesystem:   ", "VFS (In-Memory RamFS, DevFS, ProcFS)", Color::TEXT_MUTED),
            ("Graphics Engine: ", "Bochs VBE 1024x768x32bpp Compositor", Color::ACCENT_GREEN),
            ("Input System: ", "PS/2 Keyboard & Mouse (IRQ 1 & 12)", Color::ACCENT_GREEN),
        ];

        for (label, val, col) in specs.iter() {
            draw_string(canvas, center_x, draw_y, label, Color::ACCENT_YELLOW);
            draw_string(canvas, center_x + 128, draw_y, val, *col);
            draw_y += FONT_HEIGHT + 2;
        }

        draw_y += 8;
        draw_string(
            canvas,
            center_x,
            draw_y,
            "Crafted with precision for the Cosmos Operating System.",
            Color::TEXT_MUTED,
        );
    }
}
