extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::fs::vfs::{DirEntry, FileType, InodeOperations, Stat};
use crate::sync::IrqSafeSpinlock;

static RNG_STATE: AtomicU64 = AtomicU64::new(0x853c49e6748fea9b);

fn next_rand() -> u8 {
    let mut x = RNG_STATE.load(Ordering::Relaxed);
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    RNG_STATE.store(x, Ordering::Relaxed);
    (x & 0xFF) as u8
}

pub struct DevFsDirectory {
    entries: BTreeMap<String, Arc<IrqSafeSpinlock<dyn InodeOperations>>>,
}

impl DevFsDirectory {
    pub fn new() -> Arc<IrqSafeSpinlock<Self>> {
        let mut entries: BTreeMap<String, Arc<IrqSafeSpinlock<dyn InodeOperations>>> = BTreeMap::new();
        entries.insert(String::from("null"), Arc::new(IrqSafeSpinlock::new(NullDev)));
        entries.insert(String::from("zero"), Arc::new(IrqSafeSpinlock::new(ZeroDev)));
        entries.insert(String::from("random"), Arc::new(IrqSafeSpinlock::new(RandomDev)));
        entries.insert(String::from("urandom"), Arc::new(IrqSafeSpinlock::new(RandomDev)));
        entries.insert(String::from("tty"), Arc::new(IrqSafeSpinlock::new(TtyDev)));
        entries.insert(String::from("console"), Arc::new(IrqSafeSpinlock::new(TtyDev)));
        entries.insert(String::from("ttys000"), Arc::new(IrqSafeSpinlock::new(TtyDev)));
        entries.insert(String::from("ttys001"), Arc::new(IrqSafeSpinlock::new(TtyDev)));
        entries.insert(String::from("disk0"), Arc::new(IrqSafeSpinlock::new(NullDev)));
        entries.insert(String::from("disk0s1"), Arc::new(IrqSafeSpinlock::new(NullDev)));
        entries.insert(String::from("rtc"), Arc::new(IrqSafeSpinlock::new(RtcDev)));

        Arc::new(IrqSafeSpinlock::new(Self { entries }))
    }
}

impl InodeOperations for DevFsDirectory {
    fn lookup(&self, name: &str) -> Result<Arc<IrqSafeSpinlock<dyn InodeOperations>>, &'static str> {
        self.entries.get(name).cloned().ok_or("Device not found")
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
            inode: 2,
            size: self.entries.len(),
            file_type: FileType::Directory,
            mode: 0o755,
        }
    }
}

pub struct NullDev;
impl InodeOperations for NullDev {
    fn read(&self, _offset: usize, _buf: &mut [u8]) -> Result<usize, &'static str> {
        Ok(0) // EOF
    }
    fn write(&mut self, _offset: usize, buf: &[u8]) -> Result<usize, &'static str> {
        Ok(buf.len())
    }
    fn stat(&self) -> Stat {
        Stat { inode: 100, size: 0, file_type: FileType::CharDevice, mode: 0o666 }
    }
}

pub struct ZeroDev;
impl InodeOperations for ZeroDev {
    fn read(&self, _offset: usize, buf: &mut [u8]) -> Result<usize, &'static str> {
        buf.fill(0);
        Ok(buf.len())
    }
    fn write(&mut self, _offset: usize, buf: &[u8]) -> Result<usize, &'static str> {
        Ok(buf.len())
    }
    fn stat(&self) -> Stat {
        Stat { inode: 101, size: 0, file_type: FileType::CharDevice, mode: 0o666 }
    }
}

pub struct RandomDev;
impl InodeOperations for RandomDev {
    fn read(&self, _offset: usize, buf: &mut [u8]) -> Result<usize, &'static str> {
        for byte in buf.iter_mut() {
            *byte = next_rand();
        }
        Ok(buf.len())
    }
    fn write(&mut self, _offset: usize, buf: &[u8]) -> Result<usize, &'static str> {
        Ok(buf.len())
    }
    fn stat(&self) -> Stat {
        Stat { inode: 102, size: 0, file_type: FileType::CharDevice, mode: 0o666 }
    }
}

pub struct TtyDev;
impl InodeOperations for TtyDev {
    fn read(&self, _offset: usize, buf: &mut [u8]) -> Result<usize, &'static str> {
        if buf.is_empty() {
            return Ok(0);
        }
        // Read character from keyboard buffer if available
        if let Some(ch) = crate::drivers::keyboard::read_char() {
            buf[0] = ch;
            Ok(1)
        } else {
            Ok(0)
        }
    }

    fn write(&mut self, _offset: usize, buf: &[u8]) -> Result<usize, &'static str> {
        if let Ok(s) = core::str::from_utf8(buf) {
            crate::print!("{}", s);
        }
        Ok(buf.len())
    }

    fn stat(&self) -> Stat {
        Stat { inode: 103, size: 0, file_type: FileType::CharDevice, mode: 0o666 }
    }
}

pub struct RtcDev;
impl InodeOperations for RtcDev {
    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize, &'static str> {
        let dt = crate::arch::x86_64::rtc::read_datetime();
        let s = format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC\n", dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second);
        let bytes = s.as_bytes();
        if offset >= bytes.len() {
            return Ok(0);
        }
        let avail = bytes.len() - offset;
        let n = avail.min(buf.len());
        buf[..n].copy_from_slice(&bytes[offset..offset + n]);
        Ok(n)
    }

    fn stat(&self) -> Stat {
        Stat { inode: 104, size: 24, file_type: FileType::CharDevice, mode: 0o444 }
    }
}
