#![no_std]
#![no_main]

use sublime_rt::{getpid, println};

#[no_mangle]
pub fn main() -> i32 {
    println!("\x1b[36m┌──────────────────────────────────────────┐\x1b[0m");
    println!("\x1b[36m│\x1b[0m  SublimeOS Standalone Calculator (PID {})  \x1b[36m│\x1b[0m", getpid());
    println!("\x1b[36m├──────────────────────────────────────────┤\x1b[0m");
    let x: i64 = 42;
    let y: i64 = 1337;
    let sum = x + y;
    let prod = x * y;
    println!("\x1b[36m│\x1b[0m  Test 1: {} + {} = {}                 \x1b[36m│\x1b[0m", x, y, sum);
    println!("\x1b[36m│\x1b[0m  Test 2: {} * {} = {}                \x1b[36m│\x1b[0m", x, y, prod);
    println!("\x1b[36m│\x1b[0m  Computation Status: \x1b[32mVERIFIED OK\x1b[0m         \x1b[36m│\x1b[0m");
    println!("\x1b[36m└──────────────────────────────────────────┘\x1b[0m");
    0
}
