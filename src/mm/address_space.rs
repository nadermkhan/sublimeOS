extern crate alloc;
use alloc::vec::Vec;
use crate::mm::pmm::PMM;
use crate::mm::vmm::{VMM, PAGE_PRESENT, PAGE_USER, PAGE_WRITABLE};
use crate::mm::{align_up, PAGE_SIZE};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmaType {
    Code,
    Data,
    Heap,
    Stack,
    Mmap,
}

#[derive(Debug, Clone)]
pub struct VirtualMemoryArea {
    pub start: u64,
    pub end: u64,
    pub flags: u64,
    pub vma_type: VmaType,
}

pub struct AddressSpace {
    pub pml4_phys: u64,
    pub vmas: Vec<VirtualMemoryArea>,
    pub brk_start: u64,
    pub brk_current: u64,
}

impl AddressSpace {
    pub fn new() -> Option<Self> {
        let pml4 = VMM.lock().create_address_space()?;
        Some(Self {
            pml4_phys: pml4,
            vmas: Vec::new(),
            brk_start: 0x4000_0000,
            brk_current: 0x4000_0000,
        })
    }

    /// Map a user memory region (e.g. text, data, or stack).
    pub fn map_region(&mut self, start: u64, length: u64, flags: u64, vma_type: VmaType) -> Result<(), &'static str> {
        let pages = align_up(length, PAGE_SIZE as u64) / (PAGE_SIZE as u64);
        let vmm = VMM.lock();
        let pml4 = unsafe { &mut *(crate::mm::phys_to_virt(self.pml4_phys) as *mut crate::mm::vmm::PageTable) };

        for i in 0..pages {
            let virt = start + i * (PAGE_SIZE as u64);
            let phys = PMM.lock().alloc_frame().ok_or("Out of memory for user region")?;
            vmm.map_page(pml4, virt, phys, flags | PAGE_PRESENT | PAGE_USER)?;
        }

        self.vmas.push(VirtualMemoryArea {
            start,
            end: start + length,
            flags,
            vma_type,
        });

        Ok(())
    }

    /// Implement the POSIX brk() system call.
    pub fn brk(&mut self, new_brk: u64) -> u64 {
        if new_brk == 0 || new_brk < self.brk_start {
            return self.brk_current;
        }

        if new_brk > self.brk_current {
            let old_aligned = align_up(self.brk_current, PAGE_SIZE as u64);
            let new_aligned = align_up(new_brk, PAGE_SIZE as u64);
            let pages = (new_aligned - old_aligned) / (PAGE_SIZE as u64);

            let vmm = VMM.lock();
            let pml4 = unsafe { &mut *(crate::mm::phys_to_virt(self.pml4_phys) as *mut crate::mm::vmm::PageTable) };

            for i in 0..pages {
                let virt = old_aligned + i * (PAGE_SIZE as u64);
                if let Some(phys) = PMM.lock().alloc_frame() {
                    let _ = vmm.map_page(pml4, virt, phys, PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER);
                } else {
                    return self.brk_current;
                }
            }
            self.brk_current = new_brk;
        }

        self.brk_current
    }
}
