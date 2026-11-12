extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use crate::fs::vfs::{DirEntry, FileType, InodeOperations, Stat};
use crate::sync::IrqSafeSpinlock;

static RNG_STATE: AtomicU64 = AtomicU64::new(0x853c49e6748fea9b);

fn next_rand() -> u8 {
    let mut x = RNG_STATE.load(Ordering::Relaxed);
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    RNG_STATE.store(x, Ordering::Relaxed);
    (x & 0xFF) as u8
}

pub struct DevFsDirectory {
    entries: BTreeMap<String, Arc<IrqSafeSpinlock<dyn InodeOperations>>>,
}

impl DevFsDirectory {
    pub fn new() -> Arc<IrqSafeSpinlock<Self>> {
        let mut entries: BTreeMap<String, Arc<IrqSafeSpinlock<dyn InodeOperations>>> = BTreeMap::new();
        entries.insert(String::from("null"), Arc::new(IrqSafeSpinlock::new(NullDev)));
        entries.insert(String::from("zero"), Arc::new(IrqSafeSpinlock::new(ZeroDev)));
        entries.insert(String::from("random"), Arc::new(IrqSafeSpinlock::new(RandomDev)));
        entries.insert(String::from("urandom"), Arc::new(IrqSafeSpinlock::new(RandomDev)));
        entries.insert(String::from("tty"), Arc::new(IrqSafeSpinlock::new(TtyDev)));
        entries.insert(String::from("console"), Arc::new(IrqSafeSpinlock::new(TtyDev)));
        entries.insert(String::from("ttys000"), Arc::new(IrqSafeSpinlock::new(TtyDev)));
        entries.insert(String::from("ttys001"), Arc::new(IrqSafeSpinlock::new(TtyDev)));
        entries.insert(String::from("disk0"), Arc::new(IrqSafeSpinlock::new(NullDev)));
        entries.insert(String::from("disk0s1"), Arc::new(IrqSafeSpinlock::new(NullDev)));
        entries.insert(String::from("rtc"), Arc::new(IrqSafeSpinlock::new(RtcDev)));
        entries.insert(String::from("fb0"), Arc::new(IrqSafeSpinlock::new(FbDev)));
        entries.insert(String::from("backlight"), Arc::new(IrqSafeSpinlock::new(BacklightDev)));
        entries.insert(String::from("ptmx"), Arc::new(IrqSafeSpinlock::new(crate::tty::pty::PtmxDev)));
        entries.insert(String::from("pts"), Arc::new(IrqSafeSpinlock::new(crate::tty::pty::PtsDirectory)));
        entries.insert(String::from("shm"), Arc::new(IrqSafeSpinlock::new(crate::mm::shm::ShmDirectory)));
        entries.insert(String::from("sem"), Arc::new(IrqSafeSpinlock::new(crate::ipc::semaphore::SemDirectory)));
        entries.insert(String::from("mqueue"), Arc::new(IrqSafeSpinlock::new(crate::ipc::mqueue::MqueueDirectory)));
        entries.insert(String::from("ttyS0"), Arc::new(IrqSafeSpinlock::new(crate::arch::x86_64::serial::UartDev(0))));
        entries.insert(String::from("ttyS1"), Arc::new(IrqSafeSpinlock::new(crate::arch::x86_64::serial::UartDev(1))));

        Arc::new(IrqSafeSpinlock::new(Self { entries }))
    }
}

impl InodeOperations for DevFsDirectory {
    fn lookup(&self, name: &str) -> Result<Arc<IrqSafeSpinlock<dyn InodeOperations>>, &'static str> {
        self.entries.get(name).cloned().ok_or("Device not found")
    }

    fn readdir(&self) -> Result<Vec<DirEntry>, &'static str> {
        let mut list = Vec::new();
        for (name, node) in self.entries.iter() {
            let stat = node.lock().stat();
            list.push(DirEntry {
                name: name.clone(),
                inode: stat.inode,
                file_type: stat.file_type,
            });
        }
        Ok(list)
    }

    fn stat(&self) -> Stat {
        Stat {
            inode: 2,
            size: self.entries.len(),
            file_type: FileType::Directory,
            mode: 0o755,
        }
    }
}

pub struct NullDev;
impl InodeOperations for NullDev {
    fn read(&self, _offset: usize, _buf: &mut [u8]) -> Result<usize, &'static str> {
        Ok(0) // EOF
    }
    fn write(&mut self, _offset: usize, buf: &[u8]) -> Result<usize, &'static str> {
        Ok(buf.len())
    }
    fn stat(&self) -> Stat {
        Stat { inode: 100, size: 0, file_type: FileType::CharDevice, mode: 0o666 }
    }
}

pub struct ZeroDev;
impl InodeOperations for ZeroDev {
    fn read(&self, _offset: usize, buf: &mut [u8]) -> Result<usize, &'static str> {
        buf.fill(0);
        Ok(buf.len())
    }
    fn write(&mut self, _offset: usize, buf: &[u8]) -> Result<usize, &'static str> {
        Ok(buf.len())
    }
    fn stat(&self) -> Stat {
        Stat { inode: 101, size: 0, file_type: FileType::CharDevice, mode: 0o666 }
    }
}

pub struct RandomDev;
impl InodeOperations for RandomDev {
    fn read(&self, _offset: usize, buf: &mut [u8]) -> Result<usize, &'static str> {
        for byte in buf.iter_mut() {
            *byte = next_rand();
        }
        Ok(buf.len())
    }
    fn write(&mut self, _offset: usize, buf: &[u8]) -> Result<usize, &'static str> {
        Ok(buf.len())
    }
    fn stat(&self) -> Stat {
        Stat { inode: 102, size: 0, file_type: FileType::CharDevice, mode: 0o666 }
    }
}

pub struct TtyDev;
impl InodeOperations for TtyDev {
    fn read(&self, _offset: usize, buf: &mut [u8]) -> Result<usize, &'static str> {
        if buf.is_empty() {
            return Ok(0);
        }
        // Read character from keyboard buffer if available
        if let Some(ch) = crate::drivers::keyboard::read_char() {
            buf[0] = ch;
            Ok(1)
        } else {
            Ok(0)
        }
    }

    fn write(&mut self, _offset: usize, buf: &[u8]) -> Result<usize, &'static str> {
        if let Ok(s) = core::str::from_utf8(buf) {
            crate::print!("{}", s);
        }
        Ok(buf.len())
    }

    fn stat(&self) -> Stat {
        Stat { inode: 103, size: 0, file_type: FileType::CharDevice, mode: 0o666 }
    }
}

pub struct RtcDev;
impl InodeOperations for RtcDev {
    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize, &'static str> {
        let dt = crate::arch::x86_64::rtc::read_datetime();
        let s = format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC\n", dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second);
        let bytes = s.as_bytes();
        if offset >= bytes.len() {
            return Ok(0);
        }
        let avail = bytes.len() - offset;
        let n = avail.min(buf.len());
        buf[..n].copy_from_slice(&bytes[offset..offset + n]);
        Ok(n)
    }

    fn stat(&self) -> Stat {
        Stat { inode: 104, size: 24, file_type: FileType::CharDevice, mode: 0o444 }
    }
}

pub const FBIOGET_VSCREENINFO: u32 = 0x4600;
pub const FBIO_BLIT_RECT: u32 = 0x4601;
pub const FBIOGET_INFO: u32 = 0x4602;
pub const FBIO_FLUSH_RECT: u32 = 0x4603;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FbVarScreenInfo {
    pub xres: u32,
    pub yres: u32,
    pub bits_per_pixel: u32,
    pub line_length: u32,
    pub smem_start: u64,
    pub smem_len: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FbBlitRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub data_ptr: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FbInfo {
    pub width: u32,
    pub height: u32,
    pub pitch: u32,
    pub bpp: u32,
    pub red_pos: u8,
    pub red_mask: u8,
    pub green_pos: u8,
    pub green_mask: u8,
    pub blue_pos: u8,
    pub blue_mask: u8,
    pub lfb_phys: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct FbRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

pub struct FbDev;

fn ensure_vbe() {
    if crate::drivers::vbe::VBE.lock().framebuffer.is_null() {
        let _ = crate::drivers::vbe::init(1024, 768);
    }
}

impl InodeOperations for FbDev {
    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize, &'static str> {
        ensure_vbe();
        let vbe = crate::drivers::vbe::VBE.lock();
        let total_size = if vbe.pitch > 0 && vbe.height > 0 {
            (vbe.pitch * vbe.height) as usize
        } else {
            1024 * 768 * 4
        };

        // Prefer reading from fast RAM shadow buffer if GopConsole allocated one
        let gop_shadow = crate::drivers::gop_console::GOP_CONSOLE
            .try_lock()
            .and_then(|c| if !c.shadow_fb.is_null() { Some(c.shadow_fb as *const u8) } else { None });

        let read_ptr = if let Some(shadow_ptr) = gop_shadow {
            shadow_ptr
        } else if !vbe.framebuffer.is_null() {
            vbe.framebuffer as *const u8
        } else {
            crate::drivers::vbe::FRAMEBUFFER_VIRT_BASE as *const u8
        };
        drop(vbe);

        if offset >= total_size {
            return Ok(0);
        }
        let avail = total_size - offset;
        let count = avail.min(buf.len());
        unsafe {
            core::ptr::copy_nonoverlapping(read_ptr.add(offset), buf.as_mut_ptr(), count);
        }
        Ok(count)
    }

    fn write(&mut self, offset: usize, buf: &[u8]) -> Result<usize, &'static str> {
        ensure_vbe();
        let vbe = crate::drivers::vbe::VBE.lock();
        let total_size = if vbe.pitch > 0 && vbe.height > 0 {
            (vbe.pitch * vbe.height) as usize
        } else {
            1024 * 768 * 4
        };
        let fb_ptr = if !vbe.framebuffer.is_null() {
            vbe.framebuffer as *mut u8
        } else {
            crate::drivers::vbe::FRAMEBUFFER_VIRT_BASE as *mut u8
        };
        drop(vbe);

        if offset >= total_size {
            return Ok(0);
        }
        let avail = total_size - offset;
        let count = avail.min(buf.len());

        // Update RAM shadow buffer if present
        if let Some(con) = crate::drivers::gop_console::GOP_CONSOLE.try_lock() {
            if !con.shadow_fb.is_null() {
                unsafe {
                    core::ptr::copy_nonoverlapping(buf.as_ptr(), con.shadow_fb.add(offset), count);
                }
            }
        }

        // Burst write to hardware Write-Combining framebuffer
        unsafe {
            core::ptr::copy_nonoverlapping(buf.as_ptr(), fb_ptr.add(offset), count);
        }
        Ok(count)
    }

    fn ioctl(&mut self, cmd: u32, arg: usize) -> Result<isize, &'static str> {
        ensure_vbe();
        match cmd {
            FBIOGET_VSCREENINFO => {
                if arg == 0 {
                    return Err("Invalid user pointer for FBIOGET_VSCREENINFO");
                }
                let vbe = crate::drivers::vbe::VBE.lock();
                let width = if vbe.width > 0 { vbe.width } else { 1024 };
                let height = if vbe.height > 0 { vbe.height } else { 768 };
                let bpp = if vbe.bpp > 0 { vbe.bpp } else { 32 };
                let pitch = if vbe.pitch > 0 { vbe.pitch } else { width * 4 };
                let phys = vbe.lfb_phys;
                let smem_len = pitch * height;
                drop(vbe);

                let info = FbVarScreenInfo {
                    xres: width,
                    yres: height,
                    bits_per_pixel: bpp,
                    line_length: pitch,
                    smem_start: phys,
                    smem_len,
                };

                unsafe {
                    let ptr = arg as *mut FbVarScreenInfo;
                    *ptr = info;
                }
                Ok(0)
            }
            FBIOGET_INFO => {
                if arg == 0 {
                    return Err("Invalid user pointer for FBIOGET_INFO");
                }
                let vbe = crate::drivers::vbe::VBE.lock();
                let info = FbInfo {
                    width: if vbe.width > 0 { vbe.width } else { 1024 },
                    height: if vbe.height > 0 { vbe.height } else { 768 },
                    pitch: if vbe.pitch > 0 { vbe.pitch } else { 4096 },
                    bpp: if vbe.bpp > 0 { vbe.bpp } else { 32 },
                    red_pos: vbe.red_pos,
                    red_mask: vbe.red_mask,
                    green_pos: vbe.green_pos,
                    green_mask: vbe.green_mask,
                    blue_pos: vbe.blue_pos,
                    blue_mask: vbe.blue_mask,
                    lfb_phys: vbe.lfb_phys,
                };
                drop(vbe);
                unsafe {
                    let ptr = arg as *mut FbInfo;
                    *ptr = info;
                }
                Ok(0)
            }
            FBIO_BLIT_RECT => {
                if arg == 0 {
                    return Err("Invalid user pointer for FBIO_BLIT_RECT");
                }
                let blit = unsafe { &*(arg as *const FbBlitRect) };
                let vbe = crate::drivers::vbe::VBE.lock();
                let screen_w = if vbe.width > 0 { vbe.width } else { 1024 };
                let screen_h = if vbe.height > 0 { vbe.height } else { 768 };
                let pitch = if vbe.pitch > 0 { vbe.pitch } else { screen_w * 4 };
                let fb_ptr = if !vbe.framebuffer.is_null() {
                    vbe.framebuffer
                } else {
                    crate::drivers::vbe::FRAMEBUFFER_VIRT_BASE as *mut u32
                };
                drop(vbe);

                let x_end = blit.x.checked_add(blit.width);
                let y_end = blit.y.checked_add(blit.height);
                if x_end.map_or(true, |xe| xe > screen_w)
                    || y_end.map_or(true, |ye| ye > screen_h)
                    || blit.data_ptr == 0
                {
                    return Err("Blit rectangle out of bounds");
                }

                let src = blit.data_ptr as *const u32;
                let fb_bytes = fb_ptr as *mut u8;

                // Sync with RAM shadow backbuffer if present
                if let Some(con) = crate::drivers::gop_console::GOP_CONSOLE.try_lock() {
                    if !con.shadow_fb.is_null() {
                        let shadow_bytes = con.shadow_fb;
                        for row in 0..blit.height {
                            let byte_off = (blit.y + row) as usize * pitch as usize + (blit.x as usize * 4);
                            let dst_row = unsafe { shadow_bytes.add(byte_off) as *mut u32 };
                            let src_row = row * blit.width;
                            unsafe {
                                core::ptr::copy_nonoverlapping(
                                    src.add(src_row as usize),
                                    dst_row,
                                    blit.width as usize,
                                );
                            }
                        }
                    }
                }

                for row in 0..blit.height {
                    let dst_byte_offset = (blit.y + row) as usize * pitch as usize + (blit.x as usize * 4);
                    let dst_row_ptr = unsafe { fb_bytes.add(dst_byte_offset) as *mut u32 };
                    let src_row = row * blit.width;
                    unsafe {
                        core::ptr::copy_nonoverlapping(
                            src.add(src_row as usize),
                            dst_row_ptr,
                            blit.width as usize,
                        );
                    }
                }
                Ok(0)
            }
            FBIO_FLUSH_RECT => {
                if arg == 0 {
                    return Err("Invalid user pointer for FBIO_FLUSH_RECT");
                }
                let rect = unsafe { &*(arg as *const FbRect) };
                let vbe = crate::drivers::vbe::VBE.lock();
                let screen_w = if vbe.width > 0 { vbe.width } else { 1024 };
                let screen_h = if vbe.height > 0 { vbe.height } else { 768 };
                let pitch = if vbe.pitch > 0 { vbe.pitch } else { screen_w * 4 };
                drop(vbe);

                if rect.x >= screen_w || rect.y >= screen_h {
                    return Ok(0);
                }
                let flush_w = rect.width.min(screen_w - rect.x);
                let flush_h = rect.height.min(screen_h - rect.y);
                if flush_w == 0 || flush_h == 0 {
                    return Ok(0);
                }

                // Burst write damaged bounding box from RAM shadow buffer to Write-Combined hardware VRAM
                if let Some(con) = crate::drivers::gop_console::GOP_CONSOLE.try_lock() {
                    if !con.shadow_fb.is_null() && !con.fb_ptr.is_null() {
                        for row in 0..flush_h {
                            let py = (rect.y + row) as usize;
                            let px = rect.x as usize;
                            let offset = py * (pitch as usize) + px * 4;
                            let src = unsafe { con.shadow_fb.add(offset) as *const u64 };
                            let dst = unsafe { con.fb_ptr.add(offset) as *mut u64 };
                            let u64_count = (flush_w as usize * 4) / 8;
                            for i in 0..u64_count {
                                unsafe {
                                    core::ptr::write_volatile(dst.add(i), *src.add(i));
                                }
                            }
                            let rem_bytes = (flush_w as usize * 4) % 8;
                            if rem_bytes > 0 {
                                let rem_offset = u64_count * 8;
                                let s8 = unsafe { con.shadow_fb.add(offset + rem_offset) };
                                let d8 = unsafe { con.fb_ptr.add(offset + rem_offset) };
                                for i in 0..rem_bytes {
                                    unsafe {
                                        core::ptr::write_volatile(d8.add(i), *s8.add(i));
                                    }
                                }
                            }
                        }
                    }
                }
                Ok(0)
            }
            _ => Err("Unsupported ioctl command on /dev/fb0"),
        }
    }

    fn stat(&self) -> Stat {
        ensure_vbe();
        let vbe = crate::drivers::vbe::VBE.lock();
        let size = if vbe.pitch > 0 && vbe.height > 0 {
            (vbe.pitch * vbe.height) as usize
        } else {
            1024 * 768 * 4
        };
        Stat {
            inode: 105,
            size,
            file_type: FileType::CharDevice,
            mode: 0o666,
        }
    }
}

/// `/dev/backlight` character device controlling MacBookPro8,2 keyboard/panel backlight (0..255).
pub struct BacklightDev;

impl InodeOperations for BacklightDev {
    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize, &'static str> {
        let cur = crate::drivers::platform::apple::ApplePlatformHardware::get_keyboard_light();
        let s = alloc::format!("{}\n", cur);
        let bytes = s.as_bytes();
        if offset >= bytes.len() {
            return Ok(0);
        }
        let to_copy = (bytes.len() - offset).min(buf.len());
        buf[..to_copy].copy_from_slice(&bytes[offset..offset + to_copy]);
        Ok(to_copy)
    }

    fn write(&mut self, _offset: usize, buf: &[u8]) -> Result<usize, &'static str> {
        if buf.is_empty() {
            return Ok(0);
        }
        let val = if let Ok(text) = core::str::from_utf8(buf) {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                return Ok(buf.len());
            }
            if let Ok(parsed) = trimmed.parse::<u16>() {
                parsed.min(255) as u8
            } else if buf.len() == 1 {
                buf[0]
            } else {
                return Err("Invalid brightness value for /dev/backlight (expected 0..255)");
            }
        } else if buf.len() == 1 {
            buf[0]
        } else {
            return Err("Invalid non-UTF8 payload for /dev/backlight");
        };

        crate::drivers::platform::apple::ApplePlatformHardware::set_keyboard_light(val);
        Ok(buf.len())
    }

    fn stat(&self) -> Stat {
        Stat {
            inode: 106,
            size: 4,
            file_type: FileType::CharDevice,
            mode: 0o666,
        }
    }
}
