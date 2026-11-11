extern crate alloc;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::mm::address_space::AddressSpace;
use crate::task::elf::{
    ElfBinary, PT_DYNAMIC, PT_INTERP, PT_LOAD, PF_W,
    DT_NEEDED, DT_STRTAB, DT_SYMTAB, DT_JMPREL, DT_PLTRELSZ, DT_RELA, DT_RELASZ,
    DT_STRSZ,
    Elf64Dyn, Elf64Rela, Elf64Sym,
    R_X86_64_JUMP_SLOT, R_X86_64_GLOB_DAT, R_X86_64_64, R_X86_64_RELATIVE,
};
use crate::kinfo;

#[derive(Debug, Clone)]
pub struct LoadedLibrary {
    pub name: String,
    pub base_vaddr: u64,
    pub symbols: Vec<(String, u64)>, // (symbol_name, resolved_vaddr)
}

pub struct DynamicLinker {
    pub loaded_libraries: Vec<LoadedLibrary>,
    pub next_base_vaddr: u64,
}

impl DynamicLinker {
    pub fn new() -> Self {
        Self {
            loaded_libraries: Vec::new(),
            next_base_vaddr: 0x0000_7000_0000_0000,
        }
    }

    /// Check whether an ELF binary contains dynamic linking structures.
    pub fn is_dynamic(elf: &ElfBinary) -> bool {
        elf.program_headers.iter().any(|ph| ph.p_type == PT_DYNAMIC || ph.p_type == PT_INTERP)
    }

    /// Dynamically load needed shared libraries and perform GOT/PLT runtime relocations.
    pub fn link_binary(&mut self, elf: &ElfBinary, space: &mut AddressSpace) -> Result<(), &'static str> {
        let dyn_ph = match elf.program_headers.iter().find(|ph| ph.p_type == PT_DYNAMIC) {
            Some(ph) => ph,
            None => return Ok(()),
        };

        let dyn_offset = dyn_ph.p_offset as usize;
        let dyn_size = dyn_ph.p_filesz as usize;
        if dyn_offset + dyn_size > elf.raw_data.len() {
            return Err("PT_DYNAMIC out of file bounds");
        }

        let num_dyn = dyn_size / core::mem::size_of::<Elf64Dyn>();
        let mut strtab_offset = 0u64;
        let mut symtab_offset = 0u64;
        let mut jmprel_offset = 0u64;
        let mut pltrelsz = 0u64;
        let mut rela_offset = 0u64;
        let mut relasz = 0u64;
        let mut needed_offsets = Vec::new();

        for i in 0..num_dyn {
            let entry_offset = dyn_offset + i * core::mem::size_of::<Elf64Dyn>();
            let entry = unsafe {
                core::ptr::read_unaligned(elf.raw_data[entry_offset..].as_ptr() as *const Elf64Dyn)
            };
            match entry.d_tag {
                DT_NEEDED => needed_offsets.push(entry.d_val),
                DT_STRTAB => strtab_offset = entry.d_val,
                DT_SYMTAB => symtab_offset = entry.d_val,
                DT_JMPREL => jmprel_offset = entry.d_val,
                DT_PLTRELSZ => pltrelsz = entry.d_val,
                DT_RELA => rela_offset = entry.d_val,
                DT_RELASZ => relasz = entry.d_val,
                _ => {}
            }
        }

        let vaddr_to_offset = |vaddr: u64| -> Option<usize> {
            for ph in &elf.program_headers {
                if ph.p_type == PT_LOAD && vaddr >= ph.p_vaddr && vaddr < ph.p_vaddr + ph.p_filesz {
                    return Some((ph.p_offset + (vaddr - ph.p_vaddr)) as usize);
                }
            }
            None
        };

        let strtab_file_off = vaddr_to_offset(strtab_offset).unwrap_or(0);
        let symtab_file_off = vaddr_to_offset(symtab_offset).unwrap_or(0);

        let get_string = |str_off: u64| -> String {
            let mut s = String::new();
            let mut idx = strtab_file_off + str_off as usize;
            while idx < elf.raw_data.len() && elf.raw_data[idx] != 0 {
                s.push(elf.raw_data[idx] as char);
                idx += 1;
            }
            s
        };

        // 1. Load all DT_NEEDED shared libraries
        for needed_off in needed_offsets {
            let lib_name = get_string(needed_off);
            if !lib_name.is_empty() {
                self.load_library(&lib_name, space)?;
            }
        }

        // 2. Collect relocations (PLT & Data)
        let mut relocations = Vec::new();
        if jmprel_offset != 0 && pltrelsz != 0 {
            if let Some(jmprel_file_off) = vaddr_to_offset(jmprel_offset) {
                let count = (pltrelsz as usize) / core::mem::size_of::<Elf64Rela>();
                for i in 0..count {
                    let r_off = jmprel_file_off + i * core::mem::size_of::<Elf64Rela>();
                    if r_off + core::mem::size_of::<Elf64Rela>() <= elf.raw_data.len() {
                        let rela = unsafe {
                            core::ptr::read_unaligned(elf.raw_data[r_off..].as_ptr() as *const Elf64Rela)
                        };
                        relocations.push(rela);
                    }
                }
            }
        }

        if rela_offset != 0 && relasz != 0 {
            if let Some(rela_file_off) = vaddr_to_offset(rela_offset) {
                let count = (relasz as usize) / core::mem::size_of::<Elf64Rela>();
                for i in 0..count {
                    let r_off = rela_file_off + i * core::mem::size_of::<Elf64Rela>();
                    if r_off + core::mem::size_of::<Elf64Rela>() <= elf.raw_data.len() {
                        let rela = unsafe {
                            core::ptr::read_unaligned(elf.raw_data[r_off..].as_ptr() as *const Elf64Rela)
                        };
                        relocations.push(rela);
                    }
                }
            }
        }

        // 3. Resolve symbols and patch GOT/PLT targets
        for rela in relocations {
            let sym_idx = rela.r_sym() as usize;
            let sym_file_off = symtab_file_off + sym_idx * core::mem::size_of::<Elf64Sym>();
            let sym_name = if sym_file_off + core::mem::size_of::<Elf64Sym>() <= elf.raw_data.len() {
                let sym = unsafe {
                    core::ptr::read_unaligned(elf.raw_data[sym_file_off..].as_ptr() as *const Elf64Sym)
                };
                get_string(sym.st_name as u64)
            } else {
                String::new()
            };

            let resolved_val = if !sym_name.is_empty() {
                let val = self.lookup_symbol(&sym_name)
                    .ok_or("Failed to resolve dynamic symbol")?;
                crate::kinfo!("Dynamic Linker: resolved '{}' -> 0x{:016X}", sym_name, val);
                val
            } else {
                0
            };

            let r_type = rela.r_type();
            let final_val = match r_type {
                R_X86_64_JUMP_SLOT | R_X86_64_GLOB_DAT => resolved_val,
                R_X86_64_64 => (resolved_val as i64 + rela.r_addend) as u64,
                R_X86_64_RELATIVE => rela.r_addend as u64,
                _ => continue,
            };

            crate::kinfo!("Dynamic Linker: patching GOT [0x{:016X}] = 0x{:016X}", rela.r_offset, final_val);
            space.write_user_u64(rela.r_offset, final_val)?;
        }

        Ok(())
    }

    /// Load a shared library from /lib/<name> into the user address space.
    pub fn load_library(&mut self, name: &str, space: &mut AddressSpace) -> Result<(), &'static str> {
        if self.loaded_libraries.iter().any(|l| l.name == name) {
            return Ok(());
        }

        let path = format!("/lib/{}", name);
        let vfs = crate::fs::vfs::VFS.lock();
        let file = vfs.open(&path, 0).map_err(|_| "Failed to open shared library")?;
        drop(vfs);

        let mut buf = alloc::vec![0u8; 65536];
        let n = file.read(&mut buf).map_err(|_| "Failed to read shared library")?;
        buf.truncate(n);

        let elf = ElfBinary::parse(&buf)?;
        let base_vaddr = self.next_base_vaddr;
        self.next_base_vaddr += 0x1000_0000; // 256 MiB per library space

        // Map PT_LOAD segments shifted by base_vaddr
        let vmm = crate::mm::vmm::VMM.lock();
        let pml4 = unsafe { &mut *(crate::mm::phys_to_virt(space.pml4_phys) as *mut crate::mm::vmm::PageTable) };

        for ph in &elf.program_headers {
            if ph.p_type != PT_LOAD || ph.p_memsz == 0 {
                continue;
            }

            let start_vaddr = base_vaddr + ph.p_vaddr;
            let end_vaddr = start_vaddr + ph.p_memsz;
            let page_start = crate::mm::align_down(start_vaddr, crate::mm::PAGE_SIZE as u64);
            let page_end = crate::mm::align_up(end_vaddr, crate::mm::PAGE_SIZE as u64);
            let pages = (page_end - page_start) / (crate::mm::PAGE_SIZE as u64);

            let mut flags = crate::mm::vmm::PAGE_PRESENT | crate::mm::vmm::PAGE_USER;
            if (ph.p_flags & PF_W) != 0 {
                flags |= crate::mm::vmm::PAGE_WRITABLE;
            }

            for p in 0..pages {
                let curr_page = page_start + p * (crate::mm::PAGE_SIZE as u64);
                let frame = crate::mm::pmm::PMM.lock().alloc_frame().ok_or("Out of memory for shared library")?;
                let virt_frame = crate::mm::phys_to_virt(frame) as *mut u8;

                unsafe {
                    core::ptr::write_bytes(virt_frame, 0, crate::mm::PAGE_SIZE);
                }

                let file_start = start_vaddr;
                let file_end = start_vaddr + ph.p_filesz;
                let page_data_start = curr_page.max(file_start);
                let page_data_end = (curr_page + crate::mm::PAGE_SIZE as u64).min(file_end);

                if page_data_end > page_data_start {
                    let copy_len = (page_data_end - page_data_start) as usize;
                    let frame_offset = (page_data_start - curr_page) as usize;
                    let file_offset = (ph.p_offset + (page_data_start - file_start)) as usize;

                    if file_offset + copy_len <= buf.len() {
                        unsafe {
                            core::ptr::copy_nonoverlapping(buf[file_offset..].as_ptr(), virt_frame.add(frame_offset), copy_len);
                        }
                    }
                }

                vmm.map_page(pml4, curr_page, frame, flags)?;
            }
        }
        drop(vmm);

        let mut symbols = Vec::new();
        self.extract_symbols(&elf, base_vaddr, &mut symbols);

        self.loaded_libraries.push(LoadedLibrary {
            name: String::from(name),
            base_vaddr,
            symbols,
        });

        kinfo!("Loaded shared library '{}' at 0x{:016X}", name, base_vaddr);
        Ok(())
    }

    /// Extract exported symbols from a shared library's dynamic symbol table.
    fn extract_symbols(&self, elf: &ElfBinary, base_vaddr: u64, out: &mut Vec<(String, u64)>) {
        let dyn_ph = match elf.program_headers.iter().find(|ph| ph.p_type == PT_DYNAMIC) {
            Some(ph) => ph,
            None => return,
        };

        let dyn_offset = dyn_ph.p_offset as usize;
        let dyn_size = dyn_ph.p_filesz as usize;
        if dyn_offset + dyn_size > elf.raw_data.len() {
            return;
        }

        let num_dyn = dyn_size / core::mem::size_of::<Elf64Dyn>();
        let mut strtab_offset = 0u64;
        let mut symtab_offset = 0u64;
        let mut strsz = 0u64;

        for i in 0..num_dyn {
            let entry_offset = dyn_offset + i * core::mem::size_of::<Elf64Dyn>();
            let entry = unsafe {
                core::ptr::read_unaligned(elf.raw_data[entry_offset..].as_ptr() as *const Elf64Dyn)
            };
            match entry.d_tag {
                DT_STRTAB => strtab_offset = entry.d_val,
                DT_SYMTAB => symtab_offset = entry.d_val,
                DT_STRSZ => strsz = entry.d_val,
                _ => {}
            }
        }

        let vaddr_to_offset = |vaddr: u64| -> Option<usize> {
            for ph in &elf.program_headers {
                if ph.p_type == PT_LOAD && vaddr >= ph.p_vaddr && vaddr < ph.p_vaddr + ph.p_filesz {
                    return Some((ph.p_offset + (vaddr - ph.p_vaddr)) as usize);
                }
            }
            None
        };

        let strtab_file_off = match vaddr_to_offset(strtab_offset) {
            Some(off) => off,
            None => return,
        };
        let symtab_file_off = match vaddr_to_offset(symtab_offset) {
            Some(off) => off,
            None => return,
        };

        // Number of symbols can be estimated from distance to next section or strsz
        let sym_count = 32usize;
        for i in 0..sym_count {
            let sym_off = symtab_file_off + i * core::mem::size_of::<Elf64Sym>();
            if sym_off + core::mem::size_of::<Elf64Sym>() > elf.raw_data.len() {
                break;
            }

            let sym = unsafe {
                core::ptr::read_unaligned(elf.raw_data[sym_off..].as_ptr() as *const Elf64Sym)
            };

            if sym.st_name != 0 && (sym.st_name as u64) < strsz {
                let mut name = String::new();
                let mut idx = strtab_file_off + sym.st_name as usize;
                while idx < elf.raw_data.len() && elf.raw_data[idx] != 0 {
                    name.push(elf.raw_data[idx] as char);
                    idx += 1;
                }

                if !name.is_empty() {
                    let resolved_vaddr = base_vaddr + sym.st_value;
                    out.push((name, resolved_vaddr));
                }
            }
        }
    }

    /// Look up a symbol across all loaded shared libraries.
    pub fn lookup_symbol(&self, name: &str) -> Option<u64> {
        for lib in &self.loaded_libraries {
            for (sym_name, vaddr) in &lib.symbols {
                if sym_name == name {
                    return Some(*vaddr);
                }
            }
        }
        None
    }
}
