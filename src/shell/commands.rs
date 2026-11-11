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
        "mount" => cmd_mount(),
        "wifi" => cmd_wifi(args),
        "ifconfig" => cmd_ifconfig(),
        "ping" => cmd_ping(args),
        "nslookup" => cmd_nslookup(args),
        "curl" | "http" => cmd_curl(args),
        "httpd" => cmd_httpd(args),
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
            println!("   2:                  Cosmic_FS Container disk1         *28.0 MB   disk0s2\n");
        }

        println!("/dev/disk1 (synthesized):");
        println!("   #:                       TYPE NAME                    SIZE       IDENTIFIER");
        println!("   0:    CosmicFS Container Scheme -                      +28.0 MB    disk1");
        println!("                                 Physical Store disk0s2");
        println!("   1:              Cosmic Volume Cosmic HD               12.4 MB    disk1s1");
        println!("   2:              Cosmic Volume Cosmic USB              15.6 MB    disk1s2");
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
    println!("  mount           List mounted filesystems (FAT32, exFAT, ext4, NTFS)");
    println!("  wifi [subcmd]   802.11 Wi-Fi manager (scan, connect, status, disconnect)");
    println!("  ifconfig        Display network interface configuration (lo0, en0, wlan0)");
    println!("  ping <target>   Send ICMP ECHO_REQUEST packets to network hosts");
    println!("  nslookup <host> Query DNS nameservers for IPv4 host address");
    println!("  curl [-i] <url> Fetch web pages & REST APIs over TCP / HTTP");
    println!("  httpd [status]  CosmicOS Embedded Web Server dashboard");
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
    crate::arch::x86_64::acpi::reboot();
}

fn cmd_halt() {
    println!("\x1b[31mShutting down CosmicOS...\x1b[0m");
    crate::arch::x86_64::acpi::poweroff();
}

fn cmd_wifi(args: &[&str]) {
    let sub = args.first().copied().unwrap_or("help");
    match sub {
        "scan" => {
            println!("Scanning 2.4 GHz and 5 GHz wireless spectrum for 802.11 beacons...");
            let aps = crate::net::wifi::WIFI_MANAGER.lock().scan().to_vec();
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
            println!("\x1b[36mInitiating association with '{}'...\x1b[0m", ssid);
            match crate::net::wifi::WIFI_MANAGER.lock().connect(ssid, pass) {
                Ok(()) => {
                    println!("\x1b[32m[OK] Connected to '{}' via WPA2-PSK (AES-CCMP)\x1b[0m", ssid);
                    println!("Interface: wlan0 (IPv4: 192.168.1.120, Netmask: 255.255.255.0, Gateway: 192.168.1.1)");
                }
                Err(e) => {
                    println!("\x1b[31mwifi connect error: {}\x1b[0m", e);
                }
            }
        }
        "status" => {
            let mgr = crate::net::wifi::WIFI_MANAGER.lock();
            if mgr.is_connected {
                let b = mgr.current_bssid.unwrap_or([0; 6]);
                println!("Wi-Fi Status: \x1b[32mConnected\x1b[0m");
                println!("  Interface:    wlan0 (IEEE 802.11ac SoftMAC)");
                println!("  SSID:         {}", mgr.current_ssid.as_deref().unwrap_or("Unknown"));
                println!("  BSSID:        {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}", b[0], b[1], b[2], b[3], b[4], b[5]);
                println!("  Channel:      {}", mgr.current_channel);
                println!("  Tx Rate:      {} Mbps", mgr.link_speed_mbps);
                println!("  Security:     WPA2-Personal (PBKDF2-SHA1 Key Derivation)");
            } else {
                println!("Wi-Fi Status: \x1b[33mDisconnected\x1b[0m (wlan0 inactive)");
                println!("Type 'wifi scan' to search for available wireless networks.");
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
            println!("CosmicOS Embedded Web Server (httpd):");
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
