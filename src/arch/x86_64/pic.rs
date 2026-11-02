use crate::arch::x86_64::io::{inb, io_wait, outb};

const PIC1_COMMAND: u16 = 0x20;
const PIC1_DATA: u16 = 0x21;
const PIC2_COMMAND: u16 = 0xA0;
const PIC2_DATA: u16 = 0xA1;

const PIC_EOI: u8 = 0x20;

const ICW1_INIT: u8 = 0x10;
const ICW1_ICW4: u8 = 0x01;
const ICW4_8086: u8 = 0x01;

/// Remap the 8259 PIC vectors to 0x20..0x2F (IRQs 32..47).
pub fn remap(offset1: u8, offset2: u8) {
    unsafe {
        let mask1 = inb(PIC1_DATA);
        let mask2 = inb(PIC2_DATA);

        outb(PIC1_COMMAND, ICW1_INIT | ICW1_ICW4);
        io_wait();
        outb(PIC2_COMMAND, ICW1_INIT | ICW1_ICW4);
        io_wait();

        outb(PIC1_DATA, offset1);
        io_wait();
        outb(PIC2_DATA, offset2);
        io_wait();

        outb(PIC1_DATA, 4); // PIC2 at IRQ2
        io_wait();
        outb(PIC2_DATA, 2); // Cascade identity
        io_wait();

        outb(PIC1_DATA, ICW4_8086);
        io_wait();
        outb(PIC2_DATA, ICW4_8086);
        io_wait();

        outb(PIC1_DATA, mask1);
        outb(PIC2_DATA, mask2);
    }
}

/// Send End-of-Interrupt (EOI) signal for the specified IRQ.
pub fn send_eoi(irq: u8) {
    unsafe {
        if irq >= 8 {
            outb(PIC2_COMMAND, PIC_EOI);
        }
        outb(PIC1_COMMAND, PIC_EOI);
    }
}

/// Unmask a specific IRQ line on the PIC.
pub fn unmask(irq: u8) {
    unsafe {
        let port = if irq < 8 { PIC1_DATA } else { PIC2_DATA };
        let shift = if irq < 8 { irq } else { irq - 8 };
        let mask = inb(port) & !(1 << shift);
        outb(port, mask);
    }
}

/// Mask all PIC interrupts.
pub fn mask_all() {
    unsafe {
        outb(PIC1_DATA, 0xFF);
        outb(PIC2_DATA, 0xFF);
    }
}
