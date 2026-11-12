/*
 * CosmosKernel - 64-bit Interrupt Assembly Stubs
 */

.code64
.section .text

.macro ISR_NOERRCODE vec
.global isr_\vec
.type isr_\vec, @function
isr_\vec:
    push 0                  /* Push dummy error code */
    push \vec               /* Push interrupt vector */
    jmp interrupt_common
.endm

.macro ISR_ERRCODE vec
.global isr_\vec
.type isr_\vec, @function
isr_\vec:
    /* CPU already pushed error code */
    push \vec               /* Push interrupt vector */
    jmp interrupt_common
.endm

/* CPU Exceptions 0..31 */
ISR_NOERRCODE 0   /* #DE Divide Error */
ISR_NOERRCODE 1   /* #DB Debug */
ISR_NOERRCODE 2   /* NMI Interrupt */
ISR_NOERRCODE 3   /* #BP Breakpoint */
ISR_NOERRCODE 4   /* #OF Overflow */
ISR_NOERRCODE 5   /* #BR Bound Range Exceeded */
ISR_NOERRCODE 6   /* #UD Invalid Opcode */
ISR_NOERRCODE 7   /* #NM Device Not Available */
ISR_ERRCODE   8   /* #DF Double Fault */
ISR_NOERRCODE 9   /* Coprocessor Segment Overrun */
ISR_ERRCODE   10  /* #TS Invalid TSS */
ISR_ERRCODE   11  /* #NP Segment Not Present */
ISR_ERRCODE   12  /* #SS Stack-Segment Fault */
ISR_ERRCODE   13  /* #GP General Protection Fault */
ISR_ERRCODE   14  /* #PF Page Fault */
ISR_NOERRCODE 15  /* Reserved */
ISR_NOERRCODE 16  /* #MF x87 FPU Floating-Point Error */
ISR_ERRCODE   17  /* #AC Alignment Check */
ISR_NOERRCODE 18  /* #MC Machine Check */
ISR_NOERRCODE 19  /* #XM SIMD Floating-Point Exception */
ISR_NOERRCODE 20  /* #VE Virtualization Exception */
ISR_ERRCODE   21  /* #CP Control Protection Exception */
ISR_NOERRCODE 22
ISR_NOERRCODE 23
ISR_NOERRCODE 24
ISR_NOERRCODE 25
ISR_NOERRCODE 26
ISR_NOERRCODE 27
ISR_NOERRCODE 28
ISR_NOERRCODE 29
ISR_ERRCODE   30  /* #SX Security Exception */
ISR_NOERRCODE 31

/* Hardware IRQs 32..47 */
ISR_NOERRCODE 32  /* IRQ 0: Timer */
ISR_NOERRCODE 33  /* IRQ 1: Keyboard */
ISR_NOERRCODE 34  /* IRQ 2: Cascade */
ISR_NOERRCODE 35  /* IRQ 3: COM2 */
ISR_NOERRCODE 36  /* IRQ 4: COM1 */
ISR_NOERRCODE 37  /* IRQ 5: LPT2 */
ISR_NOERRCODE 38  /* IRQ 6: Floppy */
ISR_NOERRCODE 39  /* IRQ 7: Spurious / LPT1 */
ISR_NOERRCODE 40  /* IRQ 8: CMOS RTC */
ISR_NOERRCODE 41  /* IRQ 9: Free */
ISR_NOERRCODE 42  /* IRQ 10: Free */
ISR_NOERRCODE 43  /* IRQ 11: Free */
ISR_NOERRCODE 44  /* IRQ 12: PS/2 Mouse */
ISR_NOERRCODE 45  /* IRQ 13: FPU */
ISR_NOERRCODE 46  /* IRQ 14: Primary ATA */
ISR_NOERRCODE 47  /* IRQ 15: Secondary ATA */

/* Local APIC Timer vector 0x30 (48) */
ISR_NOERRCODE 48

/* Syscall vector 0x80 (128) */
ISR_NOERRCODE 128

/* Spurious APIC vector 0xFF (255) */
ISR_NOERRCODE 255

interrupt_common:
    /* Save general purpose registers */
    push rax
    push rbx
    push rcx
    push rdx
    push rsi
    push rdi
    push rbp
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15

    /* First argument to interrupt_dispatch(ctx: *mut InterruptContext) is RSP */
    mov rdi, rsp

    /* Call high-level Rust interrupt dispatcher */
    call interrupt_dispatch

    /* Restore general purpose registers */
    pop r15
    pop r14
    pop r13
    pop r12
    pop r11
    pop r10
    pop r9
    pop r8
    pop rbp
    pop rdi
    pop rsi
    pop rdx
    pop rcx
    pop rbx
    pop rax

    /* Clean up vector and error code */
    add rsp, 16

    iretq
