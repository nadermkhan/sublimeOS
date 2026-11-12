#![no_std]
#![no_main]

extern crate alloc;
use sublime_rt::{args, println, read_to_string};

#[no_mangle]
pub fn main() -> i32 {
    let argv = args();
    if argv.len() < 2 {
        println!("Usage: wc <file>");
        return 1;
    }

    let mut exit_code = 0;
    for &path in &argv[1..] {
        match read_to_string(path) {
            Ok(content) => {
                let lines = content.lines().count();
                let words = content.split_whitespace().count();
                let bytes = content.len();
                println!("{:>6} {:>6} {:>6} {}", lines, words, bytes, path);
            }
            Err(_) => {
                println!("wc: {}: No such file or directory", path);
                exit_code = 1;
            }
        }
    }

    exit_code
}
