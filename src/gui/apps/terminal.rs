use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use libgui::event::{GuiEvent, MouseButton};
use libgui::gfx::Rect;
use libgui::widget::{ScrollBar, ScrollBarState, ScrollOrientation};
use crate::arch::x86_64::rtc::get_system_datetime;
use crate::fs::vfs::VFS;
use crate::gui::canvas::Canvas;
use crate::gui::color::Color;
use crate::gui::font::{
    draw_string, draw_string_bold, draw_ui_string, get_terminal_bold, set_terminal_bold,
    set_ui_font_style, FONT_HEIGHT, FONT_WIDTH,
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
    pub cursor_pos: usize,
    pub history: Vec<String>,
    pub history_idx: Option<usize>,
    pub draft_input: String,
    pub cwd: String,
    pub cursor_blink: u64,
    pub scroll_offset: usize,
    pub vbar: ScrollBarState,
    pub context_menu: Option<(i32, i32)>,
    pub ctx_hover: Option<usize>,
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
            String::from("Type 'help' for commands, Up/Down for history, Tab to complete."),
            CMD_TEXT,
        ));
        lines.push((String::new(), CMD_TEXT));

        Self {
            lines,
            input: String::new(),
            cursor_pos: 0,
            history: Vec::new(),
            history_idx: None,
            draft_input: String::new(),
            cwd: String::from("/"),
            cursor_blink: 0,
            scroll_offset: 0,
            vbar: ScrollBarState::new(0, 0, 12),
            context_menu: None,
            ctx_hover: None,
        }
    }

    const SB_W: i32 = 14;

    fn viewport_metrics(&self, x: i32, y: i32, w: i32, h: i32) -> (Rect, usize, usize, usize) {
        let pad_x = 8;
        let max_cols = ((w - pad_x * 2 - Self::SB_W) / FONT_WIDTH).max(16) as usize;
        let line_h = FONT_HEIGHT + 2;
        let max_rows = ((h - 10) / line_h).max(1) as usize;
        let mut total = 0usize;
        for (txt, _) in &self.lines {
            total += wrapped_row_count(txt, max_cols);
        }
        let prompt_chars = self.prompt_prefix().chars().count() + self.input.chars().count();
        total += if prompt_chars == 0 {
            1
        } else {
            (prompt_chars + max_cols - 1) / max_cols
        };
        let max_scroll = total.saturating_sub(max_rows);
        let sb_rect = Rect::new(x + w - Self::SB_W, y, Self::SB_W, h.max(12));
        (sb_rect, max_cols, max_rows, max_scroll)
    }

    fn handle_vbar_event(&mut self, ix: i32, iy: i32, iw: i32, ih: i32, event: GuiEvent) -> bool {
        let (sb_rect, _, max_rows, max_scroll) = self.viewport_metrics(ix, iy, iw, ih);
        self.scroll_offset = self.scroll_offset.min(max_scroll);
        let bar_val = max_scroll.saturating_sub(self.scroll_offset) as i32;
        self.vbar.sync_range(bar_val, max_scroll as i32, max_rows as i32);
        let (res, changed) =
            ScrollBar::handle_event(&mut self.vbar, sb_rect, ScrollOrientation::Vertical, event);
        if changed {
            self.scroll_offset = max_scroll.saturating_sub(self.vbar.value.max(0) as usize);
        }
        changed || res.needs_redraw()
    }

    pub fn paste_clipboard(&mut self) {
        let clip = libgui::system::SystemClipboard::get_text();
        for ch in clip.chars() {
            if ch >= ' ' && ch <= '~' && self.input.len() < 120 {
                self.input.insert(self.cursor_pos, ch);
                self.cursor_pos += 1;
            }
        }
        self.scroll_offset = 0;
    }

    pub fn copy_last_line(&self) {
        let text = if !self.input.is_empty() {
            self.input.as_str()
        } else {
            self.lines
                .iter()
                .rev()
                .find(|(s, _)| !s.is_empty())
                .map(|(s, _)| s.as_str())
                .unwrap_or("")
        };
        if !text.is_empty() {
            libgui::system::SystemClipboard::set_text(text);
        }
    }

    pub fn handle_mouse_move(&mut self, px: i32, py: i32, ix: i32, iy: i32, iw: i32, ih: i32) -> bool {
        let mut changed = false;
        if let Some((cx, cy)) = self.context_menu {
            let cw = 136;
            let row_h = 22;
            let ch = row_h * 4 + 4;
            let next = if px >= cx && px < cx + cw && py >= cy + 2 && py < cy + ch - 2 {
                Some(((py - cy - 2) / row_h) as usize)
            } else {
                None
            };
            if next != self.ctx_hover {
                self.ctx_hover = next;
                changed = true;
            }
        }
        if self.handle_vbar_event(ix, iy, iw, ih, GuiEvent::MouseMove { x: px, y: py }) {
            changed = true;
        }
        changed
    }

    pub fn handle_mouse_up(&mut self, px: i32, py: i32, ix: i32, iy: i32, iw: i32, ih: i32) -> bool {
        self.handle_vbar_event(
            ix,
            iy,
            iw,
            ih,
            GuiEvent::MouseUp {
                x: px,
                y: py,
                button: MouseButton::Left,
            },
        )
    }

    pub fn handle_right_click(&mut self, px: i32, py: i32, ix: i32, iy: i32, iw: i32, ih: i32) {
        if px >= ix && px < ix + iw && py >= iy && py < iy + ih {
            let cx = px.min(ix + iw - 138).max(ix + 2);
            let cy = py.min(iy + ih - 94).max(iy + 2);
            self.context_menu = Some((cx, cy));
            self.ctx_hover = None;
        } else {
            self.context_menu = None;
        }
    }

    pub fn handle_click(&mut self, px: i32, py: i32, ix: i32, iy: i32, iw: i32, ih: i32) {
        if let Some((cx, cy)) = self.context_menu {
            let cw = 136;
            let row_h = 22;
            let ch = row_h * 4 + 4;
            if px >= cx && px < cx + cw && py >= cy + 2 && py < cy + ch - 2 {
                let idx = ((py - cy - 2) / row_h) as usize;
                match idx {
                    0 => self.paste_clipboard(),
                    1 => self.copy_last_line(),
                    2 => {
                        self.lines.clear();
                        self.scroll_offset = 0;
                    }
                    3 => set_terminal_bold(!get_terminal_bold()),
                    _ => {}
                }
            }
            self.context_menu = None;
            return;
        }

        let _ = self.handle_vbar_event(
            ix,
            iy,
            iw,
            ih,
            GuiEvent::MouseDown {
                x: px,
                y: py,
                button: MouseButton::Left,
            },
        );
    }

    pub fn handle_wheel(&mut self, delta_y: i32) -> bool {
        if delta_y == 0 {
            return false;
        }
        let max_scroll = self.lines.len().saturating_sub(4);
        let prev = self.scroll_offset;
        if delta_y > 0 {
            self.scroll_offset = (self.scroll_offset + (delta_y as usize) * 3).min(max_scroll);
        } else {
            self.scroll_offset = self.scroll_offset.saturating_sub((-delta_y as usize) * 3);
        }
        self.scroll_offset != prev
    }

    pub fn prompt_prefix(&self) -> String {
        format!("sublime@SublimeOS:{}$ ", self.cwd)
    }

    pub fn handle_char(&mut self, ch: char) {
        self.cursor_pos = self.cursor_pos.min(self.input.len());
        match ch {
            '\x03' => {
                // Ctrl+C: abort current prompt line cleanly
                let line = format!("{}{}^C", self.prompt_prefix(), self.input);
                self.lines.push((line, CMD_WARN));
                self.input.clear();
                self.cursor_pos = 0;
                self.history_idx = None;
                self.scroll_offset = 0;
            }
            '\x0C' => {
                // Ctrl+L: clear terminal screen
                self.lines.clear();
                self.scroll_offset = 0;
            }
            '\t' => {
                // Tab: autocomplete built-in command prefix
                let trimmed = self.input.trim_start();
                if !trimmed.is_empty() && !trimmed.contains(' ') {
                    const CMDS: &[&str] = &[
                        "help", "clear", "cls", "ver", "uname", "sysinfo", "systeminfo",
                        "ps", "tasklist", "mem", "ls", "dir", "cd", "cat", "type",
                        "volumes", "df", "ifconfig", "net", "wifi", "font", "sound",
                        "run", "date", "uptime", "echo", "reboot",
                    ];
                    let lower = trimmed.to_ascii_lowercase();
                    if let Some(m) = CMDS.iter().find(|c| c.starts_with(&lower)) {
                        self.input = format!("{} ", m);
                        self.cursor_pos = self.input.len();
                    }
                }
            }
            '\n' | '\r' => {
                let cmd_line = self.input.clone();
                let prompt = format!("{}{}", self.prompt_prefix(), cmd_line);
                self.lines.push((prompt, CMD_BRIGHT));
                let trimmed = cmd_line.trim();
                if !trimmed.is_empty() {
                    if self.history.last().map(String::as_str) != Some(trimmed) {
                        self.history.push(String::from(trimmed));
                        if self.history.len() > 64 {
                            self.history.remove(0);
                        }
                    }
                }
                self.input.clear();
                self.cursor_pos = 0;
                self.history_idx = None;
                self.scroll_offset = 0;
                self.execute_command(trimmed);
            }
            '\x08' => {
                if self.cursor_pos > 0 && self.cursor_pos <= self.input.len() {
                    self.input.remove(self.cursor_pos - 1);
                    self.cursor_pos -= 1;
                }
                self.scroll_offset = 0;
            }
            c if c >= ' ' && c <= '~' => {
                if self.input.len() < 120 {
                    self.input.insert(self.cursor_pos, c);
                    self.cursor_pos += 1;
                }
                self.scroll_offset = 0;
            }
            _ => {}
        }

        if self.lines.len() > 160 {
            let drain_count = self.lines.len() - 160;
            self.lines.drain(0..drain_count);
        }
    }

    pub fn handle_key(
        &mut self,
        key: libgui::event::KeyCode,
        ch: Option<char>,
        mods: libgui::event::Modifiers,
    ) {
        self.cursor_pos = self.cursor_pos.min(self.input.len());
        if mods.ctrl {
            match ch {
                Some('v') | Some('V') => {
                    self.paste_clipboard();
                    return;
                }
                Some('c') | Some('C') => {
                    self.copy_last_line();
                    return;
                }
                _ => {}
            }
        }
        match key {
            libgui::event::KeyCode::Escape => {
                self.context_menu = None;
            }
            libgui::event::KeyCode::Left => {
                self.cursor_pos = self.cursor_pos.saturating_sub(1);
            }
            libgui::event::KeyCode::Right => {
                if self.cursor_pos < self.input.len() {
                    self.cursor_pos += 1;
                }
            }
            libgui::event::KeyCode::Home => {
                self.cursor_pos = 0;
            }
            libgui::event::KeyCode::End => {
                self.cursor_pos = self.input.len();
            }
            libgui::event::KeyCode::Delete => {
                if self.cursor_pos < self.input.len() {
                    self.input.remove(self.cursor_pos);
                }
            }
            libgui::event::KeyCode::Up => {
                if self.history.is_empty() {
                    return;
                }
                let next_idx = match self.history_idx {
                    None => {
                        self.draft_input = self.input.clone();
                        self.history.len() - 1
                    }
                    Some(idx) => idx.saturating_sub(1),
                };
                self.history_idx = Some(next_idx);
                self.input = self.history[next_idx].clone();
                self.cursor_pos = self.input.len();
            }
            libgui::event::KeyCode::Down => {
                if let Some(idx) = self.history_idx {
                    if idx + 1 < self.history.len() {
                        let next_idx = idx + 1;
                        self.history_idx = Some(next_idx);
                        self.input = self.history[next_idx].clone();
                    } else {
                        self.history_idx = None;
                        self.input = self.draft_input.clone();
                    }
                    self.cursor_pos = self.input.len();
                }
            }
            libgui::event::KeyCode::PageUp => {
                let _ = self.handle_wheel(4);
            }
            libgui::event::KeyCode::PageDown => {
                let _ = self.handle_wheel(-4);
            }
            libgui::event::KeyCode::Enter => self.handle_char('\n'),
            libgui::event::KeyCode::Backspace => self.handle_char('\x08'),
            libgui::event::KeyCode::Tab => self.handle_char('\t'),
            _ => {
                if let Some(c) = ch {
                    self.handle_char(c);
                }
            }
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

    pub fn execute_command(&mut self, raw_cmd: &str) {
        let trimmed_cmd = raw_cmd.trim();
        if trimmed_cmd.is_empty() {
            return;
        }

        let mut parts = trimmed_cmd.splitn(2, ' ');
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
                self.lines.push((String::from("  apps                List installed .app & ELF programs"), CMD_TEXT));
                self.lines.push((String::from("  run <prog> [args]   Launch or run Ring-3 ELF binary"), CMD_TEXT));
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
            "apps" => {
                let apps = crate::user::apps::discover_apps();
                self.lines.push((format!("Discovered {} app(s) & executables:", apps.len()), CMD_BRIGHT));
                for app in apps {
                    let target = if app.exec.is_empty() { "builtin" } else { app.exec.as_str() };
                    self.lines.push((format!("  {:<18} [{}]  {}", app.name, app.category, target), CMD_TEXT));
                }
            }
            "run" | "exec" => {
                if rest.is_empty() {
                    self.lines.push((String::from("Usage: run </bin/prog | studio | paint | hello>"), CMD_WARN));
                } else {
                    let parts: Vec<&str> = rest.split_whitespace().collect();
                    if !self.run_elf_command(parts[0], &parts) {
                        self.lines.push((format!("Failed to launch Ring-3 binary: {}", parts[0]), CMD_ERR));
                    }
                }
            }
            "reboot" => {
                self.lines.push((String::from("Rebooting SublimeOS..."), CMD_WARN));
                crate::arch::x86_64::acpi::reboot();
            }
            _ => {
                let parts: Vec<&str> = raw_cmd.trim().split_whitespace().collect();
                if !self.run_elf_command(cmd, &parts) {
                    self.lines.push((
                        format!("'{}' is not recognized. Type 'help' or 'apps'.", cmd),
                        CMD_ERR,
                    ));
                }
            }
        }
    }

    fn run_elf_command(&mut self, prog: &str, args: &[&str]) -> bool {
        let Some(resolved) = crate::user::apps::resolve_executable_path(prog, &self.cwd) else {
            return false;
        };
        if crate::user::apps::elf_is_gui_client(&resolved) {
            if crate::gui::spawn_user_binary(&resolved, args) {
                self.lines.push((format!("Launched Ring-3 GUI app: {}", resolved), CMD_GOOD));
                return true;
            }
            return false;
        }
        let Some(bytes) = crate::user::apps::read_vfs_binary(&resolved) else {
            return false;
        };
        match crate::task::user::spawn_user_binary_capture(&bytes, args, &self.cwd, 2000) {
            Ok((code, output)) => {
                for line in output.split('\n') {
                    let clean = line.trim_end_matches('\r');
                    if !clean.is_empty() {
                        self.lines.push((String::from(clean), CMD_TEXT));
                    }
                }
                if code != 0 {
                    self.lines.push((format!("[exit status {}]", code), CMD_WARN));
                }
                true
            }
            Err(e) => {
                self.lines.push((format!("Execution error ({}): {}", resolved, e), CMD_ERR));
                true
            }
        }
    }

    pub fn render(&mut self, canvas: &mut Canvas, x: i32, y: i32, w: i32, h: i32) {
        canvas.fill_rect(x, y, w, h, CMD_BG);
        let pad_x = 8;
        let line_h = FONT_HEIGHT + 2;
        let (sb_rect, max_cols, max_rows, max_scroll) = self.viewport_metrics(x, y, w, h);
        self.scroll_offset = self.scroll_offset.min(max_scroll);

        // Build wrapped visual lines so text NEVER overflows past the right edge or under scrollbar
        let mut visual: Vec<(String, Color)> = Vec::new();
        for (txt, col) in &self.lines {
            push_wrapped_line(&mut visual, txt, *col, max_cols);
        }

        // Active prompt + exact cursor position calculation
        let prefix = self.prompt_prefix();
        let prompt_line = format!("{}{}", prefix, self.input);
        let cursor_chars = prefix.chars().count() + self.cursor_pos.min(self.input.len());
        let prompt_start_idx = visual.len();
        push_wrapped_line(&mut visual, &prompt_line, CMD_BRIGHT, max_cols);

        let total = visual.len();
        let actual_max = total.saturating_sub(max_rows);
        self.scroll_offset = self.scroll_offset.min(actual_max);
        let bottom = total.saturating_sub(self.scroll_offset);
        let start = bottom.saturating_sub(max_rows);
        let bold = get_terminal_bold();

        let mut curr_y = y + 6;
        for (idx, (line, col)) in visual[start..bottom].iter().enumerate() {
            if bold {
                draw_string_bold(canvas, x + pad_x, curr_y, line, *col);
            } else {
                draw_string(canvas, x + pad_x, curr_y, line, *col);
            }
            curr_y += line_h;
            let _ = idx;
        }

        // Draw blinking underscore cursor at exact wrapped edit position
        self.cursor_blink = self.cursor_blink.wrapping_add(1);
        if (self.cursor_blink % 30) < 15 && self.scroll_offset == 0 {
            let tail_row_offset = cursor_chars / max_cols;
            let tail_col = cursor_chars % max_cols;
            let cursor_abs_row = prompt_start_idx + tail_row_offset;
            if cursor_abs_row >= start && cursor_abs_row < start + max_rows {
                let rel_row = (cursor_abs_row - start) as i32;
                let cx = x + pad_x + (tail_col as i32 * FONT_WIDTH);
                let cy = y + 6 + rel_row * line_h + FONT_HEIGHT - 3;
                canvas.fill_rect(cx, cy, FONT_WIDTH, 2, CMD_ACCENT);
            }
        }

        let bar_val = actual_max.saturating_sub(self.scroll_offset) as i32;
        self.vbar.sync_range(bar_val, actual_max as i32, max_rows as i32);
        let theme = libgui::gfx::current_theme();
        canvas.with_surface(|surf| {
            ScrollBar::draw(surf, sb_rect, ScrollOrientation::Vertical, self.vbar, &theme);
        });

        if let Some((cx, cy)) = self.context_menu {
            let cw = 136;
            let row_h = 22;
            let ch = row_h * 4 + 4;
            canvas.fill_rect(cx, cy, cw, ch, Color::from_libgui(theme.face_light));
            canvas.draw_rect(cx, cy, cw, ch, Color::from_libgui(theme.ctrl_border));
            let items = ["Paste", "Copy Last Line", "Clear Screen", "Toggle Bold"];
            for (i, label) in items.iter().enumerate() {
                let ry = cy + 2 + i as i32 * row_h;
                let hovered = self.ctx_hover == Some(i);
                if hovered {
                    canvas.fill_rect(cx + 2, ry, cw - 4, row_h, Color::from_libgui(theme.selection));
                }
                let tc = Color::from_libgui(if hovered { theme.selection_text } else { theme.text_primary });
                draw_ui_string(canvas, cx + 10, ry + 3, label, tc);
            }
        }
    }
}

fn wrapped_row_count(text: &str, max_cols: usize) -> usize {
    if text.is_empty() {
        1
    } else {
        let len = text.chars().count();
        (len + max_cols - 1) / max_cols
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
