pub mod acpi;
pub mod apic;
pub mod gdt;
pub mod idt;
pub mod io;
pub mod msr;
pub mod multiboot;
pub mod pic;
pub mod pit;
pub mod registers;
pub mod rtc;
pub mod serial;
pub mod syscall;
pub mod vga;

use crate::kinfo;

/// Full architecture initialization sequence for x86_64.
pub fn init() {
    // 1. Initialize COM1 Serial port hardware early for low-level logging
    serial::init_early();

    // 2. Initialize VGA text display
    vga::VGA_WRITER.lock().clear_screen();
    kinfo!("VGA console initialized");

    // 3. Initialize Global Descriptor Table & TSS with IST stacks
    gdt::init();
    kinfo!("GDT & 64-bit TSS loaded with IST stacks");

    // 4. Initialize Interrupt Descriptor Table
    idt::init();
    kinfo!("IDT initialized with 256 vector handlers");

    // 5. Remap legacy PIC to IRQs 32..47
    pic::remap(32, 40);
    kinfo!("8259 PIC remapped to vectors 32..47");

    // 6. Initialize 8254 PIT timer at 1000 Hz (1ms tick)
    pit::init(1000);
    kinfo!("8254 PIT initialized at 1000 Hz");

    // 7. Full UART subsystem initialization (IRQ 4 & 3 handlers, unmask PIC, detect COM1 & COM2)
    serial::init();

    // 8. Initialize AMD64 fast syscall (LSTAR/STAR)
    syscall::init();
    kinfo!("Fast syscall interface initialized");

    // 9. Discover ACPI tables for power management
    acpi::init();
}
