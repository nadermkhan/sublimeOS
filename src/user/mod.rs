pub mod binaries;
pub mod builder;
pub mod libc;

use crate::kinfo;

/// Initialize the Userspace Subsystem and populate `/bin` with native 64-bit ELF user binaries.
pub fn init_userland() {
    let init_elf = binaries::generate_init_binary();
    let calc_elf = binaries::generate_calc_binary();
    let sh_elf = binaries::generate_sh_binary();
    let snake_elf = binaries::generate_snake_binary();

    let _ = crate::shell::pipeline::write_file_to_vfs("/bin/init", &init_elf, false);
    let _ = crate::shell::pipeline::write_file_to_vfs("/bin/calc", &calc_elf, false);
    let _ = crate::shell::pipeline::write_file_to_vfs("/bin/sh", &sh_elf, false);
    let _ = crate::shell::pipeline::write_file_to_vfs("/bin/snake", &snake_elf, false);

    kinfo!("Userspace subsystem initialized (cosmic-libc, /bin/init, /bin/calc, /bin/sh, /bin/snake)");
}
