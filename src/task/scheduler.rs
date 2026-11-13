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
static KERNEL_CR3: AtomicU64 = AtomicU64::new(0);

/// Return the physical PML4 address of the kernel's base page tables.
pub fn kernel_cr3() -> u64 {
    KERNEL_CR3.load(Ordering::Relaxed)
}

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
    pub dead_queue: Vec<Arc<IrqSafeSpinlock<ThreadControlBlock>>>,
    pub current_thread: Option<Arc<IrqSafeSpinlock<ThreadControlBlock>>>,
    pub idle_thread: Option<Arc<IrqSafeSpinlock<ThreadControlBlock>>>,
}

impl Scheduler {
    pub const fn new() -> Self {
        Self {
            per_cpu: Vec::new(),
            sleeping_queue: Vec::new(),
            ready_queue: VecDeque::new(),
            dead_queue: Vec::new(),
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
        self.per_cpu[best_cpu].ready_queue.push_back(thread);
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
    let base_cr3 = unsafe { crate::arch::x86_64::registers::read_cr3() };
    KERNEL_CR3.store(base_cr3, Ordering::Relaxed);
    crate::task::process::init();

    let mut sched = SCHEDULER.lock();

    // Create thread 0 (Boot/Main thread)
    let boot_thread = Arc::new(IrqSafeSpinlock::new(ThreadControlBlock {
        tid: 0,
        pid: 0,
        name: String::from("kernel_main"),
        state: ThreadState::Running,
        rsp: 0,
        stack: alloc::vec![].into_boxed_slice(),
        kstack_top: 0,
        cr3: base_cr3,
        is_user: false,
        priority: 10,
        total_ticks: 0,
        affinity: !0,
        current_cpu: 0,
    }));

    // Create Idle thread for CPU 0 if ensure_cores() has not already created it
    let idle = if !sched.per_cpu.is_empty() && sched.per_cpu[0].idle_thread.is_some() {
        sched.per_cpu[0].idle_thread.as_ref().unwrap().clone()
    } else {
        Arc::new(IrqSafeSpinlock::new(ThreadControlBlock::new_kthread(
            NEXT_TID.fetch_add(1, Ordering::Relaxed),
            "idle-0",
            idle_task,
        )))
    };

    if sched.per_cpu.is_empty() {
        let mut bsp_rq = PerCpuRunQueue::new(0);
        bsp_rq.current_thread = Some(boot_thread.clone());
        bsp_rq.idle_thread = Some(idle.clone());
        sched.per_cpu.push(bsp_rq);
    } else {
        let bsp_idx = (crate::arch::x86_64::apic::get_apic_id() as usize).min(sched.per_cpu.len() - 1);
        sched.per_cpu[bsp_idx].current_thread = Some(boot_thread.clone());
        if sched.per_cpu[bsp_idx].idle_thread.is_none() {
            sched.per_cpu[bsp_idx].idle_thread = Some(idle.clone());
        }
    }

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

/// Spawn a new preemptively-scheduled Ring 3 user thread.
pub fn spawn_user_thread(
    pid: u64,
    name: &str,
    cr3: u64,
    user_rip: u64,
    user_rsp: u64,
) -> ThreadId {
    let tid = NEXT_TID.fetch_add(1, Ordering::Relaxed);
    let thread = Arc::new(IrqSafeSpinlock::new(ThreadControlBlock::new_user_thread(
        tid, pid, name, cr3, user_rip, user_rsp,
    )));
    SCHEDULER.lock().add_thread(thread);
    tid
}

/// Spawn a forked Ring 3 child thread resuming from `frame` with `RAX = 0`.
pub fn spawn_forked_user_thread(
    pid: u64,
    name: &str,
    cr3: u64,
    frame: &crate::task::thread::SyscallFrame,
) -> ThreadId {
    let tid = NEXT_TID.fetch_add(1, Ordering::Relaxed);
    let thread = Arc::new(IrqSafeSpinlock::new(ThreadControlBlock::new_forked_user_thread(
        tid, pid, name, cr3, frame,
    )));
    SCHEDULER.lock().add_thread(thread);
    tid
}

/// Check if the currently executing scheduler thread is a managed Ring 3 user thread.
pub fn is_current_user_thread() -> bool {
    let sched = SCHEDULER.lock();
    let cpu_id = if sched.per_cpu.is_empty() {
        0
    } else {
        (crate::arch::x86_64::apic::get_apic_id() as usize).min(sched.per_cpu.len() - 1)
    };
    let curr_opt = if !sched.per_cpu.is_empty() {
        sched.per_cpu[cpu_id].current_thread.as_ref()
    } else {
        sched.current_thread.as_ref()
    };
    curr_opt.map_or(false, |t| t.lock().is_user)
}

/// Sync the current thread's tracked `(pid, cr3, kstack_top)` for nested synchronous user tests.
pub fn set_current_user_context(pid: u64, cr3: u64, kstack_top: u64) {
    let sched = SCHEDULER.lock();
    let cpu_id = if sched.per_cpu.is_empty() {
        0
    } else {
        (crate::arch::x86_64::apic::get_apic_id() as usize).min(sched.per_cpu.len() - 1)
    };
    let curr_opt = if !sched.per_cpu.is_empty() {
        sched.per_cpu[cpu_id].current_thread.as_ref()
    } else {
        sched.current_thread.as_ref()
    };
    if let Some(curr) = curr_opt {
        let mut t = curr.lock();
        t.pid = pid;
        t.cr3 = if cr3 != 0 { cr3 } else { KERNEL_CR3.load(Ordering::Relaxed) };
        t.kstack_top = kstack_top;
    }
}

/// Wake any sleeping scheduler threads belonging to `pid` (e.g., on child exit).
pub fn wake_pid(pid: u64) {
    if pid == 0 {
        return;
    }
    let mut sched = SCHEDULER.lock();
    let mut still_sleeping = Vec::new();
    let mut woken = Vec::new();
    while let Some(task) = sched.sleeping_queue.pop() {
        let should_wake = {
            let mut t = task.lock();
            if t.pid == pid && matches!(t.state, ThreadState::Sleeping(_)) {
                t.state = ThreadState::Ready;
                true
            } else {
                false
            }
        };
        if should_wake {
            woken.push(task);
        } else {
            still_sleeping.push(task);
        }
    }
    sched.sleeping_queue = still_sleeping;
    for task in woken {
        sched.add_thread(task);
    }
}

/// Mark all threads belonging to `pid` as Dead and purge them from ready/sleeping queues.
pub fn kill_process_threads(pid: u64) {
    if pid == 0 {
        return;
    }
    let mut sched = SCHEDULER.lock();
    let mut killed = Vec::new();
    for rq in sched.per_cpu.iter_mut() {
        if let Some(ref curr) = rq.current_thread {
            let mut t = curr.lock();
            if t.pid == pid {
                t.state = ThreadState::Dead;
            }
        }
        let mut kept = VecDeque::new();
        while let Some(task) = rq.ready_queue.pop_front() {
            let is_target = {
                let mut t = task.lock();
                if t.pid == pid {
                    t.state = ThreadState::Dead;
                    true
                } else {
                    false
                }
            };
            if is_target {
                killed.push(task);
            } else {
                kept.push_back(task);
            }
        }
        rq.ready_queue = kept;
    }
    sched.dead_queue.extend(killed);

    let mut kept_global = VecDeque::new();
    while let Some(task) = sched.ready_queue.pop_front() {
        let is_target = {
            let mut t = task.lock();
            if t.pid == pid {
                t.state = ThreadState::Dead;
                true
            } else {
                false
            }
        };
        if is_target {
            sched.dead_queue.push(task);
        } else {
            kept_global.push_back(task);
        }
    }
    sched.ready_queue = kept_global;

    let mut kept_sleep = Vec::new();
    while let Some(task) = sched.sleeping_queue.pop() {
        let is_target = {
            let mut t = task.lock();
            if t.pid == pid {
                t.state = ThreadState::Dead;
                true
            } else {
                false
            }
        };
        if is_target {
            sched.dead_queue.push(task);
        } else {
            kept_sleep.push(task);
        }
    }
    sched.sleeping_queue = kept_sleep;
}

/// Voluntarily yield the CPU to another ready task.
pub fn yield_now() {
    schedule();
}

/// Perform context switch to the next ready task with Work-Stealing load balancing.
pub fn schedule() {
    let rflags = crate::arch::x86_64::registers::read_rflags();
    let prev_if = (rflags & 0x200) != 0;
    crate::arch::x86_64::registers::disable_interrupts();

    let dead_to_drop = schedule_inner();
    drop(dead_to_drop);

    if prev_if {
        crate::arch::x86_64::registers::enable_interrupts();
    }
}

fn schedule_inner() -> Vec<Arc<IrqSafeSpinlock<ThreadControlBlock>>> {
    let mut sched = SCHEDULER.lock();
    let dead_to_drop = core::mem::take(&mut sched.dead_queue);

    let cpu_id = if sched.per_cpu.is_empty() {
        0
    } else {
        (crate::arch::x86_64::apic::get_apic_id() as usize).min(sched.per_cpu.len() - 1)
    };

    // 1. Check sleeping queue for awakened tasks
    let current_ticks = crate::arch::x86_64::pit::get_ticks();
    let mut still_sleeping = Vec::new();
    while let Some(task) = sched.sleeping_queue.pop() {
        enum WakeAction { Ready, Dead, Sleep }
        let action = {
            let mut t = task.lock();
            match t.state {
                ThreadState::Dead => WakeAction::Dead,
                ThreadState::Sleeping(deadline) => {
                    if current_ticks >= deadline {
                        t.state = ThreadState::Ready;
                        WakeAction::Ready
                    } else {
                        WakeAction::Sleep
                    }
                }
                _ => WakeAction::Ready,
            }
        };

        match action {
            WakeAction::Ready => sched.add_thread(task),
            WakeAction::Dead => sched.dead_queue.push(task),
            WakeAction::Sleep => still_sleeping.push(task),
        }
    }
    sched.sleeping_queue = still_sleeping;

    // 2. Select next task for current CPU
    let mut next_task_opt: Option<Arc<IrqSafeSpinlock<ThreadControlBlock>>> = None;

    if !sched.per_cpu.is_empty() {
        // Pop task from local ready queue
        while let Some(task) = sched.per_cpu[cpu_id].ready_queue.pop_front() {
            let (state, aff) = {
                let t = task.lock();
                (t.state, t.affinity)
            };
            if state == ThreadState::Dead {
                sched.dead_queue.push(task);
                continue;
            }
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
                    let (state, aff) = {
                        let t = task.lock();
                        (t.state, t.affinity)
                    };
                    if state == ThreadState::Dead {
                        sched.dead_queue.push(task);
                        continue;
                    }
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
        while let Some(task) = sched.ready_queue.pop_front() {
            if task.lock().state == ThreadState::Dead {
                sched.dead_queue.push(task);
                continue;
            }
            next_task_opt = Some(task);
            break;
        }
    }

    let next_task = if let Some(task) = next_task_opt {
        task
    } else {
        // Fall back to current task if still running
        if !sched.per_cpu.is_empty() {
            if let Some(ref curr) = sched.per_cpu[cpu_id].current_thread {
                if curr.lock().state == ThreadState::Running {
                    return dead_to_drop;
                }
            }
            if let Some(ref idle) = sched.per_cpu[cpu_id].idle_thread {
                idle.clone()
            } else if let Some(ref idle) = sched.idle_thread {
                idle.clone()
            } else {
                return dead_to_drop;
            }
        } else {
            if let Some(ref curr) = sched.current_thread {
                if curr.lock().state == ThreadState::Running {
                    return dead_to_drop;
                }
            }
            if let Some(ref idle) = sched.idle_thread {
                idle.clone()
            } else {
                return dead_to_drop;
            }
        }
    };

    let prev_task = if !sched.per_cpu.is_empty() {
        if let Some(ref curr) = sched.per_cpu[cpu_id].current_thread {
            curr.clone()
        } else {
            return dead_to_drop;
        }
    } else if let Some(ref curr) = sched.current_thread {
        curr.clone()
    } else {
        return dead_to_drop;
    };

    if Arc::ptr_eq(&prev_task, &next_task) {
        return dead_to_drop;
    }

    let prev_is_idle = if !sched.per_cpu.is_empty() {
        sched.per_cpu[cpu_id]
            .idle_thread
            .as_ref()
            .map_or(false, |idle| Arc::ptr_eq(&prev_task, idle))
    } else {
        sched
            .idle_thread
            .as_ref()
            .map_or(false, |idle| Arc::ptr_eq(&prev_task, idle))
    };

    // Move current running task back to ready queue if it's still running (never enqueue idle!)
    let prev_rsp_ptr = {
        let mut prev = prev_task.lock();
        if prev.state == ThreadState::Running {
            prev.state = ThreadState::Ready;
            if !prev_is_idle {
                if !sched.per_cpu.is_empty() {
                    sched.per_cpu[cpu_id].ready_queue.push_back(prev_task.clone());
                } else {
                    sched.ready_queue.push_back(prev_task.clone());
                }
            }
        } else if prev.state == ThreadState::Dead {
            sched.dead_queue.push(prev_task.clone());
        }
        &mut prev.rsp as *mut u64
    };

    let (next_rsp, next_kstack_top, next_cr3, next_pid, _next_is_user) = {
        let mut next = next_task.lock();
        next.state = ThreadState::Running;
        (next.rsp, next.kstack_top, next.cr3, next.pid, next.is_user)
    };

    if !sched.per_cpu.is_empty() {
        sched.per_cpu[cpu_id].current_thread = Some(next_task.clone());
    }
    sched.current_thread = Some(next_task.clone());

    drop(sched);
    drop(dead_to_drop);

    if next_kstack_top != 0 {
        crate::arch::x86_64::gdt::set_kernel_stack(next_kstack_top);
        crate::arch::x86_64::syscall::set_syscall_kernel_stack(next_kstack_top);
    }
    if next_pid != 0 {
        crate::task::process::set_current_pid(next_pid);
    }
    let target_cr3 = if next_cr3 != 0 {
        next_cr3
    } else {
        KERNEL_CR3.load(Ordering::Relaxed)
    };
    if target_cr3 != 0 {
        unsafe {
            let cur_cr3 = crate::arch::x86_64::registers::read_cr3();
            if cur_cr3 != target_cr3 {
                crate::arch::x86_64::registers::write_cr3(target_cr3);
            }
        }
    }

    unsafe {
        sublime_context_switch(prev_rsp_ptr, next_rsp);
    }

    Vec::new()
}

/// Called on every timer tick (preemption).
pub fn on_timer_tick(_ticks: u64, _ctx: &mut InterruptContext) {
    if crate::arch::x86_64::pit::IN_KERNEL_TESTS.load(core::sync::atomic::Ordering::Relaxed) {
        return;
    }

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
                let mut curr = current.lock();
                curr.total_ticks += 1;
                curr.total_ticks % 10 == 0 || !sched.per_cpu[cpu_id].ready_queue.is_empty()
            } else {
                !sched.per_cpu[cpu_id].ready_queue.is_empty()
            }
        } else if let Some(ref current) = sched.current_thread {
            let mut curr = current.lock();
            curr.total_ticks += 1;
            curr.total_ticks % 10 == 0 || !sched.ready_queue.is_empty()
        } else {
            !sched.ready_queue.is_empty()
        }
    };

    if should_schedule {
        schedule();
    }
}

/// Put current thread to sleep for the specified duration (ms).
pub fn sleep_ms(ms: u64) {
    if ms == 0 {
        schedule();
        return;
    }
    let deadline = crate::arch::x86_64::pit::get_ticks() + ms;
    {
        let mut sched = SCHEDULER.lock();
        let cpu_id = if sched.per_cpu.is_empty() {
            0
        } else {
            (crate::arch::x86_64::apic::get_apic_id() as usize).min(sched.per_cpu.len() - 1)
        };
        let current = if !sched.per_cpu.is_empty() {
            sched.per_cpu[cpu_id].current_thread.as_ref().cloned()
        } else {
            sched.current_thread.as_ref().cloned()
        };
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
        let cpu_id = if sched.per_cpu.is_empty() {
            0
        } else {
            (crate::arch::x86_64::apic::get_apic_id() as usize).min(sched.per_cpu.len() - 1)
        };
        let current = if !sched.per_cpu.is_empty() {
            sched.per_cpu[cpu_id].current_thread.as_ref()
        } else {
            sched.current_thread.as_ref()
        };
        if let Some(curr) = current {
            curr.lock().state = ThreadState::Dead;
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
