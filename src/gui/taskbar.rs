use alloc::string::String;
use crate::arch::x86_64::rtc::get_system_datetime;
use crate::gui::canvas::Canvas;
use crate::gui::color::Color;
use crate::gui::desktop::DesktopAppId;
use crate::gui::font::{draw_char, draw_string};
use crate::mm::pmm::PMM;

pub const TOPBAR_HEIGHT: i32 = 26;
pub const DOCK_HEIGHT: i32 = 52;

pub enum TaskbarAction {
    None,
    ToggleStartMenu,
    ToggleSpotlight,
    LaunchApp(DesktopAppId),
    FocusWindow(u32),
    RestoreWindow(u32),
    ExitToShell,
    RebootSystem,
    HaltSystem,
}

pub struct TaskbarTab {
    pub window_id: u32,
    pub title: String,
    pub is_active: bool,
    pub is_minimized: bool,
    pub app_id: DesktopAppId,
}

pub struct Taskbar {
    pub cosmic_menu_open: bool,
    pub control_center_open: bool,
    pub volume_level: u8,
}

impl Taskbar {
    pub fn new() -> Self {
        Self {
            cosmic_menu_open: false,
            control_center_open: false,
            volume_level: 85,
        }
    }

    /// Render SublimeOS Top Menu Bar and Bottom Floating Dock.
    pub fn render(
        &self,
        canvas: &mut Canvas,
        tabs: &[TaskbarTab],
        mouse_x: i32,
        mouse_y: i32,
    ) {
        let sw = canvas.width as i32;
        let sh = canvas.height as i32;

        // ==========================================
        // 1. macOS Top Menu Bar (Global Menu Bar)
        // ==========================================
        canvas.blur_rect(0, 0, sw, TOPBAR_HEIGHT, 1);
        canvas.fill_rect(0, 0, sw, TOPBAR_HEIGHT, Color::argb(215, 26, 27, 36));
        canvas.fill_rect(0, TOPBAR_HEIGHT - 1, sw, 1, Color::argb(60, 255, 255, 255));

        // Sublime Emblem Logo ✦
        let emblem_hover = mouse_x >= 8 && mouse_x <= 32 && mouse_y <= TOPBAR_HEIGHT;
        if self.cosmic_menu_open || emblem_hover {
            canvas.fill_rounded_rect(8, 2, 24, 22, 4, Color::argb(60, 255, 255, 255));
        }
        draw_char(canvas, 16, 5, '✦', Color::WHITE);

        // Dynamic Active App Title & Menus
        let active_tab = tabs.iter().find(|t| t.is_active);
        let (app_title, submenus): (&str, &[&str]) = match active_tab.map(|t| t.app_id) {
            Some(DesktopAppId::Terminal) => ("Terminal", &["Shell", "Edit", "View", "Window", "Help"]),
            Some(DesktopAppId::Calculator) => ("Calculator", &["Edit", "View", "Help"]),
            Some(DesktopAppId::TextEdit) => ("TextEdit", &["File", "Edit", "Format", "View", "Help"]),
            Some(DesktopAppId::Sysmon) => ("Activity Monitor", &["Edit", "View", "Window", "Help"]),
            Some(DesktopAppId::About) => ("About SublimeOS", &["View", "Help"]),
            _ => ("Finder", &["File", "Edit", "View", "Go", "Window", "Help"]),
        };

        let mut menu_x = 38;

        // Bold Active Application Title
        let app_title_w = (app_title.len() as i32 * 8) + 16;
        let app_hover = mouse_x >= menu_x && mouse_x <= menu_x + app_title_w && mouse_y <= TOPBAR_HEIGHT;
        if app_hover {
            canvas.fill_rounded_rect(menu_x, 2, app_title_w, 22, 4, Color::argb(50, 255, 255, 255));
        }
        draw_string(canvas, menu_x + 8, 5, app_title, Color::WHITE);
        menu_x += app_title_w;

        // Application Specific Menus
        for &item in submenus.iter() {
            let item_w = (item.len() as i32 * 8) + 16;
            let item_hover = mouse_x >= menu_x && mouse_x <= menu_x + item_w && mouse_y <= TOPBAR_HEIGHT;
            if item_hover {
                canvas.fill_rounded_rect(menu_x, 2, item_w, 22, 4, Color::argb(40, 255, 255, 255));
            }
            draw_string(canvas, menu_x + 8, 5, item, Color::TEXT_PRIMARY);
            menu_x += item_w;
        }

        // Right Menu Extras (RAM, Battery, Control Center, Spotlight, Clock)
        let (used_mb, total_mb) = {
            let pmm = PMM.lock();
            let used = (pmm.get_used_frames() * 4) / 1024;
            let total = (pmm.get_total_frames() * 4) / 1024;
            (used, total)
        };
        let mut mem_str = String::from("RAM: ");
        push_u64(&mut mem_str, used_mb as u64);
        mem_str.push('/');
        push_u64(&mut mem_str, total_mb as u64);
        mem_str.push_str(" MB");

        let mem_x = sw - 330;
        draw_string(canvas, mem_x, 5, &mem_str, Color::ACCENT_GREEN);

        // Control Center Capsule [CC]
        let cc_x = sw - 195;
        let cc_hover = mouse_x >= cc_x - 4 && mouse_x <= cc_x + 36 && mouse_y <= TOPBAR_HEIGHT;
        if self.control_center_open || cc_hover {
            canvas.fill_rounded_rect(cc_x - 4, 2, 40, 22, 4, Color::argb(60, 255, 255, 255));
        }
        draw_string(canvas, cc_x, 5, "[CC]", if self.control_center_open { Color::WHITE } else { Color::ACCENT_CYAN });

        // Spotlight Search Icon
        let spot_x = sw - 145;
        let spot_hover = mouse_x >= spot_x - 4 && mouse_x <= spot_x + 28 && mouse_y <= TOPBAR_HEIGHT;
        if spot_hover {
            canvas.fill_rounded_rect(spot_x - 4, 2, 32, 22, 4, Color::argb(60, 255, 255, 255));
        }
        draw_char(canvas, spot_x + 6, 5, '?', Color::WHITE);

        // RTC Clock
        let time = get_system_datetime();
        let mut clock_str = String::new();
        push_2digits(&mut clock_str, time.hour);
        clock_str.push(':');
        push_2digits(&mut clock_str, time.minute);
        clock_str.push(':');
        push_2digits(&mut clock_str, time.second);
        draw_string(canvas, sw - 96, 5, &clock_str, Color::WHITE);

        // ==========================================
        // 2. Floating Frosted Glass Dock (Bottom)
        // ==========================================
        let min_tabs: alloc::vec::Vec<&TaskbarTab> = tabs.iter().filter(|t| t.is_minimized).collect();
        let dock_w = 500 + (min_tabs.len() as i32 * 48);
        let dock_h = 52;
        let dock_x = (sw - dock_w) / 2;
        let dock_y = sh - dock_h - 10;

        // Frosted Glass Optical Blur + Tint
        canvas.blur_rect(dock_x, dock_y, dock_w, dock_h, 2);
        canvas.draw_shadow(dock_x, dock_y, dock_w, dock_h, 8, 80);
        canvas.fill_rounded_rect(dock_x, dock_y, dock_w, dock_h, 16, Color::argb(210, 28, 30, 42));
        canvas.draw_rounded_rect(dock_x, dock_y, dock_w, dock_h, 16, Color::argb(60, 255, 255, 255));

        // Dock App Items (All 7 primary macOS apps)
        let dock_apps: [(DesktopAppId, &str, char, Color); 7] = [
            (DesktopAppId::FileManager, "Finder", '@', Color::ACCENT_CYAN),
            (DesktopAppId::Terminal, "Terminal", '>', Color::ACCENT_GREEN),
            (DesktopAppId::TextEdit, "TextEdit", 'T', Color::WHITE),
            (DesktopAppId::Calculator, "Calculator", '+', Color::ACCENT_ORANGE),
            (DesktopAppId::Sysmon, "Activity", '#', Color::ACCENT_YELLOW),
            (DesktopAppId::Settings, "Settings", 'S', Color::WHITE),
            (DesktopAppId::About, "About", '✦', Color::ACCENT_PURPLE),
        ];

        let mut app_x = dock_x + 16;
        let icon_y = dock_y + 8;

        for (app_id, _title, icon_ch, icon_col) in dock_apps.into_iter() {
            let is_hover = mouse_x >= app_x - 4
                && mouse_x <= app_x + 40
                && mouse_y >= icon_y - 4
                && mouse_y <= icon_y + 40;

            if is_hover {
                canvas.fill_rounded_rect(app_x - 4, icon_y - 4, 44, 44, 10, Color::argb(50, 255, 255, 255));
            }

            // App Icon Tile
            canvas.fill_rounded_rect(app_x, icon_y, 36, 36, 8, Color::BG_CARD);
            canvas.draw_rounded_rect(app_x, icon_y, 36, 36, 8, Color::WINDOW_BORDER);
            draw_char(canvas, app_x + 14, icon_y + 10, icon_ch, icon_col);

            // Active running indicator dot underneath
            let is_running = tabs.iter().any(|t| t.app_id == app_id && !t.is_minimized);
            if is_running {
                canvas.fill_circle(app_x + 18, dock_y + 46, 2, Color::ACCENT_CYAN);
            }

            app_x += 52;
        }

        // Dock Divider
        canvas.fill_rect(app_x, dock_y + 10, 1, 32, Color::argb(80, 255, 255, 255));
        app_x += 16;

        // Minimized Windows Section (Right of divider, before Trash)
        for _min_tab in min_tabs.iter() {
            let is_hover = mouse_x >= app_x - 4
                && mouse_x <= app_x + 40
                && mouse_y >= icon_y - 4
                && mouse_y <= icon_y + 40;

            if is_hover {
                canvas.fill_rounded_rect(app_x - 4, icon_y - 4, 44, 44, 10, Color::argb(60, 255, 255, 255));
            }

            // Miniature Window Tile
            canvas.fill_rounded_rect(app_x, icon_y, 36, 36, 6, Color::argb(255, 42, 45, 58));
            canvas.draw_rounded_rect(app_x, icon_y, 36, 36, 6, Color::argb(90, 255, 255, 255));
            // Mini titlebar
            canvas.fill_rect(app_x + 1, icon_y + 1, 34, 7, Color::argb(255, 55, 58, 72));
            // Mini traffic lights (3 tiny dots)
            canvas.fill_rect(app_x + 3, icon_y + 3, 2, 2, Color::argb(255, 255, 95, 86));
            canvas.fill_rect(app_x + 7, icon_y + 3, 2, 2, Color::argb(255, 255, 189, 46));
            canvas.fill_rect(app_x + 11, icon_y + 3, 2, 2, Color::argb(255, 39, 201, 63));
            // Mini doc lines inside
            canvas.fill_rect(app_x + 4, icon_y + 13, 28, 2, Color::argb(100, 255, 255, 255));
            canvas.fill_rect(app_x + 4, icon_y + 18, 20, 2, Color::argb(60, 255, 255, 255));
            canvas.fill_rect(app_x + 4, icon_y + 23, 24, 2, Color::argb(60, 255, 255, 255));

            app_x += 48;
        }

        // Trash Icon
        canvas.fill_rounded_rect(app_x, icon_y, 36, 36, 8, Color::BG_CARD);
        canvas.draw_rounded_rect(app_x, icon_y, 36, 36, 8, Color::WINDOW_BORDER);
        draw_char(canvas, app_x + 14, icon_y + 10, 'x', Color::TEXT_MUTED);

        // ==========================================
        // 3. Apple/Sublime Menu Dropdown (if open)
        // ==========================================
        if self.cosmic_menu_open {
            self.render_cosmic_menu(canvas, mouse_x, mouse_y);
        }

        // ==========================================
        // 4. macOS Control Center (if open)
        // ==========================================
        if self.control_center_open {
            self.render_control_center(canvas, sw);
        }
    }

    fn render_control_center(&self, canvas: &mut Canvas, sw: i32) {
        let cc_w = 300;
        let cc_h = 224;
        let cc_x = sw - cc_w - 12;
        let cc_y = TOPBAR_HEIGHT + 6;

        canvas.blur_rect(cc_x, cc_y, cc_w, cc_h, 2);
        canvas.draw_shadow(cc_x, cc_y, cc_w, cc_h, 12, 100);
        canvas.fill_rounded_rect(cc_x, cc_y, cc_w, cc_h, 12, Color::argb(235, 28, 30, 42));
        canvas.draw_rounded_rect(cc_x, cc_y, cc_w, cc_h, 12, Color::argb(80, 255, 255, 255));

        // 1. Wi-Fi / Ethernet Module
        canvas.fill_rounded_rect(cc_x + 10, cc_y + 10, cc_w - 20, 40, 8, Color::argb(60, 255, 255, 255));
        draw_char(canvas, cc_x + 20, cc_y + 20, 'N', Color::ACCENT_GREEN);
        draw_string(canvas, cc_x + 38, cc_y + 14, "Network: en0 (Connected)", Color::WHITE);
        draw_string(canvas, cc_x + 38, cc_y + 30, "IP: 192.168.1.175 | 1000 Mbps", Color::TEXT_MUTED);

        // 2. Display Module
        canvas.fill_rounded_rect(cc_x + 10, cc_y + 56, cc_w - 20, 44, 8, Color::argb(60, 255, 255, 255));
        draw_string(canvas, cc_x + 20, cc_y + 64, "Display Brightness", Color::WHITE);
        canvas.fill_rounded_rect(cc_x + 20, cc_y + 82, cc_w - 40, 10, 5, Color::argb(100, 255, 255, 255));
        canvas.fill_rounded_rect(cc_x + 20, cc_y + 82, (cc_w - 40) * 9 / 10, 10, 5, Color::ACCENT_BLUE);

        // 3. Sound Module
        canvas.fill_rounded_rect(cc_x + 10, cc_y + 106, cc_w - 20, 52, 8, Color::argb(60, 255, 255, 255));
        draw_string(canvas, cc_x + 20, cc_y + 114, "Sound Output (Intel HDA)", Color::WHITE);
        let slider_w = cc_w - 40;
        let filled_w = ((self.volume_level as i32 * slider_w) / 100).clamp(4, slider_w);
        canvas.fill_rounded_rect(cc_x + 20, cc_y + 134, slider_w, 10, 5, Color::argb(100, 255, 255, 255));
        canvas.fill_rounded_rect(cc_x + 20, cc_y + 134, filled_w, 10, 5, Color::ACCENT_CYAN);
        canvas.fill_circle(cc_x + 20 + filled_w, cc_y + 139, 6, Color::WHITE);

        // 4. Battery & Dark Mode Status Bar
        canvas.fill_rounded_rect(cc_x + 10, cc_y + 164, (cc_w - 26) / 2, 48, 8, Color::argb(60, 255, 255, 255));
        draw_char(canvas, cc_x + 20, cc_y + 180, 'B', Color::ACCENT_GREEN);
        draw_string(canvas, cc_x + 38, cc_y + 174, "Battery", Color::WHITE);
        draw_string(canvas, cc_x + 38, cc_y + 190, "100% (AC)", Color::TEXT_MUTED);

        let dm_x = cc_x + 10 + (cc_w - 26) / 2 + 6;
        canvas.fill_rounded_rect(dm_x, cc_y + 164, (cc_w - 26) / 2, 48, 8, Color::argb(60, 255, 255, 255));
        draw_char(canvas, dm_x + 12, cc_y + 180, 'D', Color::ACCENT_PURPLE);
        draw_string(canvas, dm_x + 30, cc_y + 174, "Appearance", Color::WHITE);
        draw_string(canvas, dm_x + 30, cc_y + 190, "Dark Glass", Color::TEXT_MUTED);
    }

    fn render_cosmic_menu(&self, canvas: &mut Canvas, mouse_x: i32, mouse_y: i32) {
        let menu_x = 8;
        let menu_y = TOPBAR_HEIGHT + 2;
        let menu_w = 210;
        let menu_h = 284;

        canvas.draw_shadow(menu_x, menu_y, menu_w, menu_h, 8, 90);
        canvas.fill_rounded_rect(menu_x, menu_y, menu_w, menu_h, 8, Color::argb(235, 30, 32, 44));
        canvas.draw_rounded_rect(menu_x, menu_y, menu_w, menu_h, 8, Color::argb(80, 255, 255, 255));

        let items = [
            "About SublimeOS",
            "System Settings...",
            "Activity Monitor",
            "Terminal",
            "Calculator",
            "TextEdit",
            "Exit to Sublime Shell",
            "Restart...",
            "Shut Down...",
        ];

        let item_h = 28;
        let mut iy = menu_y + 6;

        for (idx, &item) in items.iter().enumerate() {
            let is_hover = mouse_x >= menu_x + 4
                && mouse_x <= menu_x + menu_w - 4
                && mouse_y >= iy
                && mouse_y <= iy + item_h;

            if is_hover {
                canvas.fill_rounded_rect(menu_x + 4, iy, menu_w - 8, item_h, 4, Color::ACCENT_BLUE);
            }

            draw_string(
                canvas,
                menu_x + 16,
                iy + 6,
                item,
                if is_hover { Color::WHITE } else { Color::TEXT_PRIMARY },
            );

            if idx == 1 || idx == 5 {
                iy += item_h + 4;
                canvas.fill_rect(menu_x + 10, iy - 2, menu_w - 20, 1, Color::WINDOW_BORDER);
            } else {
                iy += item_h;
            }
        }
    }

    /// Process mouse click on Menu Bar or Dock.
    pub fn handle_click(
        &mut self,
        x: i32,
        y: i32,
        _tabs: &[TaskbarTab],
        sw: i32,
        sh: i32,
    ) -> TaskbarAction {
        // 1. Check Dropdown Menu
        if self.cosmic_menu_open {
            let menu_x = 8;
            let menu_y = TOPBAR_HEIGHT + 2;
            let menu_w = 210;
            let menu_h = 284;

            if x >= menu_x && x <= menu_x + menu_w && y >= menu_y && y <= menu_y + menu_h {
                let item_h = 28;
                let mut iy = menu_y + 6;

                let actions = [
                    TaskbarAction::LaunchApp(DesktopAppId::About),
                    TaskbarAction::LaunchApp(DesktopAppId::Settings),
                    TaskbarAction::LaunchApp(DesktopAppId::Sysmon),
                    TaskbarAction::LaunchApp(DesktopAppId::Terminal),
                    TaskbarAction::LaunchApp(DesktopAppId::Calculator),
                    TaskbarAction::LaunchApp(DesktopAppId::TextEdit),
                    TaskbarAction::ExitToShell,
                    TaskbarAction::RebootSystem,
                    TaskbarAction::HaltSystem,
                ];

                for (idx, action) in actions.into_iter().enumerate() {
                    if x >= menu_x + 4 && x <= menu_x + menu_w - 4 && y >= iy && y <= iy + item_h {
                        self.cosmic_menu_open = false;
                        return action;
                    }

                    if idx == 1 || idx == 5 {
                        iy += item_h + 4;
                    } else {
                        iy += item_h;
                    }
                }
                return TaskbarAction::None;
            } else {
                self.cosmic_menu_open = false;
            }
        }

        // 2. Check Control Center click
        if self.control_center_open {
            let cc_w = 300;
            let cc_h = 224;
            let cc_x = sw - cc_w - 12;
            let cc_y = TOPBAR_HEIGHT + 6;

            if x >= cc_x && x <= cc_x + cc_w && y >= cc_y && y <= cc_y + cc_h {
                // Check sound slider click
                if y >= cc_y + 126 && y <= cc_y + 152 && x >= cc_x + 20 && x <= cc_x + cc_w - 20 {
                    let slider_w = cc_w - 40;
                    let ratio = (x - (cc_x + 20)) as f32 / slider_w as f32;
                    self.volume_level = ((ratio * 100.0) as u8).clamp(0, 100);
                }
                return TaskbarAction::None;
            } else {
                self.control_center_open = false;
            }
        }

        // 3. Check Top Emblem Icon
        if x >= 8 && x <= 32 && y <= TOPBAR_HEIGHT {
            self.cosmic_menu_open = !self.cosmic_menu_open;
            return TaskbarAction::ToggleStartMenu;
        }

        // 4. Check Control Center [CC] Button
        if x >= sw - 199 && x <= sw - 155 && y <= TOPBAR_HEIGHT {
            self.control_center_open = !self.control_center_open;
            return TaskbarAction::None;
        }

        // 5. Check Spotlight Button 🔍
        if x >= sw - 149 && x <= sw - 110 && y <= TOPBAR_HEIGHT {
            return TaskbarAction::ToggleSpotlight;
        }

        // 6. Check Bottom Floating Dock
        let min_tabs: alloc::vec::Vec<&TaskbarTab> = _tabs.iter().filter(|t| t.is_minimized).collect();
        let dock_w = 500 + (min_tabs.len() as i32 * 48);
        let dock_h = 52;
        let dock_x = (sw - dock_w) / 2;
        let dock_y = sh - dock_h - 10;

        if x >= dock_x && x <= dock_x + dock_w && y >= dock_y && y <= dock_y + dock_h {
            let apps = [
                DesktopAppId::FileManager,
                DesktopAppId::Terminal,
                DesktopAppId::TextEdit,
                DesktopAppId::Calculator,
                DesktopAppId::Sysmon,
                DesktopAppId::Settings,
                DesktopAppId::About,
            ];

            let mut app_x = dock_x + 16;
            for app_id in apps.into_iter() {
                if x >= app_x && x <= app_x + 40 {
                    return TaskbarAction::LaunchApp(app_id);
                }
                app_x += 52;
            }

            // Skip divider
            app_x += 16;

            // Check Minimized Window restore clicks
            for min_tab in min_tabs.iter() {
                if x >= app_x && x <= app_x + 40 {
                    return TaskbarAction::RestoreWindow(min_tab.window_id);
                }
                app_x += 48;
            }
        }

        TaskbarAction::None
    }
}

fn push_2digits(s: &mut String, val: u8) {
    s.push(((val / 10) % 10 + b'0') as char);
    s.push((val % 10 + b'0') as char);
}

fn push_u64(s: &mut String, mut val: u64) {
    if val == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut idx = 0;
    while val > 0 {
        buf[idx] = ((val % 10) as u8) + b'0';
        val /= 10;
        idx += 1;
    }
    while idx > 0 {
        idx -= 1;
        s.push(buf[idx] as char);
    }
}
