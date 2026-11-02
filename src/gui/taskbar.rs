use alloc::string::String;
use crate::arch::x86_64::rtc::read_datetime;
use crate::gui::canvas::Canvas;
use crate::gui::color::Color;
use crate::gui::desktop::DesktopAppId;
use crate::gui::font::{draw_char, draw_string};
use crate::mm::pmm::PMM;

pub const TASKBAR_HEIGHT: i32 = 36;

pub enum TaskbarAction {
    None,
    ToggleStartMenu,
    LaunchApp(DesktopAppId),
    FocusWindow(u32),
    ExitToShell,
    HaltSystem,
}

pub struct TaskbarTab {
    pub window_id: u32,
    pub title: String,
    pub is_active: bool,
    pub is_minimized: bool,
}

pub struct Taskbar {
    pub start_menu_open: bool,
}

impl Taskbar {
    pub fn new() -> Self {
        Self {
            start_menu_open: false,
        }
    }

    /// Render taskbar, tabs, system tray, and start menu if open.
    pub fn render(
        &self,
        canvas: &mut Canvas,
        tabs: &[TaskbarTab],
        mouse_x: i32,
        mouse_y: i32,
    ) {
        let sw = canvas.width as i32;
        let sh = canvas.height as i32;
        let ty = sh - TASKBAR_HEIGHT;

        // 1. Taskbar Bar Background
        canvas.fill_rect(0, ty, sw, TASKBAR_HEIGHT, Color::TASKBAR_BG);
        canvas.fill_rect(0, ty, sw, 1, Color::TASKBAR_BORDER);

        // 2. Start Button
        let start_w = 104;
        let start_h = 26;
        let start_x = 6;
        let start_y = ty + 5;
        let start_hover = mouse_x >= start_x
            && mouse_x <= start_x + start_w
            && mouse_y >= start_y
            && mouse_y <= start_y + start_h;

        let start_col = if self.start_menu_open {
            Color::START_BTN_ACTIVE
        } else if start_hover {
            Color::lerp(Color::START_BTN_NORMAL, Color::WHITE, 30)
        } else {
            Color::START_BTN_NORMAL
        };

        canvas.fill_rounded_rect(start_x, start_y, start_w, start_h, 5, start_col);
        canvas.draw_rounded_rect(start_x, start_y, start_w, start_h, 5, Color::ACCENT_PURPLE);
        draw_string(
            canvas,
            start_x + 10,
            start_y + 5,
            "✦ Cosmos",
            Color::WHITE,
        );

        // 3. Window Tabs
        let mut tab_x = start_x + start_w + 10;
        let tab_w = 140;
        let tab_h = 26;
        let tab_y = ty + 5;

        for tab in tabs.iter() {
            if tab_x + tab_w > sw - 220 {
                break; // Stop before overflowing into system tray
            }

            let tab_hover = mouse_x >= tab_x
                && mouse_x <= tab_x + tab_w
                && mouse_y >= tab_y
                && mouse_y <= tab_y + tab_h;

            let tab_bg = if tab.is_active && !tab.is_minimized {
                Color::TASKBAR_TAB_ACTIVE
            } else if tab_hover {
                Color::lerp(Color::TASKBAR_TAB, Color::WHITE, 25)
            } else {
                Color::TASKBAR_TAB
            };

            canvas.fill_rounded_rect(tab_x, tab_y, tab_w, tab_h, 4, tab_bg);
            canvas.draw_rounded_rect(
                tab_x,
                tab_y,
                tab_w,
                tab_h,
                4,
                if tab.is_active {
                    Color::ACCENT_CYAN
                } else {
                    Color::WINDOW_BORDER
                },
            );

            // Active indicator pip
            if tab.is_active {
                canvas.fill_rect(tab_x + 6, tab_y + 11, 4, 4, Color::ACCENT_CYAN);
            }

            // Truncate title if needed
            let max_chars = 13;
            let display_title = if tab.title.len() > max_chars {
                let mut truncated = String::from(&tab.title[..max_chars.min(tab.title.len())]);
                truncated.push_str("..");
                truncated
            } else {
                tab.title.clone()
            };

            draw_string(
                canvas,
                tab_x + 16,
                tab_y + 5,
                &display_title,
                if tab.is_active {
                    Color::TEXT_PRIMARY
                } else {
                    Color::TEXT_MUTED
                },
            );

            tab_x += tab_w + 6;
        }

        // 4. System Tray (Right side: Memory + Clock)
        // Memory meter
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

        let mem_x = sw - 210;
        canvas.fill_rounded_rect(mem_x, ty + 6, 100, 24, 4, Color::BG_CARD);
        canvas.draw_rounded_rect(mem_x, ty + 6, 100, 24, 4, Color::WINDOW_BORDER);
        draw_string(canvas, mem_x + 8, ty + 10, &mem_str, Color::ACCENT_GREEN);

        // RTC Clock
        let time = read_datetime();
        let mut clock_str = String::new();
        push_2digits(&mut clock_str, time.hour);
        clock_str.push(':');
        push_2digits(&mut clock_str, time.minute);
        clock_str.push(':');
        push_2digits(&mut clock_str, time.second);

        let clock_x = sw - 98;
        canvas.fill_rounded_rect(clock_x, ty + 6, 90, 24, 4, Color::BG_CARD);
        canvas.draw_rounded_rect(clock_x, ty + 6, 90, 24, 4, Color::WINDOW_BORDER);
        draw_string(canvas, clock_x + 12, ty + 10, &clock_str, Color::TEXT_PRIMARY);

        // 5. Start Menu (if open)
        if self.start_menu_open {
            self.render_start_menu(canvas, mouse_x, mouse_y);
        }
    }

    fn render_start_menu(&self, canvas: &mut Canvas, mouse_x: i32, mouse_y: i32) {
        let sh = canvas.height as i32;
        let menu_x = 6;
        let menu_w = 224;
        let menu_h = 246;
        let menu_y = sh - TASKBAR_HEIGHT - menu_h - 4;

        // Shadow & Body
        canvas.draw_shadow(menu_x, menu_y, menu_w, menu_h, 6, 90);
        canvas.fill_rounded_rect(menu_x, menu_y, menu_w, menu_h, 8, Color::BG_CARD);
        canvas.draw_rounded_rect(menu_x, menu_y, menu_w, menu_h, 8, Color::WINDOW_BORDER_ACTIVE);

        // Header
        canvas.draw_gradient_h(
            menu_x + 1,
            menu_y + 1,
            menu_w - 2,
            36,
            Color::TITLEBAR_ACTIVE_START,
            Color::TITLEBAR_ACTIVE_END,
        );
        draw_string(
            canvas,
            menu_x + 12,
            menu_y + 10,
            "✦ Cosmos OS 0.1.0",
            Color::WHITE,
        );

        // Items
        let items: [(&str, char, Color); 6] = [
            ("Terminal (CosmosSH)", '>', Color::ACCENT_GREEN),
            ("System Monitor", '#', Color::ACCENT_CYAN),
            ("File Explorer", '@', Color::ACCENT_YELLOW),
            ("About Cosmos", '✦', Color::ACCENT_PURPLE),
            ("Exit to Shell", '-', Color::TEXT_PRIMARY),
            ("Halt System", 'x', Color::ACCENT_RED),
        ];

        let item_h = 32;
        let mut item_y = menu_y + 44;

        for (idx, &(title, icon_ch, icon_col)) in items.iter().enumerate() {
            let is_hover = mouse_x >= menu_x + 4
                && mouse_x <= menu_x + menu_w - 4
                && mouse_y >= item_y
                && mouse_y <= item_y + item_h;

            if is_hover {
                canvas.fill_rounded_rect(
                    menu_x + 4,
                    item_y,
                    menu_w - 8,
                    item_h,
                    4,
                    Color::argb(60, 255, 255, 255),
                );
            }

            draw_char(canvas, menu_x + 12, item_y + 8, icon_ch, icon_col);
            draw_string(canvas, menu_x + 32, item_y + 8, title, Color::TEXT_PRIMARY);

            // Divider before exit options
            if idx == 3 {
                item_y += item_h + 2;
                canvas.fill_rect(menu_x + 8, item_y - 2, menu_w - 16, 1, Color::WINDOW_BORDER);
            } else {
                item_y += item_h;
            }
        }
    }

    /// Process mouse click on taskbar / start menu.
    pub fn handle_click(
        &mut self,
        x: i32,
        y: i32,
        tabs: &[TaskbarTab],
        sw: i32,
        sh: i32,
    ) -> TaskbarAction {
        let ty = sh - TASKBAR_HEIGHT;

        // Check Start Menu items if open
        if self.start_menu_open {
            let menu_x = 6;
            let menu_w = 224;
            let menu_h = 246;
            let menu_y = sh - TASKBAR_HEIGHT - menu_h - 4;

            if x >= menu_x && x <= menu_x + menu_w && y >= menu_y && y <= menu_y + menu_h {
                let item_h = 32;
                let mut iy = menu_y + 44;

                let actions = [
                    TaskbarAction::LaunchApp(DesktopAppId::Terminal),
                    TaskbarAction::LaunchApp(DesktopAppId::Sysmon),
                    TaskbarAction::LaunchApp(DesktopAppId::FileManager),
                    TaskbarAction::LaunchApp(DesktopAppId::About),
                    TaskbarAction::ExitToShell,
                    TaskbarAction::HaltSystem,
                ];

                for (idx, action) in actions.into_iter().enumerate() {
                    if x >= menu_x + 4 && x <= menu_x + menu_w - 4 && y >= iy && y <= iy + item_h {
                        self.start_menu_open = false;
                        return action;
                    }

                    if idx == 3 {
                        iy += item_h + 2;
                    } else {
                        iy += item_h;
                    }
                }
                return TaskbarAction::None;
            } else {
                // Click outside closes start menu
                self.start_menu_open = false;
            }
        }

        // Check Start Button
        let start_x = 6;
        let start_w = 104;
        let start_y = ty + 5;
        let start_h = 26;

        if x >= start_x && x <= start_x + start_w && y >= start_y && y <= start_y + start_h {
            self.start_menu_open = !self.start_menu_open;
            return TaskbarAction::ToggleStartMenu;
        }

        // Check Window Tabs
        let mut tab_x = start_x + start_w + 10;
        let tab_w = 140;
        let tab_h = 26;
        let tab_y = ty + 5;

        for tab in tabs.iter() {
            if tab_x + tab_w > sw - 220 {
                break;
            }
            if x >= tab_x && x <= tab_x + tab_w && y >= tab_y && y <= tab_y + tab_h {
                return TaskbarAction::FocusWindow(tab.window_id);
            }
            tab_x += tab_w + 6;
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
