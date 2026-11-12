use core::sync::atomic::{AtomicU64, Ordering};
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

/// Route an ISA IRQ through I/O APIC to a target CPU vector, honoring ACPI MADT
/// Interrupt Source Overrides (GSI mapping, polarity, and trigger mode) when present.
pub fn route_irq(irq: u8, vector: u8, dest_apic_id: u8, masked: bool) {
    let mut gsi = irq as u32;
    let mut pin_flags: u32 = 0;

    {
        let madt = crate::arch::x86_64::madt::MADT.lock();
        if let Some(iso) = madt.overrides.iter().find(|o| o.bus == 0 && o.source_irq == irq) {
            gsi = iso.gsi;
            // ACPI MPS INTI polarity bits 0..1: 0b11 = Active Low (IOAPIC bit 13)
            if (iso.flags & 0b11) == 0b11 {
                pin_flags |= 1 << 13;
            }
            // ACPI MPS INTI trigger bits 2..3: 0b11 = Level-triggered (IOAPIC bit 15)
            if ((iso.flags >> 2) & 0b11) == 0b11 {
                pin_flags |= 1 << 15;
            }
        }
    }

    if gsi >= 24 {
        return;
    }

    let mask_bit = if masked { 1 << 16 } else { 0 };
    let low = (vector as u32) | pin_flags | mask_bit;
    let high = (dest_apic_id as u32) << 24;
    let entry = ((high as u64) << 32) | (low as u64);
    unsafe {
        write_redirection_entry(gsi as u8, entry);
    }
}

/// Convenience helper for peripheral drivers to unmask an ISA IRQ (0..15) through
/// the I/O APIC mapped to IDT vector `32 + irq` on the BSP. No-ops safely before
/// `ioapic::init()` maps the controller MMIO base.
pub fn unmask_isa_irq(irq: u8) {
    if get_ioapic_base_virt() == 0 {
        return;
    }
    let bsp_id = crate::arch::x86_64::apic::get_apic_id();
    route_irq(irq, 32 + irq, bsp_id, false);
}

/// Mask legacy 8259 PIC so all interrupts route exclusively via I/O APIC.
pub unsafe fn disable_legacy_pic() {
    crate::arch::x86_64::pic::mask_all();
}

/// Mask legacy 8259 PIC and log APIC-only interrupt routing.
pub fn disable_pic() {
    unsafe {
        disable_legacy_pic();
    }
    kinfo!("Legacy 8259 PIC masked (0x21=0xFF, 0xA1=0xFF; interrupts routed via I/O APIC)");
}

/// Initialize the I/O APIC subsystem.
pub fn init() {
    // Ensure legacy 8259 master & slave PICs remain completely disabled
    disable_pic();

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

    // Route ISA hardware IRQs to BSP Local APIC, keeping legacy PIT IRQ 0 masked
    let bsp_id = crate::arch::x86_64::apic::get_apic_id();
    route_irq(0, 32, bsp_id, true);  // Keep legacy PIT masked (LAPIC MMIO Timer owns ticks)
    route_irq(1, 33, bsp_id, false); // PS/2 Keyboard unmasked
    route_irq(3, 35, bsp_id, false); // COM2 UART unmasked
    route_irq(4, 36, bsp_id, false); // COM1 UART unmasked
    route_irq(12, 44, bsp_id, false); // PS/2 Mouse unmasked
}
