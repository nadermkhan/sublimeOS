extern crate alloc;
use alloc::boxed::Box;
use alloc::string::String;

pub type ThreadId = u64;

/// Saved Ring 3 register state pushed onto the kernel stack by `syscall_entry` (`src/syscall/syscall_entry.s`).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SyscallFrame {
    pub r9: u64,
    pub r8: u64,
    pub r10: u64,
    pub rdx: u64,
    pub rsi: u64,
    pub rdi: u64,
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub rbx: u64,
    pub rbp: u64,
    pub rax: u64,
    pub rcx: u64,
    pub r11: u64,
    pub user_rsp: u64,
}

extern "C" {
    fn user_thread_trampoline();
    fn fork_child_trampoline();
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadState {
    Ready,
    Running,
    Blocked,
    Sleeping(u64), // Wakeup deadline tick
    Dead,
}

pub const KERNEL_STACK_SIZE: usize = 65536; // 64 KiB kernel stack per thread

pub struct ThreadControlBlock {
    pub tid: ThreadId,
    pub pid: u64,
    pub name: String,
    pub state: ThreadState,
    pub rsp: u64,
    pub stack: Box<[u8]>,
    pub kstack_top: u64,
    pub cr3: u64,
    pub is_user: bool,
    pub priority: u8,
    pub total_ticks: u64,
    pub affinity: u64,
    pub current_cpu: u32,
}

impl ThreadControlBlock {
    /// Create a new kernel thread with the given entry function.
    pub fn new_kthread(tid: ThreadId, name: &str, entry: fn()) -> Self {
        let mut stack = alloc::vec![0u8; KERNEL_STACK_SIZE].into_boxed_slice();
        let kstack_top = (stack.as_mut_ptr() as u64 + KERNEL_STACK_SIZE as u64) & !0xFu64;

        // Set up initial stack frame for sublime_context_switch
        // sublime_context_switch will pop:
        // rflags, r15, r14, r13, r12, rbp, rbx, and ret to entry!
        let mut sp = kstack_top;

        unsafe {
            let mut push = |val: u64| {
                sp -= 8;
                *(sp as *mut u64) = val;
            };
            push(thread_trampoline as *const () as usize as u64);
            push(entry as *const () as usize as u64); // rbx
            push(0); // rbp
            push(0); // r12
            push(0); // r13
            push(0); // r14
            push(0); // r15
            push(0x202); // RFLAGS (IF enabled)
        }

        Self {
            tid,
            pid: 0,
            name: String::from(name),
            state: ThreadState::Ready,
            rsp: sp,
            stack,
            kstack_top,
            cr3: 0,
            is_user: false,
            priority: 10,
            total_ticks: 0,
            affinity: !0,
            current_cpu: 0,
        }
    }

    /// Create a preemptively-scheduled Ring 3 user thread with its own 16 KiB kernel stack and CR3.
    pub fn new_user_thread(
        tid: ThreadId,
        pid: u64,
        name: &str,
        cr3: u64,
        user_rip: u64,
        user_rsp: u64,
    ) -> Self {
        let mut stack = alloc::vec![0u8; KERNEL_STACK_SIZE].into_boxed_slice();
        let kstack_top = (stack.as_mut_ptr() as u64 + KERNEL_STACK_SIZE as u64) & !0xFu64;
        let mut sp = kstack_top;

        unsafe {
            let mut push = |val: u64| {
                sp -= 8;
                *(sp as *mut u64) = val;
            };
            push(user_thread_trampoline as *const () as usize as u64);
            push(user_rip); // rbx -> user RIP
            push(user_rsp); // rbp -> user RSP
            push(0);        // r12
            push(0);        // r13
            push(0);        // r14
            push(0);        // r15
            push(0x002);    // RFLAGS (IF=0 inside trampoline until iretq sets 0x202)
        }

        Self {
            tid,
            pid,
            name: String::from(name),
            state: ThreadState::Ready,
            rsp: sp,
            stack,
            kstack_top,
            cr3,
            is_user: true,
            priority: 10,
            total_ticks: 0,
            affinity: !0,
            current_cpu: 0,
        }
    }

    /// Create a forked Ring 3 child thread resuming directly at the parent's syscall return point with RAX=0.
    pub fn new_forked_user_thread(
        tid: ThreadId,
        pid: u64,
        name: &str,
        cr3: u64,
        frame: &SyscallFrame,
    ) -> Self {
        let mut stack = alloc::vec![0u8; KERNEL_STACK_SIZE].into_boxed_slice();
        let kstack_top = (stack.as_mut_ptr() as u64 + KERNEL_STACK_SIZE as u64) & !0xFu64;
        let mut sp = kstack_top;

        unsafe {
            let mut push = |val: u64| {
                sp -= 8;
                *(sp as *mut u64) = val;
            };
            // Topmost 5-qword iretq frame popped at end of fork_child_trampoline
            push(0x1B);                // SS (USER_DS)
            push(frame.user_rsp);      // RSP
            push(frame.r11 | 0x202);   // RFLAGS (ensure IF=1)
            push(0x23);                // CS (USER_CS)
            push(frame.rcx);           // RIP

            // Saved GP registers (popped in order r9..rbp by fork_child_trampoline)
            push(frame.rbp);
            push(frame.rbx);
            push(frame.r12);
            push(frame.r13);
            push(frame.r14);
            push(frame.r15);
            push(frame.rdi);
            push(frame.rsi);
            push(frame.rdx);
            push(frame.r10);
            push(frame.r8);
            push(frame.r9);

            // Initial sublime_context_switch frame
            push(fork_child_trampoline as *const () as usize as u64);
            push(0);     // rbx
            push(0);     // rbp
            push(0);     // r12
            push(0);     // r13
            push(0);     // r14
            push(0);     // r15
            push(0x002); // RFLAGS (IF=0 until iretq)
        }

        Self {
            tid,
            pid,
            name: String::from(name),
            state: ThreadState::Ready,
            rsp: sp,
            stack,
            kstack_top,
            cr3,
            is_user: true,
            priority: 10,
            total_ticks: 0,
            affinity: !0,
            current_cpu: 0,
        }
    }

    pub fn set_affinity(&mut self, mask: u64) {
        self.affinity = if mask == 0 { 1 } else { mask };
    }

    pub fn get_affinity(&self) -> u64 {
        self.affinity
    }
}

extern "C" fn thread_trampoline() {
    // RBX contains the entry function pointer
    let entry_fn: fn();
    unsafe {
        core::arch::asm!("mov {}, rbx", out(reg) entry_fn);
    }
    entry_fn();

    // Terminate thread when entry function returns
    crate::task::scheduler::exit_current_thread(0);
}
