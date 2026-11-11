use crate::arch::x86_64::registers::{invlpg, read_cr3, write_cr3};
use crate::mm::pmm::PMM;
use crate::mm::phys_to_virt;
use crate::sync::IrqSafeSpinlock;

pub const PAGE_PRESENT: u64 = 1 << 0;
pub const PAGE_WRITABLE: u64 = 1 << 1;
pub const PAGE_USER: u64 = 1 << 2;
pub const PAGE_WRITE_THROUGH: u64 = 1 << 3;
pub const PAGE_NO_CACHE: u64 = 1 << 4;
pub const PAGE_ACCESSED: u64 = 1 << 5;
pub const PAGE_DIRTY: u64 = 1 << 6;
pub const PAGE_HUGE: u64 = 1 << 7;
pub const PAGE_GLOBAL: u64 = 1 << 8;
pub const PAGE_NO_EXECUTE: u64 = 1 << 63;

pub const ADDRESS_MASK: u64 = 0x000F_FFFF_FFFF_F000;

#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct PageTableEntry(pub u64);

impl PageTableEntry {
    pub const fn empty() -> Self {
        PageTableEntry(0)
    }

    pub fn is_present(&self) -> bool {
        (self.0 & PAGE_PRESENT) != 0
    }

    pub fn is_huge(&self) -> bool {
        (self.0 & PAGE_HUGE) != 0
    }

    pub fn address(&self) -> u64 {
        self.0 & ADDRESS_MASK
    }

    pub fn set(&mut self, phys_addr: u64, flags: u64) {
        self.0 = (phys_addr & ADDRESS_MASK) | flags;
    }

    pub fn clear(&mut self) {
        self.0 = 0;
    }
}

#[repr(C, align(4096))]
pub struct PageTable {
    pub entries: [PageTableEntry; 512],
}

impl PageTable {
    pub const fn empty() -> Self {
        PageTable {
            entries: [PageTableEntry::empty(); 512],
        }
    }

    pub fn zero(&mut self) {
        for entry in self.entries.iter_mut() {
            entry.clear();
        }
    }
}

#[inline(always)]
pub fn pml4_index(virt: u64) -> usize {
    ((virt >> 39) & 0x1FF) as usize
}

#[inline(always)]
pub fn pdpt_index(virt: u64) -> usize {
    ((virt >> 30) & 0x1FF) as usize
}

#[inline(always)]
pub fn pd_index(virt: u64) -> usize {
    ((virt >> 21) & 0x1FF) as usize
}

#[inline(always)]
pub fn pt_index(virt: u64) -> usize {
    ((virt >> 12) & 0x1FF) as usize
}

pub struct VirtualMemoryManager {
    kernel_pml4_phys: u64,
}

impl VirtualMemoryManager {
    pub const fn new() -> Self {
        Self {
            kernel_pml4_phys: 0,
        }
    }

    pub fn init(&mut self) {
        // Read current CR3 loaded by early trampoline
        let cr3 = unsafe { read_cr3() };
        self.kernel_pml4_phys = cr3 & ADDRESS_MASK;
    }

    pub fn get_kernel_pml4(&self) -> &'static mut PageTable {
        unsafe { &mut *(phys_to_virt(self.kernel_pml4_phys) as *mut PageTable) }
    }

    /// Map a 4KiB virtual page to a physical frame.
    pub fn map_page(&self, pml4: &mut PageTable, virt: u64, phys: u64, flags: u64) -> Result<(), &'static str> {
        let pml4_idx = pml4_index(virt);
        let pdpt_idx = pdpt_index(virt);
        let pd_idx = pd_index(virt);
        let pt_idx = pt_index(virt);

        // 1. PML4 -> PDPT
        let pdpt = if !pml4.entries[pml4_idx].is_present() {
            let frame = PMM.lock().alloc_frame().ok_or("Out of memory for PDPT")?;
            let new_table = unsafe { &mut *(phys_to_virt(frame) as *mut PageTable) };
            new_table.zero();
            pml4.entries[pml4_idx].set(frame, PAGE_PRESENT | PAGE_WRITABLE | (flags & PAGE_USER));
            new_table
        } else {
            let addr = pml4.entries[pml4_idx].address();
            unsafe { &mut *(phys_to_virt(addr) as *mut PageTable) }
        };

        // 2. PDPT -> PD
        let pd = if !pdpt.entries[pdpt_idx].is_present() {
            let frame = PMM.lock().alloc_frame().ok_or("Out of memory for PD")?;
            let new_table = unsafe { &mut *(phys_to_virt(frame) as *mut PageTable) };
            new_table.zero();
            pdpt.entries[pdpt_idx].set(frame, PAGE_PRESENT | PAGE_WRITABLE | (flags & PAGE_USER));
            new_table
        } else {
            let addr = pdpt.entries[pdpt_idx].address();
            unsafe { &mut *(phys_to_virt(addr) as *mut PageTable) }
        };

        // 3. PD -> PT
        let pt = if !pd.entries[pd_idx].is_present() {
            let frame = PMM.lock().alloc_frame().ok_or("Out of memory for PT")?;
            let new_table = unsafe { &mut *(phys_to_virt(frame) as *mut PageTable) };
            new_table.zero();
            pd.entries[pd_idx].set(frame, PAGE_PRESENT | PAGE_WRITABLE | (flags & PAGE_USER));
            new_table
        } else {
            let addr = pd.entries[pd_idx].address();
            unsafe { &mut *(phys_to_virt(addr) as *mut PageTable) }
        };

        // 4. Set PT entry
        pt.entries[pt_idx].set(phys, flags | PAGE_PRESENT);

        // Invalidate TLB for this virtual address
        unsafe {
            invlpg(virt);
        }

        Ok(())
    }

    /// Unmap a virtual page.
    pub fn unmap_page(&self, pml4: &mut PageTable, virt: u64) {
        let pml4_idx = pml4_index(virt);
        if !pml4.entries[pml4_idx].is_present() {
            return;
        }

        let pdpt = unsafe { &mut *(phys_to_virt(pml4.entries[pml4_idx].address()) as *mut PageTable) };
        let pdpt_idx = pdpt_index(virt);
        if !pdpt.entries[pdpt_idx].is_present() {
            return;
        }

        let pd = unsafe { &mut *(phys_to_virt(pdpt.entries[pdpt_idx].address()) as *mut PageTable) };
        let pd_idx = pd_index(virt);
        if !pd.entries[pd_idx].is_present() {
            return;
        }

        let pt = unsafe { &mut *(phys_to_virt(pd.entries[pd_idx].address()) as *mut PageTable) };
        let pt_idx = pt_index(virt);

        pt.entries[pt_idx].clear();
        unsafe {
            invlpg(virt);
        }
    }

    /// Translate a virtual address to its corresponding physical address.
    pub fn translate(&self, pml4: &PageTable, virt: u64) -> Option<u64> {
        let pml4_idx = pml4_index(virt);
        if !pml4.entries[pml4_idx].is_present() {
            return None;
        }

        let pdpt = unsafe { &*(phys_to_virt(pml4.entries[pml4_idx].address()) as *const PageTable) };
        let pdpt_idx = pdpt_index(virt);
        if !pdpt.entries[pdpt_idx].is_present() {
            return None;
        }

        if pdpt.entries[pdpt_idx].is_huge() {
            // 1 GiB huge page
            let offset = virt & 0x3FFF_FFFF;
            return Some(pdpt.entries[pdpt_idx].address() + offset);
        }

        let pd = unsafe { &*(phys_to_virt(pdpt.entries[pdpt_idx].address()) as *const PageTable) };
        let pd_idx = pd_index(virt);
        if !pd.entries[pd_idx].is_present() {
            return None;
        }

        if pd.entries[pd_idx].is_huge() {
            // 2 MiB huge page
            let offset = virt & 0x1F_FFFF;
            return Some(pd.entries[pd_idx].address() + offset);
        }

        let pt = unsafe { &*(phys_to_virt(pd.entries[pd_idx].address()) as *const PageTable) };
        let pt_idx = pt_index(virt);
        if !pt.entries[pt_idx].is_present() {
            return None;
        }

        let offset = virt & 0xFFF;
        Some(pt.entries[pt_idx].address() + offset)
    }

    /// Create a new process address space by allocating a new PML4 table
    /// and copying kernel higher-half mappings (entries 256..512).
    pub fn create_address_space(&self) -> Option<u64> {
        let frame = PMM.lock().alloc_frame()?;
        let new_pml4 = unsafe { &mut *(phys_to_virt(frame) as *mut PageTable) };
        new_pml4.zero();

        let kernel_pml4 = self.get_kernel_pml4();
        // Copy higher half (kernel space: 256..512)
        for i in 256..512 {
            new_pml4.entries[i] = kernel_pml4.entries[i];
        }

        Some(frame)
    }

    /// Switch active address space (CR3).
    pub unsafe fn switch_address_space(&self, pml4_phys: u64) {
        write_cr3(pml4_phys);
    }
}

pub static VMM: IrqSafeSpinlock<VirtualMemoryManager> = IrqSafeSpinlock::new(VirtualMemoryManager::new());

static MMIO_NEXT_VIRT: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0xFFFFFFFFE0000000);

/// Map a physical MMIO memory range into a non-cacheable higher-half virtual address.
pub fn map_mmio_range(phys_addr: u64, size: usize) -> Result<u64, &'static str> {
    let page_offset = phys_addr & 0xFFF;
    let phys_aligned = phys_addr & !0xFFF;
    let total_size = (page_offset as usize + size + 4095) & !4095;
    let pages = total_size / 4096;

    let virt_base = MMIO_NEXT_VIRT.fetch_add(total_size as u64, core::sync::atomic::Ordering::SeqCst);

    let vmm = VMM.lock();
    let kernel_pml4 = vmm.get_kernel_pml4();
    let flags = PAGE_PRESENT | PAGE_WRITABLE | PAGE_NO_CACHE;

    for i in 0..pages {
        let p = phys_aligned + (i as u64 * 4096);
        let v = virt_base + (i as u64 * 4096);
        vmm.map_page(kernel_pml4, v, p, flags)?;
    }

    Ok(virt_base + page_offset)
}
