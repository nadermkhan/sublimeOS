extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::fs::vfs::{FileType, VFS};
use crate::gui::desktop::DesktopAppId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppManifest {
    pub name: String,
    pub bundle_path: String,
    pub exec: String,
    pub args: Vec<String>,
    pub builtin: Option<DesktopAppId>,
    pub category: String,
    pub comment: String,
    pub terminal: bool,
}

/// Check whether `head` starts with a valid 64-bit x86_64 ELF header.
pub fn is_elf64_magic(head: &[u8]) -> bool {
    if head.len() < 20 {
        return false;
    }
    if &head[0..4] != b"\x7FELF" || head[4] != 2 {
        return false;
    }
    let machine = u16::from_le_bytes([head[18], head[19]]);
    machine == 0x3E
}

/// Inspect VFS file header to check if `path` is an ELF64 x86_64 binary.
pub fn is_elf64_file(path: &str) -> bool {
    let vfs = VFS.lock();
    let Ok(file) = vfs.open(path, 0) else {
        return false;
    };
    drop(vfs);
    let mut head = [0u8; 64];
    match file.read(&mut head) {
        Ok(n) if n >= 20 => is_elf64_magic(&head[..n]),
        _ => false,
    }
}

/// Detect whether an ELF binary connects to `/run/wm.sock` so unknown user GUI binaries auto-run asynchronously.
pub fn elf_is_gui_client(path: &str) -> bool {
    let vfs = VFS.lock();
    let Ok(file) = vfs.open(path, 0) else {
        return false;
    };
    drop(vfs);
    let mut chunk = alloc::vec![0u8; 32768];
    let mut total = 0usize;
    let needle = b"/run/wm.";
    let mut tail = [0u8; 16];
    let mut tail_len = 0usize;
    loop {
        match file.read(&mut chunk) {
            Ok(0) => return false,
            Ok(n) => {
                let mut search_buf = Vec::with_capacity(tail_len + n);
                search_buf.extend_from_slice(&tail[..tail_len]);
                search_buf.extend_from_slice(&chunk[..n]);
                if search_buf.windows(needle.len()).any(|w| w == needle) {
                    return true;
                }
                let keep = (needle.len() - 1).min(search_buf.len());
                tail[..keep].copy_from_slice(&search_buf[search_buf.len() - keep..]);
                tail_len = keep;
                total += n;
                if total >= 4 * 1024 * 1024 {
                    return false;
                }
            }
            Err(_) => return false,
        }
    }
}

/// Read an entire VFS binary (up to 8 MiB) into memory for ELF execution or inspection.
pub fn read_vfs_binary(path: &str) -> Option<Vec<u8>> {
    let file = {
        let vfs = VFS.lock();
        vfs.open(path, 0).ok()?
    };
    let size_hint = {
        let st = file.inode.lock().stat();
        (st.size as usize).clamp(4096, 8 * 1024 * 1024)
    };
    let mut buf = Vec::with_capacity(size_hint);
    let mut chunk = alloc::vec![0u8; 65536];
    loop {
        match file.read(&mut chunk) {
            Ok(0) => break,
            Ok(m) => {
                buf.extend_from_slice(&chunk[..m]);
                if buf.len() >= 8 * 1024 * 1024 {
                    break;
                }
            }
            Err(_) => return None,
        }
    }
    if buf.is_empty() {
        None
    } else {
        Some(buf)
    }
}

fn parse_builtin(val: &str) -> Option<DesktopAppId> {
    match val.trim() {
        "Terminal" => Some(DesktopAppId::Terminal),
        "Sysmon" | "TaskManager" => Some(DesktopAppId::Sysmon),
        "FileManager" | "Explorer" => Some(DesktopAppId::FileManager),
        "Calculator" => Some(DesktopAppId::Calculator),
        "TextEdit" | "Notepad" => Some(DesktopAppId::TextEdit),
        "Settings" | "ControlPanel" => Some(DesktopAppId::Settings),
        "About" => Some(DesktopAppId::About),
        "Studio" => Some(DesktopAppId::Studio),
        _ => None,
    }
}

pub fn parse_app_manifest(text: &str, bundle_path: &str) -> Option<AppManifest> {
    let file_base = bundle_path
        .rsplit('/')
        .next()
        .unwrap_or(bundle_path)
        .strip_suffix(".app")
        .unwrap_or(bundle_path);
    let mut name = String::from(file_base);
    let mut exec = String::new();
    let mut args = Vec::new();
    let mut builtin = None;
    let mut category = String::from("Application");
    let mut comment = String::new();
    let mut terminal = false;

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('[') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let key = k.trim();
            let val = v.trim();
            match key {
                "Name" => name = String::from(val),
                "Exec" => exec = String::from(val),
                "Args" => {
                    args = val.split_whitespace().map(String::from).collect();
                }
                "Builtin" => builtin = parse_builtin(val),
                "Category" => category = String::from(val),
                "Comment" => comment = String::from(val),
                "Terminal" => terminal = val.eq_ignore_ascii_case("true") || val == "1",
                _ => {}
            }
        }
    }

    if exec.is_empty() && builtin.is_none() {
        // Backward compatibility for legacy stub files: "SublimeOS Executable: <Name>"
        if let Some(tag) = text.lines().next().and_then(|l| l.strip_prefix("SublimeOS Executable:")) {
            builtin = parse_builtin(tag.trim());
        }
    }

    if exec.is_empty() && builtin.is_none() {
        return None;
    }

    Some(AppManifest {
        name,
        bundle_path: String::from(bundle_path),
        exec,
        args,
        builtin,
        category,
        comment,
        terminal,
    })
}

pub fn read_app_manifest(path: &str) -> Option<AppManifest> {
    let vfs = VFS.lock();
    let file = vfs.open(path, 0).ok()?;
    drop(vfs);
    let mut buf = [0u8; 2048];
    let n = file.read(&mut buf).ok()?;
    let text = core::str::from_utf8(&buf[..n]).ok()?;
    parse_app_manifest(text, path)
}

/// Resolve a command or path against `cwd` and standard executable search directories (`/usr/local/bin`, `/bin`).
pub fn resolve_executable_path(cmd: &str, cwd: &str) -> Option<String> {
    let trimmed = cmd.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.contains('/') {
        let candidate = if trimmed.starts_with('/') {
            String::from(trimmed)
        } else if cwd == "/" {
            format!("/{}", trimmed.trim_start_matches("./"))
        } else {
            format!("{}/{}", cwd.trim_end_matches('/'), trimmed.trim_start_matches("./"))
        };
        if is_elf64_file(&candidate) {
            return Some(candidate);
        }
        return None;
    }

    for dir in ["/usr/local/bin", "/bin"] {
        let candidate = format!("{}/{}", dir, trimmed);
        if is_elf64_file(&candidate) {
            return Some(candidate);
        }
    }
    None
}

/// Discover lightweight `.app` bundles without scanning `/bin` ELF executables.
pub fn discover_app_bundles() -> Vec<AppManifest> {
    let mut out = Vec::new();

    for dir in ["/Applications", "/Applications/Utilities", "/Users/sublime/Desktop"] {
        let entries = {
            let vfs = VFS.lock();
            vfs.resolve_path(dir)
                .and_then(|n| n.lock().readdir())
                .unwrap_or_default()
        };
        for e in entries {
            if e.file_type != FileType::Directory && e.name.ends_with(".app") {
                let full = format!("{}/{}", dir, e.name);
                if let Some(m) = read_app_manifest(&full) {
                    if !out.iter().any(|x: &AppManifest| x.bundle_path == m.bundle_path) {
                        out.push(m);
                    }
                }
            }
        }
    }

    out
}

/// Discover `.app` bundles and standalone ELF64 executables across standard application directories.
pub fn discover_apps() -> Vec<AppManifest> {
    let mut out = discover_app_bundles();

    for dir in ["/usr/local/bin", "/bin"] {
        let entries = {
            let vfs = VFS.lock();
            vfs.resolve_path(dir)
                .and_then(|n| n.lock().readdir())
                .unwrap_or_default()
        };
        for e in entries {
            if e.file_type != FileType::Directory && e.name != "init" {
                let full = format!("{}/{}", dir, e.name);
                if out.iter().any(|x| x.exec == full) {
                    continue;
                }
                if is_elf64_file(&full) {
                    let is_gui = elf_is_gui_client(&full);
                    out.push(AppManifest {
                        name: e.name,
                        bundle_path: full.clone(),
                        exec: full,
                        args: Vec::new(),
                        builtin: None,
                        category: String::from(if is_gui { "User GUI App" } else { "CLI Binary" }),
                        comment: String::from("ELF64 User Executable"),
                        terminal: !is_gui,
                    });
                }
            }
        }
    }

    out
}
