extern crate alloc;
use alloc::vec::Vec;
use crate::mm::phys_to_virt;
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
    pub lapic_address: u32,
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
    let virt = phys_to_virt(madt_phys) as *const u8;
    let len = unsafe { *(virt.add(4) as *const u32) } as usize;
    if len < 44 {
        return;
    }

    let lapic_address = unsafe { *(virt.add(36) as *const u32) };
    let flags = unsafe { *(virt.add(40) as *const u32) };

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
                    let cpu_flags = unsafe { *(virt.add(offset + 4) as *const u32) };
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
                    let address = unsafe { *(virt.add(offset + 4) as *const u32) };
                    let gsi_base = unsafe { *(virt.add(offset + 8) as *const u32) };
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
                    let gsi = unsafe { *(virt.add(offset + 4) as *const u32) };
                    let iso_flags = unsafe { *(virt.add(offset + 8) as *const u16) };
                    overrides.push(MadtIso {
                        bus,
                        source_irq,
                        gsi,
                        flags: iso_flags,
                    });
                }
            }
            _ => {}
        }

        offset += entry_len;
    }

    kinfo!(
        "ACPI MADT parsed: LAPIC Base: 0x{:08X}, {} CPU core(s), {} I/O APIC(s)",
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
