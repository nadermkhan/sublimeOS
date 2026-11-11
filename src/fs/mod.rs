pub mod devfs;
pub mod exfat;
pub mod ext4;
pub mod fat32;
pub mod mount;
pub mod ntfs;
pub mod partition;
pub mod procfs;
pub mod ramfs;
pub mod vfs;

use crate::fs::devfs::DevFsDirectory;
use crate::fs::mount::{mount_all_storage, MOUNT_TABLE};
use crate::fs::procfs::ProcFsDirectory;
use crate::fs::ramfs::RamInode;
use crate::fs::vfs::VFS;
use crate::kinfo;

pub fn init() {
    // 1. Create root directory Inode
    let root = RamInode::new_dir();

    // 2. Mount /dev (Devices: disk0, disk0s1, ttys000, null, zero, random)
    let devfs = DevFsDirectory::new();
    root.lock().add_child("dev", devfs);

    // 3. Mount /proc (Sublime kernel diagnostic nodes)
    let procfs = ProcFsDirectory::new();
    root.lock().add_child("proc", procfs);

    // 4. Create CosmicOS Directory Hierarchy
    let system = RamInode::new_dir();
    let library = RamInode::new_dir();
    let applications = RamInode::new_dir();
    let users = RamInode::new_dir();
    let volumes = RamInode::new_dir();
    let private = RamInode::new_dir();
    let bin = RamInode::new_dir();
    let lib = RamInode::new_dir();
    let etc = RamInode::new_dir();
    let tmp = RamInode::new_dir();
    let var = RamInode::new_dir();

    // /System/Library/CoreServices/SystemVersion.plist
    let sys_lib = RamInode::new_dir();
    let core_services = RamInode::new_dir();
    let sys_version_plist = b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//CosmicOS//DTD PLIST 1.0//EN\" \"http://www.cosmicos.org/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n\t<key>ProductBuildVersion</key>\n\t<string>1A01</string>\n\t<key>ProductName</key>\n\t<string>CosmicOS</string>\n\t<key>ProductUserVisibleVersion</key>\n\t<string>1.0</string>\n\t<key>ProductVersion</key>\n\t<string>1.0</string>\n\t<key>KernelName</key>\n\t<string>Sublime Kernel</string>\n\t<key>KernelVersion</key>\n\t<string>1.0.0</string>\n</dict>\n</plist>\n";
    core_services.lock().add_child("SystemVersion.plist", RamInode::new_file(sys_version_plist));
    sys_lib.lock().add_child("CoreServices", core_services);
    system.lock().add_child("Library", sys_lib);

    // /Library/Preferences/.GlobalPreferences.plist
    let lib_prefs = RamInode::new_dir();
    let cosmic_prefs = b"CosmicInterfaceStyle = Dark\nCosmicFontSmoothing = 2\nCosmicKeyboardLayout = US\n";
    lib_prefs.lock().add_child(".GlobalPreferences.plist", RamInode::new_file(cosmic_prefs));
    library.lock().add_child("Preferences", lib_prefs);

    // /Applications/
    let apps_utilities = RamInode::new_dir();
    apps_utilities.lock().add_child("Terminal.app", RamInode::new_file(b"Sublime 64-bit executable Terminal\n"));
    apps_utilities.lock().add_child("Activity Monitor.app", RamInode::new_file(b"Sublime 64-bit executable ActivityMonitor\n"));
    apps_utilities.lock().add_child("Console.app", RamInode::new_file(b"Sublime 64-bit executable Console\n"));
    applications.lock().add_child("Utilities", apps_utilities);
    applications.lock().add_child("Finder.app", RamInode::new_file(b"Sublime 64-bit executable Finder\n"));
    applications.lock().add_child("System Settings.app", RamInode::new_file(b"Sublime 64-bit executable SystemSettings\n"));

    // /Users/cosmos/
    let user_cosmos = RamInode::new_dir();
    user_cosmos.lock().add_child("Desktop", RamInode::new_dir());
    user_cosmos.lock().add_child("Documents", RamInode::new_dir());
    user_cosmos.lock().add_child("Downloads", RamInode::new_dir());
    let zshrc = b"# CosmicOS zsh configuration\nexport PATH=\"/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin\"\nalias ll=\"ls -la\"\n";
    user_cosmos.lock().add_child(".zshrc", RamInode::new_file(zshrc));
    users.lock().add_child("cosmos", user_cosmos);

    // /Volumes/
    let cosmic_hd = RamInode::new_dir();
    cosmic_hd.lock().add_child(".cosmic_container", RamInode::new_file(b"CosmicFS Container Version 1.0\n"));
    volumes.lock().add_child("Cosmic HD", cosmic_hd);
    volumes.lock().add_child("Cosmic USB", RamInode::new_dir());

    // /etc/hosts & /etc/motd
    let motd_content = b"\n=======================================================\n  Welcome to CosmicOS (Sublime Kernel 1.0.0 x86_64)\n=======================================================\nType 'sw_vers' or 'system_profiler' for system info.\n\n";
    etc.lock().add_child("motd", RamInode::new_file(motd_content));
    etc.lock().add_child("hosts", RamInode::new_file(b"127.0.0.1\tlocalhost\n255.255.255.255\tbroadcast\n::1\t\tlocalhost\n"));

    root.lock().add_child("System", system);
    root.lock().add_child("Library", library);
    root.lock().add_child("Applications", applications);
    root.lock().add_child("Users", users);
    root.lock().add_child("Volumes", volumes);
    root.lock().add_child("private", private);
    root.lock().add_child("bin", bin);
    root.lock().add_child("lib", lib);
    root.lock().add_child("etc", etc);
    root.lock().add_child("tmp", tmp);
    root.lock().add_child("var", var);

    // Register root with global VFS manager
    VFS.lock().set_root(root);

    // Register initial system mounts in mount table
    {
        let mut mt = MOUNT_TABLE.lock();
        mt.add("/dev/disk0", "/", "ramfs", "rw");
        mt.add("devfs", "/dev", "devfs", "rw");
        mt.add("procfs", "/proc", "procfs", "ro");
    }

    kinfo!("VFS initialized: CosmicOS hierarchy (/System, /Library, /Applications, /Users, /Volumes)");
}

/// Automatically detect and mount physical storage block devices (FAT32, exFAT, ext4, NTFS).
pub fn mount_storage_devices() {
    mount_all_storage();
}
