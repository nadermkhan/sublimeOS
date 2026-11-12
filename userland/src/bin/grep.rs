#![no_std]
#![no_main]

extern crate alloc;
use sublime_rt::{args, println, read_to_string};

#[no_mangle]
pub fn main() -> i32 {
    let argv = args();
    if argv.len() < 2 {
        println!("Usage: grep <pattern> <file>");
        return 1;
    }

    let pattern = argv[1];
    if argv.len() >= 3 {
        let path = argv[2];
        match read_to_string(path) {
            Ok(content) => {
                let mut matches = 0;
                for line in content.lines() {
                    if line.contains(pattern) {
                        println!("{}", line);
                        matches += 1;
                    }
                }
                if matches > 0 { 0 } else { 1 }
            }
            Err(_) => {
                println!("grep: {}: No such file or directory", path);
                2
            }
        }
    } else {
        println!("Usage: grep <pattern> <file>");
        1
    }
}
