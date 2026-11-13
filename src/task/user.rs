use crate::mm::address_space::AddressSpace;
use crate::task::elf::ElfBinary;
use crate::sync::IrqSafeSpinlock;
use crate::kinfo;

pub static CURRENT_USER_ADDRESS_SPACE: IrqSafeSpinlock<Option<AddressSpace>> = IrqSafeSpinlock::new(None);

/// Implement the sys_brk syscall (12) for dynamic user heap allocation.
pub fn sys_brk(new_brk: u64) -> u64 {
    let cur_cr3 = unsafe { crate::arch::x86_64::registers::read_cr3() };
    let cur_pid = crate::task::process::get_current_pid();
    if let Some(proc) = crate::task::process::get_process(cur_pid) {
        let mut p = proc.lock();
        if let Some(ref mut s) = p.address_space {
            if s.pml4_phys == cur_cr3 {
                return s.brk(new_brk);
            }
        }
    }

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

/// Called by sys_exit (syscall 60) to terminate Ring 3 execution and return to kernel caller or scheduler.
pub fn handle_user_exit(code: i32) -> ! {
    let pid = crate::task::process::get_current_pid();
    if crate::task::scheduler::is_current_user_thread() {
        let kcr3 = crate::task::scheduler::kernel_cr3();
        if kcr3 != 0 {
            unsafe {
                if crate::arch::x86_64::registers::read_cr3() != kcr3 {
                    crate::arch::x86_64::registers::write_cr3(kcr3);
                }
            }
        }
        crate::task::process::exit_process(pid, code);
        crate::task::scheduler::exit_current_thread(code);
    }
    crate::task::process::exit_process(pid, code);
    unsafe {
        exit_user_mode_asm(code);
    }
}

/// Called by exception handlers (SIGSEGV, SIGILL, SIGFPE) on user mode faults.
pub fn handle_user_fault(code: i32) -> ! {
    let pid = crate::task::process::get_current_pid();
    if crate::task::scheduler::is_current_user_thread() {
        let kcr3 = crate::task::scheduler::kernel_cr3();
        if kcr3 != 0 {
            unsafe {
                if crate::arch::x86_64::registers::read_cr3() != kcr3 {
                    crate::arch::x86_64::registers::write_cr3(kcr3);
                }
            }
        }
        crate::task::process::exit_process(pid, code);
        crate::task::scheduler::exit_current_thread(code);
    }
    unsafe {
        exit_user_mode_asm(code);
    }
}

/// Execute a 64-bit ELF user executable synchronously in Ring 3 (used by kernel self-tests).
pub fn execute_user_binary(data: &[u8]) -> Result<i32, &'static str> {
    execute_user_binary_with_args(data, &[])
}

/// Execute a 64-bit ELF user executable synchronously in Ring 3 with command-line arguments.
pub fn execute_user_binary_with_args(data: &[u8], args: &[&str]) -> Result<i32, &'static str> {
    // 1. Parse and validate ELF64 binary
    let elf = ElfBinary::parse(data)?;

    // 2. Allocate an isolated address space for user process
    let mut address_space = AddressSpace::new().ok_or("Failed to create user process address space")?;

    // 3. Load PT_LOAD segments and allocate user stack with command-line arguments
    let (user_rip, user_rsp) = elf.load_into_address_space_with_args(&mut address_space, args)?;

    // 3.5. If dynamically linked, resolve shared libraries and perform runtime relocations
    if crate::user::linker::DynamicLinker::is_dynamic(&elf) {
        let mut linker = crate::user::linker::DynamicLinker::new();
        linker.link_binary(&elf, &mut address_space)?;
    }

    let pml4_phys = address_space.pml4_phys;

    // 4. Save parent state and establish new child process state
    let old_cr3 = unsafe { crate::arch::x86_64::registers::read_cr3() };
    let old_space = CURRENT_USER_ADDRESS_SPACE.lock().take();
    *CURRENT_USER_ADDRESS_SPACE.lock() = Some(address_space);

    let old_pid = crate::task::process::get_current_pid();
    let proc_name = args.first().copied().unwrap_or("user_bin");
    let pid = crate::task::process::alloc_process(proc_name);
    crate::task::process::set_current_pid(pid);

    let mut temp_kstack = alloc::vec![0u8; 65536].into_boxed_slice();
    let kstack_top = (temp_kstack.as_mut_ptr() as u64 + 65536) & !0xFu64;
    let old_kstack = crate::arch::x86_64::gdt::get_kernel_stack();
    crate::arch::x86_64::gdt::set_kernel_stack(kstack_top);
    crate::arch::x86_64::syscall::set_syscall_kernel_stack(kstack_top);
    crate::task::scheduler::set_current_user_context(pid, pml4_phys, kstack_top);

    // 5. Transition to Ring 3 via iretq
    let exit_code = unsafe {
        enter_user_mode_asm(user_rip, user_rsp, pml4_phys, kstack_top)
    };

    // 6. Restore parent state
    unsafe {
        if crate::arch::x86_64::registers::read_cr3() != old_cr3 {
            crate::arch::x86_64::registers::write_cr3(old_cr3);
        }
    }
    crate::task::process::set_current_pid(old_pid);
    *CURRENT_USER_ADDRESS_SPACE.lock() = old_space;
    crate::arch::x86_64::gdt::set_kernel_stack(old_kstack);
    crate::arch::x86_64::syscall::set_syscall_kernel_stack(old_kstack);
    crate::task::scheduler::set_current_user_context(old_pid, old_cr3, old_kstack);

    Ok(exit_code)
}

/// Spawn an ELF64 Ring 3 user process onto the preemptive scheduler.
/// When `wait` is true, blocks the caller until child termination and returns its exit code.
/// When `wait` is false, returns the spawned child PID immediately.
pub fn spawn_user_process(data: &[u8], args: &[&str], wait: bool) -> Result<i32, &'static str> {
    let elf = ElfBinary::parse(data)?;
    let mut address_space = AddressSpace::new().ok_or("Failed to create user process address space")?;
    let (user_rip, user_rsp) = elf.load_into_address_space_with_args(&mut address_space, args)?;

    if crate::user::linker::DynamicLinker::is_dynamic(&elf) {
        let mut linker = crate::user::linker::DynamicLinker::new();
        linker.link_binary(&elf, &mut address_space)?;
    }

    let pml4_phys = address_space.pml4_phys;
    let ppid = crate::task::process::get_current_pid();
    let proc_name = args.first().copied().unwrap_or("user_bin");
    let pid = crate::task::process::alloc_process_with_space(proc_name, ppid, address_space);
    crate::task::scheduler::spawn_user_thread(pid, proc_name, pml4_phys, user_rip, user_rsp);

    if wait {
        Ok(crate::task::process::wait_for_child_exit(pid))
    } else {
        Ok(pid as i32)
    }
}

/// Spawn `/bin/init` strictly as PID 1 onto the preemptive scheduler.
pub fn spawn_init_process(data: &[u8]) -> Result<u64, &'static str> {
    let elf = ElfBinary::parse(data)?;
    let mut address_space = AddressSpace::new().ok_or("Failed to create init process address space")?;
    let (user_rip, user_rsp) = elf.load_into_address_space(&mut address_space)?;

    if crate::user::linker::DynamicLinker::is_dynamic(&elf) {
        let mut linker = crate::user::linker::DynamicLinker::new();
        linker.link_binary(&elf, &mut address_space)?;
    }

    let pml4_phys = address_space.pml4_phys;
    let init_pid = crate::task::process::alloc_init_process_with_space(address_space);

    kinfo!(
        "Executing /bin/init in Ring 3 as PID 1 (Entry: 0x{:016X}, Stack: 0x{:016X}, PML4: 0x{:016X})",
        user_rip,
        user_rsp,
        pml4_phys
    );

    crate::task::scheduler::spawn_user_thread(init_pid, "init", pml4_phys, user_rip, user_rsp);
    Ok(init_pid)
}
