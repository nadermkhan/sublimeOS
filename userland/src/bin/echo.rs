#![no_std]
#![no_main]

extern crate alloc;
use sublime_rt::{args, print, println};

#[no_mangle]
pub fn main() -> i32 {
    let argv = args();
    if argv.len() > 1 {
        for (i, arg) in argv[1..].iter().enumerate() {
            if i > 0 {
                print!(" ");
            }
            print!("{}", arg);
        }
        println!();
    } else {
        println!();
    }
    0
}
