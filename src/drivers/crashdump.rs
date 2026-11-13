//! Pre-Allocated Persistent Crash Dump & Log Buffer Subsystem
//!
//! Provides bare-metal safe, polled crash dump recording into a pre-allocated
//! `PANIC.LOG` file on the boot FAT32 USB drive or local disk.
//!
//! Key Safety Guarantees:
//! 1. Zero Filesystem Allocations at Crash Time: Starting sector LBAs are cached at boot.
//! 2. Zero Heap Allocations: Uses static BSS buffers to prevent heap corruption crashes.
//! 3. Polled Hardware I/O: Bypasses interrupts and OS schedulers.
//! 4. Trailing Ring Buffer: Embeds the last 32 KB of live kernel log history.

extern crate alloc;
use alloc::sync::Arc;
use core::sync::atomic::{AtomicBool, Ordering};
use crate::drivers::storage::BlockDevice;
use crate::sync::IrqSafeSpinlock;

const RING_BUFFER_SIZE: usize = 32768;
const MAX_DUMP_SIZE: usize = 65536;

pub struct LogRingBuffer {
    data: [u8; RING_BUFFER_SIZE],
    head: usize,
    total_written: usize,
}

impl LogRingBuffer {
    pub const fn new() -> Self {
        Self {
            data: [0; RING_BUFFER_SIZE],
            head: 0,
            total_written: 0,
        }
    }

    pub fn push_bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.data[self.head] = b;
            self.head = (self.head + 1) % RING_BUFFER_SIZE;
            self.total_written = self.total_written.saturating_add(1);
        }
    }

    /// Extract recent linear log text from oldest to newest into destination slice.
    pub fn extract_recent(&self, out: &mut [u8]) -> usize {
        let count = self.total_written.min(RING_BUFFER_SIZE).min(out.len());
        if count == 0 {
            return 0;
        }

        let start_pos = (self.head + RING_BUFFER_SIZE - count) % RING_BUFFER_SIZE;

        for i in 0..count {
            let idx = (start_pos + i) % RING_BUFFER_SIZE;
            out[i] = self.data[idx];
        }

        count
    }
}

pub static LOG_RING: IrqSafeSpinlock<LogRingBuffer> = IrqSafeSpinlock::new(LogRingBuffer::new());

/// Log line hook called from `vga::_print` to populate circular trace buffer.
pub fn append_log_slice(slice: &[u8]) {
    if let Some(mut ring) = LOG_RING.try_lock() {
        ring.push_bytes(slice);
    }
}

#[derive(Clone)]
pub struct CrashDumpTarget {
    pub device: Arc<dyn BlockDevice>,
    pub start_lba: u64,
    pub sector_count: u32,
    pub max_bytes: usize,
    pub device_name: alloc::string::String,
}

pub static CRASH_DUMP_TARGET: IrqSafeSpinlock<Option<CrashDumpTarget>> = IrqSafeSpinlock::new(None);
static DUMP_IN_PROGRESS: AtomicBool = AtomicBool::new(false);
static DUMP_BUF: IrqSafeSpinlock<[u8; MAX_DUMP_SIZE]> = IrqSafeSpinlock::new([0; MAX_DUMP_SIZE]);
static LOG_SLICE_BUF: IrqSafeSpinlock<[u8; 16384]> = IrqSafeSpinlock::new([0; 16384]);

/// Register a pre-allocated file target (e.g. `PANIC.LOG`) discovered at boot.
pub fn register_target(
    device: Arc<dyn BlockDevice>,
    start_lba: u64,
    sector_count: u32,
    max_bytes: usize,
) {
    let dev_name = alloc::string::String::from(device.name());
    let mut target = CRASH_DUMP_TARGET.lock();
    *target = Some(CrashDumpTarget {
        device,
        start_lba,
        sector_count,
        max_bytes: max_bytes.min(MAX_DUMP_SIZE),
        device_name: dev_name,
    });
}

/// Returns whether a crash dump target is currently registered and ready for polled writes.
pub fn is_target_registered() -> bool {
    CRASH_DUMP_TARGET.lock().is_some()
}

/// Safely write panic crash diagnostic report into pre-allocated sectors on the boot media.
pub fn write_panic_dump(
    reason: &dyn core::fmt::Display,
    file: &str,
    line: u32,
    col: u32,
    apic_id: u8,
    rflags: u64,
    rsp: u64,
    rbp: u64,
    cr2: u64,
    cr3: u64,
) -> Result<usize, &'static str> {
    // 1. Guard against recursive crashes inside the crash reporter
    if DUMP_IN_PROGRESS.swap(true, Ordering::SeqCst) {
        return Err("Recursive panic dump aborted");
    }

    let target = {
        let is_panicked = crate::arch::x86_64::vga::PANICKED.load(Ordering::Relaxed);
        if is_panicked {
            unsafe { CRASH_DUMP_TARGET.force_unlock(); }
        }
        CRASH_DUMP_TARGET.try_lock().and_then(|t| t.clone())
    };

    let Some(target) = target else {
        DUMP_IN_PROGRESS.store(false, Ordering::Release);
        return Err("No pre-allocated crash dump target registered");
    };

    let is_panicked = crate::arch::x86_64::vga::PANICKED.load(Ordering::Relaxed);
    if is_panicked {
        unsafe { DUMP_BUF.force_unlock(); }
    }
    let mut dump_buf = DUMP_BUF.lock();
    dump_buf.fill(0);

    struct DumpWriter<'a> {
        buf: &'a mut [u8],
        offset: usize,
    }

    impl<'a> core::fmt::Write for DumpWriter<'a> {
        fn write_str(&mut self, s: &str) -> core::fmt::Result {
            let bytes = s.as_bytes();
            let available = self.buf.len().saturating_sub(self.offset);
            let to_copy = bytes.len().min(available);
            if to_copy > 0 {
                self.buf[self.offset..self.offset + to_copy].copy_from_slice(&bytes[..to_copy]);
                self.offset += to_copy;
            }
            Ok(())
        }
    }

    let mut writer = DumpWriter {
        buf: &mut *dump_buf,
        offset: 0,
    };

    use core::fmt::Write;
    let _ = writeln!(writer, "================================================================================");
    let _ = writeln!(writer, "                    SUBLIMEOS KERNEL PANIC CRASH DUMP                           ");
    let _ = writeln!(writer, "                      KiteKernel 1.0.0 (x86_64)                                 ");
    let _ = writeln!(writer, "                 Copyright (c) 2026 Nader Mahbub Khan                           ");
    let _ = writeln!(writer, "================================================================================");
    let _ = writeln!(writer, "Target Storage Device: {} (Start LBA: 0x{:X})", target.device_name, target.start_lba);
    let _ = writeln!(writer, "Crash Location:        {}:{}:{}", file, line, col);
    let _ = writeln!(writer, "Crash Reason:          {}", reason);
    let _ = writeln!(writer, "Faulting CPU:          APIC ID {} | RFLAGS: 0x{:08X}", apic_id, rflags);
    let _ = writeln!(writer, "Registers:             RSP: 0x{:016X} | RBP: 0x{:016X}", rsp, rbp);
    let _ = writeln!(writer, "                       CR2: 0x{:016X} (Page Fault) | CR3: 0x{:016X}", cr2, cr3);
    let _ = writeln!(writer, "================================================================================");
    let _ = writeln!(writer, "RECENT KERNEL LOG RING BUFFER (Trailing trace):");
    let _ = writeln!(writer, "--------------------------------------------------------------------------------");

    // Append recent log buffer
    let remaining = target.max_bytes.saturating_sub(writer.offset).saturating_sub(256);
    if remaining > 0 {
        if is_panicked {
            unsafe { LOG_SLICE_BUF.force_unlock(); }
        }
        let mut log_slice = LOG_SLICE_BUF.lock();
        log_slice.fill(0);
        let extracted = if let Some(ring) = LOG_RING.try_lock() {
            ring.extract_recent(&mut log_slice[..remaining.min(16384)])
        } else {
            0
        };

        if extracted > 0 {
            let _ = writer.buf[writer.offset..writer.offset + extracted].copy_from_slice(&log_slice[..extracted]);
            writer.offset += extracted;
        }
    }

    let _ = writeln!(writer, "\n================================================================================");
    let _ = writeln!(writer, "                       END OF SUBLIMEOS CRASH DUMP                              ");
    let _ = writeln!(writer, "================================================================================");

    let total_bytes = writer.offset.min(target.max_bytes);
    // Sector-align to 512 bytes
    let sectors_to_write = (((total_bytes + 511) / 512) as u32).min(target.sector_count);
    let buffer_bytes = (sectors_to_write as usize) * 512;

    let result = target.device.write_sectors(
        target.start_lba,
        sectors_to_write,
        &dump_buf[..buffer_bytes],
    );

    DUMP_IN_PROGRESS.store(false, Ordering::Release);

    match result {
        Ok(()) => Ok(total_bytes),
        Err(e) => Err(e),
    }
}
