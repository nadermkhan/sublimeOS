//! SublimeOS Pure-Rust UEFI -> Multiboot2 Bootloader (`BOOTX64.EFI`).
//!
//! Loads `\boot\sublimekernel32.elf` (or `\sublimekernel32.elf`) via UEFI SimpleFileSystem,
//! gathers UEFI GOP framebuffer, ACPI 2.0/1.0 RSDP, and coalesced E820 memory map tags,
//! exits UEFI Boot Services cleanly, transitions from 64-bit Long Mode down to 32-bit
//! non-paged Protected Mode inside a low `< 1 MiB` trampoline page, copies ELF `PT_LOAD`
//! segments (including `.bss` zeroing), and jumps to `_start` with `EAX = 0x36D76289`
//! and `EBX = mb2_info_phys`.

#![no_std]
#![no_main]

mod efi;

use core::arch::global_asm;
use core::ffi::c_void;
use core::panic::PanicInfo;
use core::ptr::{copy_nonoverlapping, null_mut, write_bytes};

const MULTIBOOT2_MAGIC: u32 = 0x36D7_6289;
const PT_LOAD: u32 = 1;
const PAGE_SIZE: usize = 4096;
const MAX_SEGMENTS: usize = 16;
const MAX_E820_ENTRIES: usize = 128;
const TRAMP_MAX_CODE_BYTES: usize = 0x800;

global_asm!(
    r#"
.text
.global tramp_code_start
.global tramp_code_end
.code64
tramp_code_start:
    cli
    cld
    mov rbx, rcx
    lea rsp, [rbx + 0xF80]
    lgdt [rbx + 0x800]

    mov rax, cr4
    btr eax, 17
    mov cr4, rax

    lea rax, [rip + tramp_compat32]
    lea r8, [rip + tramp_code_start]
    sub rax, r8
    add rax, rbx
    push 0x08
    push rax
    .byte 0x48, 0xCB

.code32
tramp_compat32:
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax
    lea esp, [ebx + 0xF80]

    mov eax, cr0
    and eax, 0x7FFFFFFF
    mov cr0, eax

    mov ecx, 0xC0000080
    rdmsr
    and eax, 0xFFFFFEFF
    wrmsr

    xor eax, eax
    mov cr3, eax
    mov eax, cr4
    and eax, 0xFFFFFFDF
    mov cr4, eax

    mov edx, [ebx + 0x83C]
    lea ebp, [ebx + 0x840]
    test edx, edx
    jz 3f
1:
    mov esi, [ebp + 0]
    mov edi, [ebp + 4]
    mov ecx, [ebp + 8]
    test ecx, ecx
    jz 2f
    rep movsb
2:
    mov ecx, [ebp + 12]
    sub ecx, [ebp + 8]
    jbe 4f
    xor eax, eax
    rep stosb
4:
    add ebp, 16
    dec edx
    jnz 1b
3:
    mov eax, [ebx + 0x830]
    mov edx, [ebx + 0x838]
    mov ebx, [ebx + 0x834]
    jmp edx
tramp_code_end:
.code64
"#
);

extern "C" {
    static tramp_code_start: u8;
    static tramp_code_end: u8;
}

#[repr(C, packed)]
#[derive(Copy, Clone)]
struct GdtPtr {
    limit: u16,
    base: u64,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct SegmentCopy {
    src_phys: u32,
    dst_phys: u32,
    filesz: u32,
    memsz: u32,
}

#[repr(C)]
struct TrampolineParams {
    gdt_ptr: GdtPtr,
    _pad0: [u8; 6],
    gdt: [u64; 4],
    magic: u32,
    mb2_phys: u32,
    entry_phys: u32,
    seg_count: u32,
    segments: [SegmentCopy; MAX_SEGMENTS],
}

#[derive(Copy, Clone)]
struct GopInfo {
    addr: u64,
    pitch: u32,
    width: u32,
    height: u32,
    bpp: u8,
    red_pos: u8,
    red_mask: u8,
    green_pos: u8,
    green_mask: u8,
    blue_pos: u8,
    blue_mask: u8,
}

struct ParsedKernel {
    entry_phys: u32,
    kernel_min: u64,
    kernel_max: u64,
    seg_count: usize,
    seg_offsets: [u64; MAX_SEGMENTS],
    segments: [SegmentCopy; MAX_SEGMENTS],
}

#[inline]
fn align_up(v: usize, a: usize) -> usize {
    (v + a - 1) & !(a - 1)
}

#[inline]
fn align_up_u64(v: u64, a: u64) -> u64 {
    (v + a - 1) & !(a - 1)
}

#[inline]
fn ranges_overlap(a0: u64, a1: u64, b0: u64, b1: u64) -> bool {
    a0 < b1 && b0 < a1
}

unsafe fn alloc_pages_avoiding(
    bs: *mut efi::BootServices,
    pages: usize,
    max_addr: u64,
    mem_type: u32,
    avoid_a: (u64, u64),
    avoid_b: (u64, u64),
) -> Option<u64> {
    let byte_len = (pages as u64) * (PAGE_SIZE as u64);
    let mut rejected: [(u64, usize); 16] = [(0, 0); 16];
    let mut rejected_cnt = 0usize;
    let mut result: Option<u64> = None;

    for _ in 0..16 {
        let mut addr = max_addr;
        let status = ((*bs).allocate_pages)(efi::ALLOCATE_MAX_ADDRESS, mem_type, pages, &mut addr);
        if status != efi::EFI_SUCCESS {
            break;
        }
        let end = addr.saturating_add(byte_len);
        if !ranges_overlap(addr, end, avoid_a.0, avoid_a.1)
            && !ranges_overlap(addr, end, avoid_b.0, avoid_b.1)
        {
            result = Some(addr);
            break;
        }
        if rejected_cnt < rejected.len() {
            rejected[rejected_cnt] = (addr, pages);
            rejected_cnt += 1;
        } else {
            ((*bs).free_pages)(addr, pages);
            break;
        }
    }

    for i in 0..rejected_cnt {
        ((*bs).free_pages)(rejected[i].0, rejected[i].1);
    }
    result
}

unsafe fn alloc_low_trampoline(bs: *mut efi::BootServices) -> Option<u64> {
    // Prefer low conventional RAM below 640 KiB, avoiding NULL page and AP trampoline 0x8000.
    if let Some(addr) = alloc_pages_avoiding(
        bs,
        1,
        0x9E000,
        efi::EFI_LOADER_CODE,
        (0x0000, 0x1000),
        (0x8000, 0x9000),
    ) {
        return Some(addr);
    }
    // Probe downward explicitly if allocator returned an undesirable block.
    let mut candidate: u64 = 0x8E000;
    while candidate >= 0x10000 {
        if candidate != 0x8000 {
            let mut addr = candidate;
            let st = ((*bs).allocate_pages)(efi::ALLOCATE_ADDRESS, efi::EFI_LOADER_CODE, 1, &mut addr);
            if st == efi::EFI_SUCCESS {
                return Some(addr);
            }
        }
        candidate -= 0x1000;
    }
    None
}

unsafe fn load_kernel_elf(
    bs: *mut efi::BootServices,
    image_handle: efi::Handle,
) -> Option<(u64, usize)> {
    let mut li_ptr: *mut c_void = null_mut();
    if ((*bs).handle_protocol)(image_handle, &efi::LOADED_IMAGE_PROTOCOL_GUID, &mut li_ptr)
        != efi::EFI_SUCCESS
        || li_ptr.is_null()
    {
        return None;
    }
    let li = &*(li_ptr as *const efi::LoadedImageProtocol);

    let mut sfs_ptr: *mut c_void = null_mut();
    if ((*bs).handle_protocol)(
        li.device_handle,
        &efi::SIMPLE_FILE_SYSTEM_PROTOCOL_GUID,
        &mut sfs_ptr,
    ) != efi::EFI_SUCCESS
        || sfs_ptr.is_null()
    {
        return None;
    }
    let sfs = &mut *(sfs_ptr as *mut efi::SimpleFileSystemProtocol);

    let mut root: *mut efi::FileProtocol = null_mut();
    if (sfs.open_volume)(sfs, &mut root) != efi::EFI_SUCCESS || root.is_null() {
        return None;
    }

    // Try `\boot\sublimekernel32.elf`, `\sublimekernel32.elf`, `\boot\cosmoskernel32.elf`, `\cosmoskernel32.elf`.
    let path_sublime_boot: [u16; 26] = [
        '\\' as u16, 'b' as u16, 'o' as u16, 'o' as u16, 't' as u16, '\\' as u16,
        's' as u16, 'u' as u16, 'b' as u16, 'l' as u16, 'i' as u16, 'm' as u16, 'e' as u16,
        'k' as u16, 'e' as u16, 'r' as u16, 'n' as u16, 'e' as u16, 'l' as u16,
        '3' as u16, '2' as u16, '.' as u16, 'e' as u16, 'l' as u16, 'f' as u16, 0,
    ];
    let path_sublime_root: [u16; 21] = [
        '\\' as u16,
        's' as u16, 'u' as u16, 'b' as u16, 'l' as u16, 'i' as u16, 'm' as u16, 'e' as u16,
        'k' as u16, 'e' as u16, 'r' as u16, 'n' as u16, 'e' as u16, 'l' as u16,
        '3' as u16, '2' as u16, '.' as u16, 'e' as u16, 'l' as u16, 'f' as u16, 0,
    ];
    let path_cosmos_boot: [u16; 25] = [
        '\\' as u16, 'b' as u16, 'o' as u16, 'o' as u16, 't' as u16, '\\' as u16,
        'c' as u16, 'o' as u16, 's' as u16, 'm' as u16, 'o' as u16, 's' as u16,
        'k' as u16, 'e' as u16, 'r' as u16, 'n' as u16, 'e' as u16, 'l' as u16,
        '3' as u16, '2' as u16, '.' as u16, 'e' as u16, 'l' as u16, 'f' as u16, 0,
    ];
    let path_cosmos_root: [u16; 20] = [
        '\\' as u16,
        'c' as u16, 'o' as u16, 's' as u16, 'm' as u16, 'o' as u16, 's' as u16,
        'k' as u16, 'e' as u16, 'r' as u16, 'n' as u16, 'e' as u16, 'l' as u16,
        '3' as u16, '2' as u16, '.' as u16, 'e' as u16, 'l' as u16, 'f' as u16, 0,
    ];

    let mut file: *mut efi::FileProtocol = null_mut();
    let paths: [&[u16]; 4] = [
        &path_sublime_boot,
        &path_sublime_root,
        &path_cosmos_boot,
        &path_cosmos_root,
    ];

    let mut opened = false;
    for p in paths.iter() {
        if ((*root).open)(root, &mut file, p.as_ptr(), efi::EFI_FILE_MODE_READ, 0) == efi::EFI_SUCCESS
            && !file.is_null()
        {
            opened = true;
            break;
        }
    }

    if !opened {
        ((*root).close)(root);
        return None;
    }

    let mut info_buf = [0u8; 256];
    let mut info_size = info_buf.len();
    if ((*file).get_info)(file, &efi::FILE_INFO_ID_GUID, &mut info_size, info_buf.as_mut_ptr())
        != efi::EFI_SUCCESS
    {
        ((*file).close)(file);
        ((*root).close)(root);
        return None;
    }
    let file_size = core::ptr::read_unaligned(info_buf.as_ptr().add(8) as *const u64) as usize;
    if file_size < 64 {
        ((*file).close)(file);
        ((*root).close)(root);
        return None;
    }

    let pages = align_up(file_size, PAGE_SIZE) / PAGE_SIZE;
    let mut buf_phys: u64 = 0x3FFF_0000;
    if ((*bs).allocate_pages)(
        efi::ALLOCATE_MAX_ADDRESS,
        efi::EFI_LOADER_DATA,
        pages,
        &mut buf_phys,
    ) != efi::EFI_SUCCESS
    {
        ((*file).close)(file);
        ((*root).close)(root);
        return None;
    }

    let mut remaining = file_size;
    let mut dst = buf_phys as *mut u8;
    while remaining > 0 {
        let mut chunk = remaining;
        if ((*file).read)(file, &mut chunk, dst) != efi::EFI_SUCCESS || chunk == 0 {
            ((*bs).free_pages)(buf_phys, pages);
            ((*file).close)(file);
            ((*root).close)(root);
            return None;
        }
        dst = dst.add(chunk);
        remaining -= chunk;
    }

    ((*file).close)(file);
    ((*root).close)(root);
    Some((buf_phys, file_size))
}

unsafe fn parse_elf(elf_ptr: *const u8, elf_len: usize) -> Option<ParsedKernel> {
    if elf_len < 64 {
        return None;
    }
    let hdr = core::slice::from_raw_parts(elf_ptr, elf_len);
    if &hdr[0..4] != b"\x7FELF" {
        return None;
    }
    let ei_class = hdr[4];
    let (entry, phoff, phentsize, phnum) = match ei_class {
        1 => {
            let entry = u32::from_le_bytes(hdr[24..28].try_into().ok()?) as u64;
            let phoff = u32::from_le_bytes(hdr[28..32].try_into().ok()?) as usize;
            let phentsize = u16::from_le_bytes(hdr[42..44].try_into().ok()?) as usize;
            let phnum = u16::from_le_bytes(hdr[44..46].try_into().ok()?) as usize;
            (entry, phoff, phentsize, phnum)
        }
        2 => {
            let entry = u64::from_le_bytes(hdr[24..32].try_into().ok()?);
            let phoff = u64::from_le_bytes(hdr[32..40].try_into().ok()?) as usize;
            let phentsize = u16::from_le_bytes(hdr[54..56].try_into().ok()?) as usize;
            let phnum = u16::from_le_bytes(hdr[56..58].try_into().ok()?) as usize;
            (entry, phoff, phentsize, phnum)
        }
        _ => return None,
    };

    if entry == 0 || entry > u32::MAX as u64 || phnum == 0 {
        return None;
    }

    let mut out = ParsedKernel {
        entry_phys: entry as u32,
        kernel_min: u64::MAX,
        kernel_max: 0,
        seg_count: 0,
        seg_offsets: [0; MAX_SEGMENTS],
        segments: [SegmentCopy {
            src_phys: 0,
            dst_phys: 0,
            filesz: 0,
            memsz: 0,
        }; MAX_SEGMENTS],
    };

    for i in 0..phnum {
        let off = phoff.checked_add(i * phentsize)?;
        if off.checked_add(phentsize)? > elf_len {
            return None;
        }
        let ph = &hdr[off..off + phentsize];
        let p_type = u32::from_le_bytes(ph[0..4].try_into().ok()?);
        if p_type != PT_LOAD {
            continue;
        }
        let (p_offset, p_paddr, p_filesz, p_memsz) = if ei_class == 1 {
            (
                u32::from_le_bytes(ph[4..8].try_into().ok()?) as u64,
                u32::from_le_bytes(ph[12..16].try_into().ok()?) as u64,
                u32::from_le_bytes(ph[16..20].try_into().ok()?) as u64,
                u32::from_le_bytes(ph[20..24].try_into().ok()?) as u64,
            )
        } else {
            (
                u64::from_le_bytes(ph[8..16].try_into().ok()?),
                u64::from_le_bytes(ph[24..32].try_into().ok()?),
                u64::from_le_bytes(ph[32..40].try_into().ok()?),
                u64::from_le_bytes(ph[40..48].try_into().ok()?),
            )
        };

        if p_memsz == 0 {
            continue;
        }
        if out.seg_count >= MAX_SEGMENTS {
            return None;
        }
        if p_offset.checked_add(p_filesz)? > elf_len as u64 {
            return None;
        }
        let p_end = p_paddr.checked_add(p_memsz)?;
        if p_end > u32::MAX as u64 || p_filesz > p_memsz {
            return None;
        }

        if p_paddr < out.kernel_min {
            out.kernel_min = p_paddr;
        }
        if p_end > out.kernel_max {
            out.kernel_max = p_end;
        }

        let idx = out.seg_count;
        out.seg_offsets[idx] = p_offset;
        out.segments[idx] = SegmentCopy {
            src_phys: 0,
            dst_phys: p_paddr as u32,
            filesz: p_filesz as u32,
            memsz: p_memsz as u32,
        };
        out.seg_count += 1;
    }

    if out.seg_count == 0 {
        return None;
    }
    out.kernel_min &= !((PAGE_SIZE as u64) - 1);
    out.kernel_max = align_up_u64(out.kernel_max, PAGE_SIZE as u64);
    Some(out)
}

#[inline]
fn mask_to_pos_size(mask: u32) -> (u8, u8) {
    if mask == 0 {
        (0, 0)
    } else {
        (mask.trailing_zeros() as u8, mask.count_ones() as u8)
    }
}

fn info_to_gop(info: &efi::GopModeInfo, fb_base: u64) -> Option<GopInfo> {
    if fb_base == 0 || info.horizontal_resolution == 0 || info.vertical_resolution == 0 {
        return None;
    }
    let (r_pos, r_sz, g_pos, g_sz, b_pos, b_sz) = match info.pixel_format {
        0 => (0, 8, 8, 8, 16, 8),   // PixelRedGreenBlueReserved8BitPerColor
        1 => (16, 8, 8, 8, 0, 8),   // PixelBlueGreenRedReserved8BitPerColor
        2 => {
            let (rp, rs) = mask_to_pos_size(info.pixel_information.red_mask);
            let (gp, gs) = mask_to_pos_size(info.pixel_information.green_mask);
            let (bp, bs) = mask_to_pos_size(info.pixel_information.blue_mask);
            (rp, rs, gp, gs, bp, bs)
        }
        _ => return None, // PixelBltOnly
    };
    Some(GopInfo {
        addr: fb_base,
        pitch: info.pixels_per_scan_line.saturating_mul(4),
        width: info.horizontal_resolution,
        height: info.vertical_resolution,
        bpp: 32,
        red_pos: r_pos,
        red_mask: r_sz,
        green_pos: g_pos,
        green_mask: g_sz,
        blue_pos: b_pos,
        blue_mask: b_sz,
    })
}

unsafe fn init_gop(bs: *mut efi::BootServices) -> Option<GopInfo> {
    let mut gop_ptr: *mut c_void = null_mut();
    if ((*bs).locate_protocol)(&efi::GRAPHICS_OUTPUT_PROTOCOL_GUID, null_mut(), &mut gop_ptr)
        != efi::EFI_SUCCESS
        || gop_ptr.is_null()
    {
        return None;
    }
    let gop = &mut *(gop_ptr as *mut efi::GraphicsOutputProtocol);
    if gop.mode.is_null() {
        return None;
    }

    // Prefer 1024x768 32bpp mode if supported, otherwise fall back to current or first non-Blt mode.
    let current_mode = (*gop.mode).mode;
    let max_mode = (*gop.mode).max_mode;
    let mut best_1024: Option<u32> = None;
    let mut any_linear: Option<u32> = None;

    for m in 0..max_mode {
        let mut size = 0usize;
        let mut info_ptr: *mut efi::GopModeInfo = null_mut();
        if (gop.query_mode)(gop, m, &mut size, &mut info_ptr) != efi::EFI_SUCCESS
            || info_ptr.is_null()
        {
            continue;
        }
        let info = &*info_ptr;
        if info.pixel_format <= 2 {
            if any_linear.is_none() {
                any_linear = Some(m);
            }
            if info.horizontal_resolution == 1024 && info.vertical_resolution == 768 {
                best_1024 = Some(m);
                break;
            }
        }
    }

    if let Some(target) = best_1024 {
        if target != current_mode {
            let _ = (gop.set_mode)(gop, target);
        }
    } else if !(*gop.mode).info.is_null() && (*(*gop.mode).info).pixel_format > 2 {
        if let Some(target) = any_linear {
            let _ = (gop.set_mode)(gop, target);
        }
    }

    let mode = &*gop.mode;
    if mode.info.is_null() {
        return None;
    }
    info_to_gop(&*mode.info, mode.frame_buffer_base)
}

unsafe fn find_acpi_rsdp(st: *const efi::SystemTable) -> Option<(u32, [u8; 36], usize)> {
    let mut ptr = efi::find_config_table(st, &efi::ACPI_20_TABLE_GUID) as *const u8;
    let mut is_v2 = true;
    if ptr.is_null() {
        ptr = efi::find_config_table(st, &efi::ACPI_10_TABLE_GUID) as *const u8;
        is_v2 = false;
    }
    if ptr.is_null() {
        return None;
    }
    let sig = core::slice::from_raw_parts(ptr, 8);
    if sig != b"RSD PTR " {
        return None;
    }
    let revision = *ptr.add(15);
    let mut out = [0u8; 36];
    if is_v2 && revision >= 2 {
        let len = core::ptr::read_unaligned(ptr.add(20) as *const u32) as usize;
        let copy_len = if len >= 36 { 36 } else { 20 };
        copy_nonoverlapping(ptr, out.as_mut_ptr(), copy_len);
        Some((if copy_len == 36 { 15 } else { 14 }, out, copy_len))
    } else {
        copy_nonoverlapping(ptr, out.as_mut_ptr(), 20);
        Some((14, out, 20))
    }
}

#[inline]
fn efi_mem_to_e820(typ: u32) -> u32 {
    match typ {
        efi::EFI_LOADER_CODE
        | efi::EFI_LOADER_DATA
        | efi::EFI_BOOT_SERVICES_CODE
        | efi::EFI_BOOT_SERVICES_DATA
        | efi::EFI_CONVENTIONAL_MEMORY => 1,
        efi::EFI_ACPI_RECLAIM_MEMORY => 3,
        efi::EFI_ACPI_MEMORY_NVS => 4,
        efi::EFI_UNUSABLE_MEMORY => 5,
        _ => 2,
    }
}

unsafe fn sort_memory_map(buf: *mut u8, count: usize, desc_size: usize) {
    let mut tmp = [0u8; 128];
    if desc_size > tmp.len() || count < 2 {
        return;
    }
    for i in 1..count {
        let cur_ptr = buf.add(i * desc_size);
        let cur_phys = (*(cur_ptr as *const efi::MemoryDescriptor)).physical_start;
        copy_nonoverlapping(cur_ptr, tmp.as_mut_ptr(), desc_size);

        let mut j = i;
        while j > 0 {
            let prev_ptr = buf.add((j - 1) * desc_size);
            let prev_phys = (*(prev_ptr as *const efi::MemoryDescriptor)).physical_start;
            if prev_phys <= cur_phys {
                break;
            }
            copy_nonoverlapping(prev_ptr, buf.add(j * desc_size), desc_size);
            j -= 1;
        }
        copy_nonoverlapping(tmp.as_ptr(), buf.add(j * desc_size), desc_size);
    }
}

unsafe fn write_u32(base: *mut u8, offset: usize, val: u32) {
    core::ptr::write_unaligned(base.add(offset) as *mut u32, val);
}

unsafe fn write_u64(base: *mut u8, offset: usize, val: u64) {
    core::ptr::write_unaligned(base.add(offset) as *mut u64, val);
}

unsafe fn build_multiboot2_info(
    mb2_phys: u64,
    mmap_buf: *mut u8,
    mmap_size: usize,
    desc_size: usize,
    gop: Option<GopInfo>,
    acpi: Option<(u32, [u8; 36], usize)>,
) {
    let base = mb2_phys as *mut u8;
    write_bytes(base, 0, PAGE_SIZE * 4);

    let count = mmap_size / desc_size;
    sort_memory_map(mmap_buf, count, desc_size);

    // Offset 0..8: total_size (patched at end) + reserved = 0.
    let mut off = 8usize;

    // Emit Tag 4 (Basic MemInfo) placeholder first; patch mem_lower / mem_upper after E820 walk.
    let tag4_off = off;
    write_u32(base, off, 4);
    write_u32(base, off + 4, 16);
    off += 16;

    // Emit Tag 6 (Memory Map), coalescing adjacent descriptors of the same E820 type.
    let tag6_off = off;
    write_u32(base, off, 6);
    write_u32(base, off + 8, 24);
    write_u32(base, off + 12, 0);
    off += 16;

    let mut e820_count = 0usize;
    let mut prev_base: u64 = 0;
    let mut prev_len: u64 = 0;
    let mut prev_typ: u32 = 0;
    let mut has_prev = false;

    let mut mem_lower_kb: u32 = 640;
    let mut mem_upper_kb: u32 = 0;

    for i in 0..count {
        let d = &*(mmap_buf.add(i * desc_size) as *const efi::MemoryDescriptor);
        if d.number_of_pages == 0 {
            continue;
        }
        let len = d.number_of_pages.saturating_mul(PAGE_SIZE as u64);
        let typ = efi_mem_to_e820(d.typ);

        if has_prev && prev_typ == typ && prev_base.saturating_add(prev_len) == d.physical_start {
            prev_len = prev_len.saturating_add(len);
        } else {
            if has_prev && e820_count < MAX_E820_ENTRIES {
                if !(e820_count >= MAX_E820_ENTRIES - 8 && prev_base >= 0x1_0000_0000) {
                    write_u64(base, off, prev_base);
                    write_u64(base, off + 8, prev_len);
                    write_u32(base, off + 16, prev_typ);
                    write_u32(base, off + 20, 0);
                    off += 24;
                    e820_count += 1;

                    if prev_typ == 1 {
                        if prev_base == 0 {
                            mem_lower_kb = (prev_len / 1024).min(640) as u32;
                        }
                        let end = prev_base.saturating_add(prev_len);
                        if prev_base <= 0x10_0000 && end > 0x10_0000 {
                            mem_upper_kb = ((end - 0x10_0000) / 1024).min(u32::MAX as u64) as u32;
                        }
                    }
                }
            }
            prev_base = d.physical_start;
            prev_len = len;
            prev_typ = typ;
            has_prev = true;
        }
    }

    if has_prev && e820_count < MAX_E820_ENTRIES {
        write_u64(base, off, prev_base);
        write_u64(base, off + 8, prev_len);
        write_u32(base, off + 16, prev_typ);
        write_u32(base, off + 20, 0);
        off += 24;
        e820_count += 1;

        if prev_typ == 1 {
            if prev_base == 0 {
                mem_lower_kb = (prev_len / 1024).min(640) as u32;
            }
            let end = prev_base.saturating_add(prev_len);
            if prev_base <= 0x10_0000 && end > 0x10_0000 {
                mem_upper_kb = ((end - 0x10_0000) / 1024).min(u32::MAX as u64) as u32;
            }
        }
    }

    write_u32(base, tag6_off + 4, (16 + e820_count * 24) as u32);
    write_u32(base, tag4_off + 8, mem_lower_kb);
    write_u32(base, tag4_off + 12, mem_upper_kb);
    off = align_up(off, 8);

    // Emit Tag 8 (Framebuffer Info) when GOP linear framebuffer is active.
    if let Some(fb) = gop {
        write_u32(base, off, 8);
        write_u32(base, off + 4, 38);
        write_u64(base, off + 8, fb.addr);
        write_u32(base, off + 16, fb.pitch);
        write_u32(base, off + 20, fb.width);
        write_u32(base, off + 24, fb.height);
        *base.add(off + 28) = fb.bpp;
        *base.add(off + 29) = 1; // RGB direct color
        *base.add(off + 30) = 0;
        *base.add(off + 31) = 0;
        *base.add(off + 32) = fb.red_pos;
        *base.add(off + 33) = fb.red_mask;
        *base.add(off + 34) = fb.green_pos;
        *base.add(off + 35) = fb.green_mask;
        *base.add(off + 36) = fb.blue_pos;
        *base.add(off + 37) = fb.blue_mask;
        off = align_up(off + 38, 8);
    }

    // Emit Tag 14/15 (ACPI RSDP inline copy).
    if let Some((tag_type, rsdp_bytes, copy_len)) = acpi {
        write_u32(base, off, tag_type);
        write_u32(base, off + 4, (8 + copy_len) as u32);
        copy_nonoverlapping(rsdp_bytes.as_ptr(), base.add(off + 8), copy_len);
        off = align_up(off + 8 + copy_len, 8);
    }

    // Emit Tag 0 (End Tag).
    write_u32(base, off, 0);
    write_u32(base, off + 4, 8);
    off += 8;

    write_u32(base, 0, off as u32);
    write_u32(base, 4, 0);
}

#[no_mangle]
pub extern "efiapi" fn efi_main(image_handle: efi::Handle, st: *mut efi::SystemTable) -> usize {
    unsafe {
        if st.is_null() {
            return 1;
        }
        let bs = (*st).boot_services;
        let con = (*st).con_out;
        if bs.is_null() {
            return 1;
        }

        let _ = ((*bs).set_watchdog_timer)(0, 0, 0, core::ptr::null());
        efi::print_ascii(con, "SublimeOS UEFI Multiboot2 Loader v1.0 (KiteKernel)\n");
        efi::print_ascii(con, "  Copyright (c) 2026 Nader Mahbub Khan. All rights reserved.\n");

        // 1. Allocate < 1 MiB trampoline page BEFORE staging allocations.
        let tramp_phys = match alloc_low_trampoline(bs) {
            Some(p) => p,
            None => {
                efi::print_ascii(con, "FATAL: Unable to allocate <1 MiB trampoline page\n");
                return 1;
            }
        };

        // 2. Load kernel ELF32/64 file into memory and parse PT_LOAD segments.
        let (file_phys, file_size) = match load_kernel_elf(bs, image_handle) {
            Some(v) => v,
            None => {
                efi::print_ascii(con, "FATAL: sublimekernel32.elf not found on ESP\n");
                return 1;
            }
        };

        let mut kernel = match parse_elf(file_phys as *const u8, file_size) {
            Some(k) => k,
            None => {
                efi::print_ascii(con, "FATAL: Invalid ELF image\n");
                return 1;
            }
        };

        let kernel_avoid = (kernel.kernel_min, kernel.kernel_max);
        let tramp_avoid = (tramp_phys, tramp_phys + PAGE_SIZE as u64);

        // 3. Stage each PT_LOAD segment outside target kernel range and trampoline page.
        for i in 0..kernel.seg_count {
            let fsz = kernel.segments[i].filesz as usize;
            if fsz == 0 {
                continue;
            }
            let seg_pages = align_up(fsz, PAGE_SIZE) / PAGE_SIZE;
            let stage_phys = match alloc_pages_avoiding(
                bs,
                seg_pages,
                0x3FFF_0000,
                efi::EFI_LOADER_DATA,
                kernel_avoid,
                tramp_avoid,
            ) {
                Some(p) => p,
                None => {
                    efi::print_ascii(con, "FATAL: Staging OOM\n");
                    return 1;
                }
            };
            copy_nonoverlapping(
                (file_phys + kernel.seg_offsets[i]) as *const u8,
                stage_phys as *mut u8,
                fsz,
            );
            kernel.segments[i].src_phys = stage_phys as u32;
        }
        ((*bs).free_pages)(file_phys, align_up(file_size, PAGE_SIZE) / PAGE_SIZE);

        // 4. Query GOP Linear Framebuffer & ACPI RSDP before ExitBootServices.
        let gop = init_gop(bs);
        let acpi = find_acpi_rsdp(st);

        // 5. Allocate Multiboot2 info buffer (< 1 GiB) and MemoryMap buffer BEFORE ExitBootServices.
        let mb2_phys = match alloc_pages_avoiding(
            bs,
            4,
            0x3FFF_0000,
            efi::EFI_LOADER_DATA,
            kernel_avoid,
            tramp_avoid,
        ) {
            Some(p) => p,
            None => {
                efi::print_ascii(con, "FATAL: MB2 allocation failed\n");
                return 1;
            }
        };

        let mmap_pages = 16usize;
        let mmap_cap = mmap_pages * PAGE_SIZE;
        let mmap_phys = match alloc_pages_avoiding(
            bs,
            mmap_pages,
            0x3FFF_0000,
            efi::EFI_LOADER_DATA,
            kernel_avoid,
            tramp_avoid,
        ) {
            Some(p) => p,
            None => {
                efi::print_ascii(con, "FATAL: MMap allocation failed\n");
                return 1;
            }
        };

        efi::print_ascii(con, "  Kernel Entry: ");
        efi::print_hex(con, kernel.entry_phys as u64);
        efi::print_ascii(con, "  Trampoline: ");
        efi::print_hex(con, tramp_phys);
        efi::print_ascii(con, "\nExiting UEFI BootServices -> Jumping to SublimeOS (KiteKernel)...\n");

        // 6. Retry GetMemoryMap + ExitBootServices with zero prints or allocations inside.
        let mut mmap_size = mmap_cap;
        let mut map_key = 0usize;
        let mut desc_size = 0usize;
        let mut desc_ver = 0u32;

        let mut exited = false;
        for _ in 0..8 {
            mmap_size = mmap_cap;
            let st_map = ((*bs).get_memory_map)(
                &mut mmap_size,
                mmap_phys as *mut u8,
                &mut map_key,
                &mut desc_size,
                &mut desc_ver,
            );
            if st_map != efi::EFI_SUCCESS {
                break;
            }
            if ((*bs).exit_boot_services)(image_handle, map_key) == efi::EFI_SUCCESS {
                exited = true;
                break;
            }
        }

        if !exited {
            efi::print_ascii(con, "FATAL: ExitBootServices failed\n");
            return 1;
        }

        // 7. Build Multiboot2 info buffer & populate trampoline page, then enter 32-bit kernel.
        build_multiboot2_info(
            mb2_phys,
            mmap_phys as *mut u8,
            mmap_size,
            desc_size,
            gop,
            acpi,
        );

        let tramp_size = (&tramp_code_end as *const u8 as usize)
            - (&tramp_code_start as *const u8 as usize);
        if tramp_size > TRAMP_MAX_CODE_BYTES {
            loop {
                core::arch::asm!("cli; hlt");
            }
        }
        write_bytes(tramp_phys as *mut u8, 0, PAGE_SIZE);
        copy_nonoverlapping(&tramp_code_start as *const u8, tramp_phys as *mut u8, tramp_size);

        let params = &mut *((tramp_phys + 0x800) as *mut TrampolineParams);
        params.gdt_ptr = GdtPtr {
            limit: 31,
            base: tramp_phys + 0x810,
        };
        params.gdt = [
            0x0000_0000_0000_0000,
            0x00CF_9A00_0000_FFFF, // 0x08: 32-bit flat CS
            0x00CF_9200_0000_FFFF, // 0x10: 32-bit flat DS
            0x00AF_9A00_0000_FFFF, // 0x18: 64-bit flat CS
        ];
        params.magic = MULTIBOOT2_MAGIC;
        params.mb2_phys = mb2_phys as u32;
        params.entry_phys = kernel.entry_phys;
        params.seg_count = kernel.seg_count as u32;
        params.segments = kernel.segments;

        let jump: extern "efiapi" fn(u64) -> ! = core::mem::transmute(tramp_phys as usize);
        jump(tramp_phys);
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {
        unsafe {
            core::arch::asm!("hlt");
        }
    }
}
