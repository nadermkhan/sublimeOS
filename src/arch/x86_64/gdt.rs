use core::arch::asm;
use core::mem::size_of;

pub const KERNEL_CS: u16 = 0x08;
pub const KERNEL_DS: u16 = 0x10;
pub const USER_DS: u16 = 0x18 | 3;
pub const USER_CS: u16 = 0x20 | 3;
pub const TSS_SELECTOR: u16 = 0x28;

pub const DOUBLE_FAULT_IST_INDEX: u16 = 1;
pub const PAGE_FAULT_IST_INDEX: u16 = 2;

#[repr(C, packed)]
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
struct IstStacks {
    double_fault_stack: [u8; 16384],
    page_fault_stack: [u8; 16384],
}

static mut IST_STACKS: IstStacks = IstStacks {
    double_fault_stack: [0; 16384],
    page_fault_stack: [0; 16384],
};

pub static mut TSS: TaskStateSegment = TaskStateSegment::new();

#[repr(C, packed)]
struct GdtDescriptor {
    size: u16,
    offset: u64,
}

#[repr(C, align(8))]
struct GdtTable {
    entries: [u64; 7], // 0: null, 1: kcode, 2: kdata, 3: udata, 4: ucode, 5-6: tss (16 bytes)
}

static mut GDT: GdtTable = GdtTable {
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

/// Initialize GDT, configure TSS with IST stacks and kernel stack, and load TSS register.
pub fn init() {
    unsafe {
        // Set up IST stacks using raw pointers
        let df_stack_top = core::ptr::addr_of!(IST_STACKS.double_fault_stack) as u64 + 16384;
        let pf_stack_top = core::ptr::addr_of!(IST_STACKS.page_fault_stack) as u64 + 16384;

        TSS.ist[DOUBLE_FAULT_IST_INDEX as usize - 1] = df_stack_top;
        TSS.ist[PAGE_FAULT_IST_INDEX as usize - 1] = pf_stack_top;

        // Load 16-byte TSS descriptor into GDT
        let tss_ptr = core::ptr::addr_of!(TSS) as u64;
        let tss_size = (size_of::<TaskStateSegment>() - 1) as u64;

        // TSS Low (64-bit)
        let mut low = 0x0000E90000000000u64; // Present, DPL 3 (or 0), Type 9 (64-bit TSS available)
        low |= tss_size & 0xFFFF;
        low |= (tss_ptr & 0x00FFFFFF) << 16;
        low |= (tss_ptr & 0xFF000000) << 32;

        // TSS High (64-bit)
        let high = tss_ptr >> 32;

        GDT.entries[5] = low;
        GDT.entries[6] = high;

        // Load GDT
        let gdt_desc = GdtDescriptor {
            size: (size_of::<GdtTable>() - 1) as u16,
            offset: core::ptr::addr_of!(GDT.entries) as u64,
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

        // Load Task Register (LTR)
        asm!("ltr {0:x}", in(reg) TSS_SELECTOR, options(nostack, preserves_flags));
    }
}

/// Set the kernel stack pointer (RSP0) in the TSS for user-to-kernel transitions.
pub fn set_kernel_stack(stack_top: u64) {
    unsafe {
        TSS.rsp[0] = stack_top;
    }
}
