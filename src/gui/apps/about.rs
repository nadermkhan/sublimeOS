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

        // Big stylized SublimeOS title
        draw_string_scaled(
            canvas,
            center_x,
            draw_y,
            "✦ SublimeOS",
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

        let hw = crate::drivers::platform::smbios::get_hardware_telemetry();

        draw_string(
            canvas,
            center_x,
            draw_y,
            &hw.model_name,
            Color::TEXT_PRIMARY,
        );
        draw_y += FONT_HEIGHT + 10;

        // Divider
        canvas.fill_rect(center_x, draw_y, w - 32, 1, Color::WINDOW_BORDER);
        draw_y += 10;

        let (fb_w, fb_h, fb_bpp, fb_text) = {
            let vbe = crate::drivers::vbe::VBE.lock();
            (vbe.width.max(640), vbe.height.max(480), vbe.bpp.max(32), vbe.text_mode_fallback)
        };
        let gfx_desc = if fb_text {
            alloc::format!("VGA Text Mode ({}x{}x{} RAM fb)", fb_w, fb_h, fb_bpp)
        } else {
            alloc::format!("UEFI GOP / LFB ({}x{} {}bpp WC)", fb_w, fb_h, fb_bpp)
        };

        let specs: [(&str, &str, Color); 8] = [
            ("Chip:         ", &hw.chip, Color::TEXT_MUTED),
            ("Cores:        ", &hw.cores_threads, Color::TEXT_MUTED),
            ("Memory:       ", &hw.memory_summary, Color::TEXT_MUTED),
            ("Graphics:     ", gfx_desc.as_str(), Color::ACCENT_GREEN),
            ("Kernel:       ", "KiteKernel 1.0.0 (x86_64)", Color::TEXT_MUTED),
            ("Serial Number:", &hw.serial_number, Color::TEXT_MUTED),
            ("Firmware:     ", &hw.system_firmware, Color::ACCENT_YELLOW),
            ("OS Loader:    ", &hw.os_loader, Color::TEXT_MUTED),
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
            "© 2026 Nader Mahbub Khan. All rights reserved.",
            Color::TEXT_MUTED,
        );
    }
}
