#![no_std]
#![no_main]

extern crate alloc;
use sublime_rt::{args, println, read_to_string};

#[no_mangle]
pub fn main() -> i32 {
    let argv = args();
    if argv.len() < 2 {
        println!("Usage: grep <pattern> [file]");
        return 1;
    }

    let pattern = argv[1];
    let content = if argv.len() >= 3 {
        let path = argv[2];
        match read_to_string(path) {
            Ok(s) => s,
            Err(_) => {
                println!("grep: {}: No such file or directory", path);
                return 2;
            }
        }
    } else {
        let mut s = alloc::string::String::new();
        let mut buf = [0u8; 512];
        loop {
            let n = sublime_rt::read(0, &mut buf);
            if n <= 0 {
                break;
            }
            if let Ok(piece) = core::str::from_utf8(&buf[..n as usize]) {
                s.push_str(piece);
            }
        }
        s
    };

    let mut matches = 0;
    for line in content.lines() {
        if line.contains(pattern) {
            println!("{}", line);
            matches += 1;
        }
    }
    if matches > 0 { 0 } else { 1 }
}
