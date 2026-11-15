#![no_std]
#![no_main]

use sublime_rt::{getpid, sleep_ms, spawn, syscommand};

#[no_mangle]
pub fn main() -> i32 {
    let pid = getpid();
    if pid == 1 {
        // PID 1 Master System Init: boot straight into SublimeOS Desktop GUI by default
        let _ = syscommand("desktop");

        // Fallback / post-GUI supervisor of userspace shell
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
