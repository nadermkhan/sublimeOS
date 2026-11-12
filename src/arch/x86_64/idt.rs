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
    fn isr_48();

    fn isr_128();
    fn isr_255();
}

pub const LAPIC_TIMER_VECTOR: usize = 48;

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

        // Local APIC MMIO Periodic Timer (Vector 48)
        IDT.entries[LAPIC_TIMER_VECTOR].set_handler(isr_48 as *const () as usize, 0, 0);

        // Syscall `int 0x80` (Vector 128) accessible from user space (DPL 3)
        IDT.entries[128].set_handler(isr_128 as *const () as usize, 0, 3);

        // Spurious interrupt (Vector 255)
        IDT.entries[255].set_handler(isr_255 as *const () as usize, 0, 0);

        load_idt();
    }
}

/// Load the initialized global IDT on the executing CPU core.
pub unsafe fn load_idt() {
    let idt_desc = IdtDescriptor {
        size: (size_of::<IdtTable>() - 1) as u16,
        offset: core::ptr::addr_of!(IDT.entries) as u64,
    };

    asm!("lidt [{}]", in(reg) &idt_desc, options(readonly, nostack, preserves_flags));
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

        // Spurious 8259 PIC interrupts (IRQ 7 -> vec 39, IRQ 15 -> vec 47).
        // Per x86 APIC/PIC specification, ignore without issuing EOI.
        39 | 47 => return,

        // Hardware IRQs routed via I/O APIC (32..=47)
        32..=47 => {
            let irq = vec - 32;
            unsafe {
                if let Some(handler) = IRQ_HANDLERS[irq] {
                    handler(ctx_ref);
                }
            }
            // Acknowledge Local APIC EOI so the In-Service Register (ISR) clears cleanly
            crate::arch::x86_64::apic::eoi();
        }

        // Local APIC MMIO Timer Interrupt (Vector 48)
        LAPIC_TIMER_VECTOR => {
            crate::arch::x86_64::apic::eoi();
            crate::arch::x86_64::pit::advance_system_tick(ctx_ref);
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
    if ctx.vector == 14 {
        let fault_addr = unsafe { read_cr2() };
        let is_present = (ctx.error_code & 1) != 0;
        let is_write = (ctx.error_code & 2) != 0;
        let current_cr3 = unsafe { crate::arch::x86_64::registers::read_cr3() };

        if is_present && is_write {
            if crate::mm::vmm::VMM.lock().handle_cow_fault(current_cr3, fault_addr).is_ok() {
                return; // CoW successfully handled: frame duplicated, permissions restored!
            }
        }

        if !is_present {
            if crate::mm::vma::handle_demand_fault(current_cr3, fault_addr, is_write).is_ok() {
                return; // Demand paging or swap-in successfully allocated and mapped frame!
            }
        }
    }

    let is_user_mode = (ctx.cs & 3) == 3 || (ctx.vector == 14 && (ctx.error_code & 4) != 0);
    if is_user_mode {
        if ctx.vector == 14 {
            let fault_addr = unsafe { read_cr2() };
            crate::kerror!("User Segmentation Fault (SIGSEGV) at 0x{:016X}! RIP: 0x{:016X}", fault_addr, ctx.rip);
            crate::task::user::handle_user_fault(139);
        } else if ctx.vector == 13 {
            crate::kerror!("User General Protection Fault (SIGSEGV) at RIP: 0x{:016X}", ctx.rip);
            crate::task::user::handle_user_fault(139);
        } else if ctx.vector == 6 {
            crate::kerror!("User Illegal Instruction (SIGILL) at RIP: 0x{:016X}", ctx.rip);
            crate::task::user::handle_user_fault(132);
        } else if ctx.vector == 0 {
            crate::kerror!("User Divide By Zero (SIGFPE) at RIP: 0x{:016X}", ctx.rip);
            crate::task::user::handle_user_fault(136);
        } else {
            crate::kerror!("User CPU Exception #{} at RIP: 0x{:016X}", ctx.vector, ctx.rip);
            crate::task::user::handle_user_fault(128 + ctx.vector as i32);
        }
    }

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
