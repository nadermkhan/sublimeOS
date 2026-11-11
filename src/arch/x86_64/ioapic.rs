use core::sync::atomic::{AtomicU64, Ordering};
use crate::arch::x86_64::io::outb;
use crate::kinfo;

pub const IOAPIC_DEFAULT_BASE: u64 = 0xFEC00000;

pub const REG_IOAPIC_ID: u8 = 0x00;
pub const REG_IOAPIC_VER: u8 = 0x01;
pub const REG_IOAPIC_ARB: u8 = 0x02;
pub const REG_REDTBL_BASE: u8 = 0x10;

static IOAPIC_BASE_VIRT: AtomicU64 = AtomicU64::new(0);

#[inline(always)]
pub fn get_ioapic_base_virt() -> u64 {
    IOAPIC_BASE_VIRT.load(Ordering::Relaxed)
}

/// Read an I/O APIC register via IOREGSEL / IOWIN.
pub unsafe fn read_reg(reg: u8) -> u32 {
    let base = get_ioapic_base_virt();
    if base == 0 {
        return 0;
    }
    let reg_sel = base as *mut u32;
    let win = (base + 0x10) as *mut u32;
    core::ptr::write_volatile(reg_sel, reg as u32);
    core::ptr::read_volatile(win)
}

/// Write an I/O APIC register via IOREGSEL / IOWIN.
pub unsafe fn write_reg(reg: u8, val: u32) {
    let base = get_ioapic_base_virt();
    if base == 0 {
        return;
    }
    let reg_sel = base as *mut u32;
    let win = (base + 0x10) as *mut u32;
    core::ptr::write_volatile(reg_sel, reg as u32);
    core::ptr::write_volatile(win, val);
}

/// Read a 64-bit Redirection Table Entry.
pub unsafe fn read_redirection_entry(index: u8) -> u64 {
    let low = read_reg(REG_REDTBL_BASE + index * 2) as u64;
    let high = read_reg(REG_REDTBL_BASE + index * 2 + 1) as u64;
    (high << 32) | low
}

/// Write a 64-bit Redirection Table Entry.
pub unsafe fn write_redirection_entry(index: u8, entry: u64) {
    let low = (entry & 0xFFFF_FFFF) as u32;
    let high = ((entry >> 32) & 0xFFFF_FFFF) as u32;
    write_reg(REG_REDTBL_BASE + index * 2, low);
    write_reg(REG_REDTBL_BASE + index * 2 + 1, high);
}

/// Route an ISA IRQ through I/O APIC to a target CPU vector.
pub fn route_irq(irq: u8, vector: u8, dest_apic_id: u8, masked: bool) {
    let mask_bit = if masked { 1 << 16 } else { 0 };
    let low = (vector as u32) | mask_bit;
    let high = (dest_apic_id as u32) << 24;
    let entry = ((high as u64) << 32) | (low as u64);
    unsafe {
        write_redirection_entry(irq, entry);
    }
}

/// Mask legacy 8259 PIC so all interrupts route exclusively via I/O APIC.
pub fn disable_pic() {
    unsafe {
        outb(0x21, 0xFF);
        outb(0xA1, 0xFF);
    }
    kinfo!("Legacy 8259 PIC disabled (interrupts routed via I/O APIC)");
}

/// Initialize the I/O APIC subsystem.
pub fn init() {
    let madt = crate::arch::x86_64::madt::MADT.lock();
    let phys_base = if let Some(ioapic) = madt.io_apics.first() {
        ioapic.address as u64
    } else {
        IOAPIC_DEFAULT_BASE
    };
    drop(madt);

    if let Ok(virt) = crate::mm::vmm::map_mmio_range(phys_base, 4096) {
        IOAPIC_BASE_VIRT.store(virt, Ordering::SeqCst);
    }

    if get_ioapic_base_virt() == 0 {
        return;
    }

    let ver = unsafe { read_reg(REG_IOAPIC_VER) };
    let max_entries = ((ver >> 16) & 0xFF) + 1;
    let version = ver & 0xFF;

    kinfo!(
        "I/O APIC Initialized: Version 0x{:02X}, Max Redirection Entries: {}",
        version,
        max_entries
    );

    // Route ISA IRQs: IRQ 1 (Keyboard) -> Vector 33, IRQ 4 (COM1) -> Vector 36
    let bsp_id = crate::arch::x86_64::apic::get_apic_id();
    route_irq(1, 33, bsp_id, false); // Keyboard unmasked
    route_irq(4, 36, bsp_id, false); // Serial UART unmasked
}
