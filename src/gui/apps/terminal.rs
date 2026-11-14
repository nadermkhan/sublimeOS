use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::arch::x86_64::rtc::get_system_datetime;
use crate::fs::vfs::VFS;
use crate::gui::canvas::Canvas;
use crate::gui::color::Color;
use crate::gui::font::{
    draw_string, draw_string_bold, get_terminal_bold, set_terminal_bold, set_ui_font_style,
    FONT_HEIGHT, FONT_WIDTH,
};
use crate::mm::pmm::PMM;
use crate::net::device::NET_MANAGER;
use crate::net::wifi::WIFI_MANAGER;

const CMD_BG: Color = Color(0xFF0B0E14);
const CMD_TEXT: Color = Color(0xFFD6DCE5);
const CMD_BRIGHT: Color = Color(0xFFFFFFFF);
const CMD_ACCENT: Color = Color(0xFF66B8FF);
const CMD_GOOD: Color = Color(0xFF68E088);
const CMD_WARN: Color = Color(0xFFFFC866);
const CMD_ERR: Color = Color(0xFFFF7A7A);

pub struct TerminalApp {
    pub lines: Vec<(String, Color)>,
    pub input: String,
    pub cwd: String,
    pub cursor_blink: u64,
}

impl TerminalApp {
    pub fn new() -> Self {
        let mut lines = Vec::new();
        lines.push((String::from("SublimeOS Terminal [Version 1.0.0]"), CMD_ACCENT));
        lines.push((
            String::from("KiteKernel x86_64 | (C) 2026 Nader Mahbub Khan"),
            CMD_TEXT,
        ));
        lines.push((
            String::from("Type 'help' for commands, or 'wifi scan' for wireless."),
            CMD_TEXT,
        ));
        lines.push((String::new(), CMD_TEXT));

        Self {
            lines,
            input: String::new(),
            cwd: String::from("/"),
            cursor_blink: 0,
        }
    }

    fn prompt_prefix(&self) -> String {
        format!("sublime@SublimeOS:{}$ ", self.cwd)
    }

    pub fn handle_char(&mut self, ch: char) {
        match ch {
            '\n' | '\r' => {
                let cmd_line = self.input.clone();
                let prompt = format!("{}{}", self.prompt_prefix(), cmd_line);
                self.lines.push((prompt, CMD_BRIGHT));
                self.input.clear();
                self.execute_command(cmd_line.trim());
            }
            '\x08' => {
                self.input.pop();
            }
            c if c >= ' ' && c <= '~' => {
                if self.input.len() < 120 {
                    self.input.push(c);
                }
            }
            _ => {}
        }

        if self.lines.len() > 160 {
            let drain_count = self.lines.len() - 160;
            self.lines.drain(0..drain_count);
        }
    }

    fn resolve_path(&self, arg: &str) -> String {
        if arg.is_empty() || arg == "." {
            return self.cwd.clone();
        }
        if arg == ".." {
            if self.cwd == "/" {
                return String::from("/");
            }
            if let Some(pos) = self.cwd.rfind('/') {
                if pos == 0 {
                    return String::from("/");
                }
                return String::from(&self.cwd[..pos]);
            }
            return String::from("/");
        }
        if arg.starts_with('/') {
            String::from(arg)
        } else if self.cwd == "/" {
            format!("/{}", arg)
        } else {
            format!("{}/{}", self.cwd, arg)
        }
    }

    fn execute_command(&mut self, raw_cmd: &str) {
        if raw_cmd.is_empty() {
            return;
        }

        let mut parts = raw_cmd.splitn(2, ' ');
        let cmd = parts.next().unwrap_or("");
        let rest = parts.next().unwrap_or("").trim();
        let lower = cmd.to_ascii_lowercase();

        match lower.as_str() {
            "help" | "?" => {
                self.lines.push((String::from("SublimeOS Shell Commands:"), CMD_ACCENT));
                self.lines.push((String::from("  clear / cls         Clear terminal"), CMD_TEXT));
                self.lines.push((String::from("  ver / uname         OS & kernel info"), CMD_TEXT));
                self.lines.push((String::from("  sysinfo / mem       Hardware & RAM stats"), CMD_TEXT));
                self.lines.push((String::from("  ps / tasklist       Active kernel tasks"), CMD_TEXT));
                self.lines.push((String::from("  ls [path] / cd      Browse VFS directories"), CMD_TEXT));
                self.lines.push((String::from("  cat <file>          Read a file"), CMD_TEXT));
                self.lines.push((String::from("  volumes / df        Mounted filesystems"), CMD_TEXT));
                self.lines.push((String::from("  ifconfig / net      Network adapters & IPs"), CMD_TEXT));
                self.lines.push((String::from("  wifi <scan|status>  Wireless 802.11 control"), CMD_TEXT));
                self.lines.push((String::from("  wifi connect <s>    Join SSID (optional pass)"), CMD_TEXT));
                self.lines.push((String::from("  font <bold|regular> Switch font style"), CMD_TEXT));
                self.lines.push((String::from("  sound               Play Intel HDA chime"), CMD_TEXT));
                self.lines.push((String::from("  date / uptime       Clock & uptime ticks"), CMD_TEXT));
                self.lines.push((String::from("  echo <text>         Print message"), CMD_TEXT));
                self.lines.push((String::from("  reboot              Restart computer"), CMD_WARN));
            }
            "clear" | "cls" => {
                self.lines.clear();
            }
            "ver" | "sw_vers" => {
                self.lines.push((String::from("SublimeOS Version 1.0.0 (Build 100.2026)"), CMD_BRIGHT));
                self.lines.push((String::from("Kernel: KiteKernel 1.0.0 (x86_64 UEFI/ACPI)"), CMD_TEXT));
            }
            "uname" => {
                self.lines.push((String::from("SublimeOS KiteKernel 1.0.0 x86_64"), CMD_BRIGHT));
            }
            "sysinfo" | "systeminfo" => {
                let hw = crate::drivers::platform::smbios::get_hardware_telemetry();
                self.lines.push((format!("OS:      SublimeOS 1.0.0 (KiteKernel)"), CMD_BRIGHT));
                self.lines.push((format!("Model:   {}", hw.model_name), CMD_TEXT));
                self.lines.push((format!("CPU:     {}", hw.chip), CMD_TEXT));
                self.lines.push((format!("Cores:   {}", hw.cores_threads), CMD_TEXT));
                self.lines.push((format!("Memory:  {}", hw.memory_summary), CMD_TEXT));
                self.lines.push((format!("BIOS:    {}", hw.system_firmware), CMD_TEXT));
            }
            "echo" => {
                let s = if rest.is_empty() { "SublimeOS" } else { rest };
                self.lines.push((String::from(s), CMD_TEXT));
            }
            "env" | "set" => {
                self.lines.push((String::from("OS=SublimeOS; KERNEL=KiteKernel; ARCH=x86_64"), CMD_TEXT));
                self.lines.push((format!("PWD={}; USER=sublime; THEME=Sublime_Classic", self.cwd), CMD_TEXT));
            }
            "mem" | "free" => {
                let (used_mb, total_mb) = {
                    let pmm = PMM.lock();
                    ((pmm.get_used_frames() * 4) / 1024, (pmm.get_total_frames() * 4) / 1024)
                };
                let (h_total, h_used, _) = crate::mm::heap::HEAP_ALLOCATOR.stats();
                self.lines.push((format!("Physical RAM: {} MB / {} MB used", used_mb, total_mb), CMD_TEXT));
                self.lines.push((format!("Kernel Heap:  {} KB / {} KB", h_used / 1024, h_total / 1024), CMD_TEXT));
            }
            "date" | "time" => {
                let dt = get_system_datetime();
                self.lines.push((
                    format!(
                        "{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC",
                        dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second
                    ),
                    CMD_TEXT,
                ));
            }
            "uptime" => {
                let ticks = crate::arch::x86_64::pit::get_ticks();
                let secs = ticks / 1000;
                self.lines.push((format!("Up {}s ({} kernel ticks @ 1000 Hz)", secs, ticks), CMD_TEXT));
            }
            "ps" | "tasklist" => {
                let tasks = crate::task::scheduler::get_task_list();
                self.lines.push((format!("TID   STATE      CPU   NAME ({} active)", tasks.len()), CMD_ACCENT));
                for (tid, name, state, cpu) in tasks.iter().take(16) {
                    self.lines.push((
                        format!("{:<5} {:<10?} cpu{:<2} {}", tid, state, cpu, name),
                        CMD_TEXT,
                    ));
                }
            }
            "pwd" => {
                self.lines.push((self.cwd.clone(), CMD_TEXT));
            }
            "cd" => {
                let target = self.resolve_path(rest);
                let ok = {
                    let vfs = VFS.lock();
                    vfs.resolve_path(&target).and_then(|n| n.lock().readdir()).is_ok()
                };
                if ok {
                    self.cwd = target;
                } else {
                    self.lines.push((format!("cd: {}: No such directory", target), CMD_ERR));
                }
            }
            "ls" | "dir" => {
                let target = self.resolve_path(rest);
                let entries_res = {
                    let vfs = VFS.lock();
                    vfs.resolve_path(&target).and_then(|n| n.lock().readdir())
                };
                match entries_res {
                    Ok(entries) => {
                        self.lines.push((format!("Directory of {}", target), CMD_ACCENT));
                        for entry in entries.iter().take(48) {
                            let tag = match entry.file_type {
                                crate::fs::vfs::FileType::Directory => "[DIR] ",
                                _ => "[FILE]",
                            };
                            self.lines.push((format!("  {} {}", tag, entry.name), CMD_TEXT));
                        }
                    }
                    Err(e) => {
                        self.lines.push((format!("ls '{}': {}", target, e), CMD_ERR));
                    }
                }
            }
            "cat" | "type" => {
                if rest.is_empty() {
                    self.lines.push((String::from("Usage: cat <path>"), CMD_WARN));
                } else {
                    let target = self.resolve_path(rest);
                    let content = VFS
                        .lock()
                        .open(&target, 0)
                        .and_then(|file| {
                            let mut buf = [0u8; 768];
                            file.read(&mut buf).map(|n| String::from_utf8_lossy(&buf[..n]).into_owned())
                        });
                    match content {
                        Ok(txt) => {
                            for line in txt.lines().take(24) {
                                self.lines.push((String::from(line), CMD_TEXT));
                            }
                        }
                        Err(e) => self.lines.push((format!("cat: {}", e), CMD_ERR)),
                    }
                }
            }
            "volumes" | "df" => {
                let vols_res = {
                    let vfs = VFS.lock();
                    vfs.resolve_path("/Volumes").and_then(|n| n.lock().readdir())
                };
                match vols_res {
                    Ok(vols) => {
                        self.lines.push((String::from("Mounted Filesystem Volumes:"), CMD_ACCENT));
                        for entry in vols {
                            self.lines.push((format!("  /Volumes/{}", entry.name), CMD_GOOD));
                        }
                    }
                    Err(_) => {
                        self.lines.push((String::from("Root filesystem: / (ramfs/tmpfs active)"), CMD_TEXT));
                    }
                }
            }
            "ifconfig" | "ip" | "net" => {
                let net = NET_MANAGER.lock();
                if net.devices.is_empty() {
                    self.lines.push((String::from("No network interfaces registered."), CMD_WARN));
                } else {
                    for (dev, cfg) in &net.devices {
                        let mac = dev.mac_address();
                        let state = if dev.is_link_up() { "UP" } else { "DOWN" };
                        self.lines.push((
                            format!("{}: <{:?}, {}>  IP {}.{}.{}.{}", dev.name(), dev.device_type(), state, cfg.ipv4[0], cfg.ipv4[1], cfg.ipv4[2], cfg.ipv4[3]),
                            CMD_ACCENT,
                        ));
                        self.lines.push((
                            format!("  MAC {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}  TX {} / RX {}", mac[0], mac[1], mac[2], mac[3], mac[4], mac[5], cfg.tx_packets, cfg.rx_packets),
                            CMD_TEXT,
                        ));
                    }
                }
            }
            "wifi" => {
                let mut wparts = rest.splitn(3, ' ');
                let sub = wparts.next().unwrap_or("status").to_ascii_lowercase();
                let arg1 = wparts.next().unwrap_or("").trim();
                let arg2 = wparts.next().unwrap_or("").trim();
                match sub.as_str() {
                    "" | "status" => {
                        let wifi = WIFI_MANAGER.lock();
                        if wifi.is_connected {
                            self.lines.push((
                                format!(
                                    "Wi-Fi Connected: {} (Ch {}, {} Mbps)",
                                    wifi.current_ssid.as_deref().unwrap_or("Unknown"),
                                    wifi.current_channel,
                                    wifi.link_speed_mbps
                                ),
                                CMD_GOOD,
                            ));
                        } else {
                            self.lines.push((
                                format!("Wi-Fi Disconnected ({} known APs)", wifi.scan_results.len()),
                                CMD_WARN,
                            ));
                        }
                    }
                    "scan" => {
                        let mut wifi = WIFI_MANAGER.lock();
                        let aps = wifi.scan().to_vec();
                        drop(wifi);
                        self.lines.push((format!("802.11 Scan found {} networks:", aps.len()), CMD_ACCENT));
                        for ap in aps {
                            let sec = ap.security.as_str();
                            self.lines.push((
                                format!("  {:<22} Ch {:<2} {} dBm [{}]", ap.ssid, ap.channel, ap.rssi, sec),
                                CMD_TEXT,
                            ));
                        }
                    }
                    "connect" => {
                        if arg1.is_empty() {
                            self.lines.push((String::from("Usage: wifi connect <SSID> [passphrase]"), CMD_WARN));
                        } else {
                            let pass = if arg2.is_empty() { None } else { Some(arg2) };
                            let mut wifi = WIFI_MANAGER.lock();
                            match wifi.connect(arg1, pass) {
                                Ok(()) => {
                                    let ch = wifi.current_channel;
                                    let spd = wifi.link_speed_mbps;
                                    self.lines.push((format!("Connected to '{}' (Ch {}, {} Mbps)", arg1, ch, spd), CMD_GOOD));
                                }
                                Err(e) => self.lines.push((format!("wifi connect failed: {}", e), CMD_ERR)),
                            }
                        }
                    }
                    "disconnect" => {
                        WIFI_MANAGER.lock().disconnect();
                        self.lines.push((String::from("Wi-Fi disconnected."), CMD_WARN));
                    }
                    _ => {
                        self.lines.push((String::from("Usage: wifi [status|scan|connect <ssid> [pass]|disconnect]"), CMD_WARN));
                    }
                }
            }
            "font" => {
                let mode = rest.to_ascii_lowercase();
                if mode == "bold" {
                    set_terminal_bold(true);
                    set_ui_font_style(1);
                    self.lines.push((String::from("Font switched to Bold UI & Terminal."), CMD_GOOD));
                } else if mode == "regular" || mode == "mono" {
                    set_terminal_bold(false);
                    set_ui_font_style(0);
                    self.lines.push((String::from("Font switched to Sublime Mono 8x16."), CMD_GOOD));
                } else {
                    let cur = if get_terminal_bold() { "Bold" } else { "Regular 8x16" };
                    self.lines.push((format!("Current Terminal Font: {} (use 'font bold' or 'font regular')", cur), CMD_TEXT));
                }
            }
            "sound" | "chime" => {
                crate::drivers::audio::play_sublime_chime();
                self.lines.push((String::from("Played SublimeOS audio chime via Intel HDA."), CMD_GOOD));
            }
            "reboot" => {
                self.lines.push((String::from("Rebooting SublimeOS..."), CMD_WARN));
                crate::arch::x86_64::acpi::reboot();
            }
            _ => {
                self.lines.push((
                    format!("'{}' is not recognized. Type 'help' for commands.", cmd),
                    CMD_ERR,
                ));
            }
        }
    }

    pub fn render(&mut self, canvas: &mut Canvas, x: i32, y: i32, w: i32, h: i32) {
        canvas.fill_rect(x, y, w, h, CMD_BG);
        let pad_x = 8;
        let max_cols = ((w - pad_x * 2) / FONT_WIDTH).max(16) as usize;
        let line_h = FONT_HEIGHT + 2;
        let max_rows = ((h - 10) / line_h).max(1) as usize;

        // Build wrapped visual lines so text NEVER overflows past the right edge
        let mut visual: Vec<(String, Color)> = Vec::new();
        for (txt, col) in &self.lines {
            push_wrapped_line(&mut visual, txt, *col, max_cols);
        }

        // Active prompt + cursor position calculation
        let prompt_line = format!("{}{}", self.prompt_prefix(), self.input);
        let prompt_chars = prompt_line.chars().count();
        let prompt_start_idx = visual.len();
        push_wrapped_line(&mut visual, &prompt_line, CMD_BRIGHT, max_cols);

        let total = visual.len();
        let start = total.saturating_sub(max_rows);
        let bold = get_terminal_bold();

        let mut curr_y = y + 6;
        for (idx, (line, col)) in visual.iter().enumerate().skip(start) {
            if bold {
                draw_string_bold(canvas, x + pad_x, curr_y, line, *col);
            } else {
                draw_string(canvas, x + pad_x, curr_y, line, *col);
            }
            curr_y += line_h;
            let _ = idx;
        }

        // Draw blinking underscore cursor at exact wrapped prompt tail
        self.cursor_blink = self.cursor_blink.wrapping_add(1);
        if (self.cursor_blink % 30) < 15 {
            let tail_row_offset = prompt_chars / max_cols;
            let tail_col = prompt_chars % max_cols;
            let cursor_abs_row = prompt_start_idx + tail_row_offset;
            if cursor_abs_row >= start && cursor_abs_row < start + max_rows {
                let rel_row = (cursor_abs_row - start) as i32;
                let cx = x + pad_x + (tail_col as i32 * FONT_WIDTH);
                let cy = y + 6 + rel_row * line_h + FONT_HEIGHT - 3;
                canvas.fill_rect(cx, cy, FONT_WIDTH, 2, CMD_ACCENT);
            }
        }
    }
}

fn push_wrapped_line(out: &mut Vec<(String, Color)>, text: &str, color: Color, max_cols: usize) {
    if text.is_empty() {
        out.push((String::new(), color));
        return;
    }
    let mut buf = String::new();
    let mut count = 0usize;
    for ch in text.chars() {
        if count >= max_cols {
            out.push((core::mem::take(&mut buf), color));
            count = 0;
        }
        buf.push(ch);
        count += 1;
    }
    if !buf.is_empty() {
        out.push((buf, color));
    }
}
