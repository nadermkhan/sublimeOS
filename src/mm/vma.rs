extern crate alloc;
use alloc::vec::Vec;
use crate::sync::IrqSafeSpinlock;
use crate::mm::pmm::PMM;
use crate::mm::vmm::{VMM, PageTable, PAGE_PRESENT, PAGE_USER, PAGE_WRITABLE};
use crate::mm::swap::SWAP_MANAGER;
use crate::mm::{phys_to_virt, align_down};

#[derive(Debug, Clone, Copy)]
pub struct VmaRegion {
    pub start: u64,
    pub end: u64,
    pub flags: u64,
    pub pml4_phys: u64,
    pub is_demand_paged: bool,
}

pub struct VmaManager {
    pub regions: Vec<VmaRegion>,
}

impl VmaManager {
    pub const fn new() -> Self {
        Self {
            regions: Vec::new(),
        }
    }

    /// Register a virtual memory area for lazy demand paging.
    pub fn register_demand_region(&mut self, pml4_phys: u64, start: u64, length: u64, flags: u64) {
        let aligned_start = align_down(start, 4096);
        let aligned_end = crate::mm::align_up(start + length, 4096);
        self.regions.push(VmaRegion {
            start: aligned_start,
            end: aligned_end,
            flags,
            pml4_phys,
            is_demand_paged: true,
        });
    }

    /// Unregister a previously registered demand-paged VMA region.
    pub fn unregister_demand_region(&mut self, pml4_phys: u64, start: u64) {
        let aligned_start = align_down(start, 4096);
        self.regions.retain(|r| !(r.pml4_phys == pml4_phys && r.start == aligned_start));
    }

    /// Look up if a virtual address falls within a demand-paged region.
    pub fn find_region(&self, pml4_phys: u64, vaddr: u64) -> Option<VmaRegion> {
        self.regions.iter()
            .find(|r| (r.pml4_phys == pml4_phys || r.pml4_phys == 0) && vaddr >= r.start && vaddr < r.end)
            .copied()
    }
}

pub static VMA_MANAGER: IrqSafeSpinlock<VmaManager> = IrqSafeSpinlock::new(VmaManager::new());

/// Top-level demand paging & swap fault handler called from IDT Vector 14 (#PF).
pub fn handle_demand_fault(pml4_phys: u64, fault_vaddr: u64, is_write: bool) -> Result<(), &'static str> {
    // 1. Check if the page is currently swapped out to swap storage
    let swapped_slot = SWAP_MANAGER.lock().find_swapped_slot(pml4_phys, fault_vaddr);
    if let Some(slot_id) = swapped_slot {
        return SWAP_MANAGER.lock().swap_in(slot_id, pml4_phys, fault_vaddr);
    }

    let aligned_vaddr = align_down(fault_vaddr, 4096);

    // 2. Check if address belongs to a registered demand-paged VMA
    let reg_opt = VMA_MANAGER.lock().find_region(pml4_phys, fault_vaddr);
    if let Some(region) = reg_opt {
        if !region.is_demand_paged {
            return Err("VMA is not demand-paged");
        }

        let frame = PMM.lock().alloc_frame().ok_or("Out of memory for demand paging")?;
        let virt_frame = phys_to_virt(frame) as *mut u8;
        unsafe {
            core::ptr::write_bytes(virt_frame, 0, 4096);
        }

        let mut map_flags = PAGE_PRESENT | PAGE_USER;
        if (region.flags & 2) != 0 || is_write {
            map_flags |= PAGE_WRITABLE;
        }

        let pml4 = unsafe { &mut *(phys_to_virt(pml4_phys) as *mut PageTable) };
        VMM.lock().map_page(pml4, aligned_vaddr, frame, map_flags)?;
        return Ok(());
    }

    // 3. Check active user process address space VMAs
    let user_space = crate::task::user::CURRENT_USER_ADDRESS_SPACE.lock();
    if let Some(ref space) = *user_space {
        if space.pml4_phys == pml4_phys {
            for vma in &space.vmas {
                if fault_vaddr >= vma.start && fault_vaddr < vma.end {
                    let frame = PMM.lock().alloc_frame().ok_or("Out of memory for user demand fault")?;
                    let virt_frame = phys_to_virt(frame) as *mut u8;
                    unsafe {
                        core::ptr::write_bytes(virt_frame, 0, 4096);
                    }
                    let mut map_flags = PAGE_PRESENT | PAGE_USER;
                    if (vma.flags & 2) != 0 || is_write {
                        map_flags |= PAGE_WRITABLE;
                    }
                    let pml4 = unsafe { &mut *(phys_to_virt(pml4_phys) as *mut PageTable) };
                    VMM.lock().map_page(pml4, aligned_vaddr, frame, map_flags)?;
                    return Ok(());
                }
            }
        }
    }

    Err("Address not found in demand-paged VMA regions")
}
