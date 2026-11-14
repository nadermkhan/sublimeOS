use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::gui::canvas::Canvas;
use crate::gui::color::Color;
use crate::gui::font::{draw_string, draw_string_bold, draw_string_ellipsis, measure_string};
use crate::mm::pmm::PMM;
use crate::net::device::NET_MANAGER;
use crate::net::wifi::WIFI_MANAGER;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SysmonTab {
    Applications,
    Processes,
    Performance,
    Networking,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SysmonAction {
    None,
    SwitchToWindow(u32),
    CloseWindow(u32),
}

pub struct SysmonApp {
    pub active_tab: SysmonTab,
    pub selected_row: usize,
    pub window_list: Vec<(u32, String, bool)>,
    pub pending_action: SysmonAction,
}

impl SysmonApp {
    pub fn new() -> Self {
        Self {
            active_tab: SysmonTab::Processes,
            selected_row: 0,
            window_list: Vec::new(),
            pending_action: SysmonAction::None,
        }
    }

    pub fn sync_windows(&mut self, wins: Vec<(u32, String, bool)>) {
        self.window_list = wins;
        if self.active_tab == SysmonTab::Applications && self.selected_row >= self.window_list.len() {
            self.selected_row = self.window_list.len().saturating_sub(1);
        }
    }

    pub fn take_action(&mut self) -> SysmonAction {
        let act = self.pending_action;
        self.pending_action = SysmonAction::None;
        act
    }

    pub fn handle_click(&mut self, px: i32, py: i32, x: i32, y: i32, w: i32, h: i32) {
        // 1. Tabs at y + 26
        let tab_y = y + 26;
        let tabs = [
            ("Applications", SysmonTab::Applications),
            ("Processes", SysmonTab::Processes),
            ("Performance", SysmonTab::Performance),
            ("Networking", SysmonTab::Networking),
        ];

        let mut tab_x = x + 10;
        for (label, tab) in tabs.iter() {
            let tw = measure_string(label) + 14;
            if px >= tab_x && px <= tab_x + tw && py >= tab_y && py <= tab_y + 22 {
                self.active_tab = *tab;
                self.selected_row = 0;
                return;
            }
            tab_x += tw + 3;
        }

        let content_x = x + 10;
        let content_w = (w - 20).max(200);
        let body_y = tab_y + 22;
        let tbl_x = content_x + 10;
        let tbl_y = body_y + 10;
        let tbl_w = (content_w - 20).max(160);

        match self.active_tab {
            SysmonTab::Applications => {
                // Rows inside application list
                let mut ry = tbl_y + 26;
                for idx in 0..self.window_list.len().min(6) {
                    if px >= tbl_x && px <= tbl_x + tbl_w && py >= ry && py < ry + 20 {
                        self.selected_row = idx;
                        return;
                    }
                    ry += 20;
                }
                // Bottom action buttons: [Switch To] [End Task]
                let btn_y = y + h - 54;
                if py >= btn_y && py <= btn_y + 22 {
                    if let Some((win_id, _, _)) = self.window_list.get(self.selected_row) {
                        if px >= tbl_x + tbl_w - 196 && px <= tbl_x + tbl_w - 102 {
                            self.pending_action = SysmonAction::SwitchToWindow(*win_id);
                        } else if px >= tbl_x + tbl_w - 94 && px <= tbl_x + tbl_w {
                            self.pending_action = SysmonAction::CloseWindow(*win_id);
                        }
                    }
                }
            }
            SysmonTab::Processes | SysmonTab::Networking => {
                let mut ry = tbl_y + 26;
                for idx in 0..7usize {
                    if px >= tbl_x && px <= tbl_x + tbl_w && py >= ry && py < ry + 19 {
                        self.selected_row = idx;
                        return;
                    }
                    ry += 19;
                }
            }
            SysmonTab::Performance => {}
        }
    }

    pub fn render(&self, canvas: &mut Canvas, x: i32, y: i32, w: i32, h: i32) {
        canvas.fill_rect(x, y, w, h, Color::XP_FACE);

        // 0. Classic Menu Bar
        canvas.fill_rect(x, y, w, 20, Color::XP_FACE);
        draw_string_ellipsis(canvas, x + 8, y + 2, "File   Options   View   Help", Color::XP_TEXT_BLACK, w - 16);
        canvas.fill_rect(x, y + 20, w, 1, Color::XP_SHADOW);

        // 1. Property Sheet Tabs
        let tab_y = y + 26;
        let tabs = [
            ("Applications", SysmonTab::Applications),
            ("Processes", SysmonTab::Processes),
            ("Performance", SysmonTab::Performance),
            ("Networking", SysmonTab::Networking),
        ];

        let mut tab_x = x + 10;
        for (label, tab) in tabs.iter() {
            let tw = measure_string(label) + 14;
            if *tab == self.active_tab {
                canvas.fill_rect(tab_x, tab_y - 2, tw, 24, Color::XP_FACE_LIGHT);
                canvas.fill_rect(tab_x, tab_y - 2, tw, 2, Color::XP_ORANGE_GLOW);
                canvas.draw_rect(tab_x, tab_y - 2, tw, 24, Color::XP_SHADOW);
                draw_string_bold(canvas, tab_x + 7, tab_y + 2, label, Color::XP_TEXT_BLACK);
            } else {
                canvas.fill_rect(tab_x, tab_y + 1, tw, 21, Color::XP_FACE_DARK);
                canvas.draw_rect(tab_x, tab_y + 1, tw, 21, Color::XP_SHADOW);
                draw_string(canvas, tab_x + 7, tab_y + 4, label, Color::XP_TEXT_GRAY);
            }
            tab_x += tw + 3;
        }

        // 2. Main Tab Page Container
        let content_x = x + 10;
        let content_w = (w - 20).max(200);
        let body_y = tab_y + 22;
        let body_h = (h - 74).max(120);
        canvas.fill_rect(content_x, body_y, content_w, body_h, Color::XP_FACE_LIGHT);
        canvas.draw_rect(content_x, body_y, content_w, body_h, Color::XP_SHADOW);

        let tbl_x = content_x + 10;
        let tbl_y = body_y + 10;
        let tbl_w = (content_w - 20).max(160);

        // Read live kernel metrics once
        let (used_mb, total_mb) = {
            let pmm = PMM.lock();
            ((pmm.get_used_frames() * 4) / 1024, (pmm.get_total_frames() * 4) / 1024)
        };
        let (h_total, h_used, _) = crate::mm::heap::HEAP_ALLOCATOR.stats();
        let ticks = crate::arch::x86_64::pit::get_ticks();
        let tasks = crate::task::scheduler::get_task_list();

        match self.active_tab {
            SysmonTab::Applications => {
                let tbl_h = (body_h - 46).max(60);
                canvas.fill_rect(tbl_x, tbl_y, tbl_w, tbl_h, Color::WHITE);
                canvas.draw_rect(tbl_x, tbl_y, tbl_w, tbl_h, Color::XP_INPUT_BORDER);

                canvas.draw_gradient_v(tbl_x + 1, tbl_y + 1, tbl_w - 2, 20, Color::XP_WHITE, Color::XP_FACE_DARK);
                canvas.fill_rect(tbl_x + 1, tbl_y + 21, tbl_w - 2, 1, Color::XP_SHADOW);
                draw_string(canvas, tbl_x + 8, tbl_y + 3, "Task Window", Color::XP_TEXT_BLACK);
                draw_string(canvas, tbl_x + tbl_w - 92, tbl_y + 3, "Status", Color::XP_TEXT_BLACK);

                let mut ry = tbl_y + 26;
                for (idx, (_, title, is_min)) in self.window_list.iter().enumerate() {
                    if ry + 18 > tbl_y + tbl_h {
                        break;
                    }
                    let is_sel = idx == self.selected_row;
                    if is_sel {
                        canvas.fill_rect(tbl_x + 2, ry - 1, tbl_w - 4, 19, Color::XP_SELECTION);
                    }
                    let fg = if is_sel { Color::WHITE } else { Color::XP_TEXT_BLACK };
                    let st = if *is_min { "Minimized" } else { "Running" };
                    draw_string_ellipsis(canvas, tbl_x + 8, ry + 1, title, fg, tbl_w - 110);
                    draw_string(canvas, tbl_x + tbl_w - 92, ry + 1, st, fg);
                    ry += 20;
                }

                let btn_y = y + h - 54;
                draw_sysmon_btn(canvas, tbl_x + tbl_w - 196, btn_y, 94, 22, "Switch To");
                draw_sysmon_btn(canvas, tbl_x + tbl_w - 94, btn_y, 94, 22, "End Task");
            }
            SysmonTab::Processes => {
                let tbl_h = (body_h - 92).max(60);
                canvas.fill_rect(tbl_x, tbl_y, tbl_w, tbl_h, Color::WHITE);
                canvas.draw_rect(tbl_x, tbl_y, tbl_w, tbl_h, Color::XP_INPUT_BORDER);

                canvas.draw_gradient_v(tbl_x + 1, tbl_y + 1, tbl_w - 2, 20, Color::XP_WHITE, Color::XP_FACE_DARK);
                canvas.fill_rect(tbl_x + 1, tbl_y + 21, tbl_w - 2, 1, Color::XP_SHADOW);

                let c_name = tbl_x + 6;
                let c_tid = tbl_x + (tbl_w * 46) / 100;
                let c_cpu = tbl_x + (tbl_w * 60) / 100;
                let c_state = tbl_x + (tbl_w * 74) / 100;

                draw_string(canvas, c_name, tbl_y + 3, "Task Name", Color::XP_TEXT_BLACK);
                draw_string(canvas, c_tid, tbl_y + 3, "TID", Color::XP_TEXT_BLACK);
                draw_string(canvas, c_cpu, tbl_y + 3, "Core", Color::XP_TEXT_BLACK);
                draw_string(canvas, c_state, tbl_y + 3, "State", Color::XP_TEXT_BLACK);

                let mut ry = tbl_y + 25;
                for (idx, (tid, name, state, cpu)) in tasks.iter().enumerate() {
                    if ry + 18 > tbl_y + tbl_h {
                        break;
                    }
                    let is_sel = idx == self.selected_row;
                    if is_sel {
                        canvas.fill_rect(tbl_x + 2, ry - 1, tbl_w - 4, 18, Color::XP_SELECTION);
                    }
                    let fg = if is_sel { Color::WHITE } else { Color::XP_TEXT_BLACK };
                    let sub_fg = if is_sel { Color::WHITE } else { Color::XP_TEXT_GRAY };
                    let st_str = format!("{:?}", state);
                    draw_string_ellipsis(canvas, c_name, ry + 1, name, fg, (c_tid - c_name - 6).max(32));
                    draw_string(canvas, c_tid, ry + 1, &format!("{}", tid), sub_fg);
                    draw_string(canvas, c_cpu, ry + 1, &format!("CPU{}", cpu), sub_fg);
                    draw_string_ellipsis(canvas, c_state, ry + 1, &st_str, fg, (tbl_x + tbl_w - c_state - 4).max(32));
                    ry += 19;
                }

                // Bottom live meter box
                let card_y = tbl_y + tbl_h + 8;
                draw_perf_strip(canvas, tbl_x, card_y, tbl_w, used_mb, total_mb, h_used, h_total, ticks);
            }
            SysmonTab::Performance => {
                draw_string_bold(canvas, tbl_x, tbl_y, "Live Kernel & Memory Telemetry", Color::XP_TEXT_BLACK);
                let mem_pct = if total_mb > 0 { ((used_mb * 100) / total_mb).min(100) } else { 0 };
                let heap_pct = if h_total > 0 { ((h_used * 100) / h_total).min(100) } else { 0 };

                let m_y = tbl_y + 28;
                draw_string(canvas, tbl_x, m_y, &format!("Physical Memory: {} / {} MB ({}%)", used_mb, total_mb, mem_pct), Color::XP_TEXT_BLACK);
                canvas.fill_rect(tbl_x, m_y + 20, tbl_w, 18, Color::BLACK);
                let bar_w = ((tbl_w - 4) * mem_pct as i32) / 100;
                canvas.fill_rect(tbl_x + 2, m_y + 22, bar_w.max(2), 14, Color::rgb(0, 210, 64));

                let h_y = m_y + 52;
                draw_string(canvas, tbl_x, h_y, &format!("Kernel Heap Allocator: {} / {} KB ({}%)", h_used / 1024, h_total / 1024, heap_pct), Color::XP_TEXT_BLACK);
                canvas.fill_rect(tbl_x, h_y + 20, tbl_w, 18, Color::BLACK);
                let hbar_w = ((tbl_w - 4) * heap_pct as i32) / 100;
                canvas.fill_rect(tbl_x + 2, h_y + 22, hbar_w.max(2), 14, Color::rgb(64, 175, 255));

                let u_y = h_y + 54;
                let cpu_cnt = crate::arch::x86_64::smp::SMP.lock().online_count();
                draw_string_ellipsis(canvas, tbl_x, u_y, &format!("SMP Cores Online: {}   |   PIT Frequency: 1000 Hz", cpu_cnt), Color::XP_TEXT_GRAY, tbl_w);
                draw_string_ellipsis(canvas, tbl_x, u_y + 22, &format!("System Uptime: {}s ({} ticks)", ticks / 1000, ticks), Color::XP_TEXT_LINK, tbl_w);
            }
            SysmonTab::Networking => {
                let tbl_h = (body_h - 72).max(60);
                canvas.fill_rect(tbl_x, tbl_y, tbl_w, tbl_h, Color::WHITE);
                canvas.draw_rect(tbl_x, tbl_y, tbl_w, tbl_h, Color::XP_INPUT_BORDER);

                canvas.draw_gradient_v(tbl_x + 1, tbl_y + 1, tbl_w - 2, 20, Color::XP_WHITE, Color::XP_FACE_DARK);
                canvas.fill_rect(tbl_x + 1, tbl_y + 21, tbl_w - 2, 1, Color::XP_SHADOW);

                let c_if = tbl_x + 8;
                let c_ip = tbl_x + (tbl_w * 28) / 100;
                let c_tx = tbl_x + (tbl_w * 58) / 100;
                let c_st = tbl_x + (tbl_w * 78) / 100;
                draw_string(canvas, c_if, tbl_y + 3, "Adapter", Color::XP_TEXT_BLACK);
                draw_string(canvas, c_ip, tbl_y + 3, "IPv4 Address", Color::XP_TEXT_BLACK);
                draw_string(canvas, c_tx, tbl_y + 3, "TX / RX", Color::XP_TEXT_BLACK);
                draw_string(canvas, c_st, tbl_y + 3, "Link", Color::XP_TEXT_BLACK);

                let net = NET_MANAGER.lock();
                let mut ry = tbl_y + 26;
                for (idx, (dev, cfg)) in net.devices.iter().enumerate() {
                    if ry + 18 > tbl_y + tbl_h {
                        break;
                    }
                    let is_sel = idx == self.selected_row;
                    if is_sel {
                        canvas.fill_rect(tbl_x + 2, ry - 1, tbl_w - 4, 18, Color::XP_SELECTION);
                    }
                    let fg = if is_sel { Color::WHITE } else { Color::XP_TEXT_BLACK };
                    let up_fg = if is_sel { Color::WHITE } else if dev.is_link_up() { Color::rgb(20, 130, 45) } else { Color::XP_TEXT_GRAY };
                    draw_string_ellipsis(canvas, c_if, ry + 1, dev.name(), fg, (c_ip - c_if - 4).max(20));
                    draw_string(canvas, c_ip, ry + 1, &format!("{}.{}.{}.{}", cfg.ipv4[0], cfg.ipv4[1], cfg.ipv4[2], cfg.ipv4[3]), fg);
                    draw_string(canvas, c_tx, ry + 1, &format!("{}/{}", cfg.tx_packets, cfg.rx_packets), fg);
                    draw_string(canvas, c_st, ry + 1, if dev.is_link_up() { "UP" } else { "DOWN" }, up_fg);
                    ry += 19;
                }
                drop(net);

                let wifi = WIFI_MANAGER.lock();
                let w_y = tbl_y + tbl_h + 10;
                let wifi_txt = if wifi.is_connected {
                    format!(
                        "Wi-Fi Active: {} (Ch {}, {} Mbps)",
                        wifi.current_ssid.as_deref().unwrap_or("WLAN"),
                        wifi.current_channel,
                        wifi.link_speed_mbps
                    )
                } else {
                    format!("Wi-Fi Idle: 802.11 BCM4331 ready ({} cached APs)", wifi.scan_results.len())
                };
                draw_string_ellipsis(canvas, tbl_x, w_y, &wifi_txt, Color::XP_TEXT_LINK, tbl_w);
            }
        }

        // 4. Bottom Status Strip
        let stat_y = y + h - 22;
        canvas.fill_rect(x, stat_y, w, 22, Color::XP_FACE);
        canvas.fill_rect(x, stat_y, w, 1, Color::XP_SHADOW);
        let mem_pct = if total_mb > 0 { (used_mb * 100) / total_mb } else { 0 };
        let footer = format!(
            "Tasks: {}   Windows: {}   RAM: {}%   Up: {}s",
            tasks.len(),
            self.window_list.len(),
            mem_pct,
            ticks / 1000
        );
        draw_string_ellipsis(canvas, tbl_x, stat_y + 4, &footer, Color::XP_TEXT_BLACK, w - 24);
    }
}

fn draw_perf_strip(
    canvas: &mut Canvas,
    tbl_x: i32,
    card_y: i32,
    tbl_w: i32,
    used_mb: usize,
    total_mb: usize,
    h_used: usize,
    h_total: usize,
    ticks: u64,
) {
    let meter_w = 128;
    let meter_h = 68;
    canvas.fill_rect(tbl_x, card_y, meter_w, meter_h, Color::BLACK);
    canvas.draw_rect(tbl_x, card_y, meter_w, meter_h, Color::XP_SHADOW);
    draw_string(canvas, tbl_x + 8, card_y + 6, "MEM Usage", Color::rgb(0, 220, 60));

    let mem_pct = if total_mb > 0 { ((used_mb * 100) / total_mb).min(100) } else { 0 };
    let bar_w = 108;
    let filled = ((mem_pct as i32 * bar_w) / 100).clamp(2, bar_w);
    canvas.fill_rect(tbl_x + 8, card_y + 28, bar_w, 12, Color::rgb(16, 48, 20));
    canvas.fill_rect(tbl_x + 8, card_y + 28, filled, 12, Color::rgb(0, 210, 64));
    draw_string(canvas, tbl_x + 8, card_y + 46, &format!("{}%", mem_pct), Color::rgb(120, 255, 140));

    let mid_x = tbl_x + meter_w + 14;
    let rem_w = (tbl_w - meter_w - 16).max(80);
    draw_string_ellipsis(canvas, mid_x, card_y + 6, &format!("Physical Memory: {}/{} MB", used_mb, total_mb), Color::XP_TEXT_BLACK, rem_w);
    draw_string_ellipsis(canvas, mid_x, card_y + 26, &format!("Kernel Heap: {}.{} / {}.{} MB", h_used / 1024 / 1024, (h_used / 102400) % 10, h_total / 1024 / 1024, (h_total / 102400) % 10), Color::XP_TEXT_BLACK, rem_w);
    draw_string_ellipsis(canvas, mid_x, card_y + 46, &format!("Uptime: {}s (1000 Hz)", ticks / 1000), Color::XP_TEXT_LINK, rem_w);
}

fn draw_sysmon_btn(canvas: &mut Canvas, x: i32, y: i32, w: i32, h: i32, label: &str) {
    canvas.draw_gradient_v(x, y, w, h, Color::XP_WHITE, Color::XP_FACE_DARK);
    canvas.draw_rounded_rect(x, y, w, h, 3, Color::XP_CTRL_BORDER);
    let tw = measure_string(label).min(w - 8);
    draw_string_ellipsis(canvas, x + (w - tw) / 2, y + 3, label, Color::XP_TEXT_BLACK, w - 8);
}
