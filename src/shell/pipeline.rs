extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use crate::arch::x86_64::vga::{start_stdout_capture, stop_stdout_capture};
use crate::fs::vfs::{FileType, VFS};
use crate::print;
use crate::println;

/// Standard UNIX Filter: grep [-i] [-v] <pattern> [file]
pub fn run_grep(input: &str, args: &[&str]) -> String {
    let mut case_insensitive = false;
    let mut invert_match = false;
    let mut pattern_opt = None;
    let mut file_path = None;

    for &arg in args {
        if arg == "-i" {
            case_insensitive = true;
        } else if arg == "-v" {
            invert_match = true;
        } else if pattern_opt.is_none() {
            pattern_opt = Some(arg);
        } else if file_path.is_none() {
            file_path = Some(arg);
        }
    }

    let pattern = match pattern_opt {
        Some(p) => p,
        None => return String::from("grep: missing pattern\n"),
    };

    // If file path is supplied, read from file instead of pipeline input
    let source_text = if let Some(path) = file_path {
        match read_file_from_vfs(path) {
            Ok(content) => content,
            Err(e) => return alloc::format!("grep: {}: {}\n", path, e),
        }
    } else {
        String::from(input)
    };

    let mut output = String::new();
    let pat_lower = pattern.to_ascii_lowercase();

    for line in source_text.lines() {
        let is_match = if case_insensitive {
            line.to_ascii_lowercase().contains(&pat_lower)
        } else {
            line.contains(pattern)
        };

        if is_match ^ invert_match {
            output.push_str(line);
            output.push('\n');
        }
    }

    output
}

/// Standard UNIX Filter: wc [-l] [-w] [-c] [file]
pub fn run_wc(input: &str, args: &[&str]) -> String {
    let mut count_lines = false;
    let mut count_words = false;
    let mut count_bytes = false;
    let mut file_path = None;

    for &arg in args {
        if arg == "-l" {
            count_lines = true;
        } else if arg == "-w" {
            count_words = true;
        } else if arg == "-c" {
            count_bytes = true;
        } else if !arg.starts_with('-') && file_path.is_none() {
            file_path = Some(arg);
        }
    }

    if !count_lines && !count_words && !count_bytes {
        count_lines = true;
        count_words = true;
        count_bytes = true;
    }

    let source = if let Some(path) = file_path {
        match read_file_from_vfs(path) {
            Ok(s) => s,
            Err(e) => return alloc::format!("wc: {}: {}\n", path, e),
        }
    } else {
        String::from(input)
    };

    let lines = source.lines().count();
    let words = source.split_whitespace().count();
    let bytes = source.len();

    let mut out = String::new();
    if count_lines {
        out.push_str(&alloc::format!("{:>8} ", lines));
    }
    if count_words {
        out.push_str(&alloc::format!("{:>8} ", words));
    }
    if count_bytes {
        out.push_str(&alloc::format!("{:>8} ", bytes));
    }
    if let Some(path) = file_path {
        out.push_str(path);
    }
    out.push('\n');
    out
}

/// Standard UNIX Filter: head [-n lines] [file]
pub fn run_head(input: &str, args: &[&str]) -> String {
    let mut n = 10;
    let mut file_path = None;
    let mut idx = 0;

    while idx < args.len() {
        if args[idx] == "-n" && idx + 1 < args.len() {
            if let Ok(parsed) = args[idx + 1].parse::<usize>() {
                n = parsed;
            }
            idx += 2;
        } else if !args[idx].starts_with('-') && file_path.is_none() {
            file_path = Some(args[idx]);
            idx += 1;
        } else {
            idx += 1;
        }
    }

    let source = if let Some(path) = file_path {
        match read_file_from_vfs(path) {
            Ok(s) => s,
            Err(e) => return alloc::format!("head: {}: {}\n", path, e),
        }
    } else {
        String::from(input)
    };

    let mut out = String::new();
    for (i, line) in source.lines().enumerate() {
        if i >= n {
            break;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Standard UNIX Filter: tail [-n lines] [file]
pub fn run_tail(input: &str, args: &[&str]) -> String {
    let mut n = 10;
    let mut file_path = None;
    let mut idx = 0;

    while idx < args.len() {
        if args[idx] == "-n" && idx + 1 < args.len() {
            if let Ok(parsed) = args[idx + 1].parse::<usize>() {
                n = parsed;
            }
            idx += 2;
        } else if !args[idx].starts_with('-') && file_path.is_none() {
            file_path = Some(args[idx]);
            idx += 1;
        } else {
            idx += 1;
        }
    }

    let source = if let Some(path) = file_path {
        match read_file_from_vfs(path) {
            Ok(s) => s,
            Err(e) => return alloc::format!("tail: {}: {}\n", path, e),
        }
    } else {
        String::from(input)
    };

    let all_lines: Vec<&str> = source.lines().collect();
    let start_idx = all_lines.len().saturating_sub(n);

    let mut out = String::new();
    for &line in &all_lines[start_idx..] {
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Standard UNIX Filter: sort [-r] [file]
pub fn run_sort(input: &str, args: &[&str]) -> String {
    let mut reverse = false;
    let mut file_path = None;

    for &arg in args {
        if arg == "-r" {
            reverse = true;
        } else if !arg.starts_with('-') && file_path.is_none() {
            file_path = Some(arg);
        }
    }

    let source = if let Some(path) = file_path {
        match read_file_from_vfs(path) {
            Ok(s) => s,
            Err(e) => return alloc::format!("sort: {}: {}\n", path, e),
        }
    } else {
        String::from(input)
    };

    let mut lines: Vec<&str> = source.lines().collect();
    lines.sort_unstable();
    if reverse {
        lines.reverse();
    }

    let mut out = String::new();
    for line in lines {
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// Standard UNIX Filter: uniq [file]
pub fn run_uniq(input: &str, args: &[&str]) -> String {
    let file_path = args.first().copied();

    let source = if let Some(path) = file_path {
        match read_file_from_vfs(path) {
            Ok(s) => s,
            Err(e) => return alloc::format!("uniq: {}: {}\n", path, e),
        }
    } else {
        String::from(input)
    };

    let mut out = String::new();
    let mut last_line: Option<&str> = None;

    for line in source.lines() {
        if last_line != Some(line) {
            out.push_str(line);
            out.push('\n');
            last_line = Some(line);
        }
    }
    out
}

/// Read an entire text file from VFS into a String.
pub fn read_file_from_vfs(path: &str) -> Result<String, &'static str> {
    let vfs = VFS.lock();
    let file = vfs.open(path, 0)?;
    drop(vfs);

    let mut buf = alloc::vec![0u8; 65536];
    let bytes_read = file.read(&mut buf)?;
    if bytes_read == 0 {
        return Ok(String::new());
    }

    let s = core::str::from_utf8(&buf[..bytes_read])
        .map_err(|_| "File contains invalid UTF-8")?;
    Ok(String::from(s))
}

/// Write data to a VFS file path (overwrite or append).
pub fn write_file_to_vfs(path: &str, data: &[u8], append: bool) -> Result<(), &'static str> {
    let mut parts: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if parts.is_empty() {
        return Err("Invalid file path");
    }
    let filename = parts.pop().unwrap();
    let parent_path = if parts.is_empty() { "/" } else { &path[..path.rfind('/').unwrap_or(0)] };

    let vfs = VFS.lock();
    let parent = vfs.resolve_path(parent_path)?;
    drop(vfs);

    let existing_node = parent.lock().lookup(filename).ok();
    let inode = match existing_node {
        Some(node) => {
            let ftype = node.lock().stat().file_type;
            if ftype == crate::fs::vfs::FileType::CharDevice
                || ftype == crate::fs::vfs::FileType::BlockDevice
            {
                node
            } else if !append {
                let _ = parent.lock().unlink(filename);
                parent.lock().create(filename, FileType::Regular)?
            } else {
                node
            }
        }
        None => parent.lock().create(filename, FileType::Regular)?,
    };

    let mut node = inode.lock();
    let offset = if append {
        node.stat().size
    } else {
        0
    };

    node.write(offset, data)?;
    Ok(())
}

/// Attempt to execute a shell command line containing POSIX pipelines ('|') or redirection ('>', '>>', '<').
/// Returns true if the command line was handled as a pipeline/redirection, false if normal single command.
pub fn try_execute_pipeline(line: &str) -> bool {
    let trimmed = line.trim();

    // Check if line contains pipeline or redirection symbols
    if !trimmed.contains('|') && !trimmed.contains('>') && !trimmed.contains('<') {
        return false;
    }

    // 1. Check for input redirection '<'
    let (cmd_after_in, mut current_input) = if let Some(in_pos) = trimmed.find('<') {
        let cmd_part = trimmed[..in_pos].trim();
        let file_part = trimmed[in_pos + 1..].trim();
        // File part may have '>' further down
        let file_name = file_part.split_whitespace().next().unwrap_or("");
        match read_file_from_vfs(file_name) {
            Ok(content) => (cmd_part, content),
            Err(e) => {
                println!("\x1b[31mcosmos: {}: {}\x1b[0m", file_name, e);
                return true;
            }
        }
    } else {
        (trimmed, String::new())
    };

    // 2. Check for output redirection '>>' or '>'
    let (pipeline_part, redirect_target, append_mode) = if let Some(app_pos) = cmd_after_in.find(">>") {
        let left = cmd_after_in[..app_pos].trim();
        let target = cmd_after_in[app_pos + 2..].trim();
        let file_name = target.split_whitespace().next().unwrap_or(target);
        (left, Some(file_name), true)
    } else if let Some(out_pos) = cmd_after_in.find('>') {
        let left = cmd_after_in[..out_pos].trim();
        let target = cmd_after_in[out_pos + 1..].trim();
        let file_name = target.split_whitespace().next().unwrap_or(target);
        (left, Some(file_name), false)
    } else {
        (cmd_after_in, None, false)
    };

    // 3. Split by pipe '|'
    let stages: Vec<&str> = pipeline_part.split('|').map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
    if stages.is_empty() {
        return false;
    }

    for (stage_idx, &stage) in stages.iter().enumerate() {
        let stage_parts: Vec<&str> = stage.split_whitespace().collect();
        if stage_parts.is_empty() {
            continue;
        }
        let stage_cmd = stage_parts[0];
        let stage_args = &stage_parts[1..];

        // Execute stage
        let stage_output = match stage_cmd {
            "grep" | "find" => run_grep(&current_input, stage_args),
            "wc" | "count" => run_wc(&current_input, stage_args),
            "head" | "first" => run_head(&current_input, stage_args),
            "tail" | "last" => run_tail(&current_input, stage_args),
            "sort" => run_sort(&current_input, stage_args),
            "uniq" | "unique" | "distinct" => run_uniq(&current_input, stage_args),
            "cat" | "view" | "read" if stage_args.is_empty() && !current_input.is_empty() => {
                current_input.clone()
            }
            _ => {
                // General shell command: capture its stdout!
                start_stdout_capture();
                crate::shell::commands::execute_command(stage);
                stop_stdout_capture().unwrap_or_default()
            }
        };

        current_input = stage_output;

        // If this is the final stage
        if stage_idx == stages.len() - 1 {
            if let Some(target) = redirect_target {
                match write_file_to_vfs(target, current_input.as_bytes(), append_mode) {
                    Ok(()) => {}
                    Err(e) => println!("\x1b[31mcosmos: failed to redirect to '{}': {}\x1b[0m", target, e),
                }
            } else {
                print!("{}", current_input);
            }
        }
    }

    true
}
