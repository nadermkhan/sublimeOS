# SublimeOS — KiteKernel (x86_64)

> **An Advanced, Pure-Rust 64-bit Monolithic Operating System for x86_64 Hardware & Bare-Metal Silicon.**
> **Developed by Nader Mahbub Khan · Copyright © 2026 Nader Mahbub Khan**

[![Target](https://img.shields.io/badge/target-x86__64--unknown--none-blue.svg)](https://www.rust-lang.org/)
[![Toolchain](https://img.shields.io/badge/rustc-nightly-orange.svg)](https://rust-lang.github.io/rustup/)
[![Lines of Code](https://img.shields.io/badge/lines_of_code-40%2C400+-brightgreen.svg)](#codebase-metrics--lines-of-code-loc)
[![Pure Rust](https://img.shields.io/badge/rust-98.2%25-orange.svg)](#codebase-metrics--lines-of-code-loc)
[![Tests](https://img.shields.io/badge/self--tests-55%20passing-success.svg)](#automated-verification-suite)
[![Developer](https://img.shields.io/badge/developer-Nader%20Mahbub%20Khan-blueviolet.svg)](#author--credits)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-green.svg)](#license)
[![Architecture](https://img.shields.io/badge/arch-x86__64%20(AMD64)-red.svg)](#1-architecture-boot--low-level-hardware)
[![UEFI](https://img.shields.io/badge/boot-UEFI%20GOP%20%7C%20Multiboot2-purple.svg)](#1-architecture-boot--low-level-hardware)
[![Multiprocessing](https://img.shields.io/badge/smp-Multi--Core%20%7C%20APIC-blueviolet.svg)](#1-architecture-boot--low-level-hardware)
[![Storage](https://img.shields.io/badge/storage-NVMe%20%7C%20AHCI%20%7C%20USB%20%7C%20ATA-brightgreen.svg)](#5-filesystems--real-block-storage)
[![Filesystems](https://img.shields.io/badge/filesystems-FAT32%20%7C%20exFAT%20%7C%20ext4%20%7C%20NTFS-brightgreen.svg)](#5-filesystems--real-block-storage)
[![Network](https://img.shields.io/badge/network-TCP%2FIP%20%7C%20Broadcom%20Wi--Fi%20%7C%20TLS%201.3-cyan.svg)](#6-networking-80211-wi-fi--sockets)
[![Audio](https://img.shields.io/badge/audio-Intel%20HDA%20%7C%20PCM%20%7C%20WAV-magenta.svg)](#7-intel-high-definition-audio--sound-subsystem)
[![UserSpace](https://img.shields.io/badge/userspace-ELF64%20%7C%20CoW%20Fork%20%7C%20Ring%203-yellow.svg)](#4-userspace-runtime-dynamic-linker--standard-library)

---

## Table of Contents
1. [Overview](#overview)
2. [Codebase Metrics & Lines of Code (LOC)](#codebase-metrics--lines-of-code-loc)
3. [Architectural Overview](#architectural-overview)
4. [Hardware Support Matrix](#hardware-support-matrix)
5. [Core Subsystems](#core-subsystems)
   - [1. Architecture, Boot & Low-Level Hardware](#1-architecture-boot--low-level-hardware)
   - [2. Memory Management Subsystem](#2-memory-management-subsystem)
   - [3. Task Management, Process Control & POSIX Signals](#3-task-management-process-control--posix-signals)
   - [4. Userspace Runtime, Dynamic Linker & Standard Library](#4-userspace-runtime-dynamic-linker--standard-library)
   - [5. Filesystems & Real Block Storage](#5-filesystems--real-block-storage)
   - [6. Networking, 802.11 Wi-Fi & Sockets](#6-networking-80211-wi-fi--sockets)
   - [7. Intel High Definition Audio & Sound Subsystem](#7-intel-high-definition-audio--sound-subsystem)
   - [8. SublimeSH Modern Shell & Terminal Text Editor](#8-sublimesh-modern-shell--terminal-text-editor)
   - [9. Aqua Graphical Desktop Environment](#9-aqua-graphical-desktop-environment)
   - [10. Apple Unibody & Broadcom Hardware Support](#10-apple-unibody--broadcom-hardware-support)
6. [Interactive Shell Command Reference](#interactive-shell-command-reference)
7. [Automated Verification Suite (55 Tests)](#automated-verification-suite)
8. [Building and Running](#building-and-running)
   - [Prerequisites](#prerequisites)
   - [Compiling Kernel & UEFI Bootloader](#compiling-kernel--uefi-bootloader)
   - [Launching in QEMU](#launching-in-qemu)
   - [Booting on Bare-Metal Hardware](#booting-on-bare-metal-hardware)
9. [Author & Credits](#author--credits)
10. [License](#license)

---

## Codebase Metrics & Lines of Code (LOC)

SublimeOS and KiteKernel are engineered completely from scratch in safe, idiomatic `#![no_std]` Rust with zero runtime dependencies. Below is the exact verified line count across all repository modules:

| Subsystem Component | Module Path | Language | Files | Lines of Code (LOC) | Architectural Purpose & Scope |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **KiteKernel Monolithic Core** | [`src/`](src/) | Rust (`#![no_std]`) | 137 | **37,661** | PMM/VMM, SMP, ACPI/MADT, VFS, ext4/FAT32/NTFS, Drivers, TCP/IP, Audio, Aqua GUI |
| **Pure-Rust UEFI Bootloader** | [`uefi-boot/`](uefi-boot/) | Rust (`uefi`) | 2 | **1,174** | Standalone `BOOTX64.EFI`, GOP linear framebuffer, ACPI XSDT, E820 coalescing |
| **Userspace Toolchain & Runtime**| [`userland/`](userland/) | Rust (`sublime_rt`)| 15 | **840** | Ring 3 supervisor (`/bin/init`), shell (`/bin/sh`), and 11 standalone ELF64 user binaries |
| **Low-Level CPU Assembly** | [`src/**/*.s`](src/) | x86_64 / ASM | 6 | **726** | Multiboot entry, AP 16-bit real-mode trampoline, IDT vector stubs, context switch |
| **Total Executable Codebase** | | **Rust + Assembly** | **160** | **40,428 LOC** | **98.2% Pure Rust · 1.8% Low-Level Assembly · 0 External Crates** |
| **Documentation & Firmware Specs**| [`docs/`](docs/) | Markdown / Spec | 4 | **1,250+** | Architecture manuals, command reference, test guides, and driver specs |

---

## Overview

**SublimeOS** is an advanced, monolithic, bare-metal 64-bit research operating system developed in idiomatic `#![no_std]` Rust with zero external runtime dependencies. Built on memory-safe abstractions wrapping minimal, audited `unsafe` hardware primitives (MMIO, DMA rings, assembly trampolines, and page tables), SublimeOS is powered by the **KiteKernel**. SublimeOS boots either via native UEFI (using its standalone pure-Rust `BOOTX64.EFI` loader) or standard Multiboot 1/2, transitions into 64-bit Long Mode, maps kernel space to the higher half (`0xFFFFFFFF80000000+`), and initializes a comprehensive operating system stack:

- **Native UEFI Bootloader & GOP Linear Framebuffer**: Standalone pure-Rust `#![no_std]` UEFI loader (`BOOTX64.EFI`) built directly from `uefi-boot/`. Configures UEFI Graphics Output Protocol (GOP) linear framebuffer (`1024x768x32bpp`), parses ACPI 2.0 XSDT / 1.0 RSDT tables, extracts and coalesces E820 physical memory regions, exits UEFI BootServices cleanly, transitions the CPU from 64-bit Long Mode down to 32-bit Protected Mode via a low `< 1 MiB` trampoline page, and enters the kernel via Multiboot2 (`0x36D76289`). This 64-to-32-to-64 bounce intentionally unifies the entry path between legacy BIOS Multiboot/Multiboot2 and modern UEFI, allowing the kernel bootstrap assembly (`boot.s`) to initialize page tables and Long Mode transitions identically regardless of host firmware.
- **Early GOP Framebuffer Console**: Mirrors kernel logs (`kinfo!`, `kwarn!`), panics, and the interactive shell onto the linear framebuffer using an 8x16 font with UTF-8 box-drawing/block element decoding and ANSI color sequences, providing immediate visual feedback on modern EFI machines without legacy VGA `0xB8000`.
- **Preemptive Multitasking & User Space**: Priority & round-robin scheduler with per-CPU runqueues and work-stealing load balancing across all online SMP cores, core affinity masks (`sched_setaffinity` / `sched_getaffinity`), 64-bit ELF binary loader, hardware privilege level transitions (`iretq`, `syscall`/`sysretq`), Copy-on-Write process fork (`fork`/`execve`/`waitpid`), POSIX signal delivery (`sigaction`, `sigprocmask`, `kill`), dynamic runtime linker (`ld-sublime.so`), userspace C standard library (`sublime-libc`), standalone `userland/` toolchain crate compiling genuine `#![no_std]` 64-bit ELF user binaries in `/bin` (`init`, `calc`, `hello`, `sh`, `snake`) with `sublime_rt` runtime, dynamic user heap allocation (`sys_brk`), full AMD64 SSE/SIMD hardware enablement (CR0.MP=1, CR4.OSFXSR=1), full register preservation across `syscall`/`sysretq`, and a 16-level nested user-mode execution frame stack. Boots directly into PID 1 (`/bin/init`) as the userspace supervisor, which spawns the interactive shell (`/bin/sh`) in Ring 3 (CPL 3).
- **SMP Multi-Core & APIC Architecture**: ACPI Multiple APIC Description Table (MADT) parser, Local APIC (LAPIC) high-speed timer, Inter-Processor Interrupt (IPI) dispatch, I/O APIC 64-bit redirection tables (cleanly disabling legacy 8259 PIC), and Application Processor (AP) core wakeup sequence (INIT-SIPI-SIPI).
- **Advanced Block Storage, Persistence & Partition Auto-Mounter**: Unified 512 KiB LRU Block Cache (`bcache.rs`) with dirty sector tracking, `sys_sync` (162), `sys_fsync` (74), and transparent partition device caching; PCIe NVMe controller driver with circular Admin/IO submission and completion queues, AHCI SATA controller driver with 32-slot command lists and PRDT DMA engines, ATA/IDE PIO LBA48, USB Mass Storage (SCSI / Bulk-Only Transport over xHCI / EHCI), and real partition table auto-mounter scanning MBR (primary and EBR chains) and GPT into `/Volumes/`.
- **Filesystems & VFS**: Pluggable Virtual File System (`/System`, `/Library`, `/Applications`, `/Users`, `/Volumes`, `/dev`, `/proc`), read/write FAT32 with cluster allocation bitmap, exFAT parser, Linux ext4 superblock/extent-tree resolver with HTree indexing, and Windows NTFS decoding supporting both resident `$DATA` records and non-resident variable-length runlist decoding (`decode_runlist`) for multi-extent fragmented files.
- **Network Protocol Stack & Broadcom Silicon**: Full TCP/IP stack (Ethernet II, ARP, IPv4, ICMP Ping, UDP, RFC 793 TCP state machine, BSD Socket API), IEEE 802.11 Wi-Fi subsystem (beacon frame parsing, 4-way handshake, WPA2-PSK with PBKDF2-SHA1), Intel e1000 Gigabit NIC, Broadcom NetXtreme BCM57765 Gigabit Ethernet (`tg3`), Broadcom BCM4331 802.11n Wi-Fi (`b43` with in-tree compile-time `ucode16_mimo.fw` ingestion and PSM SHM upload), Realtek, Qualcomm Atheros, HTTP/1.1 client (`curl`), embedded HTTP server (`httpd`), and TLS 1.3 client engine (X25519 ECDH, ChaCha20-Poly1305) operating in opportunistic encryption mode without X.509 PKI certificate chain or root CA verification.
- **USB 2.0 EHCI, Hub Class Driver & Apple Input Hardware**: EHCI Periodic/Async DMA engines, comprehensive USB 2.0 Hub Class driver (Class `0x09`) supporting downstream multi-port hubs (such as Intel 6/7-series Rate Matching Hubs `0x8087:0x0024`), `DeviceRemovable` port bitmasks, power-on-to-power-good stabilization delay, port reset debouncing and 10 ms recovery (`TRSTRCY`), multi-tier Split Transaction Translator (TT) inheritance, depth recursion bounds, automatic physical Queue Head programming (Mult=1, C-Mask=0x1C, S-Mask=0x01, TT HubAddr/PortNum), USB HID Boot Protocol keyboard, Apple multi-touch trackpad decoding with boot mouse emulation, Apple System Management Controller (SMC) keyboard backlight control (`/dev/backlight`), and complete device tree topology diagnostics via `system usb` and `lsusb`.
- **Intel HDA Audio Subsystem**: Intel High Definition Audio controller driver, pure integer quarter-wave sine synthesis, RIFF/WAVE PCM parser, PC speaker fallback, and the iconic SublimeOS startup chime.
- **POSIX Shell & Editor**: SublimeSH terminal shell (Unix/POSIX-style shell with zsh-inspired syntax and keybindings) with multi-stage pipelines (`|`), file redirection (`>`, `>>`, `<`), Unix filters (`grep`, `wc`, `head`, `tail`, `sort`, `uniq`), and full-screen in-terminal text editor (`nano` / `edit`).
- **Aqua Graphical Desktop**: 1024x768x32bpp double-buffered compositor, glass dock, top global menu bar, window manager with traffic light controls, Finder, Terminal, and Activity Monitor.

---

## Architectural Overview

```text
                    +-------------------------------------------------------------+
                    |                SublimeOS Aqua Desktop Compositor             |
                    |        (1024x768x32bpp TrueColor Double-Buffered Canvas)     |
                    |        - Top Menu Bar, Glass Dock, Window Manager, Finder   |
                    +------------------------------+------------------------------+
                                                   |
                    +------------------------------+------------------------------+
                    |              SublimeSH Interactive Diagnostic Shell          |
                    |   - Pipelines (|), File Redirection (>, >>, <), Nano Editor |
                    |   - UNIX Filters: grep, wc, head, tail, sort, uniq          |
                    +------------------------------+------------------------------+
                                                   |
     +---------------------------------------------+---------------------------------------------+
     |                               Virtual File System (VFS)                                   |
     |   +-------------+  +-------------+  +------------+  +-------------+  +----------------+   |
     |   |    RamFS    |  |  FAT32 R/W  |  |   exFAT    |  |  ext4 extents|  |  NTFS Resident |   |
     |   | (/tmp,/etc) |  | (/Volumes/*)|  | (Heap LBA) |  | (Superblock)|  | & Runlist Ext. |   |
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
     | - Dynamic Linker   | - sublime_context_switch (assembly)             | - Sockets, curl,    |
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

## Hardware Support Matrix

To provide an accurate technical assessment of driver readiness, SublimeOS categorizes hardware drivers by verified operational tier across physical x86_64 silicon and virtualized environments:

| Subsystem / Controller | Hardware / Target | Verification Tier | Operational Status & Capabilities |
| :--- | :--- | :--- | :--- |
| **CPU / SMP APIC** | x86_64 / Intel Core i7-2760QM | **Bare Metal & QEMU** | Multi-core AP bring-up (INIT-SIPI-SIPI), Local APIC timer, I/O APIC redirection, 8259 PIC blinding |
| **Firmware & Graphics** | Apple EFI 1.10 / UEFI 2.x GOP | **Bare Metal & QEMU** | Pure-Rust `BOOTX64.EFI`, GOP linear framebuffer (`1024x768x32bpp`), early text console |
| **Ethernet NIC (tg3)** | Broadcom BCM57765 (`14E4:16B0`) | **Bare Metal Verified** | 64-bit BAR0 MMIO, hardware Status Block mailboxes, Producer/Consumer DMA rings, MAC extraction |
| **Wi-Fi 802.11n (b43)** | Broadcom BCM4331 (`14E4:4331`) | **Bare Metal Verified** | BCMA backplane window switching, compiled ucode ingestion, PSM SHM upload, DMA64 queues |
| **Apple SMC & gmux** | Apple MacBookPro8,2 | **Bare Metal Verified** | Direct I/O port command/data handshakes (`0x300`/`0x304`), `/dev/backlight` keyboard backlight PWM |
| **USB 2.0 Host (EHCI)** | Intel 6-Series / C200 Chipset | **Bare Metal Verified** | Periodic Frame List, Async Queue Heads, hub split-transactions (SSPLIT/CSPLIT), HID input |
| **USB HID Input** | Apple Internal Keyboard & Trackpad | **Bare Metal Verified** | Boot Protocol keyboard parsing, multi-touch trackpad packet decoding, coordinate updates |
| **NVMe Controller** | PCIe NVMe SSD (v1.3+) | **QEMU Verified** | Circular Admin & I/O Submission/Completion Queue rings, Doorbell stride, block DMA |
| **AHCI SATA Controller**| Intel ICH9 / AHCI 1.3 | **QEMU Verified** | 32-slot command lists, Received FIS, PRDT DMA scatter-gather engines, LBA48 read/write |
| **Gigabit Ethernet** | Intel 82540EM / 82545EM (`e1000`) | **QEMU Verified** | MMIO register programming, hardware TX/RX ring buffers, link auto-negotiation |
| **Audio Controller** | Intel High Definition Audio (HDA) | **QEMU Verified** | Controller reset (`CRST`), Stream Descriptor 0 DMA buffers, 44.1 kHz 16-bit stereo PCM |
| **Display Emulation** | Bochs / QEMU VBE (`0x1234:0x1111`)| **QEMU Verified** | Linear framebuffer (`1024x768x32bpp`), software double-buffered compositor |
| **Fast / GbE NICs** | Realtek RTL8139 / RTL8169 | **Emulation Verified** | MMIO/PIO register switching, hardware reset, MAC extraction, 18 KiB RX ring, 4-desc TX |
| **Ethernet / Wi-Fi** | Qualcomm Atheros AR8151 / AR9285 | **Emulation Verified** | `ALX_MASTER` reset, station address parsing, central `NET_MANAGER` packet routing |

---

## Core Subsystems

### 1. Architecture, Boot & Low-Level Hardware
*Location: [`src/arch/x86_64/`](src/arch/x86_64/), [`uefi-boot/`](uefi-boot/), [`src/drivers/gop_console.rs`](src/drivers/gop_console.rs)*

- **Pure-Rust UEFI Multiboot2 Bootloader ([`uefi-boot/`](uefi-boot/))**: Standalone `#![no_std]` 64-bit UEFI application (`BOOTX64.EFI`) built without external toolchains or C dependencies. Locates the kernel ELF image on the EFI System Partition (`\EFI\BOOT\`), negotiates UEFI Graphics Output Protocol (GOP) video modes (`1024x768x32bpp`), extracts ACPI 2.0 XSDT / 1.0 RSDT root pointers, collects and coalesces fragmented EFI memory descriptors into standard E820 ranges, safely exits UEFI BootServices, transitions the CPU from 64-bit Long Mode down to 32-bit Compatibility Mode, disables paging and long mode in a low `< 1 MiB` trampoline page, stages ELF `PT_LOAD` segments, and jumps to kernel `_start` with `EAX = 0x36D76289`. This 64-to-32-to-64 transition unifies the kernel bootstrap architecture, ensuring identical page-table setup and Long Mode transitions in `boot.s` for both modern UEFI and legacy Multiboot firmware.
- **Early GOP Linear Framebuffer Console ([`gop_console.rs`](src/drivers/gop_console.rs))**: Real-time graphical console streaming kernel diagnostics, `kinfo!`/`kwarn!` logs, and early panic output directly to the active UEFI linear framebuffer (`0xFFFFFFFFD0000000`). Features an 8x16 monospace font, full ANSI color sequence interpretation, multi-byte UTF-8 stream decoding, and aligned box-drawing (`═`, `║`, `╔`, `╗`, `╚`, `╝`) / block element (`█`, `▀`, `▄`, `▌`, `▐`) renderers for modern EFI hardware lacking legacy VGA `0xB8000` text memory.
- **Long Mode Trampoline ([`boot.s`](src/arch/x86_64/boot.s))**: Multiboot 1 and 2 compliant. Verifies CPUID and Long Mode capability, constructs initial 4-level PML4 page tables mapping the first 1 GiB identity-mapped, the 1 GiB higher-half kernel image (`0xFFFFFFFF80000000`), and a 64 GiB Higher-Half Direct Map (HHDM at `0xFFFF800000000000`) using 2 MiB huge pages, enables PAE, `EFER.LME`, and `CR0.PG/WP`, and transitions seamlessly into 64-bit Rust.
- **Global Descriptor Table (GDT) & TSS ([`gdt.rs`](src/arch/x86_64/gdt.rs))**: Configures Kernel Code (`0x08`), Kernel Data (`0x10`), User Data (`0x1B`, DPL 3), User Code (`0x23`, DPL 3), and Task State Segment (`0x28`). Installs dedicated Interrupt Stack Tables (`IST1` for Double Fault, `IST2` for Page Fault).
- **Interrupt Descriptor Table (IDT) ([`idt.rs`](src/arch/x86_64/idt.rs))**: 256 vector gates handling all x86_64 CPU exceptions with complete register/CR2 state dumps, hardware IRQs 32..47, and `int 0x80` syscall vector. Includes trap isolation for user faults (`#PF`, `#GP`, `#UD`, `#DE`) to prevent kernel panics. Dispatches Local APIC End-of-Interrupt (EOI) tokens to MMIO register `0x0B0` on all hardware IRQs while completely blinding legacy 8259 PIC vectors.
- **Fast AMD64 System Calls ([`syscall.rs`](src/arch/x86_64/syscall.rs), [`syscall_entry.s`](src/syscall/syscall_entry.s))**: Configures MSRs `IA32_EFER.SCE`, `IA32_STAR`, `IA32_LSTAR`, and `IA32_FMASK` for hardware-accelerated user/kernel privilege transitions via `syscall` / `sysretq`.
- **ACPI & MADT Table Parser ([`acpi.rs`](src/arch/x86_64/acpi.rs), [`madt.rs`](src/arch/x86_64/madt.rs))**: Parses RSDP, RSDT, XSDT, FADT, and MADT (Multiple APIC Description Table), enumerating physical CPU cores, Local APIC base (`0xFEE00000`), I/O APIC physical address (`0xFEC00000`), and Interrupt Source Overrides.
- **Local APIC & I/O APIC Drivers ([`apic.rs`](src/arch/x86_64/apic.rs), [`ioapic.rs`](src/arch/x86_64/ioapic.rs))**: Maps LAPIC and I/O APIC MMIO registers, programs Spurious Vector Register (SVR `0x1FF`), Task Priority Register (TPR), APIC timer, Inter-Processor Interrupt (IPI) dispatch via ICR, configures 64-bit Redirection Table entries, and executes absolute legacy 8259 PIC blinding (`0x21=0xFF`, `0xA1=0xFF`).
- **SMP Multiprocessing Architecture ([`smp.rs`](src/arch/x86_64/smp.rs))**: Bootstrap Processor (BSP) detection and Application Processor (AP) core wakeup sequence using the INIT-SIPI-SIPI inter-processor interrupt protocol via 16-bit real-mode trampoline at `0x8000`.
- **Serial & VGA Text Display ([`serial.rs`](src/arch/x86_64/serial.rs), [`vga.rs`](src/arch/x86_64/vga.rs))**: 16550A UART driver with IRQ-driven ring buffers for COM1 and COM2; 80x25 color VGA text driver with scrolling, ANSI escape color rendering, and thread-safe stdout capture hooks.

### 2. Memory Management Subsystem
*Location: [`src/mm/`](src/mm/)*

- **Physical Memory Manager (PMM) ([`pmm.rs`](src/mm/pmm.rs))**: Dynamic bitmap allocator and per-frame reference tracker managing 4 KiB physical frames dynamically scaled from hardware E820 / UEFI memory descriptors across a 64 GiB architectural boundary (`MAX_MANAGEABLE_PHYS = 64 GiB`, 16,777,216 frames) backed by 2 MiB Higher-Half Direct Paging (HHDM at `0xFFFF800000000000`). Dynamically places bitmap and per-frame refcounts in physical RAM above the kernel image to eliminate static BSS bloat on small VMs while supporting systems with 16–64 GiB of physical memory and preserving transparent Copy-on-Write sharing across forked processes.
- **Virtual Memory Manager (VMM) ([`vmm.rs`](src/mm/vmm.rs))**: 4-level paging manager (PML4, PDPT, PD, PT). Supports dynamic page mapping/unmapping, address translation, TLB invalidation (`invlpg`), non-cacheable MMIO mapping ([`map_mmio_range`](src/mm/vmm.rs)), and page fault handling for Copy-on-Write ([`handle_cow_fault`](src/mm/vmm.rs)) with in-place permission promotion when reference count is 1.
- **Kernel Heap ([`heap.rs`](src/mm/heap.rs))**: Expandable `GlobalAlloc` implementation backed by PMM frames. Unlocks Rust `alloc` data structures (`Vec`, `Box`, `String`, `Arc`, `format!`).
- **User Address Space ([`address_space.rs`](src/mm/address_space.rs))**: Per-process PML4 address spaces with kernel higher-half clone, Virtual Memory Areas (VMAs), POSIX `brk()` heap expansion, direct user memory patching ([`write_user_u64`](src/mm/address_space.rs)), and Copy-on-Write address space cloning ([`clone_cow`](src/mm/address_space.rs)).
- **POSIX Shared Memory & Memory Mapping ([`shm.rs`](src/mm/shm.rs), [`process.rs`](src/task/process.rs))**:
  - `shm_open()` (via `/dev/shm/*` and `sys_shm_open`): Creates or opens named in-memory shared memory objects backed by physical PMM frames.
  - `ftruncate()` (`sys_ftruncate` Syscall 77): Resizes shared memory objects, dynamically allocating or freeing 4 KiB physical frames.
  - `mmap()` (`sys_mmap` Syscall 9, `MAP_SHARED` / `MAP_PRIVATE` / `MAP_ANONYMOUS`) and `munmap()` (`sys_munmap` Syscall 11): Maps shared physical frames or anonymous memory directly into the process PML4 address space and records Virtual Memory Areas (`VirtualMemoryArea`). Enables zero-copy Inter-Process Communication (IPC) across independent address spaces.
  - `shm_unlink()`: Removes named objects from the global registry (`SHM_REGISTRY`) and `/dev/shm`.
- **Demand Paging & Swap Subsystem ([`vma.rs`](src/mm/vma.rs), [`swap.rs`](src/mm/swap.rs))**:
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
- **POSIX Process Groups, Sessions & Job Control ([`process.rs`](src/task/process.rs), [`job.rs`](src/shell/job.rs))**:
  - Process Groups (`pgid`) and Session IDs (`sid`): `sys_setpgid` (109), `sys_getpgrp` (111), `sys_getpgid` (121), `sys_setsid` (112), and `sys_getsid` (124).
  - Terminal foreground process group management (`TIOCGPGRP` / `TIOCSPGRP`) over PTY master/slave devices.
  - Signal broadcasting to process groups (`sys_kill(-pgid, sig)`), terminal `Ctrl+Z` interception sending `SIGTSTP` to suspend foreground jobs into `TaskState::Stopped`, and shell job commands (`jobs`, `fg`, `bg`).
- **POSIX Signals Subsystem ([`signal.rs`](src/ipc/signal.rs))**:
  - Per-process signal action tables ([`SigAction`](src/ipc/signal.rs)), blocked signal bitmasks, pending signals delivery, asynchronous inter-process signaling ([`sys_kill`](src/task/process.rs)), child termination notification (`SIGCHLD` to parent), and unblockable signals (`SIGKILL`, `SIGSTOP`).
  - Syscalls: `sys_rt_sigaction` (13), `sys_rt_sigprocmask` (14), and `sys_kill` (62).
- **POSIX Semaphores & Message Queues ([`semaphore.rs`](src/ipc/semaphore.rs), [`mqueue.rs`](src/ipc/mqueue.rs))**:
  - **Named Semaphores**: `sem_open` (240), `sem_close` (241), `sem_post` (242), `sem_wait` (243), `sem_trywait` (244), `sem_getvalue` (245), `sem_unlink` (246), and `/dev/sem/*` virtual VFS filesystem integration.
  - **POSIX Message Queues**: `mq_open` (247), `mq_send` (248), `mq_receive` (249), `mq_unlink` (250), `mq_getattr` (251), and `/dev/mqueue/*` VFS directory node. Supports priority-ordered message insertion (higher priority popped first) and configurable queue limits (`mq_maxmsg`, `mq_msgsize`).
- **Kernel POSIX Threads & Futexes ([`process.rs`](src/task/process.rs), [`futex.rs`](src/ipc/futex.rs))**:
  - `sys_clone()` (Syscall 56): Linux-compatible process and kernel thread cloning supporting `CLONE_VM` (shared PML4 address space), `CLONE_FILES` (shared file descriptor table), `CLONE_FS`, `CLONE_SIGHAND`, `CLONE_THREAD`, `CLONE_PARENT_SETTID`, and `CLONE_CHILD_SETTID`. Enables multi-threaded execution with shared VM.
  - `sys_futex()` (Syscall 202): Fast userspace locking mechanism implementing `FUTEX_WAIT` (atomic value check and wait queue), `FUTEX_WAKE` (waking waiting threads), and `FUTEX_REQUEUE` (requeueing waiters across futex words for condition variables).

### 4. Userspace Runtime, Dynamic Linker & Standard Library
*Location: [`src/user/`](src/user/), [`userland/`](userland/)*

- **Standalone Userland Toolchain & Runtime (`userland/`, `sublime_rt`)**: Standalone `#![no_std]` Rust crate compiling genuine 64-bit ELF executables targeting `x86_64-unknown-linux-none` linked with `user.ld` (`0x00400000` base). The `sublime_rt` minimal runtime provides standard SysV AMD64 CRT entry (`_start`), command-line argument extraction (`args()`), directory iteration (`read_dir()`), file I/O helpers (`read_to_string()`, `write_file()`), `print!`/`println!` macros, POSIX syscall wrappers (`read`, `write`, `open`, `close`, `exit`, `brk`, `fork`, `waitpid`, `getpid`, `sched_yield`, `sleep_ms`, `sync`, `spawn`, `syscommand`, `shutdown`), and a `GlobalAlloc` user bump allocator backed by `sys_brk`.
- **System V AMD64 ABI Initial User Stack Setup & Strict Alignment**: When loading 64-bit ELF user processes, the kernel initializes the user stack in compliance with the AMD64 ABI: `[argc, argv[0]..argv[N], NULL, envp NULL, string data]`. The stack pointer is aligned such that `rsp % 16 == 8` upon entry to `_start`, ensuring that `pop rdi` aligns `rsp` to 16 bytes prior to function calls (`call cosmic_entry`), completely eliminating `#GP(0)` faults on 16-byte aligned vector instructions (`movaps`, `movups`).
- **Linux-Compatible Directory Enumeration (`SYS_GETDENTS64`)**: Implements POSIX/Linux Syscall 217 (`sys_getdents64`), packing dynamic 64-bit dirent records (`d_ino`, `d_off`, `d_reclen`, `d_type`, `d_name[]`) directly from underlying VFS inode traversal (`InodeOperations::readdir()`) into userspace memory buffers.
- **Filesystem Creation & Truncation (`O_CREAT` / `O_TRUNC`)**: `sys_open` supports standard POSIX flags `O_CREAT` (0x40) to dynamically create regular files within parent directory inodes and `O_TRUNC` (0x200) to reset existing file contents to zero length.
- **Userspace Master Boot (PID 1 `/bin/init`) & Interactive Shell (`/bin/sh`)**: The kernel transitions from self-tests directly to userspace by launching `/bin/init` strictly as **PID 1 in Ring 3** (CPL 3). Acting as system supervisor, PID 1 manages child process lifecycles and spawns `/bin/sh` (PID 2) in Ring 3. The userspace shell provides built-in commands (`help`, `clear`, `echo`, `pwd`, `uname`, `sync`, `exit`), direct execution of user binaries (`calc`, `hello`, `snake`, `echo`, `cat`, `ls`, `grep`, `wc`, `edit`), execution of arbitrary binaries via `exec <path>`, and routing of administrative/telemetry commands through `syscommand`.
- **Hardware SIMD & Syscall ABI Hardening**: Kernel architecture initialization enables SSE / SIMD instructions across all BSP and AP cores (CR0.MP=1, CR0.EM=0, CR4.OSFXSR=1, CR4.OSXMMEXCPT=1), allowing userspace binaries to execute 128-bit vector instructions (`movups`, `movaps`) without `#UD` exceptions. Full general-purpose register preservation (`rdi`, `rsi`, `rdx`, `r10`, `r8`, `r9`, `r12..r15`, `rbx`, `rbp`) across `syscall_entry.s` and `sysretq` guarantees compliance with standard compiler code generation, while a 16-level nested execution stack in `user_switch.s` enables nested process execution without clobbering parent states.
- **Blocking Standard Input (`sys_read`)**: TTY console read calls (`read(0, buf, count)`) block cleanly until incoming keyboard or serial input is available, yielding the CPU to the multitasking scheduler and ensuring low CPU utilization when awaiting input.
- **Dynamic Linker & Shared Libraries ([`linker.rs`](src/user/linker.rs))**: Dynamic runtime linker resolving shared library dependencies from `/lib`, parsing export symbols, and applying AMD64 relocations (`R_X86_64_JUMP_SLOT`, `R_X86_64_GLOB_DAT`, `R_X86_64_64`, `R_X86_64_RELATIVE`) by patching Global Offset Table (GOT) slots in user memory.
- **Userspace C Standard Library (`sublime-libc`, [`libc.rs`](src/user/libc.rs))**: Standard userspace runtime implementing POSIX syscall wrappers (`syscall0`..`syscall3`):
  - File I/O: `write()`, `read()`, `open()`, `close()`.
  - Memory Management: `brk()` and `malloc()` / `free()` heap allocator.
  - Process Credentials & Control: `fork()`, `waitpid()`, `execve()`, `kill()`, `sigaction()`, `sigprocmask()`, `signal()`, `getpid()`, `uname()`, `nanosleep()`, and `exit()`.
  - Console Printing: `puts()`, `putchar()`, ANSI formatting.
- **Native Userland Binaries (`/bin`)**:
  - `/bin/init`: Standalone compiled system init executable validating Ring 3 user privileges, `getpid()`, dynamic `brk()` heap expansion, and clean process termination.
  - `/bin/calc`: Standalone compiled 64-bit user arithmetic engine computing multiplication, addition, and vector calculations entirely in Ring 3.
  - `/bin/hello`: Standalone compiled executable validating dynamic user heap vector allocations (`Vec`), string formatting (`format!`), and process exit codes.
  - `/bin/echo`: Standalone compiled utility printing command-line arguments to stdout separated by spaces with trailing newline.
  - `/bin/cat`: Standalone compiled concatenation tool reading standard input or one or more file paths and writing contents to stdout.
  - `/bin/ls`: Standalone compiled directory listing tool using `SYS_GETDENTS64` to inspect directory hierarchies and format entries.
  - `/bin/grep`: Standalone compiled regular string search tool scanning files line-by-line and printing matching occurrences.
  - `/bin/wc`: Standalone compiled word counter printing newline count, word count, and byte count for target files.
  - `/bin/edit`: Standalone compiled modal in-terminal text editor supporting interactive line input, document viewing (`:p`), and file saving (`:w`, `:wq`) via `O_CREAT` / `O_TRUNC`.
  - `/bin/sh`: Isolated interactive user shell executing in Ring 3 with terminal prompts, POSIX argument parsing, and standard I/O bridging.
  - `/bin/snake`: Full-screen ASCII arcade game running in Ring 3 utilizing ANSI escape codes and PIT sleep timers.
  - `/bin/dyntest`: Dynamically linked executable requesting `/lib/ld-sublime.so` interpreter, dynamically importing and linking `cosmic_puts`, `cosmic_pow`, and `cosmic_exit` across shared libraries.
  - `/bin/windowserver`: Aqua display server daemon opening `/dev/fb0`, configuring double-buffered video via `ioctl`, and compositing desktop UI elements via `/lib/libgui.so`.
  - `/bin/telnetd`: Ring 3 Telnet daemon creating TCP server sockets, binding to port 23, and servicing remote terminal shell sessions over standard VFS file descriptors.
  - `/bin/sshd`: Ring 3 Secure Shell daemon binding to TCP port 22, exchanging `SSH-2.0-CosmicSSH_1.0` protocol banners, and validating remote client authentication.
- **Shared Libraries (`/lib`)**:
  - `/lib/ld-sublime.so`: Dynamic linker runtime image.
  - `/lib/libc.so`: Shared C standard library exporting `cosmic_puts` and `cosmic_exit`.
  - `/lib/libm.so`: Shared math library exporting `cosmic_pow` (integer exponentiation).

### 5. Filesystems & Real Block Storage
*Location: [`src/fs/`](src/fs/), [`src/drivers/storage/`](src/drivers/storage/)*

- **Unified Block Cache & Persistence Subsystem ([`bcache.rs`](src/fs/bcache.rs))**: 1024-sector (512 KiB) LRU Buffer Cache caching underlying physical and virtual block devices. Implements sector read caching, write-back / write-through with dirty sector bitmap, `flush_all()`, `CachedBlockDevice` transparent wrapper for disk partitions, POSIX `sys_sync` (Syscall 162) and `sys_fsync` (Syscall 74), and shell commands (`sync`, `storage cache`, `storage sync`).
- **AHCI SATA Controller Driver ([`ahci.rs`](src/drivers/storage/ahci.rs))**: PCI discovery (Class 0x01, Subclass 0x06), ABAR (BAR5) MMIO mapping, Generic Host Control (GHC.AE AHCI enable), Command List headers (32 slots), Received FIS structure, Command Tables with Physical Region Descriptor Tables (PRDT), and ATA DMA commands (`IDENTIFY DEVICE`, `READ DMA EXT`, `WRITE DMA EXT`).
- **PCIe NVMe Controller Driver ([`nvme.rs`](src/drivers/storage/nvme.rs))**: PCI discovery (Class 0x01, Subclass 0x08), 64-bit BAR0 MMIO mapping, Controller Configuration (`CC.EN`), Admin Submission & Completion Queue rings (ASQ/ACQ), Doorbell Stride dispatching, Admin Identify commands (`Identify Controller`, `Identify Namespace 1`), and I/O Submission/Completion Queue creation for direct NVMe block Read/Write commands.
- **Unified Block Device Interface ([`BlockDevice`](src/drivers/storage/mod.rs))**: Pluggable storage abstraction supporting sector reads/writes across ATA IDE, AHCI SATA, NVMe SSDs, USB mass storage drives, and memory-backed block devices.
- **FAT32 R/W ([`fat32.rs`](src/fs/fat32.rs))**: Real Read/Write implementation with partition discovery, BPB decoding, cluster allocation bitmap scanning, cluster chain traversal, LFN directory parsing, and file creation/overwriting.
- **exFAT ([`exfat.rs`](src/fs/exfat.rs))**: Sector-accurate exFAT parser supporting Main Boot Region, Volume Flags, Cluster Heap allocation, Stream Extension entries, and file traversal.
- **ext4 R/W & HTree Directory Indexing ([`ext4.rs`](src/fs/ext4.rs))**: Linux ext4 superblock parser, Block Group Descriptor Table lookup, block and inode bitmap allocation, extent tree mapping, direct write block updates, directory entry insertion, file creation, and full **HTree (B-Tree) Indexing** (`EXT4_INDEX_FL = 0x1000`). Implements `dx_root` header parsing, `dx_countlimit`, `dx_entry` binary search, multi-level tree traversal, and standards-compliant directory hashing (Half MD4, TEA, and Legacy hashes) with superblock seed integration (`s_hash_seed`), achieving fast $O(\log N)$ file lookups with collision spillover tolerance and clean directory enumeration.
- **NTFS ([`ntfs.rs`](src/fs/ntfs.rs))**: Windows NTFS VBR parsing, `$MFT` record decoding, resident `$DATA` extraction, and non-resident variable-length data runlist decoder (`decode_runlist`). Supports multi-extent fragmented cluster chains, signed LCN relative offsets, sparse zero-filled extents, and sector-by-sector DMA streaming for files of arbitrary length.
- **USB Mass Storage ([`usb/msc.rs`](src/drivers/usb/msc.rs))**: Bulk-Only Transport (BOT) driver sending SCSI Command Descriptor Blocks (`INQUIRY`, `READ CAPACITY 10`, `READ 10`, `WRITE 10`) over xHCI and EHCI controllers.
- **USB HID Input Subsystem ([`usb/hid/`](src/drivers/usb/hid/))**: USB Human Interface Device driver supporting Boot Protocol keyboards and mice over xHCI and EHCI controllers. Parses 8-byte keyboard reports, maps USB HID Usage codes to PS/2 scancodes with modifier tracking, and translates 3/4-byte mouse packets into desktop coordinate updates.

### 6. Networking, 802.11 Wi-Fi & Sockets
*Location: [`src/net/`](src/net/), [`src/drivers/net/`](src/drivers/net/)*

- **IEEE 802.11 Wi-Fi ([`wifi.rs`](src/net/wifi.rs))**: Beacon frame management (SSID, BSSID, RSSI, WPA2-RSN capabilities), WPA2-PSK authentication, and 4-way handshake state machine.
- **Network Cryptography ([`crypto.rs`](src/net/crypto.rs))**: Pure Rust SHA-1, HMAC-SHA1, and PBKDF2 key derivation (4096 iterations) for WPA2 Pairwise Master Key generation.
- **TCP/IP Protocol Stack**:
  - **Ethernet II & ARP ([`ethernet.rs`](src/net/ethernet.rs), [`arp.rs`](src/net/arp.rs))**: Frame encapsulation, hardware MAC resolution, and ARP table caching.
  - **IPv4 & ICMP ([`ip.rs`](src/net/ip.rs), [`icmp.rs`](src/net/icmp.rs))**: Packet serialization, checksum computation, fragmentation handling, and ICMP Echo Request/Reply (`ping`).
  - **UDP & DNS ([`udp.rs`](src/net/udp.rs), [`dns.rs`](src/net/dns.rs))**: Connectionless datagram transport and DNS resolution (`nslookup`).
  - **TCP State Machine ([`tcp.rs`](src/net/tcp.rs))**: RFC 793 compliant 3-way handshake (`SYN` $\to$ `SYN-ACK` $\to$ `ACK`), sliding window flow control, packet sequencing, and `FIN`/`ACK` teardown.
- **BSD Socket API & POSIX System Calls ([`socket.rs`](src/net/socket.rs))**:
  - Full suite of BSD-compliant socket system calls: `sys_socket` (41), `sys_connect` (42), `sys_accept` (43), `sys_sendto` (44), `sys_recvfrom` (45), `sys_bind` (49), `sys_listen` (50).
  - VFS `SocketInode` integration enabling standard `read()` and `write()` calls directly on socket file descriptors (`fd > 2`).
  - RFC 793 incoming connection handling for listening sockets: child socket allocation, `SYN-ACK` replies, and queueing into listener's `pending_accept` backlog.
- **Unix Domain Sockets & IPC Streams ([`unix.rs`](src/net/unix.rs))**:
  - Standard `AF_UNIX` / `AF_LOCAL` local socket IPC (`SockAddrUn`, filesystem namespace path binding).
  - Supports bidirectional in-memory byte streams (`SOCK_STREAM`) and datagram packets (`SOCK_DGRAM`) with full connection lifecycle (`bind`, `listen`, `connect`, `accept`, `send`, `recv`, `close`) and VFS `FileType::Socket` integration (`UnixSocketInode`).
- **DNS Client Cache, Multi-Nameserver Failover & LLMNR ([`dns.rs`](src/net/dns.rs))**:
  - **DNS Cache with TTL**: In-memory DNS cache (`DNS_CACHE`) storing resolved IPv4 records with real-time TTL expiration tied to kernel RTC timestamps.
  - **Multi-Nameserver Failover**: Parses `/etc/resolv.conf` (`nameserver`, `search`), managing multi-server configurations (`DNS_CONFIG`) with failover support.
  - **RFC 4795 LLMNR**: Link-Local Multicast Name Resolution listening on UDP port 5355 (`224.0.0.252` / MAC `01:00:5E:00:00:FC`), answering single-label queries (`cosmos`, `KiteKernel`, `cosmos.local`).
- **Network Socket Server Daemons**:
  - `/bin/telnetd`: Background server daemon listening on TCP port 23, spawning interactive sessions for incoming telnet clients.
  - `/bin/sshd`: Background server daemon listening on TCP port 22 with SSH-2.0 protocol identification and authentication handling.
  - Shell `telnet` client command enabling remote terminal connections from SublimeSH.
- **HTTP/1.1 & TLS 1.3 HTTPS Engine ([`http.rs`](src/net/http.rs), [`tls.rs`](src/net/tls.rs))**:
  - Pure `#![no_std]` TLS 1.3 client stack implementing RFC 8446 handshake with X25519 ECDH key agreement and ChaCha20-Poly1305 AEAD record encryption/decryption.
  - SHA-256, HMAC-SHA256, and HKDF-Extract/Expand key schedule derivation.
  - *Cryptographic Limitation*: Operates in opportunistic encryption mode; does not validate X.509 certificate chains or authenticate against a root CA store.
  - Client: `curl [-i] <url>` fetching web pages and REST APIs over plain HTTP or secure HTTPS (`curl https://...`).
  - Server: Embedded `httpd` web server serving interactive status dashboards and JSON APIs (`/api/status`).
- **mDNS & Apple Bonjour ZeroConf Responder ([`mdns.rs`](src/net/mdns.rs))**:
  - RFC 6762 / RFC 6763 compliant Multicast DNS responder listening on UDP port 5353 (`224.0.0.251` / MAC `01:00:5E:00:00:FB`).
  - Resolves `SublimeOS.local` to the system's IPv4 address (`A` record).
  - DNS Service Discovery (DNS-SD) advertising running system daemons: HTTP Web Server (`_http._tcp.local`, port 80), SSH Remote Shell (`_ssh._tcp.local`, port 22), and Telnet Server (`_telnet._tcp.local`, port 23), along with service enumeration (`_services._dns-sd._udp.local`).
  - Generates authoritative answer frames with PTR, SRV, TXT, and A resource records.
- **DHCP Client Daemon ([`dhcp.rs`](src/net/dhcp.rs))**:
  - RFC 2131 / RFC 2132 compliant dynamic host configuration client running over UDP client port 68 / server port 67 with magic cookie `0x63825363`.
  - Full state machine (`Init`, `Selecting`, `Requesting`, `Bound`), handling `DHCPDISCOVER`, `DHCPOFFER`, `DHCPREQUEST`, `DHCPACK`, and `DHCPRELEASE`.
  - Automatic dynamic lease acquisition: extracts assigned IPv4, subnet mask, default gateway router, DNS nameserver, server identifier, and lease duration, automatically applying network configurations into `NET_MANAGER`.
  - Interactive shell management via `dhcp` command (`dhcp status`, `dhcp discover [interface]`, `dhcp release`).
- **NTP Network Time Synchronization Daemon ([`ntp.rs`](src/net/ntp.rs))**:
  - RFC 5905 compliant Network Time Protocol (NTPv4) and Simple Network Time Protocol (SNTP) client daemon operating on UDP port 123.
  - Constructs 48-byte NTP client requests with leap indicator (`LI=0`), NTP version 4, and client mode 3.
  - Validates and decodes incoming server responses: stratum level (1..16), precision, root delay, root dispersion, reference ID, and 64-bit transmit timestamps.
  - Converts NTP epoch (Jan 1, 1900) to standard UNIX epoch (Jan 1, 1970) with 70-year leap-second delta offset (`NTP_UNIX_OFFSET = 2,208,988,800`).
  - Anchors the master kernel real-time clock to PIT timer ticks, providing synchronized UTC date and time across the entire system.
  - Interactive shell management via `ntp` command (`ntp status`, `ntp sync [server]`, `ntp set <unix_timestamp>`).
- **Hardware Network Drivers ([`src/drivers/net/`](src/drivers/net/))**:
  - **Intel e1000 Gigabit NIC ([`e1000.rs`](src/drivers/net/e1000.rs))**: PCI controller (`en0`) with MMIO register control and hardware packet rings.
  - **Broadcom NetXtreme BCM57765 Gigabit Ethernet ([`broadcom/tg3.rs`](src/drivers/net/broadcom/tg3.rs))**: Native Tigon3 driver (`en3`) for Apple unibody laptops (VEN_14E4, DEV_16B0). Uses 64-bit PCI BAR0 MMIO, hardware Status Block mailbox structures, Producer/Consumer DMA rings, and direct MAC address extraction (`0x0410`/`0x0414`).
  - **Broadcom BCM4331 Dual-Band 802.11n Wireless ([`broadcom/b43.rs`](src/drivers/net/broadcom/b43.rs))**: Native `b43` / BCMA driver (`wlan2`) targeting BCM4331 (`14E4:4331`). Implements BCMA PCIe backplane window switching (`PCI cfg 0x80`), core clock gating/reset, 64-bit DMA ring queues, in-tree compile-time `ucode16_mimo.fw` ingestion (`firmware/b43/`), and PSM SHM SRAM microcode uploading.
  - **Realtek Fast & Gigabit Ethernet ([`realtek.rs`](src/drivers/net/realtek.rs))**: Support for RTL8139 (10/100M Fast Ethernet), RTL8168/RTL8169 (PCI/PCIe Gigabit), and RTL8125 (2.5GbE Gaming NIC) with auto-switching MMIO/I/O port dispatch, hardware reset (`CMD_RST`), MAC extraction (`IDR0`..`IDR5`), 18 KiB RX ring buffer with wrap margin, and 4-descriptor TX rings (`en1`).
  - **Qualcomm Atheros & Killer Ethernet / Wi-Fi ([`qualcomm.rs`](src/drivers/net/qualcomm.rs))**: Support for Qualcomm Atheros AR8131/AR8151/AR8161, Killer E2200/E2400/E2500 Gigabit Gaming NICs (`en2`), and Qualcomm Atheros AR9285/AR9485/AR9565 (ath9k 802.11b/g/n wireless `wlan1`) with `ALX_MASTER` reset, station address parsing, and central `NET_MANAGER` registration.
- **POSIX Pseudo-Terminal Subsystem ([`pty.rs`](src/tty/pty.rs))**:
  - Master/slave pseudo-terminals (`/dev/ptmx`, `/dev/pts/*`), `sys_posix_openpt` (51), `sys_ptsname` (52), and `sys_unlockpt` (53).
  - Terminal line discipline with ICRNL newline translation, character echo toggling, window sizing ioctls (`TIOCGWINSZ`/`TIOCSWINSZ`), and foreground `SIGINT` signal forwarding on `Ctrl+C`.

### 7. Intel High Definition Audio & Sound Subsystem
*Location: [`src/drivers/audio/`](src/drivers/audio/)*

- **Intel HDA Driver ([`hda.rs`](src/drivers/audio/hda.rs))**: Scans PCI for Intel HDA controllers (Class 0x04, Subclass 0x03), maps MMIO BAR0, executes hardware reset (`CRST`), configures Stream Descriptor 0 for 44.1 kHz 16-bit stereo PCM, and streams samples via DMA.
- **PCM Waveform Synthesizer ([`pcm.rs`](src/drivers/audio/pcm.rs))**:
  - Quarter-wave integer sine lookup table (`integer_sin()`), avoiding floating point instructions in kernel space.
  - RIFF/WAVE parser supporting uncompressed 8-bit and 16-bit PCM WAV audio.
  - Tone generator (`generate_sine_wave()`) with smooth anti-pop envelopes.
  - SublimeOS Startup Chime (`generate_cosmic_chime()`): 4-note resonant harmonic F-Major chord (F3, C4, F4, A4) with natural acoustic decay.
  - Phonetic formant synthesis for text-to-speech audio.
- **PC Speaker Fallback ([`mod.rs`](src/drivers/audio/mod.rs))**: 8254 PIT Timer 2 square-wave hardware tone generation via I/O ports `0x42`, `0x43`, and `0x61`.

### 8. SublimeSH Modern Shell & Terminal Text Editor
*Location: [`src/shell/`](src/shell/)*

SublimeSH departs from cryptic 1970s Unix abbreviations (`cat`, `wc`, `ps`, `df`, `rm`), introducing a **modern, understandable, domain-oriented command architecture** (`system`, `tasks`, `storage`, `network`, `audio`, `list`, `view`, `create`, `delete`, `find`, `count`) while preserving 100% backwards compatibility with classic Unix utilities.

- **Modern Domain Commands & Stream Filters**:
  - `system [info|hardware|config|restart|shutdown]`: Visual telemetry dashboard, hardware profiler, kernel variables, and ACPI power management.
  - `tasks [list|top|run <bin>|kill <pid>|jobs]`: Active task scheduler states, real-time load monitor, Ring 3 binary execution, and signal delivery.
  - `storage [list|mounts]`: Block devices, partition tables (GPT/MBR), and VFS mount point inspector.
  - `network [status|ping|resolve|fetch|wifi|lease|sync]`: Interfaces, DNS lookup, HTTP/TLS 1.3 client, WPA2 Wi-Fi manager, DHCP, and NTP.
  - `audio [chime|tone|play|speak]`: Startup chime, sine wave generator, RIFF/WAVE playback, and phonetic speech synthesis.
  - `list`, `view`, `create`, `delete`, `edit`, `make folder`: Understandable file operations.
  - `find`, `count`, `first`, `last`, `sort`, `unique`: Expressive stream filters for pipelines.
- **Full-Screen Terminal Text Editor ([`editor.rs`](src/shell/editor.rs))**: In-terminal text editor (`edit <path>` / `nano <path>`) with title header, line numbers (`001 | ...`), cursor navigation, Backspace, Delete, Tab, `Ctrl+S` VFS direct save, and `Ctrl+X` exit.
- **Stream Pipelines & Redirection ([`pipeline.rs`](src/shell/pipeline.rs))**: Multi-stage piping (`view /etc/hosts | find localhost | count -l`) with stdout capture, append (`>>`), overwrite (`>`), and input redirection (`<`).
- **Full POSIX/Unix Compatibility**: Classic utilities (`ls`, `cat`, `grep`, `wc`, `head`, `tail`, `ps`, `top`, `ping`, `ifconfig`, `curl`, `reboot`, `halt`) remain active as drop-in aliases.

### 9. Aqua Graphical Desktop Environment
*Location: [`src/gui/`](src/gui/)*

- **Bochs VBE Linear Framebuffer ([`vbe.rs`](src/drivers/vbe.rs))**: 1024x768 with 32-bit ARGB true color mapped directly into higher-half virtual memory (`0xFFFFFFFFD0000000`).
- **Double-Buffered Compositor**: Software rendering engine (`Canvas`) with anti-aliasing, rounded glass rectangles, drop shadows, and horizontal gradients.
- **Window Management**: Window chrome with titlebar dragging, active/inactive focus states, and traffic light buttons (🔴 Close, 🟡 Minimize, 🟢 Zoom).
- **Top Global Menu Bar & Bottom Glass Dock**: Application menus, live RAM status gauge, translucent dock tiles, and running app indicator dots.
- **Built-in Desktop Apps**:
  - **Terminal (`Terminal.app`)**: Graphical SublimeSH shell session backed by pseudo-terminals (`/dev/pts/*`).
  - **Activity Monitor (`Activity Monitor.app`)**: Live CPU load, threads, memory pressure meter, and kernel subsystem health.
  - **Finder (`Finder.app`)**: Two-pane file manager with quick volume navigation (`Cosmic HD`, `Cosmic USB`).
  - **About SublimeOS**: Hardware specs and memory counters.

### 10. Apple Unibody & Broadcom Hardware Support
*Location: [`src/drivers/platform/apple.rs`](src/drivers/platform/apple.rs), [`src/drivers/usb/`](src/drivers/usb/), [`src/drivers/net/broadcom/`](src/drivers/net/broadcom/)*

SublimeOS provides targeted bare-metal driver support for the x86_64 architecture of the Late 2011 15-inch Apple MacBook Pro (MacBookPro8,2) equipped with an Intel Core i7-2760QM processor and companion Broadcom/Intel silicon (not ARM64 Apple Silicon):

- **Apple SMC & gmux Backlight Driver ([`platform/apple.rs`](src/drivers/platform/apple.rs))**: Native driver for Apple System Management Controller (SMC) and `gmux` hardware on Late 2011 MacBook Pro unibody laptops (MacBookPro8,2). Exposes `/dev/backlight` for reading and adjusting keyboard backlight LEDs via direct I/O port command/data handshakes (`0x300`/`0x304`).
- **USB 2.0 EHCI Periodic & Async DMA Engines ([`usb/ehci.rs`](src/drivers/usb/ehci.rs))**: Full EHCI Periodic Frame List and Interrupt Queue Head scheduling for 1ms/7ms/8ms HID polling without CPU busy-waiting. Implements USB 2.0 Hub split-transactions (SSPLIT/CSPLIT) and full standard composite configuration descriptor decoding (>64 bytes).
- **USB HID Keyboard & Multi-Touch Trackpad ([`usb/hid/`](src/drivers/usb/hid/))**: Native driver for Apple internal USB keyboards and multi-touch trackpads wired to the Intel 6-series EHCI controller. Automatically converts HID reports to standard SublimeOS input events with hardware modifier tracking.
- **Bare-Metal Apple Hardware Hardening**: Complete 8259 legacy PIC blinding, Local APIC EOI acknowledge on IRQs 32..47, and physical memory scrubbing to safeguard firmware-reserved gaps against triple faults.

---

## Interactive Shell Command Reference

SublimeOS provides **SublimeSH**, a modern, domain-oriented command shell featuring structured namespaces and intuitive verbs. Departing from cryptic 1970s Unix abbreviations, SublimeSH organizes commands around clear domains while maintaining 100% backwards compatibility with classic utilities across all virtual consoles (`Alt+F1` through `Alt+F4`), COM1 serial, and `Terminal.app`:

| Domain Namespace | Primary Modern Commands | Key Capabilities | Classic Aliases |
| :--- | :--- | :--- | :--- |
| **System & Hardware** | `system info`, `system hardware`, `system config`, `system restart` | Visual telemetry box, CPU/RAM metrics, uptime, ACPI power | `sw_vers`, `system_profiler`, `sysctl`, `reboot`, `halt` |
| **Tasks & Processes** | `tasks list`, `tasks top`, `tasks run <bin>`, `tasks kill <pid>` | Scheduler task table, load monitor, Ring 3 execution, job control | `ps`, `top`, `exec`, `kill`, `jobs`, `fg`, `bg` |
| **Storage & Files** | `storage list`, `storage mounts`, `storage cache`, `storage sync`, `list`, `view`, `edit`, `delete` | Disk partitions, filesystem mounts, 512 KiB LRU cache, flush dirty buffers, file inspection & editing | `diskutil`, `mount`, `bcache`, `sync`, `ls`, `cat`, `nano`, `rm`, `touch` |
| **Network & Web** | `network status`, `network ping`, `network resolve`, `network fetch` | Interface configs, ICMP ping, DNS lookup, HTTP/TLS 1.3 client | `ifconfig`, `ping`, `nslookup`, `curl`, `wifi`, `dhcp` |
| **Audio & Media** | `audio chime`, `audio tone`, `audio play`, `audio speak` | Harmonic F-Major chime, frequency tones, WAV DMA, speech | `chime`, `beep`, `play`, `say` |
| **Stream Filters** | `find <pat>`, `count [-l|-w|-c]`, `first [-n N]`, `last [-n N]` | Expressive stream filtering, line counting, sorting & deduplication | `grep`, `wc`, `head`, `tail`, `sort`, `uniq` |

> For the comprehensive manual documenting syntax, subcommands, flags, and practical examples, see **[`docs/COMMANDS.md`](docs/COMMANDS.md)**.

---

## Automated Verification Suite

On boot, SublimeOS automatically executes an in-kernel verification suite comprising 55 self-tests that validate every core subsystem before dropping into the interactive shell:

```text
=======================================================
             KiteKernel Verification Suite           
=======================================================
  [TEST 01..10] Memory, VFS, Scheduler & Storage Filesystems   ... [PASS 10/10]
  [TEST 11..20] Networking, Crypto, Wi-Fi & Userspace Runtime   ... [PASS 10/10]
  [TEST 21..30] CoW Fork, Dynamic Linker, Signals & Storage DMA ... [PASS 10/10]
  [TEST 31..40] PTY, TLS 1.3, POSIX IPC, DHCP & NTFS Runlists   ... [PASS 10/10]
  [TEST 41..55] ext4 HTree, NTP, Unix Sockets, SMP, Broadcom, Block Cache & Hardware Guards ... [PASS 15/15]
=======================================================
      All 55 Kernel Self-Tests PASSED Successfully!    
=======================================================
```

### Verification Coverage by Subsystem

| Subsystem Domain | Tests | Tested Mechanisms |
| :--- | :--- | :--- |
| **Memory Management** | 1, 2, 36, 44 | Frame bitmap allocator, heap expansion, POSIX shared memory (`/dev/shm`), demand paging (`#PF`), swap slots |
| **Scheduler & Processes** | 5, 18, 20, 21, 23, 27, 33, 43 | Callee-saved context switch, Ring 3 drop, CoW `fork()`, POSIX signals, work-stealing, job control, `sys_clone` |
| **Filesystems & Storage** | 3, 7, 8, 9, 10, 11, 25, 28, 39, 40, 52 | VFS mounting, FAT32 R/W, exFAT, ext4 HTree indexing, NTFS resident & runlists, NVMe/AHCI DMA, auto-mounter, 512 KiB LRU Block Cache & `sync` |
| **Networking & Protocols** | 12, 13, 14, 15, 16, 29, 31, 32, 35, 38, 41, 42, 45 | SHA-1/PBKDF2, WPA2 4-way handshake, ARP/IPv4/ICMP/TCP, sockets, TLS 1.3, telnetd/sshd, mDNS, DHCP, NTP, Unix sockets |
| **Hardware & Apple Platform** | 4, 6, 17, 24, 30, 34, 46, 47, 48, 49, 50, 51 | Anonymous pipes, UART, Intel HDA, ACPI MADT/SMP, PTY, USB HID, Broadcom tg3 & BCM4331, EHCI, SMC backlight |

> For the complete raw verification log, per-test descriptions, and instructions for running the test suite across headless, SMP, network, and storage profiles, see **[`docs/TESTING.md`](docs/TESTING.md)**.

---

## Building and Running

### Prerequisites
1. **Rust Nightly**:
   ```powershell
   rustup default nightly
   rustup target add x86_64-unknown-none x86_64-unknown-uefi
   rustup component add rust-src llvm-tools
   ```
2. **QEMU Emulator**:
   Ensure `qemu-system-x86_64` is installed and available in your PATH.

### Compiling Kernel & UEFI Bootloader
Compile the release kernel binary, standalone UEFI bootloader, and convert to Multiboot ELF:
```powershell
.\build.ps1
```
*Outputs:*
- `target/x86_64-unknown-none/release/KiteKernel` (64-bit Kernel ELF)
- `sublimekernel32.elf` (Multiboot bootable container)
- `BOOTX64.EFI` (Standalone pure-Rust 64-bit UEFI Multiboot2 bootloader)

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

SublimeOS is tested and verified on physical Intel/AMD hardware and Apple MacBook Pro (Late 2011, MacBookPro8,2):

1. **Native UEFI USB (Recommended for MacBook Pro & Modern PCs)**:
   - Format a USB drive as standard **FAT32** (GPT or MBR).
   - Stage boot media onto the USB:
     ```powershell
     .\create_boot_media.ps1 -DriveLetter E:
     ```
     *(Replace `E:` with your USB drive letter)*
   - Booting:
     - **Apple MacBook Pro / Mac**: Plug in USB, hold **Option (Alt)** at the startup chime, and select **`EFI Boot`**.
     - **Intel/AMD PC**: Press **F12 / F11 / F8** to access the UEFI Boot Menu and select the USB drive.
   - The pure-Rust `BOOTX64.EFI` loader automatically sets up UEFI GOP graphics, transitions the CPU to 32-bit Multiboot2, and jumps to `_start`.

2. **Ventoy (Multiboot ELF)**:
   - Install [Ventoy](https://www.ventoy.net) on a USB flash drive.
   - Copy `sublimekernel32.elf` directly onto the Ventoy USB drive.
   - Boot via Ventoy Multiboot menu on any PC.

3. **Legacy BIOS / GRUB2**:
   - Format a USB drive as FAT32 (MBR) and write with `.\create_boot_media.ps1 -DriveLetter E:`.
   - Install GRUB2 MBR using `grub-install` or Rufus.

---

## Author & Credits

**SublimeOS** and **KiteKernel** are engineered and maintained by:
- **Lead Developer**: [Nader Mahbub Khan](https://github.com/nadermkhan) (`muhammadnadermahbubkhan@gmail.com`)
- **Copyright**: © 2026 Nader Mahbub Khan. All rights reserved.

---

## License

SublimeOS is dual-licensed under the MIT and Apache-2.0 open-source licenses.

