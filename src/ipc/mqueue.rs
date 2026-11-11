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
pub const O_NONBLOCK: i32 = 0x800;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct MqAttr {
    pub mq_flags: i64,
    pub mq_maxmsg: i64,
    pub mq_msgsize: i64,
    pub mq_curmsgs: i64,
}

impl Default for MqAttr {
    fn default() -> Self {
        Self {
            mq_flags: 0,
            mq_maxmsg: 10,
            mq_msgsize: 8192,
            mq_curmsgs: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MqMessage {
    pub priority: u32,
    pub data: Vec<u8>,
}

pub struct PosixMessageQueue {
    pub name: String,
    pub attr: IrqSafeSpinlock<MqAttr>,
    pub messages: IrqSafeSpinlock<Vec<MqMessage>>,
}

impl PosixMessageQueue {
    pub fn new(name: &str, attr: Option<MqAttr>) -> Self {
        let actual_attr = attr.unwrap_or_default();
        Self {
            name: String::from(name),
            attr: IrqSafeSpinlock::new(actual_attr),
            messages: IrqSafeSpinlock::new(Vec::new()),
        }
    }

    /// Send a message into the queue with given priority.
    pub fn send(&self, data: &[u8], prio: u32, nonblock: bool) -> Result<(), i32> {
        let max_size = self.attr.lock().mq_msgsize as usize;
        if data.len() > max_size {
            return Err(-90); // -EMSGSIZE
        }

        loop {
            {
                let mut msgs = self.messages.lock();
                let max_msgs = self.attr.lock().mq_maxmsg as usize;

                if msgs.len() < max_msgs {
                    let msg = MqMessage {
                        priority: prio,
                        data: data.to_vec(),
                    };

                    // Insert ordered by priority descending (higher priority at front, FIFO for same priority)
                    let insert_pos = msgs
                        .iter()
                        .position(|m| m.priority < prio)
                        .unwrap_or(msgs.len());
                    msgs.insert(insert_pos, msg);

                    self.attr.lock().mq_curmsgs = msgs.len() as i64;
                    return Ok(());
                }
            }

            if nonblock {
                return Err(-11); // -EAGAIN
            }
            crate::task::scheduler::yield_now();
        }
    }

    /// Receive the highest priority message from the queue.
    /// Returns (number of bytes copied, priority).
    pub fn receive(&self, buf: &mut [u8], max_len: usize, nonblock: bool) -> Result<(usize, u32), i32> {
        loop {
            {
                let mut msgs = self.messages.lock();
                if !msgs.is_empty() {
                    let msg = msgs.remove(0);
                    self.attr.lock().mq_curmsgs = msgs.len() as i64;

                    let to_copy = buf.len().min(msg.data.len()).min(max_len);
                    buf[..to_copy].copy_from_slice(&msg.data[..to_copy]);
                    return Ok((to_copy, msg.priority));
                }
            }

            if nonblock {
                return Err(-11); // -EAGAIN
            }
            crate::task::scheduler::yield_now();
        }
    }

    pub fn getattr(&self) -> MqAttr {
        *self.attr.lock()
    }
}

impl InodeOperations for PosixMessageQueue {
    fn mq_send(&self, data: &[u8], prio: u32, nonblock: bool) -> Result<(), i32> {
        self.send(data, prio, nonblock)
    }

    fn mq_receive(&self, buf: &mut [u8], max_len: usize, nonblock: bool) -> Result<(usize, u32), i32> {
        self.receive(buf, max_len, nonblock)
    }

    fn mq_getattr(&self) -> Result<MqAttr, i32> {
        Ok(self.getattr())
    }

    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize, &'static str> {
        let attr = self.getattr();
        let s = alloc::format!(
            "Q:{} cur:{} max:{} sz:{}\n",
            self.name,
            attr.mq_curmsgs,
            attr.mq_maxmsg,
            attr.mq_msgsize
        );
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
            inode: 8600 + (self.name.len() as u64),
            size: self.attr.lock().mq_curmsgs as usize,
            file_type: FileType::CharDevice,
            mode: 0o666,
        }
    }
}

/// Registry of active POSIX message queues.
pub struct MqueueRegistry {
    pub queues: BTreeMap<String, Arc<IrqSafeSpinlock<PosixMessageQueue>>>,
}

impl MqueueRegistry {
    pub const fn new() -> Self {
        Self {
            queues: BTreeMap::new(),
        }
    }

    pub fn open_or_create(
        &mut self,
        name: &str,
        oflag: i32,
        attr: Option<MqAttr>,
    ) -> Result<Arc<IrqSafeSpinlock<PosixMessageQueue>>, &'static str> {
        if let Some(mq) = self.queues.get(name) {
            if (oflag & O_CREAT) != 0 && (oflag & O_EXCL) != 0 {
                return Err("Message queue already exists");
            }
            Ok(mq.clone())
        } else {
            if (oflag & O_CREAT) == 0 {
                return Err("Message queue does not exist");
            }
            let mq = Arc::new(IrqSafeSpinlock::new(PosixMessageQueue::new(name, attr)));
            self.queues.insert(String::from(name), mq.clone());
            kinfo!("Created POSIX message queue: /dev/mqueue/{}", name);
            Ok(mq)
        }
    }

    pub fn unlink(&mut self, name: &str) -> bool {
        self.queues.remove(name).is_some()
    }

    pub fn get(&self, name: &str) -> Option<Arc<IrqSafeSpinlock<PosixMessageQueue>>> {
        self.queues.get(name).cloned()
    }
}

pub static MQUEUE_REGISTRY: IrqSafeSpinlock<MqueueRegistry> =
    IrqSafeSpinlock::new(MqueueRegistry::new());

/// Virtual `/dev/mqueue` directory node.
pub struct MqueueDirectory;

impl InodeOperations for MqueueDirectory {
    fn lookup(&self, name: &str) -> Result<Arc<IrqSafeSpinlock<dyn InodeOperations>>, &'static str> {
        let reg = MQUEUE_REGISTRY.lock();
        if let Some(mq) = reg.get(name) {
            Ok(mq as Arc<IrqSafeSpinlock<dyn InodeOperations>>)
        } else {
            Err("Message queue not found")
        }
    }

    fn readdir(&self) -> Result<Vec<DirEntry>, &'static str> {
        let reg = MQUEUE_REGISTRY.lock();
        let mut list = Vec::new();
        for (name, mq) in reg.queues.iter() {
            let stat = mq.lock().stat();
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
            inode: 108,
            size: MQUEUE_REGISTRY.lock().queues.len(),
            file_type: FileType::Directory,
            mode: 0o777,
        }
    }
}
