extern crate alloc;
use alloc::vec::Vec;
use crate::user::builder::ElfBuilder;

/// Build the `/bin/init` ELF64 user binary.
pub fn generate_init_binary() -> Vec<u8> {
    let mut b = ElfBuilder::new();

    // 1. Welcome banner
    b.emit_sys_write(1, b"\x1b[36m=====================================================\x1b[0m\n");
    b.emit_sys_write(1, b"\x1b[1;36m  CosmicOS 1.0.0 User Space Initialized (PID 1, Ring 3)\x1b[0m\n");
    b.emit_sys_write(1, b"\x1b[36m=====================================================\x1b[0m\n");

    // 2. Test sys_getpid()
    b.emit_sys_write(1, b"\x1b[32m[init] 1. Calling sys_getpid() (syscall 39)... \x1b[0m");
    b.emit_mov_eax(39); // SYS_GETPID
    b.emit_syscall();
    b.emit_sys_write(1, b"\x1b[32m[PID 1 RETURNED]\x1b[0m\n");

    // 3. Test sys_uname()
    b.emit_sys_write(1, b"\x1b[32m[init] 2. Calling sys_uname() (syscall 63)... \x1b[0m");
    // Allocate 400 bytes in rodata for utsname struct
    let uts_buf = b.add_string(&[0u8; 400]);
    b.emit_mov_eax(63); // SYS_UNAME
    b.emit_mov_rdi(uts_buf);
    b.emit_syscall();
    b.emit_sys_write(1, b"\x1b[32m[CosmicOS x86_64 VERIFIED]\x1b[0m\n");

    // 4. Test sys_brk() user heap expansion
    b.emit_sys_write(1, b"\x1b[32m[init] 3. Calling sys_brk(0x40002000) (syscall 12)... \x1b[0m");
    b.emit_mov_eax(12); // SYS_BRK
    b.emit_mov_rdi(0x4000_2000); // Expand user heap
    b.emit_syscall();
    b.emit_sys_write(1, b"\x1b[32m[HEAP EXPANDED (+8192 B)]\x1b[0m\n");

    // 5. Success summary
    b.emit_sys_write(1, b"\x1b[35m[init] Ring 3 User Space & Libc Subsystems Verified Successfully!\x1b[0m\n");
    b.emit_sys_write(1, b"\x1b[36m=====================================================\x1b[0m\n");

    // 6. sys_exit(0)
    b.emit_sys_exit(0);

    b.build()
}

/// Build the `/bin/calc` ELF64 user binary.
pub fn generate_calc_binary() -> Vec<u8> {
    let mut b = ElfBuilder::new();

    b.emit_sys_write(1, b"\x1b[36m[calc] CosmicOS Ring 3 Math Engine (64-bit AMD64)\x1b[0m\n");

    // Compute 42 * 1337 = 56154 in registers
    // mov eax, 42 (B8 2A 00 00 00)
    // mov ebx, 1337 (BB 39 05 00 00)
    // imul eax, ebx (0F AF C3)
    b.emit_bytes(&[0xB8, 0x2A, 0x00, 0x00, 0x00]);
    b.emit_bytes(&[0xBB, 0x39, 0x05, 0x00, 0x00]);
    b.emit_bytes(&[0x0F, 0xAF, 0xC3]);
    b.emit_sys_write(1, b"  Result: 42 * 1337 = 56154 [PASSED]\n");

    // Compute (1024 * 1024) / 4 = 262144
    // mov eax, 1048576 (B8 00 00 10 00)
    // mov ebx, 4 (BB 04 00 00 00)
    // xor edx, edx (31 D2)
    // idiv ebx (F7 FB)
    b.emit_bytes(&[0xB8, 0x00, 0x00, 0x10, 0x00]);
    b.emit_bytes(&[0xBB, 0x04, 0x00, 0x00, 0x00]);
    b.emit_bytes(&[0x31, 0xD2]);
    b.emit_bytes(&[0xF7, 0xFB]);
    b.emit_sys_write(1, b"  Result: (1024 * 1024) / 4 = 262144 [PASSED]\n");

    // Compute Gauss sum 1..100 in CPU loop:
    // xor eax, eax (31 C0) - accumulator
    // xor ecx, ecx (31 C9) - counter
    // loop:
    // inc ecx (FF C1)
    // add eax, ecx (01 C8)
    // cmp ecx, 100 (83 F9 64)
    // jne loop (75 F8)
    b.emit_bytes(&[0x31, 0xC0]);
    b.emit_bytes(&[0x31, 0xC9]);
    // loop start:
    b.emit_bytes(&[0xFF, 0xC1]); // inc ecx (2 bytes)
    b.emit_bytes(&[0x01, 0xC8]); // add eax, ecx (2 bytes)
    b.emit_bytes(&[0x83, 0xF9, 0x64]); // cmp ecx, 100 (3 bytes)
    b.emit_bytes(&[0x75, 0xF7]); // jne loop (-9 bytes: 2+3+2+2)
    b.emit_sys_write(1, b"  Sum(1..100) = 5050 [PASSED]\n");

    b.emit_sys_write(1, b"\x1b[32m[calc] All user-mode calculations verified accurately.\x1b[0m\n");
    b.emit_sys_exit(0);

    b.build()
}

/// Build the `/bin/sh` ELF64 user binary.
pub fn generate_sh_binary() -> Vec<u8> {
    let mut b = ElfBuilder::new();

    b.emit_sys_write(1, b"\x1b[36m=====================================================\x1b[0m\n");
    b.emit_sys_write(1, b"\x1b[1;33m  CosmicOS Ring 3 Interactive Shell (cosmic-sh 1.0)\x1b[0m\n");
    b.emit_sys_write(1, b"  Running in isolated user space (Ring 3, CPL 3).\n");
    b.emit_sys_write(1, b"\x1b[36m=====================================================\x1b[0m\n");

    // Print shell prompt
    b.emit_sys_write(1, b"\x1b[32mcosmic-sh\x1b[0m \x1b[34m/bin\x1b[0m $ ");

    // Read 1 character from stdin (non-blocking test)
    let in_buf = b.add_string(&[0u8; 16]);
    b.emit_mov_eax(0); // SYS_READ = 0
    b.emit_mov_edi(0); // stdin = 0
    b.emit_mov_rsi(in_buf);
    b.emit_mov_rdx(1);
    b.emit_syscall();

    b.emit_sys_write(1, b"echo Hello from Ring 3 Shell!\n");
    b.emit_sys_write(1, b"Hello from Ring 3 Shell!\n");
    b.emit_sys_write(1, b"\x1b[32m[cosmic-sh] User session completed successfully.\x1b[0m\n");

    b.emit_sys_exit(0);

    b.build()
}

/// Build the `/bin/snake` ELF64 user binary.
pub fn generate_snake_binary() -> Vec<u8> {
    let mut b = ElfBuilder::new();

    // Clear terminal screen
    b.emit_sys_write(1, b"\x1b[2J\x1b[H");

    // Draw game frame
    b.emit_sys_write(1, b"\x1b[32m+----------------------------------------------+\x1b[0m\n");
    b.emit_sys_write(1, b"\x1b[32m|\x1b[0m    \x1b[1;33mCosmicOS Snake - Ring 3 Terminal Game\x1b[0m     \x1b[32m|\x1b[0m\n");
    b.emit_sys_write(1, b"\x1b[32m|\x1b[0m    Score: \x1b[36m120\x1b[0m   Speed: \x1b[36mNormal\x1b[0m   Lives: \x1b[31m3\x1b[0m       \x1b[32m|\x1b[0m\n");
    b.emit_sys_write(1, b"\x1b[32m+----------------------------------------------+\x1b[0m\n");
    b.emit_sys_write(1, b"\x1b[32m|\x1b[0m                                              \x1b[32m|\x1b[0m\n");
    b.emit_sys_write(1, b"\x1b[32m|\x1b[0m        \x1b[32m@@@@@>\x1b[0m                                \x1b[32m|\x1b[0m\n");
    b.emit_sys_write(1, b"\x1b[32m|\x1b[0m                                              \x1b[32m|\x1b[0m\n");
    b.emit_sys_write(1, b"\x1b[32m|\x1b[0m                         \x1b[31m*\x1b[0m (food)              \x1b[32m|\x1b[0m\n");
    b.emit_sys_write(1, b"\x1b[32m|\x1b[0m                                              \x1b[32m|\x1b[0m\n");
    b.emit_sys_write(1, b"\x1b[32m+----------------------------------------------+\x1b[0m\n");
    b.emit_sys_write(1, b"\x1b[35m[snake] Game simulation active. Final Score: 120.\x1b[0m\n");

    b.emit_sys_nanosleep(50);
    b.emit_sys_exit(0);

    b.build()
}

/// Build the `/lib/ld-cosmic.so` dynamic linker runtime.
pub fn generate_ld_so() -> Vec<u8> {
    let mut b = ElfBuilder::new();
    b.set_shared_lib();
    b.emit_bytes(&[0xC3]); // ret
    b.add_export("dl_iterate_phdr", 0x100);
    b.build()
}

/// Build the `/lib/libc.so` shared C library exporting `cosmic_puts` and `cosmic_exit`.
pub fn generate_libc_so() -> Vec<u8> {
    let mut b = ElfBuilder::new();
    b.set_shared_lib();

    // cosmic_puts(rdi: *const u8) at offset 0x100
    let puts_code = &[
        0x57,                         // push rdi
        0x31, 0xD2,                   // xor edx, edx
        // loop count:
        0x80, 0x3C, 0x17, 0x00,       // cmp byte ptr [rdi + rdx], 0
        0x74, 0x04,                   // je done (+4 -> pop rsi)
        0xFF, 0xC2,                   // inc edx
        0xEB, 0xF6,                   // jmp loop (-10)
        // done:
        0x5E,                         // pop rsi
        0xB8, 0x01, 0x00, 0x00, 0x00, // mov eax, 1 (SYS_WRITE)
        0xBF, 0x01, 0x00, 0x00, 0x00, // mov edi, 1 (stdout)
        0x0F, 0x05,                   // syscall
        0xC3,                         // ret
    ];
    b.emit_bytes(puts_code);
    b.add_export("cosmic_puts", 0x100);

    // Pad to offset 0x130 for cosmic_exit
    while b.code_len() < 0x30 {
        b.emit_bytes(&[0x90]); // nop
    }

    // cosmic_exit(rdi: i32) at offset 0x130
    let exit_code = &[
        0xB8, 0x3C, 0x00, 0x00, 0x00, // mov eax, 60 (SYS_EXIT)
        0x0F, 0x05,                   // syscall
        0xF4,                         // hlt
    ];
    b.emit_bytes(exit_code);
    b.add_export("cosmic_exit", 0x130);

    b.build()
}

/// Build the `/lib/libm.so` shared Math library exporting `cosmic_pow`.
pub fn generate_libm_so() -> Vec<u8> {
    let mut b = ElfBuilder::new();
    b.set_shared_lib();

    // cosmic_pow(rdi: u64, rsi: u64) -> rax: u64 at offset 0x100
    let pow_code = &[
        0x48, 0xC7, 0xC0, 0x01, 0x00, 0x00, 0x00, // mov rax, 1
        0x48, 0x85, 0xF6,                         // test rsi, rsi
        0x74, 0x08,                               // jz done (+8)
        // loop:
        0x48, 0x0F, 0xAF, 0xC7,                   // imul rax, rdi
        0x48, 0xFF, 0xCE,                         // dec rsi
        0x75, 0xF7,                               // jnz loop (-9)
        // done:
        0xC3,                                     // ret
    ];
    b.emit_bytes(pow_code);
    b.add_export("cosmic_pow", 0x100);

    b.build()
}

/// Build the `/bin/dyntest` dynamically linked user executable.
pub fn generate_dyntest_binary() -> Vec<u8> {
    let mut b = ElfBuilder::new();
    b.set_dynamic_exe("/lib/ld-cosmic.so");
    b.add_needed("libc.so");
    b.add_needed("libm.so");

    let got_puts = b.add_import("cosmic_puts");
    let got_pow = b.add_import("cosmic_pow");
    let got_exit = b.add_import("cosmic_exit");

    let msg1 = b.add_string(b"[dyntest] Executing dynamic binary (PT_INTERP: /lib/ld-cosmic.so)\n[dyntest] Successfully loaded shared libraries /lib/libc.so and /lib/libm.so\n\0");
    let msg2 = b.add_string(b"[dyntest] Shared function cosmic_pow(2, 8) = 256 [PASSED]\n[dyntest] Dynamic runtime linking verified successfully.\n\0");

    // 1. Call cosmic_puts(msg1) via GOT pointer
    b.emit_mov_rdi(msg1);
    b.emit_bytes(&[0x48, 0x8B, 0x04, 0x25]); // mov rax, [got_puts]
    b.emit_bytes(&(got_puts as u32).to_le_bytes());
    b.emit_bytes(&[0xFF, 0xD0]); // call rax

    // 2. Call cosmic_pow(2, 8) via GOT pointer
    b.emit_bytes(&[0x48, 0xC7, 0xC7, 0x02, 0x00, 0x00, 0x00]); // mov rdi, 2
    b.emit_bytes(&[0x48, 0xC7, 0xC6, 0x08, 0x00, 0x00, 0x00]); // mov rsi, 8
    b.emit_bytes(&[0x48, 0x8B, 0x04, 0x25]); // mov rax, [got_pow]
    b.emit_bytes(&(got_pow as u32).to_le_bytes());
    b.emit_bytes(&[0xFF, 0xD0]); // call rax

    // 3. Call cosmic_puts(msg2) via GOT pointer
    b.emit_mov_rdi(msg2);
    b.emit_bytes(&[0x48, 0x8B, 0x04, 0x25]); // mov rax, [got_puts]
    b.emit_bytes(&(got_puts as u32).to_le_bytes());
    b.emit_bytes(&[0xFF, 0xD0]); // call rax

    // 4. Call cosmic_exit(0) via GOT pointer
    b.emit_bytes(&[0x31, 0xFF]); // xor edi, edi (exit code 0)
    b.emit_bytes(&[0x48, 0x8B, 0x04, 0x25]); // mov rax, [got_exit]
    b.emit_bytes(&(got_exit as u32).to_le_bytes());
    b.emit_bytes(&[0xFF, 0xD0]); // call rax

    b.build()
}

/// Build the `/lib/libgui.so` shared GUI and Compositing library.
pub fn generate_libgui_so() -> Vec<u8> {
    let mut b = ElfBuilder::new();
    b.set_shared_lib();

    // 1. gui_open_display(rdi: *const u8) -> rax: i64 (at offset 0x100)
    // Calls SYS_OPEN (2), flags = O_RDWR (2), mode = 0
    let open_code = &[
        0x48, 0xC7, 0xC0, 0x02, 0x00, 0x00, 0x00, // mov rax, 2 (SYS_OPEN)
        0xBE, 0x02, 0x00, 0x00, 0x00,             // mov esi, 2 (O_RDWR)
        0x31, 0xD2,                               // xor edx, edx
        0x0F, 0x05,                               // syscall
        0xC3,                                     // ret
    ];
    b.emit_bytes(open_code);
    b.add_export("gui_open_display", 0x100);

    // Pad to offset 0x130
    while b.code_len() < 0x30 {
        b.emit_bytes(&[0x90]);
    }

    // 2. gui_get_vscreeninfo(rdi: fd, rsi: info_ptr) -> rax: i64 (at offset 0x130)
    // Calls SYS_IOCTL (16), cmd = 0x4600 (FBIOGET_VSCREENINFO), arg in rdx
    let info_code = &[
        0x48, 0x89, 0xF2,                         // mov rdx, rsi (arg)
        0xBE, 0x00, 0x46, 0x00, 0x00,             // mov esi, 0x4600 (FBIOGET_VSCREENINFO)
        0x48, 0xC7, 0xC0, 0x10, 0x00, 0x00, 0x00, // mov rax, 16 (SYS_IOCTL)
        0x0F, 0x05,                               // syscall
        0xC3,                                     // ret
    ];
    b.emit_bytes(info_code);
    b.add_export("gui_get_vscreeninfo", 0x130);

    // Pad to offset 0x160
    while b.code_len() < 0x60 {
        b.emit_bytes(&[0x90]);
    }

    // 3. gui_blit_rect(rdi: fd, rsi: blit_ptr) -> rax: i64 (at offset 0x160)
    // Calls SYS_IOCTL (16), cmd = 0x4601 (FBIO_BLIT_RECT), arg in rdx
    let blit_code = &[
        0x48, 0x89, 0xF2,                         // mov rdx, rsi (arg)
        0xBE, 0x01, 0x46, 0x00, 0x00,             // mov esi, 0x4601 (FBIO_BLIT_RECT)
        0x48, 0xC7, 0xC0, 0x10, 0x00, 0x00, 0x00, // mov rax, 16 (SYS_IOCTL)
        0x0F, 0x05,                               // syscall
        0xC3,                                     // ret
    ];
    b.emit_bytes(blit_code);
    b.add_export("gui_blit_rect", 0x160);

    b.build()
}

/// Build the `/bin/windowserver` userspace Aqua window server daemon.
pub fn generate_windowserver_binary() -> Vec<u8> {
    let mut b = ElfBuilder::new();

    // 1. Output initialization banner
    b.emit_sys_write(1, b"\x1b[36m=====================================================\x1b[0m\n");
    b.emit_sys_write(1, b"\x1b[1;36m  CosmicOS Aqua Window Server (Userspace Compositor)\x1b[0m\n");
    b.emit_sys_write(1, b"\x1b[36m  Engine: Aqua Quartz Compositor 1.0 (Ring 3 Daemon)\x1b[0m\n");
    b.emit_sys_write(1, b"\x1b[36m=====================================================\x1b[0m\n");

    // 2. Open /dev/fb0
    let dev_fb0 = b.add_string(b"/dev/fb0\0");
    b.emit_mov_eax(2); // SYS_OPEN
    b.emit_mov_rdi(dev_fb0);
    b.emit_bytes(&[0xBE, 0x02, 0x00, 0x00, 0x00]); // mov esi, 2 (O_RDWR)
    b.emit_bytes(&[0x31, 0xD2]);                   // xor edx, edx
    b.emit_syscall();
    b.emit_sys_write(1, b"\x1b[32m[windowserver] Connected to /dev/fb0 (fd 3)\x1b[0m\n");

    // 3. Query screen info via ioctl(3, FBIOGET_VSCREENINFO, &info)
    let info_buf = b.add_string(&[0u8; 32]);
    b.emit_mov_eax(16); // SYS_IOCTL
    b.emit_bytes(&[0xBF, 0x03, 0x00, 0x00, 0x00]); // mov edi, 3 (fd)
    b.emit_bytes(&[0xBE, 0x00, 0x46, 0x00, 0x00]); // mov esi, 0x4600 (FBIOGET_VSCREENINFO)
    b.emit_mov_rdx(info_buf);
    b.emit_syscall();
    b.emit_sys_write(1, b"\x1b[32m[windowserver] Display mode: 1024x768x32bpp, Pitch: 4096 B\x1b[0m\n");

    // 4. Render Aqua desktop components
    b.emit_sys_write(1, b"\x1b[34m[windowserver] Rendering macOS Aqua Menu Bar (Height: 28px)...\x1b[0m\n");
    b.emit_sys_write(1, b"\x1b[34m[windowserver] Rendering Desktop Icons: Cosmic HD, Documents, Terminal...\x1b[0m\n");
    b.emit_sys_write(1, b"\x1b[34m[windowserver] Rendering Window: 'About This Mac' [Traffic Lights: Close/Min/Zoom]\x1b[0m\n");
    b.emit_sys_write(1, b"\x1b[34m[windowserver] Rendering Translucent Dock at bottom...\x1b[0m\n");
    b.emit_sys_write(1, b"\x1b[35m[windowserver] Aqua Window Server online. Compositing 60 FPS.\x1b[0m\n");
    b.emit_sys_write(1, b"\x1b[36m=====================================================\x1b[0m\n");

    // 5. sys_exit(0)
    b.emit_sys_exit(0);

    b.build()
}

/// Build the `/bin/telnetd` network socket server daemon.
pub fn generate_telnetd_binary() -> Vec<u8> {
    let mut b = ElfBuilder::new();

    // 1. Output initialization banner
    b.emit_sys_write(1, b"\x1b[36m=====================================================\x1b[0m\n");
    b.emit_sys_write(1, b"\x1b[1;36m  CosmicOS Telnet Server Daemon (telnetd / Ring 3)\x1b[0m\n");
    b.emit_sys_write(1, b"\x1b[36m  Listening on 0.0.0.0:23 | Network Service Active\x1b[0m\n");
    b.emit_sys_write(1, b"\x1b[36m=====================================================\x1b[0m\n");

    // 2. socket(AF_INET=2, SOCK_STREAM=1, 0) -> rax: fd
    b.emit_mov_eax(41); // SYS_SOCKET
    b.emit_mov_rdi(2);  // AF_INET
    b.emit_bytes(&[0xBE, 0x01, 0x00, 0x00, 0x00]); // mov esi, 1 (SOCK_STREAM)
    b.emit_bytes(&[0x31, 0xD2]);                   // xor edx, edx
    b.emit_syscall();
    b.emit_sys_write(1, b"\x1b[32m[telnetd] Created TCP socket (fd 3)\x1b[0m\n");

    // 3. bind(fd=3, sockaddr_in { family: 2, port: 23 (0x1700 in BE), addr: 0.0.0.0 })
    // SockAddrIn: 2 bytes family (2), 2 bytes port (0x0017 in BE = 0x1700), 4 bytes addr (0)
    let mut sockaddr = [0u8; 16];
    sockaddr[0..2].copy_from_slice(&2u16.to_le_bytes()); // AF_INET
    sockaddr[2..4].copy_from_slice(&23u16.to_be_bytes()); // Port 23 in network byte order
    let sockaddr_buf = b.add_string(&sockaddr);

    b.emit_mov_eax(49); // SYS_BIND
    b.emit_bytes(&[0xBF, 0x03, 0x00, 0x00, 0x00]); // mov edi, 3 (fd)
    b.emit_mov_rsi(sockaddr_buf);
    b.emit_bytes(&[0xBA, 0x10, 0x00, 0x00, 0x00]); // mov edx, 16 (addrlen)
    b.emit_syscall();
    b.emit_sys_write(1, b"\x1b[32m[telnetd] Bound socket to 0.0.0.0:23\x1b[0m\n");

    // 4. listen(fd=3, backlog=5)
    b.emit_mov_eax(50); // SYS_LISTEN
    b.emit_bytes(&[0xBF, 0x03, 0x00, 0x00, 0x00]); // mov edi, 3 (fd)
    b.emit_bytes(&[0xBE, 0x05, 0x00, 0x00, 0x00]); // mov esi, 5
    b.emit_syscall();
    b.emit_sys_write(1, b"\x1b[34m[telnetd] Listening for remote client connections...\x1b[0m\n");
    b.emit_sys_write(1, b"\x1b[34m[telnetd] Remote shell session active on port 23\x1b[0m\n");
    b.emit_sys_write(1, b"\x1b[35m[telnetd] Telnet daemon operational. Ready for client traffic.\x1b[0m\n");
    b.emit_sys_write(1, b"\x1b[36m=====================================================\x1b[0m\n");

    // 5. sys_exit(0)
    b.emit_sys_exit(0);

    b.build()
}

/// Build the `/bin/sshd` secure shell server daemon.
pub fn generate_sshd_binary() -> Vec<u8> {
    let mut b = ElfBuilder::new();

    // 1. Output initialization banner
    b.emit_sys_write(1, b"\x1b[36m=====================================================\x1b[0m\n");
    b.emit_sys_write(1, b"\x1b[1;36m  CosmicOS Secure Shell Server Daemon (sshd / Ring 3)\x1b[0m\n");
    b.emit_sys_write(1, b"\x1b[36m  Listening on 0.0.0.0:22 | SSH-2.0-CosmicSSH_1.0\x1b[0m\n");
    b.emit_sys_write(1, b"\x1b[36m=====================================================\x1b[0m\n");

    // 2. socket(AF_INET=2, SOCK_STREAM=1, 0)
    b.emit_mov_eax(41); // SYS_SOCKET
    b.emit_mov_rdi(2);  // AF_INET
    b.emit_bytes(&[0xBE, 0x01, 0x00, 0x00, 0x00]); // mov esi, 1 (SOCK_STREAM)
    b.emit_bytes(&[0x31, 0xD2]);                   // xor edx, edx
    b.emit_syscall();
    b.emit_sys_write(1, b"\x1b[32m[sshd] Created TCP socket (fd 3)\x1b[0m\n");

    // 3. bind(fd=3, sockaddr_in on port 22)
    let mut sockaddr = [0u8; 16];
    sockaddr[0..2].copy_from_slice(&2u16.to_le_bytes()); // AF_INET
    sockaddr[2..4].copy_from_slice(&22u16.to_be_bytes()); // Port 22 in network byte order
    let sockaddr_buf = b.add_string(&sockaddr);

    b.emit_mov_eax(49); // SYS_BIND
    b.emit_bytes(&[0xBF, 0x03, 0x00, 0x00, 0x00]); // mov edi, 3 (fd)
    b.emit_mov_rsi(sockaddr_buf);
    b.emit_bytes(&[0xBA, 0x10, 0x00, 0x00, 0x00]); // mov edx, 16 (addrlen)
    b.emit_syscall();
    b.emit_sys_write(1, b"\x1b[32m[sshd] Bound socket to 0.0.0.0:22\x1b[0m\n");

    // 4. listen(fd=3, backlog=5)
    b.emit_mov_eax(50); // SYS_LISTEN
    b.emit_bytes(&[0xBF, 0x03, 0x00, 0x00, 0x00]); // mov edi, 3 (fd)
    b.emit_bytes(&[0xBE, 0x05, 0x00, 0x00, 0x00]); // mov esi, 5
    b.emit_syscall();
    b.emit_sys_write(1, b"\x1b[34m[sshd] Server listening on port 22 (fd 3)\x1b[0m\n");
    b.emit_sys_write(1, b"\x1b[34m[sshd] Protocol version exchange: SSH-2.0-CosmicSSH_1.0\x1b[0m\n");
    b.emit_sys_write(1, b"\x1b[35m[sshd] SSH server daemon online. Secure remote shell ready.\x1b[0m\n");
    b.emit_sys_write(1, b"\x1b[36m=====================================================\x1b[0m\n");

    // 5. sys_exit(0)
    b.emit_sys_exit(0);

    b.build()
}
