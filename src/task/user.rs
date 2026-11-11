use crate::mm::address_space::AddressSpace;
use crate::task::elf::ElfBinary;
use crate::sync::IrqSafeSpinlock;
use crate::kinfo;

pub static CURRENT_USER_ADDRESS_SPACE: IrqSafeSpinlock<Option<AddressSpace>> = IrqSafeSpinlock::new(None);

/// Implement the sys_brk syscall (12) for dynamic user heap allocation.
pub fn sys_brk(new_brk: u64) -> u64 {
    let mut space = CURRENT_USER_ADDRESS_SPACE.lock();
    if let Some(ref mut s) = *space {
        s.brk(new_brk)
    } else {
        0
    }
}

extern "C" {
    fn enter_user_mode_asm(user_rip: u64, user_rsp: u64, user_cr3: u64, kstack: u64) -> i32;
    fn exit_user_mode_asm(exit_code: i32) -> !;
}

#[repr(C, align(16))]
struct UserKernelStack {
    stack: [u8; 16384],
}

static mut USER_KERNEL_STACK: UserKernelStack = UserKernelStack {
    stack: [0; 16384],
};

/// Get the top virtual address of the dedicated Ring 3 kernel interrupt stack.
pub fn get_user_kernel_stack_top() -> u64 {
    unsafe {
        core::ptr::addr_of!(USER_KERNEL_STACK.stack) as u64 + 16384
    }
}

/// Called by sys_exit (syscall 60) to terminate Ring 3 execution and return to kernel caller.
pub fn handle_user_exit(code: i32) -> ! {
    unsafe {
        exit_user_mode_asm(code);
    }
}

/// Called by exception handlers (SIGSEGV, SIGILL, SIGFPE) on user mode faults.
pub fn handle_user_fault(code: i32) -> ! {
    unsafe {
        exit_user_mode_asm(code);
    }
}

/// Execute a 64-bit ELF user executable in Ring 3 with full address space isolation.
pub fn execute_user_binary(data: &[u8]) -> Result<i32, &'static str> {
    // 1. Parse and validate ELF64 binary
    let elf = ElfBinary::parse(data)?;

    // 2. Allocate an isolated address space for user process
    let mut address_space = AddressSpace::new().ok_or("Failed to create user process address space")?;

    // 3. Load PT_LOAD segments and allocate user stack
    let (user_rip, user_rsp) = elf.load_into_address_space(&mut address_space)?;

    kinfo!(
        "Executing ELF64 binary in Ring 3 (Entry: 0x{:016X}, Stack: 0x{:016X}, PML4: 0x{:016X})",
        user_rip,
        user_rsp,
        address_space.pml4_phys
    );

    // 4. Set kernel stack in TSS for hardware interrupt / syscall switching
    let kstack_top = get_user_kernel_stack_top();
    crate::arch::x86_64::gdt::set_kernel_stack(kstack_top);

    let pml4_phys = address_space.pml4_phys;
    *CURRENT_USER_ADDRESS_SPACE.lock() = Some(address_space);

    // 5. Transition to Ring 3 via iretq
    let exit_code = unsafe {
        enter_user_mode_asm(user_rip, user_rsp, pml4_phys, kstack_top)
    };

    CURRENT_USER_ADDRESS_SPACE.lock().take();

    Ok(exit_code)
}
