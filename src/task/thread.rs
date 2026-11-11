extern crate alloc;
use alloc::boxed::Box;
use alloc::string::String;

pub type ThreadId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadState {
    Ready,
    Running,
    Blocked,
    Sleeping(u64), // Wakeup deadline tick
    Dead,
}

pub const KERNEL_STACK_SIZE: usize = 16384; // 16 KiB kernel stack per thread

pub struct ThreadControlBlock {
    pub tid: ThreadId,
    pub pid: u64,
    pub name: String,
    pub state: ThreadState,
    pub rsp: u64,
    pub stack: Box<[u8]>,
    pub priority: u8,
    pub total_ticks: u64,
    pub affinity: u64,
    pub current_cpu: u32,
}

impl ThreadControlBlock {
    /// Create a new kernel thread with the given entry function.
    pub fn new_kthread(tid: ThreadId, name: &str, entry: fn()) -> Self {
        let mut stack = alloc::vec![0u8; KERNEL_STACK_SIZE].into_boxed_slice();
        let stack_top = stack.as_mut_ptr() as u64 + KERNEL_STACK_SIZE as u64;

        // Set up initial stack frame for cosmos_context_switch
        // cosmos_context_switch will pop:
        // rbx, rbp, r12, r13, r14, r15, rflags, and ret to entry!
        let mut sp = stack_top;

        unsafe {
            // 1. Return address for `ret` (popped last)
            sp -= 8;
            *(sp as *mut u64) = thread_trampoline as *const () as usize as u64;

            // 2. rbx (popped right before ret)
            sp -= 8;
            *(sp as *mut u64) = entry as *const () as usize as u64;

            // 3. rbp
            sp -= 8;
            *(sp as *mut u64) = 0;

            // 4. r12
            sp -= 8;
            *(sp as *mut u64) = 0;

            // 5. r13
            sp -= 8;
            *(sp as *mut u64) = 0;

            // 6. r14
            sp -= 8;
            *(sp as *mut u64) = 0;

            // 7. r15
            sp -= 8;
            *(sp as *mut u64) = 0;

            // 8. RFLAGS (popped first by popfq!)
            sp -= 8;
            *(sp as *mut u64) = 0x202; // IF enabled
        }

        Self {
            tid,
            pid: 0,
            name: String::from(name),
            state: ThreadState::Ready,
            rsp: sp,
            stack,
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
