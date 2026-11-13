#![no_std]
#![no_main]

extern crate alloc;

use core::arch::global_asm;
use core::panic::PanicInfo;

global_asm!(include_str!("arch/x86_64/boot.s"));
global_asm!(include_str!("arch/x86_64/ap_trampoline.s"));
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
pub mod user;

#[no_mangle]
pub extern "C" fn kernel_main(magic: u64, mb_info: u64) -> ! {
    // 1. Initialize Architecture & Hardware Abstraction Layer
    arch::x86_64::init();
    arch::x86_64::multiboot::parse_boot_info(magic, mb_info);

    kinfo!("=======================================================");
    kinfo!("  SublimeOS — KiteKernel Version 1.0.0 (x86_64)");
    kinfo!("  Copyright (c) 2026 Nader Mahbub Khan. All rights reserved.");
    kinfo!("  Kite Monolithic Core | Boot Magic: 0x{:08X}", magic);
    kinfo!("=======================================================");

    // 2. Initialize Memory Management Subsystem
    kinfo!("Initializing Memory Management Subsystem (PMM, VMM, Heap)...");
    mm::pmm::init(mb_info);
    mm::vmm::VMM.lock().init();
    mm::heap::HEAP_ALLOCATOR.init();
    let _ = drivers::vbe::init_early_gop();

    // 2.1. Initialize Local APIC, I/O APIC, and SMP Multiprocessing Subsystem
    arch::x86_64::init_apic_smp();

    // 3. Initialize Preemptive Multitasking Scheduler
    task::scheduler::init();

    // 4. Initialize Virtual File System (VFS, RamFS, DevFS, ProcFS)
    fs::init();

    // 5. Initialize Device Drivers (Keyboard, Mouse, PCI, Storage, USB)
    drivers::init();
    fs::mount_storage_devices();
    user::init_userland();

    // 6. Initialize Master Network Subsystem (Ethernet, Wi-Fi 802.11, Loopback)
    net::init();

    // 7. Initialize Master TTY Subsystem & Virtual Consoles
    tty::init();

    // 7. Enable CPU interrupts now that all subsystems and scheduler are active
    arch::x86_64::registers::enable_interrupts();
    kinfo!("CPU interrupts enabled");
    arch::x86_64::rtc::init();

    // 8. Run Kernel Self-Test Suite to verify all subsystems
    arch::x86_64::pit::IN_KERNEL_TESTS.store(true, core::sync::atomic::Ordering::SeqCst);
    tests::run_all_tests();
    arch::x86_64::pit::IN_KERNEL_TESTS.store(false, core::sync::atomic::Ordering::SeqCst);

    // Ensure stdout capture is off and hardware input buffers are unlatched before PID 1 starts
    let _ = arch::x86_64::vga::stop_stdout_capture();
    let _ = drivers::keyboard::poll_ps2_controller();
    drivers::usb::poll_hid_nonblocking();

    // Clear test output for quiet boot so userland starts with clean screen & SublimeOS logo
    if let Some(mut con) = drivers::gop_console::GOP_CONSOLE.try_lock() {
        con.clear_screen();
    }

    // 9. Launch SublimeOS Userland Master Init (PID 1) in Ring 3
    kinfo!("Launching SublimeOS Userland Master Init (PID 1) in Ring 3...");
    user::launch_init();
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    arch::x86_64::registers::disable_interrupts();
    arch::x86_64::vga::PANICKED.store(true, core::sync::atomic::Ordering::SeqCst);
    let _ = arch::x86_64::vga::stop_stdout_capture();
    drivers::gop_console::enter_panic_mode();

    // Read CPU register context
    let rsp: u64;
    let rbp: u64;
    let rflags: u64;
    unsafe {
        core::arch::asm!("mov {}, rsp", out(reg) rsp);
        core::arch::asm!("mov {}, rbp", out(reg) rbp);
        core::arch::asm!("pushfq; pop {}", out(reg) rflags);
    }
    let cr2 = unsafe { arch::x86_64::registers::read_cr2() };
    let cr3 = unsafe { arch::x86_64::registers::read_cr3() };
    let apic_id = arch::x86_64::apic::get_apic_id();

    kerror!("================================================================================");
    kerror!("                   SUBLIMEOS KERNEL PANIC (KiteKernel)                          ");
    kerror!("                   Copyright (c) 2026 Nader Mahbub Khan                         ");
    kerror!("================================================================================");
    if let Some(location) = info.location() {
        kerror!("  [CRASH LOCATION] {}:{}:{}", location.file(), location.line(), location.column());
    }
    kerror!("  [CRASH REASON]   {}", info.message());
    kerror!("  [CPU CONTEXT]    Core APIC ID: {} | RFLAGS: 0x{:08X}", apic_id, rflags);
    kerror!("  [REGISTERS]      RSP: 0x{:016X} | RBP: 0x{:016X}", rsp, rbp);
    kerror!("                   CR2 (Fault Addr): 0x{:016X} | CR3: 0x{:016X}", cr2, cr3);

    let file_str = info.location().map(|l| l.file()).unwrap_or("unknown");
    let line_num = info.location().map(|l| l.line()).unwrap_or(0);
    let col_num = info.location().map(|l| l.column()).unwrap_or(0);

    // Write crash dump to pre-allocated boot media if target is registered
    match drivers::crashdump::write_panic_dump(
        &info.message(),
        file_str,
        line_num,
        col_num,
        apic_id,
        rflags,
        rsp,
        rbp,
        cr2,
        cr3,
    ) {
        Ok(bytes) => {
            kerror!("  [STORAGE DUMP]   Crash log saved to persistent /PANIC.LOG ({} bytes).", bytes);
        }
        Err(err) => {
            kerror!("  [STORAGE DUMP]   Storage log omitted: {}.", err);
        }
    }

    kerror!("================================================================================");
    kerror!("  RECOVERY CONTROLS:");
    kerror!("    Press [R] -> Immediate Hardware Reset / Reboot");
    kerror!("    Press [S] -> Safe Power Off (ACPI / SMC Shutdown)");
    kerror!("    Press [D] -> Re-flush Crash Dump to /PANIC.LOG");
    kerror!("    Press [H] -> Halt CPU");
    kerror!("================================================================================");

    loop {
        // 1. Check if hardware power button was tapped on Apple Mac / PC
        if arch::x86_64::acpi::is_power_button_pressed() {
            kerror!("Power button pressed. Shutting down system...");
            arch::x86_64::acpi::poweroff();
        }

        // 2. Poll USB host controllers (essential for Apple MacBook Pro internal keyboard & USB peripherals)
        drivers::usb::poll_hid_panic();

        // 3. Check unified keyboard buffer (populated by USB HID reports or interrupt handlers)
        let mut key_action = None;
        if let Some(ch) = drivers::keyboard::read_char_panic() {
            match ch.to_ascii_uppercase() {
                b'R' => key_action = Some('R'),
                b'S' => key_action = Some('S'),
                b'D' => key_action = Some('D'),
                b'H' => key_action = Some('H'),
                _ => {}
            }
        }

        // 4. Poll legacy PS/2 8042 keyboard ports 0x64 & 0x60 (for traditional PCs & QEMU)
        if key_action.is_none() {
            let status = unsafe { arch::x86_64::io::inb(0x64) };
            if (status & 0x01) != 0 && status != 0xFF {
                let scancode = unsafe { arch::x86_64::io::inb(0x60) };
                if (scancode & 0x80) == 0 && scancode != 0xFF && scancode != 0x00 {
                    match scancode {
                        0x13 => key_action = Some('R'), // 'R' key make code
                        0x1F => key_action = Some('S'), // 'S' key make code
                        0x20 => key_action = Some('D'), // 'D' key make code
                        0x23 => key_action = Some('H'), // 'H' key make code
                        _ => {}
                    }
                }
            }
        }

        // 5. Poll Serial COM1 UART 0x3F8 (for remote serial consoles & headless test rigs)
        if key_action.is_none() {
            let lsr = unsafe { arch::x86_64::io::inb(0x3FD) };
            if (lsr & 0x01) != 0 && lsr != 0xFF {
                let ser_byte = unsafe { arch::x86_64::io::inb(0x3F8) };
                match ser_byte.to_ascii_uppercase() {
                    b'R' => key_action = Some('R'),
                    b'S' => key_action = Some('S'),
                    b'D' => key_action = Some('D'),
                    b'H' => key_action = Some('H'),
                    _ => {}
                }
            }
        }

        // Execute triggered recovery control
        if let Some(action) = key_action {
            match action {
                'R' => {
                    kerror!("Initiating immediate hardware reset / reboot...");
                    for _ in 0..500_000 {
                        core::hint::spin_loop();
                    }
                    arch::x86_64::acpi::reboot();
                }
                'S' => {
                    kerror!("Shutting down hardware cleanly (ACPI / SMC)...");
                    for _ in 0..500_000 {
                        core::hint::spin_loop();
                    }
                    arch::x86_64::acpi::poweroff();
                }
                'D' => {
                    kerror!("Flushing crash dump to storage...");
                    let _ = drivers::crashdump::write_panic_dump(
                        &info.message(),
                        file_str,
                        line_num,
                        col_num,
                        apic_id,
                        rflags,
                        rsp,
                        rbp,
                        cr2,
                        cr3,
                    );
                }
                'H' => {
                    kerror!("CPU halted. You may safely turn off the computer.");
                    loop {
                        arch::x86_64::registers::hlt();
                    }
                }
                _ => {}
            }
        }

        core::hint::spin_loop();
    }
}
