#![no_std]
#![no_main]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use sublime_rt::{
    chdir, close, dup, dup2, getcwd, mkdir, open, pipe, print, println, read, read_dir, sleep_ms,
    spawn, spawn_bg, sync, syscommand, unlink, O_APPEND, O_CREAT, O_RDONLY, O_TRUNC, O_WRONLY,
};

fn redraw_line(prompt: &str, line: &[char], cursor: usize) {
    let s: String = line.iter().collect();
    print!("\r\x1b[K{}{}", prompt, s);
    let dist = line.len().saturating_sub(cursor);
    if dist > 0 {
        print!("\x1b[{}D", dist);
    }
}

fn handle_tab_completion(prompt: &str, line: &mut Vec<char>, cursor: &mut usize) {
    let curr_str: String = line.iter().take(*cursor).collect();
    let token_start = match curr_str.rfind(|c: char| c.is_whitespace()) {
        Some(pos) => pos + 1,
        None => 0,
    };
    let token = &curr_str[token_start..];

    static COMMANDS: &[&str] = &[
        "help", "about", "hardware", "tasks", "storage", "display", "path",
        "clear", "network", "audio", "power", "desktop", "calc", "snake",
        "exit", "quit", "list", "view", "print", "find", "count", "edit",
        "create", "delete", "make", "ls", "cat", "echo", "grep", "wc",
        "hello", "pwd", "sync", "exec", "uptime", "date", "touch", "rm", "mkdir", "cd"
    ];

    let mut matches = Vec::new();

    if token_start == 0 {
        for &cmd in COMMANDS.iter() {
            if cmd.starts_with(token) {
                matches.push(String::from(cmd));
            }
        }
        if let Ok(entries) = read_dir("/bin") {
            for entry in entries {
                if entry.name.starts_with(token) && !matches.iter().any(|m| m == &entry.name) {
                    matches.push(entry.name);
                }
            }
        }
    } else {
        let (dir_path, name_prefix) = match token.rfind('/') {
            Some(pos) => {
                let d = if pos == 0 { "/" } else { &token[..pos] };
                let p = &token[pos + 1..];
                (String::from(d), p)
            }
            None => (getcwd(), token),
        };

        if let Ok(entries) = read_dir(&dir_path) {
            for entry in entries {
                if entry.name.starts_with(name_prefix) {
                    let full = if token.contains('/') {
                        let base = if dir_path == "/" { "/" } else { &dir_path };
                        if base == "/" {
                            alloc::format!("/{}{}", entry.name, if entry.is_dir { "/" } else { "" })
                        } else {
                            alloc::format!("{}/{}{}", base, entry.name, if entry.is_dir { "/" } else { "" })
                        }
                    } else {
                        alloc::format!("{}{}", entry.name, if entry.is_dir { "/" } else { "" })
                    };
                    matches.push(full);
                }
            }
        }
    }

    if matches.len() == 1 {
        let completion = &matches[0];
        let suffix = if completion.ends_with('/') { "" } else { " " };
        line.drain(token_start..*cursor);
        let mut insert_pos = token_start;
        for c in completion.chars() {
            line.insert(insert_pos, c);
            insert_pos += 1;
        }
        for c in suffix.chars() {
            line.insert(insert_pos, c);
            insert_pos += 1;
        }
        *cursor = insert_pos;
        redraw_line(prompt, line, *cursor);
    } else if matches.len() > 1 {
        println!();
        for m in &matches {
            print!("  \x1b[36m{}\x1b[0m", m);
        }
        println!();
        redraw_line(prompt, line, *cursor);
    }
}

fn read_line(prompt: &str, history: &mut Vec<String>) -> Option<String> {
    let mut line: Vec<char> = Vec::with_capacity(256);
    let mut cursor: usize = 0;
    let mut history_index: Option<usize> = None;
    let mut temp_line = String::new();
    let mut esc_state = 0;

    loop {
        let mut ch = [0u8; 1];
        let n = read(0, &mut ch);
        if n <= 0 {
            sleep_ms(5);
            continue;
        }
        let byte = ch[0];

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
                    // UP Arrow: Previous history
                    if !history.is_empty() {
                        let new_idx = match history_index {
                            None => {
                                temp_line = line.iter().collect();
                                history.len() - 1
                            }
                            Some(idx) if idx > 0 => idx - 1,
                            Some(idx) => idx,
                        };
                        history_index = Some(new_idx);
                        line = history[new_idx].chars().collect();
                        cursor = line.len();
                        redraw_line(prompt, &line, cursor);
                    }
                    continue;
                }
                b'B' => {
                    // DOWN Arrow: Next history
                    if let Some(idx) = history_index {
                        if idx + 1 < history.len() {
                            let new_idx = idx + 1;
                            history_index = Some(new_idx);
                            line = history[new_idx].chars().collect();
                        } else {
                            history_index = None;
                            line = temp_line.chars().collect();
                        }
                        cursor = line.len();
                        redraw_line(prompt, &line, cursor);
                    }
                    continue;
                }
                b'C' => {
                    // RIGHT Arrow: Move cursor forward
                    if cursor < line.len() {
                        cursor += 1;
                        print!("\x1b[1C");
                    }
                    continue;
                }
                b'D' => {
                    // LEFT Arrow: Move cursor backward
                    if cursor > 0 {
                        cursor -= 1;
                        print!("\x1b[1D");
                    }
                    continue;
                }
                b'H' => {
                    // HOME
                    if cursor > 0 {
                        print!("\x1b[{}D", cursor);
                        cursor = 0;
                    }
                    continue;
                }
                b'F' => {
                    // END
                    if cursor < line.len() {
                        print!("\x1b[{}C", line.len() - cursor);
                        cursor = line.len();
                    }
                    continue;
                }
                b'3' => {
                    // Delete key: consume '~'
                    let mut tilde = [0u8; 1];
                    let _ = read(0, &mut tilde);
                    if cursor < line.len() {
                        line.remove(cursor);
                        redraw_line(prompt, &line, cursor);
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
            b'\r' | b'\n' => {
                println!();
                break;
            }
            0x08 | 0x7F => {
                // Backspace
                if cursor > 0 {
                    cursor -= 1;
                    line.remove(cursor);
                    redraw_line(prompt, &line, cursor);
                }
            }
            b'\t' => {
                // Tab Autocompletion
                handle_tab_completion(prompt, &mut line, &mut cursor);
            }
            0x01 => {
                // Ctrl-A: Home
                if cursor > 0 {
                    print!("\x1b[{}D", cursor);
                    cursor = 0;
                }
            }
            0x05 => {
                // Ctrl-E: End
                if cursor < line.len() {
                    print!("\x1b[{}C", line.len() - cursor);
                    cursor = line.len();
                }
            }
            0x03 => {
                // Ctrl-C: Cancel line
                println!("^C");
                line.clear();
                cursor = 0;
                history_index = None;
                print!("{}", prompt);
            }
            0x0C => {
                // Ctrl-L: Clear screen
                print!("\x1b[2J\x1b[H");
                redraw_line(prompt, &line, cursor);
            }
            0x15 => {
                // Ctrl-U: Clear line before cursor
                line.drain(0..cursor);
                cursor = 0;
                redraw_line(prompt, &line, cursor);
            }
            32..=126 => {
                // Printable ASCII character
                let c = byte as char;
                if cursor == line.len() {
                    line.push(c);
                    cursor += 1;
                    print!("{}", c);
                } else {
                    line.insert(cursor, c);
                    cursor += 1;
                    redraw_line(prompt, &line, cursor);
                }
            }
            _ => {}
        }
    }

    let result: String = line.into_iter().collect();
    Some(result)
}

fn parse_redirection(stage: &str) -> (&str, Option<&str>, Option<&str>, bool) {
    let mut stdin_file = None;
    let mut stdout_file = None;
    let mut append = false;

    let in_pos = stage.find('<');
    let out_append_pos = stage.find(">>");
    let out_trunc_pos = if out_append_pos.is_none() { stage.find('>') } else { None };

    let mut min_redir_pos = stage.len();
    if let Some(pos) = in_pos {
        if pos < min_redir_pos { min_redir_pos = pos; }
    }
    if let Some(pos) = out_append_pos {
        if pos < min_redir_pos { min_redir_pos = pos; }
    }
    if let Some(pos) = out_trunc_pos {
        if pos < min_redir_pos { min_redir_pos = pos; }
    }

    let cmd_part = stage[..min_redir_pos].trim();

    if let Some(pos) = in_pos {
        let after = &stage[pos + 1..];
        let file = after.split_whitespace().next().unwrap_or("").trim();
        if !file.is_empty() {
            stdin_file = Some(file);
        }
    }

    if let Some(pos) = out_append_pos {
        append = true;
        let after = &stage[pos + 2..];
        let file = after.split_whitespace().next().unwrap_or("").trim();
        if !file.is_empty() {
            stdout_file = Some(file);
        }
    } else if let Some(pos) = out_trunc_pos {
        append = false;
        let after = &stage[pos + 1..];
        let file = after.split_whitespace().next().unwrap_or("").trim();
        if !file.is_empty() {
            stdout_file = Some(file);
        }
    }

    (cmd_part, stdin_file, stdout_file, append)
}

fn execute_single_cmd(trimmed: &str) {
    let trimmed = trimmed.trim();
    if trimmed.is_empty() {
        return;
    }

    let first_word = trimmed.split_whitespace().next().unwrap_or("");
    let rest = trimmed.strip_prefix(first_word).unwrap_or("").trim();

    if trimmed == "help" {
        println!("\x1b[1;36mSublimeOS\x1b[0m 1.0 — \x1b[35mKiteKernel\x1b[0m");
        println!("Developer: Nader Mahbub Khan | Copyright (c) 2026\n");
        println!("\x1b[1mAvailable Commands:\x1b[0m\n");
        println!("  \x1b[1;36mFiles & Workspace:\x1b[0m");
        println!("    list [path]         List files in directory (or 'files', 'ls')");
        println!("    view <file...>      Display contents of text file (or 'read', 'cat')");
        println!("    print [text...]     Print text to screen (or 'echo')");
        println!("    find <pattern> [f]  Search text matching pattern (or 'grep')");
        println!("    count [file...]     Count lines, words, and characters (or 'wc')");
        println!("    edit <file>         Full-screen text editor");
        println!("    create <file>       Create new file (or 'touch')");
        println!("    delete <file>       Remove file (or 'rm')");
        println!("    make folder <path>  Create new directory (or 'mkdir')");
        println!();
        println!("  \x1b[1;36mSystem & Device:\x1b[0m");
        println!("    about               System & kernel release information (or 'uname')");
        println!("    hardware            Silicon hardware & firmware specifications");
        println!("    tasks [list|kill]   Process manager and task control (or 'ps')");
        println!("    storage [list|sync] Storage drives and buffer cache");
        println!("    display [1|2]       Console font size & HiDPI (Ctrl+= / Ctrl+-)");
        println!("    cd [path]           Change working directory (or 'cd ..', 'cd /')");
        println!("    path                Print current working directory (or 'pwd')");
        println!("    clear               Clear screen");
        println!();
        println!("  \x1b[1;36mConnectivity & Audio:\x1b[0m");
        println!("    network [wifi|ping] Network interfaces, Wi-Fi scanner, and ping");
        println!("    audio [chime|play]  Audio chime and WAV playback");
        println!("    power [off|restart] Safely shut down or restart computer");
        println!();
        println!("  \x1b[1;36mApplications & Pipelines:\x1b[0m");
        println!("    desktop             Aqua graphical window compositor");
        println!("    calc                64-bit expression calculator");
        println!("    snake               Classic arcade game");
        println!("    <cmd1> | <cmd2>     UNIX pipeline between processes");
        println!("    <cmd> > <file>      Redirect stdout to file (truncate)");
        println!("    <cmd> >> <file>     Redirect stdout to file (append)");
        println!("    <cmd> < <file>      Redirect stdin from file");
        println!("    exit                Exit shell\n");
    } else if trimmed == "clear" {
        print!("\x1b[2J\x1b[H");
    } else if matches!(trimmed, "path" | "where" | "pwd") {
        println!("{}", getcwd());
    } else if first_word == "cd" {
        let target = if rest.is_empty() || rest == "~" {
            "/"
        } else {
            rest
        };
        let res = chdir(target);
        if res < 0 {
            if res == -2 {
                println!("cd: {}: No such file or directory", target);
            } else if res == -20 {
                println!("cd: {}: Not a directory", target);
            } else {
                println!("cd: {}: error {}", target, res);
            }
        }
    } else if matches!(trimmed, "about" | "uname") {
        println!("SublimeOS 1.0.0 (x86_64) — KiteKernel");
        println!("Developer: Nader Mahbub Khan | Copyright (c) 2026");
    } else if matches!(trimmed, "hardware" | "specs") {
        syscommand("hardware");
    } else if matches!(first_word, "tasks" | "ps") {
        syscommand("tasks");
    } else if matches!(first_word, "storage" | "disks") {
        syscommand("storage");
    } else if trimmed == "sync" {
        sync();
        println!("Storage cache synced.");
    } else if first_word == "power" {
        if rest == "off" || rest == "shutdown" || rest == "halt" {
            syscommand("shutdown");
        } else if rest == "restart" || rest == "reboot" || rest == "reset" {
            syscommand("reboot");
        } else {
            println!("Usage: power [off | restart]");
        }
    } else if matches!(first_word, "shutdown" | "poweroff" | "halt") {
        syscommand("shutdown");
    } else if matches!(first_word, "reboot" | "restart") {
        syscommand("reboot");
    } else if first_word == "display" || first_word == "screen" {
        let arg = if rest.is_empty() { "1" } else { rest };
        let mut cmd = String::from("font scale ");
        cmd.push_str(arg);
        syscommand(&cmd);
    } else if matches!(first_word, "print" | "echo") {
        println!("{}", rest);
    } else if first_word == "touch" || (first_word == "create" && !rest.starts_with("folder")) {
        let target = if first_word == "create" && rest.starts_with("file ") {
            rest.strip_prefix("file ").unwrap().trim()
        } else {
            rest
        };
        if target.is_empty() {
            println!("Usage: create <file> (or touch <file>)");
        } else {
            let fd = open(target, O_WRONLY | O_CREAT, 0o644);
            if fd >= 0 {
                close(fd as usize);
                println!("Created {}", target);
            } else {
                println!("create: failed to create '{}' (error {})", target, fd);
            }
        }
    } else if matches!(first_word, "rm" | "delete" | "remove") {
        let target = rest;
        if target.is_empty() {
            println!("Usage: delete <file> (or rm <file>)");
        } else {
            let res = unlink(target);
            if res == 0 {
                println!("Deleted {}", target);
            } else {
                println!("delete: failed to remove '{}' (error {})", target, res);
            }
        }
    } else if first_word == "mkdir" || (first_word == "make" && rest.starts_with("folder")) {
        let target = if first_word == "make" {
            rest.strip_prefix("folder").unwrap().trim()
        } else {
            rest
        };
        if target.is_empty() {
            println!("Usage: make folder <path> (or mkdir <path>)");
        } else {
            let res = mkdir(target, 0o755);
            if res == 0 {
                println!("Created folder {}", target);
            } else {
                println!("mkdir: failed to create folder '{}' (error {})", target, res);
            }
        }
    } else if first_word == "exec" || first_word == "run" {
        if !rest.is_empty() {
            let code = spawn(rest);
            if code < 0 {
                println!("\x1b[31mexec error: failed to spawn '{}' (code {})\x1b[0m", rest, code);
            } else {
                println!("\x1b[32m[Process Terminated] Exit code: {}\x1b[0m", code);
            }
        }
    } else if matches!(first_word, "list" | "files" | "ls") {
        let cmd_to_spawn = if first_word == "ls" {
            String::from(trimmed)
        } else if rest.is_empty() {
            String::from("ls")
        } else {
            alloc::format!("ls {}", rest)
        };
        let code = spawn(&cmd_to_spawn);
        if code < 0 {
            let target_dir = if rest.is_empty() { "/" } else { rest };
            match read_dir(target_dir) {
                Ok(entries) => {
                    for entry in entries {
                        if entry.is_dir {
                            println!("  \x1b[1;34m{}/\x1b[0m", entry.name);
                        } else {
                            println!("  {}", entry.name);
                        }
                    }
                }
                Err(e) => println!("ls: {}: cannot access (error {})", target_dir, e),
            }
        }
    } else if matches!(first_word, "view" | "read" | "show" | "cat") {
        let cmd_to_spawn = if first_word == "cat" {
            String::from(trimmed)
        } else {
            alloc::format!("cat {}", rest)
        };
        let code = spawn(&cmd_to_spawn);
        if code < 0 {
            println!("view: failed to open/read '{}'", rest);
        }
    } else if matches!(first_word, "find" | "search" | "grep") {
        let cmd_to_spawn = if first_word == "grep" {
            String::from(trimmed)
        } else {
            alloc::format!("grep {}", rest)
        };
        let code = spawn(&cmd_to_spawn);
        if code < 0 {
            println!("find: grep failed for '{}'", rest);
        }
    } else if matches!(first_word, "count" | "wc") {
        let cmd_to_spawn = if first_word == "wc" {
            String::from(trimmed)
        } else {
            alloc::format!("wc {}", rest)
        };
        let code = spawn(&cmd_to_spawn);
        if code < 0 {
            println!("count: wc failed for '{}'", rest);
        }
    } else if matches!(first_word, "edit" | "calc" | "hello" | "snake") {
        let code = spawn(trimmed);
        if code < 0 {
            println!("\x1b[31mspawn error: failed to run '{}' (code {})\x1b[0m", trimmed, code);
        }
    } else if matches!(first_word, "network" | "audio" | "sound" | "desktop" | "gui") {
        syscommand(trimmed);
    } else {
        let code = spawn(trimmed);
        if code < 0 {
            let sys_code = syscommand(trimmed);
            if sys_code < 0 {
                println!("sublimesh: command not found: '{}'. Type 'help' for available commands.", trimmed);
            }
        }
    }
}

fn execute_stage_with_redirection(stage: &str, pipe_in: Option<i32>, pipe_out: Option<i32>) {
    let (cmd, redir_in, redir_out, append) = parse_redirection(stage);
    if cmd.is_empty() {
        return;
    }

    if redir_in.is_none() && redir_out.is_none() && pipe_in.is_none() && pipe_out.is_none() {
        execute_single_cmd(cmd);
        return;
    }

    let mut in_fd_to_use = None;
    let mut close_in_fd = false;
    if let Some(file_path) = redir_in {
        let fd = open(file_path, O_RDONLY, 0);
        if fd < 0 {
            println!("sublimesh: {}: No such file or directory", file_path);
            return;
        }
        in_fd_to_use = Some(fd as i32);
        close_in_fd = true;
    } else if let Some(p_in) = pipe_in {
        in_fd_to_use = Some(p_in);
    }

    let mut out_fd_to_use = None;
    let mut close_out_fd = false;
    if let Some(file_path) = redir_out {
        let flags = O_WRONLY | O_CREAT | (if append { O_APPEND } else { O_TRUNC });
        let fd = open(file_path, flags, 0o644);
        if fd < 0 {
            println!("sublimesh: failed to open '{}' for writing", file_path);
            if close_in_fd {
                if let Some(fd_in) = in_fd_to_use {
                    close(fd_in as usize);
                }
            }
            return;
        }
        out_fd_to_use = Some(fd as i32);
        close_out_fd = true;
    } else if let Some(p_out) = pipe_out {
        out_fd_to_use = Some(p_out);
    }

    let saved_stdin = dup(0);
    let saved_stdout = dup(1);

    if let Some(in_fd) = in_fd_to_use {
        dup2(in_fd as usize, 0);
    }
    if let Some(out_fd) = out_fd_to_use {
        dup2(out_fd as usize, 1);
    }

    execute_single_cmd(cmd);

    if saved_stdout >= 0 {
        dup2(saved_stdout as usize, 1);
        close(saved_stdout as usize);
    }
    if saved_stdin >= 0 {
        dup2(saved_stdin as usize, 0);
        close(saved_stdin as usize);
    }

    if close_in_fd {
        if let Some(fd) = in_fd_to_use {
            close(fd as usize);
        }
    }
    if close_out_fd {
        if let Some(fd) = out_fd_to_use {
            close(fd as usize);
        }
    }
}

fn execute_pipeline(cmd_line: &str) {
    let stages: Vec<&str> = cmd_line
        .split('|')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    if stages.is_empty() {
        return;
    }

    if stages.len() == 1 {
        execute_stage_with_redirection(stages[0], None, None);
        return;
    }

    let mut prev_pipe_read: Option<i32> = None;

    for (i, stage) in stages.iter().enumerate() {
        let is_last = i == stages.len() - 1;
        let mut curr_pipe = [0i32; 2];
        if !is_last {
            if pipe(&mut curr_pipe) < 0 {
                println!("pipe creation failed");
                if let Some(r) = prev_pipe_read {
                    close(r as usize);
                }
                return;
            }
        }

        let next_write = if !is_last { Some(curr_pipe[1]) } else { None };
        execute_stage_with_redirection(stage, prev_pipe_read, next_write);

        if let Some(r) = prev_pipe_read {
            close(r as usize);
        }
        if let Some(w) = next_write {
            close(w as usize);
        }

        prev_pipe_read = if !is_last { Some(curr_pipe[0]) } else { None };
    }

    if let Some(r) = prev_pipe_read {
        close(r as usize);
    }
}

#[no_mangle]
pub fn main() -> i32 {
    println!("\x1b[1;36mSublimeOS\x1b[0m 1.0 (x86_64) — \x1b[35mKiteKernel\x1b[0m");
    println!("Developer: Nader Mahbub Khan | Copyright (c) 2026");
    println!("Type 'help' for available commands.\n");

    let mut history: Vec<String> = Vec::new();

    loop {
        let cwd = getcwd();
        let prompt = alloc::format!("\x1b[36msublime\x1b[0m \x1b[34m{}\x1b[0m > ", cwd);
        print!("{}", prompt);
        let line_opt = read_line(&prompt, &mut history);
        let raw_str = match line_opt {
            Some(s) => s,
            None => continue,
        };

        let trimmed = raw_str.trim();
        if trimmed.is_empty() {
            continue;
        }

        if history.last().map(|s| s.as_str()) != Some(trimmed) {
            history.push(String::from(trimmed));
        }

        if trimmed == "exit" || trimmed == "quit" {
            break;
        }

        if trimmed.ends_with('&') {
            let bg_cmd = trimmed[..trimmed.len() - 1].trim();
            if !bg_cmd.is_empty() {
                let pid = spawn_bg(bg_cmd);
                if pid < 0 {
                    println!("\x1b[31mbg spawn error: failed to run '{}' (code {})\x1b[0m", bg_cmd, pid);
                } else {
                    println!("[bg pid {}] {}", pid, bg_cmd);
                }
            }
            continue;
        }

        execute_pipeline(trimmed);
    }

    0
}
