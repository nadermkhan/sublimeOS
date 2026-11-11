/*
 * CosmosKernel - 32-bit Multiboot to 64-bit Higher-Half Trampoline
 */

.code32

.section .boot.header, "a"
.align 4
/* Multiboot 1 Header */
.set MB1_MAGIC,    0x1BADB002
.set MB1_FLAGS,    0x00000003 /* ALIGN (4K) | MEMINFO */
.set MB1_CHECKSUM, -(MB1_MAGIC + MB1_FLAGS)

.long MB1_MAGIC
.long MB1_FLAGS
.long MB1_CHECKSUM

.align 8
/* Multiboot 2 Header */
multiboot2_header:
.set MB2_MAGIC,    0xE85250D6
.set MB2_ARCH,     0 /* i386 32-bit protected mode */
.set MB2_LEN,      (multiboot2_header_end - multiboot2_header)
.set MB2_CHECKSUM, -(MB2_MAGIC + MB2_ARCH + MB2_LEN)

.long MB2_MAGIC
.long MB2_ARCH
.long MB2_LEN
.long MB2_CHECKSUM

/* Multiboot 2 Framebuffer Request Tag (Type 5) for modern UEFI GOP boot */
.align 8
.short 5           /* type: 5 = framebuffer request */
.short 0           /* flags: 0 */
.long 20           /* size: 20 bytes */
.long 1024         /* width: 1024 */
.long 768          /* height: 768 */
.long 32           /* depth: 32 bpp */

/* Multiboot 2 Information Request Tag (Type 1) */
.align 8
.short 1           /* type: 1 = information request */
.short 0           /* flags: 0 */
.long 16           /* size: 16 bytes */
.long 8            /* requested tag: 8 = framebuffer info */
.long 6            /* requested tag: 6 = memory map */

/* End tag */
.align 8
.short 0
.short 0
.long 8
multiboot2_header_end:

.section .boot.text, "ax"
.global _start
.type _start, @function
_start:
    cli
    cld

    /* Save Multiboot magic and info pointer */
    mov [boot_mb_magic], eax
    mov [boot_mb_info], ebx

    /* Set up temporary early boot stack */
    mov esp, offset boot_stack_top

    /* Check for CPUID support */
    pushfd
    pop eax
    mov ecx, eax
    xor eax, 1 << 21
    push eax
    popfd
    pushfd
    pop eax
    push ecx
    popfd
    cmp eax, ecx
    je no_cpuid

    /* Check for Long Mode */
    mov eax, 0x80000000
    cpuid
    cmp eax, 0x80000001
    jb no_long_mode

    mov eax, 0x80000001
    cpuid
    test edx, 1 << 29
    jz no_long_mode

    /* Build early page tables */
    call setup_early_paging

    /* Load CR3 with early PML4 physical address */
    mov eax, offset early_pml4
    mov cr3, eax

    /* Enable PAE (Physical Address Extension) in CR4 */
    mov eax, cr4
    or eax, 1 << 5
    mov cr4, eax

    /* Enable Long Mode (LME) and NX bit in EFER MSR (0xC0000080) */
    mov ecx, 0xC0000080
    rdmsr
    or eax, (1 << 8) | (1 << 11)
    wrmsr

    /* Enable Paging (PG) and Write Protect (WP) in CR0 */
    mov eax, cr0
    or eax, (1 << 31) | (1 << 16) | 1
    mov cr0, eax

    /* Load 64-bit GDT */
    lgdt [gdt64_ptr]

    /* Far jump into 64-bit mode trampoline in .boot.text */
    push 0x08
    mov eax, offset long_mode_start
    push eax
    retf

no_cpuid:
    mov dword ptr [0xB8000], 0x4F504F45
    hlt
    jmp no_cpuid

no_long_mode:
    mov dword ptr [0xB8000], 0x4F4D4F45
    hlt
    jmp no_long_mode

/*
 * Populate early page tables:
 * - Identity map first 1 GiB using 2 MiB huge pages
 * - Higher-half map first 1 GiB at 0xFFFFFFFF80000000 using 2 MiB huge pages
 */
setup_early_paging:
    /* Link PML4[0] -> early_pdpt_identity (Flags: Present=1, Writable=2) */
    mov eax, offset early_pdpt_identity
    or eax, 0x03
    mov dword ptr [early_pml4], eax
    mov dword ptr [early_pml4 + 4], 0

    /* Link PML4[511] -> early_pdpt_higher (0xFFFFFF8000000000) */
    mov eax, offset early_pdpt_higher
    or eax, 0x03
    mov dword ptr [early_pml4 + 511 * 8], eax
    mov dword ptr [early_pml4 + 511 * 8 + 4], 0

    /* Link early_pdpt_identity[0] -> early_pd_identity */
    mov eax, offset early_pd_identity
    or eax, 0x03
    mov dword ptr [early_pdpt_identity], eax
    mov dword ptr [early_pdpt_identity + 4], 0

    /* Link early_pdpt_higher[510] -> early_pd_higher (0xFFFFFFFF80000000) */
    mov eax, offset early_pd_higher
    or eax, 0x03
    mov dword ptr [early_pdpt_higher + 510 * 8], eax
    mov dword ptr [early_pdpt_higher + 510 * 8 + 4], 0

    /* Populate early_pd_identity with 512 * 2 MiB pages (1 GiB) */
    xor ecx, ecx
    mov ebx, 0x83 /* Present | Writable | Huge (2MB) */
.Lloop1:
    mov dword ptr [early_pd_identity + ecx * 8], ebx
    mov dword ptr [early_pd_identity + ecx * 8 + 4], 0
    add ebx, 0x200000
    inc ecx
    cmp ecx, 512
    jne .Lloop1

    /* Populate early_pd_higher with 512 * 2 MiB pages (1 GiB) */
    xor ecx, ecx
    mov ebx, 0x83
.Lloop2:
    mov dword ptr [early_pd_higher + ecx * 8], ebx
    mov dword ptr [early_pd_higher + ecx * 8 + 4], 0
    add ebx, 0x200000
    inc ecx
    cmp ecx, 512
    jne .Lloop2

    ret

.code64
/* Early 64-bit entry point running in identity-mapped lower address space */
long_mode_start:
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax

    /* Jump to higher-half kernel entry */
    mov rax, offset kernel_higher_half
    jmp rax

.section .text
kernel_higher_half:
    /* Setup 64-bit kernel stack */
    mov rsp, offset kernel_stack_top

    /* Zero out RFLAGS */
    push 0
    popfq

    /* Load Multiboot parameters: rdi = magic, rsi = info physical address */
    mov edi, [boot_mb_magic]
    mov esi, [boot_mb_info]

    /* Call the Rust kernel entry point */
    call kernel_main

    /* If kernel_main returns, halt cleanly */
.Lhang64:
    cli
    hlt
    jmp .Lhang64

.section .boot.data, "aw"
.align 8
gdt64:
    .quad 0x0000000000000000    /* 0x00: Null Descriptor */
    .quad 0x00209A0000000000    /* 0x08: 64-bit Kernel Code (Execute/Read, Ring 0) */
    .quad 0x0000920000000000    /* 0x10: 64-bit Kernel Data (Read/Write, Ring 0) */
gdt64_end:

gdt64_ptr:
    .short (gdt64_end - gdt64 - 1)
    .long gdt64

boot_mb_magic:
    .long 0
boot_mb_info:
    .long 0

.section .boot.bss, "aw", @nobits
.align 4096
early_pml4:
    .skip 4096
early_pdpt_identity:
    .skip 4096
early_pdpt_higher:
    .skip 4096
early_pd_identity:
    .skip 4096
early_pd_higher:
    .skip 4096

.align 16
boot_stack_bottom:
    .skip 16384                 /* 16 KiB early boot stack */
boot_stack_top:

.section .bss, "aw", @nobits
.align 16
.global kernel_stack_bottom
.global kernel_stack_top
kernel_stack_bottom:
    .skip 65536                 /* 64 KiB kernel stack */
kernel_stack_top:
