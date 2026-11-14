use alloc::format;
use crate::drivers::platform::smbios::get_hardware_telemetry;
use crate::gui::canvas::Canvas;
use crate::gui::color::Color;
use crate::gui::font::{draw_string, draw_string_bold, draw_string_ellipsis};

pub struct AboutApp;

impl AboutApp {
    pub fn new() -> Self {
        Self
    }

    pub fn render(&self, canvas: &mut Canvas, x: i32, y: i32, w: i32, h: i32) {
        canvas.fill_rect(x, y, w, h, Color::XP_FACE);

        // 1. Top Classic Blue SublimeOS Banner
        let banner_h = 66;
        canvas.draw_gradient_h(x, y, w, banner_h, Color::XP_TITLE_TOP, Color::rgb(78, 152, 245));
        canvas.fill_rect(x, y + banner_h, w, 3, Color::XP_ORANGE_GLOW);

        // SublimeOS 4-diamond prism emblem in banner
        let ex = x + 18;
        let ey = y + 16;
        canvas.fill_rounded_rect(ex, ey, 34, 34, 6, Color::argb(0x33, 255, 255, 255));
        canvas.fill_rect(ex + 6, ey + 6, 10, 10, Color::rgb(125, 225, 255));
        canvas.fill_rect(ex + 18, ey + 6, 10, 10, Color::WHITE);
        canvas.fill_rect(ex + 6, ey + 18, 10, 10, Color::rgb(110, 245, 165));
        canvas.fill_rect(ex + 18, ey + 18, 10, 10, Color::rgb(255, 210, 95));

        draw_string_bold(canvas, x + 64, y + 15, "SublimeOS", Color::WHITE);
        draw_string_ellipsis(
            canvas,
            x + 64,
            y + 36,
            "Desktop Workstation Edition 1.0 (KiteKernel)",
            Color::rgb(218, 236, 255),
            w - 76,
        );

        // 2. White Information Card
        let card_x = x + 16;
        let card_y = y + banner_h + 14;
        let card_w = (w - 32).max(180);
        let card_h = (h - banner_h - 30).max(120);
        canvas.fill_rect(card_x, card_y, card_w, card_h, Color::WHITE);
        canvas.draw_rect(card_x, card_y, card_w, card_h, Color::XP_SHADOW);

        let hw = get_hardware_telemetry();
        let gfx = crate::drivers::vbe::VBE.lock();
        let gfx_desc = format!("UEFI GOP {}x{} @ {}bpp", gfx.width, gfx.height, gfx.bpp);
        drop(gfx);

        let rows: [(&str, &str, Color); 8] = [
            ("OS Version:  ", "SublimeOS 1.0.0 (Build 100.2026)", Color::XP_TEXT_BLACK),
            ("Architecture:", "x86_64 SMP + APIC + KiteKernel", Color::XP_TEXT_BLACK),
            ("Processor:   ", &hw.chip, Color::XP_TEXT_BLACK),
            ("Cores:       ", &hw.cores_threads, Color::XP_TEXT_BLACK),
            ("Memory:      ", &hw.memory_summary, Color::XP_TEXT_BLACK),
            ("Graphics:    ", gfx_desc.as_str(), Color::XP_TEXT_LINK),
            ("Hardware:    ", &hw.model_name, Color::XP_TEXT_GRAY),
            ("Firmware:    ", &hw.system_firmware, Color::XP_TEXT_GRAY),
        ];

        let tx = card_x + 12;
        let max_val_w = (card_w - 126).max(60);
        let mut cy = card_y + 12;
        for (label, val, col) in rows.iter() {
            if cy + 16 > card_y + card_h - 24 {
                break;
            }
            draw_string(canvas, tx, cy, label, Color::XP_TEXT_GRAY);
            draw_string_ellipsis(canvas, tx + 106, cy, val, *col, max_val_w);
            cy += 21;
        }

        draw_string_ellipsis(
            canvas,
            tx,
            card_y + card_h - 22,
            "Copyright (C) 2026 Nader Mahbub Khan. All rights reserved.",
            Color::XP_TEXT_GRAY,
            card_w - 24,
        );
    }
}
