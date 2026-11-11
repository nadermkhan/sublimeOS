extern crate alloc;
use alloc::boxed::Box;
use alloc::vec::Vec;
use crate::sync::IrqSafeSpinlock;
use crate::mm::pmm::PMM;
use crate::mm::vmm::{VMM, PageTable, PAGE_PRESENT, PAGE_USER, PAGE_WRITABLE, PAGE_SWAPPED};
use crate::mm::{phys_to_virt, align_down};
use crate::arch::x86_64::registers::invlpg;

pub const MAX_SWAP_SLOTS: usize = 256;

pub struct SwapSlot {
    pub slot_id: usize,
    pub pml4_phys: u64,
    pub vaddr: u64,
    pub flags: u64,
    pub is_used: bool,
    pub data: Box<[u8; 4096]>,
}

pub struct SwapManager {
    pub slots: Vec<SwapSlot>,
    pub total_swapped_out: usize,
    pub total_swapped_in: usize,
}

impl SwapManager {
    pub const fn new() -> Self {
        Self {
            slots: Vec::new(),
            total_swapped_out: 0,
            total_swapped_in: 0,
        }
    }

    /// Swap out an active page mapped at `vaddr` in `pml4_phys` to a swap slot.
    pub fn swap_out(&mut self, pml4_phys: u64, vaddr: u64) -> Result<usize, &'static str> {
        let aligned_vaddr = align_down(vaddr, 4096);
        let pml4 = unsafe { &mut *(phys_to_virt(pml4_phys) as *mut PageTable) };

        let vmm = VMM.lock();
        let pte = vmm.get_pte_mut(pml4, aligned_vaddr).ok_or("Page table entry not found")?;

        if !pte.is_present() {
            return Err("Page not present in memory");
        }

        let phys_frame = pte.address();
        let flags = pte.flags();

        // Find or allocate a swap slot
        let slot_idx = if let Some(idx) = self.slots.iter().position(|s| !s.is_used) {
            idx
        } else {
            if self.slots.len() >= MAX_SWAP_SLOTS {
                return Err("Out of swap space");
            }
            let new_id = self.slots.len();
            let new_slot = SwapSlot {
                slot_id: new_id,
                pml4_phys: 0,
                vaddr: 0,
                flags: 0,
                is_used: false,
                data: Box::new([0u8; 4096]),
            };
            self.slots.push(new_slot);
            self.slots.len() - 1
        };

        // Copy frame contents to swap slot buffer
        let src_virt = phys_to_virt(phys_frame) as *const u8;
        unsafe {
            core::ptr::copy_nonoverlapping(src_virt, self.slots[slot_idx].data.as_mut_ptr(), 4096);
        }

        self.slots[slot_idx].pml4_phys = pml4_phys;
        self.slots[slot_idx].vaddr = aligned_vaddr;
        self.slots[slot_idx].flags = flags;
        self.slots[slot_idx].is_used = true;

        // Release the physical frame to PMM
        PMM.lock().free_frame(phys_frame);

        // Update PTE: Present=0, Swapped=1, encode slot_id in address bits
        let slot_id = self.slots[slot_idx].slot_id;
        pte.set((slot_id as u64) << 12, PAGE_SWAPPED | (flags & (PAGE_USER | PAGE_WRITABLE)));

        // Invalidate TLB for the swapped address
        unsafe {
            invlpg(aligned_vaddr);
        }

        self.total_swapped_out += 1;
        Ok(slot_id)
    }

    /// Swap in a previously swapped page back into physical memory.
    pub fn swap_in(&mut self, slot_id: usize, pml4_phys: u64, vaddr: u64) -> Result<(), &'static str> {
        let aligned_vaddr = align_down(vaddr, 4096);
        let slot = self.slots.iter_mut().find(|s| s.slot_id == slot_id && s.is_used)
            .ok_or("Swap slot not found or unused")?;

        let flags = slot.flags;

        // Allocate a new physical frame from PMM
        let new_frame = PMM.lock().alloc_frame().ok_or("Out of physical memory for swap-in")?;

        // Copy saved bytes from swap slot into the newly allocated frame
        let dst_virt = phys_to_virt(new_frame) as *mut u8;
        unsafe {
            core::ptr::copy_nonoverlapping(slot.data.as_ptr(), dst_virt, 4096);
        }

        // Remap page in PML4 with original flags + Present
        let pml4 = unsafe { &mut *(phys_to_virt(pml4_phys) as *mut PageTable) };
        let vmm = VMM.lock();
        if let Some(pte) = vmm.get_pte_mut(pml4, aligned_vaddr) {
            pte.set(new_frame, (flags & !PAGE_SWAPPED) | PAGE_PRESENT);
        } else {
            vmm.map_page(pml4, aligned_vaddr, new_frame, (flags & !PAGE_SWAPPED) | PAGE_PRESENT)?;
        }

        unsafe {
            invlpg(aligned_vaddr);
        }

        slot.is_used = false;
        self.total_swapped_in += 1;
        Ok(())
    }

    /// Check if an address in the specified address space is currently swapped out.
    pub fn find_swapped_slot(&self, pml4_phys: u64, vaddr: u64) -> Option<usize> {
        let aligned_vaddr = align_down(vaddr, 4096);
        self.slots.iter()
            .find(|s| s.is_used && s.pml4_phys == pml4_phys && s.vaddr == aligned_vaddr)
            .map(|s| s.slot_id)
    }

    /// Number of currently used swap slots.
    pub fn used_slots_count(&self) -> usize {
        self.slots.iter().filter(|s| s.is_used).count()
    }
}

pub static SWAP_MANAGER: IrqSafeSpinlock<SwapManager> = IrqSafeSpinlock::new(SwapManager::new());
