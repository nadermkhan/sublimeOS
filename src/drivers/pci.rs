extern crate alloc;
use alloc::vec::Vec;
use crate::arch::x86_64::io::{inl, outl};
use crate::sync::IrqSafeSpinlock;
use crate::kinfo;

const PCI_CONFIG_ADDRESS: u16 = 0xCF8;
const PCI_CONFIG_DATA: u16 = 0xCFC;

#[derive(Debug, Clone)]
pub struct PciDevice {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class_code: u8,
    pub subclass: u8,
    pub prog_if: u8,
    pub header_type: u8,
    pub bar0: u32,
}

impl PciDevice {
    pub fn class_name(&self) -> &'static str {
        match self.class_code {
            0x00 => "Legacy / Unclassified",
            0x01 => "Mass Storage Controller",
            0x02 => "Network Controller",
            0x03 => "Display Controller (VGA)",
            0x04 => "Multimedia Controller",
            0x05 => "Memory Controller",
            0x06 => "Bridge Device",
            0x07 => "Communication Controller",
            0x08 => "Generic System Peripheral",
            0x09 => "Input Device Controller",
            0x0C => "Serial Bus Controller (USB)",
            _ => "Unknown Controller",
        }
    }

    pub fn read_bar(&self, index: u8) -> u64 {
        if index > 5 {
            return 0;
        }
        let offset = 0x10 + index * 4;
        let bar_low = unsafe { PciBus::read_config_dword(self.bus, self.device, self.function, offset) };
        if bar_low == 0 || bar_low == 0xFFFF_FFFF {
            return 0;
        }
        if (bar_low & 0x01) != 0 {
            // I/O Space BAR: base address is bits 31..2
            (bar_low & 0xFFFF_FFFC) as u64
        } else if (bar_low & 0x06) == 0x04 && index < 5 {
            // 64-bit Memory Space BAR
            let bar_high = unsafe { PciBus::read_config_dword(self.bus, self.device, self.function, offset + 4) };
            ((bar_high as u64) << 32) | ((bar_low & 0xFFFF_FFF0) as u64)
        } else {
            // 32-bit Memory Space BAR
            (bar_low & 0xFFFF_FFF0) as u64
        }
    }

    /// Walk standard PCI Capabilities (offset 0x34) looking for `cap_id` (bounded to 48 hops).
    pub fn find_capability(&self, cap_id: u8) -> Option<u8> {
        unsafe {
            let status = PciBus::read_config_word(self.bus, self.device, self.function, 0x06);
            if (status & 0x0010) == 0 {
                return None;
            }
            let mut ptr = PciBus::read_config_byte(self.bus, self.device, self.function, 0x34) & 0xFC;
            for _ in 0..48 {
                if ptr < 0x40 || ptr == 0xFC {
                    break;
                }
                let id = PciBus::read_config_byte(self.bus, self.device, self.function, ptr);
                if id == cap_id {
                    return Some(ptr);
                }
                let next = PciBus::read_config_byte(self.bus, self.device, self.function, ptr + 1) & 0xFC;
                if next == ptr {
                    break;
                }
                ptr = next;
            }
        }
        None
    }

    /// Ensure the PCI function is in D0 power state if a PCI Power Management capability (0x01) exists.
    pub fn ensure_d0_power_state(&self) {
        if let Some(pm_cap) = self.find_capability(0x01) {
            unsafe {
                let pmcsr = PciBus::read_config_word(self.bus, self.device, self.function, pm_cap + 4);
                if (pmcsr & 0x0003) != 0 {
                    PciBus::write_config_word(self.bus, self.device, self.function, pm_cap + 4, pmcsr & !0x0003);
                    crate::arch::x86_64::pit::delay_ms(10);
                }
            }
        }
    }
}

pub struct PciBus {
    pub devices: Vec<PciDevice>,
}

impl PciBus {
    pub const fn new() -> Self {
        Self { devices: Vec::new() }
    }

    pub unsafe fn read_config_dword(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
        let address = ((bus as u32) << 16)
            | ((device as u32) << 11)
            | ((function as u32) << 8)
            | ((offset as u32) & 0xFC)
            | 0x80000000;

        outl(PCI_CONFIG_ADDRESS, address);
        inl(PCI_CONFIG_DATA)
    }

    pub unsafe fn read_config_word(bus: u8, device: u8, function: u8, offset: u8) -> u16 {
        let val = Self::read_config_dword(bus, device, function, offset);
        let shift = ((offset & 2) * 8) as u32;
        ((val >> shift) & 0xFFFF) as u16
    }

    pub unsafe fn read_config_byte(bus: u8, device: u8, function: u8, offset: u8) -> u8 {
        let val = Self::read_config_dword(bus, device, function, offset);
        let shift = ((offset & 3) * 8) as u32;
        ((val >> shift) & 0xFF) as u8
    }

    pub unsafe fn write_config_dword(bus: u8, device: u8, function: u8, offset: u8, value: u32) {
        let address = ((bus as u32) << 16)
            | ((device as u32) << 11)
            | ((function as u32) << 8)
            | ((offset as u32) & 0xFC)
            | 0x80000000;

        outl(PCI_CONFIG_ADDRESS, address);
        outl(PCI_CONFIG_DATA, value);
    }

    pub unsafe fn write_config_word(bus: u8, device: u8, function: u8, offset: u8, value: u16) {
        let address = ((bus as u32) << 16)
            | ((device as u32) << 11)
            | ((function as u32) << 8)
            | ((offset as u32) & 0xFC)
            | 0x80000000;

        outl(PCI_CONFIG_ADDRESS, address);
        let orig = inl(PCI_CONFIG_DATA);
        let shift = ((offset & 2) * 8) as u32;
        let mask = !(0xFFFFu32 << shift);
        let new_val = (orig & mask) | ((value as u32) << shift);
        outl(PCI_CONFIG_ADDRESS, address);
        outl(PCI_CONFIG_DATA, new_val);
    }

    pub unsafe fn write_config_byte(bus: u8, device: u8, function: u8, offset: u8, value: u8) {
        let address = ((bus as u32) << 16)
            | ((device as u32) << 11)
            | ((function as u32) << 8)
            | ((offset as u32) & 0xFC)
            | 0x80000000;

        outl(PCI_CONFIG_ADDRESS, address);
        let orig = inl(PCI_CONFIG_DATA);
        let shift = ((offset & 3) * 8) as u32;
        let mask = !(0xFFu32 << shift);
        let new_val = (orig & mask) | ((value as u32) << shift);
        outl(PCI_CONFIG_ADDRESS, address);
        outl(PCI_CONFIG_DATA, new_val);
    }

    pub unsafe fn enable_bus_mastering(bus: u8, device: u8, function: u8) {
        let cmd = Self::read_config_dword(bus, device, function, 0x04);
        // Bit 1: Memory Space Enable (0x02), Bit 2: Bus Master Enable (0x04)
        Self::write_config_word(bus, device, function, 0x04, (cmd as u16) | 0x0006);
    }

    pub fn scan_bus(&mut self) {
        self.devices.clear();

        for bus in 0..=7 {
            for dev in 0..32 {
                for func in 0..8 {
                    let dword0 = unsafe { Self::read_config_dword(bus, dev, func, 0x00) };
                    let vendor_id = (dword0 & 0xFFFF) as u16;
                    if vendor_id == 0xFFFF || vendor_id == 0x0000 {
                        if func == 0 {
                            break;
                        }
                        continue;
                    }

                    let device_id = ((dword0 >> 16) & 0xFFFF) as u16;

                    let dword2 = unsafe { Self::read_config_dword(bus, dev, func, 0x08) };
                    let prog_if = ((dword2 >> 8) & 0xFF) as u8;
                    let subclass = ((dword2 >> 16) & 0xFF) as u8;
                    let class_code = ((dword2 >> 24) & 0xFF) as u8;

                    let dword3 = unsafe { Self::read_config_dword(bus, dev, func, 0x0C) };
                    let raw_header_type = ((dword3 >> 16) & 0xFF) as u8;
                    let header_type = raw_header_type & 0x7F;

                    let bar0 = unsafe { Self::read_config_dword(bus, dev, func, 0x10) } & 0xFFFFFFF0;

                    self.devices.push(PciDevice {
                        bus,
                        device: dev,
                        function: func,
                        vendor_id,
                        device_id,
                        class_code,
                        subclass,
                        prog_if,
                        header_type,
                        bar0,
                    });

                    // Single-function PCIe endpoints may alias reads across func 1..7 if bit 7 is clear
                    if func == 0 && (raw_header_type & 0x80) == 0 {
                        break;
                    }
                }
            }
        }
    }
}

pub static PCI: IrqSafeSpinlock<PciBus> = IrqSafeSpinlock::new(PciBus::new());

pub fn init() {
    let mut pci = PCI.lock();
    pci.scan_bus();
    kinfo!("PCI bus scan complete: found {} device(s)", pci.devices.len());
}
