//! Minimal pure-Rust UEFI 2.x ABI definitions and protocol helpers for x86_64.

#![allow(dead_code)]

use core::ffi::c_void;
use core::ptr::null_mut;

pub type Handle = *mut c_void;
pub type Status = usize;

pub const EFI_SUCCESS: Status = 0;
pub const EFI_BUFFER_TOO_SMALL: Status = 0x8000_0000_0000_0005;

pub const ALLOCATE_ANY_PAGES: u32 = 0;
pub const ALLOCATE_MAX_ADDRESS: u32 = 1;
pub const ALLOCATE_ADDRESS: u32 = 2;

pub const EFI_LOADER_CODE: u32 = 1;
pub const EFI_LOADER_DATA: u32 = 2;
pub const EFI_BOOT_SERVICES_CODE: u32 = 3;
pub const EFI_BOOT_SERVICES_DATA: u32 = 4;
pub const EFI_CONVENTIONAL_MEMORY: u32 = 7;
pub const EFI_UNUSABLE_MEMORY: u32 = 8;
pub const EFI_ACPI_RECLAIM_MEMORY: u32 = 9;
pub const EFI_ACPI_MEMORY_NVS: u32 = 10;

pub const EFI_FILE_MODE_READ: u64 = 1;

#[repr(C)]
#[derive(Copy, Clone, Eq, PartialEq)]
pub struct Guid {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

pub const LOADED_IMAGE_PROTOCOL_GUID: Guid = Guid {
    data1: 0x5B1B_31A1,
    data2: 0x9562,
    data3: 0x11D2,
    data4: [0x8E, 0x3F, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B],
};

pub const SIMPLE_FILE_SYSTEM_PROTOCOL_GUID: Guid = Guid {
    data1: 0x964E_5B22,
    data2: 0x6459,
    data3: 0x11D2,
    data4: [0x8E, 0x39, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B],
};

pub const FILE_INFO_ID_GUID: Guid = Guid {
    data1: 0x0957_6E92,
    data2: 0x6D3F,
    data3: 0x11D2,
    data4: [0x8E, 0x39, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B],
};

pub const GRAPHICS_OUTPUT_PROTOCOL_GUID: Guid = Guid {
    data1: 0x9042_A9DE,
    data2: 0x23DC,
    data3: 0x4A38,
    data4: [0x96, 0xFB, 0x7A, 0xDE, 0xD0, 0x80, 0x51, 0x6A],
};

pub const ACPI_20_TABLE_GUID: Guid = Guid {
    data1: 0x8868_E871,
    data2: 0xE4F1,
    data3: 0x11D3,
    data4: [0xBC, 0x22, 0x00, 0x80, 0xC7, 0x3C, 0x88, 0x81],
};

pub const ACPI_10_TABLE_GUID: Guid = Guid {
    data1: 0xEB9D_2D30,
    data2: 0x2D88,
    data3: 0x11D3,
    data4: [0x9A, 0x16, 0x00, 0x90, 0x27, 0x3F, 0xC1, 0x4D],
};

#[repr(C)]
pub struct TableHeader {
    pub signature: u64,
    pub revision: u32,
    pub header_size: u32,
    pub crc32: u32,
    pub reserved: u32,
}

#[repr(C)]
pub struct SimpleTextOutputProtocol {
    pub reset: extern "efiapi" fn(*mut Self, bool) -> Status,
    pub output_string: extern "efiapi" fn(*mut Self, *const u16) -> Status,
}

#[repr(C)]
pub struct ConfigurationTable {
    pub vendor_guid: Guid,
    pub vendor_table: *const c_void,
}

#[repr(C)]
pub struct SystemTable {
    pub hdr: TableHeader,
    pub firmware_vendor: *const u16,
    pub firmware_revision: u32,
    pub console_in_handle: Handle,
    pub con_in: *mut c_void,
    pub console_out_handle: Handle,
    pub con_out: *mut SimpleTextOutputProtocol,
    pub std_err_handle: Handle,
    pub std_err: *mut SimpleTextOutputProtocol,
    pub runtime_services: *mut c_void,
    pub boot_services: *mut BootServices,
    pub number_of_table_entries: usize,
    pub configuration_table: *const ConfigurationTable,
}

#[repr(C)]
pub struct BootServices {
    pub hdr: TableHeader,
    pub raise_tpl: *const c_void,
    pub restore_tpl: *const c_void,
    pub allocate_pages:
        extern "efiapi" fn(alloc_type: u32, mem_type: u32, pages: usize, memory: *mut u64) -> Status,
    pub free_pages: extern "efiapi" fn(memory: u64, pages: usize) -> Status,
    pub get_memory_map: extern "efiapi" fn(
        mmap_size: *mut usize,
        mmap: *mut u8,
        map_key: *mut usize,
        desc_size: *mut usize,
        desc_ver: *mut u32,
    ) -> Status,
    pub allocate_pool: extern "efiapi" fn(u32, usize, *mut *mut u8) -> Status,
    pub free_pool: extern "efiapi" fn(*mut u8) -> Status,
    pub create_event: *const c_void,
    pub set_timer: *const c_void,
    pub wait_for_event: *const c_void,
    pub signal_event: *const c_void,
    pub close_event: *const c_void,
    pub check_event: *const c_void,
    pub install_protocol_interface: *const c_void,
    pub reinstall_protocol_interface: *const c_void,
    pub uninstall_protocol_interface: *const c_void,
    pub handle_protocol:
        extern "efiapi" fn(handle: Handle, protocol: *const Guid, interface: *mut *mut c_void) -> Status,
    pub reserved: *const c_void,
    pub register_protocol_notify: *const c_void,
    pub locate_handle: *const c_void,
    pub locate_device_path: *const c_void,
    pub install_configuration_table: *const c_void,
    pub load_image: *const c_void,
    pub start_image: *const c_void,
    pub exit: *const c_void,
    pub unload_image: *const c_void,
    pub exit_boot_services: extern "efiapi" fn(image_handle: Handle, map_key: usize) -> Status,
    pub get_next_monotonic_count: *const c_void,
    pub stall: *const c_void,
    pub set_watchdog_timer:
        extern "efiapi" fn(timeout: usize, code: u64, data_size: usize, data: *const u16) -> Status,
    pub connect_controller: *const c_void,
    pub disconnect_controller: *const c_void,
    pub open_protocol: *const c_void,
    pub close_protocol: *const c_void,
    pub open_protocol_information: *const c_void,
    pub protocols_per_handle: *const c_void,
    pub locate_handle_buffer: *const c_void,
    pub locate_protocol: extern "efiapi" fn(
        protocol: *const Guid,
        registration: *const c_void,
        interface: *mut *mut c_void,
    ) -> Status,
}

#[repr(C)]
pub struct LoadedImageProtocol {
    pub revision: u32,
    pub parent_handle: Handle,
    pub system_table: *mut SystemTable,
    pub device_handle: Handle,
}

#[repr(C)]
pub struct SimpleFileSystemProtocol {
    pub revision: u64,
    pub open_volume: extern "efiapi" fn(*mut Self, *mut *mut FileProtocol) -> Status,
}

#[repr(C)]
pub struct FileProtocol {
    pub revision: u64,
    pub open: extern "efiapi" fn(*mut Self, *mut *mut FileProtocol, *const u16, u64, u64) -> Status,
    pub close: extern "efiapi" fn(*mut Self) -> Status,
    pub delete: *const c_void,
    pub read: extern "efiapi" fn(*mut Self, *mut usize, *mut u8) -> Status,
    pub write: *const c_void,
    pub get_position: *const c_void,
    pub set_position: *const c_void,
    pub get_info: extern "efiapi" fn(*mut Self, *const Guid, *mut usize, *mut u8) -> Status,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct MemoryDescriptor {
    pub typ: u32,
    pub pad: u32,
    pub physical_start: u64,
    pub virtual_start: u64,
    pub number_of_pages: u64,
    pub attribute: u64,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct PixelBitmask {
    pub red_mask: u32,
    pub green_mask: u32,
    pub blue_mask: u32,
    pub reserved_mask: u32,
}

#[repr(C)]
pub struct GopModeInfo {
    pub version: u32,
    pub horizontal_resolution: u32,
    pub vertical_resolution: u32,
    pub pixel_format: u32,
    pub pixel_information: PixelBitmask,
    pub pixels_per_scan_line: u32,
}

#[repr(C)]
pub struct GopMode {
    pub max_mode: u32,
    pub mode: u32,
    pub info: *mut GopModeInfo,
    pub size_of_info: usize,
    pub frame_buffer_base: u64,
    pub frame_buffer_size: usize,
}

#[repr(C)]
pub struct GraphicsOutputProtocol {
    pub query_mode: extern "efiapi" fn(*mut Self, u32, *mut usize, *mut *mut GopModeInfo) -> Status,
    pub set_mode: extern "efiapi" fn(*mut Self, u32) -> Status,
    pub blt: *const c_void,
    pub mode: *mut GopMode,
}

/// Write an ASCII string to UEFI SimpleTextOutput (expanding `\n` to `\r\n`).
pub unsafe fn print_ascii(con_out: *mut SimpleTextOutputProtocol, s: &str) {
    if con_out.is_null() {
        return;
    }
    let mut buf = [0u16; 128];
    let mut idx = 0usize;
    for b in s.bytes() {
        if idx + 3 >= buf.len() {
            buf[idx] = 0;
            ((*con_out).output_string)(con_out, buf.as_ptr());
            idx = 0;
        }
        if b == b'\n' {
            buf[idx] = b'\r' as u16;
            idx += 1;
        }
        buf[idx] = b as u16;
        idx += 1;
    }
    if idx > 0 {
        buf[idx] = 0;
        ((*con_out).output_string)(con_out, buf.as_ptr());
    }
}

/// Convert a hexadecimal `u64` into console output for minimal diagnostics.
pub unsafe fn print_hex(con_out: *mut SimpleTextOutputProtocol, val: u64) {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut buf = [0u16; 19];
    buf[0] = b'0' as u16;
    buf[1] = b'x' as u16;
    for i in 0..16 {
        let nibble = ((val >> ((15 - i) * 4)) & 0xF) as usize;
        buf[2 + i] = HEX[nibble] as u16;
    }
    buf[18] = 0;
    if !con_out.is_null() {
        ((*con_out).output_string)(con_out, buf.as_ptr());
    }
}

/// Look up a UEFI configuration table pointer by GUID.
pub unsafe fn find_config_table(st: *const SystemTable, target: &Guid) -> *const c_void {
    let count = (*st).number_of_table_entries;
    let tables = (*st).configuration_table;
    if tables.is_null() {
        return null_mut();
    }
    for i in 0..count {
        let entry = &*tables.add(i);
        if &entry.vendor_guid == target {
            return entry.vendor_table;
        }
    }
    null_mut()
}
