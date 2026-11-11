extern crate alloc;
use alloc::vec::Vec;
use crate::arch::x86_64::apic;
use crate::arch::x86_64::madt::MADT;
use crate::sync::IrqSafeSpinlock;
use crate::kinfo;

#[derive(Debug, Clone, Copy)]
pub struct CpuCore {
    pub processor_id: u8,
    pub apic_id: u8,
    pub is_bsp: bool,
    pub online: bool,
}

pub struct SmpManager {
    pub cores: Vec<CpuCore>,
    pub bsp_apic_id: u8,
}

impl SmpManager {
    pub const fn new() -> Self {
        Self {
            cores: Vec::new(),
            bsp_apic_id: 0,
        }
    }

    pub fn cpu_count(&self) -> usize {
        if self.cores.is_empty() {
            1
        } else {
            self.cores.len()
        }
    }

    pub fn online_count(&self) -> usize {
        if self.cores.is_empty() {
            1
        } else {
            self.cores.iter().filter(|c| c.online).count()
        }
    }
}

pub static SMP: IrqSafeSpinlock<SmpManager> = IrqSafeSpinlock::new(SmpManager::new());

/// Initialize SMP subsystem, detecting and waking Application Processors (APs).
pub fn init() {
    let bsp_apic_id = apic::get_apic_id();
    let madt = MADT.lock();

    let mut cores = Vec::new();

    if madt.cpus.is_empty() {
        // Fallback: 1 single core (BSP)
        cores.push(CpuCore {
            processor_id: 0,
            apic_id: bsp_apic_id,
            is_bsp: true,
            online: true,
        });
    } else {
        for cpu in madt.cpus.iter() {
            let is_bsp = cpu.apic_id == bsp_apic_id;
            cores.push(CpuCore {
                processor_id: cpu.processor_id,
                apic_id: cpu.apic_id,
                is_bsp,
                online: is_bsp, // BSP is already online
            });
        }
    }
    drop(madt);

    kinfo!(
        "SMP Subsystem: {} CPU core(s) detected. BSP APIC ID: {}",
        cores.len(),
        bsp_apic_id
    );

    // Boot APs using INIT-SIPI-SIPI sequence
    for core in cores.iter_mut() {
        if !core.is_bsp {
            // 1. Send INIT IPI to reset AP core
            apic::send_init_ipi(core.apic_id);
            crate::arch::x86_64::pit::sleep_ms(10);

            // 2. Send Startup IPI (vector 0x08 = page at 0x8000 real-mode trampoline)
            apic::send_sipi(core.apic_id, 0x08);
            crate::arch::x86_64::pit::sleep_ms(1);

            // 3. Send second SIPI
            apic::send_sipi(core.apic_id, 0x08);
            crate::arch::x86_64::pit::sleep_ms(1);

            core.online = true;
            kinfo!("SMP: Initialized AP core (APIC ID {}, Processor ID {})", core.apic_id, core.processor_id);
        }
    }

    let core_count = cores.len();
    let mut smp = SMP.lock();
    smp.cores = cores;
    smp.bsp_apic_id = bsp_apic_id;
    drop(smp);

    crate::task::scheduler::SCHEDULER.lock().ensure_cores(core_count);
}
