use core::arch::asm;
use core::mem::size_of;
use core::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

pub const KERNEL_CS: u16 = 0x08;
pub const KERNEL_DS: u16 = 0x10;
pub const USER_DS: u16 = 0x18 | 3;
pub const USER_CS: u16 = 0x20 | 3;
pub const TSS_SELECTOR: u16 = 0x28;

pub const DOUBLE_FAULT_IST_INDEX: u16 = 1;
pub const MAX_CPUS: usize = 32;
const DF_STACK_SIZE: usize = 16384;

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct TaskStateSegment {
    reserved1: u32,
    pub rsp: [u64; 3], // rsp0, rsp1, rsp2
    reserved2: u64,
    pub ist: [u64; 7], // ist1 to ist7
    reserved3: u64,
    reserved4: u16,
    pub iomap_base: u16,
}

impl TaskStateSegment {
    pub const fn new() -> Self {
        Self {
            reserved1: 0,
            rsp: [0; 3],
            reserved2: 0,
            ist: [0; 7],
            reserved3: 0,
            reserved4: 0,
            iomap_base: size_of::<TaskStateSegment>() as u16,
        }
    }
}

#[repr(C, align(16))]
#[derive(Clone, Copy)]
struct DfStack([u8; DF_STACK_SIZE]);

static mut DF_STACKS: [DfStack; MAX_CPUS] = [DfStack([0; DF_STACK_SIZE]); MAX_CPUS];
pub static mut TSS_ARRAY: [TaskStateSegment; MAX_CPUS] = [TaskStateSegment::new(); MAX_CPUS];

#[repr(C, packed)]
struct GdtDescriptor {
    size: u16,
    offset: u64,
}

#[repr(C, align(8))]
#[derive(Clone, Copy)]
struct GdtTable {
    entries: [u64; 7], // 0: null, 1: kcode, 2: kdata, 3: udata, 4: ucode, 5-6: tss (16 bytes)
}

const EMPTY_GDT: GdtTable = GdtTable {
    entries: [
        0x0000000000000000, // 0x00: Null
        0x00209A0000000000, // 0x08: Kernel Code 64-bit (DPL 0)
        0x0000920000000000, // 0x10: Kernel Data (DPL 0)
        0x0000F20000000000, // 0x18: User Data (DPL 3)
        0x0020FA0000000000, // 0x20: User Code 64-bit (DPL 3)
        0,                  // 0x28: TSS Low
        0,                  // 0x30: TSS High
    ],
};

static mut GDT_ARRAY: [GdtTable; MAX_CPUS] = [EMPTY_GDT; MAX_CPUS];

const UNMAPPED_SLOT: AtomicU8 = AtomicU8::new(0xFF);
static APIC_TO_SLOT: [AtomicU8; 256] = [UNMAPPED_SLOT; 256];
static NEXT_CPU_SLOT: AtomicUsize = AtomicUsize::new(1);

#[inline]
pub fn current_cpu_slot() -> usize {
    let apic_id = crate::arch::x86_64::apic::get_apic_id() as usize;
    let slot = APIC_TO_SLOT[apic_id & 0xFF].load(Ordering::Acquire) as usize;
    if slot < MAX_CPUS {
        slot
    } else {
        0
    }
}

unsafe fn configure_and_load_slot(slot: usize) {
    let df_stack_top = core::ptr::addr_of!(DF_STACKS[slot].0) as u64 + DF_STACK_SIZE as u64;
    TSS_ARRAY[slot].ist[DOUBLE_FAULT_IST_INDEX as usize - 1] = df_stack_top;

    let tss_ptr = core::ptr::addr_of!(TSS_ARRAY[slot]) as u64;
    let tss_size = (size_of::<TaskStateSegment>() - 1) as u64;

    // TSS Low (64-bit): Present, DPL 0, Type 9 (64-bit TSS available)
    let mut low = 0x0000890000000000u64;
    low |= tss_size & 0xFFFF;
    low |= (tss_ptr & 0x00FFFFFF) << 16;
    low |= (tss_ptr & 0xFF000000) << 32;

    let high = tss_ptr >> 32;

    GDT_ARRAY[slot].entries[5] = low;
    GDT_ARRAY[slot].entries[6] = high;

    reload_kernel_gdt_segments(slot);
    asm!("ltr {0:x}", in(reg) TSS_SELECTOR, options(nostack, preserves_flags));
}

/// Initialize GDT, configure per-CPU TSS with IST1 stack, and load TSS register on BSP.
pub fn init() {
    let bsp_apic = crate::arch::x86_64::apic::get_apic_id() as usize;
    APIC_TO_SLOT[bsp_apic & 0xFF].store(0, Ordering::Release);
    unsafe {
        configure_and_load_slot(0);
    }
}

/// Initialize a dedicated per-CPU GDT and TSS on an Application Processor (AP) and load TR.
pub unsafe fn load_kernel_gdt_for_ap() {
    let apic_id = crate::arch::x86_64::apic::get_apic_id() as usize & 0xFF;
    let existing = APIC_TO_SLOT[apic_id].load(Ordering::Acquire);
    let slot = if (existing as usize) < MAX_CPUS {
        existing as usize
    } else {
        let claimed = NEXT_CPU_SLOT.fetch_add(1, Ordering::AcqRel);
        let bounded = if claimed < MAX_CPUS { claimed } else { MAX_CPUS - 1 };
        APIC_TO_SLOT[apic_id].store(bounded as u8, Ordering::Release);
        bounded
    };
    configure_and_load_slot(slot);
}

unsafe fn reload_kernel_gdt_segments(slot: usize) {
    let gdt_desc = GdtDescriptor {
        size: (size_of::<GdtTable>() - 1) as u16,
        offset: core::ptr::addr_of!(GDT_ARRAY[slot].entries) as u64,
    };

    asm!("lgdt [{}]", in(reg) &gdt_desc, options(readonly, nostack, preserves_flags));

    // Reload segment registers
    asm!(
        "push {kcs}",
        "lea {tmp}, [2f + rip]",
        "push {tmp}",
        "retfq",
        "2:",
        "mov ds, {kds:x}",
        "mov es, {kds:x}",
        "mov fs, {kds:x}",
        "mov gs, {kds:x}",
        "mov ss, {kds:x}",
        kcs = in(reg) (KERNEL_CS as u64),
        kds = in(reg) (KERNEL_DS),
        tmp = out(reg) _,
        options(preserves_flags)
    );
}

/// Set the kernel stack pointer (RSP0) in the executing core's TSS for user-to-kernel transitions.
pub fn set_kernel_stack(stack_top: u64) {
    let slot = current_cpu_slot();
    unsafe {
        core::ptr::write_volatile(core::ptr::addr_of_mut!(TSS_ARRAY[slot].rsp[0]), stack_top);
    }
}

/// Get the current kernel stack pointer (RSP0) in the executing core's TSS.
pub fn get_kernel_stack() -> u64 {
    let slot = current_cpu_slot();
    unsafe {
        core::ptr::read_volatile(core::ptr::addr_of!(TSS_ARRAY[slot].rsp[0]))
    }
}
