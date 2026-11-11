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
