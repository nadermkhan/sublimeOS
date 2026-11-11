pub mod address_space;
pub mod heap;
pub mod pmm;
pub mod shm;
pub mod vmm;

pub const PAGE_SIZE: usize = 4096;
pub const PHYSICAL_MEMORY_OFFSET: u64 = 0xFFFFFFFF80000000;

/// Convert a physical memory address to a kernel virtual address via direct mapping.
#[inline(always)]
pub fn phys_to_virt(phys: u64) -> u64 {
    phys + PHYSICAL_MEMORY_OFFSET
}

/// Convert a kernel direct-mapped virtual address to a physical address.
#[inline(always)]
pub fn virt_to_phys(virt: u64) -> u64 {
    virt - PHYSICAL_MEMORY_OFFSET
}

/// Align address down to nearest 4KiB page boundary.
#[inline(always)]
pub const fn align_down(addr: u64, align: u64) -> u64 {
    addr & !(align - 1)
}

/// Align address up to nearest 4KiB page boundary.
#[inline(always)]
pub const fn align_up(addr: u64, align: u64) -> u64 {
    (addr + align - 1) & !(align - 1)
}
