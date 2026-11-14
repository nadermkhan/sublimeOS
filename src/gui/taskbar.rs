use alloc::format;
use alloc::string::String;
use crate::arch::x86_64::rtc::get_system_datetime;
use crate::gui::canvas::Canvas;
use crate::gui::color::Color;
use crate::gui::desktop::DesktopAppId;
use crate::gui::font::{draw_string, draw_string_bold, draw_string_ellipsis, draw_string_shadow};
use crate::gui::window::{draw_mini_window_icon, Window};
use crate::mm::pmm::PMM;
use crate::net::device::NET_MANAGER;
use crate::net::wifi::WIFI_MANAGER;

pub const TASKBAR_HEIGHT: i32 = 30;

pub enum TaskbarAction {
    None,
    ToggleStartMenu,
    ToggleSpotlight,
    LaunchApp(DesktopAppId),
    OpenNetworkSettings,
    OpenDisplaySettings,
    FocusWindow(u32),
    RestoreWindow(u32),
    MinimizeAll,
    ExitToShell,
    RebootSystem,
    HaltSystem,
}

pub struct Taskbar {
    pub start_menu_open: bool,
    pub tray_popup_open: bool,
    pub volume_level: u8,
}

impl Taskbar {
    pub fn new() -> Self {
        Self {
            start_menu_open: false,
            tray_popup_open: false,
            volume_level: 85,
        }
    }

    /// Render SublimeOS Classic Bottom Taskbar, Start Button, Task Buttons, Tray, and Start Menu.
    pub fn render(
        &self,
        canvas: &mut Canvas,
        windows: &[Window],
        mouse_x: i32,
        mouse_y: i32,
    ) {
        let sw = canvas.width as i32;
        let sh = canvas.height as i32;
        let ty = sh - TASKBAR_HEIGHT;

        // ==========================================
        // 1. Classic Blue Taskbar Strip
        // ==========================================
        canvas.fill_rect(0, ty, sw, 1, Color::XP_TASKBAR_HILITE);
        canvas.draw_gradient_v(0, ty + 1, sw, 4, Color::XP_TASKBAR_HILITE, Color::XP_TASKBAR_TOP);
        canvas.draw_gradient_v(0, ty + 5, sw, TASKBAR_HEIGHT - 5, Color::XP_TASKBAR_MID, Color::XP_TASKBAR_BOT);

        // ==========================================
        // 2. Classic Green "Start" Button (Bottom-Left)
        // ==========================================
        let start_w = 100;
        let start_hover = mouse_x >= 0 && mouse_x < start_w && mouse_y >= ty;
        let (s_top, s_mid, s_bot) = if self.start_menu_open || start_hover {
            (Color::XP_START_HOVER_TOP, Color::XP_START_HOVER_MID, Color::XP_START_MID)
        } else {
            (Color::XP_START_TOP, Color::XP_START_MID, Color::XP_START_BOT)
        };

        canvas.draw_gradient_v(0, ty + 1, start_w - 2, 5, s_top, s_mid);
        canvas.draw_gradient_v(0, ty + 6, start_w - 2, TASKBAR_HEIGHT - 6, s_mid, s_bot);
        canvas.fill_rect(start_w - 3, ty + 2, 2, TASKBAR_HEIGHT - 4, s_bot);
        canvas.fill_rect(start_w - 1, ty + 4, 1, TASKBAR_HEIGHT - 8, Color::rgb(22, 86, 18));

        // Crisp SublimeOS Diamond Prism emblem
        let fx = 11;
        let fy = ty + 7;
        canvas.fill_rounded_rect(fx, fy, 15, 15, 4, Color::rgb(20, 80, 32));
        canvas.fill_rect(fx + 3, fy + 3, 4, 4, Color::rgb(130, 232, 255));
        canvas.fill_rect(fx + 8, fy + 3, 4, 4, Color::WHITE);
        canvas.fill_rect(fx + 3, fy + 8, 4, 4, Color::rgb(155, 255, 175));
        canvas.fill_rect(fx + 8, fy + 8, 4, 4, Color::rgb(255, 214, 102));

        draw_string_shadow(canvas, 33, ty + 7, "Start", Color::WHITE, Color::rgb(20, 72, 16));

        // ==========================================
        // 3. Quick Launch Icons (Show Desktop, Cmd, Explorer)
        // ==========================================
        let ql_x = start_w + 8;
        let quick_items = [
            (ql_x, "D", Color::rgb(125, 200, 255)),      // Show Desktop
            (ql_x + 24, ">", Color::rgb(230, 230, 230)), // Command Prompt
            (ql_x + 48, "E", Color::rgb(255, 214, 88)),  // Explorer
        ];
        for (idx, (qx, _, _)) in quick_items.iter().enumerate() {
            let qh = mouse_x >= *qx && mouse_x < *qx + 20 && mouse_y >= ty + 4 && mouse_y < ty + 26;
            if qh {
                canvas.fill_rounded_rect(*qx, ty + 4, 20, 22, 3, Color::argb(70, 255, 255, 255));
                canvas.draw_rounded_rect(*qx, ty + 4, 20, 22, 3, Color::argb(130, 255, 255, 255));
            }
            match idx {
                0 => {
                    // Show Desktop blotter/pen mini icon
                    canvas.fill_rect(*qx + 3, ty + 7, 14, 14, Color::rgb(218, 232, 248));
                    canvas.fill_rect(*qx + 3, ty + 7, 14, 4, Color::rgb(42, 118, 224));
                    canvas.draw_rect(*qx + 3, ty + 7, 14, 14, Color::rgb(45, 75, 125));
                }
                1 => draw_mini_window_icon(canvas, *qx + 3, ty + 8, DesktopAppId::Terminal),
                _ => draw_mini_window_icon(canvas, *qx + 3, ty + 8, DesktopAppId::FileManager),
            }
        }
        // Subtle separator bar after Quick Launch
        let sep_x = ql_x + 74;
        canvas.fill_rect(sep_x, ty + 6, 1, TASKBAR_HEIGHT - 12, Color::rgb(16, 48, 128));
        canvas.fill_rect(sep_x + 1, ty + 6, 1, TASKBAR_HEIGHT - 12, Color::rgb(82, 142, 238));

        // ==========================================
        // 4. Open Window Taskbar Buttons
        // ==========================================
        let tray_w = 204;
        let tray_x = sw - tray_w;
        let tabs_start_x = sep_x + 8;
        let tabs_avail_w = (tray_x - tabs_start_x - 8).max(80);

        if !windows.is_empty() {
            let count = windows.len() as i32;
            let tab_w = ((tabs_avail_w / count) - 4).clamp(70, 165);
            let mut tx = tabs_start_x;

            for w in windows.iter() {
                if tx + tab_w > tray_x - 4 {
                    break;
                }

                let is_pressed = w.is_active && !w.is_minimized;
                let is_hover = mouse_x >= tx && mouse_x < tx + tab_w && mouse_y >= ty + 3 && mouse_y < ty + 27;

                if is_pressed {
                    canvas.fill_rounded_rect(tx, ty + 3, tab_w, 24, 3, Color::XP_TAB_ACTIVE);
                    canvas.draw_rounded_rect(tx, ty + 3, tab_w, 24, 3, Color::rgb(12, 42, 115));
                    canvas.fill_rect(tx + 1, ty + 4, tab_w - 2, 1, Color::rgb(16, 46, 124));
                } else if is_hover {
                    canvas.draw_gradient_v(tx, ty + 3, tab_w, 24, Color::rgb(92, 154, 255), Color::XP_TAB_TOP);
                    canvas.draw_rounded_rect(tx, ty + 3, tab_w, 24, 3, Color::rgb(26, 78, 180));
                } else {
                    canvas.draw_gradient_v(tx, ty + 3, tab_w, 24, Color::XP_TAB_TOP, Color::XP_TAB_BOT);
                    canvas.draw_rounded_rect(tx, ty + 3, tab_w, 24, 3, Color::rgb(20, 62, 158));
                }

                draw_mini_window_icon(canvas, tx + 6, ty + 8, w.app.app_id());
                draw_string_ellipsis(canvas, tx + 24, ty + 7, &w.title, Color::WHITE, tab_w - 28);

                tx += tab_w + 4;
            }
        }

        // ==========================================
        // 5. Notification Area (Wi-Fi + Audio + MEM + RTC Clock)
        // ==========================================
        canvas.draw_gradient_v(tray_x, ty + 1, tray_w, TASKBAR_HEIGHT - 1, Color::XP_TRAY_TOP, Color::XP_TRAY_BOT);
        canvas.fill_rect(tray_x, ty + 1, 1, TASKBAR_HEIGHT - 1, Color::XP_TRAY_BORDER);

        // Live Wi-Fi 4-bar signal indicator
        let (wifi_conn, wifi_bars): (bool, u8) = {
            let wifi = WIFI_MANAGER.lock();
            if wifi.is_connected {
                let bars = wifi
                    .scan_results
                    .iter()
                    .find(|ap| Some(&ap.ssid) == wifi.current_ssid.as_ref())
                    .map(|ap| {
                        if ap.rssi >= -50 { 4 } else if ap.rssi >= -65 { 3 } else if ap.rssi >= -75 { 2 } else { 1 }
                    })
                    .unwrap_or(3);
                (true, bars)
            } else {
                (false, 0)
            }
        };
        let wx = tray_x + 10;
        for b in 0..4i32 {
            let bh = 3 + b * 3;
            let bar_col = if wifi_conn && (b as u8) < wifi_bars {
                Color::rgb(110, 255, 145)
            } else if !wifi_conn && b == 0 {
                Color::rgb(255, 210, 95)
            } else {
                Color::rgb(70, 140, 200)
            };
            canvas.fill_rect(wx + b * 4, ty + 20 - bh, 3, bh, bar_col);
        }

        // Mini Volume speaker icon
        let vx = tray_x + 31;
        canvas.fill_rect(vx, ty + 12, 3, 6, Color::rgb(225, 242, 255));
        canvas.fill_rect(vx + 3, ty + 10, 3, 10, Color::rgb(225, 242, 255));
        if self.volume_level > 0 {
            canvas.fill_rect(vx + 8, ty + 11, 1, 8, Color::rgb(130, 235, 255));
        }

        // RAM % badge
        let (used_mb, total_mb) = {
            let pmm = PMM.lock();
            let used = (pmm.get_used_frames() * 4) / 1024;
            let total = (pmm.get_total_frames() * 4) / 1024;
            (used, total)
        };
        let ram_pct = if total_mb > 0 { (used_mb * 100) / total_mb } else { 0 };
        let mut ram_str = String::from("M:");
        push_u64(&mut ram_str, ram_pct as u64);
        ram_str.push('%');
        draw_string(canvas, tray_x + 48, ty + 7, &ram_str, Color::rgb(220, 242, 255));

        // RTC Clock
        let time = get_system_datetime();
        let mut clock_str = String::new();
        push_2digits(&mut clock_str, time.hour);
        clock_str.push(':');
        push_2digits(&mut clock_str, time.minute);
        clock_str.push(':');
        push_2digits(&mut clock_str, time.second);
        draw_string_shadow(canvas, sw - 74, ty + 7, &clock_str, Color::WHITE, Color::rgb(8, 68, 128));

        // ==========================================
        // 6. SublimeOS Two-Column Start Menu (if open)
        // ==========================================
        if self.start_menu_open {
            self.render_start_menu(canvas, sh, mouse_x, mouse_y);
        }

        // ==========================================
        // 7. System Tray Telemetry Popup (if open)
        // ==========================================
        if self.tray_popup_open {
            self.render_tray_popup(canvas, sw, sh, used_mb, total_mb);
        }
    }

    fn render_start_menu(&self, canvas: &mut Canvas, sh: i32, mouse_x: i32, mouse_y: i32) {
        let menu_w = 376;
        let menu_h = 378;
        let menu_x = 0;
        let menu_y = sh - TASKBAR_HEIGHT - menu_h;

        // Drop shadow + Blue outer frame
        canvas.fill_rect(menu_x + 4, menu_y + 4, menu_w, menu_h, Color::argb(65, 0, 0, 0));
        canvas.fill_rect(menu_x, menu_y, menu_w, menu_h, Color::XP_FRAME_ACTIVE);
        canvas.draw_rect(menu_x, menu_y, menu_w, menu_h, Color::XP_FRAME_OUTER);

        // Top User Header (54px)
        let hdr_h = 54;
        canvas.draw_gradient_v(menu_x + 2, menu_y + 2, menu_w - 4, hdr_h, Color::XP_TITLE_HIGHLIGHT, Color::XP_TITLE_BOTTOM);
        // Framed 38x38 user avatar tile
        canvas.fill_rounded_rect(menu_x + 10, menu_y + 8, 38, 38, 4, Color::rgb(228, 240, 255));
        canvas.draw_rounded_rect(menu_x + 10, menu_y + 8, 38, 38, 4, Color::WHITE);
        canvas.fill_circle(menu_x + 29, menu_y + 21, 7, Color::rgb(64, 122, 214));
        canvas.fill_rounded_rect(menu_x + 19, menu_y + 29, 20, 12, 4, Color::rgb(64, 122, 214));
        draw_string_shadow(canvas, menu_x + 58, menu_y + 15, "SublimeOS", Color::WHITE, Color::rgb(0, 32, 110));
        draw_string(canvas, menu_x + 58, menu_y + 33, "Administrator (v1.0)", Color::rgb(198, 224, 255));

        // Orange top divider stripe just under header
        canvas.fill_rect(menu_x + 2, menu_y + 2 + hdr_h, menu_w - 4, 2, Color::XP_ORANGE_GLOW);

        let body_y = menu_y + 2 + hdr_h + 2;
        let footer_h = 40;
        let body_h = menu_h - hdr_h - footer_h - 6;
        let left_w = 190;
        let right_w = menu_w - left_w - 4;
        let right_x = menu_x + 2 + left_w;

        // Left Column (White pinned programs)
        canvas.fill_rect(menu_x + 2, body_y, left_w, body_h, Color::WHITE);
        // Right Column (Sky Blue places)
        canvas.fill_rect(right_x, body_y, right_w, body_h, Color::XP_START_PLACES_BG);
        canvas.fill_rect(right_x, body_y, 1, body_h, Color::XP_START_PLACES_DIV);

        let left_items: [(&str, &str, DesktopAppId); 6] = [
            ("Terminal", "SublimeOS Shell", DesktopAppId::Terminal),
            ("My Computer", "Explore Files", DesktopAppId::FileManager),
            ("Notepad", "Text Editor", DesktopAppId::TextEdit),
            ("Calculator", "Arithmetic", DesktopAppId::Calculator),
            ("Task Manager", "Tasks & Network", DesktopAppId::Sysmon),
            ("About SublimeOS", "System Info", DesktopAppId::About),
        ];

        let mut iy = body_y + 8;
        for (title, sub, app_id) in left_items.iter() {
            let row_h = 38;
            let hover = mouse_x >= menu_x + 6
                && mouse_x < right_x - 4
                && mouse_y >= iy
                && mouse_y < iy + row_h;

            if hover {
                canvas.fill_rect(menu_x + 6, iy, left_w - 10, row_h, Color::XP_SELECTION);
            }

            draw_mini_window_icon(canvas, menu_x + 12, iy + 11, *app_id);
            draw_string(
                canvas,
                menu_x + 34,
                iy + 4,
                title,
                if hover { Color::WHITE } else { Color::XP_TEXT_BLACK },
            );
            draw_string(
                canvas,
                menu_x + 34,
                iy + 20,
                sub,
                if hover { Color::rgb(218, 232, 255) } else { Color::XP_TEXT_GRAY },
            );
            iy += row_h + 2;
        }

        // Bottom of Left Column: "All Programs / Run..." bar
        let run_y = body_y + body_h - 32;
        canvas.fill_rect(menu_x + 10, run_y - 4, left_w - 20, 1, Color::rgb(214, 214, 214));
        let run_hover = mouse_x >= menu_x + 6 && mouse_x < right_x - 4 && mouse_y >= run_y && mouse_y < run_y + 26;
        if run_hover {
            canvas.fill_rect(menu_x + 6, run_y, left_w - 10, 26, Color::XP_SELECTION);
        }
        draw_string(
            canvas,
            menu_x + 28,
            run_y + 5,
            "Run Command...  >",
            if run_hover { Color::WHITE } else { Color::XP_TEXT_BLACK },
        );
        canvas.fill_rect(menu_x + 166, run_y + 8, 6, 9, Color::XP_START_MID);

        // Right Column: Classic Places
        let right_items = [
            "Documents",
            "My Computer",
            "Control Panel",
            "Wi-Fi & Network",
            "Display & Fonts",
            "Show Desktop",
        ];
        let mut ry = body_y + 10;
        for (idx, &label) in right_items.iter().enumerate() {
            let row_h = 28;
            let hover = mouse_x >= right_x + 4
                && mouse_x < menu_x + menu_w - 4
                && mouse_y >= ry
                && mouse_y < ry + row_h;

            if hover {
                canvas.fill_rect(right_x + 4, ry, right_w - 8, row_h, Color::XP_SELECTION);
            }

            let dot_col = match idx {
                0 | 1 => Color::rgb(235, 178, 38),
                2 | 3 => Color::rgb(42, 118, 224),
                _ => Color::rgb(65, 160, 75),
            };
            canvas.fill_rect(right_x + 12, ry + 8, 10, 10, dot_col);
            canvas.draw_rect(right_x + 12, ry + 8, 10, 10, Color::rgb(40, 70, 130));

            draw_string(
                canvas,
                right_x + 28,
                ry + 6,
                label,
                if hover { Color::WHITE } else { Color::rgb(0, 32, 96) },
            );

            if idx == 1 || idx == 3 {
                ry += row_h + 7;
                canvas.fill_rect(right_x + 10, ry - 4, right_w - 20, 1, Color::XP_START_PLACES_DIV);
            } else {
                ry += row_h + 2;
            }
        }

        // Bottom Footer (Log Off / Restart / Turn Off Computer)
        let foot_y = menu_y + menu_h - footer_h - 2;
        canvas.draw_gradient_v(menu_x + 2, foot_y, menu_w - 4, footer_h, Color::XP_TASKBAR_TOP, Color::XP_TASKBAR_BOT);

        // [Log Off] -> Exit to SublimeSH
        let log_x = menu_x + 92;
        let log_hover = mouse_x >= log_x && mouse_x < log_x + 92 && mouse_y >= foot_y + 6 && mouse_y < foot_y + 34;
        if log_hover {
            canvas.fill_rounded_rect(log_x, foot_y + 6, 92, 28, 4, Color::argb(60, 255, 255, 255));
        }
        canvas.fill_rounded_rect(log_x + 4, foot_y + 10, 20, 20, 3, Color::XP_ORANGE_GLOW);
        canvas.draw_rounded_rect(log_x + 4, foot_y + 10, 20, 20, 3, Color::WHITE);
        draw_string_shadow(canvas, log_x + 29, foot_y + 12, "Log Off", Color::WHITE, Color::rgb(12, 38, 100));

        // [Restart]
        let rst_x = menu_x + 190;
        let rst_hover = mouse_x >= rst_x && mouse_x < rst_x + 84 && mouse_y >= foot_y + 6 && mouse_y < foot_y + 34;
        if rst_hover {
            canvas.fill_rounded_rect(rst_x, foot_y + 6, 84, 28, 4, Color::argb(60, 255, 255, 255));
        }
        canvas.fill_rounded_rect(rst_x + 4, foot_y + 10, 18, 20, 3, Color::XP_START_MID);
        canvas.draw_rounded_rect(rst_x + 4, foot_y + 10, 18, 20, 3, Color::WHITE);
        draw_string_shadow(canvas, rst_x + 26, foot_y + 12, "Restart", Color::WHITE, Color::rgb(12, 38, 100));

        // [Turn Off]
        let off_x = menu_x + 280;
        let off_hover = mouse_x >= off_x && mouse_x < off_x + 90 && mouse_y >= foot_y + 6 && mouse_y < foot_y + 34;
        if off_hover {
            canvas.fill_rounded_rect(off_x, foot_y + 6, 90, 28, 4, Color::argb(60, 255, 255, 255));
        }
        canvas.fill_rounded_rect(off_x + 4, foot_y + 10, 20, 20, 3, Color::XP_CAP_RED_TOP);
        canvas.draw_rounded_rect(off_x + 4, foot_y + 10, 20, 20, 3, Color::WHITE);
        draw_string_shadow(canvas, off_x + 28, foot_y + 12, "Turn Off", Color::WHITE, Color::rgb(12, 38, 100));
    }

    fn render_tray_popup(&self, canvas: &mut Canvas, sw: i32, sh: i32, used_mb: usize, total_mb: usize) {
        let pw = 276;
        let ph = 186;
        let px = sw - pw - 8;
        let py = sh - TASKBAR_HEIGHT - ph - 6;

        canvas.fill_rect(px + 3, py + 3, pw, ph, Color::argb(55, 0, 0, 0));
        canvas.fill_rect(px, py, pw, ph, Color::XP_FACE);
        canvas.draw_rect(px, py, pw, ph, Color::XP_FRAME_OUTER);
        canvas.draw_gradient_v(px + 1, py + 1, pw - 2, 24, Color::XP_TITLE_TOP, Color::XP_TITLE_BOTTOM);
        draw_string_bold(canvas, px + 8, py + 4, "SublimeOS Status & Wi-Fi", Color::WHITE);

        let (wifi_line, wifi_col) = {
            let wifi = WIFI_MANAGER.lock();
            if wifi.is_connected {
                (
                    format!(
                        "Wi-Fi: {} ({} Mbps)",
                        wifi.current_ssid.as_deref().unwrap_or("Connected"),
                        wifi.link_speed_mbps
                    ),
                    Color::rgb(18, 122, 42),
                )
            } else if !wifi.scan_results.is_empty() {
                (
                    format!("Wi-Fi: Ready ({} APs found)", wifi.scan_results.len()),
                    Color::XP_TEXT_LINK,
                )
            } else {
                (String::from("Wi-Fi: Disconnected (802.11 ready)"), Color::XP_TEXT_GRAY)
            }
        };
        draw_string_ellipsis(canvas, px + 12, py + 32, &wifi_line, wifi_col, pw - 24);

        let nic_line = {
            let net = NET_MANAGER.lock();
            if let Some((dev, cfg)) = net.get_default_device() {
                format!(
                    "NIC: {}  IP {}.{}.{}.{}",
                    dev.name(), cfg.ipv4[0], cfg.ipv4[1], cfg.ipv4[2], cfg.ipv4[3]
                )
            } else {
                String::from("NIC: lo0 127.0.0.1")
            }
        };
        draw_string_ellipsis(canvas, px + 12, py + 52, &nic_line, Color::XP_TEXT_BLACK, pw - 24);

        // Quick action buttons inside Tray Popup: [Scan Wi-Fi] [Network Panel]
        let btn_y = py + 76;
        canvas.draw_gradient_v(px + 12, btn_y, 92, 22, Color::XP_WHITE, Color::XP_FACE_DARK);
        canvas.draw_rounded_rect(px + 12, btn_y, 92, 22, 3, Color::XP_CTRL_BORDER);
        draw_string(canvas, px + 20, btn_y + 3, "Scan Wi-Fi", Color::XP_TEXT_BLACK);

        canvas.draw_gradient_v(px + 112, btn_y, 152, 22, Color::XP_WHITE, Color::XP_FACE_DARK);
        canvas.draw_rounded_rect(px + 112, btn_y, 152, 22, 3, Color::XP_CTRL_BORDER);
        draw_string(canvas, px + 124, btn_y + 3, "Wi-Fi Settings...", Color::XP_TEXT_BLACK);

        let mut mem_str = String::from("Memory: ");
        push_u64(&mut mem_str, used_mb as u64);
        mem_str.push('/');
        push_u64(&mut mem_str, total_mb as u64);
        mem_str.push_str(" MB");
        draw_string_ellipsis(canvas, px + 12, py + 108, &mem_str, Color::XP_TEXT_BLACK, pw - 24);
        draw_string(canvas, px + 12, py + 130, &format!("Master Volume: {}%", self.volume_level), Color::XP_TEXT_BLACK);

        let slider_w = pw - 24;
        let filled_w = ((self.volume_level as i32 * slider_w) / 100).clamp(4, slider_w);
        canvas.fill_rect(px + 12, py + 154, slider_w, 8, Color::WHITE);
        canvas.draw_rect(px + 12, py + 154, slider_w, 8, Color::XP_INPUT_BORDER);
        canvas.fill_rect(px + 12, py + 154, filled_w, 8, Color::XP_SELECTION);
        canvas.fill_rect(px + 10 + filled_w, py + 150, 8, 16, Color::XP_FACE);
        canvas.draw_rect(px + 10 + filled_w, py + 150, 8, 16, Color::XP_CTRL_BORDER);
    }

    /// Process mouse clicks on Start Menu, Tray Popup, Quick Launch, Taskbar Tabs, or Start Button.
    pub fn handle_click(
        &mut self,
        x: i32,
        y: i32,
        windows: &[Window],
        sw: i32,
        sh: i32,
    ) -> TaskbarAction {
        let ty = sh - TASKBAR_HEIGHT;

        // 1. Check Open Start Menu
        if self.start_menu_open {
            let menu_w = 376;
            let menu_h = 378;
            let menu_x = 0;
            let menu_y = sh - TASKBAR_HEIGHT - menu_h;
            let hdr_h = 54;
            let body_y = menu_y + 2 + hdr_h + 2;
            let footer_h = 40;
            let body_h = menu_h - hdr_h - footer_h - 6;
            let left_w = 190;
            let right_x = menu_x + 2 + left_w;

            if x >= menu_x && x <= menu_x + menu_w && y >= menu_y && y <= menu_y + menu_h {
                // Left column pinned programs
                let left_actions = [
                    TaskbarAction::LaunchApp(DesktopAppId::Terminal),
                    TaskbarAction::LaunchApp(DesktopAppId::FileManager),
                    TaskbarAction::LaunchApp(DesktopAppId::TextEdit),
                    TaskbarAction::LaunchApp(DesktopAppId::Calculator),
                    TaskbarAction::LaunchApp(DesktopAppId::Sysmon),
                    TaskbarAction::LaunchApp(DesktopAppId::About),
                ];

                let mut iy = body_y + 8;
                for action in left_actions.into_iter() {
                    let row_h = 38;
                    if x >= menu_x + 6 && x < right_x - 4 && y >= iy && y < iy + row_h {
                        self.start_menu_open = false;
                        return action;
                    }
                    iy += row_h + 2;
                }

                // Bottom-left "Run Command..."
                let run_y = body_y + body_h - 32;
                if x >= menu_x + 6 && x < right_x - 4 && y >= run_y && y < run_y + 26 {
                    self.start_menu_open = false;
                    return TaskbarAction::ToggleSpotlight;
                }

                // Right column Places
                let right_actions = [
                    TaskbarAction::LaunchApp(DesktopAppId::FileManager),
                    TaskbarAction::LaunchApp(DesktopAppId::FileManager),
                    TaskbarAction::LaunchApp(DesktopAppId::Settings),
                    TaskbarAction::OpenNetworkSettings,
                    TaskbarAction::OpenDisplaySettings,
                    TaskbarAction::MinimizeAll,
                ];
                let mut ry = body_y + 10;
                for (idx, action) in right_actions.into_iter().enumerate() {
                    let row_h = 28;
                    if x >= right_x + 4 && x < menu_x + menu_w - 4 && y >= ry && y < ry + row_h {
                        self.start_menu_open = false;
                        return action;
                    }
                    if idx == 1 || idx == 3 {
                        ry += row_h + 7;
                    } else {
                        ry += row_h + 2;
                    }
                }

                // Footer buttons (Log Off, Restart, Turn Off)
                let foot_y = menu_y + menu_h - footer_h - 2;
                if y >= foot_y + 6 && y < foot_y + 34 {
                    if x >= menu_x + 92 && x < menu_x + 184 {
                        self.start_menu_open = false;
                        return TaskbarAction::ExitToShell;
                    }
                    if x >= menu_x + 190 && x < menu_x + 274 {
                        self.start_menu_open = false;
                        return TaskbarAction::RebootSystem;
                    }
                    if x >= menu_x + 280 && x < menu_x + 370 {
                        self.start_menu_open = false;
                        return TaskbarAction::HaltSystem;
                    }
                }
                return TaskbarAction::None;
            } else if !(x < 100 && y >= ty) {
                self.start_menu_open = false;
            }
        }

        // 2. Check Tray Status Popup
        if self.tray_popup_open {
            let pw = 276;
            let ph = 186;
            let px = sw - pw - 8;
            let py = sh - TASKBAR_HEIGHT - ph - 6;

            if x >= px && x <= px + pw && y >= py && y <= py + ph {
                // Quick action buttons
                let btn_y = py + 76;
                if y >= btn_y && y <= btn_y + 22 {
                    if x >= px + 12 && x <= px + 104 {
                        let _ = WIFI_MANAGER.lock().scan();
                        return TaskbarAction::None;
                    }
                    if x >= px + 112 && x <= px + 264 {
                        self.tray_popup_open = false;
                        return TaskbarAction::OpenNetworkSettings;
                    }
                }
                // Master Volume slider
                if y >= py + 146 && y <= py + 172 && x >= px + 12 && x <= px + pw - 12 {
                    let slider_w = pw - 24;
                    let ratio = (x - (px + 12)) as f32 / slider_w as f32;
                    self.volume_level = ((ratio * 100.0) as u8).clamp(0, 100);
                }
                return TaskbarAction::None;
            } else if !(x >= sw - 204 && y >= ty) {
                self.tray_popup_open = false;
            }
        }

        // Ignore clicks outside bottom taskbar strip
        if y < ty {
            return TaskbarAction::None;
        }

        // 3. Green Start Button
        let start_w = 100;
        if x >= 0 && x < start_w {
            self.start_menu_open = !self.start_menu_open;
            return TaskbarAction::ToggleStartMenu;
        }

        // 4. Quick Launch
        let ql_x = start_w + 8;
        if x >= ql_x && x < ql_x + 20 {
            return TaskbarAction::MinimizeAll;
        }
        if x >= ql_x + 24 && x < ql_x + 44 {
            return TaskbarAction::LaunchApp(DesktopAppId::Terminal);
        }
        if x >= ql_x + 48 && x < ql_x + 68 {
            return TaskbarAction::LaunchApp(DesktopAppId::FileManager);
        }

        // 5. Notification Tray click
        let tray_w = 204;
        let tray_x = sw - tray_w;
        if x >= tray_x {
            self.tray_popup_open = !self.tray_popup_open;
            return TaskbarAction::None;
        }

        // 6. Taskbar Window Buttons
        let sep_x = ql_x + 74;
        let tabs_start_x = sep_x + 8;
        let tabs_avail_w = (tray_x - tabs_start_x - 8).max(80);
        if !windows.is_empty() {
            let count = windows.len() as i32;
            let tab_w = ((tabs_avail_w / count) - 4).clamp(70, 165);
            let mut tx = tabs_start_x;

            for w in windows.iter() {
                if tx + tab_w > tray_x - 4 {
                    break;
                }
                if x >= tx && x < tx + tab_w {
                    return TaskbarAction::FocusWindow(w.id);
                }
                tx += tab_w + 4;
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
