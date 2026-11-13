#![no_std]
#![no_main]

extern crate alloc;
use sublime_rt::{args, println, read_to_string};

#[no_mangle]
pub fn main() -> i32 {
    let argv = args();
    if argv.len() <= 1 {
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
        let lines = s.lines().count();
        let words = s.split_whitespace().count();
        let bytes = s.len();
        println!("{:>6} {:>6} {:>6}", lines, words, bytes);
        return 0;
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
