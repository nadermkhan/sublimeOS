#![no_std]
#![no_main]

use cosmic_rt::{getpid, sleep_ms, spawn};

#[no_mangle]
pub fn main() -> i32 {
    let pid = getpid();
    if pid == 1 {
        // PID 1 Master System Init: supervisor of userspace
        loop {
            let code = spawn("/bin/sh");
            if code < 0 {
                sleep_ms(500);
            }
        }
    } else {
        0
    }
}
