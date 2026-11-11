extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use crate::sync::IrqSafeSpinlock;
use crate::println;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobState {
    Running,
    Stopped,
    Terminated,
}

#[derive(Debug, Clone)]
pub struct Job {
    pub id: usize,
    pub pgid: u64,
    pub pid: u64,
    pub command: String,
    pub state: JobState,
}

pub struct JobManager {
    pub jobs: BTreeMap<usize, Job>,
    next_id: usize,
}

impl JobManager {
    pub const fn new() -> Self {
        Self {
            jobs: BTreeMap::new(),
            next_id: 1,
        }
    }

    pub fn add_job(&mut self, pid: u64, pgid: u64, command: &str, state: JobState) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        self.jobs.insert(
            id,
            Job {
                id,
                pgid,
                pid,
                command: String::from(command),
                state,
            },
        );
        id
    }

    pub fn get_job(&self, id: usize) -> Option<Job> {
        self.jobs.get(&id).cloned()
    }

    pub fn get_job_mut(&mut self, id: usize) -> Option<&mut Job> {
        self.jobs.get_mut(&id)
    }

    pub fn remove_job(&mut self, id: usize) -> Option<Job> {
        self.jobs.remove(&id)
    }

    pub fn list_jobs(&self) -> Vec<Job> {
        self.jobs.values().cloned().collect()
    }

    pub fn set_state(&mut self, id: usize, state: JobState) {
        if let Some(j) = self.jobs.get_mut(&id) {
            j.state = state;
        }
    }

    /// Update status of jobs by checking child process states via WNOHANG | WUNTRACED.
    pub fn poll_jobs(&mut self) {
        let mut status = 0i32;
        let mut to_remove = Vec::new();

        for (&id, job) in self.jobs.iter_mut() {
            let res = crate::task::process::sys_waitpid(
                job.pid as i64,
                &mut status,
                crate::task::process::WNOHANG | crate::task::process::WUNTRACED,
            );

            if res == job.pid as i32 {
                // Check if stopped or terminated
                if (status & 0xFF) == 0x7F {
                    job.state = JobState::Stopped;
                } else {
                    job.state = JobState::Terminated;
                    to_remove.push(id);
                }
            } else if res == -10 {
                // No such child process (already cleaned up)
                job.state = JobState::Terminated;
                to_remove.push(id);
            }
        }

        for id in to_remove {
            self.jobs.remove(&id);
        }
    }
}

pub static JOB_MANAGER: IrqSafeSpinlock<JobManager> = IrqSafeSpinlock::new(JobManager::new());

/// `jobs` builtin command: list all active/stopped background jobs.
pub fn cmd_jobs() {
    let mut mgr = JOB_MANAGER.lock();
    mgr.poll_jobs();
    let jobs = mgr.list_jobs();

    if jobs.is_empty() {
        return;
    }

    for (idx, job) in jobs.iter().enumerate() {
        let sign = if idx == jobs.len() - 1 { "+" } else { "-" };
        let state_str = match job.state {
            JobState::Running => "Running",
            JobState::Stopped => "Stopped",
            JobState::Terminated => "Done",
        };
        println!("[{}]{}  {:10}  {}", job.id, sign, state_str, job.command);
    }
}

/// `fg` builtin command: bring job to foreground and send SIGCONT if stopped.
pub fn cmd_fg(args: &[&str]) {
    let mut mgr = JOB_MANAGER.lock();
    mgr.poll_jobs();

    let job_id = if let Some(&arg) = args.first() {
        let cleaned = arg.trim_start_matches('%');
        cleaned.parse::<usize>().ok()
    } else {
        // Default to most recent job
        mgr.jobs.keys().last().cloned()
    };

    let id = match job_id {
        Some(id) => id,
        None => {
            println!("fg: no current job");
            return;
        }
    };

    let job = match mgr.get_job(id) {
        Some(j) => j,
        None => {
            println!("fg: job %{} not found", id);
            return;
        }
    };

    println!("{}", job.command);

    // Send SIGCONT to job's process group
    crate::task::process::sys_kill(-(job.pgid as i32), 18);
    mgr.set_state(id, JobState::Running);
    drop(mgr);

    // Wait for foreground job
    let mut status = 0i32;
    let res = crate::task::process::sys_waitpid(
        job.pid as i64,
        &mut status,
        crate::task::process::WUNTRACED,
    );

    if res == job.pid as i32 && (status & 0xFF) == 0x7F {
        let sig = (status >> 8) & 0xFF;
        println!("\n[{}]+  Stopped (signal {})  {}", id, sig, job.command);
        JOB_MANAGER.lock().set_state(id, JobState::Stopped);
    } else {
        JOB_MANAGER.lock().remove_job(id);
    }
}

/// `bg` builtin command: resume stopped job in background.
pub fn cmd_bg(args: &[&str]) {
    let mut mgr = JOB_MANAGER.lock();
    mgr.poll_jobs();

    let job_id = if let Some(&arg) = args.first() {
        let cleaned = arg.trim_start_matches('%');
        cleaned.parse::<usize>().ok()
    } else {
        mgr.jobs.keys().last().cloned()
    };

    let id = match job_id {
        Some(id) => id,
        None => {
            println!("bg: no current job");
            return;
        }
    };

    let job = match mgr.get_job(id) {
        Some(j) => j,
        None => {
            println!("bg: job %{} not found", id);
            return;
        }
    };

    // Send SIGCONT
    crate::task::process::sys_kill(-(job.pgid as i32), 18);
    mgr.set_state(id, JobState::Running);
    println!("[{}]+  {} &", id, job.command);
}
