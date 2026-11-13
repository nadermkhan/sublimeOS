use alloc::string::String;
use crate::arch::x86_64::rtc::read_datetime;
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
    LaunchApp(DesktopAppId),
    FocusWindow(u32),
    ExitToShell,
    RebootSystem,
    HaltSystem,
}

pub struct TaskbarTab {
    pub window_id: u32,
    pub title: String,
    pub is_active: bool,
    pub is_minimized: bool,
}

pub struct Taskbar {
    pub cosmic_menu_open: bool,
}

impl Taskbar {
    pub fn new() -> Self {
        Self {
            cosmic_menu_open: false,
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
        canvas.fill_rect(0, 0, sw, TOPBAR_HEIGHT, Color::argb(225, 26, 27, 36));
        canvas.fill_rect(0, TOPBAR_HEIGHT - 1, sw, 1, Color::argb(60, 255, 255, 255));

        // Sublime Emblem Logo ✦
        let emblem_hover = mouse_x >= 8 && mouse_x <= 32 && mouse_y <= TOPBAR_HEIGHT;
        if self.cosmic_menu_open || emblem_hover {
            canvas.fill_rounded_rect(8, 2, 24, 22, 4, Color::argb(60, 255, 255, 255));
        }
        draw_char(canvas, 16, 5, '✦', Color::WHITE);

        // Active App Title & Standard Menus
        let mut menu_x = 40;
        let menus = ["Finder", "File", "Edit", "View", "Go", "Window", "Help"];
        for (i, &item) in menus.iter().enumerate() {
            let item_w = (item.len() as i32 * 8) + 16;
            let item_hover = mouse_x >= menu_x && mouse_x <= menu_x + item_w && mouse_y <= TOPBAR_HEIGHT;
            if item_hover {
                canvas.fill_rounded_rect(menu_x, 2, item_w, 22, 4, Color::argb(40, 255, 255, 255));
            }

            draw_string(
                canvas,
                menu_x + 8,
                5,
                item,
                if i == 0 { Color::WHITE } else { Color::TEXT_PRIMARY },
            );
            menu_x += item_w;
        }

        // Right Menu Extras (RAM, Battery, Control Center, Clock)
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

        let mem_x = sw - 270;
        draw_string(canvas, mem_x, 5, &mem_str, Color::ACCENT_GREEN);

        // Control Center & Battery
        draw_string(canvas, sw - 165, 5, "[100%]", Color::TEXT_PRIMARY);

        // RTC Clock
        let time = read_datetime();
        let mut clock_str = String::new();
        push_2digits(&mut clock_str, time.hour);
        clock_str.push(':');
        push_2digits(&mut clock_str, time.minute);
        clock_str.push(':');
        push_2digits(&mut clock_str, time.second);
        draw_string(canvas, sw - 86, 5, &clock_str, Color::WHITE);

        // ==========================================
        // 2. Floating Frosted Glass Dock (Bottom)
        // ==========================================
        let dock_w = 448;
        let dock_h = 52;
        let dock_x = (sw - dock_w) / 2;
        let dock_y = sh - dock_h - 10;

        // Frosted Glass Dock Base
        canvas.draw_shadow(dock_x, dock_y, dock_w, dock_h, 8, 80);
        canvas.fill_rounded_rect(dock_x, dock_y, dock_w, dock_h, 16, Color::argb(210, 28, 30, 42));
        canvas.draw_rounded_rect(dock_x, dock_y, dock_w, dock_h, 16, Color::argb(60, 255, 255, 255));

        // Dock App Items
        let dock_apps: [(DesktopAppId, &str, char, Color); 6] = [
            (DesktopAppId::FileManager, "Finder", '@', Color::ACCENT_CYAN),
            (DesktopAppId::Terminal, "Terminal", '>', Color::ACCENT_GREEN),
            (DesktopAppId::TextEdit, "TextEdit", 'T', Color::WHITE),
            (DesktopAppId::Calculator, "Calculator", '+', Color::ACCENT_ORANGE),
            (DesktopAppId::Sysmon, "Activity", '#', Color::ACCENT_YELLOW),
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
            let is_running = tabs.iter().any(|t| match app_id {
                DesktopAppId::Terminal => t.title.contains("Terminal"),
                DesktopAppId::Sysmon => t.title.contains("Activity"),
                DesktopAppId::FileManager => t.title.contains("Finder"),
                DesktopAppId::About => t.title.contains("About"),
                DesktopAppId::Calculator => t.title.contains("Calculator"),
                DesktopAppId::TextEdit => t.title.contains("TextEdit"),
            });

            if is_running {
                canvas.fill_circle(app_x + 18, dock_y + 46, 2, Color::ACCENT_CYAN);
            }

            app_x += 52;
        }

        // Dock Divider
        canvas.fill_rect(app_x, dock_y + 10, 1, 32, Color::argb(80, 255, 255, 255));
        app_x += 16;

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
    }

    fn render_cosmic_menu(&self, canvas: &mut Canvas, mouse_x: i32, mouse_y: i32) {
        let menu_x = 8;
        let menu_y = TOPBAR_HEIGHT + 2;
        let menu_w = 210;
        let menu_h = 256;

        canvas.draw_shadow(menu_x, menu_y, menu_w, menu_h, 8, 90);
        canvas.fill_rounded_rect(menu_x, menu_y, menu_w, menu_h, 8, Color::argb(235, 30, 32, 44));
        canvas.draw_rounded_rect(menu_x, menu_y, menu_w, menu_h, 8, Color::argb(80, 255, 255, 255));

        let items = [
            "About SublimeOS",
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

            if idx == 0 || idx == 4 {
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
            let menu_h = 256;

            if x >= menu_x && x <= menu_x + menu_w && y >= menu_y && y <= menu_y + menu_h {
                let item_h = 28;
                let mut iy = menu_y + 6;

                let actions = [
                    TaskbarAction::LaunchApp(DesktopAppId::About),
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

                    if idx == 0 || idx == 4 {
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

        // 2. Check Top Emblem Icon
        if x >= 8 && x <= 32 && y <= TOPBAR_HEIGHT {
            self.cosmic_menu_open = !self.cosmic_menu_open;
            return TaskbarAction::ToggleStartMenu;
        }

        // 3. Check Bottom Floating Dock
        let dock_w = 448;
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
                DesktopAppId::About,
            ];

            let mut app_x = dock_x + 16;
            for app_id in apps.into_iter() {
                if x >= app_x && x <= app_x + 40 {
                    return TaskbarAction::LaunchApp(app_id);
                }
                app_x += 52;
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
