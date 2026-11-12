extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use crate::fs::vfs::{DirEntry, FileType, InodeOperations, Stat};
use crate::sync::IrqSafeSpinlock;

pub struct ProcFsDirectory {
    entries: BTreeMap<String, Arc<IrqSafeSpinlock<dyn InodeOperations>>>,
}

impl ProcFsDirectory {
    pub fn new() -> Arc<IrqSafeSpinlock<Self>> {
        let mut entries: BTreeMap<String, Arc<IrqSafeSpinlock<dyn InodeOperations>>> = BTreeMap::new();
        entries.insert(String::from("version"), Arc::new(IrqSafeSpinlock::new(ProcVersion)));
        entries.insert(String::from("uptime"), Arc::new(IrqSafeSpinlock::new(ProcUptime)));
        entries.insert(String::from("meminfo"), Arc::new(IrqSafeSpinlock::new(ProcMeminfo)));
        entries.insert(String::from("cpuinfo"), Arc::new(IrqSafeSpinlock::new(ProcCpuinfo)));
        entries.insert(String::from("tasks"), Arc::new(IrqSafeSpinlock::new(ProcTasks)));
        entries.insert(String::from("mounts"), Arc::new(IrqSafeSpinlock::new(ProcMounts)));

        Arc::new(IrqSafeSpinlock::new(Self { entries }))
    }
}

impl InodeOperations for ProcFsDirectory {
    fn lookup(&self, name: &str) -> Result<Arc<IrqSafeSpinlock<dyn InodeOperations>>, &'static str> {
        self.entries.get(name).cloned().ok_or("File not found in /proc")
    }

    fn readdir(&self) -> Result<Vec<DirEntry>, &'static str> {
        let mut list = Vec::new();
        for (name, node) in self.entries.iter() {
            let stat = node.lock().stat();
            list.push(DirEntry {
                name: name.clone(),
                inode: stat.inode,
                file_type: stat.file_type,
            });
        }
        Ok(list)
    }

    fn stat(&self) -> Stat {
        Stat {
            inode: 3,
            size: self.entries.len(),
            file_type: FileType::Directory,
            mode: 0o555,
        }
    }
}

fn read_dynamic_string(s: &str, offset: usize, buf: &mut [u8]) -> Result<usize, &'static str> {
    let bytes = s.as_bytes();
    if offset >= bytes.len() {
        return Ok(0);
    }
    let avail = bytes.len() - offset;
    let n = avail.min(buf.len());
    buf[..n].copy_from_slice(&bytes[offset..offset + n]);
    Ok(n)
}

pub struct ProcVersion;
impl InodeOperations for ProcVersion {
    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize, &'static str> {
        let s = "KiteKernel Version 1.0.0 (SublimeOS): kitekernel-release/RELEASE_X86_64; author:Nader Mahbub Khan; copyright:2026 x86_64\n";
        read_dynamic_string(s, offset, buf)
    }
    fn stat(&self) -> Stat {
        Stat { inode: 200, size: 122, file_type: FileType::Regular, mode: 0o444 }
    }
}

pub struct ProcUptime;
impl InodeOperations for ProcUptime {
    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize, &'static str> {
        let ticks = crate::arch::x86_64::pit::get_ticks();
        let secs = ticks / 1000;
        let ms = ticks % 1000;
        let s = format!("{}.{:03} {}\n", secs, ms, ticks);
        read_dynamic_string(&s, offset, buf)
    }
    fn stat(&self) -> Stat {
        Stat { inode: 201, size: 32, file_type: FileType::Regular, mode: 0o444 }
    }
}

pub struct ProcMeminfo;
impl InodeOperations for ProcMeminfo {
    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize, &'static str> {
        let (total_f, used_f, free_f) = crate::mm::pmm::PMM.lock().stats();
        let (heap_tot, heap_alloc, heap_free) = crate::mm::heap::HEAP_ALLOCATOR.stats();

        let s = format!(
            "MemTotal:       {:8} kB\nMemFree:        {:8} kB\nMemUsed:        {:8} kB\nHeapTotal:      {:8} kB\nHeapUsed:       {:8} kB\nHeapFree:       {:8} kB\nPageSize:              4 kB\n",
            (total_f * 4096) / 1024,
            (free_f * 4096) / 1024,
            (used_f * 4096) / 1024,
            heap_tot / 1024,
            heap_alloc / 1024,
            heap_free / 1024
        );
        read_dynamic_string(&s, offset, buf)
    }
    fn stat(&self) -> Stat {
        Stat { inode: 202, size: 256, file_type: FileType::Regular, mode: 0o444 }
    }
}

pub struct ProcCpuinfo;
impl InodeOperations for ProcCpuinfo {
    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize, &'static str> {
        let s = "processor       : 0\nvendor_id       : GenuineIntel\nmodel name      : x86_64 Monolithic Core\ncpu MHz         : 2400.000\nflags           : fpu vme de pse tsc msr pae mce cx8 apic sep mtrr pge mca cmov pat pse36 clflush mmx fxsr sse sse2 syscall nx lm\n";
        read_dynamic_string(s, offset, buf)
    }
    fn stat(&self) -> Stat {
        Stat { inode: 203, size: 220, file_type: FileType::Regular, mode: 0o444 }
    }
}

pub struct ProcTasks;
impl InodeOperations for ProcTasks {
    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize, &'static str> {
        let tasks = crate::task::scheduler::get_task_list();
        let mut s = format!("{:<5} {:<15} {:<10} {:<10}\n", "TID", "NAME", "STATE", "TICKS");
        for (tid, name, state, ticks) in tasks {
            let state_str = match state {
                crate::task::thread::ThreadState::Running => "Running",
                crate::task::thread::ThreadState::Ready => "Ready",
                crate::task::thread::ThreadState::Sleeping(_) => "Sleeping",
                crate::task::thread::ThreadState::Blocked => "Blocked",
                crate::task::thread::ThreadState::Dead => "Dead",
            };
            s.push_str(&format!("{:<5} {:<15} {:<10} {:<10}\n", tid, name, state_str, ticks));
        }
        read_dynamic_string(&s, offset, buf)
    }
    fn stat(&self) -> Stat {
        Stat { inode: 204, size: 512, file_type: FileType::Regular, mode: 0o444 }
    }
}

pub struct ProcMounts;
impl InodeOperations for ProcMounts {
    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize, &'static str> {
        let s = "rootfs / ramfs rw 0 0\ndevfs /dev devfs rw 0 0\nprocfs /proc procfs rw 0 0\n";
        read_dynamic_string(s, offset, buf)
    }
    fn stat(&self) -> Stat {
        Stat { inode: 205, size: 64, file_type: FileType::Regular, mode: 0o444 }
    }
}
