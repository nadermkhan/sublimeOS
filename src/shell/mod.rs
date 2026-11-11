pub mod commands;

extern crate alloc;
use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;
use crate::print;
use crate::println;

static INPUT_QUEUE: crate::sync::IrqSafeSpinlock<VecDeque<u8>> =
    crate::sync::IrqSafeSpinlock::new(VecDeque::new());

fn get_next_input_char() -> Option<u8> {
    if let Some(ch) = crate::drivers::keyboard::read_char() {
        return Some(ch);
    }

    // Drain available bytes from serial FIFO into in-memory queue
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

const COMMAND_LIST: [&str; 18] = [
    "help", "clear", "uname", "uptime", "date", "mem", "ps", "tasks",
    "lspci", "ls", "cat", "echo", "mkdir", "touch", "rm", "gui",
    "test", "reboot"
];

pub fn run_shell() -> ! {
    println!("\x1b[36m");
    println!(r"
     ____                                _  __                     _ 
    / ___|___  ___ _ __ ___   ___  ___  | |/ /___ _ __ _ __   ___| |
   | |   / _ \/ __| '_ ` _ \ / _ \/ __| | ' // _ \ '__| '_ \ / _ \ |
   | |__| (_) \__ \ | | | | | (_) \__ \ | . \  __/ |  | | | |  __/ |
    \____\___/|___/_| |_| |_|\___/|___/ |_|\_\___|_|  |_| |_|\___|_|
    ");
    println!("\x1b[0m");
    println!("CosmosKernel Monolithic TTY v0.1.0 (x86_64 bare-metal)");
    println!("Virtual Consoles: Alt+F1 (tty1) | Alt+F2 (tty2) | Alt+F3 (dmesg) | Alt+F4 (top)");
    println!("Type 'help' for commands, TAB to autocomplete, Up/Down for history.\n");

    let prompt = "\x1b[32mcosmos\x1b[0m:\x1b[34m/\x1b[0m# ";
    print!("{}", prompt);

    let mut line_buffer: Vec<char> = Vec::with_capacity(256);
    let mut cursor_pos: usize = 0;
    let mut history: Vec<String> = Vec::new();
    let mut history_index: Option<usize> = None;
    let mut temp_line = String::new();

    // Escape sequence state for incoming arrow keys (\x1b[A, \x1b[B, etc.)
    let mut esc_state = 0; // 0: normal, 1: saw 0x1B, 2: saw '['

    loop {
        if let Some(byte) = get_next_input_char() {
            // Handle ANSI Escape sequences from keyboard/serial (e.g. Arrow keys)
            if esc_state == 1 {
                if byte == b'[' {
                    esc_state = 2;
                    continue;
                } else {
                    esc_state = 0;
                }
            } else if esc_state == 2 {
                esc_state = 0;
                match byte {
                    b'A' => {
                        // UP Arrow -> Previous history
                        if !history.is_empty() {
                            let new_idx = match history_index {
                                None => {
                                    temp_line = line_buffer.iter().collect();
                                    history.len() - 1
                                }
                                Some(idx) if idx > 0 => idx - 1,
                                Some(idx) => idx,
                            };
                            history_index = Some(new_idx);

                            // Clear current line from screen
                            clear_current_line(&line_buffer, cursor_pos, prompt);
                            line_buffer = history[new_idx].chars().collect();
                            cursor_pos = line_buffer.len();
                            print_chars(&line_buffer);
                        }
                        continue;
                    }
                    b'B' => {
                        // DOWN Arrow -> Next history
                        if let Some(idx) = history_index {
                            clear_current_line(&line_buffer, cursor_pos, prompt);
                            if idx + 1 < history.len() {
                                let new_idx = idx + 1;
                                history_index = Some(new_idx);
                                line_buffer = history[new_idx].chars().collect();
                            } else {
                                history_index = None;
                                line_buffer = temp_line.chars().collect();
                            }
                            cursor_pos = line_buffer.len();
                            print_chars(&line_buffer);
                        }
                        continue;
                    }
                    b'C' => {
                        // RIGHT Arrow -> Move cursor forward
                        if cursor_pos < line_buffer.len() {
                            print!("{}", line_buffer[cursor_pos]);
                            cursor_pos += 1;
                        }
                        continue;
                    }
                    b'D' => {
                        // LEFT Arrow -> Move cursor backward
                        if cursor_pos > 0 {
                            cursor_pos -= 1;
                            print!("\x08");
                        }
                        continue;
                    }
                    b'H' => {
                        // HOME -> Move cursor to start of line
                        while cursor_pos > 0 {
                            print!("\x08");
                            cursor_pos -= 1;
                        }
                        continue;
                    }
                    b'F' => {
                        // END -> Move cursor to end of line
                        while cursor_pos < line_buffer.len() {
                            print!("{}", line_buffer[cursor_pos]);
                            cursor_pos += 1;
                        }
                        continue;
                    }
                    b'3' => {
                        // Saw \x1b[3, wait for ~ (Delete key)
                        let _ = get_next_input_char(); // consume '~'
                        if cursor_pos < line_buffer.len() {
                            line_buffer.remove(cursor_pos);
                            redraw_tail(&line_buffer, cursor_pos);
                        }
                        continue;
                    }
                    _ => continue,
                }
            }

            match byte {
                0x1B => {
                    esc_state = 1;
                }
                b'\n' => {
                    println!();
                    let cmd_str: String = line_buffer.iter().collect();
                    let trimmed = cmd_str.trim();
                    if !trimmed.is_empty() {
                        if history.last().map(|s| s.as_str()) != Some(trimmed) {
                            history.push(String::from(trimmed));
                        }
                        commands::execute_command(trimmed);
                    }
                    line_buffer.clear();
                    cursor_pos = 0;
                    history_index = None;
                    print!("{}", prompt);
                }
                0x08 | 0x7F => {
                    // Backspace
                    if cursor_pos > 0 {
                        cursor_pos -= 1;
                        line_buffer.remove(cursor_pos);
                        print!("\x08");
                        redraw_tail(&line_buffer, cursor_pos);
                    }
                }
                b'\t' => {
                    // TAB -> Autocompletion
                    let curr: String = line_buffer.iter().take(cursor_pos).collect();
                    let prefix = curr.trim_start();
                    let mut matches = Vec::new();

                    for &cmd in COMMAND_LIST.iter() {
                        if cmd.starts_with(prefix) {
                            matches.push(cmd);
                        }
                    }

                    if matches.len() == 1 {
                        let full = matches[0];
                        let to_add = &full[prefix.len()..];
                        for c in to_add.chars() {
                            line_buffer.insert(cursor_pos, c);
                            cursor_pos += 1;
                        }
                        print!("{}", to_add);
                    } else if matches.len() > 1 {
                        println!();
                        for m in matches {
                            print!("  {}  ", m);
                        }
                        println!();
                        print!("{}", prompt);
                        print_chars(&line_buffer);
                    }
                }
                0x03 => {
                    // Ctrl+C -> Cancel line
                    println!("^C");
                    line_buffer.clear();
                    cursor_pos = 0;
                    history_index = None;
                    print!("{}", prompt);
                }
                0x0C => {
                    // Ctrl+L -> Clear screen
                    print!("\x1b[2J\x1b[H");
                    print!("{}", prompt);
                    print_chars(&line_buffer);
                }
                0x15 => {
                    // Ctrl+U -> Erase line before cursor
                    clear_current_line(&line_buffer, cursor_pos, prompt);
                    line_buffer.drain(0..cursor_pos);
                    cursor_pos = 0;
                    print_chars(&line_buffer);
                }
                0x20..=0x7E => {
                    // Printable ASCII character
                    let c = byte as char;
                    if cursor_pos == line_buffer.len() {
                        line_buffer.push(c);
                        cursor_pos += 1;
                        print!("{}", c);
                    } else {
                        line_buffer.insert(cursor_pos, c);
                        cursor_pos += 1;
                        print!("{}", c);
                        redraw_tail(&line_buffer, cursor_pos);
                    }
                }
                _ => {}
            }
        } else {
            core::hint::spin_loop();
        }
    }
}

fn print_chars(chars: &[char]) {
    for &c in chars {
        print!("{}", c);
    }
}

fn clear_current_line(buf: &[char], cursor_pos: usize, prompt: &str) {
    while cursor_pos > 0 {
        print!("\x08");
    }
    print!("\r");
    print!("{}", prompt);
    for _ in 0..buf.len() {
        print!(" ");
    }
    print!("\r");
    print!("{}", prompt);
}

fn redraw_tail(buf: &[char], from: usize) {
    for i in from..buf.len() {
        print!("{}", buf[i]);
    }
    print!(" ");
    for _ in from..=buf.len() {
        print!("\x08");
    }
}
