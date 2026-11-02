pub mod devfs;
pub mod procfs;
pub mod ramfs;
pub mod vfs;

use crate::fs::devfs::DevFsDirectory;
use crate::fs::procfs::ProcFsDirectory;
use crate::fs::ramfs::RamInode;
use crate::fs::vfs::VFS;
use crate::kinfo;

pub fn init() {
    // 1. Create root directory Inode
    let root = RamInode::new_dir();

    // 2. Mount /dev
    let devfs = DevFsDirectory::new();
    root.lock().add_child("dev", devfs);

    // 3. Mount /proc
    let procfs = ProcFsDirectory::new();
    root.lock().add_child("proc", procfs);

    // 4. Create standard POSIX directories
    let etc = RamInode::new_dir();
    let bin = RamInode::new_dir();
    let tmp = RamInode::new_dir();
    let home = RamInode::new_dir();

    // Populate /etc/os-release
    let os_release_content = b"NAME=\"CosmosKernel\"\nVERSION=\"0.1.0\"\nID=cosmos\nPRETTY_NAME=\"CosmosKernel Monolithic OS 0.1.0 (x86_64)\"\n";
    let os_release = RamInode::new_file(os_release_content);
    etc.lock().add_child("os-release", os_release);

    // Populate /etc/motd
    let motd_content = b"\n=======================================================\n  Welcome to CosmosKernel - Modern Rust Monolithic OS  \n=======================================================\nType 'help' for a list of available commands.\n\n";
    let motd = RamInode::new_file(motd_content);
    etc.lock().add_child("motd", motd);

    root.lock().add_child("etc", etc);
    root.lock().add_child("bin", bin);
    root.lock().add_child("tmp", tmp);
    root.lock().add_child("home", home);

    // Register root with global VFS manager
    VFS.lock().set_root(root);

    kinfo!("VFS initialized with rootfs, /dev, /proc, /etc, /bin, /tmp, /home");
}
