#![no_std]
#![no_main]

extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use cosmic_rt::{args, print, println, read, read_to_string, sched_yield, write, write_file};

#[no_mangle]
pub fn main() -> i32 {
    let argv = args();
    if argv.len() < 2 {
        println!("Usage: edit <file>");
        return 1;
    }

    let file_path = argv[1];
    let initial_text = read_to_string(file_path).unwrap_or_else(|_| String::new());

    let mut lines: Vec<String> = if initial_text.is_empty() {
        alloc::vec![String::new()]
    } else {
        initial_text.lines().map(String::from).collect()
    };
    if lines.is_empty() {
        lines.push(String::new());
    }

    // Clear screen and draw initial editor UI
    print!("\x1b[2J\x1b[H");
    println!("\x1b[7m SublimeOS Nano Text Editor (Ring 3 User Mode) - {} \x1b[0m", file_path);
    for (i, line) in lines.iter().enumerate() {
        println!("{:>3} | {}", i + 1, line);
    }
    println!("\x1b[7m [Commands: :w (save), :q (quit), :wq (save & quit), :p (print), <text> (append)] \x1b[0m");

    // Line-mode command loop
    let mut input_buf = [0u8; 256];
    loop {
        print!("edit> ");
        let mut len = 0;
        while len < input_buf.len() - 1 {
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
                if len > 0 {
                    len -= 1;
                    print!("\x08 \x08");
                }
            } else if b >= 32 && b <= 126 {
                input_buf[len] = b;
                len += 1;
                write(1, &[b]);
            }
        }
        input_buf[len] = 0;
        if len == 0 {
            continue;
        }

        let cmd = match core::str::from_utf8(&input_buf[..len]) {
            Ok(s) => s.trim(),
            Err(_) => continue,
        };

        if cmd == ":q" || cmd == ":quit" {
            println!("Exited without saving.");
            break;
        } else if cmd == ":w" || cmd == ":save" {
            let mut full_doc = String::new();
            for (idx, l) in lines.iter().enumerate() {
                if idx > 0 {
                    full_doc.push('\n');
                }
                full_doc.push_str(l);
            }
            full_doc.push('\n');
            match write_file(file_path, full_doc.as_bytes()) {
                Ok(_) => println!("[Wrote {} bytes to {}]", full_doc.len(), file_path),
                Err(e) => println!("Error saving file (code {})", e),
            }
        } else if cmd == ":wq" {
            let mut full_doc = String::new();
            for (idx, l) in lines.iter().enumerate() {
                if idx > 0 {
                    full_doc.push('\n');
                }
                full_doc.push_str(l);
            }
            full_doc.push('\n');
            let _ = write_file(file_path, full_doc.as_bytes());
            println!("[Saved and exited]");
            break;
        } else if cmd == ":p" || cmd == ":print" {
            println!("\x1b[7m --- File: {} --- \x1b[0m", file_path);
            for (i, line) in lines.iter().enumerate() {
                println!("{:>3} | {}", i + 1, line);
            }
        } else {
            lines.push(String::from(cmd));
            println!("Line {}: {}", lines.len(), cmd);
        }
    }

    0
}
