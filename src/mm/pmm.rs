use crate::mm::{align_down, align_up, phys_to_virt, PAGE_SIZE};
use crate::sync::IrqSafeSpinlock;
use crate::{kinfo, kwarn};

/// Maximum supported physical memory: 64 GiB
pub const MAX_MANAGEABLE_PHYS: u64 = 64 * 1024 * 1024 * 1024;
/// Maximum manageable 4 KiB frames (16,777,216 for 64 GiB)
pub const MAX_FRAMES: usize = (MAX_MANAGEABLE_PHYS / (PAGE_SIZE as u64)) as usize;

pub struct PhysicalMemoryManager {
    bitmap_ptr: *mut u64,
    bitmap_words: usize,
    ref_counts_ptr: *mut u16,
    total_frames: usize,
    used_frames: usize,
    pub usable_frames: usize,
    pub parsed_from_e820: bool,
    pub e820_available_bytes: u64,
    pub e820_reserved_bytes: u64,
    last_search_idx: usize,
}

unsafe impl Send for PhysicalMemoryManager {}
unsafe impl Sync for PhysicalMemoryManager {}

impl PhysicalMemoryManager {
    pub const fn new() -> Self {
        Self {
            bitmap_ptr: core::ptr::null_mut(),
            bitmap_words: 0,
            ref_counts_ptr: core::ptr::null_mut(),
            total_frames: 0,
            used_frames: 0,
            usable_frames: 0,
            parsed_from_e820: false,
            e820_available_bytes: 0,
            e820_reserved_bytes: 0,
            last_search_idx: 0,
        }
    }

    pub fn set_frame_allocated(&mut self, frame: usize) {
        if frame < self.total_frames && !self.bitmap_ptr.is_null() {
            let idx = frame / 64;
            let bit = frame % 64;
            unsafe {
                let word = *self.bitmap_ptr.add(idx);
                if (word & (1 << bit)) == 0 {
                    *self.bitmap_ptr.add(idx) = word | (1 << bit);
                    self.used_frames += 1;
                }
            }
        }
    }

    pub fn set_frame_free(&mut self, frame: usize) {
        if frame < self.total_frames && !self.bitmap_ptr.is_null() {
            let idx = frame / 64;
            let bit = frame % 64;
            unsafe {
                let word = *self.bitmap_ptr.add(idx);
                if (word & (1 << bit)) != 0 {
                    *self.bitmap_ptr.add(idx) = word & !(1 << bit);
                    if self.used_frames > 0 {
                        self.used_frames -= 1;
                    }
                    if idx < self.last_search_idx {
                        self.last_search_idx = idx;
                    }
                }
            }
        }
    }

    pub fn is_frame_allocated(&self, frame: usize) -> bool {
        if frame >= self.total_frames || self.bitmap_ptr.is_null() {
            true
        } else {
            let idx = frame / 64;
            let bit = frame % 64;
            unsafe {
                (*self.bitmap_ptr.add(idx) & (1 << bit)) != 0
            }
        }
    }

    /// Mark an E820 available RAM region as free using inward page alignment
    /// so partial boundary pages containing firmware data are never exposed.
    pub fn register_available_region(&mut self, base_addr: u64, length: u64) {
        if length == 0 || base_addr >= MAX_MANAGEABLE_PHYS {
            return;
        }
        let end = base_addr.saturating_add(length).min(MAX_MANAGEABLE_PHYS);
        let aligned_start = align_up(base_addr, PAGE_SIZE as u64);
        let aligned_end = align_down(end, PAGE_SIZE as u64);
        if aligned_end <= aligned_start {
            return;
        }

        let start_frame = (aligned_start as usize) / PAGE_SIZE;
        let end_frame = ((aligned_end as usize) / PAGE_SIZE).min(self.total_frames);
        for frame in start_frame..end_frame {
            self.set_frame_free(frame);
        }
    }

    /// Mark a reserved/ACPI/NVS/MMIO/kernel region as allocated using outward
    /// page alignment so partial overlapping pages are strictly protected.
    pub fn reserve_region(&mut self, base_addr: u64, length: u64) {
        if length == 0 || base_addr >= MAX_MANAGEABLE_PHYS {
            return;
        }
        let end = base_addr.saturating_add(length).min(MAX_MANAGEABLE_PHYS);
        let aligned_start = align_down(base_addr, PAGE_SIZE as u64);
        let aligned_end = align_up(end, PAGE_SIZE as u64).min(MAX_MANAGEABLE_PHYS);
        if aligned_end <= aligned_start {
            return;
        }

        let start_frame = (aligned_start as usize) / PAGE_SIZE;
        let end_frame = ((aligned_end as usize) / PAGE_SIZE).min(self.total_frames);
        for frame in start_frame..end_frame {
            self.set_frame_allocated(frame);
        }
    }

    /// Allocate a single 4KiB physical frame.
    pub fn alloc_frame(&mut self) -> Option<u64> {
        if self.bitmap_ptr.is_null() || self.total_frames == 0 {
            return None;
        }
        for offset in 0..self.bitmap_words {
            let idx = (self.last_search_idx + offset) % self.bitmap_words;
            unsafe {
                let word = *self.bitmap_ptr.add(idx);
                if word != !0 {
                    let bit = (!word).trailing_zeros() as usize;
                    let frame = idx * 64 + bit;
                    if frame < self.total_frames {
                        *self.bitmap_ptr.add(idx) = word | (1 << bit);
                        self.used_frames += 1;
                        self.last_search_idx = idx;
                        if !self.ref_counts_ptr.is_null() {
                            *self.ref_counts_ptr.add(frame) = 1;
                        }
                        return Some((frame * PAGE_SIZE) as u64);
                    }
                }
            }
        }
        None
    }

    /// Allocate multiple contiguous 4KiB physical frames.
    pub fn alloc_contiguous_frames(&mut self, count: usize) -> Option<u64> {
        if self.bitmap_ptr.is_null() || count == 0 {
            return None;
        }
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
                        if !self.ref_counts_ptr.is_null() {
                            unsafe {
                                *self.ref_counts_ptr.add(f) = 1;
                            }
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
        if frame < self.total_frames && !self.ref_counts_ptr.is_null() {
            unsafe {
                *self.ref_counts_ptr.add(frame) = 0;
            }
        }
        self.set_frame_free(frame);
    }

    /// Increment reference count for a physical frame (e.g. during fork / CoW).
    pub fn inc_frame_ref(&mut self, phys_addr: u64) {
        let frame = (phys_addr as usize) / PAGE_SIZE;
        if frame < self.total_frames && !self.ref_counts_ptr.is_null() {
            unsafe {
                let ptr = self.ref_counts_ptr.add(frame);
                *ptr = (*ptr).saturating_add(1);
            }
        }
    }

    /// Decrement reference count for a physical frame. Returns the remaining count.
    pub fn dec_frame_ref(&mut self, phys_addr: u64) -> u16 {
        let frame = (phys_addr as usize) / PAGE_SIZE;
        if frame < self.total_frames && !self.ref_counts_ptr.is_null() {
            unsafe {
                let ptr = self.ref_counts_ptr.add(frame);
                if *ptr > 0 {
                    *ptr -= 1;
                }
                *ptr
            }
        } else {
            0
        }
    }

    /// Get current reference count for a physical frame.
    pub fn get_frame_ref(&self, phys_addr: u64) -> u16 {
        let frame = (phys_addr as usize) / PAGE_SIZE;
        if frame < self.total_frames && !self.ref_counts_ptr.is_null() {
            unsafe { *self.ref_counts_ptr.add(frame) }
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
        let reported_total = if self.usable_frames > 0 {
            self.usable_frames
        } else {
            self.total_frames
        };
        let reported_used = if reported_total >= free {
            reported_total - free
        } else {
            self.used_frames
        };
        (reported_total, reported_used, free)
    }

    pub fn usable_ram_bytes(&self) -> u64 {
        if self.e820_available_bytes > 0 {
            self.e820_available_bytes
        } else if self.usable_frames > 0 {
            (self.usable_frames as u64) * (PAGE_SIZE as u64)
        } else {
            (self.total_frames as u64) * (PAGE_SIZE as u64)
        }
    }

    pub fn get_total_frames(&self) -> usize {
        if self.usable_frames > 0 {
            self.usable_frames
        } else {
            self.total_frames
        }
    }

    pub fn get_used_frames(&self) -> usize {
        self.used_frames
    }
}

pub static PMM: IrqSafeSpinlock<PhysicalMemoryManager> = IrqSafeSpinlock::new(PhysicalMemoryManager::new());

/// Allocate a single 4 KiB physical frame and zero-initialize its contents via direct map.
pub fn alloc_zeroed_frame() -> Option<u64> {
    let frame_phys = PMM.lock().alloc_frame()?;
    unsafe {
        core::ptr::write_bytes(crate::mm::phys_to_virt(frame_phys) as *mut u8, 0, PAGE_SIZE);
    }
    Some(frame_phys)
}

extern "C" {
    static __kernel_phys_end: u8;
}

/// Initialize the Physical Memory Manager using E820 Hardware Memory Map Scrubbing
/// and dynamic frame sizing backed by Higher-Half Direct Paging (up to 64 GiB).
pub fn init(mb_info_addr: u64) {
    let mb_data = *crate::arch::x86_64::multiboot::MULTIBOOT.lock();
    let mut pmm = PMM.lock();

    let mmap_tags = mb_data.memory_map_tags();

    let mut highest_avail_phys: u64 = 0;
    let mut total_avail_bytes: u64 = 0;
    let mut total_resv_bytes: u64 = 0;

    if !mmap_tags.is_empty() {
        for entry in mmap_tags {
            if entry.is_available() {
                total_avail_bytes = total_avail_bytes.saturating_add(entry.length);
                let clamped_end = entry.end_addr().min(MAX_MANAGEABLE_PHYS);
                if clamped_end > highest_avail_phys {
                    highest_avail_phys = clamped_end;
                }
            } else {
                total_resv_bytes = total_resv_bytes.saturating_add(entry.length);
            }
        }
    } else if mb_data.mem_upper_kb > 0 {
        highest_avail_phys = (1024u64 * 1024)
            .saturating_add((mb_data.mem_upper_kb as u64) * 1024)
            .min(MAX_MANAGEABLE_PHYS);
        total_avail_bytes = highest_avail_phys.saturating_sub(1024 * 1024);
        kwarn!("PMM: E820 mmap absent; using Multiboot mem_upper ({} KiB)", mb_data.mem_upper_kb);
    } else {
        highest_avail_phys = 128 * 1024 * 1024;
        total_avail_bytes = highest_avail_phys.saturating_sub(1024 * 1024);
        kwarn!("PMM: No Multiboot memory map detected; using 128 MiB emergency fallback");
    }

    let total_frames = (highest_avail_phys as usize / PAGE_SIZE).min(MAX_FRAMES);
    let bitmap_words = (total_frames + 63) / 64;
    let bitmap_bytes = bitmap_words * 8;
    let ref_counts_bytes = total_frames * 2;
    let total_metadata_bytes = bitmap_bytes + ref_counts_bytes;
    let metadata_pages = ((total_metadata_bytes + PAGE_SIZE - 1) / PAGE_SIZE) as u64;

    // Locate safe physical memory above the kernel image for PMM metadata structures
    let kernel_end_phys = unsafe { &__kernel_phys_end as *const u8 as u64 };
    let mut metadata_phys_start = align_up(kernel_end_phys, PAGE_SIZE as u64);

    // If Multiboot buffer is placed right around kernel_end_phys, avoid colliding with it
    if mb_info_addr >= metadata_phys_start && mb_info_addr < metadata_phys_start + (16 * 1024 * 1024) {
        let mb_end = mb_info_addr + mb_data.info_total_bytes;
        if mb_end > metadata_phys_start {
            metadata_phys_start = align_up(mb_end, PAGE_SIZE as u64);
        }
    }
    if mb_data.mmap_addr_phys >= metadata_phys_start && mb_data.mmap_addr_phys < metadata_phys_start + (16 * 1024 * 1024) {
        let mmap_end = mb_data.mmap_addr_phys + mb_data.mmap_length_bytes;
        if mmap_end > metadata_phys_start {
            metadata_phys_start = align_up(mmap_end, PAGE_SIZE as u64);
        }
    }

    let metadata_phys_end = metadata_phys_start + (metadata_pages * PAGE_SIZE as u64);
    let metadata_virt_base = phys_to_virt(metadata_phys_start);

    let bitmap_ptr = metadata_virt_base as *mut u64;
    let ref_counts_ptr = (metadata_virt_base + bitmap_bytes as u64) as *mut u16;

    // Initialize metadata: mark all frames allocated initially, clear ref counts
    unsafe {
        core::ptr::write_bytes(bitmap_ptr as *mut u8, 0xFF, bitmap_bytes);
        core::ptr::write_bytes(ref_counts_ptr as *mut u8, 0x00, ref_counts_bytes);
    }

    pmm.bitmap_ptr = bitmap_ptr;
    pmm.bitmap_words = bitmap_words;
    pmm.ref_counts_ptr = ref_counts_ptr;
    pmm.total_frames = total_frames;
    pmm.used_frames = total_frames;
    pmm.usable_frames = (total_avail_bytes / PAGE_SIZE as u64) as usize;
    pmm.last_search_idx = 0;
    pmm.parsed_from_e820 = !mmap_tags.is_empty();
    pmm.e820_available_bytes = total_avail_bytes;
    pmm.e820_reserved_bytes = total_resv_bytes;

    if !mmap_tags.is_empty() {
        // Pass 1: Register ONLY verified Type 1 (Available RAM) regions
        for entry in mmap_tags {
            if entry.is_available() {
                pmm.register_available_region(entry.base_addr, entry.length);
            }
        }

        // Pass 2: Strictly scrub non-available regions (Reserved, ACPI, NVS, BadRam, MMIO)
        // Applied second so firmware-reserved holes always take precedence over overlapping spans.
        for entry in mmap_tags {
            if !entry.is_available() {
                pmm.reserve_region(entry.base_addr, entry.length);
            }
        }
    } else {
        pmm.register_available_region(1024 * 1024, highest_avail_phys.saturating_sub(1024 * 1024));
    }

    // Pass 3: Strictly protect low 1 MiB (IVT, BDA, SMP 16-bit trampoline @ 0x8000, VGA, BIOS ROM)
    pmm.reserve_region(0, 1024 * 1024);

    // Protect kernel ELF physical footprint AND dynamic PMM metadata (1 MiB .. metadata_phys_end)
    if metadata_phys_end > 1024 * 1024 {
        pmm.reserve_region(1024 * 1024, metadata_phys_end - 1024 * 1024);
    }

    // Protect Multiboot information structure, E820 buffer, ACPI tag, and framebuffer
    if mb_info_addr != 0 {
        pmm.reserve_region(mb_info_addr, mb_data.info_total_bytes.max(PAGE_SIZE as u64));
    }
    if mb_data.mmap_addr_phys != 0 && mb_data.mmap_length_bytes > 0 {
        pmm.reserve_region(mb_data.mmap_addr_phys, mb_data.mmap_length_bytes);
    }
    if let Some(rsdp_p) = mb_data.acpi_rsdp_phys {
        pmm.reserve_region(rsdp_p, 64);
    }
    if let Some(fb) = mb_data.framebuffer {
        let fb_bytes = (fb.pitch as u64).saturating_mul(fb.height as u64);
        if fb_bytes > 0 {
            pmm.reserve_region(fb.addr, fb_bytes);
        }
    }

    let (total, used, free) = pmm.stats();
    kinfo!(
        "PMM Initialized ({}): Manageable: {} MB ({} frames), Used/Reserved: {} MB, Free: {} MB",
        if pmm.parsed_from_e820 { "E820 Scrubbed" } else { "Fallback" },
        (total * PAGE_SIZE) / (1024 * 1024),
        total,
        (used * PAGE_SIZE) / (1024 * 1024),
        (free * PAGE_SIZE) / (1024 * 1024)
    );
}
