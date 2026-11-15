pub mod bcache;
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
    bcache::init();

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
    let sys_version_plist = b"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//SublimeOS//DTD PLIST 1.0//EN\" \"http://www.sublimeos.org/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n\t<key>ProductBuildVersion</key>\n\t<string>1A01</string>\n\t<key>ProductName</key>\n\t<string>SublimeOS</string>\n\t<key>ProductUserVisibleVersion</key>\n\t<string>1.0</string>\n\t<key>ProductVersion</key>\n\t<string>1.0</string>\n\t<key>KernelName</key>\n\t<string>KiteKernel</string>\n\t<key>KernelVersion</key>\n\t<string>1.0.0</string>\n\t<key>Author</key>\n\t<string>Nader Mahbub Khan</string>\n\t<key>Copyright</key>\n\t<string>Copyright (c) 2026 Nader Mahbub Khan</string>\n</dict>\n</plist>\n";
    core_services.lock().add_child("SystemVersion.plist", RamInode::new_file(sys_version_plist));
    sys_lib.lock().add_child("CoreServices", core_services);
    system.lock().add_child("Library", sys_lib);

    // /Library/Preferences/.GlobalPreferences.plist
    let lib_prefs = RamInode::new_dir();
    let cosmic_prefs = b"SublimeInterfaceStyle = Dark\nSublimeFontSmoothing = 2\nSublimeKeyboardLayout = US\n";
    lib_prefs.lock().add_child(".GlobalPreferences.plist", RamInode::new_file(cosmic_prefs));
    library.lock().add_child("Preferences", lib_prefs);

    // /Applications/
    let apps_utilities = RamInode::new_dir();
    apps_utilities.lock().add_child(
        "Terminal.app",
        RamInode::new_file(b"[SublimeApp]\nName=Terminal\nBuiltin=Terminal\nCategory=Utilities\nComment=SublimeOS Command Prompt & ELF Runner\n"),
    );
    apps_utilities.lock().add_child(
        "Task Manager.app",
        RamInode::new_file(b"[SublimeApp]\nName=Task Manager\nBuiltin=Sysmon\nCategory=Utilities\nComment=Processes, Memory & Network Monitor\n"),
    );
    apps_utilities.lock().add_child(
        "Calculator.app",
        RamInode::new_file(b"[SublimeApp]\nName=Calculator\nBuiltin=Calculator\nCategory=Utilities\nComment=Standard Metro Calculator\n"),
    );
    apps_utilities.lock().add_child(
        "Notepad.app",
        RamInode::new_file(b"[SublimeApp]\nName=Notepad\nBuiltin=TextEdit\nCategory=Utilities\nComment=Plain Text & Code Editor\n"),
    );
    applications.lock().add_child("Utilities", apps_utilities);
    applications.lock().add_child(
        "File Explorer.app",
        RamInode::new_file(b"[SublimeApp]\nName=File Explorer\nBuiltin=FileManager\nCategory=System\nComment=Browse Files, Drives & Applications\n"),
    );
    applications.lock().add_child(
        "Control Panel.app",
        RamInode::new_file(b"[SublimeApp]\nName=Control Panel\nBuiltin=Settings\nCategory=System\nComment=Personalization, Network & Sound\n"),
    );
    applications.lock().add_child(
        "Widget Studio.app",
        RamInode::new_file(b"[SublimeApp]\nName=Widget Studio\nExec=/bin/studio\nCategory=Developer\nTerminal=false\nComment=Ring-3 ComCtl32 & UxTheme Demo App\n"),
    );
    applications.lock().add_child(
        "Sublime Paint.app",
        RamInode::new_file(b"[SublimeApp]\nName=Sublime Paint\nExec=/bin/paint\nCategory=Graphics\nTerminal=false\nComment=Ring-3 Drawing & BMP Wallpaper Creator\n"),
    );
    applications.lock().add_child(
        "App Center.app",
        RamInode::new_file(b"[SublimeApp]\nName=App Center\nExec=/bin/appcenter\nCategory=System\nTerminal=false\nComment=Discover, Inspect & Launch User Apps\n"),
    );
    applications.lock().add_child(
        "Media Player.app",
        RamInode::new_file(b"[SublimeApp]\nName=Media Player\nExec=/bin/player\nCategory=Multimedia\nTerminal=false\nComment=Play WAV, MP3, QOA, AVI Video & PNG/JPEG/QOI Images\n"),
    );
    applications.lock().add_child(
        "Music Player.app",
        RamInode::new_file(b"[SublimeApp]\nName=Music Player\nExec=/bin/music\nCategory=Multimedia\nTerminal=false\nComment=Music Library, Metadata, Spectrum Visualizer & Audio Player\n"),
    );
    applications.lock().add_child(
        "Code Editor.app",
        RamInode::new_file(b"[SublimeApp]\nName=Code Editor\nExec=/bin/code\nCategory=Developer\nTerminal=false\nComment=Multi-Tab Syntax-Highlighted Code Editor\n"),
    );

    // /Users/sublime/
    let user_sublime = RamInode::new_dir();
    user_sublime.lock().add_child("Desktop", RamInode::new_dir());
    let docs_dir = RamInode::new_dir();
    docs_dir.lock().add_child(
        "Welcome.txt",
        RamInode::new_file(b"Welcome to SublimeOS 1.0!\nModern Desktop Workstation running on KiteKernel x86_64.\n"),
    );
    docs_dir.lock().add_child(
        "hello.rs",
        RamInode::new_file(b"// SublimeOS Ring-3 Rust Sample\nfn greet(target: &str) -> u32 {\n    let count = 42;\n    println!(\"Hello {}! count={}\", target, count);\n    count\n}\n"),
    );
    docs_dir.lock().add_child(
        "config.json",
        RamInode::new_file(b"{\n  \"os\": \"SublimeOS\",\n  \"version\": 1,\n  \"audio\": \"Intel HDA\",\n  \"codecs\": [\"PNG\", \"JPEG\", \"QOI\", \"WAV\", \"QOA\", \"MP3\", \"AVI\"]\n}\n"),
    );
    docs_dir.lock().add_child(
        "hello.sh",
        RamInode::new_file(b"# SublimeOS Sample Script\necho Running SublimeOS Multimedia & Code Stack\nuname\n"),
    );
    user_sublime.lock().add_child("Documents", docs_dir);
    let pics_dir = RamInode::new_dir();
    pics_dir.lock().add_child(
        "SublimePrism.bmp",
        RamInode::new_file(&crate::gui::wallpaper::generate_sample_bmp(160, 90, 0)),
    );
    pics_dir.lock().add_child(
        "AuroraWave.bmp",
        RamInode::new_file(&crate::gui::wallpaper::generate_sample_bmp(160, 90, 1)),
    );
    pics_dir.lock().add_child(
        "SunsetGrid.bmp",
        RamInode::new_file(&crate::gui::wallpaper::generate_sample_bmp(160, 90, 2)),
    );
    pics_dir.lock().add_child(
        "SublimeLogo.png",
        RamInode::new_file(&crate::gui::wallpaper::generate_sample_png(128, 72, 0)),
    );
    pics_dir.lock().add_child(
        "PrismPhoto.jpg",
        RamInode::new_file(&crate::gui::wallpaper::generate_sample_jpeg(128, 72, 2)),
    );
    pics_dir.lock().add_child(
        "CosmicTile.qoi",
        RamInode::new_file(&crate::gui::wallpaper::generate_sample_qoi(128, 72, 1)),
    );
    user_sublime.lock().add_child("Pictures", pics_dir);
    let music_dir = RamInode::new_dir();
    music_dir.lock().add_child(
        "SublimeChime.wav",
        RamInode::new_file(&crate::gui::wallpaper::generate_sample_wav()),
    );
    music_dir.lock().add_child(
        "SynthChord.qoa",
        RamInode::new_file(&crate::gui::wallpaper::generate_sample_qoa()),
    );
    music_dir.lock().add_child(
        "NebulaDrift.wav",
        RamInode::new_file(&crate::gui::wallpaper::generate_sample_adpcm_track(
            "Nebula Drift",
            "Sublime Synth",
            "Cosmos OST",
            &[220, 261, 329, 293],
        )),
    );
    music_dir.lock().add_child(
        "SolarWind.wav",
        RamInode::new_file(&crate::gui::wallpaper::generate_sample_adpcm_track(
            "Solar Wind",
            "Kite Lab",
            "Cosmos OST",
            &[196, 246, 293, 392],
        )),
    );
    music_dir.lock().add_child(
        "StarlightArp.qoa",
        RamInode::new_file(&crate::gui::wallpaper::generate_sample_qoa_arp()),
    );
    user_sublime.lock().add_child("Music", music_dir);
    let videos_dir = RamInode::new_dir();
    videos_dir.lock().add_child(
        "SublimeDemo.avi",
        RamInode::new_file(&crate::gui::wallpaper::generate_sample_avi()),
    );
    user_sublime.lock().add_child("Videos", videos_dir);
    user_sublime.lock().add_child("Downloads", RamInode::new_dir());
    let trash_dir = RamInode::new_dir();
    let sublimerc = b"# SublimeOS shell configuration\nexport PATH=\"/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin\"\nalias ll=\"list\"\n";
    user_sublime.lock().add_child(".sublimerc", RamInode::new_file(sublimerc));
    user_sublime.lock().add_child(".zshrc", RamInode::new_file(sublimerc));
    users.lock().add_child("sublime", user_sublime.clone());
    users.lock().add_child("cosmos", user_sublime);

    // /Volumes/
    let cosmic_hd = RamInode::new_dir();
    cosmic_hd.lock().add_child(".sublime_container", RamInode::new_file(b"SublimeOS Container Version 1.0\n"));
    volumes.lock().add_child("Sublime HD", cosmic_hd);
    volumes.lock().add_child("Sublime USB", RamInode::new_dir());

    // /etc/hosts & /etc/motd
    let motd_content = b"\n=======================================================\n  Welcome to SublimeOS (KiteKernel 1.0.0 x86_64)\n  Copyright (c) 2026 Nader Mahbub Khan\n=======================================================\nType 'system' or 'help' for system status & commands.\n\n";
    etc.lock().add_child("motd", RamInode::new_file(motd_content));
    etc.lock().add_child("hosts", RamInode::new_file(b"127.0.0.1\tlocalhost\n255.255.255.255\tbroadcast\n::1\t\tlocalhost\n"));

    root.lock().add_child("System", system);
    root.lock().add_child("Library", library);
    root.lock().add_child("Applications", applications);
    root.lock().add_child("Users", users);
    root.lock().add_child("Volumes", volumes);
    root.lock().add_child("Trash", trash_dir);
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

    kinfo!("VFS initialized: SublimeOS hierarchy (/System, /Library, /Applications, /Users, /Volumes)");
}

/// Automatically detect and mount physical storage block devices (FAT32, exFAT, ext4, NTFS).
pub fn mount_storage_devices() {
    mount_all_storage();
}
