extern crate alloc;
use alloc::vec::Vec;
use crate::arch::x86_64::acpi::map_acpi_phys;
use crate::sync::IrqSafeSpinlock;
use crate::kinfo;

#[derive(Debug, Clone, Copy)]
pub struct MadtCpu {
    pub processor_id: u8,
    pub apic_id: u8,
    pub flags: u32,
    pub enabled: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct MadtIoApic {
    pub id: u8,
    pub address: u32,
    pub gsi_base: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct MadtIso {
    pub bus: u8,
    pub source_irq: u8,
    pub gsi: u32,
    pub flags: u16,
}

pub struct MadtInfo {
    pub lapic_address: u64,
    pub flags: u32,
    pub cpus: Vec<MadtCpu>,
    pub io_apics: Vec<MadtIoApic>,
    pub overrides: Vec<MadtIso>,
}

impl MadtInfo {
    pub const fn empty() -> Self {
        Self {
            lapic_address: 0xFEE00000,
            flags: 1, // PC-AT dual 8259 PIC present
            cpus: Vec::new(),
            io_apics: Vec::new(),
            overrides: Vec::new(),
        }
    }
}

pub static MADT: IrqSafeSpinlock<MadtInfo> = IrqSafeSpinlock::new(MadtInfo::empty());

pub fn parse_madt(madt_phys: u64) {
    let Some(hdr_virt) = map_acpi_phys(madt_phys, 44) else {
        return;
    };
    let hdr = hdr_virt as *const u8;
    let len = unsafe { core::ptr::read_unaligned(hdr.add(4) as *const u32) } as usize;
    if len < 44 || len > 1024 * 1024 {
        return;
    }

    let Some(full_virt) = map_acpi_phys(madt_phys, len) else {
        return;
    };
    let virt = full_virt as *const u8;

    let mut lapic_address = unsafe { core::ptr::read_unaligned(virt.add(36) as *const u32) } as u64;
    let flags = unsafe { core::ptr::read_unaligned(virt.add(40) as *const u32) };

    let mut cpus = Vec::new();
    let mut io_apics = Vec::new();
    let mut overrides = Vec::new();

    let mut offset = 44;
    while offset + 2 <= len {
        let entry_type = unsafe { *virt.add(offset) };
        let entry_len = unsafe { *virt.add(offset + 1) } as usize;
        if entry_len == 0 || offset + entry_len > len {
            break;
        }

        match entry_type {
            0 => {
                // Processor Local APIC
                if entry_len >= 8 {
                    let processor_id = unsafe { *virt.add(offset + 2) };
                    let apic_id = unsafe { *virt.add(offset + 3) };
                    let cpu_flags = unsafe { core::ptr::read_unaligned(virt.add(offset + 4) as *const u32) };
                    let enabled = (cpu_flags & 1) != 0 || (cpu_flags & 2) != 0;
                    cpus.push(MadtCpu {
                        processor_id,
                        apic_id,
                        flags: cpu_flags,
                        enabled,
                    });
                }
            }
            1 => {
                // I/O APIC
                if entry_len >= 12 {
                    let id = unsafe { *virt.add(offset + 2) };
                    let address = unsafe { core::ptr::read_unaligned(virt.add(offset + 4) as *const u32) };
                    let gsi_base = unsafe { core::ptr::read_unaligned(virt.add(offset + 8) as *const u32) };
                    io_apics.push(MadtIoApic {
                        id,
                        address,
                        gsi_base,
                    });
                }
            }
            2 => {
                // Interrupt Source Override
                if entry_len >= 10 {
                    let bus = unsafe { *virt.add(offset + 2) };
                    let source_irq = unsafe { *virt.add(offset + 3) };
                    let gsi = unsafe { core::ptr::read_unaligned(virt.add(offset + 4) as *const u32) };
                    let iso_flags = unsafe { core::ptr::read_unaligned(virt.add(offset + 8) as *const u16) };
                    overrides.push(MadtIso {
                        bus,
                        source_irq,
                        gsi,
                        flags: iso_flags,
                    });
                }
            }
            5 => {
                // Local APIC Address Override (64-bit physical address)
                if entry_len >= 12 {
                    let override_addr = unsafe { core::ptr::read_unaligned(virt.add(offset + 4) as *const u64) };
                    if override_addr != 0 {
                        lapic_address = override_addr;
                    }
                }
            }
            9 => {
                // Processor Local x2APIC
                if entry_len >= 16 {
                    let x2apic_id = unsafe { core::ptr::read_unaligned(virt.add(offset + 4) as *const u32) };
                    let cpu_flags = unsafe { core::ptr::read_unaligned(virt.add(offset + 8) as *const u32) };
                    let acpi_uid = unsafe { core::ptr::read_unaligned(virt.add(offset + 12) as *const u32) };
                    let enabled = (cpu_flags & 1) != 0 || (cpu_flags & 2) != 0;
                    if x2apic_id < 256 && !cpus.iter().any(|c| c.apic_id as u32 == x2apic_id) {
                        cpus.push(MadtCpu {
                            processor_id: acpi_uid as u8,
                            apic_id: x2apic_id as u8,
                            flags: cpu_flags,
                            enabled,
                        });
                    }
                }
            }
            _ => {}
        }

        offset += entry_len;
    }

    kinfo!(
        "ACPI MADT parsed: LAPIC Base: 0x{:X}, {} CPU core(s), {} I/O APIC(s)",
        lapic_address,
        cpus.len(),
        io_apics.len()
    );

    let mut madt = MADT.lock();
    madt.lapic_address = lapic_address;
    madt.flags = flags;
    madt.cpus = cpus;
    madt.io_apics = io_apics;
    madt.overrides = overrides;
}
