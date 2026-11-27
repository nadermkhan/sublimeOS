extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use crate::fs::vfs::{DirEntry, FileType, InodeOperations, Stat};
use crate::mm::pmm::PMM;
use crate::mm::{phys_to_virt, PAGE_SIZE};
use crate::sync::IrqSafeSpinlock;
use crate::kinfo;

pub const PROT_READ: i32 = 0x1;
pub const PROT_WRITE: i32 = 0x2;
pub const PROT_EXEC: i32 = 0x4;

pub const MAP_SHARED: i32 = 0x01;
pub const MAP_PRIVATE: i32 = 0x02;
pub const MAP_ANONYMOUS: i32 = 0x20;

/// In-memory POSIX shared memory object backed by real physical frames.
pub struct ShmObject {
    pub name: String,
    pub size: usize,
    pub frames: Vec<u64>,
}

impl ShmObject {
    pub fn new(name: &str) -> Self {
        Self {
            name: String::from(name),
            size: 0,
            frames: Vec::new(),
        }
    }

    /// Resize shared memory object and allocate/free physical PMM frames.
    pub fn resize(&mut self, new_size: usize) -> Result<(), &'static str> {
        let needed_pages = if new_size == 0 {
            0
        } else {
            (new_size + PAGE_SIZE - 1) / PAGE_SIZE
        };

        // Grow
        while self.frames.len() < needed_pages {
            let frame = PMM.lock().alloc_frame().ok_or("Out of physical memory for SHM")?;
            // Zero initialize frame
            unsafe {
                let ptr = phys_to_virt(frame) as *mut u8;
                core::ptr::write_bytes(ptr, 0, PAGE_SIZE);
            }
            self.frames.push(frame);
        }

        // Shrink (decrement refcount; frame is freed only when all mappings drop it)
        while self.frames.len() > needed_pages {
            if let Some(frame) = self.frames.pop() {
                PMM.lock().release_frame(frame);
            }
        }

        self.size = new_size;
        Ok(())
    }

    /// Blit ARGB32 (`0x00RRGGBB` / `0xAARRGGBB`) pixels directly from this SHM
    /// object's physical frames into a destination pixel buffer with clipping.
    pub fn blit_argb_to_canvas(
        &self,
        src_w: i32,
        src_h: i32,
        dst: &mut [u32],
        dst_w: i32,
        dst_h: i32,
        dst_x: i32,
        dst_y: i32,
        clip: (i32, i32, i32, i32),
    ) {
        if src_w <= 0 || src_h <= 0 || dst_w <= 0 || dst_h <= 0 || self.size < 4 {
            return;
        }
        let (cx0, cy0, cw, ch) = clip;
        if cw <= 0 || ch <= 0 {
            return;
        }
        let x0 = dst_x.max(cx0).max(0);
        let y0 = dst_y.max(cy0).max(0);
        let x1 = (dst_x + src_w).min(cx0 + cw).min(dst_w);
        let y1 = (dst_y + src_h).min(cy0 + ch).min(dst_h);
        if x0 >= x1 || y0 >= y1 {
            return;
        }

        let max_src_pixels = self.size / 4;
        for dy in y0..y1 {
            let sy = (dy - dst_y) as usize;
            let sx_start = (x0 - dst_x) as usize;
            let cols = (x1 - x0) as usize;
            let src_pix_start = sy * (src_w as usize) + sx_start;
            if src_pix_start >= max_src_pixels {
                break;
            }
            let avail_cols = cols.min(max_src_pixels - src_pix_start);
            let dst_row_start = (dy as usize) * (dst_w as usize) + (x0 as usize);
            if dst_row_start + avail_cols > dst.len() {
                break;
            }

            let mut copied = 0usize;
            while copied < avail_cols {
                let byte_off = (src_pix_start + copied) * 4;
                let page_idx = byte_off / PAGE_SIZE;
                let off_in_page = byte_off % PAGE_SIZE;
                if page_idx >= self.frames.len() {
                    return;
                }
                let page_bytes_left = PAGE_SIZE - off_in_page;
                let pix_in_chunk = (avail_cols - copied).min(page_bytes_left / 4);
                let virt = phys_to_virt(self.frames[page_idx]);
                if pix_in_chunk > 0 {
                    unsafe {
                        let src_ptr = (virt + off_in_page as u64) as *const u32;
                        core::ptr::copy_nonoverlapping(
                            src_ptr,
                            dst[dst_row_start + copied..].as_mut_ptr(),
                            pix_in_chunk,
                        );
                    }
                    copied += pix_in_chunk;
                } else {
                    // Straddles a page boundary only if unaligned (u32 is 4-byte aligned, so unreachable)
                    break;
                }
            }
        }
    }
}

impl InodeOperations for ShmObject {
    fn read(&self, offset: usize, buf: &mut [u8]) -> Result<usize, &'static str> {
        if offset >= self.size || buf.is_empty() {
            return Ok(0);
        }

        let to_read = buf.len().min(self.size - offset);
        let mut read_bytes = 0;

        while read_bytes < to_read {
            let curr_offset = offset + read_bytes;
            let page_idx = curr_offset / PAGE_SIZE;
            let offset_in_page = curr_offset % PAGE_SIZE;

            if page_idx >= self.frames.len() {
                break;
            }

            let frame = self.frames[page_idx];
            let virt_page = phys_to_virt(frame);
            let chunk = (to_read - read_bytes).min(PAGE_SIZE - offset_in_page);

            unsafe {
                let src = (virt_page + offset_in_page as u64) as *const u8;
                core::ptr::copy_nonoverlapping(src, buf[read_bytes..].as_mut_ptr(), chunk);
            }

            read_bytes += chunk;
        }

        Ok(read_bytes)
    }

    fn write(&mut self, offset: usize, buf: &[u8]) -> Result<usize, &'static str> {
        let end_pos = offset + buf.len();
        if end_pos > self.size {
            self.resize(end_pos)?;
        }

        let mut written_bytes = 0;
        while written_bytes < buf.len() {
            let curr_offset = offset + written_bytes;
            let page_idx = curr_offset / PAGE_SIZE;
            let offset_in_page = curr_offset % PAGE_SIZE;

            if page_idx >= self.frames.len() {
                break;
            }

            let frame = self.frames[page_idx];
            let virt_page = phys_to_virt(frame);
            let chunk = (buf.len() - written_bytes).min(PAGE_SIZE - offset_in_page);

            unsafe {
                let dst = (virt_page + offset_in_page as u64) as *mut u8;
                core::ptr::copy_nonoverlapping(buf[written_bytes..].as_ptr(), dst, chunk);
            }

            written_bytes += chunk;
        }

        Ok(written_bytes)
    }

    fn truncate(&mut self, size: usize) -> Result<(), &'static str> {
        self.resize(size)
    }

    fn get_physical_frames(&self) -> Option<alloc::vec::Vec<u64>> {
        Some(self.frames.clone())
    }

    fn stat(&self) -> Stat {
        Stat {
            inode: 8000 + (self.name.len() as u64),
            size: self.size,
            file_type: FileType::Regular,
            mode: 0o666,
        }
    }
}

pub struct ShmRegistry {
    pub objects: BTreeMap<String, Arc<IrqSafeSpinlock<ShmObject>>>,
}

impl ShmRegistry {
    pub const fn new() -> Self {
        Self {
            objects: BTreeMap::new(),
        }
    }

    pub fn open_or_create(&mut self, name: &str) -> Arc<IrqSafeSpinlock<ShmObject>> {
        if let Some(obj) = self.objects.get(name) {
            obj.clone()
        } else {
            let obj = Arc::new(IrqSafeSpinlock::new(ShmObject::new(name)));
            self.objects.insert(String::from(name), obj.clone());
            kinfo!("Created POSIX shared memory object: /dev/shm/{}", name);
            obj
        }
    }

    pub fn unlink(&mut self, name: &str) -> bool {
        self.objects.remove(name).is_some()
    }

    pub fn get(&self, name: &str) -> Option<Arc<IrqSafeSpinlock<ShmObject>>> {
        self.objects.get(name).cloned()
    }
}

pub static SHM_REGISTRY: IrqSafeSpinlock<ShmRegistry> = IrqSafeSpinlock::new(ShmRegistry::new());

/// Virtual `/dev/shm` directory node.
pub struct ShmDirectory;

impl InodeOperations for ShmDirectory {
    fn lookup(&self, name: &str) -> Result<Arc<IrqSafeSpinlock<dyn InodeOperations>>, &'static str> {
        let reg = SHM_REGISTRY.lock();
        if let Some(obj) = reg.get(name) {
            Ok(obj as Arc<IrqSafeSpinlock<dyn InodeOperations>>)
        } else {
            Err("Shared memory object not found")
        }
    }

    fn readdir(&self) -> Result<Vec<DirEntry>, &'static str> {
        let reg = SHM_REGISTRY.lock();
        let mut list = Vec::new();
        for (name, obj) in reg.objects.iter() {
            let stat = obj.lock().stat();
            list.push(DirEntry {
                name: name.clone(),
                inode: stat.inode,
                file_type: FileType::Regular,
            });
        }
        Ok(list)
    }

    fn create(&mut self, name: &str, _file_type: FileType) -> Result<Arc<IrqSafeSpinlock<dyn InodeOperations>>, &'static str> {
        let obj = SHM_REGISTRY.lock().open_or_create(name);
        Ok(obj as Arc<IrqSafeSpinlock<dyn InodeOperations>>)
    }

    fn unlink(&mut self, name: &str) -> Result<(), &'static str> {
        if SHM_REGISTRY.lock().unlink(name) {
            Ok(())
        } else {
            Err("No such file or directory")
        }
    }

    fn stat(&self) -> Stat {
        Stat {
            inode: 106,
            size: SHM_REGISTRY.lock().objects.len(),
            file_type: FileType::Directory,
            mode: 0o777,
        }
    }
}
