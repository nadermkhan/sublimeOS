extern crate alloc;
use alloc::vec::Vec;
use core::sync::atomic::{fence, AtomicBool, AtomicU64, Ordering};
use crate::arch::x86_64::apic;
use crate::arch::x86_64::madt::MADT;
use crate::arch::x86_64::pit::{delay_ms, delay_us};
use crate::arch::x86_64::registers::read_cr3;
use crate::mm::phys_to_virt;
use crate::mm::pmm::PMM;
use crate::mm::vmm::{VMM, ADDRESS_MASK};
use crate::sync::IrqSafeSpinlock;
use crate::{kinfo, kwarn};

pub const AP_TRAMPOLINE_PHYS: u64 = 0x8000;
pub const AP_SIPI_VECTOR: u8 = 0x08; // 0x08 * 4096 == 0x8000
const AP_STACK_PAGES: usize = 4; // 16 KiB per AP core

extern "C" {
    static ap_trampoline_start: u8;
    static ap_trampoline_end: u8;
    static ap_mailbox_cr3: u8;
    static ap_mailbox_stack: u8;
    static ap_mailbox_entry: u8;
    static ap_mailbox_arg: u8;
    static ap_gdt: u8;
    static ap_code_start: u8;
}

const ATOMIC_FALSE: AtomicBool = AtomicBool::new(false);
const ATOMIC_ZERO: AtomicU64 = AtomicU64::new(0);
static AP_BOOT_ACK: [AtomicBool; 256] = [ATOMIC_FALSE; 256];
static AP_STACK_TOPS: [AtomicU64; 256] = [ATOMIC_ZERO; 256];
static TRAMPOLINE_STAGED: AtomicBool = AtomicBool::new(false);

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

/// Compute symbol offset within the 16-bit AP trampoline blob.
#[inline]
unsafe fn trampoline_offset(sym: *const u8) -> usize {
    let start = core::ptr::addr_of!(ap_trampoline_start) as usize;
    (sym as usize).saturating_sub(start)
}

/// Copy the 16-bit -> 64-bit AP trampoline image into low physical page 0x8000.
fn install_trampoline_page() -> Result<(), &'static str> {
    let start_ptr = core::ptr::addr_of!(ap_trampoline_start);
    let end_ptr = core::ptr::addr_of!(ap_trampoline_end);
    let len = (end_ptr as usize).saturating_sub(start_ptr as usize);
    if len == 0 || len > 4096 {
        return Err("AP trampoline exceeds single 4KiB SIPI page");
    }
    let mailbox_off = unsafe { trampoline_offset(core::ptr::addr_of!(ap_mailbox_cr3)) };
    let gdt_off = unsafe { trampoline_offset(core::ptr::addr_of!(ap_gdt)) };
    if mailbox_off != 0x08 || gdt_off != 0x28 {
        return Err("AP trampoline fixed header layout mismatch");
    }

    // Explicitly guarantee physical page 0x8000 is reserved from general allocation
    PMM.lock().reserve_region(AP_TRAMPOLINE_PHYS, 4096);

    // Ensure low 1 GiB identity map is still active in PML4[0] so APs survive CR0.PG
    let pml4 = VMM.lock().get_kernel_pml4();
    if !pml4.entries[0].is_present() {
        return Err("Kernel PML4[0] identity map absent for AP Long Mode transition");
    }

    let cr3_phys = unsafe { read_cr3() } & ADDRESS_MASK;
    if cr3_phys >= 0x1_0000_0000 {
        return Err("Kernel PML4 physical address must be below 4 GiB for 32-bit AP load");
    }

    let dst = phys_to_virt(AP_TRAMPOLINE_PHYS) as *mut u8;
    unsafe {
        core::ptr::write_bytes(dst, 0, 4096);
        core::ptr::copy_nonoverlapping(start_ptr, dst, len);
    }
    fence(Ordering::SeqCst);
    TRAMPOLINE_STAGED.store(true, Ordering::Release);
    Ok(())
}

/// Write per-AP mailbox fields (CR3, stack top, Long Mode entry fn, arg) before SIPI.
unsafe fn prepare_ap_mailbox(apic_id: u8, stack_top: u64) {
    let base = phys_to_virt(AP_TRAMPOLINE_PHYS) as *mut u8;
    let cr3_phys = read_cr3() & ADDRESS_MASK;

    let off_cr3 = trampoline_offset(core::ptr::addr_of!(ap_mailbox_cr3));
    let off_stack = trampoline_offset(core::ptr::addr_of!(ap_mailbox_stack));
    let off_entry = trampoline_offset(core::ptr::addr_of!(ap_mailbox_entry));
    let off_arg = trampoline_offset(core::ptr::addr_of!(ap_mailbox_arg));

    core::ptr::write_volatile(base.add(off_cr3) as *mut u64, cr3_phys);
    core::ptr::write_volatile(base.add(off_stack) as *mut u64, stack_top);
    core::ptr::write_volatile(base.add(off_entry) as *mut u64, ap_kernel_entry as *const () as u64);
    core::ptr::write_volatile(base.add(off_arg) as *mut u64, apic_id as u64);
    fence(Ordering::SeqCst);
}

fn get_or_alloc_ap_stack(apic_id: u8) -> Option<u64> {
    let existing = AP_STACK_TOPS[apic_id as usize].load(Ordering::Acquire);
    if existing != 0 {
        return Some(existing);
    }
    let stack_phys = PMM.lock().alloc_contiguous_frames(AP_STACK_PAGES)?;
    let stack_top = phys_to_virt(stack_phys) + (AP_STACK_PAGES * 4096) as u64;
    AP_STACK_TOPS[apic_id as usize].store(stack_top, Ordering::Release);
    Some(stack_top)
}

/// Higher-half 64-bit Rust entry point reached by each secondary AP core after
/// transitioning Real Mode -> 32-bit Protected Mode -> 64-bit Long Mode at 0x8000.
#[no_mangle]
pub extern "C" fn ap_kernel_entry(apic_id_arg: u64) -> ! {
    unsafe {
        crate::arch::x86_64::registers::enable_sse();
        crate::arch::x86_64::registers::enable_write_combining();
        crate::arch::x86_64::gdt::load_kernel_gdt_for_ap();
        crate::arch::x86_64::idt::load_idt();
    }
    crate::arch::x86_64::apic::init_ap();

    let hw_apic_id = apic::get_apic_id();
    AP_BOOT_ACK[hw_apic_id as usize].store(true, Ordering::SeqCst);
    AP_BOOT_ACK[(apic_id_arg as u8) as usize].store(true, Ordering::SeqCst);
    fence(Ordering::SeqCst);

    loop {
        unsafe {
            core::arch::asm!("cli; hlt", options(nomem, nostack));
        }
    }
}

/// Check if the immutable entry stub and 16->64 code sections of the AP trampoline at 0x8000
/// remain intact (excluding the mutable mailbox and CPU-touched GDT descriptors).
pub fn is_trampoline_staged() -> bool {
    if !TRAMPOLINE_STAGED.load(Ordering::Acquire) {
        return false;
    }
    let start_ptr = core::ptr::addr_of!(ap_trampoline_start);
    let end_ptr = core::ptr::addr_of!(ap_trampoline_end);
    let total_len = (end_ptr as usize).saturating_sub(start_ptr as usize);
    let stub_len = unsafe { trampoline_offset(core::ptr::addr_of!(ap_mailbox_cr3)) };
    let code_off = unsafe { trampoline_offset(core::ptr::addr_of!(ap_code_start)) };
    if stub_len == 0 || code_off >= total_len {
        return false;
    }

    let expected = unsafe { core::slice::from_raw_parts(start_ptr, total_len) };
    let actual = unsafe { core::slice::from_raw_parts(phys_to_virt(AP_TRAMPOLINE_PHYS) as *const u8, total_len) };
    expected[..stub_len] == actual[..stub_len] && expected[code_off..] == actual[code_off..]
}

/// Check whether a specific APIC ID has acknowledged booting through the Long Mode trampoline.
pub fn has_ap_acknowledged(apic_id: u8) -> bool {
    AP_BOOT_ACK[apic_id as usize].load(Ordering::Acquire)
}

/// Issue Intel INIT-SIPI-SIPI sequence targeting `apic_id` with its dedicated stack
/// and poll for actual 64-bit Long Mode acknowledgement from `ap_kernel_entry`.
fn wake_ap_core(apic_id: u8, stack_top: u64) -> bool {
    if !is_trampoline_staged() {
        return false;
    }
    AP_BOOT_ACK[apic_id as usize].store(false, Ordering::SeqCst);
    unsafe {
        prepare_ap_mailbox(apic_id, stack_top);
    }

    // 1. Send INIT IPI to reset AP core and wait 10ms
    apic::send_init_ipi(apic_id);
    delay_ms(10);

    // 2. Send first Startup IPI (vector 0x08 -> physical 0x8000)
    apic::send_sipi(apic_id, AP_SIPI_VECTOR);
    for _ in 0..50 {
        if AP_BOOT_ACK[apic_id as usize].load(Ordering::Acquire) {
            return true;
        }
        delay_us(100);
    }

    // 3. Send second SIPI if AP did not acknowledge first SIPI within 5ms
    apic::send_sipi(apic_id, AP_SIPI_VECTOR);
    for _ in 0..500 {
        if AP_BOOT_ACK[apic_id as usize].load(Ordering::Acquire) {
            return true;
        }
        delay_us(100);
    }

    false
}

/// Safely restage the 16-bit trampoline mailbox for `apic_id` and re-execute INIT-SIPI-SIPI.
pub fn reboot_ap(apic_id: u8) -> bool {
    let Some(stack_top) = get_or_alloc_ap_stack(apic_id) else {
        return false;
    };
    wake_ap_core(apic_id, stack_top)
}

/// Initialize SMP subsystem, staging the 16-bit trampoline at 0x8000 and waking enabled APs.
pub fn init() {
    let bsp_apic_id = apic::get_apic_id();
    let madt = MADT.lock();

    let mut cores = Vec::new();

    if madt.cpus.is_empty() {
        cores.push(CpuCore {
            processor_id: 0,
            apic_id: bsp_apic_id,
            is_bsp: true,
            online: true,
        });
    } else {
        for cpu in madt.cpus.iter() {
            let is_bsp = cpu.apic_id == bsp_apic_id;
            if is_bsp || cpu.enabled {
                cores.push(CpuCore {
                    processor_id: cpu.processor_id,
                    apic_id: cpu.apic_id,
                    is_bsp,
                    online: is_bsp, // BSP is already online; APs must acknowledge trampoline
                });
            }
        }
        if !cores.iter().any(|c| c.is_bsp) {
            cores.push(CpuCore {
                processor_id: 0,
                apic_id: bsp_apic_id,
                is_bsp: true,
                online: true,
            });
        }
    }
    drop(madt);

    if let Err(e) = install_trampoline_page() {
        kwarn!("SMP: Unable to install 16-bit AP trampoline at 0x{:X}: {}", AP_TRAMPOLINE_PHYS, e);
    }

    kinfo!(
        "SMP Subsystem: {} enabled CPU core(s) detected (BSP APIC ID {}, trampoline @ 0x{:X})",
        cores.len(),
        bsp_apic_id,
        AP_TRAMPOLINE_PHYS
    );

    // Boot enabled APs using Intel INIT-SIPI-SIPI protocol with interrupt-safe delays & ACK verification
    if is_trampoline_staged() {
        for core in cores.iter_mut() {
            if core.is_bsp {
                continue;
            }
            let Some(stack_top) = get_or_alloc_ap_stack(core.apic_id) else {
                kwarn!("SMP: Out of frames for APIC ID {} kernel stack", core.apic_id);
                continue;
            };

            let booted = wake_ap_core(core.apic_id, stack_top);
            core.online = booted;
            if booted {
                kinfo!(
                    "SMP: AP core Online (APIC ID {}, Processor ID {}) via 16->64 trampoline",
                    core.apic_id,
                    core.processor_id
                );
            } else {
                kwarn!(
                    "SMP: AP core (APIC ID {}, Processor ID {}) failed to respond to SIPI timeout",
                    core.apic_id,
                    core.processor_id
                );
            }
        }
    }

    let core_count = cores.len();
    let mut smp = SMP.lock();
    smp.cores = cores;
    smp.bsp_apic_id = bsp_apic_id;
    drop(smp);

    crate::task::scheduler::SCHEDULER.lock().ensure_cores(core_count);
}
