use core::alloc::{GlobalAlloc, Layout};
use core::ptr::NonNull;
use crate::mm::pmm::PMM;
use crate::mm::{align_up, phys_to_virt, PAGE_SIZE};
use crate::sync::IrqSafeSpinlock;
use crate::kinfo;

pub const INITIAL_HEAP_PAGES: usize = 256; // 1 MiB initial heap allocation

struct FreeBlock {
    size: usize,
    next: Option<NonNull<FreeBlock>>,
}

pub struct HeapAllocator {
    head: Option<NonNull<FreeBlock>>,
    allocated_bytes: usize,
    total_bytes: usize,
}

unsafe impl Send for HeapAllocator {}

impl HeapAllocator {
    pub const fn new() -> Self {
        Self {
            head: None,
            allocated_bytes: 0,
            total_bytes: 0,
        }
    }

    /// Expand heap by allocating physical frames and using higher-half direct mapping.
    pub fn expand(&mut self, pages: usize) -> Result<(), &'static str> {
        let mut pmm = PMM.lock();
        let phys = pmm.alloc_contiguous_frames(pages).ok_or("Out of physical frames for heap")?;
        let virt = phys_to_virt(phys) as usize;
        let added_bytes = pages * PAGE_SIZE;

        self.total_bytes += added_bytes;

        unsafe {
            self.add_free_region(virt, added_bytes);
        }
        Ok(())
    }

    /// Insert a free memory region into the free list.
    unsafe fn add_free_region(&mut self, addr: usize, size: usize) {
        if size < core::mem::size_of::<FreeBlock>() {
            return;
        }

        let new_block = addr as *mut FreeBlock;
        (*new_block).size = size;
        (*new_block).next = self.head;
        self.head = Some(NonNull::new_unchecked(new_block));
    }

    /// Allocate a block satisfying layout requirements.
    pub fn alloc(&mut self, layout: Layout) -> *mut u8 {
        let align = layout.align().max(core::mem::align_of::<FreeBlock>());
        let header_size = core::mem::size_of::<FreeBlock>();
        let size = align_up((layout.size() + header_size) as u64, align as u64) as usize;

        // Find a suitable block in the free list
        let mut prev: Option<NonNull<FreeBlock>> = None;
        let mut curr = self.head;

        while let Some(mut block_ptr) = curr {
            let block = unsafe { block_ptr.as_mut() };

            if block.size >= size {
                // Remove block from list
                if let Some(mut prev_ptr) = prev {
                    unsafe { prev_ptr.as_mut().next = block.next };
                } else {
                    self.head = block.next;
                }

                // If excess size is large enough, split it
                let excess = block.size - size;
                if excess >= header_size + 16 {
                    block.size = size;
                    let excess_addr = (block_ptr.as_ptr() as usize) + size;
                    unsafe {
                        self.add_free_region(excess_addr, excess);
                    }
                }

                self.allocated_bytes += block.size;
                let payload_ptr = unsafe { (block_ptr.as_ptr() as *mut u8).add(header_size) };
                return payload_ptr;
            }

            prev = curr;
            curr = block.next;
        }

        // Need more memory: expand heap
        let needed_pages = align_up(size as u64, PAGE_SIZE as u64) as usize / PAGE_SIZE;
        let expand_pages = needed_pages.max(64); // Expand at least 256 KiB at a time

        if self.expand(expand_pages).is_ok() {
            self.alloc(layout)
        } else {
            core::ptr::null_mut()
        }
    }

    /// Free a previously allocated block.
    pub fn dealloc(&mut self, ptr: *mut u8, _layout: Layout) {
        if ptr.is_null() {
            return;
        }

        let header_size = core::mem::size_of::<FreeBlock>();
        let block_ptr = unsafe { ptr.sub(header_size) as *mut FreeBlock };
        let block_size = unsafe { (*block_ptr).size };

        if self.allocated_bytes >= block_size {
            self.allocated_bytes -= block_size;
        }

        unsafe {
            self.add_free_region(block_ptr as usize, block_size);
        }
    }

    pub fn stats(&self) -> (usize, usize, usize) {
        (self.total_bytes, self.allocated_bytes, self.total_bytes.saturating_sub(self.allocated_bytes))
    }
}

pub struct KernelHeapAllocator {
    inner: IrqSafeSpinlock<HeapAllocator>,
}

impl KernelHeapAllocator {
    pub const fn new() -> Self {
        Self {
            inner: IrqSafeSpinlock::new(HeapAllocator::new()),
        }
    }

    pub fn init(&self) {
        let mut heap = self.inner.lock();
        if heap.expand(INITIAL_HEAP_PAGES).is_ok() {
            let (total, _used, free) = heap.stats();
            kinfo!(
                "Heap Initialized: Total: {} KB, Free: {} KB",
                total / 1024,
                free / 1024
            );
        }
    }

    pub fn stats(&self) -> (usize, usize, usize) {
        self.inner.lock().stats()
    }
}

unsafe impl GlobalAlloc for KernelHeapAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        self.inner.lock().alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        self.inner.lock().dealloc(ptr, layout);
    }
}

#[global_allocator]
pub static HEAP_ALLOCATOR: KernelHeapAllocator = KernelHeapAllocator::new();
