extern crate alloc;
use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::arch::x86_64::idt::InterruptContext;
use crate::sync::IrqSafeSpinlock;
use crate::task::thread::{ThreadControlBlock, ThreadId, ThreadState};
use crate::kinfo;

extern "C" {
    fn sublime_context_switch(prev_rsp: *mut u64, next_rsp: u64);
}

static NEXT_TID: AtomicU64 = AtomicU64::new(1);

/// Per-CPU Core RunQueue managing local thread scheduling and work-stealing metrics.
pub struct PerCpuRunQueue {
    pub cpu_id: u32,
    pub ready_queue: VecDeque<Arc<IrqSafeSpinlock<ThreadControlBlock>>>,
    pub current_thread: Option<Arc<IrqSafeSpinlock<ThreadControlBlock>>>,
    pub idle_thread: Option<Arc<IrqSafeSpinlock<ThreadControlBlock>>>,
    pub total_ticks: u64,
}

impl PerCpuRunQueue {
    pub fn new(cpu_id: u32) -> Self {
        Self {
            cpu_id,
            ready_queue: VecDeque::new(),
            current_thread: None,
            idle_thread: None,
            total_ticks: 0,
        }
    }

    pub fn queue_len(&self) -> usize {
        self.ready_queue.len()
    }
}

pub struct Scheduler {
    pub per_cpu: Vec<PerCpuRunQueue>,
    pub sleeping_queue: Vec<Arc<IrqSafeSpinlock<ThreadControlBlock>>>,
    pub ready_queue: VecDeque<Arc<IrqSafeSpinlock<ThreadControlBlock>>>,
    pub current_thread: Option<Arc<IrqSafeSpinlock<ThreadControlBlock>>>,
    pub idle_thread: Option<Arc<IrqSafeSpinlock<ThreadControlBlock>>>,
}

impl Scheduler {
    pub const fn new() -> Self {
        Self {
            per_cpu: Vec::new(),
            sleeping_queue: Vec::new(),
            ready_queue: VecDeque::new(),
            current_thread: None,
            idle_thread: None,
        }
    }

    /// Add a new thread, placing it onto the least-loaded CPU core allowed by its affinity.
    pub fn add_thread(&mut self, thread: Arc<IrqSafeSpinlock<ThreadControlBlock>>) {
        let affinity = thread.lock().affinity;

        if self.per_cpu.is_empty() {
            self.ready_queue.push_back(thread);
            return;
        }

        // Find the least-loaded online CPU that satisfies affinity
        let mut best_cpu = 0;
        let mut min_load = usize::MAX;

        for (idx, rq) in self.per_cpu.iter().enumerate() {
            if (affinity & (1 << idx)) != 0 {
                let load = rq.queue_len();
                if load < min_load {
                    min_load = load;
                    best_cpu = idx;
                }
            }
        }

        thread.lock().current_cpu = best_cpu as u32;
        self.per_cpu[best_cpu].ready_queue.push_back(thread.clone());

        if best_cpu == 0 {
            self.ready_queue.push_back(thread);
        }
    }

    /// Ensure per-CPU runqueues exist for all discovered cores.
    pub fn ensure_cores(&mut self, core_count: usize) {
        while self.per_cpu.len() < core_count {
            let cpu_id = self.per_cpu.len() as u32;
            let idle = Arc::new(IrqSafeSpinlock::new(ThreadControlBlock::new_kthread(
                NEXT_TID.fetch_add(1, Ordering::Relaxed),
                &format!("idle-{}", cpu_id),
                idle_task,
            )));
            let mut rq = PerCpuRunQueue::new(cpu_id);
            rq.idle_thread = Some(idle);
            self.per_cpu.push(rq);
        }
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
        name: String::from("kernel_main"),
        state: ThreadState::Running,
        rsp: 0,
        stack: alloc::vec![].into_boxed_slice(),
        priority: 10,
        total_ticks: 0,
        affinity: !0,
        current_cpu: 0,
    }));

    // Create Idle thread for CPU 0
    let idle = Arc::new(IrqSafeSpinlock::new(ThreadControlBlock::new_kthread(
        NEXT_TID.fetch_add(1, Ordering::Relaxed),
        "idle-0",
        idle_task,
    )));

    let mut bsp_rq = PerCpuRunQueue::new(0);
    bsp_rq.current_thread = Some(boot_thread.clone());
    bsp_rq.idle_thread = Some(idle.clone());
    sched.per_cpu.push(bsp_rq);

    sched.current_thread = Some(boot_thread);
    sched.idle_thread = Some(idle);

    kinfo!("Preemptive Multitasking Scheduler initialized (Multi-Core Work-Stealing)");
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

/// Perform context switch to the next ready task with Work-Stealing load balancing.
pub fn schedule() {
    let mut sched = SCHEDULER.lock();

    let cpu_id = if sched.per_cpu.is_empty() {
        0
    } else {
        (crate::arch::x86_64::apic::get_apic_id() as usize).min(sched.per_cpu.len() - 1)
    };

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
            sched.add_thread(task);
        } else {
            still_sleeping.push(task);
        }
    }
    sched.sleeping_queue = still_sleeping;

    // 2. Select next task for current CPU
    let mut next_task_opt: Option<Arc<IrqSafeSpinlock<ThreadControlBlock>>> = None;

    if !sched.per_cpu.is_empty() {
        // Pop task from local ready queue
        while let Some(task) = sched.per_cpu[cpu_id].ready_queue.pop_front() {
            let aff = task.lock().affinity;
            if (aff & (1 << cpu_id)) != 0 {
                next_task_opt = Some(task);
                break;
            } else {
                // If affinity no longer allows this CPU, re-route to an allowed core
                sched.add_thread(task);
            }
        }

        // If local ready queue is empty, perform Work-Stealing from busiest CPU
        if next_task_opt.is_none() && sched.per_cpu.len() > 1 {
            let mut busiest_cpu = None;
            let mut max_len = 0;

            for (idx, rq) in sched.per_cpu.iter().enumerate() {
                if idx != cpu_id && rq.queue_len() > max_len {
                    max_len = rq.queue_len();
                    busiest_cpu = Some(idx);
                }
            }

            if let Some(src_cpu) = busiest_cpu {
                let mut unstealable = Vec::new();
                while let Some(task) = sched.per_cpu[src_cpu].ready_queue.pop_back() {
                    let aff = task.lock().affinity;
                    if (aff & (1 << cpu_id)) != 0 {
                        task.lock().current_cpu = cpu_id as u32;
                        next_task_opt = Some(task);
                        break;
                    } else {
                        unstealable.push(task);
                    }
                }
                while let Some(t) = unstealable.pop() {
                    sched.per_cpu[src_cpu].ready_queue.push_back(t);
                }
            }
        }
    } else {
        next_task_opt = sched.ready_queue.pop_front();
    }

    let next_task = if let Some(task) = next_task_opt {
        task
    } else {
        // Fall back to current task if still running
        if !sched.per_cpu.is_empty() {
            if let Some(ref curr) = sched.per_cpu[cpu_id].current_thread {
                if curr.lock().state == ThreadState::Running {
                    return;
                }
            }
            if let Some(ref idle) = sched.per_cpu[cpu_id].idle_thread {
                idle.clone()
            } else if let Some(ref idle) = sched.idle_thread {
                idle.clone()
            } else {
                return;
            }
        } else {
            if let Some(ref curr) = sched.current_thread {
                if curr.lock().state == ThreadState::Running {
                    return;
                }
            }
            if let Some(ref idle) = sched.idle_thread {
                idle.clone()
            } else {
                return;
            }
        }
    };

    let prev_task = if !sched.per_cpu.is_empty() {
        if let Some(ref curr) = sched.per_cpu[cpu_id].current_thread {
            curr.clone()
        } else {
            return;
        }
    } else if let Some(ref curr) = sched.current_thread {
        curr.clone()
    } else {
        return;
    };

    if Arc::ptr_eq(&prev_task, &next_task) {
        return;
    }

    // Move current running task back to ready queue if it's still running
    {
        let mut prev = prev_task.lock();
        if prev.state == ThreadState::Running {
            prev.state = ThreadState::Ready;
            drop(prev);
            if !sched.per_cpu.is_empty() {
                sched.per_cpu[cpu_id].ready_queue.push_back(prev_task.clone());
            } else {
                sched.ready_queue.push_back(prev_task.clone());
            }
        }
    }

    {
        let mut next = next_task.lock();
        next.state = ThreadState::Running;
    }

    if !sched.per_cpu.is_empty() {
        sched.per_cpu[cpu_id].current_thread = Some(next_task.clone());
    }
    sched.current_thread = Some(next_task.clone());

    let prev_rsp_ptr = &mut prev_task.lock().rsp as *mut u64;
    let next_rsp = next_task.lock().rsp;

    drop(sched);

    unsafe {
        sublime_context_switch(prev_rsp_ptr, next_rsp);
    }
}

/// Called on every timer tick (preemption).
pub fn on_timer_tick(_ticks: u64, _ctx: &mut InterruptContext) {
    let should_schedule = {
        let mut sched = SCHEDULER.lock();
        let cpu_id = if sched.per_cpu.is_empty() {
            0
        } else {
            (crate::arch::x86_64::apic::get_apic_id() as usize).min(sched.per_cpu.len() - 1)
        };

        if !sched.per_cpu.is_empty() {
            sched.per_cpu[cpu_id].total_ticks += 1;
            if let Some(ref current) = sched.per_cpu[cpu_id].current_thread {
                current.lock().total_ticks += 1;
                true
            } else {
                false
            }
        } else if let Some(ref current) = sched.current_thread {
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

/// Set CPU affinity mask for current thread or specified thread ID.
pub fn set_affinity(id: u64, mask: u64) -> bool {
    let sched = SCHEDULER.lock();
    if id == 0 {
        if let Some(ref curr) = sched.current_thread {
            curr.lock().set_affinity(mask);
            return true;
        }
    }
    for rq in sched.per_cpu.iter() {
        if let Some(ref curr) = rq.current_thread {
            let mut t = curr.lock();
            if id == 0 || t.tid == id || t.pid == id {
                t.set_affinity(mask);
                return true;
            }
        }
        for task in rq.ready_queue.iter() {
            let mut t = task.lock();
            if t.tid == id || t.pid == id {
                t.set_affinity(mask);
                return true;
            }
        }
    }
    if let Some(ref curr) = sched.current_thread {
        let mut t = curr.lock();
        if id == 0 || t.tid == id || t.pid == id {
            t.set_affinity(mask);
            return true;
        }
    }
    false
}

/// Get CPU affinity mask for current thread or specified thread ID.
pub fn get_affinity(id: u64) -> u64 {
    let sched = SCHEDULER.lock();
    if id == 0 {
        if let Some(ref curr) = sched.current_thread {
            return curr.lock().get_affinity();
        }
    }
    for rq in sched.per_cpu.iter() {
        if let Some(ref curr) = rq.current_thread {
            let t = curr.lock();
            if id == 0 || t.tid == id || t.pid == id {
                return t.get_affinity();
            }
        }
        for task in rq.ready_queue.iter() {
            let t = task.lock();
            if t.tid == id || t.pid == id {
                return t.get_affinity();
            }
        }
    }
    if let Some(ref curr) = sched.current_thread {
        let t = curr.lock();
        if id == 0 || t.tid == id || t.pid == id {
            return t.get_affinity();
        }
    }
    !0
}

/// Get list of all tasks for shell `ps` / `tasks` command.
pub fn get_task_list() -> Vec<(ThreadId, String, ThreadState, u64)> {
    let sched = SCHEDULER.lock();
    let mut list = Vec::new();

    for rq in sched.per_cpu.iter() {
        if let Some(ref curr) = rq.current_thread {
            let t = curr.lock();
            list.push((t.tid, t.name.clone(), t.state, t.total_ticks));
        }
        for task in rq.ready_queue.iter() {
            let t = task.lock();
            list.push((t.tid, t.name.clone(), t.state, t.total_ticks));
        }
    }

    if sched.per_cpu.is_empty() {
        if let Some(ref curr) = sched.current_thread {
            let t = curr.lock();
            list.push((t.tid, t.name.clone(), t.state, t.total_ticks));
        }
        for task in sched.ready_queue.iter() {
            let t = task.lock();
            list.push((t.tid, t.name.clone(), t.state, t.total_ticks));
        }
    }

    for task in sched.sleeping_queue.iter() {
        let t = task.lock();
        list.push((t.tid, t.name.clone(), t.state, t.total_ticks));
    }

    list
}
