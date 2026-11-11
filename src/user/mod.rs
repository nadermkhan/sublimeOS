pub mod binaries;
pub mod builder;
pub mod libc;
pub mod linker;

use crate::kinfo;

/// Initialize the Userspace Subsystem and populate `/bin` and `/lib` with native 64-bit ELF user binaries and shared libraries.
pub fn init_userland() {
    let init_elf = binaries::generate_init_binary();
    let calc_elf = binaries::generate_calc_binary();
    let sh_elf = binaries::generate_sh_binary();
    let snake_elf = binaries::generate_snake_binary();
    let dyntest_elf = binaries::generate_dyntest_binary();

    let ld_so = binaries::generate_ld_so();
    let libc_so = binaries::generate_libc_so();
    let libm_so = binaries::generate_libm_so();

    let _ = crate::shell::pipeline::write_file_to_vfs("/bin/init", &init_elf, false);
    let _ = crate::shell::pipeline::write_file_to_vfs("/bin/calc", &calc_elf, false);
    let _ = crate::shell::pipeline::write_file_to_vfs("/bin/sh", &sh_elf, false);
    let _ = crate::shell::pipeline::write_file_to_vfs("/bin/snake", &snake_elf, false);
    let _ = crate::shell::pipeline::write_file_to_vfs("/bin/dyntest", &dyntest_elf, false);

    let _ = crate::shell::pipeline::write_file_to_vfs("/lib/ld-cosmic.so", &ld_so, false);
    let _ = crate::shell::pipeline::write_file_to_vfs("/lib/libc.so", &libc_so, false);
    let _ = crate::shell::pipeline::write_file_to_vfs("/lib/libm.so", &libm_so, false);

    kinfo!("Userspace subsystem initialized (cosmic-libc, /bin/*, /lib/ld-cosmic.so, /lib/libc.so, /lib/libm.so)");
}
