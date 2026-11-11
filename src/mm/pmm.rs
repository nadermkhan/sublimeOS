use crate::mm::{align_up, PAGE_SIZE};
use crate::sync::IrqSafeSpinlock;
use crate::kinfo;

// Support managing up to 1 GiB of physical memory (262,144 frames of 4 KiB)
pub const MAX_FRAMES: usize = 262_144;
pub const BITMAP_WORDS: usize = MAX_FRAMES / 64;

static mut FRAME_REF_COUNTS: [u16; MAX_FRAMES] = [0; MAX_FRAMES];

pub struct PhysicalMemoryManager {
    bitmap: [u64; BITMAP_WORDS],
    total_frames: usize,
    used_frames: usize,
}

impl PhysicalMemoryManager {
    pub const fn new() -> Self {
        Self {
            bitmap: [!0; BITMAP_WORDS], // Initially mark all as allocated/reserved
            total_frames: 0,
            used_frames: 0,
        }
    }

    pub fn set_frame_allocated(&mut self, frame: usize) {
        if frame < self.total_frames {
            let idx = frame / 64;
            let bit = frame % 64;
            if (self.bitmap[idx] & (1 << bit)) == 0 {
                self.bitmap[idx] |= 1 << bit;
                self.used_frames += 1;
            }
        }
    }

    pub fn set_frame_free(&mut self, frame: usize) {
        if frame < self.total_frames {
            let idx = frame / 64;
            let bit = frame % 64;
            if (self.bitmap[idx] & (1 << bit)) != 0 {
                self.bitmap[idx] &= !(1 << bit);
                if self.used_frames > 0 {
                    self.used_frames -= 1;
                }
            }
        }
    }

    pub fn is_frame_allocated(&self, frame: usize) -> bool {
        if frame >= self.total_frames {
            true
        } else {
            let idx = frame / 64;
            let bit = frame % 64;
            (self.bitmap[idx] & (1 << bit)) != 0
        }
    }

    /// Allocate a single 4KiB physical frame.
    pub fn alloc_frame(&mut self) -> Option<u64> {
        for idx in 0..BITMAP_WORDS {
            if self.bitmap[idx] != !0 {
                // There is at least one free frame in this 64-bit word
                let word = self.bitmap[idx];
                let bit = (!word).trailing_zeros() as usize;
                let frame = idx * 64 + bit;
                if frame < self.total_frames {
                    self.bitmap[idx] |= 1 << bit;
                    self.used_frames += 1;
                    unsafe {
                        FRAME_REF_COUNTS[frame] = 1;
                    }
                    return Some((frame * PAGE_SIZE) as u64);
                }
            }
        }
        None
    }

    /// Allocate multiple contiguous 4KiB physical frames.
    pub fn alloc_contiguous_frames(&mut self, count: usize) -> Option<u64> {
        let mut consecutive = 0;
        let mut start_frame = 0;

        for frame in 0..self.total_frames {
            if !self.is_frame_allocated(frame) {
                if consecutive == 0 {
                    start_frame = frame;
                }
                consecutive += 1;
                if consecutive == count {
                    for f in start_frame..(start_frame + count) {
                        self.set_frame_allocated(f);
                        unsafe {
                            FRAME_REF_COUNTS[f] = 1;
                        }
                    }
                    return Some((start_frame * PAGE_SIZE) as u64);
                }
            } else {
                consecutive = 0;
            }
        }
        None
    }

    /// Free an allocated physical frame.
    pub fn free_frame(&mut self, phys_addr: u64) {
        let frame = (phys_addr as usize) / PAGE_SIZE;
        if frame < MAX_FRAMES {
            unsafe {
                FRAME_REF_COUNTS[frame] = 0;
            }
        }
        self.set_frame_free(frame);
    }

    /// Increment reference count for a physical frame (e.g. during fork / CoW).
    pub fn inc_frame_ref(&mut self, phys_addr: u64) {
        let frame = (phys_addr as usize) / PAGE_SIZE;
        if frame < MAX_FRAMES {
            unsafe {
                FRAME_REF_COUNTS[frame] = FRAME_REF_COUNTS[frame].saturating_add(1);
            }
        }
    }

    /// Decrement reference count for a physical frame. Returns the remaining count.
    pub fn dec_frame_ref(&mut self, phys_addr: u64) -> u16 {
        let frame = (phys_addr as usize) / PAGE_SIZE;
        if frame < MAX_FRAMES {
            unsafe {
                if FRAME_REF_COUNTS[frame] > 0 {
                    FRAME_REF_COUNTS[frame] -= 1;
                }
                FRAME_REF_COUNTS[frame]
            }
        } else {
            0
        }
    }

    /// Get current reference count for a physical frame.
    pub fn get_frame_ref(&self, phys_addr: u64) -> u16 {
        let frame = (phys_addr as usize) / PAGE_SIZE;
        if frame < MAX_FRAMES {
            unsafe { FRAME_REF_COUNTS[frame] }
        } else {
            0
        }
    }

    pub fn stats(&self) -> (usize, usize, usize) {
        let free = if self.total_frames >= self.used_frames {
            self.total_frames - self.used_frames
        } else {
            0
        };
        (self.total_frames, self.used_frames, free)
    }

    pub fn get_total_frames(&self) -> usize {
        self.total_frames
    }

    pub fn get_used_frames(&self) -> usize {
        self.used_frames
    }
}

pub static PMM: IrqSafeSpinlock<PhysicalMemoryManager> = IrqSafeSpinlock::new(PhysicalMemoryManager::new());

extern "C" {
    static __kernel_phys_end: u8;
}

/// Initialize the Physical Memory Manager.
pub fn init(_mb_info_addr: u64) {
    let mut pmm = PMM.lock();

    // Default assume at least 128 MiB RAM if multiboot info is not fully parsed
    let default_ram_size: usize = 128 * 1024 * 1024;
    pmm.total_frames = (default_ram_size / PAGE_SIZE).min(MAX_FRAMES);
    pmm.used_frames = pmm.total_frames;

    // By default, mark memory above 1MB up to total_frames as free
    for frame in (1024 * 1024 / PAGE_SIZE)..pmm.total_frames {
        pmm.set_frame_free(frame);
    }

    // Reserve low 1MB (BIOS, IVT, BDA, VGA buffer)
    for frame in 0..(1024 * 1024 / PAGE_SIZE) {
        pmm.set_frame_allocated(frame);
    }

    // Reserve kernel binary physical footprint (from 1MB up to __kernel_phys_end aligned to 4KB)
    let kernel_end_phys = unsafe { &__kernel_phys_end as *const u8 as usize };
    let kernel_end_aligned = align_up(kernel_end_phys as u64, PAGE_SIZE as u64) as usize;
    let kernel_frames_end = kernel_end_aligned / PAGE_SIZE;

    for frame in (1024 * 1024 / PAGE_SIZE)..=kernel_frames_end {
        pmm.set_frame_allocated(frame);
    }

    // Reserve early page tables space (up to 8MB)
    let early_reserved_end = (8 * 1024 * 1024) / PAGE_SIZE;
    for frame in kernel_frames_end..early_reserved_end {
        pmm.set_frame_allocated(frame);
    }

    let (total, used, free) = pmm.stats();
    kinfo!(
        "PMM Initialized: Total: {} MB ({} frames), Used: {} MB, Free: {} MB",
        (total * PAGE_SIZE) / (1024 * 1024),
        total,
        (used * PAGE_SIZE) / (1024 * 1024),
        (free * PAGE_SIZE) / (1024 * 1024)
    );
}
