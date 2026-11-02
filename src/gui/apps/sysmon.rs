use alloc::string::String;
use crate::arch::x86_64::pit::get_ticks;
use crate::gui::canvas::Canvas;
use crate::gui::color::Color;
use crate::gui::font::{draw_string, FONT_HEIGHT};
use crate::mm::heap::HEAP_ALLOCATOR;
use crate::mm::pmm::PMM;

pub struct SysmonApp;

impl SysmonApp {
    pub fn new() -> Self {
        Self
    }

    pub fn render(&self, canvas: &mut Canvas, x: i32, y: i32, w: i32, h: i32) {
        // Background card
        canvas.fill_rect(x, y, w, h, Color::BG_INPUT);

        let mut draw_y = y + 10;
        let left_x = x + 12;

        // 1. Header: CPU & Uptime
        let ticks = get_ticks();
        let secs = ticks / 1000;
        let mut cpu_str = String::from("CPU & Uptime: ");
        push_u64(&mut cpu_str, secs);
        cpu_str.push_str("s | Ticks: ");
        push_u64(&mut cpu_str, ticks);
        cpu_str.push_str(" (PIT 1000 Hz)");
        draw_string(canvas, left_x, draw_y, &cpu_str, Color::ACCENT_CYAN);
        draw_y += FONT_HEIGHT + 8;

        // 2. Physical Memory Section
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

        let mut ram_title = String::from("Physical RAM: ");
        push_u64(&mut ram_title, used_kib as u64);
        ram_title.push_str(" KiB / ");
        push_u64(&mut ram_title, total_kib as u64);
        ram_title.push_str(" KiB (");
        push_u64(&mut ram_title, percent as u64);
        ram_title.push_str("%)");
        draw_string(canvas, left_x, draw_y, &ram_title, Color::TEXT_PRIMARY);
        draw_y += FONT_HEIGHT + 4;

        // Progress bar for RAM
        let bar_w = w - 24;
        let bar_h = 16;
        canvas.fill_rounded_rect(left_x, draw_y, bar_w, bar_h, 4, Color::BG_CARD);
        canvas.draw_rounded_rect(left_x, draw_y, bar_w, bar_h, 4, Color::WINDOW_BORDER);

        let fill_w = ((bar_w - 4) * percent as i32) / 100;
        if fill_w > 0 {
            canvas.draw_gradient_h(
                left_x + 2,
                draw_y + 2,
                fill_w,
                bar_h - 4,
                Color::ACCENT_CYAN,
                Color::ACCENT_GREEN,
            );
        }
        draw_y += bar_h + 10;

        // 3. Kernel Heap Section
        let (_total, heap_alloc, heap_free) = HEAP_ALLOCATOR.stats();
        let mut heap_str = String::from("Kernel Heap:  ");
        push_u64(&mut heap_str, (heap_alloc / 1024) as u64);
        heap_str.push_str(" KiB allocated | ");
        push_u64(&mut heap_str, (heap_free / 1024) as u64);
        heap_str.push_str(" KiB free");
        draw_string(canvas, left_x, draw_y, &heap_str, Color::TEXT_PRIMARY);
        draw_y += FONT_HEIGHT + 10;

        // Divider
        canvas.fill_rect(left_x, draw_y, bar_w, 1, Color::WINDOW_BORDER);
        draw_y += 8;

        // 4. Subsystems Status Table
        draw_string(canvas, left_x, draw_y, "Active Kernel Subsystems:", Color::ACCENT_YELLOW);
        draw_y += FONT_HEIGHT + 4;

        let subsystems = [
            ("[✓] PMM Physical Allocator (128 MB managed)", Color::ACCENT_GREEN),
            ("[✓] VMM 4-Level Paging Engine (Higher-half)", Color::ACCENT_GREEN),
            ("[✓] Heap Allocator (Block Coalescing)", Color::ACCENT_GREEN),
            ("[✓] Scheduler (Preemptive Round-Robin)", Color::ACCENT_GREEN),
            ("[✓] VFS (RamFS / DevFS / ProcFS)", Color::ACCENT_GREEN),
            ("[✓] Bochs VBE Display (1024x768x32bpp)", Color::ACCENT_GREEN),
            ("[✓] Hardware IRQ: PIT (32), KBD (33), MOUSE (44)", Color::ACCENT_GREEN),
        ];

        for (desc, col) in subsystems.iter() {
            if draw_y + FONT_HEIGHT > y + h - 6 {
                break;
            }
            draw_string(canvas, left_x + 4, draw_y, desc, *col);
            draw_y += FONT_HEIGHT + 2;
        }
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
