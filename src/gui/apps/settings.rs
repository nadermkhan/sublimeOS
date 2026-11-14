use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::drivers::platform::smbios::get_hardware_telemetry;
use crate::drivers::vbe::VBE;
use crate::fs::vfs::VFS;
use crate::gui::canvas::Canvas;
use crate::gui::color::Color;
use crate::gui::font::{
    draw_string, draw_string_bold, draw_string_ellipsis, get_terminal_bold, get_ui_font_style,
    measure_string, set_terminal_bold, set_ui_font_style, ui_font_style_name,
};
use crate::mm::pmm::PMM;
use crate::net::device::NET_MANAGER;
use crate::net::wifi::{WifiAccessPoint, WIFI_MANAGER};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    General,
    Display,
    Sound,
    Network,
    Storage,
}

pub struct SettingsApp {
    pub active_tab: SettingsTab,
    pub audio_volume: u8,
    pub status_feedback: String,
    pub selected_ap: Option<usize>,
    pub wifi_passphrase: String,
    pub pass_focused: bool,
}

impl SettingsApp {
    pub fn new() -> Self {
        Self {
            active_tab: SettingsTab::General,
            audio_volume: 85,
            status_feedback: String::from("SublimeOS Control Panel Ready"),
            selected_ap: None,
            wifi_passphrase: String::new(),
            pass_focused: false,
        }
    }

    pub fn set_tab(&mut self, tab: SettingsTab) {
        self.active_tab = tab;
        self.pass_focused = false;
    }

    pub fn handle_char(&mut self, ch: char) {
        if self.active_tab != SettingsTab::Network || !self.pass_focused {
            return;
        }
        match ch {
            '\x08' => {
                self.wifi_passphrase.pop();
            }
            '\n' | '\r' => {
                self.connect_selected_wifi();
            }
            c if c >= ' ' && c <= '~' => {
                if self.wifi_passphrase.len() < 48 {
                    self.wifi_passphrase.push(c);
                }
            }
            _ => {}
        }
    }

    fn connect_selected_wifi(&mut self) {
        let mut wifi = WIFI_MANAGER.lock();
        let target_ssid = self
            .selected_ap
            .and_then(|idx| wifi.scan_results.get(idx).map(|ap| ap.ssid.clone()))
            .or_else(|| wifi.scan_results.first().map(|ap| ap.ssid.clone()));

        let Some(ssid) = target_ssid else {
            self.status_feedback = String::from("Click 'Scan Wi-Fi' to discover networks first");
            return;
        };

        let pass = if self.wifi_passphrase.is_empty() {
            None
        } else {
            Some(self.wifi_passphrase.as_str())
        };
        match wifi.connect(&ssid, pass) {
            Ok(()) => {
                self.status_feedback = format!("Wi-Fi connected to '{}'", ssid);
            }
            Err(e) => {
                self.status_feedback = format!("Wi-Fi error: {}", e);
            }
        }
    }

    pub fn handle_click(&mut self, px: i32, py: i32, x: i32, y: i32, w: i32, _h: i32) {
        // 1. Property Sheet Tabs at top
        let tab_y = y + 8;
        let tabs = [
            ("General", SettingsTab::General),
            ("Display", SettingsTab::Display),
            ("Sound", SettingsTab::Sound),
            ("Network", SettingsTab::Network),
            ("Storage", SettingsTab::Storage),
        ];

        let mut tx = x + 10;
        for (label, tab) in tabs.iter() {
            let tw = measure_string(label) + 18;
            if px >= tx && px <= tx + tw && py >= tab_y && py <= tab_y + 22 {
                self.active_tab = *tab;
                self.pass_focused = false;
                return;
            }
            tx += tw + 4;
        }

        let page_x = x + 10;
        let page_y = y + 32;
        let page_w = w - 20;
        let content_x = page_x + 14;
        let content_w = (page_w - 28).max(160);

        match self.active_tab {
            SettingsTab::Display => {
                let btn_y = page_y + 132;
                if py >= btn_y && py <= btn_y + 24 {
                    if px >= content_x && px <= content_x + 148 {
                        let next = (get_ui_font_style() + 1) % 3;
                        set_ui_font_style(next);
                        self.status_feedback = format!("Font: {}", ui_font_style_name(next));
                    } else if px >= content_x + 158 && px <= content_x + 318 {
                        let next_bold = !get_terminal_bold();
                        set_terminal_bold(next_bold);
                        self.status_feedback = format!(
                            "Terminal font set to {}",
                            if next_bold { "Bold" } else { "Regular 8x16" }
                        );
                    }
                }
            }
            SettingsTab::Sound => {
                let slider_x = content_x + 110;
                let slider_w = (content_w - 170).max(110);
                if py >= page_y + 72 && py <= page_y + 94 && px >= slider_x && px <= slider_x + slider_w {
                    let ratio = (px - slider_x) as f32 / slider_w as f32;
                    self.audio_volume = ((ratio * 100.0) as u8).clamp(0, 100);
                    self.status_feedback = format!("Master volume set to {}%", self.audio_volume);
                }
                if py >= page_y + 114 && py <= page_y + 138 && px >= slider_x && px <= slider_x + 124 {
                    crate::drivers::audio::play_sublime_chime();
                    self.status_feedback = String::from("Playing SublimeOS Startup Chime via Intel HDA");
                }
            }
            SettingsTab::Network => {
                // Row of action buttons at page_y + 38: [Scan Wi-Fi] [Connect/Disconnect] [DHCP Renew]
                let btn_y = page_y + 38;
                if py >= btn_y && py <= btn_y + 22 {
                    if px >= content_x && px <= content_x + 96 {
                        let mut wifi = WIFI_MANAGER.lock();
                        let count = wifi.scan().len();
                        if self.selected_ap.is_none() && count > 0 {
                            self.selected_ap = Some(0);
                        }
                        self.status_feedback = format!("Wi-Fi scan found {} access points", count);
                        return;
                    }
                    if px >= content_x + 104 && px <= content_x + 206 {
                        let is_conn = WIFI_MANAGER.lock().is_connected;
                        if is_conn {
                            WIFI_MANAGER.lock().disconnect();
                            self.status_feedback = String::from("Wi-Fi disconnected");
                        } else {
                            self.connect_selected_wifi();
                        }
                        return;
                    }
                    if px >= content_x + 214 && px <= content_x + 318 {
                        let target_iface = {
                            let net = NET_MANAGER.lock();
                            net.get_default_device().map(|(d, _)| String::from(d.name()))
                        };
                        if let Some(iface) = target_iface {
                            if let Ok(frame) = crate::net::dhcp::start_dhcp_discovery(&iface) {
                                let dev = NET_MANAGER.lock().get_device(&iface).map(|(d, _)| d);
                                if let Some(d) = dev {
                                    let _ = d.send_packet(&frame);
                                }
                                self.status_feedback = format!("Sent DHCP Discover on {}", iface);
                            } else {
                                self.status_feedback = String::from("No active NIC for DHCP");
                            }
                        } else {
                            self.status_feedback = String::from("No network adapter registered");
                        }
                        return;
                    }
                }

                // Passphrase input field around page_y + 194
                let pass_x = content_x + 92;
                let pass_y = page_y + 194;
                let pass_w = (content_w - 92).max(120);
                if px >= pass_x && px <= pass_x + pass_w && py >= pass_y && py <= pass_y + 22 {
                    self.pass_focused = true;
                    return;
                }
                self.pass_focused = false;

                // AP list rows starting at page_y + 124 (up to 3 visible rows, each 20px)
                let list_y = page_y + 124;
                for idx in 0..3usize {
                    let ry = list_y + idx as i32 * 20;
                    if px >= content_x && px <= content_x + content_w && py >= ry && py < ry + 20 {
                        let len = WIFI_MANAGER.lock().scan_results.len();
                        if idx < len {
                            self.selected_ap = Some(idx);
                            let ssid = WIFI_MANAGER.lock().scan_results[idx].ssid.clone();
                            self.status_feedback = format!("Selected Wi-Fi AP: {}", ssid);
                        }
                        return;
                    }
                }
            }
            _ => {}
        }
    }

    pub fn render(&self, canvas: &mut Canvas, x: i32, y: i32, w: i32, h: i32) {
        canvas.fill_rect(x, y, w, h, Color::XP_FACE);

        // 1. Property Sheet Tabs
        let tab_y = y + 8;
        let tabs = [
            ("General", SettingsTab::General),
            ("Display", SettingsTab::Display),
            ("Sound", SettingsTab::Sound),
            ("Network", SettingsTab::Network),
            ("Storage", SettingsTab::Storage),
        ];

        let mut tx = x + 10;
        for (label, tab) in tabs.iter() {
            let tw = measure_string(label) + 18;
            let is_sel = *tab == self.active_tab;
            if is_sel {
                canvas.fill_rect(tx, tab_y - 2, tw, 24, Color::XP_FACE_LIGHT);
                canvas.fill_rect(tx, tab_y - 2, tw, 2, Color::XP_ORANGE_GLOW);
                canvas.draw_rect(tx, tab_y - 2, tw, 24, Color::XP_SHADOW);
                draw_string_bold(canvas, tx + 9, tab_y + 2, label, Color::XP_TEXT_BLACK);
            } else {
                canvas.fill_rect(tx, tab_y + 1, tw, 21, Color::XP_FACE_DARK);
                canvas.draw_rect(tx, tab_y + 1, tw, 21, Color::XP_SHADOW);
                draw_string(canvas, tx + 9, tab_y + 4, label, Color::XP_TEXT_GRAY);
            }
            tx += tw + 4;
        }

        // 2. White/Cream Tab Page Container
        let page_x = x + 10;
        let page_y = y + 32;
        let page_w = w - 20;
        let page_h = (h - 72).max(120);
        canvas.fill_rect(page_x, page_y, page_w, page_h, Color::XP_FACE_LIGHT);
        canvas.draw_rect(page_x, page_y, page_w, page_h, Color::XP_SHADOW);

        let content_x = page_x + 14;
        let content_w = (page_w - 28).max(140);
        let mut cy = page_y + 14;

        match self.active_tab {
            SettingsTab::General => {
                let hw = get_hardware_telemetry();
                draw_string_bold(canvas, content_x, cy, "SublimeOS System Properties", Color::XP_TEXT_BLACK);
                cy += 22;
                canvas.fill_rect(content_x, cy, content_w, 1, Color::XP_SHADOW);
                cy += 12;

                let rows: [(&str, &str); 6] = [
                    ("System:", "SublimeOS 1.0.0 (KiteKernel x86_64)"),
                    ("Computer:", &hw.model_name),
                    ("Processor:", &hw.chip),
                    ("Cores:", &hw.cores_threads),
                    ("Memory:", &hw.memory_summary),
                    ("Firmware:", &hw.system_firmware),
                ];
                for (label, val) in rows.iter() {
                    draw_string(canvas, content_x, cy, label, Color::XP_TEXT_GRAY);
                    draw_string_ellipsis(canvas, content_x + 104, cy, val, Color::XP_TEXT_BLACK, content_w - 104);
                    cy += 24;
                }
            }
            SettingsTab::Display => {
                let (gw, gh, gbpp, gpitch) = {
                    let v = VBE.lock();
                    (v.width, v.height, v.bpp, v.pitch)
                };
                draw_string_bold(canvas, content_x, cy, "Display & Typography Properties", Color::XP_TEXT_BLACK);
                cy += 22;
                canvas.fill_rect(content_x, cy, content_w, 1, Color::XP_SHADOW);
                cy += 12;

                let res_str = format!("{} x {} @ {} bpp (Pitch: {} B)", gw, gh, gbpp, gpitch);
                let style_name = ui_font_style_name(get_ui_font_style());
                let term_mode = if get_terminal_bold() { "Bold High-Contrast" } else { "Standard Mono 8x16" };
                let rows: [(&str, &str); 4] = [
                    ("Resolution:", res_str.as_str()),
                    ("Theme:", "Sublime Classic Blue"),
                    ("UI Font:", style_name),
                    ("Terminal Font:", term_mode),
                ];
                for (label, val) in rows.iter() {
                    draw_string(canvas, content_x, cy, label, Color::XP_TEXT_GRAY);
                    draw_string_ellipsis(canvas, content_x + 124, cy, val, Color::XP_TEXT_BLACK, content_w - 124);
                    cy += 22;
                }

                // Interactive font toggle buttons
                let btn_y = page_y + 132;
                canvas.draw_gradient_v(content_x, btn_y, 148, 24, Color::XP_WHITE, Color::XP_FACE_DARK);
                canvas.draw_rounded_rect(content_x, btn_y, 148, 24, 3, Color::XP_CTRL_BORDER);
                draw_string(canvas, content_x + 16, btn_y + 4, "Cycle UI Font", Color::XP_TEXT_BLACK);

                canvas.draw_gradient_v(content_x + 158, btn_y, 160, 24, Color::XP_WHITE, Color::XP_FACE_DARK);
                canvas.draw_rounded_rect(content_x + 158, btn_y, 160, 24, 3, Color::XP_CTRL_BORDER);
                draw_string(canvas, content_x + 170, btn_y + 4, "Toggle Term Bold", Color::XP_TEXT_BLACK);
            }
            SettingsTab::Sound => {
                draw_string_bold(canvas, content_x, cy, "Sounds and Audio Devices", Color::XP_TEXT_BLACK);
                cy += 22;
                canvas.fill_rect(content_x, cy, content_w, 1, Color::XP_SHADOW);
                cy += 14;

                draw_string(canvas, content_x, cy, "Audio Device:", Color::XP_TEXT_GRAY);
                draw_string_ellipsis(canvas, content_x + 110, cy, "Intel High Definition Audio (HDA)", Color::XP_TEXT_LINK, content_w - 110);
                cy += 28;

                draw_string(canvas, content_x, cy, "Master Volume:", Color::XP_TEXT_GRAY);
                let slider_x = content_x + 110;
                let slider_w = (content_w - 170).max(110);
                canvas.fill_rect(slider_x, cy + 4, slider_w, 6, Color::WHITE);
                canvas.draw_rect(slider_x, cy + 4, slider_w, 6, Color::XP_INPUT_BORDER);
                let filled_w = ((self.audio_volume as i32 * slider_w) / 100).clamp(4, slider_w);
                canvas.fill_rect(slider_x, cy + 4, filled_w, 6, Color::XP_SELECTION);
                canvas.fill_rect(slider_x + filled_w - 4, cy, 8, 16, Color::XP_FACE);
                canvas.draw_rect(slider_x + filled_w - 4, cy, 8, 16, Color::XP_CTRL_BORDER);

                let vol_text = format!("{}%", self.audio_volume);
                draw_string(canvas, slider_x + slider_w + 10, cy, &vol_text, Color::XP_TEXT_BLACK);
                cy += 38;

                canvas.draw_gradient_v(slider_x, cy, 124, 24, Color::XP_WHITE, Color::XP_FACE_DARK);
                canvas.draw_rounded_rect(slider_x, cy, 124, 24, 3, Color::XP_CTRL_BORDER);
                draw_string(canvas, slider_x + 14, cy + 4, "Play Chime", Color::XP_TEXT_BLACK);
            }
            SettingsTab::Network => {
                let (wifi_conn, wifi_ssid, wifi_spd, wifi_aps): (bool, Option<String>, u32, Vec<WifiAccessPoint>) = {
                    let wlock = WIFI_MANAGER.lock();
                    (
                        wlock.is_connected,
                        wlock.current_ssid.clone(),
                        wlock.link_speed_mbps,
                        wlock.scan_results.clone(),
                    )
                };

                let status_line = if wifi_conn {
                    format!(
                        "Wireless: Connected to {} ({} Mbps)",
                        wifi_ssid.as_deref().unwrap_or("WLAN"),
                        wifi_spd
                    )
                } else {
                    String::from("Wireless: Ready (Click Scan Wi-Fi)")
                };
                draw_string_bold(canvas, content_x, cy, &status_line, if wifi_conn { Color::rgb(20, 120, 40) } else { Color::XP_TEXT_BLACK });

                // Buttons: [Scan Wi-Fi] [Connect/Disconnect] [DHCP Renew]
                let btn_y = page_y + 38;
                draw_ui_button(canvas, content_x, btn_y, 96, 22, "Scan Wi-Fi");
                draw_ui_button(
                    canvas,
                    content_x + 104,
                    btn_y,
                    102,
                    22,
                    if wifi_conn { "Disconnect" } else { "Connect" },
                );
                draw_ui_button(canvas, content_x + 214, btn_y, 104, 22, "DHCP Renew");

                // Active kernel NIC summary line
                let net_summary = {
                    let net = NET_MANAGER.lock();
                    let mut parts = String::new();
                    for (dev, cfg) in &net.devices {
                        if !parts.is_empty() {
                            parts.push_str(" | ");
                        }
                        let st = if dev.is_link_up() { "UP" } else { "DN" };
                        parts.push_str(&format!(
                            "{}({}):{}.{}.{}.{}",
                            dev.name(), st, cfg.ipv4[0], cfg.ipv4[1], cfg.ipv4[2], cfg.ipv4[3]
                        ));
                    }
                    if parts.is_empty() {
                        String::from("No NICs registered")
                    } else {
                        parts
                    }
                };
                draw_string_ellipsis(canvas, content_x, page_y + 68, &net_summary, Color::XP_TEXT_LINK, content_w);

                // Wi-Fi Access Points header + box
                draw_string_bold(canvas, content_x, page_y + 92, "Available 802.11 Networks:", Color::XP_TEXT_GRAY);
                canvas.draw_gradient_v(content_x, page_y + 110, content_w, 12, Color::XP_WHITE, Color::XP_FACE_DARK);
                canvas.fill_rect(content_x, page_y + 122, content_w, 64, Color::WHITE);
                canvas.draw_rect(content_x, page_y + 122, content_w, 64, Color::XP_INPUT_BORDER);

                let list_y = page_y + 124;
                if wifi_aps.is_empty() {
                    draw_string_ellipsis(
                        canvas,
                        content_x + 8,
                        list_y + 20,
                        "No scan results yet. Click 'Scan Wi-Fi' above.",
                        Color::XP_TEXT_GRAY,
                        content_w - 16,
                    );
                } else {
                    for (idx, ap) in wifi_aps.iter().take(3).enumerate() {
                        let ry = list_y + idx as i32 * 20;
                        let is_sel = self.selected_ap == Some(idx);
                        if is_sel {
                            canvas.fill_rect(content_x + 2, ry, content_w - 4, 19, Color::XP_SELECTION);
                        }
                        let sec = ap.security.as_str();
                        let bars = ap.signal_bars();
                        let row = format!("{}  Ch{} {}dBm {} {}", ap.ssid, ap.channel, ap.rssi, sec, bars);
                        draw_string_ellipsis(
                            canvas,
                            content_x + 8,
                            ry + 2,
                            &row,
                            if is_sel { Color::WHITE } else { Color::XP_TEXT_BLACK },
                            content_w - 16,
                        );
                    }
                }

                // Passphrase field
                let pass_y = page_y + 194;
                draw_string(canvas, content_x, pass_y + 3, "Passkey:", Color::XP_TEXT_GRAY);
                let pass_x = content_x + 92;
                let pass_w = (content_w - 92).max(100);
                canvas.fill_rect(pass_x, pass_y, pass_w, 22, Color::WHITE);
                canvas.draw_rect(
                    pass_x,
                    pass_y,
                    pass_w,
                    22,
                    if self.pass_focused { Color::XP_SELECTION } else { Color::XP_INPUT_BORDER },
                );
                if self.wifi_passphrase.is_empty() {
                    draw_string_ellipsis(
                        canvas,
                        pass_x + 6,
                        pass_y + 3,
                        if self.pass_focused { "Type passphrase + Enter..." } else { "Click here to type WPA2 key" },
                        Color::XP_TEXT_GRAY,
                        pass_w - 12,
                    );
                } else {
                    draw_string_ellipsis(canvas, pass_x + 6, pass_y + 3, &self.wifi_passphrase, Color::XP_TEXT_BLACK, pass_w - 12);
                }
            }
            SettingsTab::Storage => {
                draw_string_bold(canvas, content_x, cy, "Mounted Storage & Memory Cache", Color::XP_TEXT_BLACK);
                cy += 22;
                canvas.fill_rect(content_x, cy, content_w, 1, Color::XP_SHADOW);
                cy += 12;

                let (used_mb, total_mb) = {
                    let pmm = PMM.lock();
                    ((pmm.get_used_frames() * 4) / 1024, (pmm.get_total_frames() * 4) / 1024)
                };
                let vols = VFS
                    .lock()
                    .resolve_path("/Volumes")
                    .and_then(|node| node.lock().readdir())
                    .unwrap_or_default();
                let mut rows: Vec<(String, String)> = Vec::new();
                rows.push((String::from("Root VFS:"), String::from("/ (ramfs + devfs + tmpfs)")));
                if vols.is_empty() {
                    rows.push((String::from("Volumes:"), String::from("No external partitions detected")));
                } else {
                    for entry in vols.iter().take(4) {
                        rows.push((String::from("Mounted:"), format!("/Volumes/{}", entry.name)));
                    }
                }
                rows.push((String::from("Physical RAM:"), format!("{} MB / {} MB allocated", used_mb, total_mb)));

                for (label, val) in rows.iter() {
                    draw_string(canvas, content_x, cy, label, Color::XP_TEXT_GRAY);
                    draw_string_ellipsis(canvas, content_x + 114, cy, val, Color::XP_TEXT_BLACK, content_w - 114);
                    cy += 24;
                }
            }
        }

        // Bottom feedback status & Apply indicator
        let bot_y = y + h - 32;
        draw_string_ellipsis(canvas, x + 12, bot_y + 4, &self.status_feedback, Color::XP_TEXT_GRAY, w - 104);
        let ok_x = x + w - 84;
        draw_ui_button(canvas, ok_x, bot_y, 72, 22, "OK");
    }
}

fn draw_ui_button(canvas: &mut Canvas, x: i32, y: i32, w: i32, h: i32, label: &str) {
    canvas.draw_gradient_v(x, y, w, h, Color::XP_WHITE, Color::XP_FACE_DARK);
    canvas.draw_rounded_rect(x, y, w, h, 3, Color::XP_CTRL_BORDER);
    let tw = measure_string(label).min(w - 8);
    let tx = x + ((w - tw) / 2).max(4);
    let ty = y + (h - 16) / 2;
    draw_string_ellipsis(canvas, tx, ty, label, Color::XP_TEXT_BLACK, w - 8);
}
