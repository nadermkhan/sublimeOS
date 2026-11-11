extern crate alloc;
use alloc::format;
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
        "sw_vers" => cmd_sw_vers(),
        "system_profiler" => cmd_system_profiler(),
        "sysctl" => cmd_sysctl(args),
        "diskutil" => cmd_diskutil(args),
        "open" => cmd_open(args),
        "say" => cmd_say(args),
        "defaults" => cmd_defaults(args),
        "whoami" => println!("cosmos"),
        "hostname" => println!("CosmicOS.local"),
        "top" => cmd_top(),
        "help" => cmd_help(),
        "clear" => cmd_clear(),
        "uname" => cmd_uname(),
        "uptime" => cmd_uptime(),
        "date" => cmd_date(),
        "mem" => cmd_mem(),
        "ps" => cmd_ps(),
        "lspci" => cmd_lspci(),
        "ls" => cmd_ls(args),
        "cat" => cmd_cat(args),
        "echo" => cmd_echo(args),
        "mkdir" => cmd_mkdir(args),
        "touch" => cmd_touch(args),
        "rm" => cmd_rm(args),
        "gui" => cmd_gui(),
        "test" => crate::tests::run_all_tests(),
        "reboot" => cmd_reboot(),
        "halt" => cmd_halt(),
        unknown => {
            println!("zsh: command not found: {}. Type 'help' for available CosmicOS commands.", unknown);
        }
    }
}

fn cmd_sw_vers() {
    println!("ProductName:\t\tCosmicOS");
    println!("ProductVersion:\t\t1.0");
    println!("BuildVersion:\t\t1A01");
    println!("KernelVersion:\t\tSublime Kernel 1.0.0");
}

fn cmd_system_profiler() {
    println!("\x1b[1mHardware:\x1b[0m\n");
    println!("    \x1b[1mHardware Overview:\x1b[0m\n");
    println!("      Model Name:            CosmicStation Pro");
    println!("      Model Identifier:      CosmicStation1,1");
    println!("      Model Number:          CS-2026");
    println!("      Chip:                  Sublime Multi-Core Architecture (AMD64 / x86_64)");
    println!("      Total Number of Cores: 1 Performance");
    println!("      Memory:                128 MB Unified RAM");
    println!("      System Firmware:       1.0.0");
    println!("      OS Loader:             Sublime Bootloader");
    println!("      Serial Number (system):CS1000SUBX64");
    println!("      Hardware UUID:         4C4C4544-0051-5010-8043-B5C04F345233\n");
}

fn cmd_sysctl(args: &[&str]) {
    let key = args.first().copied().unwrap_or("-a");
    match key {
        "hw.model" => println!("hw.model: CosmicStation1,1"),
        "hw.ncpu" => println!("hw.ncpu: 1"),
        "hw.memsize" => {
            let (total_frames, _, _) = crate::mm::pmm::PMM.lock().stats();
            println!("hw.memsize: {}", total_frames * 4096);
        }
        "hw.pagesize" => println!("hw.pagesize: 4096"),
        "kern.ostype" => println!("kern.ostype: CosmicOS"),
        "kern.osrelease" => println!("kern.osrelease: 1.0.0"),
        "kern.version" => println!("kern.version: Sublime Kernel Version 1.0.0: sublime-release/RELEASE_X86_64"),
        "-a" | "all" => {
            println!("kern.ostype = CosmicOS");
            println!("kern.osrelease = 1.0.0");
            println!("kern.osrevision = 199506");
            println!("kern.version = Sublime Kernel Version 1.0.0: root:sublime/RELEASE_X86_64");
            println!("hw.model = CosmicStation1,1");
            println!("hw.ncpu = 1");
            println!("hw.byteorder = 1234");
            println!("hw.pagesize = 4096");
            let (total_frames, _, _) = crate::mm::pmm::PMM.lock().stats();
            println!("hw.memsize = {}", total_frames * 4096);
        }
        _ => println!("sysctl: unknown oid '{}'", key),
    }
}

fn cmd_diskutil(args: &[&str]) {
    let sub = args.first().copied().unwrap_or("list");
    if sub == "list" {
        println!("/dev/disk0 (internal, physical):");
        println!("   #:                       TYPE NAME                    SIZE       IDENTIFIER");
        println!("   0:      GUID_partition_scheme                        *128.0 MB   disk0");
        println!("   1:                        EFI EFI                     100.0 MB   disk0s1");
        println!("   2:                  Cosmic_FS Container disk1         *28.0 MB   disk0s2\n");
        println!("/dev/disk1 (synthesized):");
        println!("   #:                       TYPE NAME                    SIZE       IDENTIFIER");
        println!("   0:    CosmicFS Container Scheme -                      +28.0 MB    disk1");
        println!("                                 Physical Store disk0s2");
        println!("   1:              Cosmic Volume Cosmic HD               12.4 MB    disk1s1");
        println!("   2:              Cosmic Volume Preboot                 1.2 MB     disk1s2");
    } else {
        println!("Usage: diskutil list");
    }
}

fn cmd_open(args: &[&str]) {
    if args.is_empty() {
        println!("The file does not exist.");
        return;
    }
    let target = args[0];
    if target.contains("Finder.app") || target == "gui" || target == "." {
        println!("\x1b[35m[Cosmic LaunchServices] Launching Finder.app...\x1b[0m");
        crate::gui::launch_desktop();
    } else {
        println!("[Cosmic LaunchServices] Opening '{}'...", target);
        cmd_cat(args);
    }
}

fn cmd_say(args: &[&str]) {
    let text = if args.is_empty() { "Hello, I am CosmicOS." } else {
        // print all args
        &args.join(" ")
    };
    println!("\x1b[35m[SpeechSynthesis (Cosmic Voice)]\x1b[0m \"{}\"", text);
}

fn cmd_defaults(args: &[&str]) {
    let action = args.first().copied().unwrap_or("read");
    if action == "read" {
        println!("{{\n    CosmicInterfaceStyle = Dark;\n    CosmicFontSmoothing = 2;\n    CosmicKeyboardLayout = US;\n    CosmicMetricUnits = 1;\n    CosmicTemperatureUnit = Celsius;\n}}");
    } else {
        println!("Usage: defaults read");
    }
}

fn cmd_top() {
    let (_total_frames, used_frames, free_frames) = crate::mm::pmm::PMM.lock().stats();
    let uptime_secs = crate::arch::x86_64::pit::get_ticks() / 1000;
    println!("Processes: 3 total, 1 running, 2 sleeping, 3 threads");
    println!("Load Avg: 0.12, 0.08, 0.02  CPU usage: 1.2% user, 2.4% sys, 96.4% idle");
    println!("SharedLibs: numLib=12, resident=1.4M");
    println!("MemRegions: num=48, resident=12M, private=2M");
    println!(
        "PhysMem: {}M used (12M wired), {}M unused.",
        (used_frames * 4096) / (1024 * 1024),
        (free_frames * 4096) / (1024 * 1024)
    );
    println!("Uptime: {} seconds\n", uptime_secs);
    cmd_ps();
}

fn cmd_help() {
    println!("\x1b[36mCosmicOS (Sublime Kernel) Shell Commands:\x1b[0m");
    println!("  sw_vers         Display CosmicOS product and build version");
    println!("  system_profiler Display hardware system overview");
    println!("  sysctl [-a]     Get kernel and hardware state (hw.model, hw.memsize)");
    println!("  diskutil [list] Manage CosmicFS containers and disk volumes");
    println!("  open <app/path> Open application or document (e.g. 'open Finder.app')");
    println!("  say <text>      Speech synthesis simulated audio banner");
    println!("  defaults [read] Access CosmicOS user preferences and defaults");
    println!("  top             Display live Sublime tasks & memory stats");
    println!("  ps              List active Sublime tasks and kernel threads");
    println!("  uname           Print Sublime kernel version info");
    println!("  uptime          Show system uptime in seconds");
    println!("  date            Current UTC hardware clock timestamp");
    println!("  mem             Memory usage (Physical Unified RAM & Heap)");
    println!("  lspci           Enumerate PCI Express controller devices");
    println!("  ls [path]       List contents of a directory (default '/')");
    println!("  cat <path>      Read file content");
    println!("  mkdir <path>    Create directory");
    println!("  touch <path>    Create empty file");
    println!("  rm <path>       Remove file");
    println!("  clear           Clear the terminal screen");
    println!("  reboot          Restart computer");
    println!("  halt            Shut down computer");
}

fn cmd_gui() {
    println!("\x1b[35m[LaunchServices] Starting CosmicOS Aqua Desktop...\x1b[0m");
    crate::gui::launch_desktop();
}

fn cmd_clear() {
    crate::arch::x86_64::vga::VGA_WRITER.lock().clear_screen();
    print!("\x1b[2J\x1b[H");
}

fn cmd_uname() {
    println!("CosmicOS CosmicOS.local 1.0.0 Sublime Kernel Version 1.0.0 x86_64");
}

fn cmd_uptime() {
    let ticks = crate::arch::x86_64::pit::get_ticks();
    let seconds = ticks / 1000;
    println!("up {} seconds ({} ticks at 1000 Hz)", seconds, ticks);
}

fn cmd_date() {
    let dt = crate::arch::x86_64::rtc::read_datetime();
    println!("{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC", dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second);
}

fn cmd_mem() {
    let (total_frames, used_frames, free_frames) = crate::mm::pmm::PMM.lock().stats();
    let (heap_total, heap_allocated, heap_free) = crate::mm::heap::HEAP_ALLOCATOR.stats();

    println!("Unified Memory:    {} MB Total", (total_frames * 4096) / (1024 * 1024));
    println!("Wired Memory:      {} KB Used", (used_frames * 4096) / 1024);
    println!("Free Memory:       {} KB Free", (free_frames * 4096) / 1024);
    println!("XNU Kernel Heap:   {} KB Total ({} KB Allocated, {} KB Free)", heap_total / 1024, heap_allocated / 1024, heap_free / 1024);
}

fn cmd_ps() {
    println!("  PID TTY           TIME CMD");
    println!("    0 ttys000    0:00.02 kernel_idle");
    println!("    1 ttys000    0:00.08 launchd");
    println!("    2 ttys000    0:00.05 zsh");
}

fn cmd_lspci() {
    let pci = crate::drivers::pci::PCI.lock();
    println!("Found {} PCI device(s):", pci.devices.len());
    for dev in pci.devices.iter() {
        println!(
            "{:02x}:{:02x}.{:x} {} [Class {:02x}{:02x}] (Vendor: {:04x}, Device: {:04x}, BAR0: 0x{:08x})",
            dev.bus, dev.device, dev.function, dev.class_name(), dev.class_code, dev.subclass, dev.vendor_id, dev.device_id, dev.bar0
        );
    }
}

fn cmd_ls(args: &[&str]) {
    let path = args.first().copied().unwrap_or("/");
    let vfs = VFS.lock();
    match vfs.resolve_path(path).and_then(|node| node.lock().readdir()) {
        Ok(entries) => {
            for entry in entries {
                let suffix = if entry.file_type == FileType::Directory { "/" } else { "" };
                print!("{}  ", format!("{}{}", entry.name, suffix));
            }
            println!();
        }
        Err(e) => println!("ls: {}: {}", path, e),
    }
}

fn cmd_cat(args: &[&str]) {
    if args.is_empty() {
        println!("usage: cat <file>");
        return;
    }
    let path = args[0];
    let vfs = VFS.lock();
    match vfs.open(path, 0) {
        Ok(file) => {
            let mut buf = [0u8; 1024];
            loop {
                match file.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        if let Ok(s) = core::str::from_utf8(&buf[..n]) {
                            print!("{}", s);
                        }
                    }
                    Err(e) => {
                        println!("cat: error: {}", e);
                        break;
                    }
                }
            }
        }
        Err(e) => println!("cat: {}: {}", path, e),
    }
}

fn cmd_echo(args: &[&str]) {
    println!("{}", args.join(" "));
}

fn cmd_mkdir(args: &[&str]) {
    if args.is_empty() {
        println!("usage: mkdir <directory>");
        return;
    }
    let path = args[0];
    let mut parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if parts.is_empty() {
        return;
    }
    let new_dir_name = parts.pop().unwrap();
    let parent_path = if parts.is_empty() { "/" } else { &path[..path.rfind('/').unwrap_or(0)] };

    let vfs = VFS.lock();
    match vfs.resolve_path(parent_path) {
        Ok(parent_node) => {
            if let Err(e) = parent_node.lock().create(new_dir_name, FileType::Directory) {
                println!("mkdir: {}: {}", path, e);
            }
        }
        Err(e) => println!("mkdir: {}: {}", parent_path, e),
    }
}

fn cmd_touch(args: &[&str]) {
    if args.is_empty() {
        println!("usage: touch <file>");
        return;
    }
    let path = args[0];
    let mut parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if parts.is_empty() {
        return;
    }
    let new_file_name = parts.pop().unwrap();
    let parent_path = if parts.is_empty() { "/" } else { &path[..path.rfind('/').unwrap_or(0)] };

    let vfs = VFS.lock();
    match vfs.resolve_path(parent_path) {
        Ok(parent_node) => {
            if let Err(e) = parent_node.lock().create(new_file_name, FileType::Regular) {
                println!("touch: {}: {}", path, e);
            }
        }
        Err(e) => println!("touch: {}: {}", parent_path, e),
    }
}

fn cmd_rm(args: &[&str]) {
    if args.is_empty() {
        println!("usage: rm <file>");
        return;
    }
    let path = args[0];
    let mut parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if parts.is_empty() {
        return;
    }
    let file_name = parts.pop().unwrap();
    let parent_path = if parts.is_empty() { "/" } else { &path[..path.rfind('/').unwrap_or(0)] };

    let vfs = VFS.lock();
    match vfs.resolve_path(parent_path) {
        Ok(parent_node) => {
            if let Err(e) = parent_node.lock().unlink(file_name) {
                println!("rm: {}: {}", path, e);
            }
        }
        Err(e) => println!("rm: {}: {}", parent_path, e),
    }
}

fn cmd_reboot() {
    println!("\x1b[31mRestarting CosmicOS...\x1b[0m");
    unsafe {
        crate::arch::x86_64::io::outb(0x64, 0xFE);
    }
}

fn cmd_halt() {
    println!("\x1b[31mShutting down CosmicOS...\x1b[0m");
    crate::arch::x86_64::registers::disable_interrupts();
    loop {
        crate::arch::x86_64::registers::hlt();
    }
}
