extern crate alloc;
use alloc::collections::{BTreeMap, VecDeque};
use alloc::format;
use alloc::sync::Arc;
use alloc::vec::Vec;
use crate::fs::vfs::{DirEntry, FileType, InodeOperations, Stat};
use crate::sync::IrqSafeSpinlock;

pub const TCGETS: u32 = 0x5401;
pub const TCSETS: u32 = 0x5402;
pub const TIOCGWINSZ: u32 = 0x5413;
pub const TIOCSWINSZ: u32 = 0x5414;
pub const TIOCGPTN: u32 = 0x80045430;
pub const TIOCSPTLCK: u32 = 0x40045431;

pub const ECHO: u32 = 0x0008;
pub const ICANON: u32 = 0x0002;
pub const ICRNL: u32 = 0x0100;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Winsize {
    pub ws_row: u16,
    pub ws_col: u16,
    pub ws_xpixel: u16,
    pub ws_ypixel: u16,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Termios {
    pub c_iflag: u32,
    pub c_oflag: u32,
    pub c_cflag: u32,
    pub c_lflag: u32,
    pub c_line: u8,
    pub c_cc: [u8; 32],
}

pub struct PtyPair {
    pub id: usize,
    pub master_to_slave: VecDeque<u8>,
    pub slave_to_master: VecDeque<u8>,
    pub locked: bool,
    pub echo: bool,
    pub canonical: bool,
    pub icrnl: bool,
    pub winsize: Winsize,
    pub fg_pgid: u32,
}

impl PtyPair {
    pub fn new(id: usize) -> Self {
        Self {
            id,
            master_to_slave: VecDeque::with_capacity(1024),
            slave_to_master: VecDeque::with_capacity(1024),
            locked: true,
            echo: true,
            canonical: true,
            icrnl: true,
            winsize: Winsize {
                ws_row: 24,
                ws_col: 80,
                ws_xpixel: 0,
                ws_ypixel: 0,
            },
            fg_pgid: 0,
        }
    }
}

pub struct PtyTable {
    pub ptys: BTreeMap<usize, Arc<IrqSafeSpinlock<PtyPair>>>,
    pub next_id: usize,
}

impl PtyTable {
    pub const fn new() -> Self {
        Self {
            ptys: BTreeMap::new(),
            next_id: 0,
        }
    }

    pub fn allocate(&mut self) -> (usize, Arc<IrqSafeSpinlock<PtyPair>>) {
        let id = self.next_id;
        self.next_id += 1;
        let pair = Arc::new(IrqSafeSpinlock::new(PtyPair::new(id)));
        self.ptys.insert(id, pair.clone());
        (id, pair)
    }

    pub fn get(&self, id: usize) -> Option<Arc<IrqSafeSpinlock<PtyPair>>> {
        self.ptys.get(&id).cloned()
    }

    pub fn remove(&mut self, id: usize) {
        self.ptys.remove(&id);
    }
}

pub static PTY_TABLE: IrqSafeSpinlock<PtyTable> = IrqSafeSpinlock::new(PtyTable::new());

pub fn allocate_pty_master() -> Arc<IrqSafeSpinlock<dyn InodeOperations>> {
    let (id, pair) = PTY_TABLE.lock().allocate();
    Arc::new(IrqSafeSpinlock::new(PtyMasterDev { id, pair }))
}

pub fn get_pty_slave(id: usize) -> Option<Arc<IrqSafeSpinlock<dyn InodeOperations>>> {
    let pair = PTY_TABLE.lock().get(id)?;
    Some(Arc::new(IrqSafeSpinlock::new(PtySlaveDev { id, pair })))
}

/// Controller / Master side of pseudo-terminal (e.g. telnetd, sshd, windowserver)
pub struct PtyMasterDev {
    pub id: usize,
    pub pair: Arc<IrqSafeSpinlock<PtyPair>>,
}

impl InodeOperations for PtyMasterDev {
    fn read(&self, _offset: usize, buf: &mut [u8]) -> Result<usize, &'static str> {
        let mut pty = self.pair.lock();
        if buf.is_empty() || pty.slave_to_master.is_empty() {
            return Ok(0);
        }
        let mut n = 0;
        while n < buf.len() && !pty.slave_to_master.is_empty() {
            buf[n] = pty.slave_to_master.pop_front().unwrap();
            n += 1;
        }
        Ok(n)
    }

    fn write(&mut self, _offset: usize, buf: &[u8]) -> Result<usize, &'static str> {
        let mut pty = self.pair.lock();
        for &byte in buf {
            // Check for Ctrl+C (ETX = 0x03) -> deliver SIGINT
            if byte == 0x03 {
                let target_pid = if pty.fg_pgid > 0 { pty.fg_pgid as i32 } else { crate::task::process::get_current_pid() as i32 };
                crate::task::process::sys_kill(target_pid, 2); // SIGINT
                if pty.echo {
                    pty.slave_to_master.push_back(b'^');
                    pty.slave_to_master.push_back(b'C');
                    pty.slave_to_master.push_back(b'\n');
                }
                continue;
            }

            // Input translation: carriage return to newline
            let ch = if pty.icrnl && byte == b'\r' {
                b'\n'
            } else {
                byte
            };

            // Terminal line echo back to master controller
            if pty.echo {
                pty.slave_to_master.push_back(ch);
            }

            // Deliver into slave input queue
            pty.master_to_slave.push_back(ch);
        }
        Ok(buf.len())
    }

    fn stat(&self) -> Stat {
        Stat {
            inode: 5000 + self.id as u64,
            size: 0,
            file_type: FileType::CharDevice,
            mode: 0o620,
        }
    }

    fn ioctl(&mut self, cmd: u32, arg: usize) -> Result<isize, &'static str> {
        let mut pty = self.pair.lock();
        match cmd {
            TIOCGPTN => {
                if arg == 0 {
                    return Err("Null pointer in TIOCGPTN");
                }
                let ptr = arg as *mut u32;
                unsafe { *ptr = self.id as u32; }
                Ok(0)
            }
            TIOCSPTLCK => {
                if arg == 0 {
                    return Err("Null pointer in TIOCSPTLCK");
                }
                let ptr = arg as *const i32;
                let val = unsafe { *ptr };
                pty.locked = val != 0;
                Ok(0)
            }
            TIOCGWINSZ => {
                if arg == 0 {
                    return Err("Null pointer in TIOCGWINSZ");
                }
                let ptr = arg as *mut Winsize;
                unsafe { *ptr = pty.winsize; }
                Ok(0)
            }
            TIOCSWINSZ => {
                if arg == 0 {
                    return Err("Null pointer in TIOCSWINSZ");
                }
                let ptr = arg as *const Winsize;
                pty.winsize = unsafe { *ptr };
                Ok(0)
            }
            TCGETS => {
                if arg == 0 {
                    return Err("Null pointer in TCGETS");
                }
                let ptr = arg as *mut Termios;
                let mut term = Termios {
                    c_iflag: if pty.icrnl { ICRNL } else { 0 },
                    c_oflag: 0,
                    c_cflag: 0,
                    c_lflag: (if pty.echo { ECHO } else { 0 }) | (if pty.canonical { ICANON } else { 0 }),
                    c_line: 0,
                    c_cc: [0; 32],
                };
                term.c_cc[0] = 0x03; // VINTR = Ctrl+C
                unsafe { *ptr = term; }
                Ok(0)
            }
            TCSETS => {
                if arg == 0 {
                    return Err("Null pointer in TCSETS");
                }
                let ptr = arg as *const Termios;
                let term = unsafe { *ptr };
                pty.echo = (term.c_lflag & ECHO) != 0;
                pty.canonical = (term.c_lflag & ICANON) != 0;
                pty.icrnl = (term.c_iflag & ICRNL) != 0;
                Ok(0)
            }
            _ => Err("Unsupported PTY ioctl"),
        }
    }
}

/// Terminal / Slave side of pseudo-terminal (e.g. /dev/pts/0 connected to sh/zsh)
pub struct PtySlaveDev {
    pub id: usize,
    pub pair: Arc<IrqSafeSpinlock<PtyPair>>,
}

impl InodeOperations for PtySlaveDev {
    fn read(&self, _offset: usize, buf: &mut [u8]) -> Result<usize, &'static str> {
        let mut pty = self.pair.lock();
        if buf.is_empty() || pty.master_to_slave.is_empty() {
            return Ok(0);
        }
        let mut n = 0;
        while n < buf.len() && !pty.master_to_slave.is_empty() {
            buf[n] = pty.master_to_slave.pop_front().unwrap();
            n += 1;
        }
        Ok(n)
    }

    fn write(&mut self, _offset: usize, buf: &[u8]) -> Result<usize, &'static str> {
        let mut pty = self.pair.lock();
        for &byte in buf {
            pty.slave_to_master.push_back(byte);
        }
        Ok(buf.len())
    }

    fn stat(&self) -> Stat {
        Stat {
            inode: 6000 + self.id as u64,
            size: 0,
            file_type: FileType::CharDevice,
            mode: 0o620,
        }
    }

    fn ioctl(&mut self, cmd: u32, arg: usize) -> Result<isize, &'static str> {
        let mut pty = self.pair.lock();
        match cmd {
            TIOCGWINSZ => {
                if arg == 0 {
                    return Err("Null pointer in TIOCGWINSZ");
                }
                let ptr = arg as *mut Winsize;
                unsafe { *ptr = pty.winsize; }
                Ok(0)
            }
            TIOCSWINSZ => {
                if arg == 0 {
                    return Err("Null pointer in TIOCSWINSZ");
                }
                let ptr = arg as *const Winsize;
                pty.winsize = unsafe { *ptr };
                Ok(0)
            }
            _ => Err("Unsupported PTY slave ioctl"),
        }
    }
}

/// /dev/ptmx Multiplexer Device Node
pub struct PtmxDev;

impl InodeOperations for PtmxDev {
    fn read(&self, _offset: usize, _buf: &mut [u8]) -> Result<usize, &'static str> {
        Ok(0)
    }

    fn write(&mut self, _offset: usize, buf: &[u8]) -> Result<usize, &'static str> {
        Ok(buf.len())
    }

    fn stat(&self) -> Stat {
        Stat {
            inode: 104,
            size: 0,
            file_type: FileType::CharDevice,
            mode: 0o666,
        }
    }
}

/// /dev/pts Virtual Directory Node
pub struct PtsDirectory;

impl InodeOperations for PtsDirectory {
    fn lookup(&self, name: &str) -> Result<Arc<IrqSafeSpinlock<dyn InodeOperations>>, &'static str> {
        let id = name.parse::<usize>().map_err(|_| "Invalid PTS index")?;
        get_pty_slave(id).ok_or("PTS device not found")
    }

    fn readdir(&self) -> Result<Vec<DirEntry>, &'static str> {
        let table = PTY_TABLE.lock();
        let mut entries = Vec::new();
        for &id in table.ptys.keys() {
            entries.push(DirEntry {
                name: format!("{}", id),
                inode: 6000 + id as u64,
                file_type: FileType::CharDevice,
            });
        }
        Ok(entries)
    }

    fn stat(&self) -> Stat {
        Stat {
            inode: 105,
            size: PTY_TABLE.lock().ptys.len(),
            file_type: FileType::Directory,
            mode: 0o755,
        }
    }
}
