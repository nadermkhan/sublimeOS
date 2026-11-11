extern crate alloc;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use crate::sync::IrqSafeSpinlock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    Regular,
    Directory,
    CharDevice,
    BlockDevice,
    Pipe,
}

#[derive(Debug, Clone)]
pub struct Stat {
    pub inode: u64,
    pub size: usize,
    pub file_type: FileType,
    pub mode: u32,
}

#[derive(Debug, Clone)]
pub struct DirEntry {
    pub name: String,
    pub inode: u64,
    pub file_type: FileType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SeekWhence {
    Start(usize),
    Current(isize),
    End(isize),
}

pub trait InodeOperations: Send + Sync {
    fn read(&self, _offset: usize, _buf: &mut [u8]) -> Result<usize, &'static str> {
        Err("Read not supported")
    }

    fn write(&mut self, _offset: usize, _buf: &[u8]) -> Result<usize, &'static str> {
        Err("Write not supported")
    }

    fn ioctl(&mut self, _cmd: u32, _arg: usize) -> Result<isize, &'static str> {
        Err("ioctl not supported")
    }

    fn lookup(&self, _name: &str) -> Result<Arc<IrqSafeSpinlock<dyn InodeOperations>>, &'static str> {
        Err("Not a directory")
    }

    fn create(&mut self, _name: &str, _file_type: FileType) -> Result<Arc<IrqSafeSpinlock<dyn InodeOperations>>, &'static str> {
        Err("Cannot create file")
    }

    fn readdir(&self) -> Result<Vec<DirEntry>, &'static str> {
        Err("Not a directory")
    }

    fn unlink(&mut self, _name: &str) -> Result<(), &'static str> {
        Err("Cannot unlink")
    }

    fn add_child_node(&mut self, _name: &str, _node: Arc<IrqSafeSpinlock<dyn InodeOperations>>) -> Result<(), &'static str> {
        Err("Cannot add child node to this Inode")
    }

    fn truncate(&mut self, _size: usize) -> Result<(), &'static str> {
        Ok(())
    }

    fn get_physical_frames(&self) -> Option<alloc::vec::Vec<u64>> {
        None
    }

    fn stat(&self) -> Stat;
}

pub struct File {
    pub inode: Arc<IrqSafeSpinlock<dyn InodeOperations>>,
    pub offset: IrqSafeSpinlock<usize>,
    pub flags: u32,
}

impl File {
    pub fn new(inode: Arc<IrqSafeSpinlock<dyn InodeOperations>>, flags: u32) -> Self {
        Self {
            inode,
            offset: IrqSafeSpinlock::new(0),
            flags,
        }
    }

    pub fn read(&self, buf: &mut [u8]) -> Result<usize, &'static str> {
        let mut off = self.offset.lock();
        let bytes_read = self.inode.lock().read(*off, buf)?;
        *off += bytes_read;
        Ok(bytes_read)
    }

    pub fn write(&self, buf: &[u8]) -> Result<usize, &'static str> {
        let mut off = self.offset.lock();
        let bytes_written = self.inode.lock().write(*off, buf)?;
        *off += bytes_written;
        Ok(bytes_written)
    }

    pub fn ioctl(&self, cmd: u32, arg: usize) -> Result<isize, &'static str> {
        self.inode.lock().ioctl(cmd, arg)
    }

    pub fn seek(&self, whence: SeekWhence) -> Result<usize, &'static str> {
        let mut off = self.offset.lock();
        let stat = self.inode.lock().stat();
        let new_offset = match whence {
            SeekWhence::Start(pos) => pos,
            SeekWhence::Current(delta) => {
                let res = (*off as isize) + delta;
                if res < 0 {
                    return Err("Negative seek offset");
                }
                res as usize
            }
            SeekWhence::End(delta) => {
                let res = (stat.size as isize) + delta;
                if res < 0 {
                    return Err("Negative seek offset");
                }
                res as usize
            }
        };
        *off = new_offset;
        Ok(new_offset)
    }

    pub fn stat(&self) -> Stat {
        self.inode.lock().stat()
    }
}

pub struct VfsManager {
    root: Option<Arc<IrqSafeSpinlock<dyn InodeOperations>>>,
}

impl VfsManager {
    pub const fn new() -> Self {
        Self { root: None }
    }

    pub fn set_root(&mut self, root: Arc<IrqSafeSpinlock<dyn InodeOperations>>) {
        self.root = Some(root);
    }

    pub fn get_root(&self) -> Option<Arc<IrqSafeSpinlock<dyn InodeOperations>>> {
        self.root.clone()
    }

    /// Resolve an absolute path (e.g., "/dev/null" or "/etc/motd").
    pub fn resolve_path(&self, path: &str) -> Result<Arc<IrqSafeSpinlock<dyn InodeOperations>>, &'static str> {
        let root = self.root.as_ref().ok_or("No root filesystem mounted")?.clone();
        if path == "/" || path.is_empty() {
            return Ok(root);
        }

        let mut curr = root;
        for component in path.split('/').filter(|s| !s.is_empty()) {
            let next = curr.lock().lookup(component)?;
            curr = next;
        }

        Ok(curr)
    }

    /// Open a file given its path.
    pub fn open(&self, path: &str, flags: u32) -> Result<Arc<File>, &'static str> {
        let inode = self.resolve_path(path)?;
        Ok(Arc::new(File::new(inode, flags)))
    }
}

pub static VFS: IrqSafeSpinlock<VfsManager> = IrqSafeSpinlock::new(VfsManager::new());
