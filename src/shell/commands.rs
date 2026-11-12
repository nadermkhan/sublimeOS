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
        // Modern Core Domain Commands
        "system" | "sys" => cmd_system(args),
        "hardware" | "specs" => cmd_system_profiler(),
        "about" => cmd_sw_vers(),
        "tasks" | "process" | "proc" => cmd_tasks(args),
        "storage" | "disks" | "disk" => cmd_storage(args),
        "power" => {
            let sub = args.first().copied().unwrap_or("status");
            match sub {
                "off" | "shutdown" | "halt" => cmd_halt(),
                "restart" | "reboot" | "reset" => cmd_reboot(),
                _ => println!("Usage: power [off | restart]"),
            }
        }
        "sync" => {
            let _ = crate::fs::bcache::BLOCK_CACHE.lock().flush_all();
            println!("Storage cache synced.");
        },
        "network" | "net" => cmd_network(args),
        "audio" | "sound" => cmd_audio(args),
        "display" | "screen" => cmd_font(args),
        "path" | "where" => println!("/"),

        // Modern File Operations
        "list" | "files" => cmd_ls(args),
        "view" | "read" => cmd_cat(args),
        "print" => cmd_echo(args),
        "edit" => cmd_nano(args),
        "create" => cmd_touch(args),
        "delete" | "remove" => cmd_rm(args),
        "folder" => {
            let sub = args.first().copied().unwrap_or("");
            if sub == "create" || sub == "new" || sub == "make" {
                cmd_mkdir(&args[1..]);
            } else {
                println!("Usage: folder create <path>");
            }
        }
        "make" => {
            if args.len() >= 2 && (args[0] == "folder" || args[0] == "dir" || args[0] == "directory") {
                cmd_mkdir(&args[1..]);
            } else {
                println!("Usage: make folder <path>");
            }
        }

        // Modern Stream Filters
        "find" => cmd_grep(args),
        "count" => cmd_wc(args),
        "first" => cmd_head(args),
        "last" => cmd_tail(args),
        "unique" | "distinct" => cmd_uniq(args),

        // Desktop
        "desktop" => cmd_gui(),

        // Classic Utilities & Backward-Compatible Shortcuts
        "sw_vers" => cmd_sw_vers(),
        "system_profiler" => cmd_system_profiler(),
        "sysctl" => cmd_sysctl(args),
        "diskutil" => cmd_diskutil(args),
        "open" => cmd_open(args),
        "say" => cmd_say(args),
        "beep" => cmd_beep(args),
        "chime" => cmd_chime(),
        "play" => cmd_play(args),
        "exec" => cmd_exec(args),
        "defaults" => cmd_defaults(args),
        "whoami" => println!("cosmos"),
        "hostname" => println!("sublimeos.local"),
        "top" => cmd_top(),
        "help" => cmd_help(),
        "clear" => cmd_clear(),
        "uname" => cmd_uname(),
        "uptime" => cmd_uptime(),
        "date" => cmd_date(),
        "mem" => cmd_mem(),
        "ps" => cmd_ps(),
        "jobs" => crate::shell::job::cmd_jobs(),
        "fg" => crate::shell::job::cmd_fg(args),
        "bg" => crate::shell::job::cmd_bg(args),
        "lspci" => cmd_lspci(),
        "lsusb" | "usb" => cmd_usb(),
        "ls" => cmd_ls(args),
        "cat" => cmd_cat(args),
        "echo" => cmd_echo(args),
        "grep" => cmd_grep(args),
        "wc" => cmd_wc(args),
        "head" => cmd_head(args),
        "tail" => cmd_tail(args),
        "sort" => cmd_sort(args),
        "uniq" => cmd_uniq(args),
        "nano" => cmd_nano(args),
        "mkdir" => cmd_mkdir(args),
        "touch" => cmd_touch(args),
        "rm" => cmd_rm(args),
        "mount" => cmd_mount(),
        "wifi" => cmd_wifi(args),
        "dhcp" => cmd_dhcp(args),
        "ntp" => cmd_ntp(args),
        "ifconfig" => cmd_ifconfig(),
        "ping" => cmd_ping(args),
        "nslookup" => cmd_nslookup(args),
        "curl" | "http" => cmd_curl(args),
        "httpd" => cmd_httpd(args),
        "telnet" => cmd_telnet(args),
        "telnetd" => cmd_telnetd(args),
        "sshd" => cmd_sshd(args),
        "gui" => cmd_gui(),
        "windowserver" => cmd_windowserver(),
        "test" => crate::tests::run_all_tests(),
        "reboot" | "restart" => cmd_reboot(),
        "shutdown" | "poweroff" => cmd_shutdown(args),
        "halt" => cmd_halt(),
        "font" => cmd_font(args),
        unknown => {
            println!("sublimesh: command not found: '{}'. Type 'help' for available SublimeOS commands.", unknown);
        }
    }
}

fn cmd_system(args: &[&str]) {
    let sub = args.first().copied().unwrap_or("info");
    match sub {
        "info" | "status" | "" => cmd_system_info(),
        "hardware" | "hw" | "profiler" => cmd_system_profiler(),
        "config" | "sysctl" => cmd_sysctl(&args[1..]),
        "version" | "vers" => cmd_sw_vers(),
        "uptime" => cmd_uptime(),
        "date" | "time" => cmd_date(),
        "memory" | "mem" => cmd_mem(),
        "restart" | "reboot" => cmd_reboot(),
        "shutdown" | "halt" | "poweroff" => cmd_halt(),
        "usb" | "devices" => cmd_usb(),
        _ => {
            println!("Usage: system [info | hardware | usb | config | version | memory | uptime | date | restart | shutdown]");
        }
    }
}

fn cmd_system_info() {
    let hw = crate::drivers::platform::smbios::get_hardware_telemetry();
    println!("\x1b[36m┌────────────────────────────────────────────────────────┐\x1b[0m");
    println!("\x1b[36m│\x1b[0m  \x1b[1;37mSublimeOS\x1b[0m — \x1b[35mKiteKernel 1.0.0 (x86_64)\x1b[0m            \x1b[36m│\x1b[0m");
    println!("\x1b[36m├────────────────────────────────────────────────────────┤\x1b[0m");
    println!("\x1b[36m│\x1b[0m  \x1b[33mModel:\x1b[0m     {:<42}\x1b[36m│\x1b[0m", hw.model_name);
    println!("\x1b[36m│\x1b[0m  \x1b[33mProcessor:\x1b[0m {:<42}\x1b[36m│\x1b[0m", hw.chip);
    println!("\x1b[36m│\x1b[0m  \x1b[33mCores:\x1b[0m     {:<42}\x1b[36m│\x1b[0m", hw.cores_threads);
    println!("\x1b[36m│\x1b[0m  \x1b[33mMemory:\x1b[0m    {:<42}\x1b[36m│\x1b[0m", hw.memory_summary);
    let ticks = crate::arch::x86_64::pit::get_ticks();
    println!("\x1b[36m│\x1b[0m  \x1b[33mUptime:\x1b[0m    {}s                                      \x1b[36m│\x1b[0m", ticks / 1000);
    let dt = crate::arch::x86_64::rtc::get_system_datetime();
    println!("\x1b[36m│\x1b[0m  \x1b[33mClock:\x1b[0m     {:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC              \x1b[36m│\x1b[0m", dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second);
    let (fb_w, fb_h) = crate::drivers::vbe::dimensions();
    println!("\x1b[36m│\x1b[0m  \x1b[33mDisplay:\x1b[0m   {}x{} (32bpp)                           \x1b[36m│\x1b[0m", fb_w, fb_h);
    println!("\x1b[36m└────────────────────────────────────────────────────────┘\x1b[0m");
}

fn cmd_tasks(args: &[&str]) {
    let sub = args.first().copied().unwrap_or("list");
    match sub {
        "list" | "ps" | "" => cmd_ps(),
        "top" | "monitor" => cmd_top(),
        "run" | "exec" => cmd_exec(&args[1..]),
        "kill" | "end" => {
            if let Some(pid_str) = args.get(1) {
                if let Ok(pid) = pid_str.parse::<i32>() {
                    crate::task::process::sys_kill(pid, 9);
                    println!("Sent termination signal (SIGKILL) to task PID {}", pid);
                } else {
                    println!("Usage: tasks kill <pid>");
                }
            } else {
                println!("Usage: tasks kill <pid>");
            }
        }
        "jobs" => crate::shell::job::cmd_jobs(),
        _ => {
            println!("Usage: tasks [list | top | run <binary> | kill <pid> | jobs]");
        }
    }
}

fn cmd_storage(args: &[&str]) {
    let sub = args.first().copied().unwrap_or("list");
    match sub {
        "list" | "disks" | "" => cmd_diskutil(&["list"]),
        "mounts" | "mount" => cmd_mount(),
        "cache" => {
            let (cached_blocks, hits, misses, writes) = crate::fs::bcache::BLOCK_CACHE.lock().stats();
            let total = hits + misses;
            let ratio = if total > 0 { (hits * 100) / total } else { 0 };
            println!("Unified Block Cache (Buffer Cache):");
            println!("  Cached Sectors:   {} / 1024 ({} KiB)", cached_blocks, (cached_blocks * 512) / 1024);
            println!("  Cache Hits:       {} ({}%)", hits, ratio);
            println!("  Cache Misses:     {}", misses);
            println!("  Sector Writes:    {}", writes);
        }
        "sync" | "flush" => {
            let _ = crate::fs::bcache::BLOCK_CACHE.lock().flush_all();
            println!("Storage cache synced.");
        }
        _ => {
            println!("Usage: storage [list | mounts | cache | sync]");
        }
    }
}

fn cmd_network(args: &[&str]) {
    let sub = args.first().copied().unwrap_or("status");
    match sub {
        "status" | "devices" | "info" | "" => cmd_ifconfig(),
        "ping" => cmd_ping(&args[1..]),
        "resolve" | "lookup" | "dns" => cmd_nslookup(&args[1..]),
        "fetch" | "get" | "http" => cmd_curl(&args[1..]),
        "wifi" => cmd_wifi(&args[1..]),
        "lease" | "dhcp" => cmd_dhcp(&args[1..]),
        "sync" | "time" | "ntp" => cmd_ntp(&args[1..]),
        "server" => {
            let srv = args.get(1).copied().unwrap_or("status");
            match srv {
                "http" | "httpd" => cmd_httpd(&args[2..]),
                "telnet" | "telnetd" => cmd_telnetd(&args[2..]),
                "ssh" | "sshd" => cmd_sshd(&args[2..]),
                _ => println!("Usage: network server [http | telnet | ssh]"),
            }
        }
        _ => {
            println!("Usage: network [status | ping <host> | resolve <host> | fetch <url> | wifi | lease | sync | server]");
        }
    }
}

fn cmd_audio(args: &[&str]) {
    let sub = args.first().copied().unwrap_or("chime");
    match sub {
        "chime" => cmd_chime(),
        "tone" | "beep" => cmd_beep(&args[1..]),
        "play" => cmd_play(&args[1..]),
        "speak" | "say" => cmd_say(&args[1..]),
        _ => println!("Usage: audio [chime | tone [freq] [ms] | play <file.wav> | speak <text>]"),
    }
}

fn cmd_sw_vers() {
    println!("ProductName:\t\tSublimeOS");
    println!("ProductVersion:\t\t1.0");
    println!("BuildVersion:\t\t1A01");
    println!("KernelVersion:\t\tKiteKernel 1.0.0");
    println!("Author:\t\t\tNader Mahbub Khan");
    println!("Copyright:\t\tCopyright (c) 2026 Nader Mahbub Khan");
}

fn cmd_system_profiler() {
    let hw = crate::drivers::platform::smbios::get_hardware_telemetry();
    println!("\x1b[1mHardware Overview:\x1b[0m\n");
    println!("  Model Name:            {}", hw.model_name);
    println!("  Model Identifier:      {}", hw.model_identifier);
    println!("  Processor Name:        {}", hw.chip);
    println!("  Total Cores:           {}", hw.cores_threads);
    println!("  Memory:                {}", hw.memory_summary);
    println!("  System Firmware:       {}", hw.system_firmware);
    println!("  Serial Number:         {}", hw.serial_number);
    println!("  Hardware UUID:         {}\n", hw.hardware_uuid);
}

fn cmd_sysctl(args: &[&str]) {
    let hw = crate::drivers::platform::smbios::get_hardware_telemetry();
    let key = args.first().copied().unwrap_or("-a");
    match key {
        "hw.model" => println!("hw.model: {}", hw.model_identifier),
        "hw.machine" => println!("hw.machine: x86_64"),
        "hw.ncpu" => {
            let (_, threads) = crate::arch::x86_64::registers::get_cpu_cores_and_threads();
            println!("hw.ncpu: {}", threads);
        }
        "hw.memsize" => {
            let bytes = crate::mm::pmm::PMM.lock().usable_ram_bytes();
            println!("hw.memsize: {}", bytes);
        }
        "hw.pagesize" => println!("hw.pagesize: 4096"),
        "kern.ostype" => println!("kern.ostype: SublimeOS"),
        "kern.osrelease" => println!("kern.osrelease: 1.0.0"),
        "kern.version" => println!("kern.version: KiteKernel 1.0.0: sublime-release/RELEASE_X86_64"),
        "-a" | "all" => {
            println!("kern.ostype = SublimeOS");
            println!("kern.osrelease = 1.0.0");
            println!("kern.osrevision = 202601");
            println!("kern.version = KiteKernel 1.0.0: root:sublime/RELEASE_X86_64");
            println!("hw.model = {}", hw.model_identifier);
            let (_, threads) = crate::arch::x86_64::registers::get_cpu_cores_and_threads();
            println!("hw.ncpu = {}", threads);
            println!("hw.byteorder = 1234");
            println!("hw.pagesize = 4096");
            let bytes = crate::mm::pmm::PMM.lock().usable_ram_bytes();
            println!("hw.memsize = {}", bytes);
        }
        _ => println!("sysctl: unknown oid '{}'", key),
    }
}

fn cmd_diskutil(args: &[&str]) {
    let sub = args.first().copied().unwrap_or("list");
    if sub == "list" {
        let storage = crate::drivers::storage::STORAGE_MANAGER.lock();
        if !storage.devices.is_empty() {
            for dev in storage.devices.iter() {
                let size_mb = (dev.total_sectors() * 512) / (1024 * 1024);
                println!("/dev/{} (internal, physical):", dev.name());
                println!("   #:                       TYPE NAME                    SIZE       IDENTIFIER");
                println!("   0:      GUID_partition_scheme                        *{} MB    {}", size_mb, dev.name());

                let partitions = crate::fs::partition::scan_device_partitions(dev.clone());
                for (p_idx, part) in partitions.iter().enumerate() {
                    let part_mb = (part.total_sectors() * 512) / (1024 * 1024);
                    let (type_name, vol_name) = match crate::fs::mount::probe_filesystem(part.clone()) {
                        Ok((_, fs_type, label)) => (
                            match fs_type {
                                "fat32" => "MS-DOS FAT32",
                                "exfat" => "Microsoft exFAT",
                                "ext4" => "Linux ext4",
                                "ntfs" => "Windows NTFS",
                                _ => "Unknown FS",
                            },
                            label,
                        ),
                        Err(_) => ("Unformatted / RAW", alloc::string::String::new()),
                    };

                    println!(
                        "   {}: {:>26} {:<24} {:>4} MB   {}",
                        p_idx + 1,
                        type_name,
                        vol_name,
                        part_mb,
                        part.name()
                    );
                }
                println!();
            }
        } else {
            println!("/dev/disk0 (internal, physical):");
            println!("   #:                       TYPE NAME                    SIZE       IDENTIFIER");
            println!("   0:      GUID_partition_scheme                        *128.0 MB   disk0");
            println!("   1:                        EFI EFI                     100.0 MB   disk0s1");
            println!("   2:                  Sublime_FS Container disk1         *28.0 MB   disk0s2\n");
        }

        println!("/dev/disk1 (synthesized):");
        println!("   #:                       TYPE NAME                    SIZE       IDENTIFIER");
        println!("   0:    SublimeFS Container Scheme -                      +28.0 MB    disk1");
        println!("                                 Physical Store disk0s2");
        println!("   1:              Cosmic Volume Sublime HD               12.4 MB    disk1s1");
        println!("   2:              Cosmic Volume Sublime USB              15.6 MB    disk1s2");
    } else {
        println!("Usage: diskutil list");
    }
}

fn cmd_mount() {
    let entries = crate::fs::mount::MOUNT_TABLE.lock().list();
    for entry in entries {
        println!("{} on {} ({}, {})", entry.device_name, entry.mount_point, entry.fs_type, entry.options);
    }
}

fn cmd_open(args: &[&str]) {
    if args.is_empty() {
        println!("The file does not exist.");
        return;
    }
    let target = args[0];
    if target.contains("Finder.app") || target == "gui" || target == "." {
        println!("\x1b[35m[Sublime LaunchServices] Launching Finder.app...\x1b[0m");
        crate::gui::launch_desktop();
    } else {
        println!("[Sublime LaunchServices] Opening '{}'...", target);
        cmd_cat(args);
    }
}

fn cmd_say(args: &[&str]) {
    let text = if args.is_empty() { "Hello, I am SublimeOS." } else {
        // print all args
        &args.join(" ")
    };
    println!("\x1b[35m[SpeechSynthesis (Sublime Voice)]\x1b[0m \"{}\"", text);
    crate::drivers::audio::play_speech(text);
}

fn cmd_beep(args: &[&str]) {
    let freq = args.first().and_then(|s| s.parse::<u32>().ok()).unwrap_or(880);
    let ms = args.get(1).and_then(|s| s.parse::<u32>().ok()).unwrap_or(250);
    println!("\x1b[36mBeeping at {} Hz for {} ms...\x1b[0m", freq, ms);
    crate::drivers::audio::play_tone(freq, ms);
}

fn cmd_chime() {
    println!("\x1b[35m[SublimeOS Audio] Playing Sublime Startup Chime (F-Major Harmonic Chord)...\x1b[0m");
    crate::drivers::audio::play_sublime_chime();
}

fn cmd_play(args: &[&str]) {
    if args.is_empty() {
        println!("Usage: play <audio_file.wav>");
        return;
    }
    let path = args[0];
    println!("\x1b[36mPlaying audio file '{}'...\x1b[0m", path);
    match crate::drivers::audio::play_wav_file(path) {
        Ok(()) => println!("\x1b[32mAudio playback complete.\x1b[0m"),
        Err(e) => println!("\x1b[31mplay error: {}\x1b[0m", e),
    }
}

fn cmd_exec(args: &[&str]) {
    if args.is_empty() {
        println!("Usage: exec <elf_binary_path>");
        return;
    }
    let path = args[0];
    println!("\x1b[36m[LaunchServices] Loading ELF64 binary '{}' into Ring 3 user space...\x1b[0m", path);

    let vfs = crate::fs::vfs::VFS.lock();
    let file = match vfs.open(path, 0) {
        Ok(f) => f,
        Err(e) => {
            println!("\x1b[31mexec: Failed to open '{}': {}\x1b[0m", path, e);
            return;
        }
    };
    drop(vfs);

    let mut buf = alloc::vec![0u8; 262144]; // Read up to 256 KiB
    let bytes_read = match file.read(&mut buf) {
        Ok(n) => n,
        Err(e) => {
            println!("\x1b[31mexec: Failed to read file: {}\x1b[0m", e);
            return;
        }
    };

    if bytes_read == 0 {
        println!("\x1b[31mexec: Binary file is empty\x1b[0m");
        return;
    }

    match crate::task::user::execute_user_binary(&buf[..bytes_read]) {
        Ok(code) => println!("\x1b[32m[Process Terminated] Exit code: {}\x1b[0m", code),
        Err(e) => println!("\x1b[31mexec error: {}\x1b[0m", e),
    }
}

fn cmd_nano(args: &[&str]) {
    if args.is_empty() {
        println!("Usage: nano <file>");
        return;
    }
    crate::shell::editor::run_editor(args[0]);
}

fn cmd_grep(args: &[&str]) {
    let output = crate::shell::pipeline::run_grep("", args);
    print!("{}", output);
}

fn cmd_wc(args: &[&str]) {
    let output = crate::shell::pipeline::run_wc("", args);
    print!("{}", output);
}

fn cmd_head(args: &[&str]) {
    let output = crate::shell::pipeline::run_head("", args);
    print!("{}", output);
}

fn cmd_tail(args: &[&str]) {
    let output = crate::shell::pipeline::run_tail("", args);
    print!("{}", output);
}

fn cmd_sort(args: &[&str]) {
    let output = crate::shell::pipeline::run_sort("", args);
    print!("{}", output);
}

fn cmd_uniq(args: &[&str]) {
    let output = crate::shell::pipeline::run_uniq("", args);
    print!("{}", output);
}

fn cmd_defaults(args: &[&str]) {
    let action = args.first().copied().unwrap_or("read");
    if action == "read" {
        println!("{{\n    SublimeInterfaceStyle = Dark;\n    SublimeFontSmoothing = 2;\n    SublimeKeyboardLayout = US;\n    SublimeMetricUnits = 1;\n    SublimeTemperatureUnit = Celsius;\n}}");
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
    println!("\x1b[1;36mSublimeOS Command Center\x1b[0m — \x1b[35mKiteKernel 1.0.0\x1b[0m");
    println!("Developer: Nader Mahbub Khan | Copyright (c) 2026\n");
    println!("  \x1b[1;33mFILES & WORKSPACE\x1b[0m");
    println!("    \x1b[32mlist\x1b[0m [path]                       List directory contents");
    println!("    \x1b[32mview\x1b[0m <file>                       Display file contents");
    println!("    \x1b[32mprint\x1b[0m [text...]                   Print text to screen");
    println!("    \x1b[32mfind\x1b[0m <pattern> [file]             Search text matching pattern");
    println!("    \x1b[32mcount\x1b[0m <file>                      Count lines, words, and characters");
    println!("    \x1b[32medit\x1b[0m <file>                       Full-screen text editor");
    println!("    \x1b[32mcreate\x1b[0m <file>                     Create new file");
    println!("    \x1b[32mdelete\x1b[0m <file>                     Remove file");
    println!("    \x1b[32mmake folder\x1b[0m <path>                Create directory");
    println!();
    println!("  \x1b[1;33mSYSTEM & DEVICE\x1b[0m");
    println!("    \x1b[32mabout\x1b[0m                             System & kernel release information");
    println!("    \x1b[32mhardware\x1b[0m                          Silicon hardware & firmware specifications");
    println!("    \x1b[32msystem\x1b[0m [info|config|uptime]        System status dashboard");
    println!("    \x1b[32mtasks\x1b[0m [list|top|run <bin>|kill]   Process manager & task control");
    println!("    \x1b[32mdisplay\x1b[0m [1|2]                     Console font scale & HiDPI (Ctrl+= / Ctrl+-)");
    println!("    \x1b[32mpath\x1b[0m                              Print current working directory");
    println!("    \x1b[32mpower\x1b[0m [off|restart]               Safely shut down or restart computer");
    println!("    \x1b[32mclear\x1b[0m                             Clear screen");
    println!();
    println!("  \x1b[1;33mSTORAGE & CONNECTIVITY\x1b[0m");
    println!("    \x1b[32mstorage\x1b[0m [list|sync]               Storage drives, partitions, and cache");
    println!("    \x1b[32mnetwork\x1b[0m [status|wifi|ping]        Network interfaces, Wi-Fi scanner, ping");
    println!("    \x1b[32maudio\x1b[0m [chime|play]                Audio chime and sound playback");
    println!();
    println!("  \x1b[1;33mAPPLICATIONS\x1b[0m");
    println!("    \x1b[32mdesktop\x1b[0m                           Aqua graphical window compositor");
    println!("    \x1b[32mcalc\x1b[0m                              64-bit expression calculator");
    println!("    \x1b[32msnake\x1b[0m                             Classic arcade game");
}

fn cmd_gui() {
    println!("\x1b[35m[LaunchServices] Starting SublimeOS Aqua Desktop...\x1b[0m");
    crate::gui::launch_desktop();
}

fn cmd_windowserver() {
    cmd_exec(&["/bin/windowserver"]);
}

fn cmd_telnetd(_args: &[&str]) {
    cmd_exec(&["/bin/telnetd"]);
}

fn cmd_sshd(_args: &[&str]) {
    cmd_exec(&["/bin/sshd"]);
}

fn cmd_telnet(args: &[&str]) {
    if args.is_empty() {
        println!("Usage: telnet <host_ip> [port]");
        return;
    }
    let host_str = args[0];
    let port: u16 = if args.len() > 1 {
        args[1].parse().unwrap_or(23)
    } else {
        23
    };

    let ip_parts: Vec<&str> = host_str.split('.').collect();
    if ip_parts.len() != 4 {
        println!("\x1b[31mtelnet: Invalid IPv4 address '{}'\x1b[0m", host_str);
        return;
    }

    let ip = [
        ip_parts[0].parse().unwrap_or(127),
        ip_parts[1].parse().unwrap_or(0),
        ip_parts[2].parse().unwrap_or(0),
        ip_parts[3].parse().unwrap_or(1),
    ];

    println!("Trying {}.{}.{}.{}...", ip[0], ip[1], ip[2], ip[3]);
    let sock_id = match crate::net::socket::socket_open() {
        Ok(id) => id,
        Err(e) => {
            println!("\x1b[31mtelnet: Unable to open socket: {}\x1b[0m", e);
            return;
        }
    };

    if let Err(e) = crate::net::socket::socket_connect(sock_id, ip, port) {
        println!("\x1b[31mtelnet: Unable to connect to remote host: {}\x1b[0m", e);
        let _ = crate::net::socket::socket_close(sock_id);
        return;
    }

    println!("Connected to {}.{}.{}.{}:{}.", ip[0], ip[1], ip[2], ip[3], port);
    println!("Escape character is '^]'.");

    let mut rx_buf = [0u8; 1024];
    if let Ok(n) = crate::net::socket::socket_recv(sock_id, &mut rx_buf, 1000) {
        if n > 0 {
            if let Ok(s) = core::str::from_utf8(&rx_buf[..n]) {
                crate::print!("{}", s);
            }
        }
    }

    let _ = crate::net::socket::socket_close(sock_id);
    println!("Connection closed by foreign host.");
}

fn cmd_clear() {
    crate::arch::x86_64::vga::VGA_WRITER.lock().clear_screen();
    print!("\x1b[2J\x1b[H");
}

fn cmd_uname() {
    println!("SublimeOS sublimeos.local 1.0.0 KiteKernel Version 1.0.0 x86_64 (by Nader Mahbub Khan, c 2026)");
}

fn cmd_uptime() {
    let ticks = crate::arch::x86_64::pit::get_ticks();
    let seconds = ticks / 1000;
    println!("up {} seconds ({} ticks at 1000 Hz)", seconds, ticks);
}

fn cmd_date() {
    let dt = crate::arch::x86_64::rtc::get_system_datetime();
    let unix = crate::arch::x86_64::rtc::get_system_time_unix();
    let synced = if crate::arch::x86_64::rtc::is_time_synchronized() { " (NTP synced)" } else { "" };
    println!("{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC (Unix: {}){}", dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second, unix, synced);
}

fn cmd_mem() {
    let (total_frames, used_frames, free_frames) = crate::mm::pmm::PMM.lock().stats();
    let (heap_total, heap_allocated, heap_free) = crate::mm::heap::HEAP_ALLOCATOR.stats();

    println!("Unified Memory:      {} MB Total", (total_frames * 4096) / (1024 * 1024));
    println!("Wired Memory:        {} KB Used", (used_frames * 4096) / 1024);
    println!("Free Memory:         {} KB Free", (free_frames * 4096) / 1024);
    println!("Sublime Kernel Heap: {} KB Total ({} KB Allocated, {} KB Free)", heap_total / 1024, heap_allocated / 1024, heap_free / 1024);
}

fn cmd_ps() {
    let tasks = crate::task::scheduler::get_task_list();
    println!("  PID  STATE       TICKS  NAME");
    if !tasks.is_empty() {
        for (tid, name, state, ticks) in tasks {
            let state_str = match state {
                crate::task::thread::ThreadState::Running => "Running ",
                crate::task::thread::ThreadState::Ready => "Ready   ",
                crate::task::thread::ThreadState::Blocked => "Blocked ",
                crate::task::thread::ThreadState::Sleeping(_) => "Sleeping",
                crate::task::thread::ThreadState::Dead => "Dead    ",
            };
            println!("{:5}  {:10}  {:5}  {}", tid, state_str, ticks, name);
        }
    } else {
        println!("    0  Running        10  kernel_idle");
        println!("    1  Ready          25  launchd");
        println!("    2  Running        45  sublimesh");
    }
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

fn cmd_usb() {
    println!("\x1b[1;36mSublimeOS USB Subsystem & Device Topology\x1b[0m\n");

    let usb = crate::drivers::usb::USB.lock();

    // 1. xHCI Controllers
    for (idx, xhci) in usb.xhci_controllers.iter().enumerate() {
        println!(
            "  \x1b[1mController #{}: xHCI USB 3.0 / Extensible Host Controller\x1b[0m",
            idx + 1
        );
        println!(
            "    PCI: {:02x}:{:02x}.{} | BAR0: 0x{:08X} | Ports: {} | Slots: {}",
            xhci.pci.bus, xhci.pci.device, xhci.pci.function, xhci.pci.bar0, xhci.max_ports, xhci.max_slots
        );
        for &port in &xhci.connected_devices {
            println!("    └── Root Port {}: Connected (Active Slot)", port);
        }
        println!();
    }

    // 2. EHCI Controllers & Topology Tree
    for (idx, ehci) in usb.ehci_controllers.iter().enumerate() {
        println!(
            "  \x1b[1mController #{}: EHCI USB 2.0 / Enhanced Host Controller\x1b[0m",
            usb.xhci_controllers.len() + idx + 1
        );
        println!(
            "    PCI: {:02x}:{:02x}.{} | BAR0: 0x{:08X} | Root Ports: {}",
            ehci.pci.bus, ehci.pci.device, ehci.pci.function, ehci.pci.bar0, ehci.num_ports
        );

        if ehci.discovered_devices.is_empty() {
            println!("    (No USB 2.0 devices enumerated)");
        } else {
            for dev in &ehci.discovered_devices {
                if dev.is_hub {
                    println!(
                        "    └── Root Port {}: \x1b[33mHub\x1b[0m (Addr {}, {} ports, {:?}) [VID:PID {:04X}:{:04X}] - {}",
                        dev.root_port,
                        dev.address,
                        dev.hub_ports.unwrap_or(0),
                        dev.speed,
                        dev.vendor_id,
                        dev.product_id,
                        dev.name
                    );
                    for child in &ehci.discovered_devices {
                        if let Some((p_addr, p_port)) = child.parent_hub {
                            if p_addr == dev.address && !child.is_hub {
                                println!(
                                    "        └── Port {}: \x1b[32mDevice\x1b[0m (Addr {}, {:?}) [VID:PID {:04X}:{:04X}] - {}",
                                    p_port,
                                    child.address,
                                    child.speed,
                                    child.vendor_id,
                                    child.product_id,
                                    child.name
                                );
                            }
                        }
                    }
                } else if dev.parent_hub.is_none() {
                    println!(
                        "    └── Root Port {}: \x1b[32mDevice\x1b[0m (Addr {}, {:?}) [VID:PID {:04X}:{:04X}] - {}",
                        dev.root_port,
                        dev.address,
                        dev.speed,
                        dev.vendor_id,
                        dev.product_id,
                        dev.name
                    );
                }
            }
        }
        println!();
    }

    // 3. Registered USB HID Input Devices
    let hid = crate::drivers::usb::hid::USB_HID.lock();
    println!("  \x1b[1mActive USB Human Interface Devices (HID):\x1b[0m");
    if hid.devices.is_empty() {
        println!("    (None registered)");
    } else {
        for dev_arc in &hid.devices {
            let dev = dev_arc.lock();
            println!("    • {} ({:?})", dev.name, dev.dev_type);
        }
    }
    println!();
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
    println!("\x1b[31mRestarting SublimeOS...\x1b[0m");
    for _ in 0..500_000 {
        core::hint::spin_loop();
    }
    crate::arch::x86_64::acpi::reboot();
}

fn cmd_halt() {
    println!("\x1b[31mShutting down SublimeOS...\x1b[0m");
    for _ in 0..500_000 {
        core::hint::spin_loop();
    }
    crate::arch::x86_64::acpi::poweroff();
}

fn cmd_shutdown(args: &[&str]) {
    if args.iter().any(|&a| a == "-r" || a == "--reboot" || a == "reboot") {
        cmd_reboot();
    } else {
        cmd_halt();
    }
}

fn cmd_font(args: &[&str]) {
    use crate::drivers::gop_console::GOP_CONSOLE;
    if let Some(mut con) = GOP_CONSOLE.try_lock() {
        if args.is_empty() {
            println!("SublimeOS Console Typography Engine");
            println!("------------------------------------");
            println!("  Display Resolution: {}x{} ({} bpp, Pitch: {} B)", con.width, con.height, con.bpp, con.pitch);
            println!("  Character Grid:     {} cols x {} rows", con.cols, con.rows);
            println!("  Font Scale:         {}x ({}x{} pixels/cell)", con.font_scale, con.cell_w(), con.cell_h());
            println!("  Anti-Aliasing:      {}", if con.anti_alias { "Enabled (Morphological Subpixel Smoothing)" } else { "Disabled (Crisp 1-bit Binary)" });
            println!("  Screen Margins:     {}px horizontal, {}px vertical", con.margin_x, con.margin_y);
            println!();
            println!("Usage:");
            println!("  font scale 1        Switch to 8x16 font (maximum density)");
            println!("  font scale 2        Switch to 16x32 font (HiDPI / large comfort)");
            println!("  font aa on          Enable subpixel curve anti-aliasing");
            println!("  font aa off         Disable anti-aliasing (pure 1-bit)");
            println!("Hotkeys: Ctrl+= / Ctrl+- (change font scale)");
        } else if args[0] == "scale" && args.len() >= 2 {
            match args[1] {
                "1" => {
                    con.set_font_scale(1);
                    println!("Font scale set to 1x (8x16). Grid is now {} cols x {} rows.", con.cols, con.rows);
                }
                "2" => {
                    con.set_font_scale(2);
                    println!("Font scale set to 2x (16x32). Grid is now {} cols x {} rows.", con.cols, con.rows);
                }
                _ => println!("Invalid scale. Supported scales: 1, 2"),
            }
        } else if args[0] == "aa" && args.len() >= 2 {
            match args[1] {
                "on" | "true" | "1" => {
                    con.set_anti_alias(true);
                    println!("Font anti-aliasing enabled.");
                }
                "off" | "false" | "0" => {
                    con.set_anti_alias(false);
                    println!("Font anti-aliasing disabled.");
                }
                _ => println!("Usage: font aa on | font aa off"),
            }
        } else {
            println!("Usage: font [scale 1|2] | [aa on|off]");
        }
    } else {
        println!("Display console lock busy.");
    }
}

fn cmd_wifi(args: &[&str]) {
    let sub = args.first().copied().unwrap_or("help");
    match sub {
        "scan" => {
            println!("Scanning 2.4 GHz and 5 GHz wireless spectrum via PCIe 802.11 hardware...");
            let aps = crate::net::wifi::WIFI_MANAGER.lock().scan().to_vec();
            if aps.is_empty() {
                println!("  No 802.11 access points detected (no physical Wi-Fi radio attached or no beacons received).");
                return;
            }
            println!("{:<20} {:<18} {:<8} {:<4} {:<12} {:<12}", "SSID", "BSSID", "RSSI", "CH", "SECURITY", "SIGNAL");
            println!("{:-<20} {:-<18} {:-<8} {:-<4} {:-<12} {:-<12}", "", "", "", "", "", "");
            for ap in aps {
                let b = ap.bssid;
                let bssid_str = format!("{:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}", b[0], b[1], b[2], b[3], b[4], b[5]);
                let rssi_str = format!("{}dBm", ap.rssi);
                println!("{:<20} {:<18} {:<8} {:<4} {:<12} {:<12}",
                    ap.ssid, bssid_str, rssi_str, ap.channel, ap.security.as_str(), ap.signal_bars());
            }
        }
        "connect" => {
            if args.len() < 2 {
                println!("Usage: wifi connect <SSID> [passphrase]");
                return;
            }
            let ssid = args[1];
            let pass = args.get(2).copied();
            let ifname = crate::net::device::NET_MANAGER
                .lock()
                .devices
                .iter()
                .find(|(d, _)| d.device_type() == crate::net::device::NetDeviceType::Wireless80211)
                .map(|(d, _)| alloc::string::String::from(d.name()))
                .unwrap_or_else(|| alloc::string::String::from("wlan0"));

            println!("\x1b[36mInitiating 802.11 association with '{}'...\x1b[0m", ssid);
            match crate::net::wifi::WIFI_MANAGER.lock().connect(ssid, pass) {
                Ok(()) => {
                    println!("\x1b[32m[OK] Associated with '{}' via 802.11 MAC\x1b[0m", ssid);
                    println!("Interface: {} (run 'dhcp' or configure IPv4 after 4-way handshake)", ifname);
                }
                Err(e) => {
                    println!("\x1b[31mwifi connect error: {}\x1b[0m", e);
                }
            }
        }
        "status" => {
            let ifname = crate::net::device::NET_MANAGER
                .lock()
                .devices
                .iter()
                .find(|(d, _)| d.device_type() == crate::net::device::NetDeviceType::Wireless80211)
                .map(|(d, _)| alloc::string::String::from(d.name()))
                .unwrap_or_else(|| alloc::string::String::from("wlan0"));

            let mgr = crate::net::wifi::WIFI_MANAGER.lock();
            if mgr.is_connected {
                let b = mgr.current_bssid.unwrap_or([0; 6]);
                println!("Wi-Fi Status: \x1b[32mConnected\x1b[0m");
                println!("  Interface:    {} (IEEE 802.11n/ac)", ifname);
                println!("  SSID:         {}", mgr.current_ssid.as_deref().unwrap_or("Unknown"));
                println!("  BSSID:        {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}", b[0], b[1], b[2], b[3], b[4], b[5]);
                println!("  Channel:      {}", mgr.current_channel);
                println!("  Tx Rate:      {} Mbps", mgr.link_speed_mbps);
                println!("  Security:     WPA2-Personal (PBKDF2-SHA1 Key Derivation)");
            } else {
                println!("Wi-Fi Status: \x1b[33mDisconnected\x1b[0m ({} inactive)", ifname);
                println!("Type 'wifi scan' or 'wifi connect <SSID> [password]' to associate.");
            }
        }
        "disconnect" => {
            crate::net::wifi::WIFI_MANAGER.lock().disconnect();
            println!("wlan0: Disconnected from wireless network.");
        }
        _ => {
            println!("Usage: wifi [scan | connect <ssid> [passphrase] | status | disconnect]");
        }
    }
}

fn cmd_dhcp(args: &[&str]) {
    let subcmd = args.get(0).copied().unwrap_or("status");
    match subcmd {
        "status" => {
            let client = crate::net::dhcp::DHCP_CLIENT.lock();
            println!("SublimeOS DHCP Client Status:");
            println!("  State:     {:?}", client.state);
            println!("  Interface: {}", if client.interface_name.is_empty() { "none" } else { &client.interface_name });
            if let Some(ref lease) = client.active_lease {
                println!("  IP:        {}.{}.{}.{}", lease.assigned_ip[0], lease.assigned_ip[1], lease.assigned_ip[2], lease.assigned_ip[3]);
                println!("  Subnet:    {}.{}.{}.{}", lease.subnet_mask[0], lease.subnet_mask[1], lease.subnet_mask[2], lease.subnet_mask[3]);
                println!("  Router:    {}.{}.{}.{}", lease.router[0], lease.router[1], lease.router[2], lease.router[3]);
                println!("  DNS:       {}.{}.{}.{}", lease.dns_server[0], lease.dns_server[1], lease.dns_server[2], lease.dns_server[3]);
                println!("  Server ID: {}.{}.{}.{}", lease.server_id[0], lease.server_id[1], lease.server_id[2], lease.server_id[3]);
                println!("  Lease:     {} seconds", lease.lease_time);
            } else {
                println!("  No active lease assigned.");
            }
        }
        "discover" | "renew" => {
            let iface = args.get(1).copied().unwrap_or("en0");
            println!("Sending DHCPDISCOVER on {}...", iface);
            match crate::net::dhcp::start_dhcp_discovery(iface) {
                Ok(frame) => {
                    let net = crate::net::device::NET_MANAGER.lock();
                    if let Some((dev, _)) = net.get_device(iface) {
                        let _ = dev.send_packet(&frame);
                        println!("DHCPDISCOVER transmitted (broadcast)");
                    }
                }
                Err(e) => println!("DHCP discovery error: {}", e),
            }
        }
        "release" => {
            let mut client = crate::net::dhcp::DHCP_CLIENT.lock();
            if let Some(lease) = client.active_lease.take() {
                client.state = crate::net::dhcp::DhcpState::Init;
                println!("Released DHCP lease for {}.{}.{}.{}", lease.assigned_ip[0], lease.assigned_ip[1], lease.assigned_ip[2], lease.assigned_ip[3]);
            } else {
                println!("No active DHCP lease to release.");
            }
        }
        _ => {
            println!("Usage: dhcp [status | discover [interface] | release]");
        }
    }
}

fn cmd_ntp(args: &[&str]) {
    let subcmd = args.get(0).copied().unwrap_or("status");
    match subcmd {
        "status" => {
            let status = crate::net::ntp::format_ntp_status();
            print!("{}", status);
        }
        "sync" => {
            let server_str = args.get(1).copied().unwrap_or("10.0.2.2");
            match crate::net::parse_or_resolve_ip(server_str) {
                Ok(server_ip) => {
                    println!("Sending NTP request to {}.{}.{}.{}:123...", server_ip[0], server_ip[1], server_ip[2], server_ip[3]);
                    match crate::net::ntp::send_ntp_request(server_ip) {
                        Ok(()) => println!("NTP request sent. Awaiting time datagram response..."),
                        Err(e) => println!("NTP sync error: {}", e),
                    }
                }
                Err(e) => println!("Invalid NTP server address: {}", e),
            }
        }
        "set" => {
            if let Some(arg) = args.get(1) {
                if let Ok(unix) = arg.parse::<u64>() {
                    crate::arch::x86_64::rtc::set_system_time(unix);
                    let dt = crate::arch::x86_64::rtc::unix_to_datetime(unix);
                    println!("System time set to {:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC (Unix: {})", dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second, unix);
                } else {
                    println!("Usage: ntp set <unix_timestamp_seconds>");
                }
            } else {
                println!("Usage: ntp set <unix_timestamp_seconds>");
            }
        }
        _ => {
            println!("Usage: ntp [status | sync [server_ip] | set <unix_timestamp>]");
        }
    }
}

fn cmd_ifconfig() {
    let net = crate::net::device::NET_MANAGER.lock();
    for (dev, cfg) in &net.devices {
        let mac = dev.mac_address();
        let mac_str = format!("{:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}", mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]);
        let is_up = dev.is_link_up();
        let status = if is_up { "active" } else { "inactive" };

        match dev.device_type() {
            crate::net::device::NetDeviceType::Loopback => {
                println!("{}: flags=8049<UP,LOOPBACK,RUNNING,MULTICAST> mtu 16384", dev.name());
                println!("        inet {}.{}.{}.{} netmask {}.{}.{}.{}",
                    cfg.ipv4[0], cfg.ipv4[1], cfg.ipv4[2], cfg.ipv4[3],
                    cfg.netmask[0], cfg.netmask[1], cfg.netmask[2], cfg.netmask[3]
                );
            }
            crate::net::device::NetDeviceType::Ethernet => {
                println!("{}: flags=8863<UP,BROADCAST,SMART,RUNNING,SIMPLEX,MULTICAST> mtu 1500", dev.name());
                println!("        ether {}", mac_str);
                if cfg.ipv4 != [0, 0, 0, 0] {
                    println!("        inet {}.{}.{}.{} netmask {}.{}.{}.{} broadcast 10.0.2.255",
                        cfg.ipv4[0], cfg.ipv4[1], cfg.ipv4[2], cfg.ipv4[3],
                        cfg.netmask[0], cfg.netmask[1], cfg.netmask[2], cfg.netmask[3]
                    );
                }
                println!("        media: autoselect (1000baseT <full-duplex>)");
                println!("        status: {}", status);
            }
            crate::net::device::NetDeviceType::Wireless80211 => {
                println!("{}: flags=8843<UP,BROADCAST,RUNNING,SIMPLEX,MULTICAST> mtu 1500", dev.name());
                println!("        ether {}", mac_str);
                if cfg.ipv4 != [0, 0, 0, 0] {
                    println!("        inet {}.{}.{}.{} netmask {}.{}.{}.{} gateway {}.{}.{}.{}",
                        cfg.ipv4[0], cfg.ipv4[1], cfg.ipv4[2], cfg.ipv4[3],
                        cfg.netmask[0], cfg.netmask[1], cfg.netmask[2], cfg.netmask[3],
                        cfg.gateway[0], cfg.gateway[1], cfg.gateway[2], cfg.gateway[3]
                    );
                }
                let wifi_connected = crate::net::wifi::WIFI_MANAGER.lock().is_connected;
                let wifi_status = if wifi_connected { "active" } else { "inactive" };
                println!("        media: IEEE 802.11 (Wi-Fi 802.11ac 866 Mbps)");
                println!("        status: {}", wifi_status);
            }
        }
    }
}

fn cmd_ping(args: &[&str]) {
    if args.is_empty() {
        println!("Usage: ping <destination_ip | hostname>");
        return;
    }
    let target = args[0];
    let dst_ip = match crate::net::parse_or_resolve_ip(target) {
        Ok(ip) => ip,
        Err(e) => {
            println!("ping: cannot resolve {}: {}", target, e);
            return;
        }
    };

    println!("PING {} ({}.{}.{}.{}): 56 data bytes",
        target, dst_ip[0], dst_ip[1], dst_ip[2], dst_ip[3]
    );

    let mut received = 0;
    let mut total_rtt = 0u64;

    for seq in 1..=4 {
        match crate::net::ping_ip(dst_ip, seq) {
            Ok(rtt) => {
                received += 1;
                total_rtt += rtt;
                println!("64 bytes from {}.{}.{}.{}: icmp_seq={} ttl=64 time={} ms",
                    dst_ip[0], dst_ip[1], dst_ip[2], dst_ip[3], seq, rtt
                );
            }
            Err(e) => {
                println!("Request timeout for icmp_seq {}: {}", seq, e);
            }
        }

        // Small interval between pings
        crate::arch::x86_64::pit::sleep_ms(200);
    }

    println!("\n--- {} ping statistics ---", target);
    let loss_pct = ((4 - received) * 100) / 4;
    println!("4 packets transmitted, {} packets received, {}% packet loss", received, loss_pct);
    if received > 0 {
        let avg_rtt = total_rtt / received as u64;
        println!("round-trip avg = {} ms", avg_rtt);
    }
}

fn cmd_nslookup(args: &[&str]) {
    if args.is_empty() {
        println!("Usage: nslookup <hostname>");
        return;
    }
    let target = args[0];
    println!("Server:\t\t1.1.1.1");
    println!("Address:\t1.1.1.1#53\n");

    match crate::net::parse_or_resolve_ip(target) {
        Ok(ip) => {
            println!("Non-authoritative answer:");
            println!("Name:\t{}", target);
            println!("Address:\t{}.{}.{}.{}", ip[0], ip[1], ip[2], ip[3]);
        }
        Err(e) => {
            println!("** server can't find {}: {}", target, e);
        }
    }
}

fn cmd_curl(args: &[&str]) {
    if args.is_empty() {
        println!("Usage: curl [-i] <url>");
        println!("Example: curl http://127.0.0.1:8080/api/status");
        return;
    }

    let mut include_headers = false;
    let mut url_target = args[0];

    if args[0] == "-i" {
        include_headers = true;
        if args.len() < 2 {
            println!("Usage: curl [-i] <url>");
            return;
        }
        url_target = args[1];
    }

    println!("\x1b[36mConnecting to {} via TCP...\x1b[0m", url_target);
    match crate::net::http::http_get(url_target) {
        Ok(resp) => {
            if resp.status_code == 200 {
                println!("\x1b[32mHTTP/1.1 {} {}\x1b[0m", resp.status_code, resp.status_text);
            } else {
                println!("\x1b[33mHTTP/1.1 {} {}\x1b[0m", resp.status_code, resp.status_text);
            }

            if include_headers {
                for (k, v) in &resp.headers {
                    println!("{}: {}", k, v);
                }
                println!();
            }

            println!("{}", resp.body_as_str());
        }
        Err(e) => {
            println!("\x1b[31mcurl error: {}\x1b[0m", e);
        }
    }
}

fn cmd_httpd(args: &[&str]) {
    let sub = args.first().copied().unwrap_or("status");
    match sub {
        "status" => {
            println!("SublimeOS Embedded Web Server (httpd):");
            println!("  Port:       8080 / 80 (TCP)");
            println!("  Status:     \x1b[32mActive (Listening on all interfaces)\x1b[0m");
            println!("  Endpoints:  /");
            println!("              /api/status");
            println!("  Try:        curl http://127.0.0.1:8080/api/status");
        }
        "test" => {
            println!("Testing local httpd response on /api/status:");
            let resp = crate::net::http::generate_httpd_response("/api/status");
            println!("{}", resp);
        }
        _ => {
            println!("Usage: httpd [status | test]");
        }
    }
}
