#![no_std]
#![no_main]

extern crate alloc;
use cosmic_rt::{args, close, open, println, read, write, O_RDONLY};

#[no_mangle]
pub fn main() -> i32 {
    let argv = args();
    if argv.len() <= 1 {
        let mut buf = [0u8; 512];
        loop {
            let n = read(0, &mut buf);
            if n <= 0 {
                break;
            }
            write(1, &buf[..n as usize]);
        }
        return 0;
    }

    let mut exit_code = 0;
    for &path in &argv[1..] {
        let fd = open(path, O_RDONLY, 0);
        if fd < 0 {
            println!("cat: {}: No such file or directory", path);
            exit_code = 1;
            continue;
        }

        let mut buf = [0u8; 512];
        loop {
            let n = read(fd as usize, &mut buf);
            if n <= 0 {
                break;
            }
            write(1, &buf[..n as usize]);
        }
        close(fd as usize);
    }

    exit_code
}
