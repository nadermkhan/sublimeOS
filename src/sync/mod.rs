//! Synchronization primitives and Kernel Lock Hierarchy.
//!
//! # CosmicOS Strict Lock Hierarchy
//! To eliminate multi-core deadlock hazards across preemptive scheduler ticks,
//! page fault exceptions, and I/O interrupts, the kernel defines a strict lock
//! order across all subsystems:
//!
//! 1. **Level 1: Task Scheduler & Process Table** (`SCHEDULER`, `PROCESS_TABLE`)
//!    - Manages task queues, thread control blocks, process IDs, and wait states.
//! 2. **Level 2: User Address Space** (`CURRENT_USER_ADDRESS_SPACE`)
//!    - Virtual Memory Areas (VMAs) and per-process PML4 address spaces.
//! 3. **Level 3: Virtual File System** (`VFS`, individual `InodeOperations` locks)
//!    - Directory path resolution, mount tables, and open file handles.
//! 4. **Level 4: Virtual Memory Management** (`VMM`)
//!    - 4-level paging manipulations, non-cacheable MMIO mappings, and TLB flushes.
//! 5. **Level 5: Physical Memory & Heap Allocator** (`PMM`, `HEAP_ALLOCATOR`)
//!    - Physical frame allocation bitmap, reference counts, and dynamic kernel heap.
//! 6. **Level 6: Hardware Controllers & Drivers** (`STORAGE_MANAGER`, `PCI`, `UART`, `HDA`, `NET`)
//!    - Controller registers, DMA descriptor rings, and network interface queues.
//!
//! **Lock Acquisition Invariant**: A CPU core holding a lock at Level $N$ may
//! ONLY acquire locks at Level $> N$. Never acquire a lock at Level $\le N$.

pub mod spinlock;

pub use spinlock::{IrqSafeSpinlock, IrqSafeSpinlockGuard, Spinlock, SpinlockGuard};
