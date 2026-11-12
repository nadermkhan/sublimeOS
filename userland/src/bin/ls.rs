#![no_std]
#![no_main]

extern crate alloc;
use cosmic_rt::{args, print, println, read_dir};

#[no_mangle]
pub fn main() -> i32 {
    let argv = args();
    let target = if argv.len() > 1 { argv[1] } else { "/" };

    match read_dir(target) {
        Ok(entries) => {
            for entry in entries {
                let suffix = if entry.is_dir { "/" } else { "" };
                print!("{}{:<2}  ", entry.name, suffix);
            }
            println!();
            0
        }
        Err(_) => {
            println!("ls: cannot access '{}': No such directory", target);
            1
        }
    }
}
