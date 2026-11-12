# SublimeOS Automated Verification Suite & Testing Guide

SublimeOS contains a comprehensive, in-kernel automated self-test verification suite ([`src/tests/mod.rs`](../src/tests/mod.rs)) executed during kernel boot (or on demand via the `test` shell command).

The suite verifies low-level architectural invariants, hardware drivers, memory management, filesystem integrity, network protocols, and userspace isolation with **52 tests** passing across both UEFI and legacy BIOS boots.

---

## 1. Test Profiles & Environments

Testing is conducted against multiple hardware configurations using both QEMU simulation profiles and bare-metal hardware:

| Profile | Firmware | Machine | Cores | Memory | Devices Under Test |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **`uefi_ovmf`** | OVMF EDK2 x86_64 | Q35 | 4 SMP | 256 MiB | Native `BOOTX64.EFI`, GOP Framebuffer Console, ACPI XSDT, AHCI, EHCI USB HID |
| **`pc_1core`** | SeaBIOS | i440FX (pc) | 1 Core | 256 MiB | Multiboot1, PIC/PIT, PMM/VMM, RamFS, e1000, Ring 3 Userspace, Signals |
| **`pc_smp4`** | SeaBIOS | i440FX (pc) | 4 SMP | 256 MiB | Multi-Core INIT-SIPI-SIPI AP trampoline (`0x8000`), LAPIC MMIO Timer, Work-Stealing |
| **`q35_hda_smp4`** | SeaBIOS | Q35 (PCIe) | 4 SMP | 256 MiB | PCIe topology, Intel HDA Audio DMA, AHCI SATA, NVMe controller, Broadcom tg3/b43 |
| **`bare_metal`** | Apple EFI | MacBookPro8,2 | 4C/8T i7 | 8–16 GiB | Late 2011 MacBook Pro: BCM57765, BCM4331 MIMO, Apple Trackpad, SMC `/dev/backlight` |

---

## 2. In-Kernel Self-Test Log (53/53 PASS)

```text
=======================================================
             KiteKernel Verification Suite           
=======================================================
  [TEST] Physical Frame Allocator ... [PASS]
  [TEST] Dynamic Heap & Alloc Crate ... [PASS]
  [TEST] Virtual File System & Mounts ... [PASS]
  [TEST] Anonymous IPC Pipes ... [PASS]
  [TEST] Task Context Switch & Yield ... [PASS]
  [TEST] 16550A UART & /dev/ttyS0 ... [PASS]
  [TEST] FAT32 Read/Write & Cluster Alloc ... [INFO] FAT32 Filesystem Mounted ... [PASS]
  [TEST] exFAT Parser & Inode Traversal ... [INFO] exFAT Filesystem Mounted ... [PASS]
  [TEST] ext4 Superblock & Inode Mapping ... [INFO] ext4 Filesystem Mounted ... [PASS]
  [TEST] NTFS Resident $DATA & Index Root ... [INFO] NTFS Filesystem Mounted ... [PASS]
  [TEST] USB Mass Storage (SCSI / BOT) ... [PASS]
  [TEST] Network Crypto (SHA-1, HMAC-SHA1, PBKDF2) ... [PASS]
  [TEST] IEEE 802.11 Wi-Fi (Beacons & WPA2-PSK) ... [INFO] 4-Way Handshake ... [PASS]
  [TEST] TCP/IP Protocol Stack & ICMP Ping ... [PASS]
  [TEST] TCP Protocol & Socket Engine ... [PASS]
  [TEST] HTTP Client & Embedded Server ... [PASS]
  [TEST] Audio Subsystem & PCM Synthesis ... [PASS]
  [TEST] ELF64 Loader & Ring 3 User Mode ... [INFO] Executing in Ring 3 ... [PASS]
  [TEST] POSIX Pipelines, Redirection & Editor ... [PASS]
  [TEST] Userspace Libc & Native Binaries (/bin) ... [INFO] Executing in Ring 3 ... [PASS]
  [TEST] Process Fork & Copy-on-Write (CoW) ... [PASS]
  [TEST] Dynamic Linker & Shared Libs (ld-cosmic.so) ... [INFO] Dynamic Linking Verified ... [PASS]
  [TEST] POSIX Signals & Process Control ... [PASS]
  [TEST] SMP Multi-Core, ACPI MADT & APIC ... [PASS]
  [TEST] Storage Drivers (AHCI SATA & NVMe SSD) ... [INFO] Registered Block Device ... [PASS]
  [TEST] Userspace Aqua WindowServer, /dev/fb0 & libgui ... [INFO] Aqua Window Server Online ... [PASS]
  [TEST] Multi-Core Scheduler, Work-Stealing & Core Affinity ... [PASS]
  [TEST] Partition Table Auto-Mounter (GPT / MBR into /Volumes) ... [INFO] Auto-Mounted ... [PASS]
  [TEST] Network Socket Server Daemons (telnetd & sshd) ... [PASS]
  [TEST] Pseudo-Terminal (PTY) Subsystem & Line Discipline ... [PASS]
  [TEST] TLS 1.3 Cryptography & Secure HTTPS Engine ... [PASS]
  [TEST] Realtek RTL8139/8169 & Qualcomm Atheros Drivers ... [PASS]
  [TEST] POSIX Job Control, Process Groups & Sessions ... [PASS]
  [TEST] USB HID Keyboard & Mouse Input Subsystem ... [PASS]
  [TEST] mDNS / Bonjour ZeroConf Responder (SublimeOS.local & DNS-SD) ... [PASS]
  [TEST] POSIX Shared Memory (shm_open, ftruncate, /dev/shm, mmap MAP_SHARED) ... [PASS]
  [TEST] POSIX Semaphores & Message Queues (sem_*, mq_*) ... [PASS]
  [TEST] DHCP Client Daemon (RFC 2131/2132 Discovery, Offer, Request, Lease) ... [PASS]
  [TEST] NTFS Non-Resident Data Runlist Decoder & Fragmented Extents ... [PASS]
  [TEST] ext4 Directory HTree (B-Tree) Indexing ... [PASS]
  [TEST] NTP Network Time Synchronization Daemon ... [PASS]
  [TEST] Unix Domain Sockets (AF_UNIX IPC & Streams) ... [PASS]
  [TEST] Kernel POSIX Threads (sys_clone) & Futexes ... [PASS]
  [TEST] Demand Paging (#PF Lazy Allocation) & Swap Subsystem ... [PASS]
  [TEST] DNS Client Cache with TTL & LLMNR (RFC 4795) ... [PASS]
  [TEST] E820 Memory Map Scrubbing & Reserved Region Guard ... [PASS]
  [TEST] 16-Bit AP Trampoline (0x8000) & LAPIC MMIO Timer ... [PASS]
  [TEST] Bare-Metal ACPI XSDT/MADT, PCI Display & Storage Guards ... [PASS]
  [TEST] Broadcom BCM57765 (tg3) & BCM4331 (b43/BCMA) Drivers ... [INFO] BCM4331 Online ... [PASS]
  [TEST] EHCI Periodic DMA Pipes, Apple Trackpad & SMC Backlight ... [PASS]
  [TEST] USB 2.0 Hub Split-Transactions & Early GOP Console ... [PASS]
  [TEST] Unified Storage Block Cache (Buffer Cache & Sync) ... [PASS]
  [TEST] USB 2.0 Hub Downstream Enumeration & Topology ... [PASS]
=======================================================
      All Kernel Self-Tests PASSED Successfully!       
=======================================================
```

---

## 3. Subsystem Test Details

### Memory Management & Paging (Tests 1, 2, 21, 36, 43, 44, 46)
- **PMM & Frame Allocator**: Allocates/frees single frames, contiguous multi-frame spans, and verifies bounds clamping.
- **Dynamic Heap**: Verifies kernel heap expansions with `alloc` structures (`Vec`, `String`, `Box`, `Arc`).
- **Copy-on-Write (`fork`)**: Allocates user memory, writes known magic values, clones the address space marking PTEs with `PAGE_COW` (bit 9), triggers a write fault, and confirms the `#PF` handler transparently allocates a private physical frame.
- **Demand Paging & Swap**: Tests unmapped access interception, lazy 4 KiB allocation, TLB invalidation (`invlpg`), and swap slot serialization with `PAGE_SWAPPED` (bit 10).
- **E820 Scrubbing**: Validates that memory holes, ACPI reclaim regions, and hardware MMIO regions are never marked as available conventional RAM.

### Multiprocessing & Interrupts (Tests 5, 23, 24, 27, 47, 48)
- **APIC & SMP**: Validates AP core INIT-SIPI-SIPI wakeup sequence, inter-processor interrupts (IPI), and per-core scheduler queues.
- **Legacy PIC Disabling**: Verifies total masking of ports `0x21` and `0xA1` (`0xFF`) and routes all IRQs exclusively through the I/O APIC.
- **Local APIC EOI**: Confirms hardware interrupt dispatches write the EOI token (`0x00`) to APIC MMIO register `0x0B0`.

### Storage & Filesystems (Tests 3, 7, 8, 9, 10, 11, 25, 28, 39, 40, 52)
- **Real Filesystem Decoders**: Exercises FAT32 cluster allocation, exFAT directory indexing, ext4 HTree B-Tree lookups, and NTFS multi-extent non-resident runlist decoding (`decode_runlist`).
- **Storage Controllers**: Validates AHCI SATA Command List / PRDT DMA tables and NVMe Admin / I/O circular rings.
- **Unified Storage Block Cache (Test 52)**: Exercises the 1024-sector (512 KiB) LRU buffer cache (`bcache.rs`), verifying sector cache hits, dirty tracking, cache write-through/write-back persistence, and atomic cache flushing via `sys_sync` (Syscall 162) and `sys_fsync` (Syscall 74).

### Userspace Toolchain & Runtime (Tests 18, 20, 22)
- **Standalone Userland Binaries (`/bin`)**: Executes genuine `#![no_std]` compiled 64-bit ELF binaries (`/bin/init`, `/bin/calc`, `/bin/hello`, `/bin/echo`, `/bin/cat`, `/bin/ls`, `/bin/grep`, `/bin/wc`, `/bin/edit`, `/bin/sh`, `/bin/snake`) built with the isolated `userland/` crate and `sublime_rt` runtime targeting `x86_64-unknown-linux-none` linked with `user.ld`.
- **System V AMD64 ABI Initial User Stack Setup & 16-Byte Alignment**: Kernel ELF loader initializes the initial user stack with `argc`, `argv[0]..argv[N]`, `NULL` argv sentinel, `NULL` envp sentinel, and string buffer data. Ensures `user_rsp % 16 == 8` so that entry `pop rdi` leaves `rsp` 16-byte aligned before function calls, ensuring zero `#GP(0)` faults on 16-byte aligned SSE vector instructions (`movaps`).
- **Linux-Compatible Directory Enumeration (`SYS_GETDENTS64`)**: Implements Syscall 217 (`sys_getdents64`) packing 64-bit `LinuxDirent64Hdr` records (`d_ino`, `d_off`, `d_reclen`, `d_type`, `d_name[]`) from `InodeOperations::readdir()` directly into userland memory.
- **Dynamic File Creation & Truncation (`O_CREAT` / `O_TRUNC`)**: `sys_open` supports POSIX `O_CREAT` (0x40) file creation within directory inodes and `O_TRUNC` (0x200) content truncation.
- **Master Boot Sequence (PID 1 Init & Ring 3 Shell)**: Following self-tests, the kernel transitions directly into userspace by executing `/bin/init` as **PID 1 in Ring 3** (CPL 3). PID 1 initializes userspace supervision and spawns `/bin/sh` (PID 2) in Ring 3, achieving full hardware privilege isolation.
- **Hardware SIMD & Syscall ABI**: Validates AMD64 SSE/SIMD execution (`movups`, `movaps` with CR0.MP, CR4.OSFXSR, CR4.OSXMMEXCPT enabled) and full register preservation (`rdi`, `rsi`, `rdx`, `r10`, `r8`, `r9`, `r12..r15`, `rbx`, `rbp`) across `syscall_entry.s` and `sysretq`.
- **Nested Execution Frame Stack**: Implements a 16-level saved frame stack in `user_switch.s`, allowing nested user process execution (`init` -> `sh` -> `calc`) without parent register or CR3 corruption.
- **Blocking Console Input**: `sys_read(0, ...)` yields the CPU cleanly when no input is ready and awakens immediately upon keyboard or serial interrupt.

### Hardware Drivers & Networking (Tests 12, 13, 14, 15, 16, 30, 31, 32, 34, 38, 41, 49, 50, 51, 53)
- **Broadcom Silicon**: Exercises Tigon3 (`tg3`) Status Block ring synchronization and BCM4331 (`b43`) PSM SHM microcode upload with 64-bit DMA descriptor setup.
- **USB 2.0 EHCI & Apple Hardware**: Tests EHCI Periodic Frame List traversal, Apple Trackpad packet decoding, and SMC `/dev/backlight` registers.
- **GOP Console**: Validates that multi-byte UTF-8 box characters (`██╔═╗`) decode into single-cell coordinates without column-wrapping bugs.
- **USB 2.0 Hub Class Driver & Downstream Enumeration (Test 53)**: Validates multi-port USB Hub descriptor parsing with `DeviceRemovable` bitmasks, power-on-to-power-good stabilization delay, port reset debounce and 10 ms recovery (`TRSTRCY`), multi-tier Split Transaction Translator (TT) inheritance, depth recursion bounds, automatic physical Queue Head programming (Mult=1, C-Mask=0x1C, S-Mask=0x01, TT HubAddr/PortNum), and device tree topology inspection via `system usb` and `lsusb`.

---

## 4. Running the Tests

### From Within SublimeOS Shell
```sh
cosmos@SublimeOS / % test
```

### From Host Automated Python Harness
```powershell
python scratch/qemu_verify_v2.py
```
Outputs individual test profile results and asserts `ALL PROFILES PASSED: True`.
