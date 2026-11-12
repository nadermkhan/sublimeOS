#![no_std]
#![no_main]

use sublime_rt::println;

#[no_mangle]
pub fn main() -> i32 {
    println!("\x1b[2J\x1b[H\x1b[32m+----------------------------------------------+\x1b[0m");
    println!("\x1b[32m|\x1b[0m    \x1b[1;33mSublimeOS Snake - Ring 3 Terminal Game\x1b[0m     \x1b[32m|\x1b[0m");
    println!("\x1b[32m|\x1b[0m    Score: \x1b[36m120\x1b[0m   Speed: \x1b[36mNormal\x1b[0m   Lives: \x1b[31m3\x1b[0m       \x1b[32m|\x1b[0m");
    println!("\x1b[32m+----------------------------------------------+\x1b[0m");
    println!("\x1b[32m|\x1b[0m        \x1b[32m@@@@@>\x1b[0m                                \x1b[32m|\x1b[0m");
    println!("\x1b[32m|\x1b[0m                         \x1b[31m*\x1b[0m (food)              \x1b[32m|\x1b[0m");
    println!("\x1b[32m+----------------------------------------------+\x1b[0m");
    0
}
