use alloc::format;
use crate::drivers::platform::smbios::get_hardware_telemetry;
use crate::gui::canvas::Canvas;
use crate::gui::color::Color;
use crate::gui::font::{draw_ui_string, draw_ui_string_ellipsis, draw_ui_string_sized};

pub struct AboutApp;

impl AboutApp {
    pub fn new() -> Self {
        Self
    }

    pub fn render(&self, canvas: &mut Canvas, x: i32, y: i32, w: i32, h: i32) {
        let theme = libgui::gfx::current_theme();
        canvas.fill_rect(x, y, w, h, Color::from_libgui(theme.face));

        // 1. Flat Metro Header Banner (Material You / Win8.1 contrast-aware foreground)
        let banner_h = 68;
        canvas.fill_rect(x, y, w, banner_h, Color::from_libgui(theme.title_top));
        let banner_fg = Color::from_libgui(libgui::gfx::contrast_text_on(theme.title_top));
        let banner_sub = Color::lerp(banner_fg, Color::from_libgui(theme.title_top), 55);

        // SublimeOS Prismatic 'S' emblem in banner
        let ex = x + 18;
        let ey = y + 17;
        canvas.fill_rect(ex, ey, 34, 34, Color::argb(38, 255, 255, 255));
        crate::gui::desktop::draw_sublime_logo(
            canvas,
            ex + 4,
            ey + 4,
            26,
            banner_fg,
            Color::rgb(96, 205, 255),
        );

        draw_ui_string_sized(canvas, x + 64, y + 12, 20, true, "SublimeOS", banner_fg);
        draw_ui_string_ellipsis(
            canvas,
            x + 64,
            y + 39,
            "Modern Desktop Workstation 1.0 (KiteKernel)",
            banner_sub,
            w - 76,
        );

        // 2. Clean Flat White Information Card
        let card_x = x + 16;
        let card_y = y + banner_h + 14;
        let card_w = (w - 32).max(180);
        let card_h = (h - banner_h - 30).max(120);
        canvas.fill_rect(card_x, card_y, card_w, card_h, Color::from_libgui(theme.face_light));
        canvas.draw_rect(card_x, card_y, card_w, card_h, Color::from_libgui(theme.border_shadow));

        let hw = get_hardware_telemetry();
        let gfx = crate::drivers::vbe::VBE.lock();
        let gfx_desc = format!("UEFI GOP {}x{} @ {}bpp", gfx.width, gfx.height, gfx.bpp);
        drop(gfx);

        let fg = Color::from_libgui(theme.text_primary);
        let sub = Color::from_libgui(theme.text_secondary);
        let lnk = Color::from_libgui(theme.text_link);

        let rows: [(&str, &str, Color); 8] = [
            ("OS Version:  ", "SublimeOS 1.0.0 (Build 100.2026)", fg),
            ("Architecture:", "x86_64 SMP + APIC + KiteKernel", fg),
            ("Processor:   ", &hw.chip, fg),
            ("Cores:       ", &hw.cores_threads, fg),
            ("Memory:      ", &hw.memory_summary, fg),
            ("Graphics:    ", gfx_desc.as_str(), lnk),
            ("Hardware:    ", &hw.model_name, sub),
            ("Firmware:    ", &hw.system_firmware, sub),
        ];

        let tx = card_x + 12;
        let max_val_w = (card_w - 126).max(60);
        let mut cy = card_y + 12;
        for (label, val, col) in rows.iter() {
            if cy + 16 > card_y + card_h - 24 {
                break;
            }
            draw_ui_string(canvas, tx, cy, label, sub);
            draw_ui_string_ellipsis(canvas, tx + 106, cy, val, *col, max_val_w);
            cy += 21;
        }

        draw_ui_string_ellipsis(
            canvas,
            tx,
            card_y + card_h - 22,
            "Copyright (C) 2026 Nader Mahbub Khan. All rights reserved.",
            sub,
            card_w - 24,
        );
    }
}
