pub mod commands;

extern crate alloc;
use alloc::string::String;
use crate::print;
use crate::println;

static INPUT_QUEUE: crate::sync::IrqSafeSpinlock<alloc::collections::VecDeque<u8>> =
    crate::sync::IrqSafeSpinlock::new(alloc::collections::VecDeque::new());

fn get_next_input_char() -> Option<u8> {
    if let Some(ch) = crate::drivers::keyboard::read_char() {
        return Some(ch);
    }

    // Drain all available bytes from serial FIFO into in-memory queue immediately
    {
        let serial = crate::arch::x86_64::serial::SERIAL1.lock();
        let mut queue = INPUT_QUEUE.lock();
        while serial.is_data_ready() {
            let b = serial.read_byte();
            if b == b'\r' {
                queue.push_back(b'\n');
            } else if b == b'\n' {
                if queue.back() != Some(&b'\n') {
                    queue.push_back(b'\n');
                }
            } else {
                queue.push_back(b);
            }
        }
    }

    INPUT_QUEUE.lock().pop_front()
}

pub fn run_shell() -> ! {
    println!("\x1b[32m");
    println!(r"
     ____                                _  __                     _ 
    / ___|___  ___ _ __ ___   ___  ___  | |/ /___ _ __ _ __   ___| |
   | |   / _ \/ __| '_ ` _ \ / _ \/ __| | ' // _ \ '__| '_ \ / _ \ |
   | |__| (_) \__ \ | | | | | (_) \__ \ | . \  __/ |  | | | |  __/ |
    \____\___/|___/_| |_| |_|\___/|___/ |_|\_\___|_|  |_| |_|\___|_|
    ");
    println!("\x1b[0m");
    println!("Type 'help' to see the command list or 'test' to run kernel verification.\n");

    let mut line = String::new();
    print!("\x1b[32mcosmos\x1b[0m:\x1b[34m/\x1b[0m# ");

    loop {
        if let Some(ch) = get_next_input_char() {
            match ch {
                b'\n' => {
                    println!();
                    commands::execute_command(&line);
                    line.clear();
                    print!("\x1b[32mcosmos\x1b[0m:\x1b[34m/\x1b[0m# ");
                }
                0x08 | 0x7F => {
                    // Backspace / DEL
                    if !line.is_empty() {
                        line.pop();
                        print!("\x08 \x08");
                    }
                }
                0x20..=0x7E => {
                    line.push(ch as char);
                    print!("{}", ch as char);
                }
                _ => {}
            }
        } else {
            core::hint::spin_loop();
        }
    }
}
