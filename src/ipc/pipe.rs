extern crate alloc;
use alloc::collections::VecDeque;
use alloc::sync::Arc;
use crate::fs::vfs::{FileType, InodeOperations, Stat};
use crate::sync::IrqSafeSpinlock;

pub const PIPE_BUFFER_CAPACITY: usize = 4096;

pub struct PipeBuffer {
    buffer: VecDeque<u8>,
    readers_open: usize,
    writers_open: usize,
}

impl PipeBuffer {
    pub fn new() -> Arc<IrqSafeSpinlock<Self>> {
        Arc::new(IrqSafeSpinlock::new(Self {
            buffer: VecDeque::with_capacity(PIPE_BUFFER_CAPACITY),
            readers_open: 1,
            writers_open: 1,
        }))
    }
}

pub struct PipeReadEnd {
    pipe: Arc<IrqSafeSpinlock<PipeBuffer>>,
}

impl PipeReadEnd {
    pub fn new(pipe: Arc<IrqSafeSpinlock<PipeBuffer>>) -> Arc<IrqSafeSpinlock<Self>> {
        Arc::new(IrqSafeSpinlock::new(Self { pipe }))
    }
}

impl InodeOperations for PipeReadEnd {
    fn read(&self, _offset: usize, buf: &mut [u8]) -> Result<usize, &'static str> {
        let mut pipe = self.pipe.lock();
        if pipe.buffer.is_empty() {
            if pipe.writers_open == 0 {
                return Ok(0); // EOF
            }
            return Ok(0); // Would block or empty
        }

        let mut read_bytes = 0;
        for slot in buf.iter_mut() {
            if let Some(byte) = pipe.buffer.pop_front() {
                *slot = byte;
                read_bytes += 1;
            } else {
                break;
            }
        }
        Ok(read_bytes)
    }

    fn stat(&self) -> Stat {
        let size = self.pipe.lock().buffer.len();
        Stat { inode: 300, size, file_type: FileType::Pipe, mode: 0o600 }
    }
}

impl Drop for PipeReadEnd {
    fn drop(&mut self) {
        let mut pipe = self.pipe.lock();
        if pipe.readers_open > 0 {
            pipe.readers_open -= 1;
        }
    }
}

pub struct PipeWriteEnd {
    pipe: Arc<IrqSafeSpinlock<PipeBuffer>>,
}

impl PipeWriteEnd {
    pub fn new(pipe: Arc<IrqSafeSpinlock<PipeBuffer>>) -> Arc<IrqSafeSpinlock<Self>> {
        Arc::new(IrqSafeSpinlock::new(Self { pipe }))
    }
}

impl InodeOperations for PipeWriteEnd {
    fn write(&mut self, _offset: usize, buf: &[u8]) -> Result<usize, &'static str> {
        let mut pipe = self.pipe.lock();
        if pipe.readers_open == 0 {
            return Err("Broken pipe (SIGPIPE)");
        }

        let mut written = 0;
        for &byte in buf {
            if pipe.buffer.len() < PIPE_BUFFER_CAPACITY {
                pipe.buffer.push_back(byte);
                written += 1;
            } else {
                break;
            }
        }
        Ok(written)
    }

    fn stat(&self) -> Stat {
        let size = self.pipe.lock().buffer.len();
        Stat { inode: 301, size, file_type: FileType::Pipe, mode: 0o600 }
    }
}

impl Drop for PipeWriteEnd {
    fn drop(&mut self) {
        let mut pipe = self.pipe.lock();
        if pipe.writers_open > 0 {
            pipe.writers_open -= 1;
        }
    }
}

/// Create a new connected pipe pair (read_end, write_end).
pub fn create_pipe() -> (Arc<IrqSafeSpinlock<dyn InodeOperations>>, Arc<IrqSafeSpinlock<dyn InodeOperations>>) {
    let pipe = PipeBuffer::new();
    let reader = PipeReadEnd::new(pipe.clone());
    let writer = PipeWriteEnd::new(pipe);
    (reader as Arc<IrqSafeSpinlock<dyn InodeOperations>>, writer as Arc<IrqSafeSpinlock<dyn InodeOperations>>)
}
