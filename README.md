# CosmicOS — Sublime Kernel (x86_64)

> **A Clean, Mature, and Feature-Complete 64-bit Monolithic Operating System Kernel Written in Pure Rust.**

[![Target](https://img.shields.io/badge/target-x86__64--unknown--none-blue.svg)](https://www.rust-lang.org/)
[![Toolchain](https://img.shields.io/badge/rustc-nightly-orange.svg)](https://rust-lang.github.io/rustup/)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-green.svg)](LICENSE)
[![Architecture](https://img.shields.io/badge/arch-x86__64%20(AMD64)-red.svg)]()
[![OS](https://img.shields.io/badge/OS-CosmicOS%201.0-purple.svg)]()
[![Filesystems](https://img.shields.io/badge/filesystems-FAT32%20%7C%20exFAT%20%7C%20ext4%20%7C%20NTFS-brightgreen.svg)]()
[![Network](https://img.shields.io/badge/network-TCP%2FIP%20%7C%20802.11%20Wi--Fi%20%7C%20HTTP-cyan.svg)]()
[![Audio](https://img.shields.io/badge/audio-Intel%20HDA%20%7C%20PCM%20%7C%20WAV-magenta.svg)]()
[![UserSpace](https://img.shields.io/badge/userspace-ELF64%20%7C%20Ring%203%20%7C%20Syscall-blueviolet.svg)]()

CosmicOS is a complete, monolithic, bare-metal 64-bit operating system developed in safe, idiomatic `#![no_std]` Rust with zero external runtime dependencies. Powered by the **Sublime Kernel**, CosmicOS boots natively via Multiboot 1/2, transitions to 64-bit Long Mode, maps kernel space to the higher half (`0xFFFFFFFF80000000+`), and initializes an enterprise-grade OS stack:
- **Multitasking & User Space**: Preemptive round-robin & priority scheduler, 64-bit ELF binary loader, hardware memory isolation, Ring 3 user privilege transition (`iretq`, `syscall`/`sysretq`), copy-on-write process fork (`fork`/`execve`/`waitpid`), POSIX signal subsystem (`sigaction`, `sigprocmask`, `kill`), dynamic runtime linker (`ld-cosmic.so`), userspace C standard library (`cosmic-libc`), and native user binaries in `/bin` (`/bin/init`, `/bin/calc`, `/bin/sh`, `/bin/snake`, `/bin/dyntest`) and shared libraries in `/lib` (`ld-cosmic.so`, `libc.so`, `libm.so`).
- **SMP Multi-Core & APIC Architecture**: ACPI MADT table parsing, Local APIC (LAPIC) timer and IPI dispatch, I/O APIC redirection table routing (masking legacy 8259 PIC), and SMP multi-processor core initialization (INIT-SIPI-SIPI).
- **Storage & Block Devices**: Real Read/Write FAT32 cluster allocator, exFAT parser, Linux ext4 extent tree mapping, Windows NTFS resident `$DATA` / `$MFT` parser, AHCI SATA controller driver (Command Lists, PRDT DMA), PCIe NVMe controller driver (Admin & I/O SQ/CQ circular rings, doorbells), and USB Mass Storage (SCSI / Bulk-Only Transport).
- **Network Protocol Stack**: Full TCP/IP stack (Ethernet II, ARP, IPv4, ICMP Ping, UDP, RFC 793 TCP state machine, BSD Socket API), IEEE 802.11 Wi-Fi subsystem (beacon frame parsing, 4-way handshake, WPA2-PSK with PBKDF2-SHA1), Intel e1000 Gigabit NIC driver, HTTP/1.1 client (`curl`), and embedded HTTP server (`httpd`).
- **Intel HDA Audio Subsystem**: Intel High Definition Audio (HDA) controller driver, pure integer quarter-wave sine synthesis, RIFF/WAVE PCM parser, hardware PC speaker fallback, and the iconic CosmicOS startup chime.
- **POSIX Shell, Pipelines & In-Terminal Editor**: Shell with pipelines (`|`), I/O redirection (`>`, `>>`, `<`), Unix filters (`grep`, `wc`, `head`, `tail`, `sort`, `uniq`), and full-screen terminal editor (`nano` / `edit`).
- **Aqua Graphical Desktop**: 1024x768x32bpp double-buffered compositor, glass dock, top global menu bar, window manager with traffic light controls, Finder, Terminal, and Activity Monitor.

---

## Architectural Overview

```
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
     |   |   DevFS (/dev/*)  |  |   ProcFS (/proc/*) |  |   USB Mass Storage (SCSI / BOT)    |   |
     |   +-------------------+  +--------------------+  +------------------------------------+   |
     +---------------------------------------------+---------------------------------------------+
                                                   |
     +--------------------+------------------------+-----------------------+---------------------+
     |    User Mode Ring 3|        Preemptive Scheduler                    |     Network Stack   |
     | - 64-bit ELF Loader| - Round-Robin & Sleep Deadlines                | - ARP, IPv4, ICMP   |
     | - iretq / sysretq  | - cosmos_context_switch (assembly)             | - TCP State Machine |
     | - Trap & Fault Iso | - Thread & Process Control Blocks              | - Sockets, curl,    |
     | - exec <binary>    | - Anonymous Pipes & POSIX Signals              |   httpd web server  |
     +--------------------+------------------------+-----------------------+---------------------+
                                                   |
     +---------------------------------------------+---------------------------------------------+
     |                                  Audio Subsystem                                          |
     |   - Intel High Definition Audio (HDA) PCI Bus Master Controller Driver                    |
     |   - Pure Integer Quarter-Wave Sine Tone Synthesizer & Speech Formants                     |
     |   - RIFF / WAVE PCM Audio Parser & 4-Note Harmonic Startup Chime Generator               |
     |   - Fallback 8254 PIT Timer 2 PC Speaker Hardware Tone Generator                          |
     +---------------------------------------------+---------------------------------------------+
                                                   |
     +---------------------------------------------+---------------------------------------------+
     |                             Memory Management Subsystem                                   |
     |   +----------------------------------+  +---------------------------------------------+   |
     |   |   Physical Memory Manager (PMM)  |  |        Virtual Memory Manager (VMM)         |   |
     |   |   Bitmap Frame Allocator (4 KiB) |  |     4-Level Paging (PML4, PDPT, PD, PT)     |   |
     |   +----------------------------------+  +---------------------------------------------+   |
     |   +-----------------------------------------------------------------------------------+   |
     |   |           Expandable Kernel Heap (GlobalAlloc -> Box, Vec, String, Arc)           |   |
     |   +-----------------------------------------------------------------------------------+   |
     +---------------------------------------------+---------------------------------------------+
                                                   |
     +---------------------------------------------+---------------------------------------------+
     |                              x86_64 Hardware & Drivers                                    |
     |  - Boot Trampoline (32 to 64-bit Long Mode) - Intel e1000 Gigabit Ethernet Controller     |
     |  - GDT & 64-bit TSS with IST Stacks         - IEEE 802.11 Wi-Fi (WPA2-PSK / PBKDF2)       |
     |  - IDT (256 Vector Traps & IRQs)            - xHCI / EHCI USB Host Controller Drivers     |
     |  - 8259 Dual PIC Remapping & Local APIC     - ATA / IDE PIO LBA48 & AHCI SATA Controller  |
     |  - 8254 PIT Timer (1000 Hz) & CMOS RTC      - PS/2 Keyboard & Mouse Drivers               |
     |  - 16550A UART COM1 / COM2 (115200 baud)    - Bochs VBE Display (1024x768x32bpp)         |
     +-------------------------------------------------------------------------------------------+
```

---

## Core Subsystems

### 1. Architecture & Low-Level Hardware (`src/arch/x86_64`)
- **Long Mode Trampoline (`boot.s`)**: Multiboot 1 and 2 compliant. Verifies CPUID and Long Mode, builds initial 4-level PML4 page tables mapping the first 1 GiB physically and to the higher half (`0xFFFFFFFF80000000`), enables PAE, `EFER.LME`, and `CR0.PG/WP`, and jumps into higher-half 64-bit Rust.
- **Global Descriptor Table (GDT) & TSS (`gdt.rs`)**: Configures 64-bit Kernel Code (`0x08`), Kernel Data (`0x10`), User Data (`0x1B`, DPL 3), User Code (`0x23`, DPL 3), and Task State Segment (`0x28`). Installs dedicated Interrupt Stack Tables (`IST1` for Double Fault, `IST2` for Page Fault).
- **Interrupt Descriptor Table (IDT) (`idt.rs`)**: 256 vector gates handling all x86_64 CPU exceptions with registers/CR2 dump, hardware IRQs 32..47, and `int 0x80` syscall vector. Isolates user-mode traps (`#PF`, `#GP`, `#UD`, `#DE`) to prevent kernel panics.
- **Fast AMD64 System Calls (`syscall.rs`)**: Configures MSRs `IA32_EFER.SCE`, `IA32_STAR`, `IA32_LSTAR` (`syscall_entry`), and `IA32_FMASK` for hardware-accelerated user/kernel transitions.
- **ACPI & MADT Table Parser (`acpi.rs`, `madt.rs`)**: Parses RSDP, RSDT, XSDT, FADT, and MADT (Multiple APIC Description Table), enumerating physical CPU cores, Local APIC base (`0xFEE00000`), I/O APIC address (`0xFEC00000`), and Interrupt Source Overrides.
- **Local APIC & I/O APIC Drivers (`apic.rs`, `ioapic.rs`)**: Maps LAPIC and I/O APIC MMIO registers, programs Spurious Vector Register (SVR), calibrates APIC timer, sends Inter-Processor Interrupts (IPIs) via ICR, configures 64-bit Redirection Table entries, and masks legacy 8259 PIC.
- **SMP Multiprocessing Architecture (`smp.rs`)**: Bootstrap Processor (BSP) detection and Application Processor (AP) core wakeup sequence using INIT-SIPI-SIPI inter-processor interrupts.
- **Serial & VGA Text Display (`serial.rs`, `vga.rs`)**: 16550A UART driver with IRQ-driven ring buffers for COM1 and COM2; 80x25 color VGA text driver with scrolling, ANSI escape color rendering, and thread-safe stdout capture hooks.

### 2. Memory Management (`src/mm`)
- **Physical Memory Manager (PMM) (`pmm.rs`)**: Fast bitmap allocator managing 4 KiB physical frames up to 1 GiB. Tracks per-frame reference counts (`FRAME_REF_COUNTS`) for Copy-on-Write sharing across forked processes.
- **Virtual Memory Manager (VMM) (`vmm.rs`)**: 4-level paging manager. Supports dynamic page mapping/unmapping, address translation, TLB invalidation (`invlpg`), non-cacheable MMIO mapping (`map_mmio_range`), and page fault handler for Copy-on-Write (`handle_cow_fault`) with seamless permission promotion.
- **Kernel Heap (`heap.rs`)**: Expandable `GlobalAlloc` implementation backed by PMM frames. Unlocks Rust `alloc` data structures (`Vec`, `Box`, `String`, `Arc`, `format!`).
- **User Address Space (`address_space.rs`)**: Per-process PML4 address spaces with kernel higher-half clone, Virtual Memory Areas (VMAs), POSIX `brk()` heap expansion, direct user memory patching (`write_user_u64`), and Copy-on-Write address space cloning (`clone_cow`).

### 3. Task Management & 64-bit Ring 3 User Space (`src/task`)
- **Preemptive Multitasking (`scheduler.rs`, `switch.s`)**: Timer-preempted scheduler with sleeping queues, priority queues, and atomic assembly context switching saving callee-saved registers (`rbx, rbp, r12-r15, rflags`).
- **ELF64 Binary Loader (`elf.rs`)**: Validates 64-bit ELF headers (`\x7FELF`, EM_X86_64), parses program headers (`PT_LOAD`, `PT_DYNAMIC`, `PT_INTERP`), dynamic sections (`DT_NEEDED`, `DT_SYMTAB`, `DT_STRTAB`, `DT_JMPREL`), allocates user frames, maps segments with hardware permissions (`PAGE_USER`, `PAGE_WRITABLE`), zero-initializes BSS sections, and allocates a 64 KiB user stack at `0x00007FFFFFFF0000`.
- **Ring 3 Transition Engine (`user_switch.s`, `user.rs`)**:
  - `enter_user_mode_asm`: sets user segments (`0x1B`), configures `TSS.rsp[0]`, constructs 64-bit `iretq` frame `[SS, RSP, RFLAGS, CS, RIP]`, and drops CPU privilege level to Ring 3 (CPL 3).
  - `exit_user_mode_asm`: on `SYS_EXIT` or user fault, safely restores kernel CR3, kernel segment registers, and kernel stack, cleanly returning the process exit code to the kernel caller.
  - Shell command `exec <path>` to launch any 64-bit ELF binary.
- **POSIX Process Fork & Lifecycle (`process.rs`, `idt.rs`, `syscall/`)**:
  - `sys_fork()` (Syscall 57): Clones parent address space via `clone_cow()`, marking all writable user pages as read-only with `PAGE_COW` (bit 9) and incrementing physical frame reference counts.
  - Page Fault CoW Engine (Exception 14): When child or parent attempts to write to a CoW page, the page fault handler transparently allocates a new 4096-byte frame, copies memory, remaps with `PAGE_WRITABLE`, or promotes permissions in-place if refcount is 1, immediately resuming user code via `iretq`.
  - `sys_waitpid()` (Syscall 61): Blocks until target child or any child terminates, returning status code and reaping process structures.
  - `sys_execve()` (Syscall 59): Replaces current process image with a new ELF executable.
  - **POSIX Signals Subsystem (`signal.rs`, `sys_rt_sigaction`, `sys_rt_sigprocmask`, `sys_kill`)**: Per-process signal action tables (`SigAction`), blocked signal bitmasks, pending signals delivery, asynchronous inter-process signaling (`sys_kill`), child termination signals (`SIGCHLD` to parent), and unblockable signals (`SIGKILL`, `SIGSTOP`).

### 4. Userspace C Standard Library (`cosmic-libc`), Dynamic Linker & Binaries (`src/user`)
- **Dynamic Linker & Shared Libraries (`linker.rs`)**: Dynamic runtime linker resolving shared library dependencies from `/lib`, parsing export symbols, and performing AMD64 relocations (`R_X86_64_JUMP_SLOT`, `R_X86_64_GLOB_DAT`, `R_X86_64_64`, `R_X86_64_RELATIVE`) by patching Global Offset Table (GOT) slots in user memory.
- **Userspace C Standard Library (`libc.rs`)**: Standard userspace runtime implementing POSIX syscall wrappers (`syscall0`..`syscall3`):
  - File I/O: `write()`, `read()`, `open()`, `close()`.
  - Memory Management: `brk()` and `malloc()` / `free()` heap allocator.
  - Process Credentials & Control: `fork()`, `waitpid()`, `execve()`, `kill()`, `sigaction()`, `sigprocmask()`, `signal()`, `getpid()`, `uname()`, `nanosleep()`, and `exit()`.
  - Console Printing: `puts()`, `putchar()`, ANSI formatting.
- **ELF64 Binary Synthesizer (`builder.rs`)**: Lightweight assembler generating self-contained 64-bit ELF executables and shared libraries (`ET_EXEC`, `ET_DYN`) with dynamic tables, strings, symbols, and PLT/GOT relocation headers.
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

### 5. Advanced Filesystems & Real Block Storage (`src/fs`, `src/drivers/storage`)
- **AHCI SATA Controller Driver (`src/drivers/storage/ahci.rs`)**: PCI discovery (Class 0x01, Subclass 0x06), ABAR (BAR5) MMIO mapping, Generic Host Control (GHC.AE AHCI enable), Command List headers (32 slots), Received FIS structure, Command Tables with Physical Region Descriptor Tables (PRDT), and ATA DMA commands (`IDENTIFY DEVICE`, `READ DMA EXT`, `WRITE DMA EXT`).
- **PCIe NVMe Controller Driver (`src/drivers/storage/nvme.rs`)**: PCI discovery (Class 0x01, Subclass 0x08), 64-bit BAR0 MMIO mapping, Controller Configuration (`CC.EN`), Admin Submission & Completion Queue rings (ASQ/ACQ), Doorbell Stride dispatching, Admin Identify commands (`Identify Controller`, `Identify Namespace 1`), and I/O Submission/Completion Queue creation for direct NVMe block Read/Write commands.
- **Unified Block Device Interface (`BlockDevice`)**: Pluggable storage abstraction supporting sector reads/writes across ATA IDE, AHCI SATA, NVMe SSDs, USB mass storage drives, and memory-backed block devices.
- **FAT32 R/W (`fat32.rs`)**: Real Read/Write implementation with partition discovery, BPB decoding, cluster allocation bitmap scanning, cluster chain traversal, LFN directory parsing, and file creation/overwriting.
- **exFAT (`exfat.rs`)**: Sector-accurate exFAT parser supporting Main Boot Region, Volume Flags, Cluster Heap allocation, Stream Extension entries, and file traversal.
- **ext4 (`ext4.rs`)**: Linux ext4 superblock parser, Block Group Descriptor Table lookup, inode table traversal, and B-tree extent tree (`ext4_extent_header`, `ext4_extent`) physical block resolution.
- **NTFS (`ntfs.rs`)**: Windows NTFS VBR parsing, `$MFT` record decoding, and resident `$DATA` attribute extraction.
- **USB Mass Storage (`src/drivers/usb/`)**: Bulk-Only Transport (BOT) driver sending SCSI Command Descriptor Blocks (`INQUIRY`, `READ CAPACITY 10`, `READ 10`, `WRITE 10`) over xHCI and EHCI controllers.

### 6. Networking, 802.11 Wi-Fi & Sockets (`src/net`)
- **IEEE 802.11 Wi-Fi (`wifi.rs`)**: Beacon frame management (SSID, BSSID, RSSI, WPA2-RSN capabilities), WPA2-PSK authentication, and 4-way handshake state machine.
- **Network Cryptography (`src/drivers/net/crypto.rs`)**: Pure Rust SHA-1, HMAC-SHA1, and PBKDF2 key derivation (4096 iterations) for WPA2 Pairwise Master Key generation.
- **TCP/IP Protocol Stack**:
  - **Ethernet II & ARP (`ethernet.rs`, `arp.rs`)**: Frame encapsulation, hardware MAC resolution, and ARP table caching.
  - **IPv4 & ICMP (`ipv4.rs`, `icmp.rs`)**: Packet serialization, checksum computation, fragmentation handling, and ICMP Echo Request/Reply (`ping`).
  - **UDP & DNS (`udp.rs`)**: Connectionless datagram transport and DNS resolution (`nslookup`).
  - **TCP State Machine (`tcp.rs`)**: RFC 793 compliant 3-way handshake (`SYN` $\to$ `SYN-ACK` $\to$ `ACK`), sliding window flow control, packet sequencing, and `FIN`/`ACK` teardown.
- **BSD Socket API (`socket.rs`)**: Standard socket primitives (`socket_open`, `socket_connect`, `socket_send`, `socket_recv`, `socket_close`).
- **HTTP/1.1 Engine (`http.rs`)**:
  - Client: `curl [-i] <url>` fetching web pages and REST APIs over TCP.
  - Server: Embedded `httpd` web server serving interactive status dashboards and JSON APIs (`/api/status`).

### 7. Intel High Definition Audio & Sound Subsystem (`src/drivers/audio`)
- **Intel HDA Driver (`hda.rs`)**: Scans PCI for Intel HDA controllers (Class 0x04, Subclass 0x03), maps MMIO BAR0, executes hardware reset (`CRST`), configures Stream Descriptor 0 for 44.1 kHz 16-bit stereo PCM, and streams samples via DMA.
- **PCM Waveform Synthesizer (`pcm.rs`)**:
  - Quarter-wave integer sine lookup table (`integer_sin()`), avoiding floating point instructions in kernel space.
  - RIFF/WAVE parser supporting uncompressed 8-bit and 16-bit PCM WAV audio.
  - Tone generator (`generate_sine_wave()`) with smooth anti-pop envelopes.
  - CosmicOS Startup Chime (`generate_cosmic_chime()`): 4-note resonant harmonic F-Major chord (F3, C4, F4, A4) with natural acoustic decay.
  - Phonetic formant synthesis for text-to-speech audio.
- **PC Speaker Fallback (`mod.rs`)**: 8254 PIT Timer 2 square-wave hardware tone generation via I/O ports `0x42`, `0x43`, and `0x61`.

### 8. POSIX Shell, Pipelines, Redirection & Editor (`src/shell`)
- **POSIX Pipelines (`pipeline.rs`)**: Arbitrary multi-stage command piping (`cmd1 | cmd2 | cmd3`) with in-memory stdout interception.
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
- **In-Terminal Text Editor (`editor.rs`)**: Full-screen terminal text editor (`nano <path>` or `edit <path>`) with title header, line numbers (`001 | ...`), cursor positioning, arrow keys, Backspace, Delete, Tab, `Ctrl+S` direct VFS saving, and `Ctrl+X` exit.

### 9. Aqua Graphical Desktop (`src/gui`)
- **Bochs VBE Linear Framebuffer**: 1024x768 with 32-bit ARGB true color mapped directly into higher-half virtual memory (`0xFFFFFFFFD0000000`).
- **Double-Buffered Compositor**: Software rendering engine (`Canvas`) with anti-aliasing, rounded glass rectangles, drop shadows, and horizontal gradients.
- **Window Management**: Window chrome with titlebar dragging, active/inactive focus states, and traffic light buttons (🔴 Close, 🟡 Minimize, 🟢 Zoom).
- **Top Global Menu Bar & Bottom Glass Dock**: Application menus, live RAM status gauge, translucent dock tiles, and running app indicator dots.
- **Built-in Desktop Apps**:
  - **Terminal (`Terminal.app`)**: Graphical zsh shell session.
  - **Activity Monitor (`Activity Monitor.app`)**: Live CPU load, threads, memory pressure meter, and kernel subsystem health.
  - **Finder (`Finder.app`)**: Two-pane file manager with quick volume navigation (`Cosmic HD`, `Cosmic USB`).
  - **About CosmicOS**: Hardware specs and memory counters.

---

## Interactive Shell Command Reference (43 Commands)

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
| **Filesystem & I/O** | `ls [path]` | List contents of a directory (default `/`) |
| | `cat <path>` | Read file contents to terminal |
| | `echo <text>` | Output text to console or pipeline |
| | `touch <path>` | Create an empty file |
| | `mkdir <path>` | Create a new directory |
| | `rm <path>` | Remove a file |
| | `mount` | Display mounted filesystems (RamFS, DevFS, ProcFS, FAT32, exFAT, ext4, NTFS) |
| | `diskutil [list]` | Manage storage containers and physical partitions (`disk0`, `disk0s1`) |
| **Text Editing & Filters**| `nano <path>` / `edit` | Launch in-terminal full-screen text editor (`Ctrl+S` save, `Ctrl+X` exit) |
| | `grep [-i] [-v] <pat>` | Filter lines matching pattern (supports `-i` case-insensitive, `-v` invert) |
| | `wc [-l] [-w] [-c]` | Count lines, words, and bytes |
| | `head [-n N]` | Print first N lines of input |
| | `tail [-n N]` | Print last N lines of input |
| | `sort [-r]` | Sort lines alphabetically or reversed |
| | `uniq` | Omit repeated adjacent lines |
| | `\|`, `>`, `>>`, `<` | Shell pipelines and file redirection |
| **Network & Web** | `ifconfig` | Display network interfaces (`lo0`, `en0`, `wlan0`) and IP configurations |
| | `ping <target>` | Send ICMP ECHO_REQUEST packets to network hosts |
| | `nslookup <host>` | Resolve domain names to IPv4 addresses via DNS |
| | `wifi [subcmd]` | 802.11 Wi-Fi manager (`scan`, `connect <ssid> <pass>`, `status`, `disconnect`) |
| | `curl [-i] <url>` | Fetch web pages & REST APIs over TCP / HTTP |
| | `httpd [status]` | Embedded web server dashboard and REST API |
| **Audio & Speech** | `beep [freq] [ms]` | Play audio tone through Intel HDA / PC Speaker (default 880 Hz, 250 ms) |
| | `chime` | Play iconic CosmicOS Sublime startup chime (F-Major harmonic chord) |
| | `play <file.wav>` | Play RIFF/WAVE PCM audio file from VFS or external storage |
| | `say <text>` | Synthesize phonetic speech audio |
| **User Space & Execution**| `exec <binary>` | Execute 64-bit ELF binary in isolated Ring 3 user space |
| | `open <app/file>` | LaunchServices application launcher (e.g. `open Finder.app`) |
| | `defaults [read]` | Read user preferences and plist defaults |
| **Hardware & Power** | `lspci` | Enumerate PCI Express controller devices |
| | `gui` | Launch CosmicOS Aqua graphical desktop compositor |
| | `clear` | Clear terminal screen |
| | `test` | Run the complete automated kernel verification suite |
| | `reboot` / `halt` | ACPI power reset or shutdown |

---

## Automated Verification Suite (25 Self-Tests)

On startup, CosmicOS executes its automated self-test verification suite, thoroughly testing each subsystem:

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
  [TEST] Storage Drivers (AHCI SATA & NVMe SSD) ... [PASS]
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
   Ensure `qemu-system-x86_64` is installed and in your PATH.

### 1. Build Kernel Binary
```powershell
.\build.ps1
```
This compiles the 64-bit kernel (`target/x86_64-unknown-none/release/cosmoskernel`) with zero warnings and generates the multiboot-compatible `cosmoskernel32.elf`.

### 2. Run in QEMU

#### Terminal Mode (Headless COM1 Serial):
```powershell
.\run.ps1 -Headless
```

#### Graphical Aqua Desktop Mode (with Intel HDA sound and PS/2 mouse):
```powershell
.\run.ps1
```

---

## Booting on Real Bare-Metal Hardware

CosmicOS is tested on real Intel and AMD x86_64 PCs:
1. **Ventoy (Recommended)**:
   - Install [Ventoy](https://www.ventoy.net) on any USB flash drive.
   - Copy `cosmoskernel32.elf` into the root of the Ventoy USB drive.
   - Insert USB into any PC (UEFI or Legacy BIOS), power on, select `cosmoskernel32.elf`, and boot!
2. **GRUB2 / Rufus**:
   - Format USB as FAT32.
   - Run `.\create_boot_media.ps1 -DriveLetter E:` (replace `E:` with your USB drive letter).
   - Boot via GRUB2 multiboot loader.

---

## License
Dual-licensed under either of:
- **MIT License** ([LICENSE-MIT](LICENSE-MIT))
- **Apache License, Version 2.0** ([LICENSE-APACHE](LICENSE-APACHE))
