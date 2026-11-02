use core::sync::atomic::{AtomicU64, Ordering};
use crate::arch::x86_64::idt::{register_irq_handler, InterruptContext};
use crate::arch::x86_64::io::outb;
use crate::arch::x86_64::pic::unmask;

const PIT_CHANNEL_0: u16 = 0x40;
const PIT_COMMAND: u16 = 0x43;
const PIT_BASE_FREQUENCY: u32 = 1193182;

static TICKS: AtomicU64 = AtomicU64::new(0);

/// Initialize the 8254 PIT to fire at the requested frequency (Hz).
pub fn init(frequency_hz: u32) {
    let divisor = (PIT_BASE_FREQUENCY / frequency_hz) as u16;

    unsafe {
        // Channel 0, Lobyte/Hibyte access, Mode 3 (Square Wave), Binary mode
        outb(PIT_COMMAND, 0x36);
        outb(PIT_CHANNEL_0, (divisor & 0xFF) as u8);
        outb(PIT_CHANNEL_0, ((divisor >> 8) & 0xFF) as u8);
    }

    // Register IRQ 0 handler
    register_irq_handler(0, timer_irq_handler);
    unmask(0);
}

fn timer_irq_handler(ctx: &mut InterruptContext) {
    let ticks = TICKS.fetch_add(1, Ordering::Relaxed) + 1;

    // Notify multitasking scheduler on timer tick
    crate::task::scheduler::on_timer_tick(ticks, ctx);
}

/// Get total timer ticks elapsed since boot.
pub fn get_ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}

/// Simple busy sleep in milliseconds (based on PIT ticks at 1000 Hz).
pub fn sleep_ms(ms: u64) {
    let target = get_ticks() + ms;
    while get_ticks() < target {
        core::hint::spin_loop();
    }
}
