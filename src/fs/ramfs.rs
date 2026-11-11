extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::fs::vfs::{DirEntry, FileType, InodeOperations, Stat};
use crate::sync::IrqSafeSpinlock;

static NEXT_INODE: AtomicU64 = AtomicU64::new(1);

enum RamNodeContent {
    File(Vec<u8>),
    Directory(BTreeMap<String, Arc<IrqSafeSpinlock<dyn InodeOperations>>>),
}

pub struct RamInode {
    inode_num: u64,
    file_type: FileType,
    content: RamNodeContent,
}

impl RamInode {
    pub fn new_dir() -> Arc<IrqSafeSpinlock<Self>> {
        Arc::new(IrqSafeSpinlock::new(Self {
            inode_num: NEXT_INODE.fetch_add(1, Ordering::Relaxed),
            file_type: FileType::Directory,
            content: RamNodeContent::Directory(BTreeMap::new()),
        }))
    }

    pub fn new_file(data: &[u8]) -> Arc<IrqSafeSpinlock<Self>> {
        Arc::new(IrqSafeSpinlock::new(Self {
            inode_num: NEXT_INODE.fetch_add(1, Ordering::Relaxed),
            file_type: FileType::Regular,
            content: RamNodeContent::File(data.to_vec()),
        }))
    }

    pub fn add_child(&mut self, name: &str, node: Arc<IrqSafeSpinlock<dyn InodeOperations>>) {
        if let RamNodeContent::Directory(ref mut entries) = self.content {
            entries.insert(String::from(name), node);
        }
    }
}

impl InodeOperations for RamInode {
    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize, &'static str> {
        match self.content {
            RamNodeContent::File(ref data) => {
                if offset >= data.len() {
                    return Ok(0);
                }
                let available = data.len() - offset;
                let to_read = available.min(buf.len());
                buf[..to_read].copy_from_slice(&data[offset..offset + to_read]);
                Ok(to_read)
            }
            RamNodeContent::Directory(_) => Err("Is a directory"),
        }
    }

    fn write(&mut self, offset: usize, buf: &[u8]) -> Result<usize, &'static str> {
        match self.content {
            RamNodeContent::File(ref mut data) => {
                if offset + buf.len() > data.len() {
                    data.resize(offset + buf.len(), 0);
                }
                data[offset..offset + buf.len()].copy_from_slice(buf);
                Ok(buf.len())
            }
            RamNodeContent::Directory(_) => Err("Is a directory"),
        }
    }

    fn lookup(&self, name: &str) -> Result<Arc<IrqSafeSpinlock<dyn InodeOperations>>, &'static str> {
        match self.content {
            RamNodeContent::Directory(ref entries) => {
                let node = entries.get(name).ok_or("File not found")?;
                Ok(node.clone() as Arc<IrqSafeSpinlock<dyn InodeOperations>>)
            }
            RamNodeContent::File(_) => Err("Not a directory"),
        }
    }

    fn create(&mut self, name: &str, file_type: FileType) -> Result<Arc<IrqSafeSpinlock<dyn InodeOperations>>, &'static str> {
        match self.content {
            RamNodeContent::Directory(ref mut entries) => {
                if entries.contains_key(name) {
                    return Err("File already exists");
                }
                let new_node = match file_type {
                    FileType::Directory => RamInode::new_dir(),
                    _ => RamInode::new_file(&[]),
                };
                entries.insert(String::from(name), new_node.clone());
                Ok(new_node as Arc<IrqSafeSpinlock<dyn InodeOperations>>)
            }
            RamNodeContent::File(_) => Err("Not a directory"),
        }
    }

    fn readdir(&self) -> Result<Vec<DirEntry>, &'static str> {
        match self.content {
            RamNodeContent::Directory(ref entries) => {
                let mut list = Vec::new();
                for (name, node) in entries.iter() {
                    let stat = node.lock().stat();
                    list.push(DirEntry {
                        name: name.clone(),
                        inode: stat.inode,
                        file_type: stat.file_type,
                    });
                }
                Ok(list)
            }
            RamNodeContent::File(_) => Err("Not a directory"),
        }
    }

    fn unlink(&mut self, name: &str) -> Result<(), &'static str> {
        match self.content {
            RamNodeContent::Directory(ref mut entries) => {
                entries.remove(name).ok_or("File not found")?;
                Ok(())
            }
            RamNodeContent::File(_) => Err("Not a directory"),
        }
    }

    fn add_child_node(&mut self, name: &str, node: Arc<IrqSafeSpinlock<dyn InodeOperations>>) -> Result<(), &'static str> {
        match self.content {
            RamNodeContent::Directory(ref mut entries) => {
                entries.insert(String::from(name), node);
                Ok(())
            }
            RamNodeContent::File(_) => Err("Not a directory"),
        }
    }

    fn stat(&self) -> Stat {
        let size = match self.content {
            RamNodeContent::File(ref data) => data.len(),
            RamNodeContent::Directory(ref entries) => entries.len(),
        };
        Stat {
            inode: self.inode_num,
            size,
            file_type: self.file_type,
            mode: if self.file_type == FileType::Directory { 0o755 } else { 0o644 },
        }
    }
}
