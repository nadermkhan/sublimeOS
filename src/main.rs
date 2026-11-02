#![no_std]
#![no_main]

extern crate alloc;

use core::arch::global_asm;
use core::panic::PanicInfo;

global_asm!(include_str!("arch/x86_64/boot.s"));
global_asm!(include_str!("arch/x86_64/interrupts.s"));
global_asm!(include_str!("task/switch.s"));
global_asm!(include_str!("syscall/syscall_entry.s"));

pub mod arch;
pub mod drivers;
pub mod fs;
pub mod gui;
pub mod ipc;
pub mod mm;
pub mod shell;
pub mod sync;
pub mod syscall;
pub mod task;
pub mod tests;

#[no_mangle]
pub extern "C" fn kernel_main(magic: u64, mb_info: u64) -> ! {
    // 1. Initialize Architecture & Hardware Abstraction Layer
    arch::x86_64::init();

    kinfo!("=======================================================");
    kinfo!("  CosmosKernel Monolithic Kernel 0.1.0 (x86_64)");
    kinfo!("  Bootloader Magic: 0x{:08X}, Info: 0x{:08X}", magic, mb_info);
    kinfo!("=======================================================");

    // 2. Initialize Memory Management Subsystem
    kinfo!("Initializing Memory Management Subsystem (PMM, VMM, Heap)...");
    mm::pmm::init(mb_info);
    mm::vmm::VMM.lock().init();
    mm::heap::HEAP_ALLOCATOR.init();

    // 3. Initialize Preemptive Multitasking Scheduler
    task::scheduler::init();

    // 4. Initialize Virtual File System (VFS, RamFS, DevFS, ProcFS)
    fs::init();

    // 5. Initialize Device Drivers (Keyboard, PCI Bus Scanner)
    drivers::init();

    // 6. Enable CPU interrupts now that all subsystems and scheduler are active
    arch::x86_64::registers::enable_interrupts();
    kinfo!("CPU interrupts enabled");

    // 7. Run Kernel Self-Test Suite to verify all subsystems
    tests::run_all_tests();

    // 8. Launch GUI Desktop if VBE graphics adapter is present
    if drivers::vbe::is_available() {
        kinfo!("Bochs VBE display adapter detected. Launching Cosmos Desktop GUI...");
        kinfo!("(Press Escape or click 'Exit to Shell' to switch to text shell)");
        gui::launch_desktop();
    }

    // 9. Interactive Cosmos Shell fallback / continuation
    kinfo!("Launching Cosmos Interactive Shell (CosmosSH)...");
    shell::run_shell();
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    arch::x86_64::registers::disable_interrupts();
    kerror!("==================== KERNEL PANIC ====================");
    if let Some(location) = info.location() {
        kerror!("Panicked at {}:{}:{}", location.file(), location.line(), location.column());
    }
    kerror!("{}", info.message());
    kerror!("System halted.");
    kerror!("======================================================");

    loop {
        arch::x86_64::registers::hlt();
    }
}
