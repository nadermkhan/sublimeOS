#![no_std]
#![no_main]

extern crate alloc;

use sublime_rt::{print, println, read, sched_yield, spawn, sync, syscommand, write};

fn read_line(buf: &mut [u8]) -> usize {
    let mut len = 0;
    while len < buf.len() - 1 {
        let mut ch = [0u8; 1];
        let n = read(0, &mut ch);
        if n <= 0 {
            sched_yield();
            continue;
        }
        let b = ch[0];
        if b == b'\r' || b == b'\n' {
            println!();
            break;
        } else if b == 0x08 || b == 127 {
            // Backspace handling
            if len > 0 {
                len -= 1;
                print!("\x08 \x08");
            }
        } else if b >= 32 && b <= 126 {
            buf[len] = b;
            len += 1;
            let echo = [b];
            write(1, &echo);
        }
    }
    buf[len] = 0;
    len
}

#[no_mangle]
pub fn main() -> i32 {
    println!("\x1b[1;36mSublimeOS\x1b[0m 1.0 (x86_64) — \x1b[35mKiteKernel\x1b[0m");
    println!("Developer: Nader Mahbub Khan | Copyright (c) 2026");
    println!("Type 'help' for available commands.\n");

    let prompt = "\x1b[36msublime\x1b[0m \x1b[34m/\x1b[0m > ";

    let mut line_buf = [0u8; 256];

    loop {
        print!("{}", prompt);
        let n = read_line(&mut line_buf);
        if n == 0 {
            continue;
        }

        let raw_str = match core::str::from_utf8(&line_buf[..n]) {
            Ok(s) => s,
            Err(_) => continue,
        };

        let trimmed = raw_str.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Built-in commands & Userland Binaries
        let first_word = trimmed.split_whitespace().next().unwrap_or("");

        if trimmed == "help" {
            println!("\x1b[1;36mSublimeOS\x1b[0m 1.0 — \x1b[35mKiteKernel\x1b[0m");
            println!("Developer: Nader Mahbub Khan | Copyright (c) 2026\n");
            println!("\x1b[1mAvailable Commands:\x1b[0m\n");
            println!("  \x1b[1;36mFiles & Workspace:\x1b[0m");
            println!("    list [path]         List files in directory (or 'files')");
            println!("    view <file...>      Display contents of text file (or 'read')");
            println!("    print [text...]     Print text to screen");
            println!("    find <pattern> <f>  Search text matching pattern");
            println!("    count <file...>     Count lines, words, and characters");
            println!("    edit <file>         Full-screen text editor");
            println!("    create <file>       Create new file");
            println!("    delete <file>       Remove file");
            println!("    make folder <path>  Create new directory");
            println!();
            println!("  \x1b[1;36mSystem & Device:\x1b[0m");
            println!("    about               System & kernel release information");
            println!("    hardware            Silicon hardware & firmware specifications");
            println!("    tasks [list|kill]   Process manager and task control");
            println!("    storage [list|sync] Storage drives and buffer cache");
            println!("    display [1|2]       Console font size & HiDPI (Ctrl+= / Ctrl+-)");
            println!("    path                Print current working directory");
            println!("    clear               Clear screen");
            println!();
            println!("  \x1b[1;36mConnectivity & Audio:\x1b[0m");
            println!("    network [wifi|ping] Network interfaces, Wi-Fi scanner, and ping");
            println!("    audio [chime|play]  Audio chime and WAV playback");
            println!("    power [off|restart] Safely shut down or restart computer");
            println!();
            println!("  \x1b[1;36mApplications:\x1b[0m");
            println!("    desktop             Aqua graphical window compositor");
            println!("    calc                64-bit expression calculator");
            println!("    snake               Classic arcade game");
            println!("    exit                Exit shell\n");
        } else if trimmed == "clear" {
            print!("\x1b[2J\x1b[H");
        } else if trimmed == "path" || trimmed == "where" || trimmed == "pwd" {
            println!("/");
        } else if trimmed == "about" || trimmed == "uname" {
            println!("SublimeOS 1.0.0 (x86_64) — KiteKernel");
            println!("Developer: Nader Mahbub Khan | Copyright (c) 2026");
        } else if trimmed == "hardware" || trimmed == "specs" {
            syscommand("hardware");
        } else if trimmed == "sync" {
            sync();
            println!("Storage cache synced.");
        } else if first_word == "power" {
            let rest = trimmed.strip_prefix("power").unwrap_or("").trim();
            if rest == "off" || rest == "shutdown" || rest == "halt" {
                syscommand("shutdown");
            } else if rest == "restart" || rest == "reboot" || rest == "reset" {
                syscommand("reboot");
            } else {
                println!("Usage: power [off | restart]");
            }
        } else if first_word == "display" || first_word == "screen" {
            let rest = trimmed.split_whitespace().nth(1).unwrap_or("1");
            let mut cmd = alloc::string::String::from("font scale ");
            cmd.push_str(rest);
            syscommand(&cmd);
        } else if first_word == "exec" || first_word == "run" {
            let cmd_to_exec = trimmed.split_at(trimmed.find(' ').unwrap_or(0)).1.trim();
            if !cmd_to_exec.is_empty() {
                let code = spawn(cmd_to_exec);
                if code < 0 {
                    println!("\x1b[31mexec error: failed to spawn '{}' (code {})\x1b[0m", cmd_to_exec, code);
                } else {
                    println!("\x1b[32m[Process Terminated] Exit code: {}\x1b[0m", code);
                }
            }
        } else if trimmed == "exit" || trimmed == "quit" {
            break;
        } else if matches!(first_word, "list" | "files" | "ls") {
            let code = spawn(trimmed);
            if code < 0 {
                let rest = trimmed.strip_prefix(first_word).unwrap_or("").trim();
                let fallback = if rest.is_empty() { alloc::format!("ls") } else { alloc::format!("ls {}", rest) };
                spawn(&fallback);
            }
        } else if matches!(first_word, "view" | "read" | "show" | "cat") {
            let code = spawn(trimmed);
            if code < 0 {
                let rest = trimmed.strip_prefix(first_word).unwrap_or("").trim();
                let fallback = alloc::format!("cat {}", rest);
                spawn(&fallback);
            }
        } else if matches!(first_word, "print" | "echo") {
            let rest = trimmed.strip_prefix(first_word).unwrap_or("").trim();
            println!("{}", rest);
        } else if matches!(first_word, "find" | "search" | "grep") {
            let code = spawn(trimmed);
            if code < 0 {
                let rest = trimmed.strip_prefix(first_word).unwrap_or("").trim();
                let fallback = alloc::format!("grep {}", rest);
                spawn(&fallback);
            }
        } else if matches!(first_word, "count" | "wc") {
            let code = spawn(trimmed);
            if code < 0 {
                let rest = trimmed.strip_prefix(first_word).unwrap_or("").trim();
                let fallback = alloc::format!("wc {}", rest);
                spawn(&fallback);
            }
        } else if matches!(first_word, "edit" | "calc" | "hello" | "snake") {
            let code = spawn(trimmed);
            if code < 0 {
                println!("\x1b[31mspawn error: failed to run '{}' (code {})\x1b[0m", trimmed, code);
            }
        } else if matches!(first_word,
            "system" | "network" | "storage" | "tasks" | "disks" | "wifi" | "audio" |
            "sound" | "first" | "last" | "sys" | "net" | "mem" |
            "reboot" | "restart" | "halt" | "shutdown" | "poweroff" | "touch" | "rm" |
            "create" | "delete" | "remove" | "make" | "folder" | "nano" | "date" | "uptime" |
            "font" | "desktop" | "gui"
        ) {
            // Route administrative and telemetry commands to kernel subsystem dispatcher
            syscommand(trimmed);
        } else {
            // Attempt to spawn binary from /bin/<command>
            let code = spawn(trimmed);
            if code < 0 {
                println!("sublimesh: command not found: '{}'. Type 'help' for available commands.", trimmed);
            }
        }
    }

    0
}
