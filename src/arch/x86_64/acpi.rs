use crate::arch::x86_64::io::{outb, outw};
use crate::mm::phys_to_virt;
use crate::sync::IrqSafeSpinlock;
use crate::kinfo;

#[derive(Debug, Clone, Copy)]
pub struct AcpiInfo {
    pub rsdp_addr: u64,
    pub rsdt_addr: u64,
    pub fadt_addr: u64,
    pub dsdt_addr: u64,
    pub pm1a_cnt_blk: u16,
    pub pm1b_cnt_blk: u16,
    pub slp_typa: u16,
    pub slp_typb: u16,
    pub reset_reg_port: u16,
    pub reset_value: u8,
    pub has_reset_reg: bool,
    pub is_available: bool,
}

impl AcpiInfo {
    pub const fn empty() -> Self {
        Self {
            rsdp_addr: 0,
            rsdt_addr: 0,
            fadt_addr: 0,
            dsdt_addr: 0,
            pm1a_cnt_blk: 0,
            pm1b_cnt_blk: 0,
            slp_typa: 5, // Default ACPI S5 SLP_TYP
            slp_typb: 5,
            reset_reg_port: 0,
            reset_value: 0,
            has_reset_reg: false,
            is_available: false,
        }
    }
}

pub static ACPI: IrqSafeSpinlock<AcpiInfo> = IrqSafeSpinlock::new(AcpiInfo::empty());

/// Initialize ACPI table detection.
pub fn init() {
    let mut info = AcpiInfo::empty();

    if let Some(rsdp_phys) = find_rsdp() {
        info.rsdp_addr = rsdp_phys;
        parse_rsdp(rsdp_phys, &mut info);
        info.is_available = true;
        kinfo!(
            "ACPI 1.0/2.0 Initialized: FADT: 0x{:X}, PM1a_CNT: 0x{:X}",
            info.fadt_addr,
            info.pm1a_cnt_blk
        );
    } else {
        kinfo!("ACPI RSDP not found; using emulator and standard I/O port fallbacks.");
    }

    *ACPI.lock() = info;
}

fn find_rsdp() -> Option<u64> {
    // 1. Search Extended BIOS Data Area (EBDA)
    let ebda_ptr = phys_to_virt(0x040E) as *const u16;
    let ebda_base = unsafe { (*ebda_ptr as u64) << 4 };
    if ebda_base >= 0x80000 && ebda_base < 0xA0000 {
        if let Some(found) = scan_for_rsdp(ebda_base, 1024) {
            return Some(found);
        }
    }

    // 2. Search main BIOS ROM area (0xE0000 - 0xFFFFF)
    scan_for_rsdp(0x000E0000, 0x20000)
}

fn scan_for_rsdp(phys_base: u64, length: usize) -> Option<u64> {
    let virt_base = phys_to_virt(phys_base) as *const u8;
    let sig = b"RSD PTR ";

    let mut offset = 0;
    while offset + 20 <= length {
        let slice = unsafe { core::slice::from_raw_parts(virt_base.add(offset), 8) };
        if slice == sig {
            // Validate checksum (20 bytes modulo 256 == 0)
            let full_slice = unsafe { core::slice::from_raw_parts(virt_base.add(offset), 20) };
            let sum: u8 = full_slice.iter().fold(0u8, |acc, &b| acc.wrapping_add(b));
            if sum == 0 {
                return Some(phys_base + offset as u64);
            }
        }
        offset += 16; // RSDP is aligned on 16-byte boundary
    }
    None
}

fn parse_rsdp(rsdp_phys: u64, out: &mut AcpiInfo) {
    let virt = phys_to_virt(rsdp_phys) as *const u8;
    let rsdt_address = unsafe { *(virt.add(16) as *const u32) } as u64;
    out.rsdt_addr = rsdt_address;

    if rsdt_address == 0 {
        return;
    }

    // Parse RSDT
    let rsdt_virt = phys_to_virt(rsdt_address) as *const u8;
    let rsdt_sig = unsafe { core::slice::from_raw_parts(rsdt_virt, 4) };
    if rsdt_sig != b"RSDT" {
        return;
    }

    let rsdt_len = unsafe { *(rsdt_virt.add(4) as *const u32) } as usize;
    if rsdt_len < 36 {
        return;
    }

    let entries_count = (rsdt_len - 36) / 4;
    let entries_ptr = unsafe { rsdt_virt.add(36) as *const u32 };

    for i in 0..entries_count {
        let table_phys = unsafe { *entries_ptr.add(i) } as u64;
        let table_virt = phys_to_virt(table_phys) as *const u8;
        let sig = unsafe { core::slice::from_raw_parts(table_virt, 4) };

        if sig == b"FACP" {
            out.fadt_addr = table_phys;
            parse_fadt(table_virt, out);
        } else if sig == b"APIC" {
            crate::arch::x86_64::madt::parse_madt(table_phys);
        }
    }
}

fn parse_fadt(fadt_virt: *const u8, out: &mut AcpiInfo) {
    let fadt_len = unsafe { *(fadt_virt.add(4) as *const u32) } as usize;

    if fadt_len >= 72 {
        out.dsdt_addr = unsafe { *(fadt_virt.add(40) as *const u32) } as u64;
        out.pm1a_cnt_blk = unsafe { *(fadt_virt.add(64) as *const u32) } as u16;
        out.pm1b_cnt_blk = unsafe { *(fadt_virt.add(68) as *const u32) } as u16;

        if fadt_len >= 129 {
            let flags = unsafe { *(fadt_virt.add(112) as *const u32) };
            if (flags & (1 << 10)) != 0 {
                // Reset register supported
                out.has_reset_reg = true;
                out.reset_reg_port = unsafe { *(fadt_virt.add(116 + 4) as *const u32) } as u16;
                out.reset_value = unsafe { *fadt_virt.add(128) };
            }
        }
    }

    // Try finding _S5 AML object in DSDT
    if out.dsdt_addr != 0 {
        let dsdt_virt = phys_to_virt(out.dsdt_addr) as *const u8;
        let dsdt_len = unsafe { *(dsdt_virt.add(4) as *const u32) } as usize;
        if dsdt_len > 36 {
            let aml_slice = unsafe { core::slice::from_raw_parts(dsdt_virt.add(36), dsdt_len - 36) };
            for i in 0..aml_slice.len().saturating_sub(6) {
                if &aml_slice[i..i + 4] == b"_S5_" {
                    // Found _S5 package
                    if i + 8 < aml_slice.len() && aml_slice[i + 4] == 0x12 {
                        let slp_a = aml_slice[i + 7] as u16;
                        out.slp_typa = slp_a;
                    }
                    break;
                }
            }
        }
    }
}

/// Perform ACPI / hardware power off.
pub fn poweroff() -> ! {
    crate::arch::x86_64::registers::disable_interrupts();

    let acpi = ACPI.lock();

    // 1. Try native ACPI PM1a/PM1b control block
    if acpi.pm1a_cnt_blk != 0 {
        let val_a = (acpi.slp_typa << 10) | (1 << 13);
        unsafe {
            outw(acpi.pm1a_cnt_blk, val_a);
            if acpi.pm1b_cnt_blk != 0 {
                let val_b = (acpi.slp_typb << 10) | (1 << 13);
                outw(acpi.pm1b_cnt_blk, val_b);
            }
        }
    }

    // 2. Hardware emulators power-off ports (QEMU modern, QEMU legacy, Bochs, VirtualBox)
    unsafe {
        outw(0x604, 0x2000);   // QEMU modern ACPI shutdown
        outw(0xB004, 0x2000);  // QEMU old / Bochs shutdown
        outw(0x4004, 0x3400);  // VirtualBox ACPI shutdown
    }

    // 3. Fallback halt loop
    loop {
        crate::arch::x86_64::registers::hlt();
    }
}

/// Perform ACPI / hardware system reboot.
pub fn reboot() -> ! {
    crate::arch::x86_64::registers::disable_interrupts();

    let acpi = ACPI.lock();

    // 1. ACPI reset register
    if acpi.has_reset_reg && acpi.reset_reg_port != 0 {
        unsafe {
            outb(acpi.reset_reg_port, acpi.reset_value);
        }
    }

    // 2. PS/2 8042 keyboard controller pulse reset
    unsafe {
        outb(0x64, 0xFE);
    }

    // 3. CPU Triple Fault
    unsafe {
        let null_idt: [u8; 6] = [0; 6];
        core::arch::asm!("lidt [{}]", in(reg) &null_idt, options(readonly, nostack, preserves_flags));
        core::arch::asm!("int3", options(noreturn));
    }
}
