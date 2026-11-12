pub mod acpi;
pub mod apic;
pub mod gdt;
pub mod idt;
pub mod io;
pub mod ioapic;
pub mod madt;
pub mod msr;
pub mod multiboot;
pub mod pic;
pub mod pit;
pub mod registers;
pub mod rtc;
pub mod serial;
pub mod smp;
pub mod syscall;
pub mod vga;

use crate::kinfo;

/// Full architecture initialization sequence for x86_64.
pub fn init() {
    // 0. Enable SSE / SIMD instructions in CR0/CR4 & PAT Write-Combining (WC)
    unsafe {
        registers::enable_sse();
        registers::enable_write_combining();
    }

    // 1. Initialize COM1 Serial port hardware early for low-level logging
    serial::init_early();

    // 2. Initialize VGA text display
    vga::VGA_WRITER.lock().clear_screen();
    kinfo!("VGA console initialized");

    // 3. Initialize Global Descriptor Table & TSS with IST stacks
    gdt::init();
    kinfo!("GDT & 64-bit TSS loaded with IST stacks");

    // 4. Remap legacy 8259 PIC to vectors 32..47 and mask all lines (0xFF) before IDT/APIC
    pic::remap(32, 40);
    pic::mask_all();
    kinfo!("8259 PIC remapped to vectors 32..47 and globally masked");

    // 5. Initialize Interrupt Descriptor Table
    idt::init();
    kinfo!("IDT initialized with 256 vector handlers");

    // 6. Initialize 8254 PIT timer calibration support
    pit::init(1000);
    kinfo!("8254 PIT initialized at 1000 Hz (IRQ 0 masked; LAPIC drives system ticks)");

    // 7. Full UART subsystem initialization (installs IRQ 4 & 3 handlers; routed via I/O APIC)
    serial::init();

    // 8. Initialize AMD64 fast syscall (LSTAR/STAR)
    syscall::init();
    kinfo!("Fast syscall interface initialized");
}

/// Initialize ACPI, Local APIC, I/O APIC, and SMP multiprocessing once memory management is active.
pub fn init_apic_smp() {
    // Ensure legacy 8259 master & slave PICs remain completely masked before APIC startup
    unsafe {
        ioapic::disable_legacy_pic();
    }
    acpi::init();
    apic::init();
    ioapic::init();
    smp::init();
}
