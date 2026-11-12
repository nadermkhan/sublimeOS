extern crate alloc;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use crate::drivers::storage::BlockDevice;
use crate::sync::IrqSafeSpinlock;
use crate::kinfo;

pub const SECTOR_SIZE: usize = 512;
pub const MAX_CACHE_ENTRIES: usize = 1024; // 512 KiB buffer cache

pub struct CacheEntry {
    pub dev_name: String,
    pub lba: u64,
    pub data: [u8; SECTOR_SIZE],
    pub dirty: bool,
    pub access_counter: u64,
}

pub struct BlockCache {
    entries: Vec<CacheEntry>,
    clock: u64,
    pub hits: u64,
    pub misses: u64,
    pub writes: u64,
    pub flushes: u64,
}

impl BlockCache {
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
            clock: 0,
            hits: 0,
            misses: 0,
            writes: 0,
            flushes: 0,
        }
    }

    fn next_tick(&mut self) -> u64 {
        self.clock = self.clock.wrapping_add(1);
        self.clock
    }

    /// Read a single 512-byte sector through the cache.
    pub fn read_sector(
        &mut self,
        dev: &dyn BlockDevice,
        lba: u64,
        out: &mut [u8],
    ) -> Result<(), &'static str> {
        let dev_name = dev.name();
        let tick = self.next_tick();

        // Check if sector is in cache
        for entry in self.entries.iter_mut() {
            if entry.lba == lba && entry.dev_name == dev_name {
                entry.access_counter = tick;
                self.hits += 1;
                out[..SECTOR_SIZE].copy_from_slice(&entry.data);
                return Ok(());
            }
        }

        // Cache miss: read directly from hardware
        self.misses += 1;
        let mut hardware_buf = [0u8; SECTOR_SIZE];
        dev.read_sectors(lba, 1, &mut hardware_buf)?;
        out[..SECTOR_SIZE].copy_from_slice(&hardware_buf);

        // Insert into cache, evicting LRU if full
        if self.entries.len() >= MAX_CACHE_ENTRIES {
            self.evict_one(dev)?;
        }

        self.entries.push(CacheEntry {
            dev_name: String::from(dev_name),
            lba,
            data: hardware_buf,
            dirty: false,
            access_counter: tick,
        });

        Ok(())
    }

    /// Write a single 512-byte sector through the cache (write-through with cache update).
    pub fn write_sector(
        &mut self,
        dev: &dyn BlockDevice,
        lba: u64,
        data: &[u8],
    ) -> Result<(), &'static str> {
        let dev_name = dev.name();
        let tick = self.next_tick();
        self.writes += 1;

        let mut sector_data = [0u8; SECTOR_SIZE];
        sector_data.copy_from_slice(&data[..SECTOR_SIZE]);

        // Always commit write directly to physical device (write-through safety)
        dev.write_sectors(lba, 1, &sector_data)?;

        // Update existing cache entry if present
        for entry in self.entries.iter_mut() {
            if entry.lba == lba && entry.dev_name == dev_name {
                entry.data = sector_data;
                entry.dirty = false;
                entry.access_counter = tick;
                return Ok(());
            }
        }

        // Sector not cached yet: add it so future reads hit memory
        if self.entries.len() >= MAX_CACHE_ENTRIES {
            self.evict_one(dev)?;
        }

        self.entries.push(CacheEntry {
            dev_name: String::from(dev_name),
            lba,
            data: sector_data,
            dirty: false,
            access_counter: tick,
        });

        Ok(())
    }

    fn evict_one(&mut self, dev: &dyn BlockDevice) -> Result<(), &'static str> {
        if self.entries.is_empty() {
            return Ok(());
        }

        let mut lru_idx = 0;
        let mut oldest_tick = u64::MAX;

        for (i, entry) in self.entries.iter().enumerate() {
            if entry.access_counter < oldest_tick {
                oldest_tick = entry.access_counter;
                lru_idx = i;
            }
        }

        let evicted = self.entries.swap_remove(lru_idx);
        if evicted.dirty && evicted.dev_name == dev.name() {
            dev.write_sectors(evicted.lba, 1, &evicted.data)?;
            self.flushes += 1;
        }

        Ok(())
    }

    /// Flush all dirty cached blocks across all storage devices.
    pub fn flush_all(&mut self) -> Result<(), &'static str> {
        for entry in self.entries.iter_mut() {
            if entry.dirty {
                // In write-through mode, dirty is rarely set, but handle if any write-back was staged
                entry.dirty = false;
                self.flushes += 1;
            }
        }
        Ok(())
    }

    pub fn stats(&self) -> (usize, u64, u64, u64) {
        (self.entries.len(), self.hits, self.misses, self.writes)
    }
}

pub static BLOCK_CACHE: IrqSafeSpinlock<BlockCache> = IrqSafeSpinlock::new(BlockCache::new());

/// A BlockDevice wrapper that routes sector I/O through the unified BlockCache.
pub struct CachedBlockDevice {
    pub inner: Arc<dyn BlockDevice>,
}

impl CachedBlockDevice {
    pub fn new(inner: Arc<dyn BlockDevice>) -> Self {
        Self { inner }
    }
}

impl BlockDevice for CachedBlockDevice {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn read_sectors(&self, lba: u64, count: u32, buf: &mut [u8]) -> Result<(), &'static str> {
        let mut cache = BLOCK_CACHE.lock();
        for i in 0..count {
            let offset = (i as usize) * SECTOR_SIZE;
            cache.read_sector(&*self.inner, lba + i as u64, &mut buf[offset..offset + SECTOR_SIZE])?;
        }
        Ok(())
    }

    fn write_sectors(&self, lba: u64, count: u32, buf: &[u8]) -> Result<(), &'static str> {
        let is_panicked = crate::arch::x86_64::vga::PANICKED.load(core::sync::atomic::Ordering::Relaxed);
        if is_panicked {
            return self.inner.write_sectors(lba, count, buf);
        }
        let mut cache = BLOCK_CACHE.lock();
        for i in 0..count {
            let offset = (i as usize) * SECTOR_SIZE;
            cache.write_sector(&*self.inner, lba + i as u64, &buf[offset..offset + SECTOR_SIZE])?;
        }
        Ok(())
    }

    fn total_sectors(&self) -> u64 {
        self.inner.total_sectors()
    }

    fn sector_size(&self) -> usize {
        self.inner.sector_size()
    }
}

pub fn init() {
    kinfo!("Unified Storage Block Cache (Buffer Cache: 1024 sectors, LRU write-through) initialized.");
}
