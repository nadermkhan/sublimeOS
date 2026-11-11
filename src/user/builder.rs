extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use crate::task::elf::{
    Elf64Dyn, Elf64Rela, Elf64Sym,
    DT_NULL, DT_NEEDED, DT_PLTRELSZ, DT_PLTGOT, DT_STRTAB, DT_SYMTAB,
    DT_RELAENT, DT_STRSZ, DT_SYMENT, DT_JMPREL,
    R_X86_64_JUMP_SLOT,
};

pub const ELF_MAGIC: [u8; 4] = [0x7F, b'E', b'L', b'F'];
pub const BASE_VADDR: u64 = 0x00400000;
pub const ENTRY_POINT: u64 = 0x00400078;
pub const RODATA_VADDR: u64 = 0x00400400;

pub struct ElfBuilder {
    code: Vec<u8>,
    rodata: Vec<u8>,
    is_dylib: bool,
    is_dynamic: bool,
    interp: Option<String>,
    needed_libs: Vec<String>,
    imports: Vec<String>,
    exports: Vec<(String, u64)>,
}

impl ElfBuilder {
    pub fn new() -> Self {
        Self {
            code: Vec::new(),
            rodata: Vec::new(),
            is_dylib: false,
            is_dynamic: false,
            interp: None,
            needed_libs: Vec::new(),
            imports: Vec::new(),
            exports: Vec::new(),
        }
    }

    /// Mark this binary as a shared dynamic library (`.so`, ET_DYN).
    pub fn set_shared_lib(&mut self) {
        self.is_dylib = true;
    }

    /// Mark this binary as a dynamically linked executable (ET_EXEC with PT_DYNAMIC and PT_INTERP).
    pub fn set_dynamic_exe(&mut self, interp_path: &str) {
        self.is_dynamic = true;
        self.interp = Some(String::from(interp_path));
    }

    /// Add a shared library dependency (DT_NEEDED).
    pub fn add_needed(&mut self, lib_name: &str) {
        self.needed_libs.push(String::from(lib_name));
    }

    /// Register an imported dynamic function and return its 64-bit GOT virtual address.
    pub fn add_import(&mut self, sym_name: &str) -> u64 {
        let got_vaddr = 0x00401000 + (self.imports.len() as u64 * 8);
        self.imports.push(String::from(sym_name));
        got_vaddr
    }

    /// Export a function from a shared library with its function virtual address.
    pub fn add_export(&mut self, sym_name: &str, func_vaddr: u64) {
        self.exports.push((String::from(sym_name), func_vaddr));
    }

    /// Register a read-only string in the RODATA section and return its 64-bit virtual address.
    pub fn add_string(&mut self, s: &[u8]) -> u64 {
        let vaddr = RODATA_VADDR + self.rodata.len() as u64;
        self.rodata.extend_from_slice(s);
        vaddr
    }

    /// Return current code size.
    pub fn code_len(&self) -> usize {
        self.code.len()
    }

    /// Emit arbitrary raw machine code bytes.
    pub fn emit_bytes(&mut self, bytes: &[u8]) {
        self.code.extend_from_slice(bytes);
    }

    /// Emit `mov eax, imm32` (5 bytes: B8 imm32) - set syscall number
    pub fn emit_mov_eax(&mut self, imm: u32) {
        self.code.push(0xB8);
        self.code.extend_from_slice(&imm.to_le_bytes());
    }

    /// Emit `mov edi, imm32` (5 bytes: BF imm32) - set arg 1
    pub fn emit_mov_edi(&mut self, imm: u32) {
        self.code.push(0xBF);
        self.code.extend_from_slice(&imm.to_le_bytes());
    }

    /// Emit `movabs rdi, imm64` (10 bytes: 48 BF imm64) - set arg 1 (64-bit pointer)
    pub fn emit_mov_rdi(&mut self, imm: u64) {
        self.code.extend_from_slice(&[0x48, 0xBF]);
        self.code.extend_from_slice(&imm.to_le_bytes());
    }

    /// Emit `movabs rsi, imm64` (10 bytes: 48 BE imm64) - set arg 2 (pointer/buffer)
    pub fn emit_mov_rsi(&mut self, imm: u64) {
        self.code.extend_from_slice(&[0x48, 0xBE]);
        self.code.extend_from_slice(&imm.to_le_bytes());
    }

    /// Emit `movabs rdx, imm64` (10 bytes: 48 BA imm64) - set arg 3 (length/count)
    pub fn emit_mov_rdx(&mut self, imm: u64) {
        self.code.extend_from_slice(&[0x48, 0xBA]);
        self.code.extend_from_slice(&imm.to_le_bytes());
    }

    /// Emit `movabs r10, imm64` (10 bytes: 49 BA imm64) - set arg 4
    pub fn emit_mov_r10(&mut self, imm: u64) {
        self.code.extend_from_slice(&[0x49, 0xBA]);
        self.code.extend_from_slice(&imm.to_le_bytes());
    }

    /// Emit `syscall` instruction (2 bytes: 0F 05)
    pub fn emit_syscall(&mut self) {
        self.code.extend_from_slice(&[0x0F, 0x05]);
    }

    /// Emit `int 0x80` instruction (2 bytes: CD 80)
    pub fn emit_int80(&mut self) {
        self.code.extend_from_slice(&[0xCD, 0x80]);
    }

    /// Helper: Emit a complete `write(fd, msg, len)` system call.
    pub fn emit_sys_write(&mut self, fd: u32, msg: &[u8]) {
        let str_vaddr = self.add_string(msg);
        self.emit_mov_eax(1); // SYS_WRITE = 1
        self.emit_mov_edi(fd);
        self.emit_mov_rsi(str_vaddr);
        self.emit_mov_rdx(msg.len() as u64);
        self.emit_syscall();
    }

    /// Helper: Emit a complete `nanosleep(ms)` system call.
    pub fn emit_sys_nanosleep(&mut self, ms: u64) {
        self.emit_mov_eax(35); // SYS_NANOSLEEP = 35
        self.emit_mov_edi(ms as u32);
        self.emit_syscall();
    }

    /// Helper: Emit `exit(code)` system call.
    pub fn emit_sys_exit(&mut self, code: i32) {
        self.emit_mov_eax(60); // SYS_EXIT = 60
        self.emit_mov_edi(code as u32);
        self.emit_syscall();
    }

    /// Assemble and build the complete 64-bit ELF binary image.
    pub fn build(self) -> Vec<u8> {
        let is_complex = self.is_dylib || self.is_dynamic;
        if !is_complex {
            // Standard static ELF executable
            let rodata_offset = 1024usize;
            let mut binary = Vec::with_capacity(rodata_offset + self.rodata.len());

            // 1. ELF64 Header (64 bytes)
            binary.extend_from_slice(&ELF_MAGIC); // e_ident[0..4]
            binary.push(2);                       // 64-bit (ELFCLASS64)
            binary.push(1);                       // Little-endian (ELFDATA2LSB)
            binary.push(1);                       // ELF Version 1
            binary.extend_from_slice(&[0u8; 9]);  // OS ABI and padding
            binary.extend_from_slice(&2u16.to_le_bytes()); // e_type: ET_EXEC (2)
            binary.extend_from_slice(&0x3Eu16.to_le_bytes()); // e_machine: EM_X86_64 (0x3E)
            binary.extend_from_slice(&1u32.to_le_bytes()); // e_version: 1
            binary.extend_from_slice(&ENTRY_POINT.to_le_bytes()); // e_entry
            binary.extend_from_slice(&64u64.to_le_bytes()); // e_phoff: 64
            binary.extend_from_slice(&0u64.to_le_bytes()); // e_shoff: 0
            binary.extend_from_slice(&0u32.to_le_bytes()); // e_flags: 0
            binary.extend_from_slice(&64u16.to_le_bytes()); // e_ehsize: 64
            binary.extend_from_slice(&56u16.to_le_bytes()); // e_phentsize: 56
            binary.extend_from_slice(&1u16.to_le_bytes()); // e_phnum: 1
            binary.extend_from_slice(&64u16.to_le_bytes()); // e_shentsize: 64
            binary.extend_from_slice(&0u16.to_le_bytes()); // e_shnum: 0
            binary.extend_from_slice(&0u16.to_le_bytes()); // e_shstrndx: 0
            debug_assert_eq!(binary.len(), 64);

            let total_file_size = (rodata_offset + self.rodata.len()) as u64;
            let total_mem_size = 4096u64;

            // 2. Program Header (56 bytes, PT_LOAD)
            binary.extend_from_slice(&1u32.to_le_bytes()); // p_type: PT_LOAD (1)
            binary.extend_from_slice(&7u32.to_le_bytes()); // p_flags: PF_R | PF_W | PF_X (7)
            binary.extend_from_slice(&0u64.to_le_bytes()); // p_offset: 0
            binary.extend_from_slice(&BASE_VADDR.to_le_bytes()); // p_vaddr
            binary.extend_from_slice(&BASE_VADDR.to_le_bytes()); // p_paddr
            binary.extend_from_slice(&total_file_size.to_le_bytes()); // p_filesz
            binary.extend_from_slice(&total_mem_size.to_le_bytes()); // p_memsz
            binary.extend_from_slice(&4096u64.to_le_bytes()); // p_align: 4096
            debug_assert_eq!(binary.len(), 120);

            // 3. Append Code (from offset 120 up to rodata_offset)
            binary.extend_from_slice(&self.code);
            while binary.len() < rodata_offset {
                binary.push(0x90);
            }
            // 4. Append RODATA
            binary.extend_from_slice(&self.rodata);
            return binary;
        }

        // Dynamic Executable or Shared Library
        let base_vaddr = if self.is_dylib { 0 } else { BASE_VADDR };
        let phnum: u16 = if self.is_dynamic { 3 } else { 2 };
        let entry_point = base_vaddr + 0x100;

        let rodata_offset = 0x400usize;
        let dyn_offset = 0x600usize;
        let sym_offset = 0x800usize;
        let str_offset = 0x900usize;
        let rela_offset = 0xA00usize;
        let total_file_size = 0xC00u64;
        let total_mem_size = 8192u64;

        let mut binary = Vec::with_capacity(total_mem_size as usize);

        // 1. ELF Header (64 bytes)
        binary.extend_from_slice(&ELF_MAGIC);
        binary.push(2); // 64-bit
        binary.push(1); // Little endian
        binary.push(1); // ELF version 1
        binary.extend_from_slice(&[0u8; 9]);
        let e_type = if self.is_dylib { 3u16 } else { 2u16 }; // ET_DYN (3) or ET_EXEC (2)
        binary.extend_from_slice(&e_type.to_le_bytes());
        binary.extend_from_slice(&0x3Eu16.to_le_bytes()); // EM_X86_64
        binary.extend_from_slice(&1u32.to_le_bytes());
        binary.extend_from_slice(&entry_point.to_le_bytes());
        binary.extend_from_slice(&64u64.to_le_bytes()); // e_phoff: 64
        binary.extend_from_slice(&0u64.to_le_bytes());
        binary.extend_from_slice(&0u32.to_le_bytes());
        binary.extend_from_slice(&64u16.to_le_bytes());
        binary.extend_from_slice(&56u16.to_le_bytes()); // phentsize
        binary.extend_from_slice(&phnum.to_le_bytes());
        binary.extend_from_slice(&64u16.to_le_bytes());
        binary.extend_from_slice(&0u16.to_le_bytes());
        binary.extend_from_slice(&0u16.to_le_bytes());

        // 2. Program Headers
        if self.is_dynamic {
            // PT_INTERP (offset 0x200)
            binary.extend_from_slice(&3u32.to_le_bytes()); // PT_INTERP (3)
            binary.extend_from_slice(&4u32.to_le_bytes()); // PF_R (4)
            binary.extend_from_slice(&0x200u64.to_le_bytes()); // p_offset
            binary.extend_from_slice(&(base_vaddr + 0x200).to_le_bytes()); // p_vaddr
            binary.extend_from_slice(&(base_vaddr + 0x200).to_le_bytes());
            let interp_len = self.interp.as_ref().map(|s| s.len() + 1).unwrap_or(0) as u64;
            binary.extend_from_slice(&interp_len.to_le_bytes());
            binary.extend_from_slice(&interp_len.to_le_bytes());
            binary.extend_from_slice(&1u64.to_le_bytes());
        }

        // PT_LOAD
        binary.extend_from_slice(&1u32.to_le_bytes()); // PT_LOAD (1)
        binary.extend_from_slice(&7u32.to_le_bytes()); // PF_R | PF_W | PF_X
        binary.extend_from_slice(&0u64.to_le_bytes()); // p_offset
        binary.extend_from_slice(&base_vaddr.to_le_bytes()); // p_vaddr
        binary.extend_from_slice(&base_vaddr.to_le_bytes());
        binary.extend_from_slice(&total_file_size.to_le_bytes());
        binary.extend_from_slice(&total_mem_size.to_le_bytes());
        binary.extend_from_slice(&4096u64.to_le_bytes());

        // PT_DYNAMIC
        binary.extend_from_slice(&2u32.to_le_bytes()); // PT_DYNAMIC (2)
        binary.extend_from_slice(&6u32.to_le_bytes()); // PF_R | PF_W
        binary.extend_from_slice(&(dyn_offset as u64).to_le_bytes());
        binary.extend_from_slice(&(base_vaddr + dyn_offset as u64).to_le_bytes());
        binary.extend_from_slice(&(base_vaddr + dyn_offset as u64).to_le_bytes());
        binary.extend_from_slice(&0x200u64.to_le_bytes()); // 512 bytes
        binary.extend_from_slice(&0x200u64.to_le_bytes());
        binary.extend_from_slice(&8u64.to_le_bytes());

        // Pad up to code entry offset 0x100 = 256
        while binary.len() < 0x100 {
            binary.push(0x90);
        }

        // Append Code
        binary.extend_from_slice(&self.code);

        // Pad to interp offset 0x200
        while binary.len() < 0x200 {
            binary.push(0);
        }
        if let Some(ref interp) = self.interp {
            binary.extend_from_slice(interp.as_bytes());
            binary.push(0);
        }

        // Pad to RODATA at 0x400
        while binary.len() < rodata_offset {
            binary.push(0);
        }
        binary.extend_from_slice(&self.rodata);

        // Build String Table (.dynstr)
        let mut dynstr = Vec::new();
        dynstr.push(0); // Index 0 is empty string

        let mut needed_str_indices = Vec::new();
        for lib in &self.needed_libs {
            let idx = dynstr.len() as u32;
            dynstr.extend_from_slice(lib.as_bytes());
            dynstr.push(0);
            needed_str_indices.push(idx);
        }

        let mut sym_str_indices = Vec::new();
        for imp in &self.imports {
            let idx = dynstr.len() as u32;
            dynstr.extend_from_slice(imp.as_bytes());
            dynstr.push(0);
            sym_str_indices.push(idx);
        }

        let mut exp_str_indices = Vec::new();
        for (exp, _) in &self.exports {
            let idx = dynstr.len() as u32;
            dynstr.extend_from_slice(exp.as_bytes());
            dynstr.push(0);
            exp_str_indices.push(idx);
        }

        // Build Symbol Table (.dynsym)
        let mut dynsym: Vec<Elf64Sym> = Vec::new();
        // Index 0: STN_UNDEF
        dynsym.push(Elf64Sym {
            st_name: 0,
            st_info: 0,
            st_other: 0,
            st_shndx: 0,
            st_value: 0,
            st_size: 0,
        });

        // Imports (undefined symbols in executable)
        for &str_idx in &sym_str_indices {
            dynsym.push(Elf64Sym {
                st_name: str_idx,
                st_info: 0x12, // STB_GLOBAL | STT_FUNC
                st_other: 0,
                st_shndx: 0,   // SHN_UNDEF
                st_value: 0,
                st_size: 0,
            });
        }

        // Exports (defined symbols in shared library)
        for (i, &str_idx) in exp_str_indices.iter().enumerate() {
            let func_vaddr = self.exports[i].1;
            dynsym.push(Elf64Sym {
                st_name: str_idx,
                st_info: 0x12, // STB_GLOBAL | STT_FUNC
                st_other: 0,
                st_shndx: 1,   // Section 1 (.text)
                st_value: func_vaddr,
                st_size: 64,
            });
        }

        // Build Relocation Table (.rela.plt)
        let mut dynrela: Vec<Elf64Rela> = Vec::new();
        for (i, _) in self.imports.iter().enumerate() {
            let sym_idx = (i + 1) as u64; // index in dynsym
            let r_info = (sym_idx << 32) | (R_X86_64_JUMP_SLOT as u64);
            let r_offset = 0x00401000 + (i as u64 * 8);
            dynrela.push(Elf64Rela {
                r_offset,
                r_info,
                r_addend: 0,
            });
        }

        // Build Dynamic Section (PT_DYNAMIC at 0x600)
        let mut dyntags: Vec<Elf64Dyn> = Vec::new();
        for &str_idx in &needed_str_indices {
            dyntags.push(Elf64Dyn { d_tag: DT_NEEDED, d_val: str_idx as u64 });
        }
        dyntags.push(Elf64Dyn { d_tag: DT_STRTAB, d_val: base_vaddr + str_offset as u64 });
        dyntags.push(Elf64Dyn { d_tag: DT_SYMTAB, d_val: base_vaddr + sym_offset as u64 });
        dyntags.push(Elf64Dyn { d_tag: DT_STRSZ, d_val: dynstr.len() as u64 });
        dyntags.push(Elf64Dyn { d_tag: DT_SYMENT, d_val: core::mem::size_of::<Elf64Sym>() as u64 });

        if !dynrela.is_empty() {
            dyntags.push(Elf64Dyn { d_tag: DT_PLTGOT, d_val: 0x00401000 });
            dyntags.push(Elf64Dyn { d_tag: DT_JMPREL, d_val: base_vaddr + rela_offset as u64 });
            dyntags.push(Elf64Dyn { d_tag: DT_PLTRELSZ, d_val: (dynrela.len() * core::mem::size_of::<Elf64Rela>()) as u64 });
            dyntags.push(Elf64Dyn { d_tag: DT_RELAENT, d_val: core::mem::size_of::<Elf64Rela>() as u64 });
        }
        dyntags.push(Elf64Dyn { d_tag: DT_NULL, d_val: 0 });

        // Pad to dyn_offset 0x600
        while binary.len() < dyn_offset {
            binary.push(0);
        }
        for d in &dyntags {
            binary.extend_from_slice(&d.d_tag.to_le_bytes());
            binary.extend_from_slice(&d.d_val.to_le_bytes());
        }

        // Pad to sym_offset 0x800
        while binary.len() < sym_offset {
            binary.push(0);
        }
        for s in &dynsym {
            binary.extend_from_slice(&s.st_name.to_le_bytes());
            binary.push(s.st_info);
            binary.push(s.st_other);
            binary.extend_from_slice(&s.st_shndx.to_le_bytes());
            binary.extend_from_slice(&s.st_value.to_le_bytes());
            binary.extend_from_slice(&s.st_size.to_le_bytes());
        }

        // Pad to str_offset 0x900
        while binary.len() < str_offset {
            binary.push(0);
        }
        binary.extend_from_slice(&dynstr);

        // Pad to rela_offset 0xA00
        while binary.len() < rela_offset {
            binary.push(0);
        }
        for r in &dynrela {
            binary.extend_from_slice(&r.r_offset.to_le_bytes());
            binary.extend_from_slice(&r.r_info.to_le_bytes());
            binary.extend_from_slice(&r.r_addend.to_le_bytes());
        }

        // Pad up to total_file_size
        while binary.len() < total_file_size as usize {
            binary.push(0);
        }

        binary
    }
}
