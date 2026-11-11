extern crate alloc;
use alloc::vec::Vec;
use crate::mm::address_space::{AddressSpace, VmaType};
use crate::mm::pmm::PMM;
use crate::mm::vmm::{VMM, PAGE_PRESENT, PAGE_USER, PAGE_WRITABLE};
use crate::mm::{align_down, align_up, phys_to_virt, PAGE_SIZE};

// Standard ELF constants
pub const ELF_MAGIC: [u8; 4] = [0x7F, b'E', b'L', b'F'];
pub const ELF_CLASS_64: u8 = 2;
pub const ELF_DATA_2LSB: u8 = 1; // Little Endian
pub const ELF_TYPE_EXEC: u16 = 2;
pub const ELF_TYPE_DYN: u16 = 3;
pub const ELF_MACHINE_X86_64: u16 = 0x3E;

pub const PT_NULL: u32 = 0;
pub const PT_LOAD: u32 = 1;
pub const PT_DYNAMIC: u32 = 2;
pub const PT_INTERP: u32 = 3;
pub const PT_NOTE: u32 = 4;
pub const PT_SHLIB: u32 = 5;
pub const PT_PHDR: u32 = 6;
pub const PT_TLS: u32 = 7;

pub const PF_X: u32 = 1; // Executable
pub const PF_W: u32 = 2; // Writable
pub const PF_R: u32 = 4; // Readable

// Dynamic Section Tags (ELF-64)
pub const DT_NULL: i64 = 0;
pub const DT_NEEDED: i64 = 1;
pub const DT_PLTRELSZ: i64 = 2;
pub const DT_PLTGOT: i64 = 3;
pub const DT_STRTAB: i64 = 5;
pub const DT_SYMTAB: i64 = 6;
pub const DT_RELA: i64 = 7;
pub const DT_RELASZ: i64 = 8;
pub const DT_RELAENT: i64 = 9;
pub const DT_STRSZ: i64 = 10;
pub const DT_SYMENT: i64 = 11;
pub const DT_JMPREL: i64 = 23;

// Relocation Types (x86_64 ABI)
pub const R_X86_64_NONE: u32 = 0;
pub const R_X86_64_64: u32 = 1;
pub const R_X86_64_COPY: u32 = 5;
pub const R_X86_64_GLOB_DAT: u32 = 6;
pub const R_X86_64_JUMP_SLOT: u32 = 7;
pub const R_X86_64_RELATIVE: u32 = 8;

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Elf64Dyn {
    pub d_tag: i64,
    pub d_val: u64,
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Elf64Rela {
    pub r_offset: u64,
    pub r_info: u64,
    pub r_addend: i64,
}

impl Elf64Rela {
    pub fn r_sym(&self) -> u32 {
        (self.r_info >> 32) as u32
    }
    pub fn r_type(&self) -> u32 {
        (self.r_info & 0xFFFF_FFFF) as u32
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Elf64Sym {
    pub st_name: u32,
    pub st_info: u8,
    pub st_other: u8,
    pub st_shndx: u16,
    pub st_value: u64,
    pub st_size: u64,
}

pub const USER_STACK_TOP: u64 = 0x0000_7FFF_FFFF_0000;
pub const USER_STACK_PAGES: u64 = 16; // 64 KiB user stack

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Elf64Header {
    pub ident: [u8; 16],
    pub elf_type: u16,
    pub machine: u16,
    pub version: u32,
    pub entry: u64,
    pub phoff: u64,
    pub shoff: u64,
    pub flags: u32,
    pub ehsize: u16,
    pub phentsize: u16,
    pub phnum: u16,
    pub shentsize: u16,
    pub shnum: u16,
    pub shstrndx: u16,
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Elf64ProgramHeader {
    pub p_type: u32,
    pub p_flags: u32,
    pub p_offset: u64,
    pub p_vaddr: u64,
    pub p_paddr: u64,
    pub p_filesz: u64,
    pub p_memsz: u64,
    pub p_align: u64,
}

#[derive(Debug, Clone)]
pub struct ElfBinary<'a> {
    pub header: Elf64Header,
    pub program_headers: Vec<Elf64ProgramHeader>,
    pub raw_data: &'a [u8],
}

impl<'a> ElfBinary<'a> {
    /// Parse and validate an ELF64 binary image.
    pub fn parse(data: &'a [u8]) -> Result<Self, &'static str> {
        if data.len() < core::mem::size_of::<Elf64Header>() {
            return Err("ELF image too small for 64-bit ELF header");
        }

        // Validate Magic
        if &data[0..4] != ELF_MAGIC {
            return Err("Invalid ELF magic (expected 0x7F 'ELF')");
        }

        // Validate 64-bit
        if data[4] != ELF_CLASS_64 {
            return Err("Unsupported ELF class: only 64-bit (ELF64) supported");
        }

        // Validate Little Endian
        if data[5] != ELF_DATA_2LSB {
            return Err("Unsupported ELF endianness: only little-endian supported");
        }

        // Parse ELF Header
        let header = unsafe { core::ptr::read_unaligned(data.as_ptr() as *const Elf64Header) };

        // Validate Machine (EM_X86_64 = 0x3E)
        if header.machine != ELF_MACHINE_X86_64 {
            return Err("Unsupported ELF architecture: target must be x86_64 (EM_X86_64)");
        }

        // Parse Program Headers
        let mut program_headers = Vec::with_capacity(header.phnum as usize);
        let ph_size = header.phentsize as usize;
        let ph_offset = header.phoff as usize;

        if ph_offset + (header.phnum as usize * ph_size) > data.len() {
            return Err("Program header table extends beyond file boundaries");
        }

        for i in 0..header.phnum as usize {
            let offset = ph_offset + i * ph_size;
            let ph = unsafe {
                core::ptr::read_unaligned(data[offset..].as_ptr() as *const Elf64ProgramHeader)
            };
            program_headers.push(ph);
        }

        Ok(Self {
            header,
            program_headers,
            raw_data: data,
        })
    }

    pub fn entry_point(&self) -> u64 {
        self.header.entry
    }

    /// Load the ELF PT_LOAD segments and user stack into the given AddressSpace.
    /// Returns (entry_point, user_stack_top).
    pub fn load_into_address_space(&self, addr_space: &mut AddressSpace) -> Result<(u64, u64), &'static str> {
        let vmm = VMM.lock();
        let pml4 = unsafe { &mut *(phys_to_virt(addr_space.pml4_phys) as *mut crate::mm::vmm::PageTable) };

        for ph in &self.program_headers {
            if ph.p_type != PT_LOAD || ph.p_memsz == 0 {
                continue;
            }

            let start_vaddr = ph.p_vaddr;
            let end_vaddr = start_vaddr + ph.p_memsz;
            let page_start = align_down(start_vaddr, PAGE_SIZE as u64);
            let page_end = align_up(end_vaddr, PAGE_SIZE as u64);
            let pages = (page_end - page_start) / (PAGE_SIZE as u64);

            let mut page_flags = PAGE_PRESENT | PAGE_USER;
            if (ph.p_flags & PF_W) != 0 {
                page_flags |= PAGE_WRITABLE;
            }

            for p in 0..pages {
                let curr_page = page_start + p * (PAGE_SIZE as u64);

                // Allocate physical frame
                let frame = PMM.lock().alloc_frame().ok_or("Out of physical memory for ELF segment")?;
                let virt_frame = phys_to_virt(frame) as *mut u8;

                // Zero out the frame
                unsafe {
                    core::ptr::write_bytes(virt_frame, 0, PAGE_SIZE);
                }

                // Check if file data overlaps with this page
                let file_data_start = ph.p_vaddr;
                let file_data_end = ph.p_vaddr + ph.p_filesz;

                let page_data_start = curr_page.max(file_data_start);
                let page_data_end = (curr_page + PAGE_SIZE as u64).min(file_data_end);

                if page_data_end > page_data_start {
                    let copy_len = (page_data_end - page_data_start) as usize;
                    let frame_offset = (page_data_start - curr_page) as usize;
                    let file_offset = (ph.p_offset + (page_data_start - file_data_start)) as usize;

                    if file_offset + copy_len <= self.raw_data.len() {
                        unsafe {
                            core::ptr::copy_nonoverlapping(
                                self.raw_data[file_offset..].as_ptr(),
                                virt_frame.add(frame_offset),
                                copy_len,
                            );
                        }
                    }
                }

                // Map page into user PML4
                vmm.map_page(pml4, curr_page, frame, page_flags)?;
            }

            let vma_type = if (ph.p_flags & PF_X) != 0 {
                VmaType::Code
            } else {
                VmaType::Data
            };

            addr_space.vmas.push(crate::mm::address_space::VirtualMemoryArea {
                start: page_start,
                end: page_end,
                flags: page_flags,
                vma_type,
            });
        }

        // Allocate and map User Stack (16 pages = 64 KiB)
        let stack_bottom = USER_STACK_TOP - (USER_STACK_PAGES * PAGE_SIZE as u64);
        for p in 0..USER_STACK_PAGES {
            let curr_page = stack_bottom + p * (PAGE_SIZE as u64);
            let frame = PMM.lock().alloc_frame().ok_or("Out of physical memory for user stack")?;
            let virt_frame = phys_to_virt(frame) as *mut u8;

            unsafe {
                core::ptr::write_bytes(virt_frame, 0, PAGE_SIZE);
            }

            vmm.map_page(pml4, curr_page, frame, PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER)?;
        }

        addr_space.vmas.push(crate::mm::address_space::VirtualMemoryArea {
            start: stack_bottom,
            end: USER_STACK_TOP,
            flags: PAGE_PRESENT | PAGE_WRITABLE | PAGE_USER,
            vma_type: VmaType::Stack,
        });

        // Align stack top to 16 bytes minus 8 (standard AMD64 ABI stack alignment for initial RIP entry)
        let user_rsp = (USER_STACK_TOP - 16) & !0xF;

        Ok((self.header.entry, user_rsp))
    }
}
