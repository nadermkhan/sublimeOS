extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use crate::fs::vfs::{DirEntry, FileType, InodeOperations, Stat};
use crate::sync::IrqSafeSpinlock;
use crate::kinfo;

pub const O_CREAT: i32 = 0x40;
pub const O_EXCL: i32 = 0x80;

/// POSIX Named Semaphore structure.
pub struct PosixSemaphore {
    pub name: String,
    pub value: IrqSafeSpinlock<i32>,
}

impl PosixSemaphore {
    pub fn new(name: &str, initial_value: i32) -> Self {
        Self {
            name: String::from(name),
            value: IrqSafeSpinlock::new(initial_value.max(0)),
        }
    }

    /// Try to decrement (lock) semaphore without blocking.
    /// Returns 0 on success, or -11 (-EAGAIN) if value is 0.
    pub fn trywait(&self) -> i32 {
        let mut val = self.value.lock();
        if *val > 0 {
            *val -= 1;
            0
        } else {
            -11 // -EAGAIN
        }
    }

    /// Decrement (lock) semaphore. In single-core or cooperative multi-tasking,
    /// decrements if > 0 or yields until available.
    pub fn wait(&self) -> i32 {
        loop {
            {
                let mut val = self.value.lock();
                if *val > 0 {
                    *val -= 1;
                    return 0;
                }
            }
            crate::task::scheduler::yield_now();
        }
    }

    /// Increment (unlock) semaphore.
    pub fn post(&self) -> i32 {
        let mut val = self.value.lock();
        *val += 1;
        0
    }

    /// Get current value.
    pub fn getvalue(&self) -> i32 {
        *self.value.lock()
    }
}

impl InodeOperations for PosixSemaphore {
    fn sem_wait(&self) -> Result<(), i32> {
        let ret = self.wait();
        if ret == 0 { Ok(()) } else { Err(ret) }
    }

    fn sem_trywait(&self) -> Result<(), i32> {
        let ret = self.trywait();
        if ret == 0 { Ok(()) } else { Err(ret) }
    }

    fn sem_post(&self) -> Result<(), i32> {
        let ret = self.post();
        if ret == 0 { Ok(()) } else { Err(ret) }
    }

    fn sem_getvalue(&self) -> Result<i32, i32> {
        Ok(self.getvalue())
    }

    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize, &'static str> {
        let val = self.getvalue();
        let s = alloc::format!("{}\n", val);
        let bytes = s.as_bytes();
        if offset >= bytes.len() {
            return Ok(0);
        }
        let to_read = buf.len().min(bytes.len() - offset);
        buf[..to_read].copy_from_slice(&bytes[offset..offset + to_read]);
        Ok(to_read)
    }

    fn stat(&self) -> Stat {
        Stat {
            inode: 8500 + (self.name.len() as u64),
            size: 4,
            file_type: FileType::CharDevice,
            mode: 0o666,
        }
    }
}

/// Registry of active POSIX named semaphores.
pub struct SemaphoreRegistry {
    pub sems: BTreeMap<String, Arc<IrqSafeSpinlock<PosixSemaphore>>>,
}

impl SemaphoreRegistry {
    pub const fn new() -> Self {
        Self {
            sems: BTreeMap::new(),
        }
    }

    pub fn open_or_create(
        &mut self,
        name: &str,
        oflag: i32,
        _mode: u32,
        initial_value: u32,
    ) -> Result<Arc<IrqSafeSpinlock<PosixSemaphore>>, &'static str> {
        if let Some(sem) = self.sems.get(name) {
            if (oflag & O_CREAT) != 0 && (oflag & O_EXCL) != 0 {
                return Err("Semaphore already exists");
            }
            Ok(sem.clone())
        } else {
            if (oflag & O_CREAT) == 0 {
                return Err("Semaphore does not exist");
            }
            let sem = Arc::new(IrqSafeSpinlock::new(PosixSemaphore::new(name, initial_value as i32)));
            self.sems.insert(String::from(name), sem.clone());
            kinfo!("Created POSIX semaphore: /dev/sem/{} (init={})", name, initial_value);
            Ok(sem)
        }
    }

    pub fn unlink(&mut self, name: &str) -> bool {
        self.sems.remove(name).is_some()
    }

    pub fn get(&self, name: &str) -> Option<Arc<IrqSafeSpinlock<PosixSemaphore>>> {
        self.sems.get(name).cloned()
    }
}

pub static SEMAPHORE_REGISTRY: IrqSafeSpinlock<SemaphoreRegistry> =
    IrqSafeSpinlock::new(SemaphoreRegistry::new());

/// Virtual `/dev/sem` directory node.
pub struct SemDirectory;

impl InodeOperations for SemDirectory {
    fn lookup(&self, name: &str) -> Result<Arc<IrqSafeSpinlock<dyn InodeOperations>>, &'static str> {
        let reg = SEMAPHORE_REGISTRY.lock();
        if let Some(sem) = reg.get(name) {
            Ok(sem as Arc<IrqSafeSpinlock<dyn InodeOperations>>)
        } else {
            Err("Semaphore not found")
        }
    }

    fn readdir(&self) -> Result<Vec<DirEntry>, &'static str> {
        let reg = SEMAPHORE_REGISTRY.lock();
        let mut list = Vec::new();
        for (name, sem) in reg.sems.iter() {
            let stat = sem.lock().stat();
            list.push(DirEntry {
                name: name.clone(),
                inode: stat.inode,
                file_type: FileType::CharDevice,
            });
        }
        Ok(list)
    }

    fn stat(&self) -> Stat {
        Stat {
            inode: 107,
            size: SEMAPHORE_REGISTRY.lock().sems.len(),
            file_type: FileType::Directory,
            mode: 0o777,
        }
    }
}
