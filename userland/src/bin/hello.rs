#![no_std]
#![no_main]

extern crate alloc;

use alloc::format;
use alloc::vec::Vec;
use cosmic_rt::{getpid, println};

#[no_mangle]
pub fn main() -> i32 {
    println!("\x1b[32m[HELLO]\x1b[0m Genuine Compiled Userland Binary Active (PID {})!", getpid());

    // Test dynamic user heap allocation backed by sys_brk
    let mut numbers = Vec::new();
    for i in 1..=10 {
        numbers.push(i);
    }
    let total: i32 = numbers.iter().sum();
    let message = format!("User heap test: Vector sum [1..10] = {}", total);
    println!("\x1b[32m[HELLO]\x1b[0m {}", message);
    println!("\x1b[32m[HELLO]\x1b[0m Exit code: 0");
    0
}
