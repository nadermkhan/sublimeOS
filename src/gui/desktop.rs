use crate::gui::canvas::Canvas;
use crate::gui::color::Color;
use crate::gui::font::{draw_string, draw_ui_string, draw_ui_string_shadow, measure_ui_string};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DesktopAppId {
    Terminal,
    Sysmon,
    FileManager,
    About,
    Calculator,
    TextEdit,
    Settings,
    Studio,
    RemoteGui,
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
    pub target_path: Option<&'static str>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DesktopMenuAction {
    None,
    OpenIcon(usize),
    OpenTerminal,
    OpenFiles,
    OpenStudio,
    OpenDisplaySettings,
    ToggleTheme,
    NextWallpaper,
    Refresh,
}

pub struct Desktop {
    pub icons: [DesktopIcon; 7],
    pub selected_icon: Option<usize>,
    pub hovered_icon: Option<usize>,
    last_click_icon: Option<usize>,
    last_click_ms: u64,
    pub last_launched_target: Option<&'static str>,
    pub context_menu: Option<(i32, i32)>,
    pub context_icon: Option<usize>,
    wallpaper_cache: alloc::vec::Vec<u32>,
    cached_w: u32,
    cached_h: u32,
    cached_theme: u8,
    cached_wall_gen: u64,
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
                target_path: Some("/"),
            },
            DesktopIcon {
                id: DesktopAppId::FileManager,
                kind: XpIconKind::MyDocuments,
                title: "Documents",
                x: 56,
                y: 98,
                target_path: Some("/Users/sublime/Documents"),
            },
            DesktopIcon {
                id: DesktopAppId::Settings,
                kind: XpIconKind::NetworkPlaces,
                title: "Control Panel",
                x: 56,
                y: 178,
                target_path: None,
            },
            DesktopIcon {
                id: DesktopAppId::FileManager,
                kind: XpIconKind::RecycleBin,
                title: "Trash",
                x: 56,
                y: 258,
                target_path: Some("/Trash"),
            },
            DesktopIcon {
                id: DesktopAppId::Terminal,
                kind: XpIconKind::CommandPrompt,
                title: "Terminal",
                x: 56,
                y: 338,
                target_path: None,
            },
            DesktopIcon {
                id: DesktopAppId::TextEdit,
                kind: XpIconKind::Notepad,
                title: "Notepad",
                x: 56,
                y: 418,
                target_path: None,
            },
            DesktopIcon {
                id: DesktopAppId::Calculator,
                kind: XpIconKind::Calculator,
                title: "Calculator",
                x: 56,
                y: 498,
                target_path: None,
            },
        ];

        Self {
            icons,
            selected_icon: None,
            hovered_icon: None,
            last_click_icon: None,
            last_click_ms: 0,
            last_launched_target: None,
            context_menu: None,
            context_icon: None,
            wallpaper_cache: alloc::vec::Vec::new(),
            cached_w: 0,
            cached_h: 0,
            cached_theme: 255,
            cached_wall_gen: 0,
        }
    }

    pub fn icon_rect(&self, idx: usize) -> Option<libgui::gfx::Rect> {
        let icon = self.icons.get(idx)?;
        let title_w = measure_ui_string(icon.title);
        let hit_w = title_w.max(56) + 12;
        let hit_x = icon.x + 18 - hit_w / 2;
        Some(libgui::gfx::Rect::new(hit_x - 2, icon.y - 6, hit_w + 4, 70))
    }

    /// Update hovered icon index and return `(old_rect, new_rect)` when hover state changes.
    pub fn update_hover(&mut self, mouse_x: i32, mouse_y: i32) -> (Option<libgui::gfx::Rect>, Option<libgui::gfx::Rect>) {
        let mut next = None;
        for (idx, icon) in self.icons.iter().enumerate() {
            let title_w = measure_ui_string(icon.title);
            let hit_w = title_w.max(56) + 12;
            let hit_x = icon.x + 18 - hit_w / 2;
            if mouse_x >= hit_x && mouse_x <= hit_x + hit_w && mouse_y >= icon.y - 4 && mouse_y <= icon.y + 62 {
                next = Some(idx);
                break;
            }
        }
        if next == self.hovered_icon {
            return (None, None);
        }
        let prev = self.hovered_icon.and_then(|i| self.icon_rect(i));
        self.hovered_icon = next;
        let cur = next.and_then(|i| self.icon_rect(i));
        (prev, cur)
    }

    /// Render Geometric Metro Ribbon Wallpaper (cached across frames per theme) and left-aligned icons.
    pub fn render(&mut self, canvas: &mut Canvas, mouse_x: i32, mouse_y: i32) {
        let theme = libgui::gfx::current_theme();
        let theme_id = theme.kind as u8;
        let wall_gen = crate::gui::wallpaper::wallpaper_generation();
        let w = canvas.width as i32;
        let h = canvas.height as i32;
        let work_h = (h - 36).max(1);
        let work_pixels = (canvas.width as usize).saturating_mul(work_h as usize);

        if self.cached_w == canvas.width
            && self.cached_h == canvas.height
            && self.cached_theme == theme_id
            && self.cached_wall_gen == wall_gen
            && self.wallpaper_cache.len() == work_pixels
            && canvas.buffer.len() >= work_pixels
        {
            let (cx0, cy0, cx1, cy1) = canvas.clip_bounds();
            let rx0 = cx0.clamp(0, w) as usize;
            let rx1 = cx1.clamp(0, w) as usize;
            let ry0 = cy0.clamp(0, work_h) as usize;
            let ry1 = cy1.clamp(0, work_h) as usize;
            if rx1 > rx0 && ry1 > ry0 {
                let stride = canvas.width as usize;
                let span = rx1 - rx0;
                for row in ry0..ry1 {
                    let off = row * stride + rx0;
                    canvas.buffer[off..off + span].copy_from_slice(&self.wallpaper_cache[off..off + span]);
                }
            }
        } else {
            if canvas.buffer.len() >= work_pixels {
                crate::gui::wallpaper::render_wallpaper_to_buffer(
                    &mut canvas.buffer,
                    canvas.width as usize,
                    0,
                    0,
                    w,
                    work_h,
                    &theme,
                );
                self.wallpaper_cache.resize(work_pixels, 0);
                self.wallpaper_cache.copy_from_slice(&canvas.buffer[..work_pixels]);
                self.cached_w = canvas.width;
                self.cached_h = canvas.height;
                self.cached_theme = theme_id;
                self.cached_wall_gen = wall_gen;
            }
        }

        // 4. Left-Aligned Modern Desktop Icons (sharp Metro highlight boxes, zero rounded bevels)
        let (cx0, cy0, cx1, cy1) = canvas.clip_bounds();
        for (idx, icon) in self.icons.iter().enumerate() {
            let title_w = measure_ui_string(icon.title);
            let hit_w = title_w.max(56) + 12;
            let hit_x = icon.x + 18 - hit_w / 2;
            if hit_x + hit_w < cx0 || hit_x > cx1 || icon.y + 64 < cy0 || icon.y - 6 > cy1 {
                continue;
            }

            let is_hovered = mouse_x >= hit_x
                && mouse_x <= hit_x + hit_w
                && mouse_y >= icon.y - 4
                && mouse_y <= icon.y + 62;
            let is_selected = self.selected_icon == Some(idx);

            if is_selected {
                canvas.fill_rect(hit_x, icon.y - 4, hit_w, 64, Color::argb(120, 0, 120, 215));
                canvas.draw_rect(hit_x, icon.y - 4, hit_w, 64, Color::argb(230, 110, 195, 255));
            } else if is_hovered {
                canvas.fill_rect(hit_x, icon.y - 4, hit_w, 64, Color::argb(55, 255, 255, 255));
                canvas.draw_rect(hit_x, icon.y - 4, hit_w, 64, Color::argb(110, 255, 255, 255));
            }

            draw_xp_icon(canvas, icon.x, icon.y, icon.kind);

            let tx = icon.x + 18 - (title_w / 2);
            draw_ui_string_shadow(
                canvas,
                tx,
                icon.y + 42,
                icon.title,
                Color::WHITE,
                Color::argb(200, 0, 12, 32),
            );
        }
    }

    /// Select icon on single-click; return `Some(app_id)` only on double-click (real-OS desktop behavior).
    pub fn check_click(&mut self, x: i32, y: i32) -> Option<DesktopAppId> {
        let now_ms = crate::arch::x86_64::pit::get_ticks();
        for (idx, icon) in self.icons.iter().enumerate() {
            let title_w = measure_ui_string(icon.title);
            let hit_w = title_w.max(56) + 12;
            let hit_x = icon.x + 18 - hit_w / 2;
            if x >= hit_x && x <= hit_x + hit_w && y >= icon.y - 4 && y <= icon.y + 62 {
                let is_double = self.last_click_icon == Some(idx)
                    && now_ms.saturating_sub(self.last_click_ms) <= 550;
                self.selected_icon = Some(idx);
                if is_double {
                    self.last_click_icon = None;
                    self.last_launched_target = icon.target_path;
                    return Some(icon.id);
                } else {
                    self.last_click_icon = Some(idx);
                    self.last_click_ms = now_ms;
                    return None;
                }
            }
        }
        self.selected_icon = None;
        self.last_click_icon = None;
        None
    }

    pub fn open_context_menu(&mut self, x: i32, y: i32, sw: i32, sh: i32) {
        self.context_icon = None;
        for (idx, icon) in self.icons.iter().enumerate() {
            let title_w = measure_ui_string(icon.title);
            let hit_w = title_w.max(56) + 12;
            let hit_x = icon.x + 18 - hit_w / 2;
            if x >= hit_x && x <= hit_x + hit_w && y >= icon.y - 4 && y <= icon.y + 62 {
                self.context_icon = Some(idx);
                self.selected_icon = Some(idx);
                break;
            }
        }
        let mw = 192;
        let mh = 176;
        let mx = x.min((sw - mw - 4).max(0)).max(0);
        let my = y.min((sh - 36 - mh).max(0)).max(0);
        self.context_menu = Some((mx, my));
    }

    pub fn handle_context_click(&mut self, x: i32, y: i32) -> Option<DesktopMenuAction> {
        let (mx, my) = self.context_menu?;
        let ctx_icon = self.context_icon;
        let mw = 192;
        let mh = 176;
        self.context_menu = None;
        self.context_icon = None;
        if x < mx || x > mx + mw || y < my || y > my + mh {
            return None;
        }
        let row = (y - (my + 4)) / 24;
        if let Some(idx) = ctx_icon {
            return Some(match row {
                0 => DesktopMenuAction::OpenIcon(idx),
                1 => DesktopMenuAction::OpenTerminal,
                2 => DesktopMenuAction::OpenFiles,
                3 => DesktopMenuAction::Refresh,
                4 => DesktopMenuAction::ToggleTheme,
                5 => DesktopMenuAction::NextWallpaper,
                _ => DesktopMenuAction::OpenDisplaySettings,
            });
        }
        Some(match row {
            0 => DesktopMenuAction::OpenTerminal,
            1 => DesktopMenuAction::OpenFiles,
            2 => DesktopMenuAction::OpenStudio,
            3 => DesktopMenuAction::Refresh,
            4 => DesktopMenuAction::ToggleTheme,
            5 => DesktopMenuAction::NextWallpaper,
            _ => DesktopMenuAction::OpenDisplaySettings,
        })
    }

    pub fn render_context_menu(&self, canvas: &mut Canvas, mouse_x: i32, mouse_y: i32) {
        let Some((mx, my)) = self.context_menu else {
            return;
        };
        let mw = 192;
        let mh = 176;
        let theme = libgui::gfx::current_theme();
        canvas.fill_rect(mx + 2, my + 2, mw, mh, Color::argb(40, 0, 0, 0));
        canvas.fill_rect(mx, my, mw, mh, Color::from_libgui(theme.face_light));
        canvas.draw_rect(mx, my, mw, mh, Color::from_libgui(theme.ctrl_border));

        let open_lbl;
        let first_lbl = if let Some(idx) = self.context_icon {
            open_lbl = alloc::format!("Open {}", self.icons[idx].title);
            open_lbl.as_str()
        } else {
            "Open Terminal"
        };
        let labels: [&str; 7] = if self.context_icon.is_some() {
            [
                first_lbl,
                "Open Terminal",
                "Open File Explorer",
                "Refresh Desktop",
                "Switch Theme (Light/Dark)",
                "Next Wallpaper",
                "Personalize & Wallpapers...",
            ]
        } else {
            [
                "Open Terminal",
                "Open File Explorer",
                "Launch Widget Studio",
                "Refresh Desktop",
                "Switch Theme (Light/Dark)",
                "Next Wallpaper",
                "Personalize & Wallpapers...",
            ]
        };
        for (i, &lbl) in labels.iter().enumerate() {
            let ry = my + 4 + (i as i32) * 24;
            let hover = mouse_x >= mx + 2 && mouse_x < mx + mw - 2 && mouse_y >= ry && mouse_y < ry + 22;
            if hover {
                canvas.fill_rect(mx + 2, ry, mw - 4, 22, Color::from_libgui(theme.selection));
            }
            draw_ui_string(
                canvas,
                mx + 10,
                ry + 2,
                lbl,
                Color::from_libgui(if hover { theme.selection_text } else { theme.text_primary }),
            );
            if i == 2 || i == 3 {
                canvas.fill_rect(mx + 6, ry + 23, mw - 12, 1, Color::from_libgui(theme.border_shadow));
            }
        }
    }
}

/// Draw crisp 36x36 Windows 8.1 Modern flat desktop icon artwork.
pub fn draw_xp_icon(canvas: &mut Canvas, x: i32, y: i32, kind: XpIconKind) {
    match kind {
        XpIconKind::MyComputer => {
            canvas.fill_rect(x + 2, y + 3, 32, 22, Color::rgb(42, 52, 68));
            canvas.fill_rect(x + 4, y + 5, 28, 17, Color::rgb(0, 120, 215));
            canvas.fill_rect(x + 4, y + 5, 16, 17, Color::rgb(28, 162, 248));
            canvas.fill_rect(x + 14, y + 25, 8, 4, Color::rgb(175, 182, 192));
            canvas.fill_rect(x + 9, y + 29, 18, 4, Color::rgb(205, 212, 220));
        }
        XpIconKind::MyDocuments => {
            canvas.fill_rect(x + 3, y + 6, 12, 4, Color::rgb(232, 172, 28));
            canvas.fill_rect(x + 3, y + 9, 30, 23, Color::rgb(252, 204, 62));
            canvas.fill_rect(x + 3, y + 14, 30, 18, Color::rgb(245, 186, 36));
            canvas.draw_rect(x + 3, y + 9, 30, 23, Color::rgb(198, 144, 16));
        }
        XpIconKind::NetworkPlaces => {
            canvas.fill_rect(x + 4, y + 4, 28, 28, Color::rgb(0, 130, 153));
            canvas.fill_circle(x + 18, y + 18, 8, Color::WHITE);
            canvas.fill_circle(x + 18, y + 18, 5, Color::rgb(0, 130, 153));
            canvas.fill_rect(x + 16, y + 7, 4, 4, Color::WHITE);
            canvas.fill_rect(x + 16, y + 25, 4, 4, Color::WHITE);
            canvas.fill_rect(x + 7, y + 16, 4, 4, Color::WHITE);
            canvas.fill_rect(x + 25, y + 16, 4, 4, Color::WHITE);
        }
        XpIconKind::RecycleBin => {
            canvas.fill_rect(x + 7, y + 6, 22, 3, Color::rgb(225, 232, 240));
            canvas.fill_rect(x + 9, y + 10, 18, 22, Color::rgb(212, 226, 242));
            canvas.draw_rect(x + 9, y + 10, 18, 22, Color::rgb(112, 138, 170));
            canvas.fill_rect(x + 13, y + 14, 2, 14, Color::rgb(112, 138, 170));
            canvas.fill_rect(x + 17, y + 14, 2, 14, Color::rgb(112, 138, 170));
            canvas.fill_rect(x + 21, y + 14, 2, 14, Color::rgb(112, 138, 170));
        }
        XpIconKind::CommandPrompt => {
            canvas.fill_rect(x + 3, y + 5, 30, 26, Color::rgb(26, 28, 34));
            canvas.fill_rect(x + 3, y + 5, 30, 5, Color::rgb(0, 120, 215));
            canvas.draw_rect(x + 3, y + 5, 30, 26, Color::rgb(110, 130, 160));
            draw_string(canvas, x + 9, y + 13, ">_", Color::rgb(120, 245, 165));
        }
        XpIconKind::Notepad => {
            canvas.fill_rect(x + 6, y + 4, 24, 29, Color::WHITE);
            canvas.fill_rect(x + 6, y + 4, 24, 6, Color::rgb(38, 114, 236));
            canvas.draw_rect(x + 6, y + 4, 24, 29, Color::rgb(90, 115, 155));
            canvas.fill_rect(x + 10, y + 14, 16, 2, Color::rgb(120, 165, 220));
            canvas.fill_rect(x + 10, y + 19, 14, 2, Color::rgb(120, 165, 220));
            canvas.fill_rect(x + 10, y + 24, 16, 2, Color::rgb(120, 165, 220));
        }
        XpIconKind::Calculator => {
            canvas.fill_rect(x + 6, y + 3, 24, 30, Color::rgb(172, 25, 61));
            canvas.fill_rect(x + 9, y + 6, 18, 7, Color::WHITE);
            for r in 0..3 {
                for c in 0..3 {
                    let kc = if c == 2 && r == 2 {
                        Color::rgb(255, 200, 75)
                    } else {
                        Color::rgb(240, 242, 248)
                    };
                    canvas.fill_rect(x + 9 + c * 6, y + 16 + r * 5, 5, 4, kc);
                }
            }
        }
    }
}

/// Draw crisp geometric SublimeOS Prismatic 'S' Crest logo (replaces Windows flag emblems).
pub fn draw_sublime_logo(
    canvas: &mut Canvas,
    x: i32,
    y: i32,
    size: i32,
    primary: Color,
    accent: Color,
) {
    let s = size.max(10);
    let pad = (s / 8).max(1);
    let w = (s - pad * 2).max(6);
    let h = w;
    let x0 = x + pad;
    let y0 = y + pad;
    let bar = (s / 5).max(2);
    let stem = (s / 5).max(2);
    let mid_y = y0 + (h - bar) / 2;

    // Subtle outer prism corner frame + top-right cosmic facet dot
    canvas.fill_rect(x0, y0, w - bar - 1, bar, primary);
    canvas.fill_rect(x0 + w - bar, y0, bar, bar, accent);
    canvas.fill_rect(x0, y0 + bar, stem, (mid_y - y0 - bar).max(1), primary);
    canvas.fill_rect(x0 + 1, mid_y, (w - 2).max(4), bar, accent);
    let lower_y = mid_y + bar;
    let lower_h = ((y0 + h - bar) - lower_y).max(1);
    canvas.fill_rect(x0 + w - stem, lower_y, stem, lower_h, primary);
    canvas.fill_rect(x0 + bar + 1, y0 + h - bar, (w - bar - 1).max(3), bar, primary);
    canvas.fill_rect(x0, y0 + h - bar, bar, bar, accent);
}
