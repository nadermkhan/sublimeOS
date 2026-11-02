# CosmosKernel

> **A Clean, Neat, and Mature 64-bit Monolithic Operating System Kernel Written in Rust.**

[![Target](https://img.shields.io/badge/target-x86__64--unknown--none-blue.svg)](https://www.rust-lang.org/)
[![Toolchain](https://img.shields.io/badge/rustc-nightly-orange.svg)](https://rust-lang.github.io/rustup/)
[![License](https://img.shields.io/badge/license-MIT%2FApache--2.0-green.svg)](LICENSE)
[![Architecture](https://img.shields.io/badge/arch-x86__64%20(AMD64)-red.svg)]()

CosmosKernel is a complete, monolithic, bare-metal 64-bit operating system kernel developed in safe and idiomatic Rust. It boots directly via Multiboot / Multiboot2, transitions into 64-bit Long Mode, maps kernel space to the higher half, sets up full memory management, preemptive multitasking, a virtual file system (VFS), POSIX device nodes, inter-process communication, hardware drivers, system calls, and launches an interactive kernel shell (`CosmosSH`).

---

## Key Subsystems & Architecture

```
                    +------------------------------------------+
                    |        Cosmos Desktop GUI Compositor     |
                    |  (1024x768x32bpp Double-Buffered Canvas) |
                    |  - Windows, Mouse, Taskbar, Apps, Theme  |
                    +--------------------+---------------------+
                                         |
                    +--------------------+---------------------+
                    |        Cosmos Interactive Shell          |
                    |        (Built-in Diagnostic CLI)         |
                    +--------------------+---------------------+
                                         |
    +------------------------------------+------------------------------------+
    |                        Virtual File System (VFS)                        |
    |   +------------------+  +------------------+  +---------------------+   |
    |   |      RamFS       |  |      DevFS       |  |       ProcFS        |   |
    |   | (/etc, /bin, /tmp|  | (/dev/null, tty, |  | (/proc/cpuinfo,    |   |
    |   |  /home, rootfs)  |  |  zero, rtc, rnd) |  |  meminfo, version) |   |
    |   +------------------+  +------------------+  +---------------------+   |
    +------------------------------------+------------------------------------+
                                         |
    +-------------------+----------------+-------------------+----------------+
    |    Syscall API    |        Preemptive Sched   |       IPC Pipes & Signals|
    | (syscall / sysret)| (Round-Robin Context Switch)       | (Unix FIFO & POSIX Sig)|
    +-------------------+----------------+-------------------+----------------+
                                         |
    +------------------------------------+------------------------------------+
    |                         Memory Management                               |
    |   +-----------------------------+  +--------------------------------+   |
    |   |   Physical Memory (PMM)     |  |     Virtual Memory (VMM)       |   |
    |   |   Bitmap Frame Allocator    |  |     4-Level Paging (PML4)      |   |
    |   +-----------------------------+  +--------------------------------+   |
    |   +-----------------------------------------------------------------+   |
    |   |           Expandable Kernel Heap (GlobalAlloc)                  |   |
    |   |           Enables Rust alloc (Box, Vec, String, Arc)            |   |
    |   +-----------------------------------------------------------------+   |
    +------------------------------------+------------------------------------+
                                         |
    +------------------------------------+------------------------------------+
    |                     x86_64 Hardware & Drivers                           |
    |  - Boot Trampoline (32 to 64-bit)    - 16550 UART COM1 Serial Driver    |
    |  - GDT & 64-bit TSS with IST Stacks  - 80x25 Color VGA Console Driver   |
    |  - IDT (256 Vectors & CPU Traps)     - 8254 PIT Timer (1000 Hz ticks)   |
    |  - 8259 Dual PIC Remapping (32..47)  - CMOS Real-Time Clock (RTC)       |
    |  - PS/2 Keyboard Driver (IRQ 1)      - Bochs VBE Display (1024x768x32)  |
    |  - PS/2 Mouse Driver (IRQ 12)        - PCI Bus Enumerator (lspci)       |
    +-------------------------------------------------------------------------+
```

### 1. Architecture & CPU (`src/arch/x86_64`)
- **Boot Protocol**: Multiboot 1 and Multiboot 2 headers. Starts in 32-bit protected mode, performs CPUID and Long Mode checks, builds early 4-level page tables with identity and higher-half mapping, enables PAE, EFER.LME, and CR0.PG/WP, loads 64-bit GDT, and jumps into higher-half 64-bit Rust `kernel_main`.
- **Global Descriptor Table (GDT) & TSS**: 64-bit Kernel Code (0x08), Kernel Data (0x10), User Data (0x1B), User Code (0x23), and 64-bit Task State Segment (0x28). Configures dedicated IST stacks for Double Fault (`IST1`) and Page Fault (`IST2`).
- **Interrupt Descriptor Table (IDT)**: Complete 256-vector table. Handles all 32 x86_64 CPU exceptions with register dumps, CR2 fault decoding, hardware IRQs 32..47, and user syscall vector `0x80`.
- **Interrupt Controller (PIC & APIC)**: Remaps legacy dual 8259 PICs to IRQ 32..47 and manages Local APIC.
- **Timers & RTC**: 8254 Programmable Interval Timer (PIT) configured at 1000 Hz (1ms tick) for preemptive scheduling; CMOS Real-Time Clock with BCD decoding for UTC time.
- **Console & Serial**: 80x25 color VGA text console with scrolling and hardware cursor; 16550 UART driver for COM1 (115200 baud).
- **Fast System Calls**: Sets up AMD64 `syscall`/`sysret` via MSRs `STAR`, `LSTAR`, and `FMASK`.

### 2. Memory Management (`src/mm`)
- **Physical Memory Manager (PMM)**: Bitmap frame allocator tracking all 4KiB page frames up to 1 GiB. Functions: `alloc_frame()`, `alloc_contiguous_frames(count)`, `free_frame(phys)`.
- **Virtual Memory Manager (VMM)**: 4-level paging (`PageTableEntry`, `PageTable`) manipulating PML4, PDPT, PD, and PT. Supports `map_page`, `unmap_page`, `translate`, `invlpg`, and `create_address_space`.
- **Kernel Heap Allocator**: Implements `core::alloc::GlobalAlloc` managing an expandable heap. Automatically requests physical frames from PMM and extends free lists with block coalescing. Unlocks Rust's standard `alloc` crate (`Box`, `Vec`, `String`, `Arc`, `format!`).
- **User Address Space (`AddressSpace`)**: Manages per-process Virtual Memory Areas (VMAs) for code, data, heap, user stack, and POSIX `brk()`.

### 3. Preemptive Task Management & Scheduler (`src/task`)
- **Process & Thread Abstractions**: Process Control Block (`ProcessControlBlock`) with PID, PPID, and file descriptor table; Thread Control Block (`ThreadControlBlock`) with 16 KiB kernel stack.
- **Preemptive Scheduler**: Round-Robin and Priority scheduler driven by PIT timer ticks. Manages ready queue, sleeping queue (with tick deadlines), and idle thread.
- **Assembly Context Switching**: Hand-crafted assembly routine `cosmos_context_switch(prev_rsp, next_rsp)` preserving callee-saved registers (`rbx, rbp, r12-r15, rflags`) and switching stacks atomically.

### 4. Virtual File System (`src/fs`)
- **VFS Abstraction**: Standard POSIX-style interface with `InodeOperations`, `Stat`, `DirEntry`, `File`, and global mount manager.
- **RamFS**: In-memory hierarchical filesystem supporting creation, reading, writing, seeking, and deletion.
- **DevFS (`/dev`)**:
  - `/dev/null`: Discards writes, EOF on read.
  - `/dev/zero`: Continuous stream of zero bytes.
  - `/dev/random` & `/dev/urandom`: Fast pseudo-random number generator.
  - `/dev/tty` & `/dev/console`: Reads keyboard input, writes to VGA and serial.
  - `/dev/rtc`: Formatted date & time string from CMOS RTC.
- **ProcFS (`/proc`)**:
  - `/proc/version`: Kernel version, architecture, compiler.
  - `/proc/uptime`: System uptime in seconds and timer ticks.
  - `/proc/meminfo`: Physical RAM total/free/used and heap allocation statistics.
  - `/proc/cpuinfo`: CPU model, frequency, feature flags.
  - `/proc/tasks`: Real-time task list with states and ticks.
  - `/proc/mounts`: Mounted filesystem table.
- Standard directories pre-populated: `/etc/os-release`, `/etc/motd`, `/bin`, `/tmp`, `/home`.

### 5. Inter-Process Communication (`src/ipc`)
- **Anonymous Pipes**: Circular memory buffer with read/write ends, reader/writer reference counts, and EOF signaling. Implements `InodeOperations`.
- **POSIX Signals**: Full signal set (`SIGINT`, `SIGTERM`, `SIGKILL`, `SIGSEGV`, `SIGALRM`, `SIGCHLD`, etc.) with pending masks and delivery state.

### 6. Hardware Drivers (`src/drivers`)
- **PS/2 Keyboard**: IRQ 1 handler, Set 1/2 scancode decoder, Shift/Ctrl/Alt/CapsLock modifier tracking, and thread-safe input ring buffer.
- **PCI Bus Scanner**: Scans PCI configuration space (ports 0xCF8/0xCFC), identifying host bridges, ISA bridges, IDE controllers, VGA controllers, and NICs (`lspci`).

### 7. Interactive Kernel Shell (`CosmosSH`)
Built-in interactive CLI available on both VGA display and COM1 serial:
- `help` - Command overview
- `clear` - Clear console screen
- `uname` - Kernel version and architecture
- `uptime` - Monotonic uptime from PIT/RTC
- `date` - UTC date & time from CMOS RTC
- `mem` - Physical frames & heap statistics
- `ps` / `tasks` - Task list, states, and runtime ticks
- `lspci` - Enumerate PCI devices
- `ls [path]` - List directory contents
- `cat <file>` - Display file contents
- `echo <text>` - Print text
- `mkdir <path>` - Create directory
- `touch <path>` - Create empty file
- `rm <path>` - Remove file
- `gui` - Launch the interactive Cosmos Desktop GUI (1024x768)
- `test` - Run kernel verification self-tests
- `reboot` - Reboot system
- `halt` - Halt CPU

### 8. Graphical User Interface Subsystem (`src/gui`)
CosmosKernel features a full-fledged, clean, modern desktop GUI environment:
- **Linear Framebuffer Graphics**: Bochs VBE display controller running at 1024x768 with 32-bit ARGB true color mapped directly to higher-half virtual memory (`0xFFFFFFFFD0000000`).
- **Compositor & 2D Canvas**: Double-buffered software rendering engine (`Canvas`) with Bresenham lines, rectangles, rounded corners, circles, alpha blending, gradients, and soft drop shadows.
- **Hardware Mouse Driver**: PS/2 auxiliary 8042 controller driver with IRQ 12 interrupt handler, 3-byte packet decoding, delta clamping, and arrow cursor sprite with drop shadow.
- **Window Management**: Interactive movable windows with titlebars, drag-and-drop, focus switching, maximize/minimize, and macOS-style circular control buttons (Close, Maximize, Minimize).
- **Cosmic Space Desktop**: Deep space gradient wallpaper with starfield, centered glowing logo banner, and interactive desktop icons.
- **Taskbar & Start Menu**: Bottom-docked acrylic taskbar, Start button with popup menu, running window tabs, live hardware RTC clock, and real-time physical RAM usage indicator.
- **Desktop Applications**:
  - **CosmosSH Terminal**: Graphical virtual terminal window executing shell commands with formatted output and cursor blinking.
  - **System Monitor**: Live Task Manager displaying CPU/PIT ticks, graphical RAM usage progress bar, heap memory stats, and active kernel subsystems status.
  - **File Explorer**: Graphical two-pane directory tree and file inspector for browsing `/etc`, `/proc`, `/dev`, and viewing file contents.
  - **About Cosmos**: System specification dialog detailing architecture, toolchain, memory layout, and kernel capabilities.

---

## Project Structure

```
cosmos/
├── .cargo/
│   └── config.toml                  # Target & rust-lld linker flags
├── Cargo.toml                       # Package configuration (pure Rust dependencies)
├── kernel.ld                        # Higher-half x86_64 ELF linker script
├── build.ps1                        # PowerShell one-click build script
├── run.ps1                          # PowerShell QEMU launch script
├── Makefile                         # GNU Makefile for build/run/clean
├── README.md                        # Documentation
└── src/
    ├── main.rs                      # Kernel entry point & initialization sequence
    ├── arch/
    │   ├── mod.rs                   # Architecture HAL
    │   └── x86_64/
    │       ├── boot.s               # Multiboot 1/2 headers, 32->64 trampoline, paging
    │       ├── gdt.rs               # GDT, TSS, IST stacks
    │       ├── idt.rs               # IDT, 256 vector handlers, CPU traps
    │       ├── interrupts.s         # Interrupt stubs & register context frame
    │       ├── io.rs                # Type-safe Port I/O (inb, outb, inw, outw, inl, outl)
    │       ├── msr.rs               # MSR access (IA32_EFER, STAR, LSTAR, FMASK)
    │       ├── registers.rs         # CR0, CR2, CR3, CR4, RFLAGS manipulation
    │       ├── pic.rs               # Dual 8259 PIC remapping (IRQs 32..47)
    │       ├── pit.rs               # 8254 PIT (1000 Hz timer ticks)
    │       ├── rtc.rs               # CMOS Real-Time Clock driver
    │       ├── apic.rs              # Local APIC detection and driver
    │       ├── serial.rs            # 16550 UART driver for COM1
    │       ├── syscall.rs           # Fast AMD64 syscall MSR setup
    │       └── vga.rs               # 80x25 color VGA text driver with scrolling
    ├── mm/
    │   ├── mod.rs                   # Memory subsystem definitions & HHDM conversions
    │   ├── pmm.rs                   # Bitmap physical frame allocator
    │   ├── vmm.rs                   # 4-level paging (PML4 -> PDPT -> PD -> PT)
    │   ├── heap.rs                  # Expandable GlobalAlloc kernel heap
    │   └── address_space.rs         # Process address spaces & VMAs
    ├── sync/
    │   ├── mod.rs                   # Synchronization primitives
    │   └── spinlock.rs              # Spinlock & IrqSafeSpinlock
    ├── task/
    │   ├── mod.rs                   # Process and thread management
    │   ├── thread.rs                # TCB, thread states, kernel stack
    │   ├── process.rs               # PCB, file descriptors, exit status
    │   ├── scheduler.rs             # Preemptive round-robin / priority scheduler
    │   └── switch.s                 # Assembly context switch routine
    ├── fs/
    │   ├── mod.rs                   # Root filesystem mounting
    │   ├── vfs.rs                   # InodeOperations, File, Stat, DirEntry
    │   ├── ramfs.rs                 # In-memory RamFS
    │   ├── devfs.rs                 # /dev (null, zero, random, tty, rtc)
    │   └── procfs.rs                # /proc (version, uptime, meminfo, cpuinfo, tasks)
    ├── ipc/
    │   ├── mod.rs                   # Inter-process communication
    │   ├── pipe.rs                  # Anonymous Unix pipes
    │   └── signal.rs                # POSIX signal definitions & delivery
    ├── drivers/
    │   ├── mod.rs                   # Device driver manager
    │   ├── keyboard.rs              # PS/2 keyboard driver & scancode decoder
    │   ├── mouse.rs                 # PS/2 mouse driver & IRQ 12 packet decoder
    │   ├── pci.rs                   # PCI Configuration Space scanner
    │   └── vbe.rs                   # Bochs VBE display adapter (1024x768x32bpp)
    ├── gui/
    │   ├── mod.rs                   # Compositor event loop & DesktopManager
    │   ├── canvas.rs                # Double-buffered 2D drawing canvas & primitives
    │   ├── color.rs                 # ARGB8888 color, alpha blending & cosmic palette
    │   ├── desktop.rs               # Space wallpaper, starfield & desktop icons
    │   ├── font.rs                  # 8x16 monospace font renderer
    │   ├── font_data.rs             # CP437 ASCII glyph bitmap tables
    │   ├── mouse.rs                 # Arrow mouse cursor sprite & compositing
    │   ├── taskbar.rs               # Taskbar dock, Start menu, clock & RAM meter
    │   ├── window.rs                # Window chrome, dragging & control buttons
    │   └── apps/
    │       ├── mod.rs               # Application suite coordinator
    │       ├── terminal.rs          # Graphical virtual terminal (CosmosSH)
    │       ├── sysmon.rs            # Live task manager & memory usage meter
    │       ├── files.rs             # Graphical file explorer & content previewer
    │       └── about.rs             # About Cosmos operating system dialog
    ├── syscall/
    │   ├── mod.rs                   # Syscall dispatcher
    │   ├── table.rs                 # POSIX system call numbers
    │   ├── fs_calls.rs              # Syscall implementations (read, write, uname)
    │   └── syscall_entry.s          # AMD64 fast syscall assembly entry
    ├── shell/
    │   ├── mod.rs                   # CosmosSH interactive loop
    │   └── commands.rs              # Shell commands implementation
    └── tests/
        └── mod.rs                   # Kernel automated verification harness
```

---

## Building and Running

### Prerequisites
1. **Rust Nightly**:
   ```bash
   rustup default nightly
   rustup target add x86_64-unknown-none
   rustup component add rust-src llvm-tools
   ```
2. **QEMU**:
   Install `qemu-system-x86_64` (available in MinGW, MSYS2, or official QEMU Windows installer).

### 1. Build
To compile the kernel:
```powershell
.\build.ps1
```
Or with `make`:
```bash
make build
```

This compiles `target/x86_64-unknown-none/release/cosmoskernel` and generates the Multiboot-compatible `cosmoskernel32.elf`.

### 2. Run in QEMU

#### Terminal Mode (COM1 Serial):
```powershell
.\run.ps1
```
Or:
```bash
make run
```

#### Graphical VGA Window (with PS/2 Keyboard):
```powershell
.\run.ps1 -Graphic
```
Or:
```bash
make run-graphic
```

---

## Verification Test Output

On boot, CosmosKernel executes its self-test harness, verifying physical frame allocation, heap allocation, filesystem operations, pipes, and task context switching:

```
=======================================================
             CosmosKernel Verification Suite           
=======================================================
  [TEST] Physical Frame Allocator ... [PASS]
  [TEST] Dynamic Heap & Alloc Crate ... [PASS]
  [TEST] Virtual File System & Mounts ... [PASS]
  [TEST] Anonymous IPC Pipes ... [PASS]
  [TEST] Task Context Switch & Yield ... [PASS]
=======================================================
      All Kernel Self-Tests PASSED Successfully!       
=======================================================
```

You are then greeted with the interactive prompt:
```
cosmos:/# uname
CosmosKernel 0.1.0-monolithic x86_64 GNU/Cosmos #1 SMP PREEMPT 2026

cosmos:/# cat /proc/meminfo
MemTotal:         131072 kB
MemFree:          121856 kB
MemUsed:            9216 kB
HeapTotal:          1024 kB
HeapUsed:             21 kB
HeapFree:           1002 kB
PageSize:              4 kB
```
