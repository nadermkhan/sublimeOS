/*
 * KiteKernel - 16-bit Real Mode to 64-bit Long Mode AP Bootstrap Trampoline
 * Copied to physical address 0x8000 before SIPI vector 0x08 is dispatched.
 *
 * Fixed layout within page 0x8000 (runtime-asserted in smp::install_trampoline_page):
 *   0x8000: 2-byte explicit short jump (.byte 0xEB, rel8) + 6 bytes NOP padding
 *   0x8008: ap_mailbox_cr3   (u64)
 *   0x8010: ap_mailbox_stack (u64)
 *   0x8018: ap_mailbox_entry (u64)
 *   0x8020: ap_mailbox_arg   (u64)
 *   0x8028: ap_gdt           (5 * 8 = 40 bytes)
 *   0x8050: ap_gdt_ptr       (6 bytes + 2 pad)
 *   0x8058: ap_entry16       (16-bit -> 32-bit -> 64-bit trampoline code)
 */

.set AP_TRAMPOLINE_PHYS, 0x8000

.section .rodata
.align 16
.global ap_trampoline_start
.global ap_trampoline_end
.global ap_mailbox_cr3
.global ap_mailbox_stack
.global ap_mailbox_entry
.global ap_mailbox_arg
.global ap_gdt
.global ap_code_start

ap_trampoline_start:
.code16
    /* Explicit 2-byte near short jump + 6 NOPs so header is always exactly 8 bytes */
    .byte 0xEB
    .byte (ap_entry16 - ap_trampoline_start - 2)
    .byte 0x90, 0x90, 0x90, 0x90, 0x90, 0x90

ap_mailbox_cr3:
    .quad 0                 /* 0x8008 */
ap_mailbox_stack:
    .quad 0                 /* 0x8010 */
ap_mailbox_entry:
    .quad 0                 /* 0x8018 */
ap_mailbox_arg:
    .quad 0                 /* 0x8020 */

.align 8
ap_gdt:                     /* 0x8028 */
    .quad 0x0000000000000000    /* 0x00: Null Descriptor */
    .quad 0x00CF9B000000FFFF    /* 0x08: 32-bit Protected Mode Code (Accessed=1, RX) */
    .quad 0x00CF93000000FFFF    /* 0x10: 32-bit Protected Mode Data (Accessed=1, RW) */
    .quad 0x00209B0000000000    /* 0x18: 64-bit Long Mode Code (L=1, Accessed=1, RX) */
    .quad 0x0000930000000000    /* 0x20: 64-bit Long Mode Data (Accessed=1, RW) */
ap_gdt_end:

ap_gdt_ptr:                 /* 0x8050 */
    .short (ap_gdt_end - ap_gdt - 1)
    .long AP_TRAMPOLINE_PHYS + (ap_gdt - ap_trampoline_start)
    .short 0

ap_code_start:
ap_entry16:                 /* 0x8058 */
    cli
    cld
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov ss, ax

    /* Load temporary 32-bit/64-bit GDT descriptor at physical 0x8050 */
    mov bx, 0x8050
    lgdt [bx]

    /* Switch to 32-bit Protected Mode (CR0.PE = 1) */
    mov eax, cr0
    or eax, 1
    mov cr0, eax

    /* Far jump to 32-bit protected mode code segment (selector 0x08) */
    .byte 0xEA
    .word AP_TRAMPOLINE_PHYS + (ap_pm32 - ap_trampoline_start)
    .word 0x08

.code32
ap_pm32:
    mov eax, 0x10
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax

    /* Enable PAE (bit 5), OSFXSR (bit 9), OSXMMEXCPT (bit 10) in CR4 */
    mov eax, cr4
    or eax, (1 << 5) | (1 << 9) | (1 << 10)
    mov cr4, eax

    /* Load kernel primary PML4 physical address (< 4 GiB) from mailbox at 0x8008 */
    mov eax, 0x8008
    mov eax, dword ptr [eax]
    mov cr3, eax

    /* Enable 64-bit Long Mode (EFER.LME = bit 8) and NXE (bit 11) in MSR 0xC0000080 */
    mov ecx, 0xC0000080
    rdmsr
    or eax, (1 << 8) | (1 << 11)
    wrmsr

    /* Enable Paging (CR0.PG = bit 31), Write Protect (CR0.WP = bit 16), MP, clear EM */
    mov eax, cr0
    and eax, ~(1 << 2)
    or eax, (1 << 31) | (1 << 16) | (1 << 1)
    mov cr0, eax

    /* Far jump into 64-bit Long Mode code segment (selector 0x18) */
    .byte 0xEA
    .long AP_TRAMPOLINE_PHYS + (ap_lm64 - ap_trampoline_start)
    .word 0x18

.code64
ap_lm64:
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax

    /* Load per-AP kernel stack (0x8010), arg (0x8020), and entry pointer (0x8018) */
    mov eax, 0x8010
    mov rsp, qword ptr [rax]
    mov eax, 0x8020
    mov rdi, qword ptr [rax]
    mov eax, 0x8018
    mov rax, qword ptr [rax]
    call rax

ap_halt_loop:
    cli
    hlt
    jmp ap_halt_loop

ap_trampoline_end:
