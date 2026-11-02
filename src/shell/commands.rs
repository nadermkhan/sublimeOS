extern crate alloc;
use alloc::vec::Vec;
use crate::fs::vfs::{FileType, VFS};
use crate::print;
use crate::println;

pub fn execute_command(line: &str) {
    let parts: Vec<&str> = line.trim().split_whitespace().collect();
    if parts.is_empty() {
        return;
    }

    let cmd = parts[0];
    let args = &parts[1..];

    match cmd {
        "help" => cmd_help(),
        "clear" => cmd_clear(),
        "uname" => cmd_uname(),
        "uptime" => cmd_uptime(),
        "date" => cmd_date(),
        "mem" => cmd_mem(),
        "ps" | "tasks" => cmd_tasks(),
        "lspci" => cmd_lspci(),
        "ls" => cmd_ls(args),
        "cat" => cmd_cat(args),
        "echo" => cmd_echo(args),
        "mkdir" => cmd_mkdir(args),
        "touch" => cmd_touch(args),
        "rm" => cmd_rm(args),
        "test" => crate::tests::run_all_tests(),
        "reboot" => cmd_reboot(),
        "halt" => cmd_halt(),
        unknown => {
            println!("cosmos: command not found: {}. Type 'help' for available commands.", unknown);
        }
    }
}

fn cmd_help() {
    println!("\x1b[36mCosmosKernel Built-in Commands:\x1b[0m");
    println!("  help            Show this help reference");
    println!("  clear           Clear the VGA and terminal screen");
    println!("  uname           Print kernel and operating system info");
    println!("  uptime          Display kernel uptime and timer ticks");
    println!("  date            Display current UTC date/time from CMOS RTC");
    println!("  mem             Display physical frame allocator and heap status");
    println!("  ps / tasks      List all active tasks and kernel threads");
    println!("  lspci           Enumerate PCI devices and controllers");
    println!("  ls [path]       List contents of a directory (default '/')");
    println!("  cat <path>      Display file contents (try /proc/version or /etc/motd)");
    println!("  echo <text>     Print text to console");
    println!("  mkdir <path>    Create a directory in the VFS");
    println!("  touch <path>    Create an empty file in the VFS");
    println!("  rm <path>       Remove a file from the VFS");
    println!("  test            Execute kernel verification self-test suite");
    println!("  reboot          Reboot the system");
    println!("  halt            Power off / halt processor");
}

fn cmd_clear() {
    crate::arch::x86_64::vga::VGA_WRITER.lock().clear_screen();
    print!("\x1b[2J\x1b[H"); // ANSI clear screen escape code for serial
}

fn cmd_uname() {
    println!("CosmosKernel 0.1.0-monolithic x86_64 GNU/Cosmos #1 SMP PREEMPT 2026");
}

fn cmd_uptime() {
    let ticks = crate::arch::x86_64::pit::get_ticks();
    let total_secs = ticks / 1000;
    let mins = total_secs / 60;
    let secs = total_secs % 60;
    let ms = ticks % 1000;
    println!("Uptime: {}m {}.{:03}s ({} timer ticks)", mins, secs, ms, ticks);
}

fn cmd_date() {
    let dt = crate::arch::x86_64::rtc::read_datetime();
    println!("{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC", dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second);
}

fn cmd_mem() {
    let (total_f, used_f, free_f) = crate::mm::pmm::PMM.lock().stats();
    let (heap_tot, heap_used, heap_free) = crate::mm::heap::HEAP_ALLOCATOR.stats();

    println!("\x1b[33m--- Memory Statistics ---\x1b[0m");
    println!("Physical RAM:  {} MB Total ({} frames)", (total_f * 4096) / (1024 * 1024), total_f);
    println!("RAM Used:      {} MB ({} frames)", (used_f * 4096) / (1024 * 1024), used_f);
    println!("RAM Free:      {} MB ({} frames)", (free_f * 4096) / (1024 * 1024), free_f);
    println!("Kernel Heap:   {} KB Total, {} KB Used, {} KB Free", heap_tot / 1024, heap_used / 1024, heap_free / 1024);
}

fn cmd_tasks() {
    let tasks = crate::task::scheduler::get_task_list();
    println!("{:<5} {:<15} {:<10} {:<10}", "TID", "NAME", "STATE", "TICKS");
    println!("-------------------------------------------");
    for (tid, name, state, ticks) in tasks {
        let state_str = match state {
            crate::task::thread::ThreadState::Running => "Running",
            crate::task::thread::ThreadState::Ready => "Ready",
            crate::task::thread::ThreadState::Sleeping(_) => "Sleeping",
            crate::task::thread::ThreadState::Blocked => "Blocked",
            crate::task::thread::ThreadState::Dead => "Dead",
        };
        println!("{:<5} {:<15} {:<10} {:<10}", tid, name, state_str, ticks);
    }
}

fn cmd_lspci() {
    let pci = crate::drivers::pci::PCI.lock();
    println!("Found {} PCI device(s):", pci.devices.len());
    for dev in pci.devices.iter() {
        println!(
            "{:02x}:{:02x}.{:x} {} [Class {:02x}{:02x}] (Vendor: {:04x}, Device: {:04x})",
            dev.bus, dev.device, dev.function, dev.class_name(), dev.class_code, dev.subclass, dev.vendor_id, dev.device_id
        );
    }
}

fn cmd_ls(args: &[&str]) {
    let path = if args.is_empty() { "/" } else { args[0] };
    let vfs = VFS.lock();

    match vfs.resolve_path(path) {
        Ok(inode) => {
            match inode.lock().readdir() {
                Ok(entries) => {
                    for entry in entries {
                        let type_char = match entry.file_type {
                            FileType::Directory => "d",
                            FileType::Regular => "-",
                            FileType::CharDevice => "c",
                            FileType::BlockDevice => "b",
                            FileType::Pipe => "p",
                        };
                        println!("{} {:<16} (inode {})", type_char, entry.name, entry.inode);
                    }
                }
                Err(e) => println!("ls: cannot list {}: {}", path, e),
            }
        }
        Err(e) => println!("ls: cannot access {}: {}", path, e),
    }
}

fn cmd_cat(args: &[&str]) {
    if args.is_empty() {
        println!("Usage: cat <file>");
        return;
    }

    let path = args[0];
    let vfs = VFS.lock();

    match vfs.open(path, 0) {
        Ok(file) => {
            let mut buf = [0u8; 512];
            loop {
                match file.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if let Ok(s) = core::str::from_utf8(&buf[..n]) {
                            print!("{}", s);
                        } else {
                            for &b in &buf[..n] {
                                print!("{}", b as char);
                            }
                        }
                    }
                    Err(e) => {
                        println!("cat: error reading {}: {}", path, e);
                        break;
                    }
                }
            }
        }
        Err(e) => println!("cat: {}: {}", path, e),
    }
}

fn cmd_echo(args: &[&str]) {
    for (i, arg) in args.iter().enumerate() {
        if i > 0 {
            print!(" ");
        }
        print!("{}", arg);
    }
    println!();
}

fn cmd_mkdir(args: &[&str]) {
    if args.is_empty() {
        println!("Usage: mkdir <path>");
        return;
    }

    let path = args[0];
    let vfs = VFS.lock();

    let (parent_path, name) = if let Some(idx) = path.rfind('/') {
        let parent = if idx == 0 { "/" } else { &path[..idx] };
        (parent, &path[idx + 1..])
    } else {
        ("/", path)
    };

    match vfs.resolve_path(parent_path) {
        Ok(parent_node) => {
            if let Err(e) = parent_node.lock().create(name, FileType::Directory) {
                println!("mkdir: failed to create directory {}: {}", path, e);
            }
        }
        Err(e) => println!("mkdir: cannot access {}: {}", parent_path, e),
    }
}

fn cmd_touch(args: &[&str]) {
    if args.is_empty() {
        println!("Usage: touch <path>");
        return;
    }

    let path = args[0];
    let vfs = VFS.lock();

    let (parent_path, name) = if let Some(idx) = path.rfind('/') {
        let parent = if idx == 0 { "/" } else { &path[..idx] };
        (parent, &path[idx + 1..])
    } else {
        ("/", path)
    };

    match vfs.resolve_path(parent_path) {
        Ok(parent_node) => {
            if let Err(e) = parent_node.lock().create(name, FileType::Regular) {
                println!("touch: failed to create file {}: {}", path, e);
            }
        }
        Err(e) => println!("touch: cannot access {}: {}", parent_path, e),
    }
}

fn cmd_rm(args: &[&str]) {
    if args.is_empty() {
        println!("Usage: rm <path>");
        return;
    }

    let path = args[0];
    let vfs = VFS.lock();

    let (parent_path, name) = if let Some(idx) = path.rfind('/') {
        let parent = if idx == 0 { "/" } else { &path[..idx] };
        (parent, &path[idx + 1..])
    } else {
        ("/", path)
    };

    match vfs.resolve_path(parent_path) {
        Ok(parent_node) => {
            if let Err(e) = parent_node.lock().unlink(name) {
                println!("rm: cannot remove {}: {}", path, e);
            }
        }
        Err(e) => println!("rm: cannot access {}: {}", parent_path, e),
    }
}

fn cmd_reboot() {
    println!("Rebooting system...");
    unsafe {
        // Pulse 8042 reset line
        let mut good = 0x02u8;
        while (good & 0x02) != 0 {
            good = crate::arch::x86_64::io::inb(0x64);
        }
        crate::arch::x86_64::io::outb(0x64, 0xFE);
    }
    loop {
        crate::arch::x86_64::registers::hlt();
    }
}

fn cmd_halt() {
    println!("System halted. You may safely turn off the computer.");
    crate::arch::x86_64::registers::disable_interrupts();
    loop {
        crate::arch::x86_64::registers::hlt();
    }
}
