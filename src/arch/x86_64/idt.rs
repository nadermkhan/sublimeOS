use core::arch::asm;
use core::mem::size_of;
use crate::arch::x86_64::gdt::{DOUBLE_FAULT_IST_INDEX, KERNEL_CS, PAGE_FAULT_IST_INDEX};
use crate::arch::x86_64::registers::read_cr2;
use crate::kerror;

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct IdtEntry {
    offset_low: u16,
    gdt_selector: u16,
    ist: u8,
    type_attr: u8,
    offset_mid: u16,
    offset_high: u32,
    reserved: u32,
}

impl IdtEntry {
    pub const fn empty() -> Self {
        Self {
            offset_low: 0,
            gdt_selector: 0,
            ist: 0,
            type_attr: 0,
            offset_mid: 0,
            offset_high: 0,
            reserved: 0,
        }
    }

    pub fn set_handler(&mut self, handler: usize, ist_index: u8, dpl: u8) {
        self.offset_low = handler as u16;
        self.gdt_selector = KERNEL_CS;
        self.ist = ist_index & 0x7;
        self.type_attr = 0x8E | ((dpl & 0x3) << 5); // 0x8E = 64-bit Interrupt Gate, Present
        self.offset_mid = (handler >> 16) as u16;
        self.offset_high = (handler >> 32) as u32;
        self.reserved = 0;
    }
}

#[repr(C, packed)]
struct IdtDescriptor {
    size: u16,
    offset: u64,
}

#[repr(C, align(16))]
pub struct IdtTable {
    pub entries: [IdtEntry; 256],
}

pub static mut IDT: IdtTable = IdtTable {
    entries: [IdtEntry::empty(); 256],
};

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct InterruptContext {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rbp: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rbx: u64,
    pub rax: u64,
    pub vector: u64,
    pub error_code: u64,
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

extern "C" {
    fn isr_0(); fn isr_1(); fn isr_2(); fn isr_3();
    fn isr_4(); fn isr_5(); fn isr_6(); fn isr_7();
    fn isr_8(); fn isr_9(); fn isr_10(); fn isr_11();
    fn isr_12(); fn isr_13(); fn isr_14(); fn isr_15();
    fn isr_16(); fn isr_17(); fn isr_18(); fn isr_19();
    fn isr_20(); fn isr_21(); fn isr_22(); fn isr_23();
    fn isr_24(); fn isr_25(); fn isr_26(); fn isr_27();
    fn isr_28(); fn isr_29(); fn isr_30(); fn isr_31();

    fn isr_32(); fn isr_33(); fn isr_34(); fn isr_35();
    fn isr_36(); fn isr_37(); fn isr_38(); fn isr_39();
    fn isr_40(); fn isr_41(); fn isr_42(); fn isr_43();
    fn isr_44(); fn isr_45(); fn isr_46(); fn isr_47();

    fn isr_128();
    fn isr_255();
}

pub fn init() {
    unsafe {
        let isrs: [unsafe extern "C" fn(); 32] = [
            isr_0, isr_1, isr_2, isr_3, isr_4, isr_5, isr_6, isr_7,
            isr_8, isr_9, isr_10, isr_11, isr_12, isr_13, isr_14, isr_15,
            isr_16, isr_17, isr_18, isr_19, isr_20, isr_21, isr_22, isr_23,
            isr_24, isr_25, isr_26, isr_27, isr_28, isr_29, isr_30, isr_31,
        ];

        for (vec, &isr) in isrs.iter().enumerate() {
            let ist = match vec {
                8 => DOUBLE_FAULT_IST_INDEX as u8,
                14 => PAGE_FAULT_IST_INDEX as u8,
                _ => 0,
            };
            IDT.entries[vec].set_handler(isr as *const () as usize, ist, 0);
        }

        // Hardware IRQs (32..47)
        let irqs: [unsafe extern "C" fn(); 16] = [
            isr_32, isr_33, isr_34, isr_35, isr_36, isr_37, isr_38, isr_39,
            isr_40, isr_41, isr_42, isr_43, isr_44, isr_45, isr_46, isr_47,
        ];

        for (i, &irq) in irqs.iter().enumerate() {
            IDT.entries[32 + i].set_handler(irq as *const () as usize, 0, 0);
        }

        // Syscall `int 0x80` (Vector 128) accessible from user space (DPL 3)
        IDT.entries[128].set_handler(isr_128 as *const () as usize, 0, 3);

        // Spurious interrupt (Vector 255)
        IDT.entries[255].set_handler(isr_255 as *const () as usize, 0, 0);

        let idt_desc = IdtDescriptor {
            size: (size_of::<IdtTable>() - 1) as u16,
            offset: core::ptr::addr_of!(IDT.entries) as u64,
        };

        asm!("lidt [{}]", in(reg) &idt_desc, options(readonly, nostack, preserves_flags));
    }
}

pub type IrqHandlerFn = fn(&mut InterruptContext);
static mut IRQ_HANDLERS: [Option<IrqHandlerFn>; 16] = [None; 16];

pub fn register_irq_handler(irq: usize, handler: IrqHandlerFn) {
    if irq < 16 {
        unsafe {
            IRQ_HANDLERS[irq] = Some(handler);
        }
    }
}

#[no_mangle]
pub extern "C" fn interrupt_dispatch(ctx: *mut InterruptContext) {
    let ctx_ref = unsafe { &mut *ctx };
    let vec = ctx_ref.vector as usize;

    match vec {
        // CPU Exceptions
        0..=31 => handle_exception(ctx_ref),

        // Hardware IRQs (32..47)
        32..=47 => {
            let irq = vec - 32;
            // Acknowledge interrupt to PIC before dispatching handler
            crate::arch::x86_64::pic::send_eoi(irq as u8);

            unsafe {
                if let Some(handler) = IRQ_HANDLERS[irq] {
                    handler(ctx_ref);
                }
            }
        }

        // Syscall via int 0x80
        128 => {
            crate::syscall::handle_syscall_int80(ctx_ref);
        }

        // Spurious vector
        255 => {}

        _ => {
            kerror!("Unhandled Interrupt Vector {}", vec);
        }
    }
}

fn handle_exception(ctx: &InterruptContext) {
    match ctx.vector {
        14 => {
            // Page Fault
            let fault_addr = unsafe { read_cr2() };
            let err = ctx.error_code;
            let present = (err & 1) != 0;
            let write = (err & 2) != 0;
            let user = (err & 4) != 0;
            let instruction_fetch = (err & 16) != 0;

            panic!(
                "PAGE FAULT at virtual address 0x{:016X}!\n  RIP: 0x{:016X}\n  Error Code: 0x{:X} (present: {}, write: {}, user: {}, fetch: {})",
                fault_addr, ctx.rip, err, present, write, user, instruction_fetch
            );
        }
        8 => {
            panic!(
                "DOUBLE FAULT Exception!\n  RIP: 0x{:016X}, RSP: 0x{:016X}, Error Code: 0x{:X}",
                ctx.rip, ctx.rsp, ctx.error_code
            );
        }
        13 => {
            panic!(
                "GENERAL PROTECTION FAULT!\n  RIP: 0x{:016X}, Error Code: 0x{:X}",
                ctx.rip, ctx.error_code
            );
        }
        vec => {
            panic!(
                "CPU EXCEPTION #{}! RIP: 0x{:016X}, Error: 0x{:X}, RFLAGS: 0x{:X}",
                vec, ctx.rip, ctx.error_code, ctx.rflags
            );
        }
    }
}
