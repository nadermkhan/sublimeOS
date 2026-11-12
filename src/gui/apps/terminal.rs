use alloc::string::String;
use alloc::vec::Vec;
use crate::arch::x86_64::pit::get_ticks;
use crate::arch::x86_64::rtc::read_datetime;
use crate::fs::vfs::VFS;
use crate::gui::canvas::Canvas;
use crate::gui::color::Color;
use crate::gui::font::{draw_string, FONT_HEIGHT, FONT_WIDTH};
use crate::mm::heap::HEAP_ALLOCATOR;
use crate::mm::pmm::PMM;

pub struct TerminalApp {
    lines: Vec<(String, Color)>,
    input_buffer: String,
    cursor_timer: u64,
}

impl TerminalApp {
    pub fn new() -> Self {
        let mut app = Self {
            lines: Vec::new(),
            input_buffer: String::new(),
            cursor_timer: 0,
        };

        app.lines.push((
            String::from("Last login: Wed Nov 11 on ttys000"),
            Color::TEXT_MUTED,
        ));
        app.lines.push((
            String::from("CosmicOS (Sublime Kernel 1.0.0 x86_64)"),
            Color::ACCENT_CYAN,
        ));
        app.lines.push((String::new(), Color::TEXT_PRIMARY));

        app
    }

    pub fn handle_char(&mut self, ch: char) {
        match ch {
            '\n' | '\r' => {
                let cmd = core::mem::take(&mut self.input_buffer);
                let mut prompt_line = String::from("cosmos@CosmicOS ~ % ");
                prompt_line.push_str(&cmd);
                self.lines.push((prompt_line, Color::TEXT_PRIMARY));

                self.execute_command(cmd.trim());
            }
            '\x08' => {
                // Backspace
                self.input_buffer.pop();
            }
            c if c >= ' ' && c <= '~' => {
                self.input_buffer.push(c);
            }
            _ => {}
        }
    }

    fn execute_command(&mut self, cmd_line: &str) {
        if cmd_line.is_empty() {
            return;
        }

        let mut parts = cmd_line.split_whitespace();
        let cmd = parts.next().unwrap_or("");
        let arg = parts.next().unwrap_or("");

        match cmd {
            "help" => {
                self.lines.push((String::from("Available CosmosSH commands:"), Color::ACCENT_YELLOW));
                self.lines.push((String::from("  help      - Display this help message"), Color::TEXT_PRIMARY));
                self.lines.push((String::from("  clear     - Clear terminal screen"), Color::TEXT_PRIMARY));
                self.lines.push((String::from("  uname     - Print system and kernel information"), Color::TEXT_PRIMARY));
                self.lines.push((String::from("  uptime    - Show system uptime in seconds and ticks"), Color::TEXT_PRIMARY));
                self.lines.push((String::from("  date      - Display current UTC hardware RTC date & time"), Color::TEXT_PRIMARY));
                self.lines.push((String::from("  mem       - Display physical and heap memory statistics"), Color::TEXT_PRIMARY));
                self.lines.push((String::from("  ps        - List running kernel tasks and threads"), Color::TEXT_PRIMARY));
                self.lines.push((String::from("  ls [path] - List directory contents (e.g. ls /etc, ls /proc)"), Color::TEXT_PRIMARY));
                self.lines.push((String::from("  cat <file>- Read and print file content"), Color::TEXT_PRIMARY));
                self.lines.push((String::from("  echo <msg>- Print text message to terminal"), Color::TEXT_PRIMARY));
                self.lines.push((String::from("  reboot    - Reboot the system via 8042 controller"), Color::TEXT_PRIMARY));
            }
            "clear" => {
                self.lines.clear();
            }
            "sw_vers" => {
                self.lines.push((String::from("ProductName:        SublimeOS"), Color::TEXT_PRIMARY));
                self.lines.push((String::from("ProductVersion:     1.0"), Color::TEXT_PRIMARY));
                self.lines.push((String::from("BuildVersion:       1A01"), Color::TEXT_PRIMARY));
                self.lines.push((String::from("KernelVersion:      KiteKernel 1.0.0"), Color::TEXT_PRIMARY));
            }
            "system_profiler" => {
                let hw = crate::drivers::platform::smbios::get_hardware_telemetry();
                self.lines.push((alloc::format!("Model:  {}", hw.model_name), Color::ACCENT_CYAN));
                self.lines.push((alloc::format!("Ident:  {}", hw.model_identifier), Color::TEXT_PRIMARY));
                self.lines.push((alloc::format!("Chip:   {}", hw.chip), Color::TEXT_PRIMARY));
                self.lines.push((alloc::format!("Cores:  {}", hw.cores_threads), Color::TEXT_PRIMARY));
                self.lines.push((alloc::format!("Memory: {}", hw.memory_summary), Color::TEXT_PRIMARY));
                self.lines.push((alloc::format!("Serial: {}", hw.serial_number), Color::TEXT_MUTED));
            }
            "say" => {
                let rest = cmd_line.strip_prefix("say").unwrap_or("").trim();
                let mut s = String::from("[SpeechSynthesis (Sublime Voice)] \"");
                s.push_str(if rest.is_empty() { "Hello from SublimeOS" } else { rest });
                s.push('"');
                self.lines.push((s, Color::ACCENT_PURPLE));
            }
            "defaults" => {
                self.lines.push((String::from("{ SublimeInterfaceStyle = Dark; SublimeKeyboardLayout = US; }"), Color::TEXT_PRIMARY));
            }
            "uname" => {
                self.lines.push((
                    String::from("SublimeOS sublime.local 1.0.0 KiteKernel Version 1.0.0 x86_64"),
                    Color::ACCENT_GREEN,
                ));
            }
            "uptime" => {
                let ticks = get_ticks();
                let secs = ticks / 1000;
                let mut s = String::from("Uptime: ");
                push_u64(&mut s, secs);
                s.push_str("s (");
                push_u64(&mut s, ticks);
                s.push_str(" timer ticks at 1000 Hz)");
                self.lines.push((s, Color::ACCENT_GREEN));
            }
            "date" => {
                let time = read_datetime();
                let mut s = String::from("UTC Time: ");
                push_u64(&mut s, time.year as u64);
                s.push('-');
                push_2digits(&mut s, time.month);
                s.push('-');
                push_2digits(&mut s, time.day);
                s.push(' ');
                push_2digits(&mut s, time.hour);
                s.push(':');
                push_2digits(&mut s, time.minute);
                s.push(':');
                push_2digits(&mut s, time.second);
                self.lines.push((s, Color::ACCENT_GREEN));
            }
            "mem" => {
                let (total_frames, used_frames) = {
                    let pmm = PMM.lock();
                    (pmm.get_total_frames(), pmm.get_used_frames())
                };
                let mut s1 = String::from("Physical RAM: ");
                push_u64(&mut s1, (used_frames * 4) as u64);
                s1.push_str(" KiB used / ");
                push_u64(&mut s1, (total_frames * 4) as u64);
                s1.push_str(" KiB total (");
                push_u64(&mut s1, (total_frames - used_frames) as u64);
                s1.push_str(" frames free)");
                self.lines.push((s1, Color::ACCENT_CYAN));

                let (_heap_total, heap_alloc, heap_free) = HEAP_ALLOCATOR.stats();
                let mut s2 = String::from("Kernel Heap:  ");
                push_u64(&mut s2, heap_alloc as u64);
                s2.push_str(" bytes allocated, ");
                push_u64(&mut s2, heap_free as u64);
                s2.push_str(" bytes free");
                self.lines.push((s2, Color::ACCENT_CYAN));
            }
            "ps" => {
                self.lines.push((String::from("PID  TID  STATUS   NAME"), Color::ACCENT_YELLOW));
                self.lines.push((String::from("  0    0  RUNNING  [kernel_idle]"), Color::TEXT_PRIMARY));
                self.lines.push((String::from("  1    1  READY    [kmain_worker]"), Color::TEXT_PRIMARY));
                self.lines.push((String::from("  2    2  RUNNING  [cosmos_gui]"), Color::ACCENT_GREEN));
            }
            "ls" => {
                let path = if arg.is_empty() { "/" } else { arg };
                let vfs = VFS.lock();
                match vfs.resolve_path(path).and_then(|node| node.lock().readdir()) {
                    Ok(entries) => {
                        let mut s = String::from("Entries in ");
                        s.push_str(path);
                        s.push(':');
                        self.lines.push((s, Color::ACCENT_YELLOW));
                        for entry in entries {
                            let type_str = match entry.file_type {
                                crate::fs::vfs::FileType::Directory => "[DIR] ",
                                crate::fs::vfs::FileType::Regular => "[FILE]",
                                crate::fs::vfs::FileType::CharDevice => "[DEV] ",
                                _ => "[UNK] ",
                            };
                            let mut item = String::from("  ");
                            item.push_str(type_str);
                            item.push(' ');
                            item.push_str(&entry.name);
                            self.lines.push((item, Color::TEXT_PRIMARY));
                        }
                    }
                    Err(e) => {
                        let mut err_msg = String::from("ls error: ");
                        err_msg.push_str(e);
                        self.lines.push((err_msg, Color::ACCENT_RED));
                    }
                }
            }
            "cat" => {
                if arg.is_empty() {
                    self.lines.push((String::from("Usage: cat <filename>"), Color::ACCENT_YELLOW));
                    return;
                }
                match VFS.lock().open(arg, 0) {
                    Ok(file) => {
                        let mut buf = [0u8; 512];
                        match file.read(&mut buf) {
                            Ok(bytes_read) => {
                                if let Ok(text) = core::str::from_utf8(&buf[..bytes_read]) {
                                    for line in text.lines() {
                                        self.lines.push((String::from(line), Color::TEXT_PRIMARY));
                                    }
                                }
                            }
                            Err(e) => {
                                let mut err_msg = String::from("cat read error: ");
                                err_msg.push_str(e);
                                self.lines.push((err_msg, Color::ACCENT_RED));
                            }
                        }
                    }
                    Err(e) => {
                        let mut err_msg = String::from("cat open error: ");
                        err_msg.push_str(e);
                        self.lines.push((err_msg, Color::ACCENT_RED));
                    }
                }
            }
            "echo" => {
                let rest = cmd_line.strip_prefix("echo").unwrap_or("").trim();
                self.lines.push((String::from(rest), Color::TEXT_PRIMARY));
            }
            "reboot" => {
                self.lines.push((String::from("Rebooting system..."), Color::ACCENT_RED));
                unsafe {
                    crate::arch::x86_64::io::outb(0x64, 0xFE);
                }
            }
            _ => {
                let mut err = String::from("cosmos: command not found: ");
                err.push_str(cmd);
                self.lines.push((err, Color::ACCENT_RED));
            }
        }
    }

    pub fn render(&mut self, canvas: &mut Canvas, x: i32, y: i32, w: i32, h: i32) {
        // Dark background for terminal
        canvas.fill_rect(x, y, w, h, Color::BG_INPUT);

        let max_visible_lines = (h / FONT_HEIGHT) as usize;
        let total_lines = self.lines.len() + 1; // +1 for current input line

        let start_line = if total_lines > max_visible_lines {
            total_lines - max_visible_lines
        } else {
            0
        };

        let mut draw_y = y + 4;

        for line_idx in start_line..self.lines.len() {
            let (ref text, color) = self.lines[line_idx];
            draw_string(canvas, x + 6, draw_y, text, color);
            draw_y += FONT_HEIGHT;
        }

        // Draw current prompt & input line
        if draw_y + FONT_HEIGHT <= y + h {
            draw_string(canvas, x + 6, draw_y, "cosmos@CosmicOS ~ % ", Color::ACCENT_GREEN);
            let prompt_w = 20 * FONT_WIDTH;
            draw_string(canvas, x + 6 + prompt_w, draw_y, &self.input_buffer, Color::TEXT_PRIMARY);

            // Blinking cursor
            self.cursor_timer = self.cursor_timer.wrapping_add(1);
            if (self.cursor_timer % 30) < 15 {
                let cursor_x = x + 6 + prompt_w + (self.input_buffer.len() as i32 * FONT_WIDTH);
                canvas.fill_rect(cursor_x, draw_y + 13, FONT_WIDTH, 2, Color::ACCENT_GREEN);
            }
        }
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
