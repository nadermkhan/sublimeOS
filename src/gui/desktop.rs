use crate::gui::canvas::Canvas;
use crate::gui::color::Color;
use crate::gui::font::{draw_string, draw_string_shadow, measure_string};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DesktopAppId {
    Terminal,
    Sysmon,
    FileManager,
    About,
    Calculator,
    TextEdit,
    Settings,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum XpIconKind {
    MyComputer,
    MyDocuments,
    NetworkPlaces,
    RecycleBin,
    CommandPrompt,
    Notepad,
    Calculator,
}

pub struct DesktopIcon {
    pub id: DesktopAppId,
    pub kind: XpIconKind,
    pub title: &'static str,
    pub x: i32,
    pub y: i32,
}

pub struct Desktop {
    pub icons: [DesktopIcon; 7],
    pub selected_icon: Option<usize>,
    wallpaper_cache: alloc::vec::Vec<u32>,
    cached_w: u32,
    cached_h: u32,
}

impl Desktop {
    pub fn new() -> Self {
        let icons = [
            DesktopIcon {
                id: DesktopAppId::FileManager,
                kind: XpIconKind::MyComputer,
                title: "My Computer",
                x: 56,
                y: 18,
            },
            DesktopIcon {
                id: DesktopAppId::FileManager,
                kind: XpIconKind::MyDocuments,
                title: "Documents",
                x: 56,
                y: 98,
            },
            DesktopIcon {
                id: DesktopAppId::Settings,
                kind: XpIconKind::NetworkPlaces,
                title: "Control Panel",
                x: 56,
                y: 178,
            },
            DesktopIcon {
                id: DesktopAppId::FileManager,
                kind: XpIconKind::RecycleBin,
                title: "Trash",
                x: 56,
                y: 258,
            },
            DesktopIcon {
                id: DesktopAppId::Terminal,
                kind: XpIconKind::CommandPrompt,
                title: "Terminal",
                x: 56,
                y: 338,
            },
            DesktopIcon {
                id: DesktopAppId::TextEdit,
                kind: XpIconKind::Notepad,
                title: "Notepad",
                x: 56,
                y: 418,
            },
            DesktopIcon {
                id: DesktopAppId::Calculator,
                kind: XpIconKind::Calculator,
                title: "Calculator",
                x: 56,
                y: 498,
            },
        ];

        Self {
            icons,
            selected_icon: None,
            wallpaper_cache: alloc::vec::Vec::new(),
            cached_w: 0,
            cached_h: 0,
        }
    }

    /// Render Windows XP Bliss rolling hills wallpaper (cached across frames) and left-aligned icons.
    pub fn render(&mut self, canvas: &mut Canvas, mouse_x: i32, mouse_y: i32) {
        let w = canvas.width as i32;
        let h = canvas.height as i32;
        let work_h = (h - 30).max(1);
        let work_pixels = (canvas.width as usize).saturating_mul(work_h as usize);

        if self.cached_w == canvas.width
            && self.cached_h == canvas.height
            && self.wallpaper_cache.len() == work_pixels
            && canvas.buffer.len() >= work_pixels
        {
            canvas.buffer[..work_pixels].copy_from_slice(&self.wallpaper_cache);
        } else {
            // 1. Bliss Blue Sky Gradient (top 64% of workspace)
            let sky_h = (work_h * 64) / 100;
            canvas.draw_gradient_v(0, 0, w, sky_h, Color::XP_SKY_TOP, Color::XP_SKY_HORIZON);
            canvas.fill_rect(0, sky_h, w, work_h - sky_h, Color::XP_SKY_HORIZON);

            // 2. Soft Bliss Clouds in the upper sky
            draw_bliss_cloud(canvas, w / 5, work_h / 7, 130, 32);
            draw_bliss_cloud(canvas, (w * 58) / 100, work_h / 5, 175, 38);
            draw_bliss_cloud(canvas, (w * 80) / 100, work_h / 9, 110, 26);

            // 3. Procedural Rolling Green Hills (far ridge + bright foreground meadow)
            let stride = canvas.width as usize;
            for px in 0..w {
                let nx = (px * 1000) / w.max(1); // 0..1000 normalized X

                // Far distant hill: gentle swell higher on right/center
                let d1 = nx - 650;
                let hill_far_y = (work_h * 57) / 100 + ((d1 * d1) / 3800) - (nx / 25);

                // Foreground signature Bliss hill: swooping crest on the left-center descending right
                let d2 = nx - 310;
                let hill_near_y = (work_h * 54) / 100 + ((d2 * d2) / 1950);

                let far_top = hill_far_y.clamp(0, work_h);
                let near_top = hill_near_y.clamp(0, work_h);

                // Paint distant hill strip visible above foreground hill
                if far_top < near_top {
                    for py in far_top..near_top {
                        let depth = ((py - far_top) * 255) / ((work_h - far_top).max(1));
                        let col = Color::lerp(Color::XP_HILL_FAR, Color::XP_HILL_MID, depth as u32);
                        canvas.buffer[py as usize * stride + px as usize] = col.to_u32();
                    }
                }

                // Paint lush foreground green hill down to taskbar top
                let near_span = (work_h - near_top).max(1);
                for py in near_top..work_h {
                    let t = ((py - near_top) * 255) / near_span;
                    let col = if t < 96 {
                        Color::lerp(Color::XP_HILL_LIGHT, Color::XP_HILL_MID, ((t * 255) / 96) as u32)
                    } else {
                        Color::lerp(Color::XP_HILL_MID, Color::XP_HILL_DARK, (((t - 96) * 255) / 159) as u32)
                    };
                    canvas.buffer[py as usize * stride + px as usize] = col.to_u32();
                }
            }

            if canvas.buffer.len() >= work_pixels {
                self.wallpaper_cache.resize(work_pixels, 0);
                self.wallpaper_cache.copy_from_slice(&canvas.buffer[..work_pixels]);
                self.cached_w = canvas.width;
                self.cached_h = canvas.height;
            }
        }

        // 4. Left-Aligned Windows XP Desktop Icons
        for (idx, icon) in self.icons.iter().enumerate() {
            let title_w = measure_string(icon.title);
            let hit_w = title_w.max(56) + 12;
            let hit_x = icon.x + 18 - hit_w / 2;

            let is_hovered = mouse_x >= hit_x
                && mouse_x <= hit_x + hit_w
                && mouse_y >= icon.y - 4
                && mouse_y <= icon.y + 62;
            let is_selected = self.selected_icon == Some(idx);

            if is_selected {
                canvas.fill_rect(hit_x, icon.y - 4, hit_w, 64, Color::argb(145, 49, 106, 197));
                canvas.draw_rect(hit_x, icon.y - 4, hit_w, 64, Color::argb(220, 180, 212, 255));
            } else if is_hovered {
                canvas.fill_rect(hit_x, icon.y - 4, hit_w, 64, Color::argb(65, 255, 255, 255));
            }

            draw_xp_icon(canvas, icon.x, icon.y, icon.kind);

            let tx = icon.x + 18 - (title_w / 2);
            if is_selected {
                canvas.fill_rect(tx - 3, icon.y + 41, title_w + 6, 17, Color::XP_SELECTION);
                draw_string(canvas, tx, icon.y + 42, icon.title, Color::WHITE);
            } else {
                draw_string_shadow(
                    canvas,
                    tx,
                    icon.y + 42,
                    icon.title,
                    Color::WHITE,
                    Color::argb(210, 0, 20, 50),
                );
            }
        }
    }

    /// Check which icon is clicked and update desktop selection.
    pub fn check_click(&mut self, x: i32, y: i32) -> Option<DesktopAppId> {
        for (idx, icon) in self.icons.iter().enumerate() {
            let title_w = measure_string(icon.title);
            let hit_w = title_w.max(56) + 12;
            let hit_x = icon.x + 18 - hit_w / 2;
            if x >= hit_x && x <= hit_x + hit_w && y >= icon.y - 4 && y <= icon.y + 62 {
                self.selected_icon = Some(idx);
                return Some(icon.id);
            }
        }
        self.selected_icon = None;
        None
    }
}

fn draw_bliss_cloud(canvas: &mut Canvas, cx: i32, cy: i32, width: i32, height: i32) {
    let puffs = [
        (-width / 3, 4, width / 3, (height * 3) / 4),
        (-width / 8, -4, (width * 2) / 5, height),
        (width / 6, -1, width / 3, (height * 4) / 5),
        (width / 3, 6, width / 4, (height * 3) / 5),
    ];
    for (ox, oy, pw, ph) in puffs {
        canvas.fill_rounded_rect(
            cx + ox - pw / 2,
            cy + oy - ph / 2,
            pw,
            ph,
            ph / 2,
            Color::argb(55, 255, 255, 255),
        );
        canvas.fill_rounded_rect(
            cx + ox - pw / 2 + 4,
            cy + oy - ph / 2 + 3,
            (pw - 8).max(4),
            (ph - 6).max(4),
            ph / 2,
            Color::argb(65, 255, 255, 255),
        );
    }
}

/// Draw crisp 36x36 Windows XP style icon artwork.
pub fn draw_xp_icon(canvas: &mut Canvas, x: i32, y: i32, kind: XpIconKind) {
    match kind {
        XpIconKind::MyComputer => {
            // Classic CRT/LCD monitor + base
            canvas.fill_rounded_rect(x + 3, y + 2, 30, 23, 3, Color::rgb(220, 224, 232));
            canvas.draw_rounded_rect(x + 3, y + 2, 30, 23, 3, Color::rgb(70, 90, 120));
            canvas.fill_rect(x + 6, y + 5, 24, 16, Color::rgb(42, 118, 224));
            canvas.fill_rect(x + 6, y + 5, 24, 4, Color::rgb(110, 178, 255));
            canvas.fill_rect(x + 13, y + 25, 10, 4, Color::rgb(180, 184, 194));
            canvas.fill_rect(x + 8, y + 29, 20, 5, Color::rgb(210, 212, 220));
            canvas.draw_rect(x + 8, y + 29, 20, 5, Color::rgb(90, 95, 110));
        }
        XpIconKind::MyDocuments => {
            // Classic yellow XP folder + white paper sheet peeking out
            canvas.fill_rect(x + 4, y + 6, 13, 5, Color::rgb(242, 198, 65));
            canvas.fill_rounded_rect(x + 3, y + 9, 30, 23, 2, Color::rgb(252, 215, 86));
            canvas.fill_rect(x + 8, y + 11, 20, 10, Color::WHITE);
            canvas.fill_rect(x + 3, y + 14, 30, 18, Color::rgb(245, 190, 48));
            canvas.draw_rect(x + 3, y + 9, 30, 23, Color::rgb(176, 128, 18));
        }
        XpIconKind::NetworkPlaces => {
            // Classic XP blue control/network globe + wrench card
            canvas.fill_rounded_rect(x + 4, y + 4, 28, 28, 4, Color::rgb(236, 233, 216));
            canvas.draw_rounded_rect(x + 4, y + 4, 28, 28, 4, Color::rgb(0, 60, 116));
            canvas.fill_circle(x + 18, y + 17, 9, Color::rgb(45, 135, 235));
            canvas.draw_circle(x + 18, y + 17, 9, Color::rgb(18, 72, 150));
            canvas.fill_rect(x + 11, y + 14, 7, 5, Color::rgb(92, 194, 76));
            canvas.fill_rect(x + 19, y + 18, 6, 5, Color::rgb(92, 194, 76));
        }
        XpIconKind::RecycleBin => {
            // Classic translucent-blue XP bin
            canvas.fill_rect(x + 7, y + 5, 22, 4, Color::rgb(190, 210, 235));
            canvas.draw_rect(x + 7, y + 5, 22, 4, Color::rgb(70, 100, 145));
            canvas.fill_rect(x + 9, y + 9, 18, 23, Color::rgb(218, 232, 248));
            canvas.draw_rect(x + 9, y + 9, 18, 23, Color::rgb(85, 120, 170));
            canvas.fill_rect(x + 12, y + 13, 3, 15, Color::rgb(145, 180, 220));
            canvas.fill_rect(x + 17, y + 13, 3, 15, Color::rgb(145, 180, 220));
            canvas.fill_rect(x + 22, y + 13, 2, 15, Color::rgb(145, 180, 220));
        }
        XpIconKind::CommandPrompt => {
            // Classic ">_" black terminal box with blue header
            canvas.fill_rect(x + 3, y + 4, 30, 28, Color::BLACK);
            canvas.fill_rect(x + 3, y + 4, 30, 6, Color::rgb(0, 84, 227));
            canvas.draw_rect(x + 3, y + 4, 30, 28, Color::rgb(200, 200, 210));
            draw_string(canvas, x + 9, y + 13, ">_", Color::rgb(130, 255, 160));
        }
        XpIconKind::Notepad => {
            // Classic spiral-bound blue/white XP Notepad
            canvas.fill_rect(x + 6, y + 4, 24, 29, Color::WHITE);
            canvas.fill_rect(x + 6, y + 4, 24, 6, Color::rgb(42, 122, 228));
            canvas.draw_rect(x + 6, y + 4, 24, 29, Color::rgb(70, 95, 135));
            canvas.fill_rect(x + 10, y + 14, 16, 2, Color::rgb(130, 165, 215));
            canvas.fill_rect(x + 10, y + 19, 14, 2, Color::rgb(130, 165, 215));
            canvas.fill_rect(x + 10, y + 24, 16, 2, Color::rgb(130, 165, 215));
        }
        XpIconKind::Calculator => {
            // Classic XP Calculator blue housing + white screen + colored keys
            canvas.fill_rounded_rect(x + 6, y + 3, 24, 30, 3, Color::rgb(142, 178, 228));
            canvas.draw_rounded_rect(x + 6, y + 3, 24, 30, 3, Color::rgb(35, 72, 130));
            canvas.fill_rect(x + 9, y + 6, 18, 7, Color::WHITE);
            canvas.draw_rect(x + 9, y + 6, 18, 7, Color::rgb(90, 115, 150));
            for r in 0..3 {
                for c in 0..3 {
                    let kc = if c == 2 && r == 2 {
                        Color::rgb(218, 65, 42)
                    } else {
                        Color::rgb(245, 244, 238)
                    };
                    canvas.fill_rect(x + 9 + c * 6, y + 16 + r * 5, 5, 4, kc);
                }
            }
        }
    }
}
