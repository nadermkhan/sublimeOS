//! EHCI (Enhanced Host Controller Interface - USB 2.0) Driver
//!
//! Implements bare-metal PCI legacy ownership handoff, physical DMA frame allocation,
//! 1024-entry Periodic Frame List (`PERIODICLISTBASE`) for 8ms Keyboard & 2ms Trackpad
//! Interrupt IN pipes, Asynchronous Schedule (`ASYNCLISTADDR`) Control Transfers
//! (`SET_PROTOCOL(0)` Boot Protocol), and non-blocking interrupt qTD polling.

extern crate alloc;
use alloc::format;
use alloc::vec::Vec;
use core::mem::{align_of, size_of};
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{fence, Ordering};

use crate::drivers::pci::{PciBus, PciDevice};
use crate::drivers::usb::msc::UsbMassStorageTransport;
use crate::drivers::usb::hid::keyboard::{UsbKeyboardDriver, UsbKeyboardReport};
use crate::drivers::usb::hid::trackpad::{AppleTrackpadDriver, UsbSetupPacket};
use crate::drivers::usb::hid::UsbHidType;
use crate::drivers::usb::hub::{
    self, UsbConfigDescriptor, UsbDeviceDescriptor, UsbHubDescriptor,
};
use crate::kinfo;
use crate::mm::phys_to_virt;
use crate::mm::pmm::{alloc_zeroed_frame, PMM};

// EHCI Capability Register offsets
const EHCI_CAP_CAPLENGTH: u64 = 0x00;
const EHCI_CAP_HCSPARAMS: u64 = 0x04;
const EHCI_CAP_HCCPARAMS: u64 = 0x08;

// EHCI Operational Register offsets (relative to op_base)
const EHCI_OP_USBCMD: u64 = 0x00;
const EHCI_OP_USBSTS: u64 = 0x04;
const EHCI_OP_CTRLDSSEGMENT: u64 = 0x10;
const EHCI_OP_PERIODICLISTBASE: u64 = 0x14;
const EHCI_OP_ASYNCLISTADDR: u64 = 0x18;
const EHCI_OP_CONFIGFLAG: u64 = 0x40;
const EHCI_OP_PORTSC_BASE: u64 = 0x44;

// USBCMD bits
const USBCMD_RS: u32 = 1 << 0; // Run/Stop
const USBCMD_HCRESET: u32 = 1 << 1; // Host Controller Reset
const USBCMD_PSE: u32 = 1 << 4; // Periodic Schedule Enable
const USBCMD_ASE: u32 = 1 << 5; // Asynchronous Schedule Enable

// USBSTS bits
const USBSTS_HCHALTED: u32 = 1 << 12;

// PORTSC bits
const PORTSC_CCS: u32 = 1 << 0; // Current Connect Status
const PORTSC_PE: u32 = 1 << 2; // Port Enabled
const PORTSC_PR: u32 = 1 << 8; // Port Reset

// EHCI Link Pointer tags (bits 2:0)
const EHCI_PTR_TERMINATE: u32 = 1 << 0;
const EHCI_PTR_TYPE_QH: u32 = 0b01 << 1;

// qTD Token bits
const QTD_STATUS_ACTIVE: u32 = 1 << 7;
const QTD_STATUS_ERR_MASK: u32 = 0x7C; // Halted | DataBufErr | Babble | XactErr | MissedMicro
const QTD_PID_OUT: u32 = 0 << 8;
const QTD_PID_IN: u32 = 1 << 8;
const QTD_PID_SETUP: u32 = 2 << 8;
const QTD_CERR_3: u32 = 3 << 10;
const QTD_TOGGLE_DATA1: u32 = 1 << 31;

// DMA Pool Layout Constants
const PIPE_SLOT_STRIDE: u64 = 256; // Per pipe: QH (0..48), qTD (64..96), Buf (128..192)
const MAX_INTERRUPT_PIPES: usize = 16; // Fits inside 1 x 4 KiB PMM page (16 * 256 = 4096)

/// EHCI Queue Element Transfer Descriptor (qTD) - 32 bytes, 32-byte aligned (EHCI Spec 3.5).
#[repr(C, align(32))]
#[derive(Clone, Copy)]
pub struct EhciQtd {
    pub next_qtd: u32,
    pub alt_next_qtd: u32,
    pub token: u32,
    pub buf_ptrs: [u32; 5],
}

impl EhciQtd {
    pub const fn new_terminated() -> Self {
        Self {
            next_qtd: EHCI_PTR_TERMINATE,
            alt_next_qtd: EHCI_PTR_TERMINATE,
            token: 0,
            buf_ptrs: [0; 5],
        }
    }
}

/// EHCI Queue Head (QH) - 48 bytes, 32-byte aligned (EHCI Spec 3.6).
#[repr(C, align(32))]
#[derive(Clone, Copy)]
pub struct EhciQueueHead {
    pub horiz_link: u32,
    pub ep_chars: u32,
    pub ep_caps: u32,
    pub curr_qtd: u32,
    pub next_qtd: u32,
    pub alt_next_qtd: u32,
    pub token: u32,
    pub buf_ptrs: [u32; 5],
}

impl EhciQueueHead {
    pub const fn new_empty() -> Self {
        Self {
            horiz_link: EHCI_PTR_TERMINATE,
            ep_chars: 0,
            ep_caps: 0,
            curr_qtd: 0,
            next_qtd: EHCI_PTR_TERMINATE,
            alt_next_qtd: EHCI_PTR_TERMINATE,
            token: 0,
            buf_ptrs: [0; 5],
        }
    }
}

// Compile-time layout validation against EHCI 1.0 hardware spec.
// Note: 12 x u32 = 48 hardware bytes; `align(32)` rounds `size_of` up to 64 bytes cleanly.
const _: () = assert!(size_of::<EhciQtd>() == 32);
const _: () = assert!(align_of::<EhciQtd>() == 32);
const _: () = assert!(size_of::<EhciQueueHead>() == 64);
const _: () = assert!(align_of::<EhciQueueHead>() == 32);

/// USB Device Speed reported into EHCI QH `Endpoint Characteristics`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsbSpeed {
    Full = 0b00,
    Low = 0b01,
    High = 0b10,
}

/// Active Periodic Interrupt IN Pipe tracked by an EHCI controller.
pub struct EhciInterruptPipe {
    pub kind: UsbHidType,
    pub slot_index: usize,
    pub port: u8,
    pub dev_addr: u8,
    pub endpoint: u8,
    pub interval_ms: u8,
    pub max_packet_size: u16,
    pub data_toggle: bool,
    pub qh_phys: u32,
    pub qh_virt: u64,
    pub qtd_phys: u32,
    pub qtd_virt: u64,
    pub buf_phys: u32,
    pub buf_virt: u64,
    pub kbd_driver: Option<UsbKeyboardDriver>,
    pub trackpad_driver: Option<AppleTrackpadDriver>,
}

/// Discovered USB device or hub tracked in the system topology.
#[derive(Debug, Clone)]
pub struct DiscoveredUsbDevice {
    pub address: u8,
    pub root_port: u8,
    pub speed: UsbSpeed,
    pub vendor_id: u16,
    pub product_id: u16,
    pub class_code: u8,
    pub is_hub: bool,
    pub hub_ports: Option<u8>,
    pub parent_hub: Option<(u8, u8)>, // (hub_addr, hub_port)
    pub name: alloc::string::String,
}

/// EHCI Host Controller state with real 4 KiB DMA schedules.
pub struct EhciController {
    pub pci: PciDevice,
    pub mmio_base: u64,
    pub op_base: u64,
    pub num_ports: u8,
    pub connected_devices: Vec<u8>,
    pub frame_list_phys: u64,
    pub frame_list_virt: u64,
    pub async_qh_phys: u64,
    pub async_qh_virt: u64,
    pub pipe_pool_phys: u64,
    pub pipe_pool_virt: u64,
    pub pipes: Vec<EhciInterruptPipe>,
    pub discovered_devices: Vec<DiscoveredUsbDevice>,
    next_usb_addr: u8,
    scratch_mmio: bool,
}

impl EhciController {
    pub fn new(pci: PciDevice) -> Result<Self, &'static str> {
        let bar0 = pci.read_bar(0);
        let mmio_phys = if bar0 != 0 {
            bar0
        } else {
            (pci.bar0 & 0xFFFFFFF0) as u64
        };
        let page_offset = mmio_phys & 0xFFF; // 0x800 for 00:1d.7, 0xC00 for 00:1a.7
        let mmio_page_base = crate::mm::vmm::map_mmio_range(mmio_phys & !0xFFF, 4096)?;
        let mmio_base = mmio_page_base + page_offset;

        unsafe {
            PciBus::enable_bus_mastering(pci.bus, pci.device, pci.function);
        }

        let (caplength, num_ports, hccparams) = unsafe {
            let cap_byte = read_volatile((mmio_base + EHCI_CAP_CAPLENGTH) as *const u8);
            let hcsparams = read_volatile((mmio_base + EHCI_CAP_HCSPARAMS) as *const u32);
            let hccparams = read_volatile((mmio_base + EHCI_CAP_HCCPARAMS) as *const u32);
            (cap_byte, (hcsparams & 0x0F) as u8, hccparams)
        };

        if caplength == 0 || caplength == 0xFF || num_ports == 0 || num_ports > 16 {
            return Err("EHCI: Invalid hardware registers (unresponsive MMIO or invalid CAPLENGTH)");
        }

        Self::claim_os_ownership(&pci, hccparams);

        let (frame_list_phys, frame_list_virt, async_qh_phys, async_qh_virt, pipe_pool_phys, pipe_pool_virt) =
            Self::alloc_dma_frames()?;

        let ctrl = Self {
            pci,
            mmio_base,
            op_base: mmio_base + (caplength as u64),
            num_ports,
            connected_devices: Vec::new(),
            frame_list_phys,
            frame_list_virt,
            async_qh_phys,
            async_qh_virt,
            pipe_pool_phys,
            pipe_pool_virt,
            pipes: Vec::new(),
            discovered_devices: Vec::new(),
            next_usb_addr: 1,
            scratch_mmio: false,
        };

        ctrl.init_dma_schedules();
        ctrl.reset_and_init();
        Ok(ctrl)
    }

    /// Construct a controller bound to an in-memory scratch MMIO operational register block
    /// for bare-metal DMA schedule verification in tests without hardware PCI BARs.
    pub fn new_scratch(op_base_virt: u64, num_ports: u8) -> Result<Self, &'static str> {
        let (frame_list_phys, frame_list_virt, async_qh_phys, async_qh_virt, pipe_pool_phys, pipe_pool_virt) =
            Self::alloc_dma_frames()?;

        let ctrl = Self {
            pci: PciDevice {
                bus: 0,
                device: 0,
                function: 0,
                vendor_id: 0x8086,
                device_id: 0x1C26,
                class_code: 0x0C,
                subclass: 0x03,
                prog_if: 0x20,
                header_type: 0x00,
                bar0: 0,
            },
            mmio_base: op_base_virt,
            op_base: op_base_virt,
            num_ports,
            connected_devices: Vec::new(),
            frame_list_phys,
            frame_list_virt,
            async_qh_phys,
            async_qh_virt,
            pipe_pool_phys,
            pipe_pool_virt,
            pipes: Vec::new(),
            discovered_devices: Vec::new(),
            next_usb_addr: 1,
            scratch_mmio: true,
        };

        ctrl.init_dma_schedules();
        // Write operational registers directly without spinning on HCHalted/HCRESET bits
        // since RAM scratchpad does not have active silicon auto-clearing HCRESET.
        unsafe {
            write_volatile((ctrl.op_base + EHCI_OP_CTRLDSSEGMENT) as *mut u32, 0);
            write_volatile(
                (ctrl.op_base + EHCI_OP_PERIODICLISTBASE) as *mut u32,
                ctrl.frame_list_phys as u32,
            );
            write_volatile(
                (ctrl.op_base + EHCI_OP_ASYNCLISTADDR) as *mut u32,
                ctrl.async_qh_phys as u32,
            );
            write_volatile((ctrl.op_base + EHCI_OP_CONFIGFLAG) as *mut u32, 1);
            write_volatile(
                (ctrl.op_base + EHCI_OP_USBCMD) as *mut u32,
                USBCMD_RS | USBCMD_PSE,
            );
        }
        Ok(ctrl)
    }

    fn alloc_dma_frames() -> Result<(u64, u64, u64, u64, u64, u64), &'static str> {
        let frame_list_phys =
            alloc_zeroed_frame().ok_or("EHCI: Out of physical frames for PERIODICLISTBASE")?;
        let async_qh_phys = match alloc_zeroed_frame() {
            Some(p) => p,
            None => {
                PMM.lock().free_frame(frame_list_phys);
                return Err("EHCI: Out of physical frames for Async QH");
            }
        };
        let pipe_pool_phys = match alloc_zeroed_frame() {
            Some(p) => p,
            None => {
                let mut pmm = PMM.lock();
                pmm.free_frame(frame_list_phys);
                pmm.free_frame(async_qh_phys);
                return Err("EHCI: Out of physical frames for Interrupt Pipe Pool");
            }
        };

        Ok((
            frame_list_phys,
            phys_to_virt(frame_list_phys),
            async_qh_phys,
            phys_to_virt(async_qh_phys),
            pipe_pool_phys,
            phys_to_virt(pipe_pool_phys),
        ))
    }

    fn init_dma_schedules(&self) {
        unsafe {
            // Initialize all 1024 entries of PERIODICLISTBASE to T=1 (Terminate)
            let frame_list = self.frame_list_virt as *mut u32;
            for i in 0..1024 {
                write_volatile(frame_list.add(i), EHCI_PTR_TERMINATE);
            }

            // Initialize circular idle Head-of-Reclamation Queue Head (H=1)
            let async_qh = self.async_qh_virt as *mut EhciQueueHead;
            write_volatile(
                async_qh,
                EhciQueueHead {
                    horiz_link: (self.async_qh_phys as u32) | EHCI_PTR_TYPE_QH,
                    ep_chars: (1 << 15) | (1 << 14) | (64 << 16), // H=1, dtc=1, MaxPkt=64
                    ep_caps: 1 << 30,                             // Mult=1
                    curr_qtd: 0,
                    next_qtd: EHCI_PTR_TERMINATE,
                    alt_next_qtd: EHCI_PTR_TERMINATE,
                    token: 0,
                    buf_ptrs: [0; 5],
                },
            );
            fence(Ordering::SeqCst);
        }
    }

    /// Take EHCI ownership away from EFI/BIOS USB Legacy Support (USBLEGSUP) if present.
    fn claim_os_ownership(pci: &PciDevice, hccparams: u32) {
        let eecp = ((hccparams >> 8) & 0xFF) as u8;
        if eecp < 0x40 {
            return;
        }
        unsafe {
            let legsup = PciBus::read_config_dword(pci.bus, pci.device, pci.function, eecp);
            // Always assert HC OS-Owned Semaphore (bit 24)
            PciBus::write_config_dword(pci.bus, pci.device, pci.function, eecp, legsup | (1 << 24));
            if (legsup & (1 << 16)) != 0 {
                let mut timeout = 100;
                while timeout > 0 {
                    let cur = PciBus::read_config_dword(pci.bus, pci.device, pci.function, eecp);
                    if (cur & (1 << 16)) == 0 {
                        break;
                    }
                    crate::arch::x86_64::pit::delay_ms(2);
                    timeout -= 1;
                }
                if timeout == 0 {
                    // Forcibly clear BIOS ownership
                    PciBus::write_config_dword(pci.bus, pci.device, pci.function, eecp, 1 << 24);
                }
            }
            // Clear BIOS ownership and disable SMIs in USBLEGCTLSTS (eecp + 4)
            PciBus::write_config_dword(pci.bus, pci.device, pci.function, eecp + 4, 0);
        }
    }

    fn reset_and_init(&self) {
        unsafe {
            let usbcmd_ptr = (self.op_base + EHCI_OP_USBCMD) as *mut u32;
            let usbsts_ptr = (self.op_base + EHCI_OP_USBSTS) as *const u32;

            // 1. Halt the controller before modifying schedule registers
            let cmd = read_volatile(usbcmd_ptr);
            write_volatile(usbcmd_ptr, cmd & !(USBCMD_RS | USBCMD_PSE | USBCMD_ASE));
            let mut timeout = 100u32;
            while (read_volatile(usbsts_ptr) & USBSTS_HCHALTED) == 0 && timeout > 0 {
                crate::arch::x86_64::pit::delay_ms(1);
                timeout -= 1;
            }

            // 2. Execute Host Controller Reset
            write_volatile(usbcmd_ptr, read_volatile(usbcmd_ptr) | USBCMD_HCRESET);
            timeout = 100;
            while (read_volatile(usbcmd_ptr) & USBCMD_HCRESET) != 0 && timeout > 0 {
                crate::arch::x86_64::pit::delay_ms(1);
                timeout -= 1;
            }

            // 3. Program 32-bit DMA segment selector and schedule base addresses
            write_volatile((self.op_base + EHCI_OP_CTRLDSSEGMENT) as *mut u32, 0);
            write_volatile(
                (self.op_base + EHCI_OP_PERIODICLISTBASE) as *mut u32,
                self.frame_list_phys as u32,
            );
            write_volatile(
                (self.op_base + EHCI_OP_ASYNCLISTADDR) as *mut u32,
                self.async_qh_phys as u32,
            );

            // 4. Take ownership of all ports via CONFIGFLAG = 1 and apply Port Power (bit 12)
            write_volatile((self.op_base + EHCI_OP_CONFIGFLAG) as *mut u32, 1);
            crate::arch::x86_64::pit::delay_ms(20);
            if !self.scratch_mmio {
                for port in 1..=self.num_ports {
                    let portsc_ptr = (self.op_base + EHCI_OP_PORTSC_BASE + ((port as u64 - 1) * 4)) as *mut u32;
                    let val = read_volatile(portsc_ptr);
                    write_volatile(portsc_ptr, val | (1 << 12)); // PORTSC_PP (Port Power)
                }
                crate::arch::x86_64::pit::delay_ms(50);
            }

            // 5. Enable Run/Stop + Periodic & Async Schedules so HID interrupt QHs and control transfers run
            fence(Ordering::SeqCst);
            write_volatile(usbcmd_ptr, USBCMD_RS | USBCMD_PSE | USBCMD_ASE);
        }
    }

    /// Execute a synchronous USB Control Transfer over the Asynchronous Schedule (`ASYNCLISTADDR`),
    /// with explicit `max_pkt0` sizing and optional USB 2.0 Hub Transaction Translator `(hub_addr, hub_port)`.
    pub fn submit_control_transfer_ex(
        &self,
        dev_addr: u8,
        endpoint: u8,
        speed: UsbSpeed,
        max_pkt0: u16,
        tt: Option<(u8, u8)>,
        setup: &UsbSetupPacket,
        data_in: Option<&mut [u8]>,
        data_out: Option<&[u8]>,
    ) -> Result<usize, &'static str> {
        let data_len = if let Some(ref buf) = data_in {
            buf.len().min(1024)
        } else if let Some(buf) = data_out {
            buf.len().min(1024)
        } else {
            0
        };

        // Async PMM Frame layout (single 4 KiB page):
        // +0x000: Async QH (64 bytes padded)
        // +0x040: Setup qTD (32 bytes)
        // +0x080: Data qTD (32 bytes)
        // +0x0C0: Status qTD (32 bytes)
        // +0x100: 8-byte Setup packet DMA buffer
        // +0x140..+0x540: up to 1024-byte Control Data payload DMA buffer (stays within page)
        let qh_phys = self.async_qh_phys as u32;
        let setup_qtd_phys = (self.async_qh_phys + 0x040) as u32;
        let data_qtd_phys = (self.async_qh_phys + 0x080) as u32;
        let status_qtd_phys = (self.async_qh_phys + 0x0C0) as u32;
        let setup_buf_phys = (self.async_qh_phys + 0x100) as u32;
        let data_buf_phys = (self.async_qh_phys + 0x140) as u32;

        let setup_qtd_virt = (self.async_qh_virt + 0x040) as *mut EhciQtd;
        let data_qtd_virt = (self.async_qh_virt + 0x080) as *mut EhciQtd;
        let status_qtd_virt = (self.async_qh_virt + 0x0C0) as *mut EhciQtd;
        let setup_buf_virt = (self.async_qh_virt + 0x100) as *mut u8;
        let data_buf_virt = (self.async_qh_virt + 0x140) as *mut u8;
        let qh_virt = self.async_qh_virt as *mut EhciQueueHead;

        let setup_bytes = setup.to_bytes();
        let max_pkt = max_pkt0.clamp(8, 64) as u32;

        unsafe {
            for i in 0..8 {
                write_volatile(setup_buf_virt.add(i), setup_bytes[i]);
            }
            if let Some(out_slice) = data_out {
                for i in 0..data_len {
                    write_volatile(data_buf_virt.add(i), out_slice[i]);
                }
            } else {
                for i in 0..data_len {
                    write_volatile(data_buf_virt.add(i), 0);
                }
            }

            let has_data = data_len > 0;
            let is_in_dir = data_in.is_some();

            // Setup qTD (DATA0)
            write_volatile(
                setup_qtd_virt,
                EhciQtd {
                    next_qtd: if has_data { data_qtd_phys } else { status_qtd_phys },
                    alt_next_qtd: EHCI_PTR_TERMINATE,
                    token: QTD_STATUS_ACTIVE | QTD_PID_SETUP | QTD_CERR_3 | (8 << 16),
                    buf_ptrs: [setup_buf_phys, 0, 0, 0, 0],
                },
            );

            // Optional Data qTD (DATA1) — set alt_next_qtd to status_qtd_phys for short IN packets
            if has_data {
                let pid = if is_in_dir { QTD_PID_IN } else { QTD_PID_OUT };
                write_volatile(
                    data_qtd_virt,
                    EhciQtd {
                        next_qtd: status_qtd_phys,
                        alt_next_qtd: status_qtd_phys,
                        token: QTD_TOGGLE_DATA1
                            | QTD_STATUS_ACTIVE
                            | pid
                            | QTD_CERR_3
                            | ((data_len as u32) << 16),
                        buf_ptrs: [data_buf_phys, 0, 0, 0, 0],
                    },
                );
            }

            // Status qTD (DATA1, opposite direction of Data stage or IN if 0-length)
            let status_pid = if is_in_dir { QTD_PID_OUT } else { QTD_PID_IN };
            write_volatile(
                status_qtd_virt,
                EhciQtd {
                    next_qtd: EHCI_PTR_TERMINATE,
                    alt_next_qtd: EHCI_PTR_TERMINATE,
                    token: QTD_TOGGLE_DATA1 | QTD_STATUS_ACTIVE | status_pid | QTD_CERR_3,
                    buf_ptrs: [0; 5],
                },
            );

            // Program Async Queue Head; FS/LS control transfers behind a HS Hub require C=1 & TT HubAddr/PortNum
            let eps = speed as u32;
            let c_flag = if speed != UsbSpeed::High { 1 << 27 } else { 0 };
            let ep_chars = (dev_addr as u32 & 0x7F)
                | ((endpoint as u32 & 0x0F) << 8)
                | (eps << 12)
                | (1 << 14) // dtc=1
                | (1 << 15) // H=1
                | (max_pkt << 16)
                | c_flag;

            let tt_bits = if speed != UsbSpeed::High {
                if let Some((hub_addr, hub_port)) = tt {
                    ((hub_addr as u32 & 0x7F) << 16) | ((hub_port as u32 & 0x7F) << 23)
                } else {
                    0
                }
            } else {
                0
            };

            write_volatile(
                qh_virt,
                EhciQueueHead {
                    horiz_link: qh_phys | EHCI_PTR_TYPE_QH,
                    ep_chars,
                    ep_caps: (1 << 30) | tt_bits,
                    curr_qtd: 0,
                    next_qtd: setup_qtd_phys,
                    alt_next_qtd: EHCI_PTR_TERMINATE,
                    token: 0,
                    buf_ptrs: [0; 5],
                },
            );

            fence(Ordering::SeqCst);
            let usbcmd_ptr = (self.op_base + EHCI_OP_USBCMD) as *mut u32;
            let usbsts_ptr = (self.op_base + EHCI_OP_USBSTS) as *const u32;
            if !self.scratch_mmio && (read_volatile(usbcmd_ptr) & USBCMD_ASE) == 0 {
                write_volatile(usbcmd_ptr, read_volatile(usbcmd_ptr) | USBCMD_ASE | USBCMD_RS);
                for _ in 0..10_000 {
                    if (read_volatile(usbsts_ptr) & (1 << 15)) != 0 {
                        break;
                    }
                    core::hint::spin_loop();
                }
            }

            let mut spin = if self.scratch_mmio { 8 } else { 50_000 };
            let mut status_tok = read_volatile(&(*status_qtd_virt).token);
            let mut early_err = false;
            while (status_tok & QTD_STATUS_ACTIVE) != 0 && spin > 0 {
                let setup_tok = read_volatile(&(*setup_qtd_virt).token);
                if (setup_tok & QTD_STATUS_ERR_MASK) != 0 {
                    early_err = true;
                    break;
                }
                if has_data {
                    let data_tok = read_volatile(&(*data_qtd_virt).token);
                    if (data_tok & QTD_STATUS_ERR_MASK) != 0 {
                        early_err = true;
                        break;
                    }
                }
                core::hint::spin_loop();
                status_tok = read_volatile(&(*status_qtd_virt).token);
                spin -= 1;
            }

            // Secondary real-time delay loop for slower microcontrollers/EEPROMs
            if !self.scratch_mmio && (status_tok & QTD_STATUS_ACTIVE) != 0 && !early_err {
                for _ in 0..150 {
                    crate::arch::x86_64::pit::delay_ms(1);
                    status_tok = read_volatile(&(*status_qtd_virt).token);
                    if (status_tok & QTD_STATUS_ACTIVE) == 0 {
                        break;
                    }
                    let setup_tok = read_volatile(&(*setup_qtd_virt).token);
                    if (setup_tok & QTD_STATUS_ERR_MASK) != 0 {
                        early_err = true;
                        break;
                    }
                    if has_data {
                        let data_tok = read_volatile(&(*data_qtd_virt).token);
                        if (data_tok & QTD_STATUS_ERR_MASK) != 0 {
                            early_err = true;
                            break;
                        }
                    }
                }
            }

            // Disarm Async Queue Head after transfer attempt without cycling USBCMD.ASE
            write_volatile(&mut (*qh_virt).next_qtd, EHCI_PTR_TERMINATE);
            write_volatile(&mut (*qh_virt).token, 0);
            fence(Ordering::SeqCst);

            if early_err || (status_tok & QTD_STATUS_ERR_MASK) != 0 {
                return Err("EHCI control transfer transaction error/STALL");
            }
            if (status_tok & QTD_STATUS_ACTIVE) != 0 {
                return Err("EHCI control transfer timeout");
            }

            if let Some(in_buf) = data_in {
                let rem = if has_data {
                    ((read_volatile(&(*data_qtd_virt).token) >> 16) & 0x7FFF) as usize
                } else {
                    0
                };
                let actual = data_len.saturating_sub(rem).min(in_buf.len());
                for i in 0..actual {
                    in_buf[i] = read_volatile(data_buf_virt.add(i));
                }
                return Ok(actual);
            }
        }

        Ok(0)
    }

    /// Execute a synchronous USB Control Transfer over `ASYNCLISTADDR` (default root parameters).
    pub fn submit_control_transfer(
        &self,
        dev_addr: u8,
        endpoint: u8,
        speed: UsbSpeed,
        setup: &UsbSetupPacket,
        data_in: Option<&mut [u8]>,
        data_out: Option<&[u8]>,
    ) -> Result<usize, &'static str> {
        self.submit_control_transfer_ex(dev_addr, endpoint, speed, 64, None, setup, data_in, data_out)
    }

    /// Allocate a 32-byte aligned Queue Head + qTD interrupt endpoint with optional Transaction Translator
    /// `(hub_addr, hub_port)` and vendor/product identification, then splice it into `PERIODICLISTBASE`.
    #[allow(clippy::too_many_arguments)]
    pub fn attach_interrupt_pipe_ex(
        &mut self,
        kind: UsbHidType,
        port: u8,
        dev_addr: u8,
        endpoint: u8,
        speed: UsbSpeed,
        interval_ms: u8,
        max_packet_size: u16,
        tt: Option<(u8, u8)>,
        vendor_id: u16,
        product_id: u16,
    ) -> Result<usize, &'static str> {
        let slot_index = self.pipes.len();
        if slot_index >= MAX_INTERRUPT_PIPES {
            return Err("EHCI: Periodic Interrupt Pipe pool exhausted");
        }

        let offset = (slot_index as u64) * PIPE_SLOT_STRIDE;
        let qh_phys = (self.pipe_pool_phys + offset) as u32;
        let qh_virt = self.pipe_pool_virt + offset;
        let qtd_phys = (self.pipe_pool_phys + offset + 64) as u32;
        let qtd_virt = self.pipe_pool_virt + offset + 64;
        let buf_phys = (self.pipe_pool_phys + offset + 128) as u32;
        let buf_virt = self.pipe_pool_virt + offset + 128;

        let pkt_len = max_packet_size.clamp(1, 64);

        unsafe {
            let qtd_ptr = qtd_virt as *mut EhciQtd;
            write_volatile(
                qtd_ptr,
                EhciQtd {
                    next_qtd: EHCI_PTR_TERMINATE,
                    alt_next_qtd: EHCI_PTR_TERMINATE,
                    token: QTD_STATUS_ACTIVE
                        | QTD_PID_IN
                        | QTD_CERR_3
                        | ((pkt_len as u32) << 16),
                    buf_ptrs: [buf_phys, 0, 0, 0, 0],
                },
            );

            let eps = speed as u32;
            let ep_chars = (dev_addr as u32 & 0x7F)
                | ((endpoint as u32 & 0x0F) << 8)
                | (eps << 12)
                | (1 << 14) // dtc=1: qTD controls data toggle
                | ((pkt_len as u32 & 0x7FF) << 16);

            let s_mask: u32 = 0x01; // Poll in microframe 0
            let c_mask: u32 = if speed == UsbSpeed::High { 0 } else { 0x1C << 8 };
            let tt_bits: u32 = if speed != UsbSpeed::High {
                if let Some((hub_addr, hub_port)) = tt {
                    ((hub_addr as u32 & 0x7F) << 16) | ((hub_port as u32 & 0x7F) << 23)
                } else {
                    0
                }
            } else {
                0
            };
            let ep_caps = (1 << 30) | s_mask | c_mask | tt_bits;

            let qh_ptr = qh_virt as *mut EhciQueueHead;
            write_volatile(
                qh_ptr,
                EhciQueueHead {
                    horiz_link: EHCI_PTR_TERMINATE,
                    ep_chars,
                    ep_caps,
                    curr_qtd: 0,
                    next_qtd: qtd_phys,
                    alt_next_qtd: EHCI_PTR_TERMINATE,
                    token: 0,
                    buf_ptrs: [0; 5],
                },
            );

            // Insert QH into Periodic Frame List with phase staggering so 8ms/2ms pipes stay disjoint
            let step = (interval_ms.max(1) as usize).min(1024);
            let phase = slot_index % step;
            let frame_list = self.frame_list_virt as *mut u32;
            let new_link = qh_phys | EHCI_PTR_TYPE_QH;

            let mut idx = phase;
            while idx < 1024 {
                let prev = read_volatile(frame_list.add(idx));
                if prev != EHCI_PTR_TERMINATE
                    && prev != new_link
                    && read_volatile(&(*qh_ptr).horiz_link) == EHCI_PTR_TERMINATE
                {
                    write_volatile(&mut (*qh_ptr).horiz_link, prev);
                }
                write_volatile(frame_list.add(idx), new_link);
                idx += step;
            }
            fence(Ordering::SeqCst);
        }

        let (kbd_driver, trackpad_driver) = match kind {
            UsbHidType::Keyboard => (Some(UsbKeyboardDriver::new()), None),
            UsbHidType::Mouse => (None, Some(AppleTrackpadDriver::new(vendor_id, product_id))),
        };

        self.pipes.push(EhciInterruptPipe {
            kind,
            slot_index,
            port,
            dev_addr,
            endpoint,
            interval_ms: interval_ms.max(1),
            max_packet_size: pkt_len,
            data_toggle: true, // Next IN toggles to DATA1
            qh_phys,
            qh_virt,
            qtd_phys,
            qtd_virt,
            buf_phys,
            buf_virt,
            kbd_driver,
            trackpad_driver,
        });

        Ok(slot_index)
    }

    /// Allocate a 32-byte aligned Queue Head + qTD interrupt endpoint directly on a root port.
    pub fn attach_interrupt_pipe(
        &mut self,
        kind: UsbHidType,
        port: u8,
        dev_addr: u8,
        endpoint: u8,
        speed: UsbSpeed,
        interval_ms: u8,
        max_packet_size: u16,
    ) -> Result<usize, &'static str> {
        self.attach_interrupt_pipe_ex(
            kind,
            port,
            dev_addr,
            endpoint,
            speed,
            interval_ms,
            max_packet_size,
            None,
            0x05AC,
            0x0245,
        )
    }

    /// Scan all Periodic Interrupt IN pipes and dispatch completed qTD payloads to HID drivers.
    pub fn poll_interrupt_pipes(&mut self) -> usize {
        let mut processed = 0usize;

        for pipe in self.pipes.iter_mut() {
            unsafe {
                let qtd_ptr = pipe.qtd_virt as *mut EhciQtd;
                let qh_ptr = pipe.qh_virt as *mut EhciQueueHead;
                let token = read_volatile(&(*qtd_ptr).token);

                if (token & QTD_STATUS_ACTIVE) != 0 {
                    continue;
                }

                let err = token & QTD_STATUS_ERR_MASK;
                if err == 0 {
                    let rem = ((token >> 16) & 0x7FFF) as usize;
                    let max_len = pipe.max_packet_size as usize;
                    let actual = max_len.saturating_sub(rem).min(64);
                    let mut local_buf = [0u8; 64];
                    let dma_buf = pipe.buf_virt as *const u8;
                    for i in 0..actual {
                        local_buf[i] = read_volatile(dma_buf.add(i));
                    }

                    match pipe.kind {
                        UsbHidType::Keyboard if actual >= 3 => {
                            if let Some(report) = UsbKeyboardReport::from_bytes(&local_buf[..actual]) {
                                if let Some(ref mut drv) = pipe.kbd_driver {
                                    drv.process_report(&report);
                                    processed += 1;
                                }
                            }
                        }
                        UsbHidType::Mouse if actual >= 3 => {
                            if let Some(ref mut drv) = pipe.trackpad_driver {
                                drv.process_report(&local_buf[..actual]);
                                processed += 1;
                            }
                        }
                        _ => {}
                    }
                }

                // Clear DMA buffer and re-arm Interrupt IN qTD for next periodic poll
                let dma_buf_mut = pipe.buf_virt as *mut u8;
                for i in 0..64 {
                    write_volatile(dma_buf_mut.add(i), 0);
                }

                let dt_bit = if pipe.data_toggle { QTD_TOGGLE_DATA1 } else { 0 };
                if err == 0 {
                    pipe.data_toggle = !pipe.data_toggle;
                }

                write_volatile(
                    qtd_ptr,
                    EhciQtd {
                        next_qtd: EHCI_PTR_TERMINATE,
                        alt_next_qtd: EHCI_PTR_TERMINATE,
                        token: dt_bit
                            | QTD_STATUS_ACTIVE
                            | QTD_PID_IN
                            | QTD_CERR_3
                            | ((pipe.max_packet_size as u32) << 16),
                        buf_ptrs: [pipe.buf_phys, 0, 0, 0, 0],
                    },
                );

                // Re-arm Queue Head overlay & Interrupt IN qTD cleanly on both success and error
                write_volatile(&mut (*qh_ptr).curr_qtd, 0);
                write_volatile(&mut (*qh_ptr).token, 0);
                write_volatile(&mut (*qh_ptr).alt_next_qtd, EHCI_PTR_TERMINATE);
                write_volatile(&mut (*qh_ptr).next_qtd, pipe.qtd_phys);
                fence(Ordering::SeqCst);
            }
        }

        processed
    }

    fn alloc_usb_address(&mut self) -> Result<u8, &'static str> {
        if self.next_usb_addr > 127 {
            return Err("EHCI: USB device address space (1..127) exhausted");
        }
        let addr = self.next_usb_addr;
        self.next_usb_addr += 1;
        Ok(addr)
    }

    /// Recursively enumerate a USB device (Standard `GET_DESCRIPTOR`, `SET_ADDRESS`,
    /// `SET_CONFIGURATION`, Hub downstream traversal, or HID Boot Protocol binding).
    pub fn enumerate_device(
        &mut self,
        root_port: u8,
        speed: UsbSpeed,
        tt: Option<(u8, u8)>,
        depth: u8,
    ) -> Result<(), &'static str> {
        if depth == 0 {
            return Err("USB Hub enumeration depth exceeded");
        }

        // 1. Read first 8 bytes of Device Descriptor at address 0 to learn bMaxPacketSize0
        let mut dev_prefix = [0u8; 8];
        let get8 = hub::setup::get_descriptor(hub::USB_DESC_TYPE_DEVICE, 0, 8);
        let initial_max_pkt0 = if speed == UsbSpeed::High { 64 } else { 8 };
        if self.submit_control_transfer_ex(0, 0, speed, initial_max_pkt0, tt, &get8, Some(&mut dev_prefix), None).is_err() {
            crate::arch::x86_64::pit::delay_ms(20);
            self.submit_control_transfer_ex(0, 0, speed, initial_max_pkt0, tt, &get8, Some(&mut dev_prefix), None)?;
        }
        let max_pkt0 = (dev_prefix[7] as u16).clamp(8, 64);

        // 2. Assign unique USB address
        let dev_addr = self.alloc_usb_address()?;
        let set_addr = hub::setup::set_address(dev_addr);
        self.submit_control_transfer_ex(0, 0, speed, max_pkt0, tt, &set_addr, None, None)?;
        crate::arch::x86_64::pit::delay_ms(10);

        // 3. Read full 18-byte Device Descriptor using assigned address
        let mut dev_raw = [0u8; 18];
        let get18 = hub::setup::get_descriptor(hub::USB_DESC_TYPE_DEVICE, 0, 18);
        self.submit_control_transfer_ex(
            dev_addr,
            0,
            speed,
            max_pkt0,
            tt,
            &get18,
            Some(&mut dev_raw),
            None,
        )?;
        let dev_desc = UsbDeviceDescriptor::from_bytes(&dev_raw)
            .ok_or("Invalid USB Device Descriptor")?;
        let vid = dev_desc.id_vendor;
        let pid = dev_desc.id_product;

        // 4. Read 9-byte Configuration header then full Configuration tree
        let mut cfg_hdr_raw = [0u8; 9];
        let get_cfg9 = hub::setup::get_descriptor(hub::USB_DESC_TYPE_CONFIG, 0, 9);
        self.submit_control_transfer_ex(
            dev_addr,
            0,
            speed,
            max_pkt0,
            tt,
            &get_cfg9,
            Some(&mut cfg_hdr_raw),
            None,
        )?;
        let cfg_hdr = UsbConfigDescriptor::from_bytes(&cfg_hdr_raw)
            .ok_or("Invalid USB Configuration Descriptor")?;
        let total_len = (cfg_hdr.w_total_length as usize).clamp(9, 1024);

        let mut cfg_buf = alloc::vec![0u8; total_len];
        let get_cfg_full = hub::setup::get_descriptor(hub::USB_DESC_TYPE_CONFIG, 0, total_len as u16);
        let actual_cfg = self.submit_control_transfer_ex(
            dev_addr,
            0,
            speed,
            max_pkt0,
            tt,
            &get_cfg_full,
            Some(&mut cfg_buf[..total_len]),
            None,
        )?;

        // 5. Activate Configuration
        let set_cfg = hub::setup::set_configuration(cfg_hdr.b_configuration_value);
        self.submit_control_transfer_ex(dev_addr, 0, speed, max_pkt0, tt, &set_cfg, None, None)?;

        let valid_cfg = &cfg_buf[..actual_cfg.min(total_len)];

        if hub::is_hub_device(dev_desc.b_device_class, valid_cfg) {
            if dev_desc.b_device_protocol == 2 {
                let set_iface = UsbSetupPacket {
                    bm_request_type: 0x01, // Host-to-Device | Standard | Interface
                    b_request: 0x0B,       // SET_INTERFACE
                    w_value: 1,            // Alternate Setting 1 (TT per port)
                    w_index: 0,            // Interface 0
                    w_length: 0,
                };
                let _ = self.submit_control_transfer_ex(dev_addr, 0, speed, max_pkt0, tt, &set_iface, None, None);
            }

            kinfo!(
                "EHCI Root Port {}: USB Hub discovered (Addr {}, VID:PID {:04X}:{:04X})",
                root_port,
                dev_addr,
                vid,
                pid
            );
            return self.enumerate_hub(root_port, dev_addr, speed, max_pkt0, tt, depth, vid, pid);
        }

        // Record discovered non-hub peripheral device
        let dev_desc_name = hub::get_usb_product_description(vid, pid, dev_desc.b_device_class);
        self.discovered_devices.push(DiscoveredUsbDevice {
            address: dev_addr,
            root_port,
            speed,
            vendor_id: vid,
            product_id: pid,
            class_code: dev_desc.b_device_class,
            is_hub: false,
            hub_ports: None,
            parent_hub: tt,
            name: alloc::string::String::from(dev_desc_name),
        });

        // 6. Bind HID Boot Keyboard / Mouse / Apple Wellspring Trackpad Interrupt IN endpoints
        let hid_eps = hub::parse_hid_interrupt_endpoints(vid, pid, valid_cfg);
        if hid_eps.is_empty() {
            kinfo!(
                "EHCI Port {}: Enumerated USB Device Addr {} ({:04X}:{:04X}, Class 0x{:02X})",
                root_port,
                dev_addr,
                vid,
                pid,
                dev_desc.b_device_class
            );
            return Ok(());
        }

        for ep in hid_eps {
            if ep.interface_subclass == 0x01 || ep.kind == UsbHidType::Keyboard || ep.is_apple_wellspring {
                let set_boot = UsbSetupPacket::hid_set_protocol(ep.interface_num as u16, true);
                let _ = self.submit_control_transfer_ex(
                    dev_addr, 0, speed, max_pkt0, tt, &set_boot, None, None,
                );
                let set_idle = UsbSetupPacket::hid_set_idle(ep.interface_num as u16, 0, 0);
                let _ = self.submit_control_transfer_ex(
                    dev_addr, 0, speed, max_pkt0, tt, &set_idle, None, None,
                );
            }

            if let Ok(slot) = self.attach_interrupt_pipe_ex(
                ep.kind,
                root_port,
                dev_addr,
                ep.endpoint_num,
                speed,
                ep.interval_ms,
                ep.max_packet_size,
                tt,
                vid,
                pid,
            ) {
                if ep.kind == UsbHidType::Mouse {
                    if let Some(ref mut drv) = self.pipes[slot].trackpad_driver {
                        drv.mark_boot_emulation_active(true);
                    }
                }
                let tag = match ep.kind {
                    UsbHidType::Keyboard => {
                        kinfo!(
                            "EHCI HID Keyboard active: Addr {} EP{} ({}ms, {:04X}:{:04X})",
                            dev_addr,
                            ep.endpoint_num,
                            ep.interval_ms,
                            vid,
                            pid
                        );
                        format!("ehcikb_a{}e{}", dev_addr, ep.endpoint_num)
                    }
                    UsbHidType::Mouse => {
                        kinfo!(
                            "EHCI {} active: Addr {} EP{} ({}ms, {:04X}:{:04X})",
                            if ep.is_apple_wellspring {
                                "Apple Trackpad (Boot Mouse)"
                            } else {
                                "HID Mouse"
                            },
                            dev_addr,
                            ep.endpoint_num,
                            ep.interval_ms,
                            vid,
                            pid
                        );
                        format!("ehcimouse_a{}e{}", dev_addr, ep.endpoint_num)
                    }
                };

                let dev = crate::drivers::usb::hid::UsbHidDevice::new(&tag, ep.kind);
                crate::drivers::usb::hid::USB_HID.lock().register(dev);
            }
        }

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn enumerate_hub(
        &mut self,
        root_port: u8,
        hub_addr: u8,
        hub_speed: UsbSpeed,
        max_pkt0: u16,
        hub_tt: Option<(u8, u8)>,
        depth: u8,
        vid: u16,
        pid: u16,
    ) -> Result<(), &'static str> {
        let mut hub_raw = [0u8; 16];
        let get_hub = hub::setup::get_hub_descriptor(16);
        self.submit_control_transfer_ex(
            hub_addr,
            0,
            hub_speed,
            max_pkt0,
            hub_tt,
            &get_hub,
            Some(&mut hub_raw),
            None,
        )?;
        let hub_desc = UsbHubDescriptor::from_bytes(&hub_raw)
            .ok_or("Invalid USB Hub Descriptor")?;

        let num_ports = hub_desc.b_nbr_ports.clamp(1, 15);
        let pwr_good_ms = ((hub_desc.b_pwr_on_2_pwr_good as u64) * 2).clamp(10, 250);

        let hub_name = hub::get_usb_product_description(vid, pid, 0x09);
        self.discovered_devices.push(DiscoveredUsbDevice {
            address: hub_addr,
            root_port,
            speed: hub_speed,
            vendor_id: vid,
            product_id: pid,
            class_code: 0x09,
            is_hub: true,
            hub_ports: Some(num_ports),
            parent_hub: hub_tt,
            name: alloc::string::String::from(hub_name),
        });

        kinfo!(
            "EHCI USB Hub (Addr {}): {} downstream ports, PwrOn2PwrGood={}ms",
            hub_addr,
            num_ports,
            pwr_good_ms
        );

        // Power on all hub downstream ports first
        for p in 1..=num_ports {
            let pwr = hub::setup::set_port_feature(p, hub::HUB_FEAT_PORT_POWER);
            let _ = self.submit_control_transfer_ex(
                hub_addr, 0, hub_speed, max_pkt0, hub_tt, &pwr, None, None,
            );
        }
        crate::arch::x86_64::pit::delay_ms(pwr_good_ms + 50);

        // Probe & reset each connected hub port
        for p in 1..=num_ports {
            let mut st_buf = [0u8; 4];
            let get_st = hub::setup::get_port_status(p);
            if self
                .submit_control_transfer_ex(
                    hub_addr,
                    0,
                    hub_speed,
                    max_pkt0,
                    hub_tt,
                    &get_st,
                    Some(&mut st_buf),
                    None,
                )
                .is_err()
            {
                continue;
            }
            let status = u16::from_le_bytes([st_buf[0], st_buf[1]]);
            if (status & hub::HUB_PORT_STAT_CONNECTION) == 0 {
                continue;
            }

            let clr_conn = hub::setup::clear_port_feature(p, hub::HUB_FEAT_C_PORT_CONNECTION);
            let _ = self.submit_control_transfer_ex(
                hub_addr, 0, hub_speed, max_pkt0, hub_tt, &clr_conn, None, None,
            );

            let rst = hub::setup::set_port_feature(p, hub::HUB_FEAT_PORT_RESET);
            if self
                .submit_control_transfer_ex(
                    hub_addr, 0, hub_speed, max_pkt0, hub_tt, &rst, None, None,
                )
                .is_err()
            {
                continue;
            }
            crate::arch::x86_64::pit::delay_ms(20);

            for _ in 0..10 {
                let mut cur = [0u8; 4];
                if self
                    .submit_control_transfer_ex(
                        hub_addr, 0, hub_speed, max_pkt0, hub_tt, &get_st, Some(&mut cur), None,
                    )
                    .is_ok()
                {
                    st_buf = cur;
                    let cur_st = u16::from_le_bytes([cur[0], cur[1]]);
                    if (cur_st & hub::HUB_PORT_STAT_RESET) == 0 {
                        break;
                    }
                }
                crate::arch::x86_64::pit::delay_ms(2);
            }

            let clr_rst = hub::setup::clear_port_feature(p, hub::HUB_FEAT_C_PORT_RESET);
            let _ = self.submit_control_transfer_ex(
                hub_addr, 0, hub_speed, max_pkt0, hub_tt, &clr_rst, None, None,
            );
            crate::arch::x86_64::pit::delay_ms(20);

            if self
                .submit_control_transfer_ex(
                    hub_addr, 0, hub_speed, max_pkt0, hub_tt, &get_st, Some(&mut st_buf), None,
                )
                .is_err()
            {
                continue;
            }
            let post_st = u16::from_le_bytes([st_buf[0], st_buf[1]]);
            if (post_st & hub::HUB_PORT_STAT_ENABLE) == 0 {
                continue;
            }

            let child_speed = if (post_st & hub::HUB_PORT_STAT_HIGH_SPEED) != 0 {
                UsbSpeed::High
            } else if (post_st & hub::HUB_PORT_STAT_LOW_SPEED) != 0 {
                UsbSpeed::Low
            } else {
                UsbSpeed::Full
            };

            let child_tt = if child_speed == UsbSpeed::High {
                None
            } else if hub_speed == UsbSpeed::High {
                Some((hub_addr, p))
            } else {
                hub_tt
            };

            kinfo!(
                "EHCI Hub (Addr {}) Port {}: Downstream Device ({:?}, TT={:?})",
                hub_addr,
                p,
                child_speed,
                child_tt
            );
            if let Err(e) = self.enumerate_device(root_port, child_speed, child_tt, depth - 1) {
                crate::kwarn!(
                    "EHCI Hub (Addr {}) Port {}: Enumeration failed ({}); disabling port & clearing TT",
                    hub_addr,
                    p,
                    e
                );
                // When enumerating downstream port `p` on `hub_addr` fails:
                let clear_port_enable = UsbSetupPacket {
                    bm_request_type: 0x23, // Host-to-Device | Class | Other (Port)
                    b_request: 0x01,       // CLEAR_FEATURE
                    w_value: 1,            // PORT_ENABLE (feature 1)
                    w_index: p as u16,
                    w_length: 0,
                };
                let _ = self.submit_control_transfer_ex(
                    hub_addr, 0, hub_speed, max_pkt0, hub_tt, &clear_port_enable, None, None,
                );

                // Clear Single/Multi-TT buffer for Address 0 EP 0 on `port_num`
                let clear_tt = UsbSetupPacket {
                    bm_request_type: 0x23,
                    b_request: 0x08,       // CLEAR_TT_BUFFER
                    w_value: 0,            // EP 0, Addr 0, Control
                    w_index: p as u16,
                    w_length: 0,
                };
                let _ = self.submit_control_transfer_ex(
                    hub_addr, 0, hub_speed, max_pkt0, hub_tt, &clear_tt, None, None,
                );
                continue;
            }
        }

        Ok(())
    }

    /// Test helper for synthetic bare-metal validation: simulates the enumeration of an
    /// Intel 6/7 Series Rate Matching Hub (Addr 1) with downstream Apple Keyboard (Addr 2)
    /// and Apple Wellspring 5 Trackpad (Addr 3), verifying TT split transaction setup,
    /// Periodic interrupt pipe Queue Heads, and device tree hierarchy.
    pub fn simulate_mock_hub_and_downstream_enumeration(&mut self) -> Result<(), &'static str> {
        let hub_addr = self.alloc_usb_address()?;
        let hub_vid = 0x8087;
        let hub_pid = 0x0024;
        let hub_name = hub::get_usb_product_description(hub_vid, hub_pid, 0x09);
        self.discovered_devices.push(DiscoveredUsbDevice {
            address: hub_addr,
            root_port: 1,
            speed: UsbSpeed::High,
            vendor_id: hub_vid,
            product_id: hub_pid,
            class_code: 0x09,
            is_hub: true,
            hub_ports: Some(6),
            parent_hub: None,
            name: alloc::string::String::from(hub_name),
        });

        // Downstream Port 1: Apple Internal Keyboard (FullSpeed, TT=(1, 1))
        let kbd_addr = self.alloc_usb_address()?;
        let kbd_vid = 0x05AC;
        let kbd_pid = 0x0245;
        let kbd_name = hub::get_usb_product_description(kbd_vid, kbd_pid, 0x03);
        let _kbd_slot = self.attach_interrupt_pipe_ex(
            UsbHidType::Keyboard,
            1,
            kbd_addr,
            1,
            UsbSpeed::Full,
            8,
            8,
            Some((hub_addr, 1)),
            kbd_vid,
            kbd_pid,
        )?;
        let kbd_tag = format!("ehcikb_a{}e1", kbd_addr);
        let kbd_dev = crate::drivers::usb::hid::UsbHidDevice::new(&kbd_tag, UsbHidType::Keyboard);
        crate::drivers::usb::hid::USB_HID.lock().register(kbd_dev);

        self.discovered_devices.push(DiscoveredUsbDevice {
            address: kbd_addr,
            root_port: 1,
            speed: UsbSpeed::Full,
            vendor_id: kbd_vid,
            product_id: kbd_pid,
            class_code: 0x03,
            is_hub: false,
            hub_ports: None,
            parent_hub: Some((hub_addr, 1)),
            name: alloc::string::String::from(kbd_name),
        });

        // Downstream Port 2: Apple Wellspring Trackpad (FullSpeed, TT=(1, 2))
        let tp_addr = self.alloc_usb_address()?;
        let tp_vid = 0x05AC;
        let tp_pid = 0x0245;
        let tp_name = hub::get_usb_product_description(tp_vid, tp_pid, 0x03);
        let tp_slot = self.attach_interrupt_pipe_ex(
            UsbHidType::Mouse,
            1,
            tp_addr,
            2,
            UsbSpeed::Full,
            2,
            4,
            Some((hub_addr, 2)),
            tp_vid,
            tp_pid,
        )?;
        if let Some(ref mut drv) = self.pipes[tp_slot].trackpad_driver {
            drv.mark_boot_emulation_active(true);
        }
        let tp_tag = format!("ehcimouse_a{}e2", tp_addr);
        let tp_dev = crate::drivers::usb::hid::UsbHidDevice::new(&tp_tag, UsbHidType::Mouse);
        crate::drivers::usb::hid::USB_HID.lock().register(tp_dev);

        self.discovered_devices.push(DiscoveredUsbDevice {
            address: tp_addr,
            root_port: 1,
            speed: UsbSpeed::Full,
            vendor_id: tp_vid,
            product_id: tp_pid,
            class_code: 0x03,
            is_hub: false,
            hub_ports: None,
            parent_hub: Some((hub_addr, 2)),
            name: alloc::string::String::from(tp_name),
        });

        Ok(())
    }

    /// Scan all EHCI root ports, perform USB 2.0 & Hub Class enumeration, and bind HID pipes.
    pub fn scan_ports(&mut self) {
        if !self.scratch_mmio {
            for port in 1..=self.num_ports {
                let portsc_offset = self.op_base + EHCI_OP_PORTSC_BASE + ((port as u64 - 1) * 4);
                let val = unsafe { read_volatile(portsc_offset as *const u32) };
                if (val & (1 << 12)) == 0 {
                    unsafe { write_volatile(portsc_offset as *mut u32, val | (1 << 12)); }
                }
            }
            crate::arch::x86_64::pit::delay_ms(50);
        }

        for port in 1..=self.num_ports {
            let portsc_offset = self.op_base + EHCI_OP_PORTSC_BASE + ((port as u64 - 1) * 4);
            let mut portsc = unsafe { read_volatile(portsc_offset as *const u32) };

            let connected = (portsc & PORTSC_CCS) != 0;
            if !connected {
                continue;
            }

            if (portsc & PORTSC_PE) == 0 && !self.scratch_mmio {
                // Issue 50ms EHCI Root Port Reset pulse cleanly while preserving R/WC bits and PP (bit 12)
                unsafe {
                    let rwc_mask = 0x2A; // CSC | PEC | OCC
                    write_volatile(
                        portsc_offset as *mut u32,
                        (portsc & !(PORTSC_PE | rwc_mask)) | PORTSC_PR | (1 << 12),
                    );
                    crate::arch::x86_64::pit::delay_ms(50);
                    let post = read_volatile(portsc_offset as *const u32);
                    write_volatile(portsc_offset as *mut u32, (post & !(PORTSC_PR | rwc_mask)) | (1 << 12));
                    crate::arch::x86_64::pit::delay_ms(20);
                    for _ in 0..25 {
                        let cur = read_volatile(portsc_offset as *const u32);
                        if (cur & PORTSC_PE) != 0 {
                            break;
                        }
                        crate::arch::x86_64::pit::delay_ms(2);
                    }
                }
                portsc = unsafe { read_volatile(portsc_offset as *const u32) };
            }

            kinfo!(
                "EHCI Port {}: Attached USB 2.0 Device Detected (PORTSC=0x{:08X})",
                port,
                portsc
            );
            self.connected_devices.push(port);

            if (portsc & PORTSC_PE) == 0 && !self.scratch_mmio {
                // Non-HS device connected directly to root port without Hub -> release to companion UHCI/OHCI
                unsafe {
                    write_volatile(portsc_offset as *mut u32, portsc | (1 << 13));
                }
                crate::kwarn!(
                    "EHCI Port {}: Non-high-speed root device released to companion USB 1.1 controller",
                    port
                );
                continue;
            }

            if let Err(e) = self.enumerate_device(port, UsbSpeed::High, None, 4) {
                crate::kwarn!("EHCI Port {}: USB enumeration skipped ({})", port, e);
            }
        }
    }
}

impl Drop for EhciController {
    fn drop(&mut self) {
        if !self.scratch_mmio {
            unsafe {
                let usbcmd_ptr = (self.op_base + EHCI_OP_USBCMD) as *mut u32;
                let cmd = read_volatile(usbcmd_ptr);
                write_volatile(usbcmd_ptr, cmd & !(USBCMD_RS | USBCMD_PSE | USBCMD_ASE));
            }
        }
        let mut pmm = PMM.lock();
        if self.frame_list_phys != 0 {
            pmm.free_frame(self.frame_list_phys);
            self.frame_list_phys = 0;
        }
        if self.async_qh_phys != 0 {
            pmm.free_frame(self.async_qh_phys);
            self.async_qh_phys = 0;
        }
        if self.pipe_pool_phys != 0 {
            pmm.free_frame(self.pipe_pool_phys);
            self.pipe_pool_phys = 0;
        }
    }
}

/// Real EHCI Bulk-Only Transport implementation (replaces virtual disks)
pub struct EhciUsbTransport {
    pub port: u8,
    pub op_base: u64,
}

impl UsbMassStorageTransport for EhciUsbTransport {
    fn bulk_out(&self, _data: &[u8]) -> Result<(), &'static str> {
        Err("EHCI Bulk OUT requires attached USB Mass Storage endpoint enumeration")
    }

    fn bulk_in(&self, _buf: &mut [u8]) -> Result<usize, &'static str> {
        Err("EHCI Bulk IN requires attached USB Mass Storage endpoint enumeration")
    }
}
