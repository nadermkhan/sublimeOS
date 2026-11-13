#![no_std]
#![no_main]

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use sublime_rt::{
    args, chdir, close, dup, dup2, getcwd, getpid, mkdir, open, pipe, print, println, read,
    read_dir, read_to_string, sleep_ms, spawn, spawn_bg, sync, syscommand, unlink, O_APPEND,
    O_CREAT, O_RDONLY, O_TRUNC, O_WRONLY,
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
        "hello", "pwd", "sync", "exec", "uptime", "date", "touch", "rm", "mkdir",
        "cd", "env", "export", "unset", "source", "sh"
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
                            format!("/{}{}", entry.name, if entry.is_dir { "/" } else { "" })
                        } else {
                            format!("{}/{}{}", base, entry.name, if entry.is_dir { "/" } else { "" })
                        }
                    } else {
                        format!("{}{}", entry.name, if entry.is_dir { "/" } else { "" })
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

/// Strip comment starting with '#' unless protected inside single or double quotes.
fn strip_comments(line: &str) -> String {
    let mut out = String::new();
    let mut in_single = false;
    let mut in_double = false;

    for c in line.chars() {
        if c == '\'' && !in_double {
            in_single = !in_single;
        } else if c == '"' && !in_single {
            in_double = !in_double;
        } else if c == '#' && !in_single && !in_double {
            break;
        }
        out.push(c);
    }
    out
}

/// Expand shell variables ($VAR, ${VAR}, $?, $$, and \$ escape).
fn expand_variables(input: &str, env: &BTreeMap<String, String>) -> String {
    let mut out = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    let mut in_single_quotes = false;

    while i < chars.len() {
        let c = chars[i];
        if c == '\'' {
            in_single_quotes = !in_single_quotes;
            out.push(c);
            i += 1;
            continue;
        }

        if c == '\\' && i + 1 < chars.len() && chars[i + 1] == '$' {
            out.push('$');
            i += 2;
            continue;
        }

        if c == '$' && !in_single_quotes && i + 1 < chars.len() {
            let next = chars[i + 1];
            if next == '?' {
                let val = env.get("?").map(|s| s.as_str()).unwrap_or("0");
                out.push_str(val);
                i += 2;
                continue;
            } else if next == '$' {
                let pid_str = format!("{}", getpid());
                out.push_str(&pid_str);
                i += 2;
                continue;
            } else if next == '{' {
                if let Some(close_pos) = chars[i + 2..].iter().position(|&ch| ch == '}') {
                    let var_name: String = chars[i + 2..i + 2 + close_pos].iter().collect();
                    if let Some(val) = env.get(&var_name) {
                        out.push_str(val);
                    }
                    i = i + 2 + close_pos + 1;
                    continue;
                }
            } else if next.is_alphabetic() || next == '_' {
                let mut end = i + 1;
                while end < chars.len() && (chars[end].is_alphanumeric() || chars[end] == '_') {
                    end += 1;
                }
                let var_name: String = chars[i + 1..end].iter().collect();
                if let Some(val) = env.get(&var_name) {
                    out.push_str(val);
                }
                i = end;
                continue;
            }
        }

        out.push(c);
        i += 1;
    }

    out
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum ChainOp {
    Start,
    And, // &&
    Or,  // ||
    Seq, // ;
}

struct ChainedCmd<'a> {
    cmd: &'a str,
    op: ChainOp,
}

/// Parse a command line into chained commands (&&, ||, ;) while respecting quotes.
fn parse_command_chain(line: &str) -> Vec<ChainedCmd<'_>> {
    let mut result = Vec::new();
    let bytes = line.as_bytes();
    let mut start = 0;
    let mut i = 0;
    let mut current_op = ChainOp::Start;
    let mut in_single = false;
    let mut in_double = false;

    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\'' && !in_double {
            in_single = !in_single;
            i += 1;
            continue;
        }
        if b == b'"' && !in_single {
            in_double = !in_double;
            i += 1;
            continue;
        }

        if !in_single && !in_double {
            if b == b'&' && i + 1 < bytes.len() && bytes[i + 1] == b'&' {
                let cmd_slice = line[start..i].trim();
                if !cmd_slice.is_empty() {
                    result.push(ChainedCmd { cmd: cmd_slice, op: current_op });
                }
                current_op = ChainOp::And;
                i += 2;
                start = i;
                continue;
            } else if b == b'|' && i + 1 < bytes.len() && bytes[i + 1] == b'|' {
                let cmd_slice = line[start..i].trim();
                if !cmd_slice.is_empty() {
                    result.push(ChainedCmd { cmd: cmd_slice, op: current_op });
                }
                current_op = ChainOp::Or;
                i += 2;
                start = i;
                continue;
            } else if b == b';' {
                let cmd_slice = line[start..i].trim();
                if !cmd_slice.is_empty() {
                    result.push(ChainedCmd { cmd: cmd_slice, op: current_op });
                }
                current_op = ChainOp::Seq;
                i += 1;
                start = i;
                continue;
            }
        }
        i += 1;
    }

    let remaining = line[start..].trim();
    if !remaining.is_empty() {
        result.push(ChainedCmd { cmd: remaining, op: current_op });
    }

    result
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

/// Strip surrounding single or double quotes if present.
fn unquote(s: &str) -> &str {
    let s = s.trim();
    if (s.starts_with('"') && s.ends_with('"') && s.len() >= 2)
        || (s.starts_with('\'') && s.ends_with('\'') && s.len() >= 2)
    {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

fn execute_single_cmd(trimmed: &str, env: &mut BTreeMap<String, String>) -> i32 {
    let trimmed = trimmed.trim();
    if trimmed.is_empty() {
        return 0;
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
        println!("    cd [path]           Change working directory (or 'cd ..', 'cd /')");
        println!("    path                Print current working directory (or 'pwd')");
        println!("    clear               Clear screen");
        println!();
        println!("  \x1b[1;36mEnvironment & Shell Scripting:\x1b[0m");
        println!("    env                 Print all active environment variables");
        println!("    export KEY=VALUE    Define or export environment variable");
        println!("    unset KEY           Unset environment variable");
        println!("    source <file.sh>    Execute commands from shell script file (or 'sh')");
        println!("    cmd1 && cmd2        Conditional execution: run cmd2 only if cmd1 succeeds");
        println!("    cmd1 || cmd2        Conditional execution: run cmd2 only if cmd1 fails");
        println!("    cmd1 ; cmd2         Sequential execution: run cmd1 then cmd2");
        println!();
        println!("  \x1b[1;36mSystem & Device:\x1b[0m");
        println!("    about               System & kernel release information (or 'uname')");
        println!("    hardware            Silicon hardware & firmware specifications");
        println!("    tasks [list|kill]   Process manager and task control (or 'ps')");
        println!("    storage [list|sync] Storage drives and buffer cache");
        println!("    display [1|2]       Console font size & HiDPI (Ctrl+= / Ctrl+-)");
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
        0
    } else if trimmed == "clear" {
        print!("\x1b[2J\x1b[H");
        0
    } else if matches!(trimmed, "path" | "where" | "pwd") {
        println!("{}", getcwd());
        0
    } else if matches!(trimmed, "about" | "uname") {
        println!("SublimeOS 1.0.0 (x86_64) — KiteKernel");
        println!("Developer: Nader Mahbub Khan | Copyright (c) 2026");
        0
    } else if matches!(trimmed, "hardware" | "specs") {
        syscommand("hardware") as i32
    } else if matches!(first_word, "tasks" | "ps") {
        syscommand("tasks") as i32
    } else if matches!(first_word, "storage" | "disks") {
        syscommand("storage") as i32
    } else if trimmed == "sync" {
        sync();
        println!("Storage cache synced.");
        0
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
            1
        } else {
            env.insert(String::from("PWD"), getcwd());
            0
        }
    } else if trimmed == "env" {
        for (k, v) in env.iter() {
            if k != "?" {
                println!("{}={}", k, v);
            }
        }
        0
    } else if first_word == "export" {
        if rest.is_empty() {
            for (k, v) in env.iter() {
                if k != "?" {
                    println!("export {}=\"{}\"", k, v);
                }
            }
            0
        } else if let Some(eq_pos) = rest.find('=') {
            let k = rest[..eq_pos].trim();
            let v = unquote(rest[eq_pos + 1..].trim());
            if !k.is_empty() {
                env.insert(String::from(k), String::from(v));
            }
            0
        } else {
            let k = rest.trim();
            if !k.is_empty() && !env.contains_key(k) {
                env.insert(String::from(k), String::new());
            }
            0
        }
    } else if first_word == "unset" {
        if !rest.is_empty() {
            env.remove(rest);
        }
        0
    } else if first_word == "source" || first_word == "." || (first_word == "sh" && !rest.is_empty()) {
        let script_path = rest;
        match read_to_string(script_path) {
            Ok(content) => execute_script(&content, env),
            Err(e) => {
                println!("source: {}: No such file or directory (error {})", script_path, e);
                1
            }
        }
    } else if first_word == "power" {
        if rest == "off" || rest == "shutdown" || rest == "halt" {
            syscommand("shutdown") as i32
        } else if rest == "restart" || rest == "reboot" || rest == "reset" {
            syscommand("reboot") as i32
        } else {
            println!("Usage: power [off | restart]");
            1
        }
    } else if matches!(first_word, "shutdown" | "poweroff" | "halt") {
        syscommand("shutdown") as i32
    } else if matches!(first_word, "reboot" | "restart") {
        syscommand("reboot") as i32
    } else if first_word == "display" || first_word == "screen" {
        let arg = if rest.is_empty() { "1" } else { rest };
        let mut cmd = String::from("font scale ");
        cmd.push_str(arg);
        syscommand(&cmd) as i32
    } else if matches!(first_word, "print" | "echo") {
        println!("{}", unquote(rest));
        0
    } else if first_word == "touch" || (first_word == "create" && !rest.starts_with("folder")) {
        let target = if first_word == "create" && rest.starts_with("file ") {
            rest.strip_prefix("file ").unwrap().trim()
        } else {
            rest
        };
        if target.is_empty() {
            println!("Usage: create <file> (or touch <file>)");
            1
        } else {
            let fd = open(target, O_WRONLY | O_CREAT, 0o644);
            if fd >= 0 {
                close(fd as usize);
                println!("Created {}", target);
                0
            } else {
                println!("create: failed to create '{}' (error {})", target, fd);
                1
            }
        }
    } else if matches!(first_word, "rm" | "delete" | "remove") {
        let target = rest;
        if target.is_empty() {
            println!("Usage: delete <file> (or rm <file>)");
            1
        } else {
            let res = unlink(target);
            if res == 0 {
                println!("Deleted {}", target);
                0
            } else {
                println!("delete: failed to remove '{}' (error {})", target, res);
                1
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
            1
        } else {
            let res = mkdir(target, 0o755);
            if res == 0 {
                println!("Created folder {}", target);
                0
            } else {
                println!("mkdir: failed to create folder '{}' (error {})", target, res);
                1
            }
        }
    } else if first_word == "exec" || first_word == "run" {
        if !rest.is_empty() {
            let code = spawn(rest);
            if code < 0 {
                println!("\x1b[31mexec error: failed to spawn '{}' (code {})\x1b[0m", rest, code);
                1
            } else {
                println!("\x1b[32m[Process Terminated] Exit code: {}\x1b[0m", code);
                code as i32
            }
        } else {
            0
        }
    } else if matches!(first_word, "list" | "files" | "ls") {
        let cmd_to_spawn = if first_word == "ls" {
            String::from(trimmed)
        } else if rest.is_empty() {
            String::from("ls")
        } else {
            format!("ls {}", rest)
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
                    0
                }
                Err(e) => {
                    println!("ls: {}: cannot access (error {})", target_dir, e);
                    1
                }
            }
        } else {
            code as i32
        }
    } else if matches!(first_word, "view" | "read" | "show" | "cat") {
        let cmd_to_spawn = if first_word == "cat" {
            String::from(trimmed)
        } else {
            format!("cat {}", rest)
        };
        let code = spawn(&cmd_to_spawn);
        if code < 0 {
            println!("view: failed to open/read '{}'", rest);
            1
        } else {
            code as i32
        }
    } else if matches!(first_word, "find" | "search" | "grep") {
        let cmd_to_spawn = if first_word == "grep" {
            String::from(trimmed)
        } else {
            format!("grep {}", rest)
        };
        let code = spawn(&cmd_to_spawn);
        if code < 0 {
            println!("find: grep failed for '{}'", rest);
            1
        } else {
            code as i32
        }
    } else if matches!(first_word, "count" | "wc") {
        let cmd_to_spawn = if first_word == "wc" {
            String::from(trimmed)
        } else {
            format!("wc {}", rest)
        };
        let code = spawn(&cmd_to_spawn);
        if code < 0 {
            println!("count: wc failed for '{}'", rest);
            1
        } else {
            code as i32
        }
    } else if matches!(first_word, "edit" | "calc" | "hello" | "snake") {
        let code = spawn(trimmed);
        if code < 0 {
            println!("\x1b[31mspawn error: failed to run '{}' (code {})\x1b[0m", trimmed, code);
            1
        } else {
            code as i32
        }
    } else if trimmed.ends_with(".sh") {
        // Direct execution of script file
        match read_to_string(trimmed) {
            Ok(content) => execute_script(&content, env),
            Err(_) => {
                println!("sublimesh: {}: No such file or directory", trimmed);
                1
            }
        }
    } else if matches!(first_word, "network" | "audio" | "sound" | "desktop" | "gui") {
        syscommand(trimmed) as i32
    } else {
        let code = spawn(trimmed);
        if code < 0 {
            let sys_code = syscommand(trimmed);
            if sys_code < 0 {
                println!("sublimesh: command not found: '{}'. Type 'help' for available commands.", trimmed);
                127
            } else {
                sys_code as i32
            }
        } else {
            code as i32
        }
    }
}

fn execute_stage_with_redirection(
    stage: &str,
    pipe_in: Option<i32>,
    pipe_out: Option<i32>,
    env: &mut BTreeMap<String, String>,
) -> i32 {
    let (cmd, redir_in, redir_out, append) = parse_redirection(stage);
    if cmd.is_empty() {
        return 0;
    }

    if redir_in.is_none() && redir_out.is_none() && pipe_in.is_none() && pipe_out.is_none() {
        return execute_single_cmd(cmd, env);
    }

    let mut in_fd_to_use = None;
    let mut close_in_fd = false;
    if let Some(file_path) = redir_in {
        let fd = open(file_path, O_RDONLY, 0);
        if fd < 0 {
            println!("sublimesh: {}: No such file or directory", file_path);
            return 1;
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
            return 1;
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

    let code = execute_single_cmd(cmd, env);

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

    code
}

fn execute_pipeline(cmd_line: &str, env: &mut BTreeMap<String, String>) -> i32 {
    let stages: Vec<&str> = cmd_line
        .split('|')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    if stages.is_empty() {
        return 0;
    }

    if stages.len() == 1 {
        return execute_stage_with_redirection(stages[0], None, None, env);
    }

    let mut prev_pipe_read: Option<i32> = None;
    let mut last_code = 0;

    for (i, stage) in stages.iter().enumerate() {
        let is_last = i == stages.len() - 1;
        let mut curr_pipe = [0i32; 2];
        if !is_last {
            if pipe(&mut curr_pipe) < 0 {
                println!("pipe creation failed");
                if let Some(r) = prev_pipe_read {
                    close(r as usize);
                }
                return 1;
            }
        }

        let next_write = if !is_last { Some(curr_pipe[1]) } else { None };
        last_code = execute_stage_with_redirection(stage, prev_pipe_read, next_write, env);

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

    last_code
}

/// Execute a single full command line with variable expansion and command chaining (&&, ||, ;).
fn execute_line(raw_line: &str, env: &mut BTreeMap<String, String>) -> i32 {
    let clean = strip_comments(raw_line);
    let trimmed = clean.trim();
    if trimmed.is_empty() {
        return 0;
    }

    if trimmed.ends_with('&') {
        let bg_cmd = trimmed[..trimmed.len() - 1].trim();
        if !bg_cmd.is_empty() {
            let expanded_bg = expand_variables(bg_cmd, env);
            let pid = spawn_bg(&expanded_bg);
            if pid < 0 {
                println!("\x1b[31mbg spawn error: failed to run '{}' (code {})\x1b[0m", expanded_bg, pid);
                return 1;
            } else {
                println!("[bg pid {}] {}", pid, expanded_bg);
                return 0;
            }
        }
        return 0;
    }

    let expanded = expand_variables(trimmed, env);
    let expanded_trimmed = expanded.trim();
    if expanded_trimmed.is_empty() {
        return 0;
    }

    let chain = parse_command_chain(expanded_trimmed);
    let mut last_code = 0;

    for item in chain {
        let should_run = match item.op {
            ChainOp::Start | ChainOp::Seq => true,
            ChainOp::And => last_code == 0,
            ChainOp::Or => last_code != 0,
        };

        if should_run {
            last_code = execute_pipeline(item.cmd, env);
            env.insert(String::from("?"), format!("{}", last_code));
        }
    }

    last_code
}

/// Execute a multiline script string line by line.
fn execute_script(script: &str, env: &mut BTreeMap<String, String>) -> i32 {
    let mut last_code = 0;
    for line in script.lines() {
        let clean = strip_comments(line);
        let trimmed = clean.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed == "exit" || trimmed == "quit" {
            break;
        }
        last_code = execute_line(trimmed, env);
    }
    last_code
}

#[no_mangle]
pub fn main() -> i32 {
    let mut env: BTreeMap<String, String> = BTreeMap::new();
    env.insert(String::from("USER"), String::from("sublime"));
    env.insert(String::from("HOME"), String::from("/"));
    env.insert(String::from("PATH"), String::from("/bin"));
    env.insert(String::from("SHELL"), String::from("/bin/sh"));
    env.insert(String::from("OS"), String::from("SublimeOS"));
    env.insert(String::from("KERNEL"), String::from("KiteKernel"));
    env.insert(String::from("TERM"), String::from("xterm-256color"));
    env.insert(String::from("PWD"), getcwd());
    env.insert(String::from("?"), String::from("0"));

    // Check if invoked with a script argument: /bin/sh <script_path>
    let argv = args();
    if argv.len() >= 2 {
        let script_path = argv[1];
        match read_to_string(script_path) {
            Ok(content) => return execute_script(&content, &mut env),
            Err(e) => {
                println!("sublimesh: {}: No such file or directory (error {})", script_path, e);
                return 1;
            }
        }
    }

    println!("\x1b[1;36mSublimeOS\x1b[0m 1.0 (x86_64) — \x1b[35mKiteKernel\x1b[0m");
    println!("Developer: Nader Mahbub Khan | Copyright (c) 2026");

    // Automatically source /etc/rc.sh on interactive shell boot if it exists
    if let Ok(rc_content) = read_to_string("/etc/rc.sh") {
        execute_script(&rc_content, &mut env);
    } else {
        println!("Type 'help' for available commands.\n");
    }

    let mut history: Vec<String> = Vec::new();

    loop {
        let cwd = getcwd();
        let prompt = format!("\x1b[36msublime\x1b[0m \x1b[34m{}\x1b[0m > ", cwd);
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

        execute_line(trimmed, &mut env);
    }

    0
}
