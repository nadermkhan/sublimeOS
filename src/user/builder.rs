extern crate alloc;
use alloc::vec::Vec;

pub const ELF_MAGIC: [u8; 4] = [0x7F, b'E', b'L', b'F'];
pub const BASE_VADDR: u64 = 0x00400000;
pub const ENTRY_POINT: u64 = 0x00400078;
pub const RODATA_VADDR: u64 = 0x00400400;

pub struct ElfBuilder {
    code: Vec<u8>,
    rodata: Vec<u8>,
}

impl ElfBuilder {
    pub fn new() -> Self {
        Self {
            code: Vec::new(),
            rodata: Vec::new(),
        }
    }

    /// Register a read-only string in the RODATA section and return its 64-bit virtual address.
    pub fn add_string(&mut self, s: &[u8]) -> u64 {
        let vaddr = RODATA_VADDR + self.rodata.len() as u64;
        self.rodata.extend_from_slice(s);
        vaddr
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

        // Pad code up to RODATA_VADDR (offset 0x400 = 1024)
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
        let total_mem_size = 4096u64; // 1 page (4 KiB)

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

        // Fill gap between code and rodata with NOPs (0x90)
        while binary.len() < rodata_offset {
            binary.push(0x90);
        }

        // 4. Append RODATA
        binary.extend_from_slice(&self.rodata);

        binary
    }
}
