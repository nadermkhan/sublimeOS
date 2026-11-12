use crate::arch::x86_64::io::{inb, inw, outb, outl, outw};
use crate::mm::phys_to_virt;
use crate::mm::vmm::map_mmio_range;
use crate::sync::IrqSafeSpinlock;
use crate::{kinfo, kwarn};

const EARLY_DIRECT_MAP_LIMIT: u64 = 0x4000_0000; // 1 GiB boot direct map window

#[derive(Debug, Clone, Copy)]
pub struct AcpiInfo {
    pub rsdp_addr: u64,
    pub rsdp_revision: u8,
    pub rsdt_addr: u64,
    pub xsdt_addr: u64,
    pub fadt_addr: u64,
    pub dsdt_addr: u64,
    pub madt_addr: u64,
    pub hpet_addr: u64,
    pub mcfg_addr: u64,
    pub pm1a_evt_blk: u16,
    pub pm1b_evt_blk: u16,
    pub pm1a_cnt_blk: u16,
    pub pm1b_cnt_blk: u16,
    pub smi_cmd_port: u16,
    pub acpi_enable: u8,
    pub acpi_disable: u8,
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
            rsdp_revision: 0,
            rsdt_addr: 0,
            xsdt_addr: 0,
            fadt_addr: 0,
            dsdt_addr: 0,
            madt_addr: 0,
            hpet_addr: 0,
            mcfg_addr: 0,
            pm1a_evt_blk: 0,
            pm1b_evt_blk: 0,
            pm1a_cnt_blk: 0,
            pm1b_cnt_blk: 0,
            smi_cmd_port: 0,
            acpi_enable: 0,
            acpi_disable: 0,
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

/// Safely map any ACPI physical region into kernel virtual space (works both inside <1GiB direct map
/// and for high firmware tables located above 1 GiB on bare metal).
pub fn map_acpi_phys(phys_addr: u64, size: usize) -> Option<u64> {
    if phys_addr == 0 || size == 0 {
        return None;
    }
    if phys_addr.saturating_add(size as u64) < EARLY_DIRECT_MAP_LIMIT {
        Some(phys_to_virt(phys_addr))
    } else {
        map_mmio_range(phys_addr, size).ok()
    }
}

fn validate_checksum(ptr: *const u8, len: usize) -> bool {
    if len == 0 {
        return false;
    }
    let slice = unsafe { core::slice::from_raw_parts(ptr, len) };
    slice.iter().fold(0u8, |acc, &b| acc.wrapping_add(b)) == 0
}

/// Initialize ACPI table detection (UEFI Multiboot2 RSDP, EBDA, BIOS ROM; XSDT 64-bit & RSDT 32-bit).
pub fn init() {
    let mut info = AcpiInfo::empty();

    if let Some(rsdp_phys) = find_rsdp() {
        info.rsdp_addr = rsdp_phys;
        parse_rsdp(rsdp_phys, &mut info);
        info.is_available = true;
        kinfo!(
            "ACPI Initialized (Rev {}, {}): MADT: 0x{:X}, FADT: 0x{:X}, PM1a_CNT: 0x{:X}",
            info.rsdp_revision,
            if info.xsdt_addr != 0 { "XSDT 64-bit" } else { "RSDT 32-bit" },
            info.madt_addr,
            info.fadt_addr,
            info.pm1a_cnt_blk
        );
    } else {
        kwarn!("ACPI RSDP not found; using default APIC base 0xFEE00000 and standard I/O fallbacks.");
    }

    *ACPI.lock() = info;
}

fn find_rsdp() -> Option<u64> {
    // 0. Check Multiboot2 ACPI RSDP tag (Tag 14 / Tag 15) provided on modern UEFI firmware
    if let Some(mb_rsdp) = crate::arch::x86_64::multiboot::MULTIBOOT.lock().acpi_rsdp_phys {
        if let Some(virt) = map_acpi_phys(mb_rsdp, 36) {
            let ptr = virt as *const u8;
            let sig = unsafe { core::slice::from_raw_parts(ptr, 8) };
            if sig == b"RSD PTR " && validate_checksum(ptr, 20) {
                return Some(mb_rsdp);
            }
        }
    }

    // 1. Search Extended BIOS Data Area (EBDA)
    let ebda_ptr = phys_to_virt(0x040E) as *const u16;
    let ebda_base = unsafe { (core::ptr::read_unaligned(ebda_ptr) as u64) << 4 };
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
        let ptr = unsafe { virt_base.add(offset) };
        let slice = unsafe { core::slice::from_raw_parts(ptr, 8) };
        if slice == sig && validate_checksum(ptr, 20) {
            return Some(phys_base + offset as u64);
        }
        offset += 16; // RSDP is aligned on 16-byte boundary
    }
    None
}

fn parse_sdt_table(table_phys: u64, out: &mut AcpiInfo) {
    let Some(hdr_virt) = map_acpi_phys(table_phys, 36) else {
        return;
    };
    let hdr_ptr = hdr_virt as *const u8;
    let table_len = unsafe { core::ptr::read_unaligned(hdr_ptr.add(4) as *const u32) } as usize;
    if table_len < 36 || table_len > 1024 * 1024 {
        return;
    }

    // Map full table if it spans beyond initial 36 bytes / >1GiB page
    let Some(full_virt) = map_acpi_phys(table_phys, table_len) else {
        return;
    };
    let table_virt = full_virt as *const u8;
    if !validate_checksum(table_virt, table_len) {
        return;
    }

    let sig = unsafe { core::slice::from_raw_parts(table_virt, 4) };
    if sig == b"FACP" {
        out.fadt_addr = table_phys;
        parse_fadt(table_virt, table_len, out);
    } else if sig == b"APIC" {
        out.madt_addr = table_phys;
        crate::arch::x86_64::madt::parse_madt(table_phys);
    } else if sig == b"HPET" {
        out.hpet_addr = table_phys;
    } else if sig == b"MCFG" {
        out.mcfg_addr = table_phys;
    }
}

fn parse_rsdp(rsdp_phys: u64, out: &mut AcpiInfo) {
    let Some(virt_addr) = map_acpi_phys(rsdp_phys, 36) else {
        return;
    };
    let virt = virt_addr as *const u8;
    let revision = unsafe { *virt.add(15) };
    let rsdt_address = unsafe { core::ptr::read_unaligned(virt.add(16) as *const u32) } as u64;
    out.rsdp_revision = revision;
    out.rsdt_addr = rsdt_address;

    // ACPI 2.0+: Prefer 64-bit XSDT when extended checksum is valid
    if revision >= 2 && validate_checksum(virt, 36) {
        let xsdt_address = unsafe { core::ptr::read_unaligned(virt.add(24) as *const u64) };
        if xsdt_address != 0 {
            out.xsdt_addr = xsdt_address;
            if let Some(xsdt_hdr_virt) = map_acpi_phys(xsdt_address, 36) {
                let hdr = xsdt_hdr_virt as *const u8;
                let sig = unsafe { core::slice::from_raw_parts(hdr, 4) };
                let xsdt_len = unsafe { core::ptr::read_unaligned(hdr.add(4) as *const u32) } as usize;
                if sig == b"XSDT" && xsdt_len >= 36 && xsdt_len <= 1024 * 1024 {
                    if let Some(xsdt_full_virt) = map_acpi_phys(xsdt_address, xsdt_len) {
                        let xsdt_virt = xsdt_full_virt as *const u8;
                        if validate_checksum(xsdt_virt, xsdt_len) {
                            let entries_count = (xsdt_len - 36) / 8;
                            let entries_ptr = unsafe { xsdt_virt.add(36) as *const u64 };
                            for i in 0..entries_count {
                                let table_phys = unsafe { core::ptr::read_unaligned(entries_ptr.add(i)) };
                                if table_phys != 0 {
                                    parse_sdt_table(table_phys, out);
                                }
                            }
                            return;
                        }
                    }
                }
            }
        }
    }

    if rsdt_address == 0 {
        return;
    }

    // Fallback to ACPI 1.0 32-bit RSDT
    let Some(rsdt_hdr_virt) = map_acpi_phys(rsdt_address, 36) else {
        return;
    };
    let rsdt_hdr = rsdt_hdr_virt as *const u8;
    let rsdt_sig = unsafe { core::slice::from_raw_parts(rsdt_hdr, 4) };
    if rsdt_sig != b"RSDT" {
        return;
    }

    let rsdt_len = unsafe { core::ptr::read_unaligned(rsdt_hdr.add(4) as *const u32) } as usize;
    if rsdt_len < 36 || rsdt_len > 1024 * 1024 {
        return;
    }

    let Some(rsdt_full_virt) = map_acpi_phys(rsdt_address, rsdt_len) else {
        return;
    };
    let rsdt_virt = rsdt_full_virt as *const u8;
    if !validate_checksum(rsdt_virt, rsdt_len) {
        return;
    }

    let entries_count = (rsdt_len - 36) / 4;
    let entries_ptr = unsafe { rsdt_virt.add(36) as *const u32 };

    for i in 0..entries_count {
        let table_phys = unsafe { core::ptr::read_unaligned(entries_ptr.add(i)) } as u64;
        if table_phys != 0 {
            parse_sdt_table(table_phys, out);
        }
    }
}

fn parse_fadt(fadt_virt: *const u8, fadt_len: usize, out: &mut AcpiInfo) {
    if fadt_len >= 72 {
        if fadt_len >= 56 {
            out.smi_cmd_port = unsafe { core::ptr::read_unaligned(fadt_virt.add(48) as *const u32) } as u16;
            out.acpi_enable = unsafe { *fadt_virt.add(52) };
            out.acpi_disable = unsafe { *fadt_virt.add(53) };
        }
        out.dsdt_addr = unsafe { core::ptr::read_unaligned(fadt_virt.add(40) as *const u32) } as u64;
        if fadt_len >= 148 {
            let x_dsdt = unsafe { core::ptr::read_unaligned(fadt_virt.add(140) as *const u64) };
            if x_dsdt != 0 {
                out.dsdt_addr = x_dsdt;
            }
        }
        out.pm1a_evt_blk = unsafe { core::ptr::read_unaligned(fadt_virt.add(56) as *const u32) } as u16;
        out.pm1b_evt_blk = unsafe { core::ptr::read_unaligned(fadt_virt.add(60) as *const u32) } as u16;
        out.pm1a_cnt_blk = unsafe { core::ptr::read_unaligned(fadt_virt.add(64) as *const u32) } as u16;
        out.pm1b_cnt_blk = unsafe { core::ptr::read_unaligned(fadt_virt.add(68) as *const u32) } as u16;

        // ACPI 2.0+ Extended Generic Address Structure (GAS) fallbacks
        if out.pm1a_evt_blk == 0 && fadt_len >= 136 {
            let x_pm1a_evt = unsafe { core::ptr::read_unaligned(fadt_virt.add(124 + 4) as *const u64) };
            if x_pm1a_evt != 0 {
                out.pm1a_evt_blk = x_pm1a_evt as u16;
            }
        }
        if out.pm1a_cnt_blk == 0 && fadt_len >= 160 {
            let x_pm1a_cnt = unsafe { core::ptr::read_unaligned(fadt_virt.add(148 + 4) as *const u64) };
            if x_pm1a_cnt != 0 {
                out.pm1a_cnt_blk = x_pm1a_cnt as u16;
            }
        }
        if out.pm1b_cnt_blk == 0 && fadt_len >= 172 {
            let x_pm1b_cnt = unsafe { core::ptr::read_unaligned(fadt_virt.add(160 + 4) as *const u64) };
            if x_pm1b_cnt != 0 {
                out.pm1b_cnt_blk = x_pm1b_cnt as u16;
            }
        }

        if fadt_len >= 129 {
            let flags = unsafe { core::ptr::read_unaligned(fadt_virt.add(112) as *const u32) };
            if (flags & (1 << 10)) != 0 {
                // Reset register supported
                out.has_reset_reg = true;
                out.reset_reg_port = unsafe { core::ptr::read_unaligned(fadt_virt.add(116 + 4) as *const u32) } as u16;
                out.reset_value = unsafe { *fadt_virt.add(128) };
            }
        }
    }

    // Try finding _S5 AML object in DSDT
    if out.dsdt_addr != 0 {
        if let Some(dsdt_hdr_virt) = map_acpi_phys(out.dsdt_addr, 36) {
            let dsdt_len = unsafe { core::ptr::read_unaligned((dsdt_hdr_virt as *const u8).add(4) as *const u32) } as usize;
            if dsdt_len > 36 && dsdt_len <= 1024 * 1024 {
                if let Some(dsdt_full_virt) = map_acpi_phys(out.dsdt_addr, dsdt_len) {
                    let dsdt_virt = dsdt_full_virt as *const u8;
                    let aml_slice = unsafe { core::slice::from_raw_parts(dsdt_virt.add(36), dsdt_len - 36) };
                    for i in 0..aml_slice.len().saturating_sub(12) {
                        if &aml_slice[i..i + 4] == b"_S5_" {
                            let mut idx = i + 4;
                            if aml_slice[idx] == 0x12 { // PackageOp
                                idx += 1;
                                let lead = aml_slice[idx];
                                let pkg_len_bytes = match (lead >> 6) & 3 {
                                    0 => 1,
                                    1 => 2,
                                    2 => 3,
                                    _ => 4,
                                };
                                idx += pkg_len_bytes; // at NumElements
                                idx += 1; // at Element 0
                                if idx < aml_slice.len() {
                                    let val = match aml_slice[idx] {
                                        0x00 => 0,
                                        0x01 => 1,
                                        0x0A if idx + 1 < aml_slice.len() => aml_slice[idx + 1] as u16,
                                        0x0B if idx + 2 < aml_slice.len() => u16::from_le_bytes([aml_slice[idx + 1], aml_slice[idx + 2]]),
                                        b => (b & 0x07) as u16,
                                    };
                                    out.slp_typa = val;
                                    out.slp_typb = val;
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Check whether the ACPI power button was physically tapped or held on hardware.
pub fn is_power_button_pressed() -> bool {
    let port = ACPI.lock().pm1a_evt_blk;
    if port != 0 {
        let sts = unsafe { inw(port) };
        if (sts & (1 << 8)) != 0 && sts != 0xFFFF {
            return true;
        }
    }
    // Check standard Intel Cougar Point / PCH ACPI base (0x400 / 0x1800)
    let sts_400 = unsafe { inw(0x400) };
    if (sts_400 & (1 << 8)) != 0 && sts_400 != 0xFFFF {
        return true;
    }
    let sts_1800 = unsafe { inw(0x1800) };
    if (sts_1800 & (1 << 8)) != 0 && sts_1800 != 0xFFFF {
        return true;
    }
    false
}

/// Perform ACPI / hardware power off.
pub fn poweroff() -> ! {
    crate::arch::x86_64::registers::disable_interrupts();

    let acpi = ACPI.lock();

    // 1. Transition chipset from firmware SMI to OS ACPI mode if configured
    if acpi.smi_cmd_port != 0 && acpi.acpi_enable != 0 {
        unsafe {
            outb(acpi.smi_cmd_port, acpi.acpi_enable);
            crate::arch::x86_64::pit::delay_us(50);
        }
    }

    // 2. Intel PCH (Cougar Point 6-Series): Enable ACPI I/O decode on LPC Bridge (Bus 0, Dev 31, Func 0)
    //    PCI offset 0x40 = PMBASE, offset 0x44 = ACPI_CNTL (bit 7 = ACPI_EN)
    let pmbase_raw = unsafe { crate::drivers::pci::PciBus::read_config_dword(0, 31, 0, 0x40) };
    let pch_pmbase = (pmbase_raw & 0xFF80) as u16;
    if pch_pmbase != 0 {
        let acpi_cntl = unsafe { crate::drivers::pci::PciBus::read_config_dword(0, 31, 0, 0x44) };
        unsafe {
            crate::drivers::pci::PciBus::write_config_dword(0, 31, 0, 0x44, acpi_cntl | 0x80);
        }
    }

    // 3. Clear all status/wake bits and disable all wake events across all ACPI blocks.
    //    Crucial: In Intel ACPI hardware, if WAK_STS (bit 15 of PM1_STS) is set, writes
    //    to SLP_EN are completely ignored by the PCH hardware sleep state sequencer!
    if acpi.pm1a_evt_blk != 0 {
        unsafe {
            outw(acpi.pm1a_evt_blk, 0xFFFF);
            outw(acpi.pm1a_evt_blk + 2, 0x0000);
        }
    }
    if acpi.pm1b_evt_blk != 0 {
        unsafe {
            outw(acpi.pm1b_evt_blk, 0xFFFF);
            outw(acpi.pm1b_evt_blk + 2, 0x0000);
        }
    }
    if pch_pmbase != 0 {
        unsafe {
            outw(pch_pmbase + 0x00, 0xFFFF);      // PM1_STS (clear WAK_STS and sticky bits)
            outw(pch_pmbase + 0x02, 0x0000);      // PM1_EN (disable wake interrupts)
            outl(pch_pmbase + 0x20, 0xFFFF_FFFF); // GPE0_STS (clear all general purpose events)
            outl(pch_pmbase + 0x28, 0x0000_0000); // GPE0_EN (disable all GPEs)
        }
    }
    unsafe {
        outw(0x400, 0xFFFF);
        outw(0x402, 0x0000);
        outw(0x1800, 0xFFFF);
        outw(0x1802, 0x0000);
    }

    // 4. Flush all dirty CPU cache lines to DRAM before power state change
    unsafe {
        core::arch::asm!("wbinvd");
    }

    // 5. Try native ACPI PM1a/PM1b control block (two-step write: SLP_TYP then SLP_TYP | SLP_EN)
    if acpi.pm1a_cnt_blk != 0 {
        let val_typ_a = (acpi.slp_typa & 0x07) << 10;
        let val_en_a = val_typ_a | (1 << 13);
        unsafe {
            outw(acpi.pm1a_cnt_blk, val_typ_a);
            crate::arch::x86_64::pit::delay_us(10);
            outw(acpi.pm1a_cnt_blk, val_en_a);
            if acpi.pm1b_cnt_blk != 0 {
                let val_typ_b = (acpi.slp_typb & 0x07) << 10;
                let val_en_b = val_typ_b | (1 << 13);
                outw(acpi.pm1b_cnt_blk, val_typ_b);
                crate::arch::x86_64::pit::delay_us(10);
                outw(acpi.pm1b_cnt_blk, val_en_b);
            }
        }
    }

    // 6. Write S5 (Soft Off) directly to Intel PCH PM1_CNT
    if pch_pmbase != 0 {
        unsafe {
            // Intel Series 6 Cougar Point S5: SLP_TYP = 7 (0x1C00 -> 0x3C00)
            outw(pch_pmbase + 0x04, 0x1C00);
            crate::arch::x86_64::pit::delay_us(10);
            outw(pch_pmbase + 0x04, 0x3C00);
            crate::arch::x86_64::pit::delay_us(50);

            // Fallback S5: SLP_TYP = 5 (0x1400 -> 0x3400)
            outw(pch_pmbase + 0x04, 0x1400);
            crate::arch::x86_64::pit::delay_us(10);
            outw(pch_pmbase + 0x04, 0x3400);
            crate::arch::x86_64::pit::delay_us(50);
        }
    }

    // 7. Standard legacy & virtual machine shutdown ports
    unsafe {
        outw(0x404, 0x3C00);
        outw(0x404, 0x3400);
        outw(0x1804, 0x3C00);
        outw(0x1804, 0x3400);
        outw(0x604, 0x2000);   // QEMU modern ACPI shutdown
        outw(0xB004, 0x2000);  // QEMU legacy / Bochs
        outw(0x4004, 0x3400);  // VirtualBox
    }

    // 8. Turn off display and keyboard backlights via Apple SMC / gmux
    crate::drivers::platform::apple::ApplePlatformHardware::set_keyboard_light(0);
    crate::drivers::platform::apple::ApplePlatformHardware::set_display_brightness(0);
    if let Some(mut con) = crate::drivers::gop_console::GOP_CONSOLE.try_lock() {
        con.clear_screen();
    }

    // 9. Low-power halt loop
    loop {
        crate::arch::x86_64::registers::hlt();
    }
}

/// Perform ACPI / hardware system reboot.
pub fn reboot() -> ! {
    crate::arch::x86_64::registers::disable_interrupts();

    let acpi = ACPI.lock();

    // Loop up to 10 iterations across hardware reset mechanisms
    for _ in 0..10 {
        // 1. ACPI Reset Register (FADT offset 116)
        if acpi.has_reset_reg && acpi.reset_reg_port != 0 {
            unsafe {
                outb(acpi.reset_reg_port, acpi.reset_value);
                crate::arch::x86_64::pit::delay_us(50);
            }
        }

        // 2. Linux-standard PCI Reset via Port 0xCF9
        let cf9 = unsafe { inb(0xCF9) };
        unsafe {
            // Warm reset attempt (assert reset request then CPU reset line 0x06)
            outb(0xCF9, (cf9 & !0x06) | 0x02);
            crate::arch::x86_64::pit::delay_us(50);
            outb(0xCF9, (cf9 & !0x06) | 0x06);
            crate::arch::x86_64::pit::delay_us(50);

            // Cold reset / full power-cycle reset attempt (0x0E)
            outb(0xCF9, (cf9 & !0x0E) | 0x02);
            crate::arch::x86_64::pit::delay_us(50);
            outb(0xCF9, (cf9 & !0x0E) | 0x0E);
            crate::arch::x86_64::pit::delay_us(100);
        }

        // 3. PS/2 8042 Keyboard Controller Pulse Reset (legacy PCs)
        unsafe {
            outb(0x64, 0xFE);
            crate::arch::x86_64::pit::delay_us(50);
        }
    }

    // 4. Guaranteed x86_64 Hardware Triple Fault fallback
    #[repr(C, packed)]
    struct IdtDesc {
        limit: u16,
        base: u64,
    }
    let null_idtr = IdtDesc { limit: 0, base: 0 };
    unsafe {
        core::arch::asm!(
            "lidt [{}]",
            "ud2",
            in(reg) &null_idtr,
            options(noreturn)
        );
    }
}
