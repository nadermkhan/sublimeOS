#![no_std]
#![no_main]

extern crate alloc;

use core::arch::global_asm;
use core::panic::PanicInfo;

global_asm!(include_str!("arch/x86_64/boot.s"));
global_asm!(include_str!("arch/x86_64/interrupts.s"));
global_asm!(include_str!("task/switch.s"));
global_asm!(include_str!("task/user_switch.s"));
global_asm!(include_str!("syscall/syscall_entry.s"));

pub mod arch;
pub mod drivers;
pub mod fs;
pub mod gui;
pub mod ipc;
pub mod mm;
pub mod net;
pub mod shell;
pub mod sync;
pub mod syscall;
pub mod task;
pub mod tests;
pub mod tty;

#[no_mangle]
pub extern "C" fn kernel_main(magic: u64, mb_info: u64) -> ! {
    // 1. Initialize Architecture & Hardware Abstraction Layer
    arch::x86_64::init();
    arch::x86_64::multiboot::parse_boot_info(magic, mb_info);

    kinfo!("=======================================================");
    kinfo!("  CosmicOS — Sublime Kernel Version 1.0.0 (x86_64)");
    kinfo!("  Sublime Monolithic Core | Boot Magic: 0x{:08X}", magic);
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

    // 5. Initialize Device Drivers (Keyboard, Mouse, PCI, Storage, USB)
    drivers::init();
    fs::mount_storage_devices();

    // 6. Initialize Master Network Subsystem (Ethernet, Wi-Fi 802.11, Loopback)
    net::init();

    // 7. Initialize Master TTY Subsystem & Virtual Consoles
    tty::init();

    // 7. Enable CPU interrupts now that all subsystems and scheduler are active
    arch::x86_64::registers::enable_interrupts();
    kinfo!("CPU interrupts enabled");

    // 8. Run Kernel Self-Test Suite to verify all subsystems
    tests::run_all_tests();

    // 9. Launch the CosmicOS Interactive TTY Shell
    kinfo!("Launching CosmicOS Interactive TTY Shell (CosmicSH)...");
    shell::run_shell();
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    arch::x86_64::registers::disable_interrupts();
    let _ = arch::x86_64::vga::stop_stdout_capture();
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
