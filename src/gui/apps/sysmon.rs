use alloc::string::String;
use crate::arch::x86_64::pit::get_ticks;
use crate::gui::canvas::Canvas;
use crate::gui::color::Color;
use crate::gui::font::{draw_char, draw_string};
use crate::mm::heap::HEAP_ALLOCATOR;
use crate::mm::pmm::PMM;

pub struct SysmonApp;

impl SysmonApp {
    pub fn new() -> Self {
        Self
    }

    pub fn render(&self, canvas: &mut Canvas, x: i32, y: i32, w: i32, h: i32) {
        // CosmicOS Activity Monitor dark canvas background
        canvas.fill_rect(x, y, w, h, Color::argb(255, 26, 28, 38));

        let mut draw_y = y + 8;
        let left_x = x + 12;
        let content_w = w - 24;

        // 1. Activity Monitor Tabs Header
        let tabs = ["CPU", "Unified Memory", "Energy", "Disk", "Network"];
        let mut tab_x = left_x;
        for (i, &tab) in tabs.iter().enumerate() {
            let tw = (tab.len() as i32 * 8) + 16;
            if i == 0 {
                canvas.fill_rounded_rect(tab_x, draw_y, tw, 22, 5, Color::ACCENT_BLUE);
                draw_string(canvas, tab_x + 8, draw_y + 3, tab, Color::WHITE);
            } else {
                canvas.fill_rounded_rect(tab_x, draw_y, tw, 22, 5, Color::argb(50, 255, 255, 255));
                draw_string(canvas, tab_x + 8, draw_y + 3, tab, Color::TEXT_MUTED);
            }
            tab_x += tw + 6;
        }
        draw_y += 28;

        // 2. Process / Mach Tasks Table Header
        canvas.fill_rounded_rect(left_x, draw_y, content_w, 20, 3, Color::argb(45, 255, 255, 255));
        draw_string(canvas, left_x + 6, draw_y + 2, "Process Name", Color::TEXT_MUTED);
        draw_string(canvas, left_x + 190, draw_y + 2, "% CPU", Color::TEXT_MUTED);
        draw_string(canvas, left_x + 250, draw_y + 2, "Threads", Color::TEXT_MUTED);
        draw_string(canvas, left_x + 330, draw_y + 2, "Memory", Color::TEXT_MUTED);
        draw_string(canvas, left_x + 410, draw_y + 2, "Kind", Color::TEXT_MUTED);
        draw_y += 24;

        // Process entries
        let processes = [
            ("kernel_task", "0.8%", "2", "2.8 MB", "Sublime Core"),
            ("launchd", "0.1%", "1", "340 KB", "System"),
            ("WindowServer", "1.6%", "1", "1.6 MB", "Aqua / GOP"),
            ("Terminal", "0.2%", "1", "580 KB", "zsh / ttys000"),
            ("Finder", "0.3%", "1", "820 KB", "Native App"),
            ("Activity Monitor", "0.5%", "1", "940 KB", "Native App"),
        ];

        for (idx, (pname, cpu, th, mem, kind)) in processes.iter().enumerate() {
            if draw_y + 18 > y + h - 110 {
                break;
            }
            if idx % 2 == 1 {
                canvas.fill_rect(left_x, draw_y - 2, content_w, 18, Color::argb(20, 255, 255, 255));
            }
            draw_char(canvas, left_x + 6, draw_y + 1, '•', Color::ACCENT_CYAN);
            draw_string(canvas, left_x + 20, draw_y + 1, pname, Color::WHITE);
            draw_string(canvas, left_x + 190, draw_y + 1, cpu, Color::ACCENT_GREEN);
            draw_string(canvas, left_x + 265, draw_y + 1, th, Color::TEXT_PRIMARY);
            draw_string(canvas, left_x + 330, draw_y + 1, mem, Color::TEXT_PRIMARY);
            draw_string(canvas, left_x + 410, draw_y + 1, kind, Color::TEXT_MUTED);
            draw_y += 20;
        }

        // 3. Bottom Unified Memory & Pressure Dashboard Card
        let card_y = y + h - 104;
        canvas.fill_rounded_rect(left_x, card_y, content_w, 96, 6, Color::argb(40, 0, 0, 0));
        canvas.draw_rounded_rect(left_x, card_y, content_w, 96, 6, Color::argb(50, 255, 255, 255));

        // Memory Pressure Meter
        let (total_frames, used_frames) = {
            let pmm = PMM.lock();
            (pmm.get_total_frames(), pmm.get_used_frames())
        };
        let used_kib = used_frames * 4;
        let total_kib = total_frames * 4;
        let percent = if total_frames > 0 {
            (used_frames * 100) / total_frames
        } else {
            0
        };

        let (_total_heap, heap_alloc, _heap_free) = HEAP_ALLOCATOR.stats();
        let ticks = get_ticks();
        let secs = ticks / 1000;

        // Left section: Memory Pressure Meter
        draw_string(canvas, left_x + 12, card_y + 8, "Memory Pressure:", Color::TEXT_PRIMARY);
        draw_string(
            canvas,
            left_x + 145,
            card_y + 8,
            if percent < 60 { "Normal" } else { "High" },
            if percent < 60 { Color::ACCENT_GREEN } else { Color::ACCENT_RED },
        );

        let meter_w = 190;
        let meter_h = 14;
        canvas.fill_rounded_rect(left_x + 12, card_y + 26, meter_w, meter_h, 3, Color::BG_CARD);
        canvas.draw_rounded_rect(left_x + 12, card_y + 26, meter_w, meter_h, 3, Color::WINDOW_BORDER);

        let fill_w = ((meter_w - 4) * percent.max(3) as i32) / 100;
        canvas.draw_gradient_h(
            left_x + 14,
            card_y + 28,
            fill_w,
            meter_h - 4,
            Color::ACCENT_GREEN,
            Color::ACCENT_CYAN,
        );

        let mut pressure_sub = String::from("Physical RAM: ");
        push_u64(&mut pressure_sub, (total_kib / 1024) as u64);
        pressure_sub.push_str(" MB Unified");
        draw_string(canvas, left_x + 12, card_y + 46, &pressure_sub, Color::TEXT_MUTED);

        // Middle section: Detailed Memory Numbers
        let mid_x = left_x + 225;
        let mut app_mem_str = String::from("Memory Used: ");
        push_u64(&mut app_mem_str, (used_kib / 1024) as u64);
        app_mem_str.push_str(" MB");
        draw_string(canvas, mid_x, card_y + 8, &app_mem_str, Color::WHITE);

        let mut wired_str = String::from("Wired Memory: ");
        push_u64(&mut wired_str, ((heap_alloc / 1024) / 1024) as u64);
        wired_str.push('.');
        push_u64(&mut wired_str, (((heap_alloc / 1024) % 1024) / 100) as u64);
        wired_str.push_str(" MB");
        draw_string(canvas, mid_x, card_y + 28, &wired_str, Color::TEXT_PRIMARY);

        let cached_str = "Compressed: 0 KB (Optimized)";
        draw_string(canvas, mid_x, card_y + 48, cached_str, Color::TEXT_MUTED);

        // Right section: CPU & Mach Status
        let info_x = left_x + 395;
        let mut cpu_uptime = String::from("Uptime: ");
        push_u64(&mut cpu_uptime, secs);
        cpu_uptime.push_str("s | 1000 Hz");
        draw_string(canvas, info_x, card_y + 8, &cpu_uptime, Color::ACCENT_CYAN);

        let threads_str = "CPU Threads: 7";
        draw_string(canvas, info_x, card_y + 28, threads_str, Color::TEXT_PRIMARY);

        draw_string(canvas, info_x, card_y + 48, "Sublime Kernel 1.0", Color::ACCENT_PURPLE);
    }
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
