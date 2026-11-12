use crate::arch::x86_64::idt::{register_irq_handler, InterruptContext};
use crate::arch::x86_64::io::{inb, outb};
use crate::sync::IrqSafeSpinlock;
use crate::kinfo;

const PS2_DATA_PORT: u16 = 0x60;
const PS2_STATUS_PORT: u16 = 0x64;
const PS2_COMMAND_PORT: u16 = 0x64;

pub struct MouseState {
    pub x: i32,
    pub y: i32,
    pub left_button: bool,
    pub right_button: bool,
    pub middle_button: bool,
    pub screen_width: i32,
    pub screen_height: i32,
}

impl MouseState {
    pub const fn new() -> Self {
        Self {
            x: 512,
            y: 384,
            left_button: false,
            right_button: false,
            middle_button: false,
            screen_width: 1024,
            screen_height: 768,
        }
    }
}

pub static MOUSE: IrqSafeSpinlock<MouseState> = IrqSafeSpinlock::new(MouseState::new());

struct MouseDriverInternal {
    packet: [u8; 3],
    cycle: u8,
}

static INTERNAL_MOUSE: IrqSafeSpinlock<MouseDriverInternal> = IrqSafeSpinlock::new(MouseDriverInternal {
    packet: [0; 3],
    cycle: 0,
});

unsafe fn mouse_wait_write() -> bool {
    for _ in 0..100_000 {
        let st = inb(PS2_STATUS_PORT);
        if st == 0xFF {
            return false; // No 8042 PS/2 controller on MacBookPro8,1/8,2
        }
        if (st & 0x02) == 0 {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

unsafe fn mouse_wait_read() -> bool {
    for _ in 0..100_000 {
        let st = inb(PS2_STATUS_PORT);
        if st == 0xFF {
            return false;
        }
        if (st & 0x01) != 0 {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

unsafe fn mouse_write(byte: u8) {
    if !mouse_wait_write() {
        return;
    }
    outb(PS2_COMMAND_PORT, 0xD4); // Tell 8042 to direct byte to mouse
    if !mouse_wait_write() {
        return;
    }
    outb(PS2_DATA_PORT, byte);
}

unsafe fn mouse_read() -> u8 {
    if !mouse_wait_read() {
        return 0xFF;
    }
    inb(PS2_DATA_PORT)
}

pub fn handle_mouse_byte(byte: u8) {
    let mut internal = INTERNAL_MOUSE.lock();
    match internal.cycle {
        0 => {
            if (byte & 0x08) != 0 {
                internal.packet[0] = byte;
                internal.cycle = 1;
            }
        }
        1 => {
            internal.packet[1] = byte;
            internal.cycle = 2;
        }
        2 => {
            internal.packet[2] = byte;
            internal.cycle = 0;
            let flags = internal.packet[0];
            let raw_x = internal.packet[1];
            let raw_y = internal.packet[2];
            if (flags & 0xC0) != 0 {
                return; // Discard overflow packets
            }
            let x_sign = (flags & 0x10) != 0;
            let y_sign = (flags & 0x20) != 0;
            let delta_x: i32 = if x_sign { (raw_x as i8) as i32 } else { raw_x as i32 };
            let delta_y: i32 = if y_sign { (raw_y as i8) as i32 } else { raw_y as i32 };
            let mut mouse = MOUSE.lock();
            mouse.x = (mouse.x + delta_x).clamp(0, mouse.screen_width - 1);
            mouse.y = (mouse.y - delta_y).clamp(0, mouse.screen_height - 1);
            mouse.left_button = (flags & 0x01) != 0;
            mouse.right_button = (flags & 0x02) != 0;
            mouse.middle_button = (flags & 0x04) != 0;
            let (mx, my) = (mouse.x, mouse.y);
            drop(mouse);
            crate::drivers::gpu::i915::set_cursor_position(mx, my);
        }
        _ => internal.cycle = 0,
    }
}

fn mouse_irq_handler(_ctx: &mut InterruptContext) {
    // Drain all pending bytes so 8042 OBF never stays latched
    for _ in 0..16 {
        let status = unsafe { inb(PS2_STATUS_PORT) };
        if status == 0xFF || (status & 0x01) == 0 {
            break;
        }
        let byte = unsafe { inb(PS2_DATA_PORT) };
        if (status & 0x20) != 0 {
            handle_mouse_byte(byte);
        } else if byte != 0xFA && byte != 0xFE && byte != 0xAA {
            crate::drivers::keyboard::KEYBOARD.lock().handle_scancode(byte);
        }
    }
}

pub fn init(width: i32, height: i32) {
    {
        let mut mouse = MOUSE.lock();
        mouse.screen_width = width;
        mouse.screen_height = height;
        mouse.x = width / 2;
        mouse.y = height / 2;
    }
    unsafe {
        if inb(PS2_STATUS_PORT) != 0xFF {
            // 1. Flush any stale bytes from 0x60 before reading 8042 command byte
            for _ in 0..32 {
                if (inb(PS2_STATUS_PORT) & 0x01) == 0 {
                    break;
                }
                let _ = inb(PS2_DATA_PORT);
            }

            // 2. Enable auxiliary PS/2 port
            if mouse_wait_write() {
                outb(PS2_COMMAND_PORT, 0xA8);
            }

            // 3. Read command byte and preserve Keyboard IRQ1 (bit 0) + Set 1 translation (bit 6)
            //    while enabling Mouse IRQ12 (bit 1) and both clocks (clear bits 4 & 5)
            if mouse_wait_write() {
                outb(PS2_COMMAND_PORT, 0x20);
                let mut status = mouse_read();
                if status != 0xFF {
                    status = (status | 0x43) & !0x30;
                    if mouse_wait_write() {
                        outb(PS2_COMMAND_PORT, 0x60);
                        if mouse_wait_write() {
                            outb(PS2_DATA_PORT, status);
                        }
                    }
                }
            }

            // 4. Configure mouse defaults and enable streaming
            mouse_write(0xF6);
            let _ = mouse_read();
            mouse_write(0xF4);
            let _ = mouse_read();

            // 5. Drain any trailing ACK or early movement bytes so 0x60 is clean before sti
            for _ in 0..16 {
                if (inb(PS2_STATUS_PORT) & 0x01) == 0 {
                    break;
                }
                let _ = inb(PS2_DATA_PORT);
            }
        }
    }
    register_irq_handler(12, mouse_irq_handler);
    crate::arch::x86_64::ioapic::unmask_isa_irq(12);
    kinfo!("PS/2 Mouse driver initialized (IRQ 12 routed via I/O APIC -> Vector 44)");
}

/// Synchronize mouse clamping dimensions with active GOP/framebuffer resolution.
pub fn set_bounds(width: i32, height: i32) {
    let mut mouse = MOUSE.lock();
    mouse.screen_width = width.max(1);
    mouse.screen_height = height.max(1);
    mouse.x = mouse.x.clamp(0, mouse.screen_width - 1);
    mouse.y = mouse.y.clamp(0, mouse.screen_height - 1);
}

/// Apply a relative pointer event (e.g. USB HID Boot Mouse / Apple Trackpad Boot Mouse Emulation).
/// Note: USB HID Boot Mouse +Y points downward (matches screen Y coordinates).
pub fn apply_relative_event(buttons: u8, dx: i8, dy: i8) {
    let mut mouse = MOUSE.lock();
    mouse.left_button = (buttons & 0x01) != 0;
    mouse.right_button = (buttons & 0x02) != 0;
    mouse.middle_button = (buttons & 0x04) != 0;
    mouse.x = (mouse.x + dx as i32).clamp(0, mouse.screen_width.saturating_sub(1));
    mouse.y = (mouse.y + dy as i32).clamp(0, mouse.screen_height.saturating_sub(1));
    let (mx, my) = (mouse.x, mouse.y);
    drop(mouse);
    crate::drivers::gpu::i915::set_cursor_position(mx, my);
}
