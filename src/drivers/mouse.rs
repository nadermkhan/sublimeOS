use crate::arch::x86_64::idt::{register_irq_handler, InterruptContext};
use crate::arch::x86_64::io::{inb, outb};
use crate::arch::x86_64::pic::unmask;
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

unsafe fn mouse_wait_write() {
    let mut timeout = 100000;
    while (inb(PS2_STATUS_PORT) & 0x02) != 0 && timeout > 0 {
        timeout -= 1;
        core::hint::spin_loop();
    }
}

unsafe fn mouse_wait_read() {
    let mut timeout = 100000;
    while (inb(PS2_STATUS_PORT) & 0x01) == 0 && timeout > 0 {
        timeout -= 1;
        core::hint::spin_loop();
    }
}

unsafe fn mouse_write(byte: u8) {
    mouse_wait_write();
    outb(PS2_COMMAND_PORT, 0xD4); // Tell 8042 to direct byte to mouse
    mouse_wait_write();
    outb(PS2_DATA_PORT, byte);
}

unsafe fn mouse_read() -> u8 {
    mouse_wait_read();
    inb(PS2_DATA_PORT)
}

fn mouse_irq_handler(_ctx: &mut InterruptContext) {
    let status = unsafe { inb(PS2_STATUS_PORT) };
    if (status & 0x20) == 0 {
        // Not mouse output
        return;
    }

    let byte = unsafe { inb(PS2_DATA_PORT) };
    let mut internal = INTERNAL_MOUSE.lock();

    match internal.cycle {
        0 => {
            // Byte 0 must have bit 3 set (sync bit)
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

            // Complete 3-byte packet received!
            let flags = internal.packet[0];
            let raw_x = internal.packet[1];
            let raw_y = internal.packet[2];

            let x_sign = (flags & 0x10) != 0;
            let y_sign = (flags & 0x20) != 0;

            let delta_x: i32 = if x_sign {
                (raw_x as i8) as i32
            } else {
                raw_x as i32
            };

            let delta_y: i32 = if y_sign {
                (raw_y as i8) as i32
            } else {
                raw_y as i32
            };

            let mut mouse = MOUSE.lock();
            mouse.x += delta_x;
            mouse.y -= delta_y; // PS/2 Y is inverted (up is positive)

            // Clamp to screen
            mouse.x = mouse.x.clamp(0, mouse.screen_width - 1);
            mouse.y = mouse.y.clamp(0, mouse.screen_height - 1);

            mouse.left_button = (flags & 0x01) != 0;
            mouse.right_button = (flags & 0x02) != 0;
            mouse.middle_button = (flags & 0x04) != 0;
        }
        _ => internal.cycle = 0,
    }
}

/// Initialize PS/2 Mouse on auxiliary 8042 port and unmask IRQ 12.
pub fn init(width: i32, height: i32) {
    {
        let mut mouse = MOUSE.lock();
        mouse.screen_width = width;
        mouse.screen_height = height;
        mouse.x = width / 2;
        mouse.y = height / 2;
    }

    unsafe {
        // Enable auxiliary PS/2 mouse device
        mouse_wait_write();
        outb(PS2_COMMAND_PORT, 0xA8);

        // Read controller command byte
        mouse_wait_write();
        outb(PS2_COMMAND_PORT, 0x20);
        let mut status = mouse_read();

        // Enable interrupt IRQ 12 (bit 1) and clear disable mouse clock (bit 5)
        status |= 0x02;
        status &= !0x20;

        // Write controller command byte
        mouse_wait_write();
        outb(PS2_COMMAND_PORT, 0x60);
        mouse_wait_write();
        outb(PS2_DATA_PORT, status);

        // Set default mouse settings (sample rate 100, resolution 4 count/mm)
        mouse_write(0xF6); // Set defaults
        let _ = mouse_read(); // ACK (0xFA)

        // Enable packet streaming
        mouse_write(0xF4);
        let _ = mouse_read(); // ACK (0xFA)
    }

    // Register IRQ 12 handler (slave PIC IRQ 4 = vector 44)
    register_irq_handler(12, mouse_irq_handler);

    // Unmask IRQ 2 (cascade on master PIC) and IRQ 12 on slave PIC
    unmask(2);
    unmask(12);

    kinfo!("PS/2 Mouse driver initialized (IRQ 12 unmasked)");
}
