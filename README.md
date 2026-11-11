# CosmicOS — Sublime Kernel (x86_64)

> **A Clean, Mature, and Feature-Complete 64-bit Monolithic Operating System Kernel Written in Pure Rust.**

[![Target](https://img.shields.io/badge/target-x86__64--unknown--none-blue.svg)](https://www.rust-lang.org/)
[![Toolchain](https://img.shields.io/badge/rustc-nightly-orange.svg)](https://rust-lang.github.io/rustup/)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-green.svg)](#license)
[![Architecture](https://img.shields.io/badge/arch-x86__64%20(AMD64)-red.svg)](#1-architecture-boot--low-level-hardware)
[![Multiprocessing](https://img.shields.io/badge/smp-Multi--Core%20%7C%20APIC-blueviolet.svg)](#1-architecture-boot--low-level-hardware)
[![Storage](https://img.shields.io/badge/storage-NVMe%20%7C%20AHCI%20%7C%20USB%20%7C%20ATA-brightgreen.svg)](#5-filesystems--real-block-storage)
[![Filesystems](https://img.shields.io/badge/filesystems-FAT32%20%7C%20exFAT%20%7C%20ext4%20%7C%20NTFS-brightgreen.svg)](#5-filesystems--real-block-storage)
[![Network](https://img.shields.io/badge/network-TCP%2FIP%20%7C%20802.11%20Wi--Fi%20%7C%20HTTP-cyan.svg)](#6-networking-80211-wi-fi--sockets)
[![Audio](https://img.shields.io/badge/audio-Intel%20HDA%20%7C%20PCM%20%7C%20WAV-magenta.svg)](#7-intel-high-definition-audio--sound-subsystem)
[![UserSpace](https://img.shields.io/badge/userspace-ELF64%20%7C%20CoW%20Fork%20%7C%20ld--cosmic.so-yellow.svg)](#4-userspace-runtime-dynamic-linker--standard-library)

---

## Table of Contents
1. [Overview](#overview)
2. [Architectural Overview](#architectural-overview)
3. [Core Subsystems](#core-subsystems)
   - [1. Architecture, Boot & Low-Level Hardware](#1-architecture-boot--low-level-hardware)
   - [2. Memory Management Subsystem](#2-memory-management-subsystem)
   - [3. Task Management, Process Control & POSIX Signals](#3-task-management-process-control--posix-signals)
   - [4. Userspace Runtime, Dynamic Linker & Standard Library](#4-userspace-runtime-dynamic-linker--standard-library)
   - [5. Filesystems & Real Block Storage](#5-filesystems--real-block-storage)
   - [6. Networking, 802.11 Wi-Fi & Sockets](#6-networking-80211-wi-fi--sockets)
   - [7. Intel High Definition Audio & Sound Subsystem](#7-intel-high-definition-audio--sound-subsystem)
   - [8. CosmicSH POSIX Shell & Terminal Text Editor](#8-cosmicsh-posix-shell--terminal-text-editor)
   - [9. Aqua Graphical Desktop Environment](#9-aqua-graphical-desktop-environment)
4. [Interactive Shell Command Reference (53 Commands)](#interactive-shell-command-reference-53-commands)
5. [Automated Verification Suite (45 Self-Tests)](#automated-verification-suite-45-self-tests)
6. [Building and Running](#building-and-running)
   - [Prerequisites](#prerequisites)
   - [Compiling Kernel](#compiling-kernel)
   - [Launching in QEMU](#launching-in-qemu)
   - [Booting on Bare-Metal Hardware](#booting-on-bare-metal-hardware)
7. [License](#license)

---

## Overview

**CosmicOS** is a complete, monolithic, bare-metal 64-bit operating system developed in safe, idiomatic `#![no_std]` Rust with zero external runtime dependencies. Powered by the **Sublime Kernel**, CosmicOS boots natively via Multiboot 1/2, transitions into 64-bit Long Mode, maps kernel space to the higher half (`0xFFFFFFFF80000000+`), and initializes an enterprise-grade OS stack:

- **Preemptive Multitasking & User Space**: Priority & round-robin scheduler with per-CPU runqueues and work-stealing load balancing across all online SMP cores, core affinity masks (`sched_setaffinity` / `sched_getaffinity`), 64-bit ELF binary loader, hardware privilege level transition (`iretq`, `syscall`/`sysretq`), Copy-on-Write process fork (`fork`/`execve`/`waitpid`), POSIX signal delivery (`sigaction`, `sigprocmask`, `kill`), dynamic runtime linker (`ld-cosmic.so`), userspace C standard library (`cosmic-libc`), native user binaries in `/bin` (`init`, `calc`, `sh`, `snake`, `dyntest`, `windowserver`, `telnetd`, `sshd`), and shared libraries in `/lib` (`ld-cosmic.so`, `libc.so`, `libm.so`, `libgui.so`).
- **SMP Multi-Core & APIC Architecture**: ACPI Multiple APIC Description Table (MADT) parser, Local APIC (LAPIC) high-speed timer, Inter-Processor Interrupt (IPI) dispatch, I/O APIC 64-bit redirection tables (cleanly disabling legacy 8259 PIC), and Application Processor (AP) core wakeup sequence (INIT-SIPI-SIPI).
- **Advanced Block Storage & Partition Auto-Mounter**: PCIe NVMe controller driver with circular Admin/IO submission and completion queues, AHCI SATA controller driver with 32-slot command lists and PRDT DMA engines, ATA/IDE PIO LBA48, USB Mass Storage (SCSI / Bulk-Only Transport over xHCI / EHCI), and real partition table auto-mounter scanning MBR (primary and EBR chains) and GPT into `/Volumes/`.
- **Filesystems & VFS**: Pluggable Virtual File System (`/System`, `/Library`, `/Applications`, `/Users`, `/Volumes`, `/dev`, `/proc`), read/write FAT32 with cluster allocation bitmap, exFAT parser, Linux ext4 superblock/extent-tree resolver, and Windows NTFS resident `$DATA` / `$MFT` record decoder.
- **Network Protocol Stack**: Full TCP/IP stack (Ethernet II, ARP, IPv4, ICMP Ping, UDP, RFC 793 TCP state machine, BSD Socket API), IEEE 802.11 Wi-Fi subsystem (beacon frame parsing, 4-way handshake, WPA2-PSK with PBKDF2-SHA1), Intel e1000 Gigabit NIC driver, HTTP/1.1 client (`curl`), and embedded HTTP server (`httpd`).
- **Intel HDA Audio Subsystem**: Intel High Definition Audio controller driver, pure integer quarter-wave sine synthesis, RIFF/WAVE PCM parser, PC speaker fallback, and the iconic CosmicOS startup chime.
- **POSIX Shell & Editor**: CosmicSH terminal shell with multi-stage pipelines (`|`), file redirection (`>`, `>>`, `<`), Unix filters (`grep`, `wc`, `head`, `tail`, `sort`, `uniq`), and full-screen in-terminal text editor (`nano` / `edit`).
- **Aqua Graphical Desktop**: 1024x768x32bpp double-buffered compositor, glass dock, top global menu bar, window manager with traffic light controls, Finder, Terminal, and Activity Monitor.

---

## Architectural Overview

```text
                    +-------------------------------------------------------------+
                    |                CosmicOS Aqua Desktop Compositor             |
                    |        (1024x768x32bpp TrueColor Double-Buffered Canvas)     |
                    |        - Top Menu Bar, Glass Dock, Window Manager, Finder   |
                    +------------------------------+------------------------------+
                                                   |
                    +------------------------------+------------------------------+
                    |              CosmicSH Interactive Diagnostic Shell          |
                    |   - Pipelines (|), File Redirection (>, >>, <), Nano Editor |
                    |   - UNIX Filters: grep, wc, head, tail, sort, uniq          |
                    +------------------------------+------------------------------+
                                                   |
     +---------------------------------------------+---------------------------------------------+
     |                               Virtual File System (VFS)                                   |
     |   +-------------+  +-------------+  +------------+  +-------------+  +----------------+   |
     |   |    RamFS    |  |  FAT32 R/W  |  |   exFAT    |  |  ext4 extents|  |  NTFS resident |   |
     |   | (/tmp,/etc) |  | (/Volumes/*)|  | (Heap LBA) |  | (Superblock)|  | ($MFT Records) |   |
     |   +-------------+  +-------------+  +------------+  +-------------+  +----------------+   |
     |   +-------------------+  +--------------------+  +------------------------------------+   |
     |   |   DevFS (/dev/*)  |  |   ProcFS (/proc/*) |  |       Unified BlockDevice Trait    |   |
     |   +-------------------+  +--------------------+  +------------------------------------+   |
     +---------------------------------------------+---------------------------------------------+
                                                   |
     +--------------------+------------------------+-----------------------+---------------------+
     |    User Mode Ring 3|        Preemptive Scheduler                    |     Network Stack   |
     | - 64-bit ELF Loader| - Round-Robin & Sleep Deadlines                | - ARP, IPv4, ICMP   |
     | - CoW Process Fork | - SMP Core Dispatch (BSP + APs)                | - TCP State Machine |
     | - Dynamic Linker   | - cosmos_context_switch (assembly)             | - Sockets, curl,    |
     | - POSIX Signals    | - Anonymous Pipes & Process Control Blocks     |   httpd web server  |
     +--------------------+------------------------+-----------------------+---------------------+
                                                   |
     +---------------------------------------------+---------------------------------------------+
     |                               Memory Management Subsystem                                 |
     |   +----------------------------------+  +---------------------------------------------+   |
     |   |   Physical Memory Manager (PMM)  |  |        Virtual Memory Manager (VMM)         |   |
     |   |   Bitmap Frame Allocator (4 KiB) |  |     4-Level Paging (PML4, PDPT, PD, PT)     |   |
     |   |   Frame Reference Counts (CoW)   |  |     Dynamic MMIO & CoW Exception Handler    |   |
     |   +----------------------------------+  +---------------------------------------------+   |
     |   +-----------------------------------------------------------------------------------+   |
     |   |           Expandable Kernel Heap (GlobalAlloc -> Box, Vec, String, Arc)           |   |
     |   +-----------------------------------------------------------------------------------+   |
     +---------------------------------------------+---------------------------------------------+
                                                   |
     +---------------------------------------------+---------------------------------------------+
     |                              x86_64 Hardware & Drivers                                    |
     |  - Boot Trampoline (32 to 64-bit Long Mode) - PCIe NVMe SSD Controller Driver (SQ/CQ)     |
     |  - GDT & 64-bit TSS with IST Stacks         - AHCI SATA Controller Driver (PRDT DMA)      |
     |  - IDT (256 Vector Traps, Exceptions & IRQs)- Intel e1000 Gigabit Ethernet Controller     |
     |  - ACPI MADT Parser & SMP Boot (INIT/SIPI)  - IEEE 802.11 Wi-Fi (WPA2-PSK / PBKDF2)       |
     |  - Local APIC & I/O APIC Redirection Tables - xHCI / EHCI USB Mass Storage (SCSI / BOT)   |
     |  - 8254 PIT Timer & CMOS Hardware RTC       - Intel High Definition Audio (HDA) Driver    |
     |  - 16550A UART COM1 / COM2 (115200 baud)    - Bochs VBE Display (1024x768x32bpp)         |
     +-------------------------------------------------------------------------------------------+
```

---

## Core Subsystems

### 1. Architecture, Boot & Low-Level Hardware
*Location: [`src/arch/x86_64/`](src/arch/x86_64/)*

- **Long Mode Trampoline ([`boot.s`](src/arch/x86_64/boot.s))**: Multiboot 1 and 2 compliant. Verifies CPUID and Long Mode capability, constructs initial 4-level PML4 page tables mapping the first 1 GiB physically and to the higher half (`0xFFFFFFFF80000000`), enables PAE, `EFER.LME`, and `CR0.PG/WP`, and transitions seamlessly into 64-bit Rust.
- **Global Descriptor Table (GDT) & TSS ([`gdt.rs`](src/arch/x86_64/gdt.rs))**: Configures Kernel Code (`0x08`), Kernel Data (`0x10`), User Data (`0x1B`, DPL 3), User Code (`0x23`, DPL 3), and Task State Segment (`0x28`). Installs dedicated Interrupt Stack Tables (`IST1` for Double Fault, `IST2` for Page Fault).
- **Interrupt Descriptor Table (IDT) ([`idt.rs`](src/arch/x86_64/idt.rs))**: 256 vector gates handling all x86_64 CPU exceptions with complete register/CR2 state dumps, hardware IRQs 32..47, and `int 0x80` syscall vector. Includes trap isolation for user faults (`#PF`, `#GP`, `#UD`, `#DE`) to prevent kernel panics.
- **Fast AMD64 System Calls ([`syscall.rs`](src/arch/x86_64/syscall.rs), [`syscall_entry.s`](src/syscall/syscall_entry.s))**: Configures MSRs `IA32_EFER.SCE`, `IA32_STAR`, `IA32_LSTAR`, and `IA32_FMASK` for hardware-accelerated user/kernel privilege transitions via `syscall` / `sysretq`.
- **ACPI & MADT Table Parser ([`acpi.rs`](src/arch/x86_64/acpi.rs), [`madt.rs`](src/arch/x86_64/madt.rs))**: Parses RSDP, RSDT, XSDT, FADT, and MADT (Multiple APIC Description Table), enumerating physical CPU cores, Local APIC base (`0xFEE00000`), I/O APIC physical address (`0xFEC00000`), and Interrupt Source Overrides.
- **Local APIC & I/O APIC Drivers ([`apic.rs`](src/arch/x86_64/apic.rs), [`ioapic.rs`](src/arch/x86_64/ioapic.rs))**: Maps LAPIC and I/O APIC MMIO registers, programs Spurious Vector Register (SVR `0x1FF`), Task Priority Register (TPR), APIC timer, Inter-Processor Interrupt (IPI) dispatch via ICR, configures 64-bit Redirection Table entries, and masks legacy 8259 PIC.
- **SMP Multiprocessing Architecture ([`smp.rs`](src/arch/x86_64/smp.rs))**: Bootstrap Processor (BSP) detection and Application Processor (AP) core wakeup sequence using the INIT-SIPI-SIPI inter-processor interrupt protocol.
- **Serial & VGA Text Display ([`serial.rs`](src/arch/x86_64/serial.rs), [`vga.rs`](src/arch/x86_64/vga.rs))**: 16550A UART driver with IRQ-driven ring buffers for COM1 and COM2; 80x25 color VGA text driver with scrolling, ANSI escape color rendering, and thread-safe stdout capture hooks.

### 2. Memory Management Subsystem
*Location: [`src/mm/`](src/mm/)*

- **Physical Memory Manager (PMM) ([`pmm.rs`](src/mm/pmm.rs))**: Fast bitmap allocator managing 4 KiB physical frames up to 1 GiB. Tracks per-frame reference counts (`FRAME_REF_COUNTS`) in BSS for transparent Copy-on-Write sharing across forked processes.
- **Virtual Memory Manager (VMM) ([`vmm.rs`](src/mm/vmm.rs))**: 4-level paging manager (PML4, PDPT, PD, PT). Supports dynamic page mapping/unmapping, address translation, TLB invalidation (`invlpg`), non-cacheable MMIO mapping ([`map_mmio_range`](src/mm/vmm.rs)), and page fault handling for Copy-on-Write ([`handle_cow_fault`](src/mm/vmm.rs)) with in-place permission promotion when reference count is 1.
- **Kernel Heap ([`heap.rs`](src/mm/heap.rs))**: Expandable `GlobalAlloc` implementation backed by PMM frames. Unlocks Rust `alloc` data structures (`Vec`, `Box`, `String`, `Arc`, `format!`).
- **User Address Space ([`address_space.rs`](src/mm/address_space.rs))**: Per-process PML4 address spaces with kernel higher-half clone, Virtual Memory Areas (VMAs), POSIX `brk()` heap expansion, direct user memory patching ([`write_user_u64`](src/mm/address_space.rs)), and Copy-on-Write address space cloning ([`clone_cow`](src/mm/address_space.rs)).
- **POSIX Shared Memory & Memory Mapping ([`shm.rs`](file:///c:/Users/NiO/Desktop/cosmos/src/mm/shm.rs), [`process.rs`](file:///c:/Users/NiO/Desktop/cosmos/src/task/process.rs))**:
  - `shm_open()` (via `/dev/shm/*` and `sys_shm_open`): Creates or opens named in-memory shared memory objects backed by physical PMM frames.
  - `ftruncate()` (`sys_ftruncate` Syscall 77): Resizes shared memory objects, dynamically allocating or freeing 4 KiB physical frames.
  - `mmap()` (`sys_mmap` Syscall 9, `MAP_SHARED` / `MAP_PRIVATE` / `MAP_ANONYMOUS`) and `munmap()` (`sys_munmap` Syscall 11): Maps shared physical frames or anonymous memory directly into the process PML4 address space and records Virtual Memory Areas (`VirtualMemoryArea`). Enables zero-copy Inter-Process Communication (IPC) across independent address spaces.
  - `shm_unlink()`: Removes named objects from the global registry (`SHM_REGISTRY`) and `/dev/shm`.
- **Demand Paging & Swap Subsystem ([`vma.rs`](file:///c:/Users/NiO/Desktop/cosmos/src/mm/vma.rs), [`swap.rs`](file:///c:/Users/NiO/Desktop/cosmos/src/mm/swap.rs))**:
  - **Demand Paging**: Lazy physical frame allocation via CPU Page Fault (`#PF` Vector 14). Tracks virtual memory regions (`VmaRegion`, `VMA_MANAGER`), deferring physical allocation until memory is first accessed. The `#PF` handler automatically intercepts unmapped faults, allocates zeroed 4 KiB frames from PMM, maps them into PML4 with appropriate permissions (`PAGE_PRESENT`, `PAGE_USER`, `PAGE_WRITABLE`), invalidates TLB (`invlpg`), and seamlessly resumes execution.
  - **Swap Subsystem**: In-kernel swap storage managing 4 KiB swap slots (`SWAP_MANAGER`). `swap_out()` copies active physical pages into swap storage, frees physical frames to PMM, and encodes the slot ID into the PTE with `PAGE_SWAPPED` (bit 10). When a swapped-out page is accessed, `#PF` transparently triggers `swap_in()`, allocating a fresh frame, restoring contents, remapping the page, and freeing the swap slot without process awareness.

### 3. Task Management, Process Control & POSIX Signals
*Location: [`src/task/`](src/task/), [`src/ipc/`](src/ipc/)*

- **Preemptive Multitasking ([`scheduler.rs`](src/task/scheduler.rs), [`switch.s`](src/task/switch.s))**: Timer-preempted scheduler with sleeping queues, priority queues, and atomic assembly context switching saving callee-saved registers (`rbx, rbp, r12-r15, rflags`).
- **ELF64 Binary Loader ([`elf.rs`](src/task/elf.rs))**: Validates 64-bit ELF headers (`\x7FELF`, `EM_X86_64`), parses program headers (`PT_LOAD`, `PT_DYNAMIC`, `PT_INTERP`), dynamic sections (`DT_NEEDED`, `DT_SYMTAB`, `DT_STRTAB`, `DT_JMPREL`), allocates user frames, maps segments with hardware permissions (`PAGE_USER`, `PAGE_WRITABLE`), zero-initializes BSS sections, and allocates a 64 KiB user stack at `0x00007FFFFFFF0000`.
- **Ring 3 Transition Engine ([`user_switch.s`](src/task/user_switch.s), [`user.rs`](src/task/user.rs))**:
  - `enter_user_mode_asm`: sets user segments (`0x1B`), configures `TSS.rsp[0]`, constructs 64-bit `iretq` frame `[SS, RSP, RFLAGS, CS, RIP]`, and drops CPU privilege level to Ring 3 (CPL 3).
  - `exit_user_mode_asm`: on `SYS_EXIT` or user fault, safely restores kernel CR3, kernel segment registers, and kernel stack, cleanly returning the process exit code to the kernel caller.
  - Shell command `exec <path>` to launch any 64-bit ELF binary.
- **POSIX Process Fork & Lifecycle ([`process.rs`](src/task/process.rs), [`syscall/`](src/syscall/))**:
  - `sys_fork()` (Syscall 57): Clones parent address space via `clone_cow()`, marking all writable user pages as read-only with `PAGE_COW` (bit 9) and incrementing physical frame reference counts.
  - Page Fault CoW Engine (Exception 14): Transparently allocates a private 4096-byte frame, copies memory, remaps with `PAGE_WRITABLE`, or promotes permissions in-place if sole owner, immediately resuming user code via `iretq`.
  - `sys_waitpid()` (Syscall 61): Blocks until target child or any child terminates or changes state, supporting POSIX `WNOHANG`, `WUNTRACED`, and `WCONTINUED`.
  - `sys_execve()` (Syscall 59): Replaces current process image with a new ELF executable.
- **POSIX Process Groups, Sessions & Job Control ([`process.rs`](file:///c:/Users/NiO/Desktop/cosmos/src/task/process.rs), [`job.rs`](file:///c:/Users/NiO/Desktop/cosmos/src/shell/job.rs))**:
  - Process Groups (`pgid`) and Session IDs (`sid`): `sys_setpgid` (109), `sys_getpgrp` (111), `sys_getpgid` (121), `sys_setsid` (112), and `sys_getsid` (124).
  - Terminal foreground process group management (`TIOCGPGRP` / `TIOCSPGRP`) over PTY master/slave devices.
  - Signal broadcasting to process groups (`sys_kill(-pgid, sig)`), terminal `Ctrl+Z` interception sending `SIGTSTP` to suspend foreground jobs into `TaskState::Stopped`, and shell job commands (`jobs`, `fg`, `bg`).
- **POSIX Signals Subsystem ([`signal.rs`](file:///c:/Users/NiO/Desktop/cosmos/src/ipc/signal.rs))**:
  - Per-process signal action tables ([`SigAction`](src/ipc/signal.rs)), blocked signal bitmasks, pending signals delivery, asynchronous inter-process signaling ([`sys_kill`](src/task/process.rs)), child termination notification (`SIGCHLD` to parent), and unblockable signals (`SIGKILL`, `SIGSTOP`).
  - Syscalls: `sys_rt_sigaction` (13), `sys_rt_sigprocmask` (14), and `sys_kill` (62).
- **POSIX Semaphores & Message Queues ([`semaphore.rs`](file:///c:/Users/NiO/Desktop/cosmos/src/ipc/semaphore.rs), [`mqueue.rs`](file:///c:/Users/NiO/Desktop/cosmos/src/ipc/mqueue.rs))**:
  - **Named Semaphores**: `sem_open` (240), `sem_close` (241), `sem_post` (242), `sem_wait` (243), `sem_trywait` (244), `sem_getvalue` (245), `sem_unlink` (246), and `/dev/sem/*` virtual VFS filesystem integration.
  - **POSIX Message Queues**: `mq_open` (247), `mq_send` (248), `mq_receive` (249), `mq_unlink` (250), `mq_getattr` (251), and `/dev/mqueue/*` VFS directory node. Supports priority-ordered message insertion (higher priority popped first) and configurable queue limits (`mq_maxmsg`, `mq_msgsize`).
- **Kernel POSIX Threads & Futexes ([`process.rs`](file:///c:/Users/NiO/Desktop/cosmos/src/task/process.rs), [`futex.rs`](file:///c:/Users/NiO/Desktop/cosmos/src/ipc/futex.rs))**:
  - `sys_clone()` (Syscall 56): Linux-compatible process and kernel thread cloning supporting `CLONE_VM` (shared PML4 address space), `CLONE_FILES` (shared file descriptor table), `CLONE_FS`, `CLONE_SIGHAND`, `CLONE_THREAD`, `CLONE_PARENT_SETTID`, and `CLONE_CHILD_SETTID`. Enables multi-threaded execution with shared VM.
  - `sys_futex()` (Syscall 202): Fast userspace locking mechanism implementing `FUTEX_WAIT` (atomic value check and wait queue), `FUTEX_WAKE` (waking waiting threads), and `FUTEX_REQUEUE` (requeueing waiters across futex words for condition variables).

### 4. Userspace Runtime, Dynamic Linker & Standard Library
*Location: [`src/user/`](src/user/)*

- **Dynamic Linker & Shared Libraries ([`linker.rs`](src/user/linker.rs))**: Dynamic runtime linker resolving shared library dependencies from `/lib`, parsing export symbols, and applying AMD64 relocations (`R_X86_64_JUMP_SLOT`, `R_X86_64_GLOB_DAT`, `R_X86_64_64`, `R_X86_64_RELATIVE`) by patching Global Offset Table (GOT) slots in user memory.
- **Userspace C Standard Library (`cosmic-libc`, [`libc.rs`](src/user/libc.rs))**: Standard userspace runtime implementing POSIX syscall wrappers (`syscall0`..`syscall3`):
  - File I/O: `write()`, `read()`, `open()`, `close()`.
  - Memory Management: `brk()` and `malloc()` / `free()` heap allocator.
  - Process Credentials & Control: `fork()`, `waitpid()`, `execve()`, `kill()`, `sigaction()`, `sigprocmask()`, `signal()`, `getpid()`, `uname()`, `nanosleep()`, and `exit()`.
  - Console Printing: `puts()`, `putchar()`, ANSI formatting.
- **ELF64 Binary Synthesizer ([`builder.rs`](src/user/builder.rs))**: Lightweight assembler generating self-contained 64-bit ELF executables and shared libraries (`ET_EXEC`, `ET_DYN`) with dynamic tables, strings, symbols, and PLT/GOT relocation headers.
- **Shared Libraries (`/lib`)**:
  - `/lib/ld-cosmic.so`: Dynamic linker runtime image.
  - `/lib/libc.so`: Shared C standard library exporting `cosmic_puts` and `cosmic_exit`.
  - `/lib/libm.so`: Shared math library exporting `cosmic_pow` (integer exponentiation).
- **Native Userland Binaries (`/bin`)**:
  - `/bin/init`: System init executable validating Ring 3 user privileges, `getpid()`, `uname()`, dynamic `brk()` heap expansion (+8192 bytes), and clean process termination.
  - `/bin/calc`: 64-bit user arithmetic engine computing multiplication, division, and Gauss summation loop (`Sum(1..100) = 5050`) entirely in Ring 3.
  - `/bin/sh`: Isolated interactive user shell executing in Ring 3 with terminal prompts and standard I/O bridging.
  - `/bin/snake`: Full-screen ASCII arcade game running in Ring 3 utilizing ANSI escape codes and PIT sleep timers.
  - `/bin/dyntest`: Dynamically linked executable requesting `/lib/ld-cosmic.so` interpreter, dynamically importing and linking `cosmic_puts`, `cosmic_pow`, and `cosmic_exit` across shared libraries.
  - `/bin/windowserver`: Aqua display server daemon opening `/dev/fb0`, configuring double-buffered video via `ioctl`, and compositing desktop UI elements via `/lib/libgui.so`.
  - `/bin/telnetd`: Ring 3 Telnet daemon creating TCP server sockets, binding to port 23, and servicing remote terminal shell sessions over standard VFS file descriptors.
  - `/bin/sshd`: Ring 3 Secure Shell daemon binding to TCP port 22, exchanging `SSH-2.0-CosmicSSH_1.0` protocol banners, and validating remote client authentication.

### 5. Filesystems & Real Block Storage
*Location: [`src/fs/`](src/fs/), [`src/drivers/storage/`](src/drivers/storage/)*

- **AHCI SATA Controller Driver ([`ahci.rs`](src/drivers/storage/ahci.rs))**: PCI discovery (Class 0x01, Subclass 0x06), ABAR (BAR5) MMIO mapping, Generic Host Control (GHC.AE AHCI enable), Command List headers (32 slots), Received FIS structure, Command Tables with Physical Region Descriptor Tables (PRDT), and ATA DMA commands (`IDENTIFY DEVICE`, `READ DMA EXT`, `WRITE DMA EXT`).
- **PCIe NVMe Controller Driver ([`nvme.rs`](src/drivers/storage/nvme.rs))**: PCI discovery (Class 0x01, Subclass 0x08), 64-bit BAR0 MMIO mapping, Controller Configuration (`CC.EN`), Admin Submission & Completion Queue rings (ASQ/ACQ), Doorbell Stride dispatching, Admin Identify commands (`Identify Controller`, `Identify Namespace 1`), and I/O Submission/Completion Queue creation for direct NVMe block Read/Write commands.
- **Unified Block Device Interface ([`BlockDevice`](src/drivers/storage/mod.rs))**: Pluggable storage abstraction supporting sector reads/writes across ATA IDE, AHCI SATA, NVMe SSDs, USB mass storage drives, and memory-backed block devices.
- **FAT32 R/W ([`fat32.rs`](src/fs/fat32.rs))**: Real Read/Write implementation with partition discovery, BPB decoding, cluster allocation bitmap scanning, cluster chain traversal, LFN directory parsing, and file creation/overwriting.
- **exFAT ([`exfat.rs`](src/fs/exfat.rs))**: Sector-accurate exFAT parser supporting Main Boot Region, Volume Flags, Cluster Heap allocation, Stream Extension entries, and file traversal.
- **ext4 R/W & HTree Directory Indexing ([`ext4.rs`](src/fs/ext4.rs))**: Linux ext4 superblock parser, Block Group Descriptor Table lookup, block and inode bitmap allocation, extent tree mapping, direct write block updates, directory entry insertion, file creation, and full **HTree (B-Tree) Indexing** (`EXT4_INDEX_FL = 0x1000`). Implements `dx_root` header parsing, `dx_countlimit`, `dx_entry` binary search, multi-level tree traversal, and standards-compliant directory hashing (Half MD4, TEA, and Legacy hashes) with superblock seed integration (`s_hash_seed`), achieving fast $O(\log N)$ file lookups with collision spillover tolerance and clean directory enumeration.
- **NTFS ([`ntfs.rs`](src/fs/ntfs.rs))**: Windows NTFS VBR parsing, `$MFT` record decoding, resident `$DATA` extraction, and non-resident variable-length data runlist decoder (`decode_runlist`). Supports multi-extent fragmented cluster chains, signed LCN relative offsets, sparse zero-filled extents, and sector-by-sector DMA streaming for files of arbitrary length.
- **USB Mass Storage ([`usb/msc.rs`](file:///c:/Users/NiO/Desktop/cosmos/src/drivers/usb/msc.rs))**: Bulk-Only Transport (BOT) driver sending SCSI Command Descriptor Blocks (`INQUIRY`, `READ CAPACITY 10`, `READ 10`, `WRITE 10`) over xHCI and EHCI controllers.
- **USB HID Input Subsystem ([`usb/hid.rs`](file:///c:/Users/NiO/Desktop/cosmos/src/drivers/usb/hid.rs))**: USB Human Interface Device driver supporting Boot Protocol keyboards and mice over xHCI and EHCI controllers. Parses 8-byte keyboard reports, maps USB HID Usage codes to PS/2 scancodes with modifier tracking, and translates 3/4-byte mouse packets into desktop coordinate updates.

### 6. Networking, 802.11 Wi-Fi & Sockets
*Location: [`src/net/`](src/net/), [`src/drivers/net/`](src/drivers/net/)*

- **IEEE 802.11 Wi-Fi ([`wifi.rs`](src/net/wifi.rs))**: Beacon frame management (SSID, BSSID, RSSI, WPA2-RSN capabilities), WPA2-PSK authentication, and 4-way handshake state machine.
- **Network Cryptography ([`crypto.rs`](src/drivers/net/crypto.rs))**: Pure Rust SHA-1, HMAC-SHA1, and PBKDF2 key derivation (4096 iterations) for WPA2 Pairwise Master Key generation.
- **TCP/IP Protocol Stack**:
  - **Ethernet II & ARP ([`ethernet.rs`](src/net/ethernet.rs), [`arp.rs`](src/net/arp.rs))**: Frame encapsulation, hardware MAC resolution, and ARP table caching.
  - **IPv4 & ICMP ([`ipv4.rs`](src/net/ipv4.rs), [`icmp.rs`](src/net/icmp.rs))**: Packet serialization, checksum computation, fragmentation handling, and ICMP Echo Request/Reply (`ping`).
  - **UDP & DNS ([`udp.rs`](src/net/udp.rs), [`dns.rs`](src/net/dns.rs))**: Connectionless datagram transport and DNS resolution (`nslookup`).
  - **TCP State Machine ([`tcp.rs`](src/net/tcp.rs))**: RFC 793 compliant 3-way handshake (`SYN` $\to$ `SYN-ACK` $\to$ `ACK`), sliding window flow control, packet sequencing, and `FIN`/`ACK` teardown.
- **BSD Socket API & POSIX System Calls ([`socket.rs`](src/net/socket.rs))**:
  - Full suite of BSD-compliant socket system calls: `sys_socket` (41), `sys_connect` (42), `sys_accept` (43), `sys_sendto` (44), `sys_recvfrom` (45), `sys_bind` (49), `sys_listen` (50).
  - VFS `SocketInode` integration enabling standard `read()` and `write()` calls directly on socket file descriptors (`fd > 2`).
  - RFC 793 incoming connection handling for listening sockets: child socket allocation, `SYN-ACK` replies, and queueing into listener's `pending_accept` backlog.
- **Unix Domain Sockets & IPC Streams ([`unix.rs`](file:///c:/Users/NiO/Desktop/cosmos/src/net/unix.rs))**:
  - Standard `AF_UNIX` / `AF_LOCAL` local socket IPC (`SockAddrUn`, filesystem namespace path binding).
  - Supports bidirectional in-memory byte streams (`SOCK_STREAM`) and datagram packets (`SOCK_DGRAM`) with full connection lifecycle (`bind`, `listen`, `connect`, `accept`, `send`, `recv`, `close`) and VFS `FileType::Socket` integration (`UnixSocketInode`).
- **DNS Client Cache, Multi-Nameserver Failover & LLMNR ([`dns.rs`](file:///c:/Users/NiO/Desktop/cosmos/src/net/dns.rs))**:
  - **DNS Cache with TTL**: In-memory DNS cache (`DNS_CACHE`) storing resolved IPv4 records with real-time TTL expiration tied to kernel RTC timestamps.
  - **Multi-Nameserver Failover**: Parses `/etc/resolv.conf` (`nameserver`, `search`), managing multi-server configurations (`DNS_CONFIG`) with failover support.
  - **RFC 4795 LLMNR**: Link-Local Multicast Name Resolution listening on UDP port 5355 (`224.0.0.252` / MAC `01:00:5E:00:00:FC`), answering single-label queries (`cosmos`, `cosmoskernel`, `cosmos.local`).
- **Network Socket Server Daemons**:
  - `/bin/telnetd`: Background server daemon listening on TCP port 23, spawning interactive sessions for incoming telnet clients.
  - `/bin/sshd`: Background server daemon listening on TCP port 22 with SSH-2.0 protocol identification and authentication handling.
  - Shell `telnet` client command enabling remote terminal connections from CosmicSH.
- **HTTP/1.1 & TLS 1.3 HTTPS Engine ([`http.rs`](src/net/http.rs), [`tls.rs`](src/net/tls.rs))**:
  - Pure `#![no_std]` TLS 1.3 client stack implementing RFC 8446 handshake with X25519 ECDH key agreement and ChaCha20-Poly1305 AEAD record encryption/decryption.
  - SHA-256, HMAC-SHA256, and HKDF-Extract/Expand key schedule derivation.
  - Client: `curl [-i] <url>` fetching web pages and REST APIs over plain HTTP or secure HTTPS (`curl https://...`).
  - Server: Embedded `httpd` web server serving interactive status dashboards and JSON APIs (`/api/status`).
- **mDNS & Apple Bonjour ZeroConf Responder ([`mdns.rs`](file:///c:/Users/NiO/Desktop/cosmos/src/net/mdns.rs))**:
  - RFC 6762 / RFC 6763 compliant Multicast DNS responder listening on UDP port 5353 (`224.0.0.251` / MAC `01:00:5E:00:00:FB`).
  - Resolves `cosmicos.local` to the system's IPv4 address (`A` record).
  - DNS Service Discovery (DNS-SD) advertising running system daemons: HTTP Web Server (`_http._tcp.local`, port 80), SSH Remote Shell (`_ssh._tcp.local`, port 22), and Telnet Server (`_telnet._tcp.local`, port 23), along with service enumeration (`_services._dns-sd._udp.local`).
  - Generates authoritative answer frames with PTR, SRV, TXT, and A resource records.
- **DHCP Client Daemon ([`dhcp.rs`](file:///c:/Users/NiO/Desktop/cosmos/src/net/dhcp.rs))**:
  - RFC 2131 / RFC 2132 compliant dynamic host configuration client running over UDP client port 68 / server port 67 with magic cookie `0x63825363`.
  - Full state machine (`Init`, `Selecting`, `Requesting`, `Bound`), handling `DHCPDISCOVER`, `DHCPOFFER`, `DHCPREQUEST`, `DHCPACK`, and `DHCPRELEASE`.
  - Automatic dynamic lease acquisition: extracts assigned IPv4, subnet mask, default gateway router, DNS nameserver, server identifier, and lease duration, automatically applying network configurations into `NET_MANAGER`.
  - Interactive shell management via `dhcp` command (`dhcp status`, `dhcp discover [interface]`, `dhcp release`).
- **NTP Network Time Synchronization Daemon ([`ntp.rs`](file:///c:/Users/NiO/Desktop/cosmos/src/net/ntp.rs))**:
  - RFC 5905 compliant Network Time Protocol (NTPv4) and Simple Network Time Protocol (SNTP) client daemon operating on UDP port 123.
  - Constructs 48-byte NTP client requests with leap indicator (`LI=0`), NTP version 4, and client mode 3.
  - Validates and decodes incoming server responses: stratum level (1..16), precision, root delay, root dispersion, reference ID, and 64-bit transmit timestamps.
  - Converts NTP epoch (Jan 1, 1900) to standard UNIX epoch (Jan 1, 1970) with 70-year leap-second delta offset (`NTP_UNIX_OFFSET = 2,208,988,800`).
  - Anchors the master kernel real-time clock to PIT timer ticks, providing synchronized UTC date and time across the entire system.
  - Interactive shell management via `ntp` command (`ntp status`, `ntp sync [server]`, `ntp set <unix_timestamp>`).
- **Hardware Network Drivers ([`src/drivers/net/`](file:///c:/Users/NiO/Desktop/cosmos/src/drivers/net/))**:
  - **Intel e1000 Gigabit NIC ([`e1000.rs`](file:///c:/Users/NiO/Desktop/cosmos/src/drivers/net/e1000.rs))**: PCI controller (`en0`) with MMIO register control and hardware packet rings.
  - **Realtek Fast & Gigabit Ethernet ([`realtek.rs`](file:///c:/Users/NiO/Desktop/cosmos/src/drivers/net/realtek.rs))**: Support for RTL8139 (10/100M Fast Ethernet), RTL8168/RTL8169 (PCI/PCIe Gigabit), and RTL8125 (2.5GbE Gaming NIC) with auto-switching MMIO/I/O port dispatch, hardware reset (`CMD_RST`), MAC extraction (`IDR0`..`IDR5`), 18 KiB RX ring buffer with wrap margin, and 4-descriptor TX rings (`en1`).
  - **Qualcomm Atheros & Killer Ethernet / Wi-Fi ([`qualcomm.rs`](file:///c:/Users/NiO/Desktop/cosmos/src/drivers/net/qualcomm.rs))**: Support for Qualcomm Atheros AR8131/AR8151/AR8161, Killer E2200/E2400/E2500 Gigabit Gaming NICs (`en2`), and Qualcomm Atheros AR9285/AR9485/AR9565 (ath9k 802.11b/g/n wireless `wlan1`) with `ALX_MASTER` reset, station address parsing, and central `NET_MANAGER` registration.
- **POSIX Pseudo-Terminal Subsystem ([`pty.rs`](file:///c:/Users/NiO/Desktop/cosmos/src/tty/pty.rs))**:
  - Master/slave pseudo-terminals (`/dev/ptmx`, `/dev/pts/*`), `sys_posix_openpt` (51), `sys_ptsname` (52), and `sys_unlockpt` (53).
  - Terminal line discipline with ICRNL newline translation, character echo toggling, window sizing ioctls (`TIOCGWINSZ`/`TIOCSWINSZ`), and foreground `SIGINT` signal forwarding on `Ctrl+C`.

### 7. Intel High Definition Audio & Sound Subsystem
*Location: [`src/drivers/audio/`](src/drivers/audio/)*

- **Intel HDA Driver ([`hda.rs`](src/drivers/audio/hda.rs))**: Scans PCI for Intel HDA controllers (Class 0x04, Subclass 0x03), maps MMIO BAR0, executes hardware reset (`CRST`), configures Stream Descriptor 0 for 44.1 kHz 16-bit stereo PCM, and streams samples via DMA.
- **PCM Waveform Synthesizer ([`pcm.rs`](src/drivers/audio/pcm.rs))**:
  - Quarter-wave integer sine lookup table (`integer_sin()`), avoiding floating point instructions in kernel space.
  - RIFF/WAVE parser supporting uncompressed 8-bit and 16-bit PCM WAV audio.
  - Tone generator (`generate_sine_wave()`) with smooth anti-pop envelopes.
  - CosmicOS Startup Chime (`generate_cosmic_chime()`): 4-note resonant harmonic F-Major chord (F3, C4, F4, A4) with natural acoustic decay.
  - Phonetic formant synthesis for text-to-speech audio.
- **PC Speaker Fallback ([`mod.rs`](src/drivers/audio/mod.rs))**: 8254 PIT Timer 2 square-wave hardware tone generation via I/O ports `0x42`, `0x43`, and `0x61`.

### 8. CosmicSH POSIX Shell & Terminal Text Editor
*Location: [`src/shell/`](src/shell/)*

- **POSIX Pipelines ([`pipeline.rs`](src/shell/pipeline.rs))**: Arbitrary multi-stage command piping (`cmd1 | cmd2 | cmd3`) with in-memory stdout interception.
- **File Redirection**:
  - `> <file>`: Redirect stdout to overwrite file.
  - `>> <file>`: Redirect stdout to append to file.
  - `< <file>`: Redirect file contents into command stdin.
- **UNIX Filter Commands**:
  - `grep [-i] [-v] <pattern> [file]`: Regular text line filtering.
  - `wc [-l] [-w] [-c] [file]`: Line, word, and byte counting.
  - `head [-n N] [file]` & `tail [-n N] [file]`: Line head/tail extraction.
  - `sort [-r] [file]`: Alphabetical and reversed line sorting.
  - `uniq [file]`: Adjacent duplicate line elimination.
- **In-Terminal Text Editor ([`editor.rs`](src/shell/editor.rs))**: Full-screen terminal text editor (`nano <path>` or `edit <path>`) with title header, line numbers (`001 | ...`), cursor positioning, arrow keys, Backspace, Delete, Tab, `Ctrl+S` direct VFS saving, and `Ctrl+X` exit.

### 9. Aqua Graphical Desktop Environment
*Location: [`src/gui/`](src/gui/)*

- **Bochs VBE Linear Framebuffer ([`vbe.rs`](src/drivers/vbe.rs))**: 1024x768 with 32-bit ARGB true color mapped directly into higher-half virtual memory (`0xFFFFFFFFD0000000`).
- **Double-Buffered Compositor**: Software rendering engine (`Canvas`) with anti-aliasing, rounded glass rectangles, drop shadows, and horizontal gradients.
- **Window Management**: Window chrome with titlebar dragging, active/inactive focus states, and traffic light buttons (🔴 Close, 🟡 Minimize, 🟢 Zoom).
- **Top Global Menu Bar & Bottom Glass Dock**: Application menus, live RAM status gauge, translucent dock tiles, and running app indicator dots.
- **Built-in Desktop Apps**:
  - **Terminal (`Terminal.app`)**: Graphical zsh shell session.
  - **Activity Monitor (`Activity Monitor.app`)**: Live CPU load, threads, memory pressure meter, and kernel subsystem health.
  - **Finder (`Finder.app`)**: Two-pane file manager with quick volume navigation (`Cosmic HD`, `Cosmic USB`).
  - **About CosmicOS**: Hardware specs and memory counters.

---

## Interactive Shell Command Reference (53 Commands)

CosmicOS features a native zsh-style shell available across all virtual consoles (`Alt+F1` through `Alt+F4`), COM1 serial, and the graphical desktop terminal:

| Category | Command | Description |
| :--- | :--- | :--- |
| **System Info** | `sw_vers` | Display CosmicOS ProductVersion (1.0) and Sublime Kernel build |
| | `system_profiler` | Hardware overview (model, CPU architecture, unified RAM, UUID) |
| | `sysctl [-a]` | Query kernel variables (`kern.version`, `hw.ncpu`, `hw.memsize`, `hw.model`) |
| | `uname` | Print kernel release and machine architecture |
| | `uptime` | Show system uptime in seconds |
| | `date` | Display current UTC hardware timestamp from CMOS RTC |
| | `whoami` / `hostname` | Print current user (`cosmos`) and host (`CosmicOS.local`) |
| | `mem` | Detailed physical frame and expandable heap allocation metrics |
| | `top` / `ps` | Display real-time active tasks, threads, and CPU/memory statistics |
| | `jobs` | List active, suspended, and background jobs (`[1]+ Running cmd`) |
| | `fg [%n]` | Bring background or stopped job to foreground with `SIGCONT` |
| | `bg [%n]` | Resume suspended background job with `SIGCONT` |
| **Filesystem & I/O** | `ls [path]` | List contents of a directory (default `/`) |
| | `cat <path>` | Read file contents to terminal |
| | `echo <text>` | Output text to console or pipeline |
| | `touch <path>` | Create an empty file |
| | `mkdir <path>` | Create a new directory |
| | `rm <path>` | Remove a file |
| | `mount` | Display mounted filesystems (RamFS, DevFS, ProcFS, FAT32, exFAT, ext4, NTFS) |
| | `diskutil [list]` | Manage storage containers and physical partitions (`disk0`, `disk0s1`) |
| **Text Editing & Filters** | `nano <path>` / `edit` | Launch in-terminal full-screen text editor (`Ctrl+S` save, `Ctrl+X` exit) |
| | `grep [-i] [-v] <pat>` | Filter lines matching pattern (supports `-i` case-insensitive, `-v` invert) |
| | `wc [-l] [-w] [-c]` | Count lines, words, and bytes |
| | `head [-n N]` | Print first N lines of input |
| | `tail [-n N]` | Print last N lines of input |
| | `sort [-r]` | Sort lines alphabetically or reversed |
| | `uniq` | Omit repeated adjacent lines |
| | `\|`, `>`, `>>`, `<` | Shell pipelines and file redirection |
| **Network & Web** | `ifconfig` | Display network interfaces (`lo0`, `en0`, `wlan0`) and IP configurations |
| | `dhcp [subcmd]` | DHCP client manager (`status`, `discover`, `release`) |
| | `ntp [subcmd]` | Network Time Protocol synchronization (`status`, `sync`, `set`) |
| | `ping <target>` | Send ICMP ECHO_REQUEST packets to network hosts |
| | `nslookup <host>` | Resolve domain names to IPv4 addresses via DNS |
| | `wifi [subcmd]` | 802.11 Wi-Fi manager (`scan`, `connect <ssid> <pass>`, `status`, `disconnect`) |
| | `curl [-i] <url>` | Fetch web pages & REST APIs over TCP / HTTP |
| | `httpd [status]` | Embedded web server dashboard and REST API |
| | `telnet <host> [port]` | Interactive remote terminal client over TCP (port 23) |
| | `telnetd [port]` | Spawn userspace Telnet server daemon (port 23) |
| | `sshd [port]` | Spawn userspace Secure Shell server daemon (port 22) |
| **Audio & Speech** | `beep [freq] [ms]` | Play audio tone through Intel HDA / PC Speaker (default 880 Hz, 250 ms) |
| | `chime` | Play iconic CosmicOS Sublime startup chime (F-Major harmonic chord) |
| | `play <file.wav>` | Play RIFF/WAVE PCM audio file from VFS or external storage |
| | `say <text>` | Synthesize phonetic speech audio |
| **User Space & Execution** | `exec <binary>` | Execute 64-bit ELF binary in isolated Ring 3 user space |
| | `windowserver` | Launch userspace Aqua Window Server daemon (`/bin/windowserver`) |
| | `open <app/file>` | LaunchServices application launcher (e.g. `open Finder.app`) |
| | `defaults [read]` | Read user preferences and plist defaults |
| **Hardware & Power** | `lspci` | Enumerate PCI Express controller devices |
| | `gui` | Launch CosmicOS Aqua graphical desktop compositor |
| | `clear` | Clear terminal screen |
| | `test` | Run the complete automated kernel verification suite |
| | `reboot` / `halt` | ACPI power reset or shutdown |

---

## Automated Verification Suite (45 Self-Tests)

On startup, CosmicOS executes its automated self-test verification suite, verifying all core subsystems before launching the interactive shell:

```text
=======================================================
             CosmosKernel Verification Suite           
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
  [TEST] mDNS / Bonjour ZeroConf Responder (cosmicos.local & DNS-SD) ... [PASS]
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
=======================================================
      All Kernel Self-Tests PASSED Successfully!       
=======================================================
```

---

## Building and Running

### Prerequisites
1. **Rust Nightly**:
   ```powershell
   rustup default nightly
   rustup target add x86_64-unknown-none
   rustup component add rust-src llvm-tools
   ```
2. **QEMU Emulator**:
   Ensure `qemu-system-x86_64` is installed and available in your PATH.

### Compiling Kernel
Compile the release kernel binary and convert to a Multiboot-compatible 32-bit ELF container:
```powershell
.\build.ps1
```
*Outputs:*
- `target/x86_64-unknown-none/release/cosmoskernel` (64-bit ELF)
- `cosmoskernel32.elf` (Multiboot bootable container)

### Launching in QEMU

#### Terminal Mode (Headless COM1 Serial):
```powershell
.\run.ps1 -Headless
```

#### Graphical Aqua Desktop Mode (1024x768x32bpp, Intel HDA Audio, PS/2 Mouse):
```powershell
.\run.ps1
```

### Booting on Bare-Metal Hardware

CosmicOS is tested and verified on real bare-metal Intel and AMD x86_64 hardware:

1. **Ventoy (Recommended)**:
   - Install [Ventoy](https://www.ventoy.net) on a USB flash drive.
   - Copy `cosmoskernel32.elf` directly onto the Ventoy USB drive.
   - Insert USB into any PC (UEFI or Legacy BIOS), boot from USB, and select `cosmoskernel32.elf`.
2. **GRUB2 / Rufus**:
   - Format a USB drive as FAT32.
   - Execute:
     ```powershell
     .\create_boot_media.ps1 -DriveLetter E:
     ```
     *(Replace `E:` with your USB drive letter)*
   - Boot via the installed GRUB2 Multiboot loader.

---

## License

CosmicOS is dual-licensed under either of:
- **MIT License** ([LICENSE-MIT](LICENSE-MIT))
- **Apache License, Version 2.0** ([LICENSE-APACHE](LICENSE-APACHE))
