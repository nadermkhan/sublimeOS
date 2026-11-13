pub mod binaries;
pub mod builder;
pub mod libc;
pub mod linker;

use crate::kinfo;

/// Initialize the Userspace Subsystem and populate `/bin` and `/lib` with native 64-bit ELF user binaries and shared libraries.
pub fn init_userland() {
    let init_elf = binaries::generate_init_binary();
    let calc_elf = binaries::generate_calc_binary();
    let hello_elf = binaries::generate_hello_binary();
    let sh_elf = binaries::generate_sh_binary();
    let snake_elf = binaries::generate_snake_binary();
    let echo_elf = binaries::generate_echo_binary();
    let cat_elf = binaries::generate_cat_binary();
    let ls_elf = binaries::generate_ls_binary();
    let grep_elf = binaries::generate_grep_binary();
    let wc_elf = binaries::generate_wc_binary();
    let edit_elf = binaries::generate_edit_binary();
    let dyntest_elf = binaries::generate_dyntest_binary();
    let windowserver_elf = binaries::generate_windowserver_binary();
    let telnetd_elf = binaries::generate_telnetd_binary();
    let sshd_elf = binaries::generate_sshd_binary();

    let ld_so = binaries::generate_ld_so();
    let libc_so = binaries::generate_libc_so();
    let libm_so = binaries::generate_libm_so();
    let libgui_so = binaries::generate_libgui_so();

    // Modern SublimeOS Command Binaries
    let _ = crate::shell::pipeline::write_file_to_vfs("/bin/list", &ls_elf, false);
    let _ = crate::shell::pipeline::write_file_to_vfs("/bin/view", &cat_elf, false);
    let _ = crate::shell::pipeline::write_file_to_vfs("/bin/print", &echo_elf, false);
    let _ = crate::shell::pipeline::write_file_to_vfs("/bin/find", &grep_elf, false);
    let _ = crate::shell::pipeline::write_file_to_vfs("/bin/count", &wc_elf, false);

    // Compatibility aliases
    let _ = crate::shell::pipeline::write_file_to_vfs("/bin/init", &init_elf, false);
    let _ = crate::shell::pipeline::write_file_to_vfs("/bin/calc", &calc_elf, false);
    let _ = crate::shell::pipeline::write_file_to_vfs("/bin/hello", &hello_elf, false);
    let _ = crate::shell::pipeline::write_file_to_vfs("/bin/sh", &sh_elf, false);
    let _ = crate::shell::pipeline::write_file_to_vfs("/bin/snake", &snake_elf, false);
    let _ = crate::shell::pipeline::write_file_to_vfs("/bin/echo", &echo_elf, false);
    let _ = crate::shell::pipeline::write_file_to_vfs("/bin/cat", &cat_elf, false);
    let _ = crate::shell::pipeline::write_file_to_vfs("/bin/ls", &ls_elf, false);
    let _ = crate::shell::pipeline::write_file_to_vfs("/bin/grep", &grep_elf, false);
    let _ = crate::shell::pipeline::write_file_to_vfs("/bin/wc", &wc_elf, false);
    let _ = crate::shell::pipeline::write_file_to_vfs("/bin/edit", &edit_elf, false);
    let _ = crate::shell::pipeline::write_file_to_vfs("/bin/dyntest", &dyntest_elf, false);
    let _ = crate::shell::pipeline::write_file_to_vfs("/bin/windowserver", &windowserver_elf, false);
    let _ = crate::shell::pipeline::write_file_to_vfs("/bin/telnetd", &telnetd_elf, false);
    let _ = crate::shell::pipeline::write_file_to_vfs("/bin/sshd", &sshd_elf, false);

    let _ = crate::shell::pipeline::write_file_to_vfs("/lib/ld-cosmic.so", &ld_so, false);
    let _ = crate::shell::pipeline::write_file_to_vfs("/lib/libc.so", &libc_so, false);
    let _ = crate::shell::pipeline::write_file_to_vfs("/lib/libm.so", &libm_so, false);
    let _ = crate::shell::pipeline::write_file_to_vfs("/lib/libgui.so", &libgui_so, false);

    let rc_sh = b"# SublimeOS Master Run-Control Init Script\n\
export USER=sublime\n\
export HOME=/\n\
export PATH=/bin\n\
export OS=SublimeOS\n\
export KERNEL=KiteKernel\n\
print \"\x1b[1;36m[SublimeOS Init]\x1b[0m Services and environment initialized.\"\n";
    let _ = crate::shell::pipeline::write_file_to_vfs("/etc/rc.sh", rc_sh, false);

    kinfo!("Userspace subsystem initialized (cosmic-libc, /bin/*, /lib/*, /etc/rc.sh)");
}

/// Launch the master userspace init supervisor as PID 1 in Ring 3.
pub fn launch_init() -> ! {
    let vfs = crate::fs::vfs::VFS.lock();
    let file = vfs.open("/bin/init", 0).expect("Failed to open /bin/init for PID 1 boot");
    drop(vfs);
    let mut buf = alloc::vec![0u8; 131072];
    let n = file.read(&mut buf).expect("Failed to read /bin/init for PID 1 boot");

    crate::task::user::spawn_init_process(&buf[..n])
        .expect("Kernel panic: Failed to spawn /bin/init (PID 1) in Ring 3");

    // Retire the kernel boot thread so the scheduler immediately dispatches PID 1
    crate::task::scheduler::exit_current_thread(0);
}
