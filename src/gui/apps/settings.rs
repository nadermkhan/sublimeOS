use alloc::format;
use alloc::string::String;
use crate::gui::canvas::Canvas;
use crate::gui::color::Color;
use crate::gui::font::{draw_char, draw_string, FONT_HEIGHT};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    General,
    Displays,
    Sound,
    Network,
    Storage,
}

pub struct SettingsApp {
    pub current_tab: SettingsTab,
    pub volume_level: u8,
    pub status_feedback: String,
    pub hw: crate::drivers::platform::smbios::SystemHardwareTelemetry,
}

impl SettingsApp {
    pub fn new() -> Self {
        Self {
            current_tab: SettingsTab::General,
            volume_level: 80,
            status_feedback: String::from("System Settings: Ready"),
            hw: crate::drivers::platform::smbios::get_hardware_telemetry(),
        }
    }

    pub fn handle_click(&mut self, px: i32, py: i32, ix: i32, iy: i32, _iw: i32, _ih: i32) {
        let sidebar_w = 140;

        // 1. Sidebar tab selection
        if px >= ix && px <= ix + sidebar_w {
            let mut ty = iy + 12;
            let tabs = [
                SettingsTab::General,
                SettingsTab::Displays,
                SettingsTab::Sound,
                SettingsTab::Network,
                SettingsTab::Storage,
            ];

            for tab in tabs.into_iter() {
                if py >= ty && py <= ty + 26 {
                    self.current_tab = tab;
                    self.status_feedback = match tab {
                        SettingsTab::General => String::from("Viewing General System Overview"),
                        SettingsTab::Displays => String::from("Display Settings (1024x768 @ 60Hz)"),
                        SettingsTab::Sound => String::from("Audio Subsystem (Intel HDA / Stereo 48kHz)"),
                        SettingsTab::Network => String::from("Network Interfaces (en0 / Gigabit Ethernet)"),
                        SettingsTab::Storage => String::from("Storage Partitions & Block Cache"),
                    };
                    return;
                }
                ty += 32;
            }
        }

        // 2. Right pane interactive controls
        let content_x = ix + sidebar_w + 16;
        if px >= content_x {
            match self.current_tab {
                SettingsTab::Sound => {
                    // Volume slider bar at iy + 92 (width 180)
                    if py >= iy + 88 && py <= iy + 104 && px >= content_x + 90 && px <= content_x + 270 {
                        let ratio = (px - (content_x + 90)) as f32 / 180.0;
                        self.volume_level = ((ratio * 100.0) as u8).clamp(0, 100);
                        self.status_feedback = format!("Master Volume set to {}%", self.volume_level);
                    }
                    // Test Chime button at iy + 128
                    if py >= iy + 124 && py <= iy + 150 && px >= content_x + 90 && px <= content_x + 200 {
                        crate::drivers::audio::play_sublime_chime();
                        self.status_feedback = String::from("Playing macOS System Chime via Intel HDA");
                    }
                }
                _ => {}
            }
        }
    }

    pub fn render(&self, canvas: &mut Canvas, x: i32, y: i32, w: i32, h: i32) {
        // macOS Settings background
        canvas.fill_rect(x, y, w, h, Color::argb(255, 30, 32, 42));

        let sidebar_w = 140;
        let content_x = x + sidebar_w + 16;

        // 1. Sidebar (Translucent dark panel)
        canvas.fill_rect(x, y, sidebar_w, h, Color::argb(255, 23, 24, 32));
        canvas.fill_rect(x + sidebar_w, y, 1, h, Color::argb(60, 255, 255, 255));

        let mut ty = y + 12;
        let tabs = [
            ("General", SettingsTab::General, 'G'),
            ("Displays", SettingsTab::Displays, 'D'),
            ("Sound", SettingsTab::Sound, 'S'),
            ("Network", SettingsTab::Network, 'N'),
            ("Storage", SettingsTab::Storage, 'H'),
        ];

        for (label, tab, icon) in tabs.iter() {
            let is_selected = self.current_tab == *tab;
            if is_selected {
                canvas.fill_rounded_rect(x + 6, ty, sidebar_w - 12, 26, 6, Color::ACCENT_BLUE);
            }
            draw_char(
                canvas,
                x + 12,
                ty + 5,
                *icon,
                if is_selected { Color::WHITE } else { Color::ACCENT_CYAN },
            );
            draw_string(
                canvas,
                x + 28,
                ty + 5,
                label,
                if is_selected { Color::WHITE } else { Color::TEXT_PRIMARY },
            );
            ty += 32;
        }

        // 2. Right Content Panel
        let hw = &self.hw;
        let mut cy = y + 14;

        match self.current_tab {
            SettingsTab::General => {
                draw_string(canvas, content_x, cy, "General System Information", Color::WHITE);
                cy += 24;
                canvas.fill_rect(content_x, cy, w - sidebar_w - 32, 1, Color::WINDOW_BORDER);
                cy += 12;

                let info: [(&str, &str); 7] = [
                    ("Computer Name:", "Sublime's Mac"),
                    ("macOS Version:", "SublimeOS 1.0 (Sonoma Style)"),
                    ("Kernel Core:  ", "KiteKernel 1.0.0 (x86_64 SMP)"),
                    ("Processor:    ", &hw.chip),
                    ("Cores/Threads:", &hw.cores_threads),
                    ("Model Ident:  ", &hw.model_name),
                    ("Serial Number:", &hw.serial_number),
                ];

                for (label, val) in info.iter() {
                    draw_string(canvas, content_x, cy, label, Color::TEXT_MUTED);
                    draw_string(canvas, content_x + 120, cy, val, Color::TEXT_PRIMARY);
                    cy += FONT_HEIGHT + 6;
                }
            }
            SettingsTab::Displays => {
                draw_string(canvas, content_x, cy, "Display & Graphics Configuration", Color::WHITE);
                cy += 24;
                canvas.fill_rect(content_x, cy, w - sidebar_w - 32, 1, Color::WINDOW_BORDER);
                cy += 12;

                let info: [(&str, &str); 5] = [
                    ("Primary Display:", "Built-in Liquid Retina (1024x768)"),
                    ("Refresh Rate:   ", "60 Hz (Hardware Synchronized)"),
                    ("Color Depth:    ", "32-bit True Color (ARGB8888)"),
                    ("Compositor:     ", "Quartz Double-Buffered GOP Engine"),
                    ("Appearance:     ", "macOS Dark Mode (Frosted Glass Active)"),
                ];

                for (label, val) in info.iter() {
                    draw_string(canvas, content_x, cy, label, Color::TEXT_MUTED);
                    draw_string(canvas, content_x + 130, cy, val, Color::TEXT_PRIMARY);
                    cy += FONT_HEIGHT + 6;
                }
            }
            SettingsTab::Sound => {
                draw_string(canvas, content_x, cy, "Sound Output & Alert Effects", Color::WHITE);
                cy += 24;
                canvas.fill_rect(content_x, cy, w - sidebar_w - 32, 1, Color::WINDOW_BORDER);
                cy += 14;

                draw_string(canvas, content_x, cy, "Output Device:", Color::TEXT_MUTED);
                draw_string(canvas, content_x + 110, cy, "Intel High Definition Audio (ALC888)", Color::ACCENT_CYAN);
                cy += 24;

                draw_string(canvas, content_x, cy, "Master Volume:", Color::TEXT_MUTED);
                // Volume slider background
                let slider_x = content_x + 110;
                let slider_w = 180;
                canvas.fill_rounded_rect(slider_x, cy + 3, slider_w, 10, 5, Color::argb(80, 255, 255, 255));
                let filled_w = ((self.volume_level as i32 * slider_w) / 100).clamp(4, slider_w);
                canvas.fill_rounded_rect(slider_x, cy + 3, filled_w, 10, 5, Color::ACCENT_BLUE);
                canvas.fill_circle(slider_x + filled_w, cy + 8, 7, Color::WHITE);

                let vol_text = format!("{}%", self.volume_level);
                draw_string(canvas, slider_x + slider_w + 14, cy, &vol_text, Color::WHITE);
                cy += 32;

                // Test Chime Button
                canvas.fill_rounded_rect(slider_x, cy, 110, 26, 6, Color::argb(60, 255, 255, 255));
                canvas.draw_rounded_rect(slider_x, cy, 110, 26, 6, Color::WINDOW_BORDER);
                draw_string(canvas, slider_x + 14, cy + 5, "Test Chime", Color::WHITE);
            }
            SettingsTab::Network => {
                draw_string(canvas, content_x, cy, "Network Interfaces & Internet", Color::WHITE);
                cy += 24;
                canvas.fill_rect(content_x, cy, w - sidebar_w - 32, 1, Color::WINDOW_BORDER);
                cy += 12;

                let info: [(&str, &str); 6] = [
                    ("Interface:    ", "en0 (Intel e1000 PCI Gigabit Ethernet)"),
                    ("Status:       ", "Connected (1000BASE-T Full Duplex)"),
                    ("IPv4 Address: ", "192.168.1.175"),
                    ("Subnet Mask:  ", "255.255.255.0"),
                    ("Router IP:    ", "192.168.1.1"),
                    ("Hardware MAC: ", "52:54:00:12:34:56"),
                ];

                for (label, val) in info.iter() {
                    draw_string(canvas, content_x, cy, label, Color::TEXT_MUTED);
                    draw_string(canvas, content_x + 120, cy, val, Color::TEXT_PRIMARY);
                    cy += FONT_HEIGHT + 6;
                }
            }
            SettingsTab::Storage => {
                draw_string(canvas, content_x, cy, "Storage & File System Hierarchy", Color::WHITE);
                cy += 24;
                canvas.fill_rect(content_x, cy, w - sidebar_w - 32, 1, Color::WINDOW_BORDER);
                cy += 12;

                let (used_mb, total_mb) = {
                    let pmm = crate::mm::pmm::PMM.lock();
                    let used = (pmm.get_used_frames() * 4) / 1024;
                    let total = (pmm.get_total_frames() * 4) / 1024;
                    (used, total)
                };

                let info: [(&str, String); 5] = [
                    ("Sublime HD:   ", format!("Root Partition '/' ({} MB used / {} MB total)", used_mb, total_mb)),
                    ("Buffer Cache: ", String::from("1024 Sectors (LRU Write-Through)")),
                    ("Mount Points: ", String::from("/, /Volumes, /System, /Library, /Applications")),
                    ("VFS Drivers:  ", String::from("RamFS, FAT32, exFAT, ext4, NTFS")),
                    ("Cache Status: ", String::from("Clean (Synchronized)")),
                ];

                for (label, val) in info.iter() {
                    draw_string(canvas, content_x, cy, label, Color::TEXT_MUTED);
                    draw_string(canvas, content_x + 120, cy, val, Color::TEXT_PRIMARY);
                    cy += FONT_HEIGHT + 6;
                }
            }
        }

        // Status bar at bottom
        let status_y = y + h - 22;
        canvas.fill_rect(x, status_y, w, 22, Color::argb(255, 20, 21, 28));
        canvas.fill_rect(x, status_y, w, 1, Color::WINDOW_BORDER);
        draw_string(canvas, x + 10, status_y + 3, &self.status_feedback, Color::TEXT_MUTED);
    }
}
