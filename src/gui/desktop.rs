use crate::gui::canvas::Canvas;
use crate::gui::color::Color;
use crate::gui::font::{draw_char, draw_string, draw_string_scaled, draw_string_shadow, measure_string};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DesktopAppId {
    Terminal,
    Sysmon,
    FileManager,
    About,
}

pub struct DesktopIcon {
    pub id: DesktopAppId,
    pub title: &'static str,
    pub x: i32,
    pub y: i32,
    pub icon_char: char,
    pub icon_color: Color,
}

pub struct Desktop {
    pub icons: [DesktopIcon; 4],
    pub selected_icon: Option<DesktopAppId>,
    stars: [(i32, i32, Color); 96],
}

impl Desktop {
    pub fn new() -> Self {
        // Generate pseudo-random star field across 1024x768
        let mut stars = [(0, 0, Color::WHITE); 96];
        let mut seed: u32 = 0xDEADBEEF;
        for i in 0..96 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let sx = (seed % 1020) as i32 + 2;
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let sy = (seed % 720) as i32 + 2;
            let brightness = (seed >> 8) % 3;
            let col = match brightness {
                0 => Color::argb(140, 255, 255, 255),
                1 => Color::argb(220, 180, 220, 255),
                _ => Color::argb(255, 255, 240, 200),
            };
            stars[i] = (sx, sy, col);
        }

        let icons = [
            DesktopIcon {
                id: DesktopAppId::Terminal,
                title: "Terminal",
                x: 24,
                y: 32,
                icon_char: '>',
                icon_color: Color::ACCENT_GREEN,
            },
            DesktopIcon {
                id: DesktopAppId::Sysmon,
                title: "SysMon",
                x: 24,
                y: 124,
                icon_char: '#',
                icon_color: Color::ACCENT_CYAN,
            },
            DesktopIcon {
                id: DesktopAppId::FileManager,
                title: "Files",
                x: 24,
                y: 216,
                icon_char: '@',
                icon_color: Color::ACCENT_YELLOW,
            },
            DesktopIcon {
                id: DesktopAppId::About,
                title: "About",
                x: 24,
                y: 308,
                icon_char: '✦',
                icon_color: Color::ACCENT_PURPLE,
            },
        ];

        Self {
            icons,
            selected_icon: None,
            stars,
        }
    }

    /// Render wallpaper, starfield, system branding, and desktop icons.
    pub fn render(&self, canvas: &mut Canvas, mouse_x: i32, mouse_y: i32) {
        let w = canvas.width as i32;
        let h = canvas.height as i32;

        // 1. Cosmic Gradient Wallpaper
        canvas.draw_gradient_v(0, 0, w, h - 36, Color::BG_DEEP_SPACE, Color::BG_NEBULA_PURPLE);

        // 2. Starfield
        for &(sx, sy, col) in self.stars.iter() {
            canvas.set_pixel_blend(sx, sy, col);
        }

        // 3. Central Glowing Watermark Banner
        let logo_text = "✦ CosmosKernel";
        let sub_text = "64-Bit Monolithic Rust Operating System";
        let logo_w = measure_string(logo_text) * 2;
        let logo_x = (w - logo_w) / 2;
        let logo_y = (h - 36) / 2 - 40;

        draw_string_scaled(
            canvas,
            logo_x,
            logo_y,
            logo_text,
            Color::argb(45, 187, 154, 247), // Soft purple glow
            2,
        );
        let sub_w = measure_string(sub_text);
        draw_string(
            canvas,
            (w - sub_w) / 2,
            logo_y + 40,
            sub_text,
            Color::argb(60, 192, 202, 245),
        );

        // 4. Desktop Icons
        for icon in self.icons.iter() {
            let is_hovered = mouse_x >= icon.x - 6
                && mouse_x <= icon.x + 58
                && mouse_y >= icon.y - 6
                && mouse_y <= icon.y + 74;
            let is_selected = self.selected_icon == Some(icon.id);

            // Icon card selection/hover highlight
            if is_selected {
                canvas.fill_rounded_rect(
                    icon.x - 6,
                    icon.y - 6,
                    64,
                    80,
                    6,
                    Color::argb(80, 67, 56, 120),
                );
                canvas.draw_rounded_rect(
                    icon.x - 6,
                    icon.y - 6,
                    64,
                    80,
                    6,
                    Color::ACCENT_PURPLE,
                );
            } else if is_hovered {
                canvas.fill_rounded_rect(
                    icon.x - 6,
                    icon.y - 6,
                    64,
                    80,
                    6,
                    Color::argb(40, 255, 255, 255),
                );
            }

            // Icon container box (48x48)
            canvas.fill_rounded_rect(
                icon.x,
                icon.y,
                48,
                48,
                8,
                Color::BG_CARD,
            );
            canvas.draw_rounded_rect(
                icon.x,
                icon.y,
                48,
                48,
                8,
                Color::WINDOW_BORDER,
            );

            // Center icon character (scaled or centered)
            draw_char(canvas, icon.x + 20, icon.y + 16, icon.icon_char, icon.icon_color);

            // Icon title centered underneath
            let title_w = measure_string(icon.title);
            let tx = icon.x + 24 - (title_w / 2);
            draw_string_shadow(
                canvas,
                tx,
                icon.y + 54,
                icon.title,
                Color::TEXT_PRIMARY,
                Color::BLACK,
            );
        }
    }

    /// Check which icon is clicked.
    pub fn check_click(&self, x: i32, y: i32) -> Option<DesktopAppId> {
        for icon in self.icons.iter() {
            if x >= icon.x - 6 && x <= icon.x + 58 && y >= icon.y - 6 && y <= icon.y + 74 {
                return Some(icon.id);
            }
        }
        None
    }
}
