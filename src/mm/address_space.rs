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

    /// Map a physical memory region (e.g. MMIO framebuffer) into user space.
    pub fn map_phys_region(
        &mut self,
        start: u64,
        length: u64,
        phys_start: u64,
        flags: u64,
        vma_type: VmaType,
    ) -> Result<(), &'static str> {
        let pages = align_up(length, PAGE_SIZE as u64) / (PAGE_SIZE as u64);
        let vmm = VMM.lock();
        let pml4 = unsafe { &mut *(crate::mm::phys_to_virt(self.pml4_phys) as *mut crate::mm::vmm::PageTable) };

        for i in 0..pages {
            let virt = start + i * (PAGE_SIZE as u64);
            let phys = phys_start + i * (PAGE_SIZE as u64);
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
                let frame_opt = PMM.lock().alloc_frame();
                if let Some(phys) = frame_opt {
                    let _ = vmm.map_page(pml4, virt, phys, PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER);
                } else {
                    return self.brk_current;
                }
            }
            self.brk_current = new_brk;
        }

        self.brk_current
    }

    /// Clone this AddressSpace using Copy-on-Write (CoW).
    /// All writable pages in the lower half (user space, PML4 entries 0..256)
    /// are marked read-only and PAGE_COW in both parent and child.
    pub fn clone_cow(&mut self) -> Option<Self> {
        let child_pml4_phys = VMM.lock().create_address_space()?;
        let parent_pml4 = unsafe { &mut *(crate::mm::phys_to_virt(self.pml4_phys) as *mut crate::mm::vmm::PageTable) };
        let child_pml4 = unsafe { &mut *(crate::mm::phys_to_virt(child_pml4_phys) as *mut crate::mm::vmm::PageTable) };

        for i in 0..256 {
            if !parent_pml4.entries[i].is_present() {
                continue;
            }

            let parent_pdpt = unsafe { &mut *(crate::mm::phys_to_virt(parent_pml4.entries[i].address()) as *mut crate::mm::vmm::PageTable) };
            let child_pdpt_frame = PMM.lock().alloc_frame()?;
            let child_pdpt = unsafe { &mut *(crate::mm::phys_to_virt(child_pdpt_frame) as *mut crate::mm::vmm::PageTable) };
            child_pdpt.zero();
            child_pml4.entries[i].set(child_pdpt_frame, parent_pml4.entries[i].flags());

            for j in 0..512 {
                if !parent_pdpt.entries[j].is_present() {
                    continue;
                }

                let parent_pd = unsafe { &mut *(crate::mm::phys_to_virt(parent_pdpt.entries[j].address()) as *mut crate::mm::vmm::PageTable) };
                let child_pd_frame = PMM.lock().alloc_frame()?;
                let child_pd = unsafe { &mut *(crate::mm::phys_to_virt(child_pd_frame) as *mut crate::mm::vmm::PageTable) };
                child_pd.zero();
                child_pdpt.entries[j].set(child_pd_frame, parent_pdpt.entries[j].flags());

                for k in 0..512 {
                    if !parent_pd.entries[k].is_present() {
                        continue;
                    }

                    let parent_pt = unsafe { &mut *(crate::mm::phys_to_virt(parent_pd.entries[k].address()) as *mut crate::mm::vmm::PageTable) };
                    let child_pt_frame = PMM.lock().alloc_frame()?;
                    let child_pt = unsafe { &mut *(crate::mm::phys_to_virt(child_pt_frame) as *mut crate::mm::vmm::PageTable) };
                    child_pt.zero();
                    child_pd.entries[k].set(child_pt_frame, parent_pd.entries[k].flags());

                    for l in 0..512 {
                        let p_entry = &mut parent_pt.entries[l];
                        if !p_entry.is_present() {
                            continue;
                        }

                        let phys = p_entry.address();
                        let mut flags = p_entry.flags();

                        // If writable, mark both parent and child as Copy-on-Write
                        if (flags & crate::mm::vmm::PAGE_WRITABLE) != 0 {
                            flags &= !crate::mm::vmm::PAGE_WRITABLE;
                            flags |= crate::mm::vmm::PAGE_COW;
                            p_entry.set(phys, flags);
                        }

                        // Child gets identical flags and points to the same frame
                        child_pt.entries[l].set(phys, flags);

                        // Increment reference count for the shared physical frame
                        PMM.lock().inc_frame_ref(phys);
                    }
                }
            }
        }

        // Reload CR3 in parent if currently active so read-only permissions take immediate effect
        let current_cr3 = unsafe { crate::arch::x86_64::registers::read_cr3() };
        if current_cr3 == self.pml4_phys {
            unsafe {
                crate::arch::x86_64::registers::write_cr3(self.pml4_phys);
            }
        }

        Some(Self {
            pml4_phys: child_pml4_phys,
            vmas: self.vmas.clone(),
            brk_start: self.brk_start,
            brk_current: self.brk_current,
        })
    }

    /// Write a 64-bit value to a virtual address in this user address space.
    pub fn write_user_u64(&self, vaddr: u64, val: u64) -> Result<(), &'static str> {
        let pml4 = unsafe { &*(crate::mm::phys_to_virt(self.pml4_phys) as *const crate::mm::vmm::PageTable) };
        if let Some(phys) = VMM.lock().translate(pml4, vaddr) {
            let ptr = crate::mm::phys_to_virt(phys) as *mut u64;
            unsafe {
                core::ptr::write_unaligned(ptr, val);
            }
            Ok(())
        } else {
            Err("Virtual address not mapped")
        }
    }
}
