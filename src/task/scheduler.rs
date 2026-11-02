extern crate alloc;
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::arch::x86_64::idt::InterruptContext;
use crate::sync::IrqSafeSpinlock;
use crate::task::thread::{ThreadControlBlock, ThreadId, ThreadState};
use crate::kinfo;

extern "C" {
    fn cosmos_context_switch(prev_rsp: *mut u64, next_rsp: u64);
}

static NEXT_TID: AtomicU64 = AtomicU64::new(1);

pub struct Scheduler {
    ready_queue: VecDeque<Arc<IrqSafeSpinlock<ThreadControlBlock>>>,
    sleeping_queue: Vec<Arc<IrqSafeSpinlock<ThreadControlBlock>>>,
    current_thread: Option<Arc<IrqSafeSpinlock<ThreadControlBlock>>>,
    idle_thread: Option<Arc<IrqSafeSpinlock<ThreadControlBlock>>>,
}

impl Scheduler {
    pub const fn new() -> Self {
        Self {
            ready_queue: VecDeque::new(),
            sleeping_queue: Vec::new(),
            current_thread: None,
            idle_thread: None,
        }
    }

    pub fn add_thread(&mut self, thread: Arc<IrqSafeSpinlock<ThreadControlBlock>>) {
        self.ready_queue.push_back(thread);
    }
}

pub static SCHEDULER: IrqSafeSpinlock<Scheduler> = IrqSafeSpinlock::new(Scheduler::new());

fn idle_task() {
    loop {
        crate::arch::x86_64::registers::enable_interrupts_and_hlt();
    }
}

pub fn init() {
    let mut sched = SCHEDULER.lock();

    // Create thread 0 (Boot/Main thread)
    let boot_thread = Arc::new(IrqSafeSpinlock::new(ThreadControlBlock {
        tid: 0,
        pid: 0,
        name: alloc::string::String::from("kernel_main"),
        state: ThreadState::Running,
        rsp: 0,
        stack: alloc::vec![].into_boxed_slice(),
        priority: 10,
        total_ticks: 0,
    }));

    // Create Idle thread
    let idle = Arc::new(IrqSafeSpinlock::new(ThreadControlBlock::new_kthread(
        NEXT_TID.fetch_add(1, Ordering::Relaxed),
        "idle",
        idle_task,
    )));

    sched.current_thread = Some(boot_thread);
    sched.idle_thread = Some(idle);

    kinfo!("Preemptive Multitasking Scheduler initialized");
}

/// Spawn a new kernel thread.
pub fn kthread_create(name: &str, entry: fn()) -> ThreadId {
    let tid = NEXT_TID.fetch_add(1, Ordering::Relaxed);
    let thread = Arc::new(IrqSafeSpinlock::new(ThreadControlBlock::new_kthread(tid, name, entry)));
    SCHEDULER.lock().add_thread(thread);
    tid
}

/// Voluntarily yield the CPU to another ready task.
pub fn yield_now() {
    schedule();
}

/// Perform context switch to the next ready task.
pub fn schedule() {
    let mut sched = SCHEDULER.lock();

    // 1. Check sleeping queue for awakened tasks
    let current_ticks = crate::arch::x86_64::pit::get_ticks();
    let mut still_sleeping = Vec::new();
    while let Some(task) = sched.sleeping_queue.pop() {
        let is_ready = {
            let mut t = task.lock();
            if let ThreadState::Sleeping(deadline) = t.state {
                if current_ticks >= deadline {
                    t.state = ThreadState::Ready;
                    true
                } else {
                    false
                }
            } else {
                true
            }
        };

        if is_ready {
            sched.ready_queue.push_back(task);
        } else {
            still_sleeping.push(task);
        }
    }
    sched.sleeping_queue = still_sleeping;

    // 2. Select next task
    let next_task = if let Some(task) = sched.ready_queue.pop_front() {
        task
    } else {
        // If no tasks are waiting in ready queue and current task is still Running, continue running it
        if let Some(ref curr) = sched.current_thread {
            if curr.lock().state == ThreadState::Running {
                return;
            }
        }
        // Current task is sleeping or dead, switch to idle task
        if let Some(ref idle) = sched.idle_thread {
            idle.clone()
        } else {
            return;
        }
    };

    let prev_task = if let Some(ref curr) = sched.current_thread {
        curr.clone()
    } else {
        return;
    };

    // If switching to the same thread, do nothing
    if Arc::ptr_eq(&prev_task, &next_task) {
        return;
    }

    // Move current running task back to ready queue if it's still running
    {
        let mut prev = prev_task.lock();
        if prev.state == ThreadState::Running {
            prev.state = ThreadState::Ready;
            drop(prev);
            sched.ready_queue.push_back(prev_task.clone());
        }
    }

    {
        let mut next = next_task.lock();
        next.state = ThreadState::Running;
    }

    sched.current_thread = Some(next_task.clone());

    let prev_rsp_ptr = &mut prev_task.lock().rsp as *mut u64;
    let next_rsp = next_task.lock().rsp;

    drop(sched); // Drop lock before context switch!

    unsafe {
        cosmos_context_switch(prev_rsp_ptr, next_rsp);
    }
}

/// Called on every timer tick (preemption).
pub fn on_timer_tick(_ticks: u64, _ctx: &mut InterruptContext) {
    let should_schedule = {
        let sched = SCHEDULER.lock();
        if let Some(ref current) = sched.current_thread {
            current.lock().total_ticks += 1;
            true
        } else {
            false
        }
    };

    if should_schedule {
        schedule();
    }
}

/// Put current thread to sleep for the specified duration (ms).
pub fn sleep_ms(ms: u64) {
    let deadline = crate::arch::x86_64::pit::get_ticks() + ms;
    {
        let mut sched = SCHEDULER.lock();
        let current = sched.current_thread.as_ref().cloned();
        if let Some(curr) = current {
            curr.lock().state = ThreadState::Sleeping(deadline);
            sched.sleeping_queue.push(curr);
        }
    }
    schedule();
}

/// Terminate current thread.
pub fn exit_current_thread(_code: i32) -> ! {
    {
        let sched = SCHEDULER.lock();
        if let Some(ref current) = sched.current_thread {
            current.lock().state = ThreadState::Dead;
        }
    }
    schedule();
    loop {
        crate::arch::x86_64::registers::enable_interrupts_and_hlt();
    }
}

/// Get list of all tasks for shell `ps` / `tasks` command.
pub fn get_task_list() -> Vec<(ThreadId, alloc::string::String, ThreadState, u64)> {
    let sched = SCHEDULER.lock();
    let mut list = Vec::new();

    if let Some(ref curr) = sched.current_thread {
        let t = curr.lock();
        list.push((t.tid, t.name.clone(), t.state, t.total_ticks));
    }

    for task in sched.ready_queue.iter() {
        let t = task.lock();
        list.push((t.tid, t.name.clone(), t.state, t.total_ticks));
    }

    for task in sched.sleeping_queue.iter() {
        let t = task.lock();
        list.push((t.tid, t.name.clone(), t.state, t.total_ticks));
    }

    list
}
