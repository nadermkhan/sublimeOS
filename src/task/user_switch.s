/*
 * CosmosKernel - AMD64 Ring 3 User Transition and Return
 */

.code64
.section .text
.global enter_user_mode_asm
.type enter_user_mode_asm, @function

.global exit_user_mode_asm
.type exit_user_mode_asm, @function

.section .data
.align 8
.global KERNEL_SAVED_RSP
KERNEL_SAVED_RSP:
    .quad 0

.global KERNEL_SAVED_CR3
KERNEL_SAVED_CR3:
    .quad 0

.section .text

/*
 * int enter_user_mode_asm(uint64_t rip, uint64_t rsp, uint64_t cr3, uint64_t kstack);
 * Arguments (SysV ABI):
 *   rdi: user_rip
 *   rsi: user_rsp
 *   rdx: user_cr3
 *   rcx: kernel_stack_top
 */
enter_user_mode_asm:
    /* 1. Save callee-saved registers of kernel caller */
    push rbx
    push rbp
    push r12
    push r13
    push r14
    push r15

    /* 2. Save current kernel CR3 */
    mov rax, cr3
    mov [rip + KERNEL_SAVED_CR3], rax

    /* 3. Save current kernel RSP */
    mov [rip + KERNEL_SAVED_RSP], rsp

    /* 4. Switch to user address space (CR3) */
    mov cr3, rdx

    /* 5. Set user data segment registers: USER_DS = 0x18 | 3 = 0x1B */
    mov ax, 0x1B
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax

    /* 6. Push 64-bit iretq stack frame:
     *    [SS, RSP, RFLAGS, CS, RIP]
     */
    push 0x1B        /* SS: User Data Segment (DPL 3) */
    push rsi         /* RSP: User Stack Pointer */
    push 0x202       /* RFLAGS: IF enabled (bit 9), reserved bit 1 = 1 */
    push 0x23        /* CS: User Code Segment (DPL 3, 0x20 | 3) */
    push rdi         /* RIP: User Entry Point */

    /* 7. Drop privilege level to Ring 3 */
    iretq

/*
 * void exit_user_mode_asm(int exit_code);
 * Argument (SysV ABI):
 *   rdi: exit_code (32-bit integer)
 */
exit_user_mode_asm:
    /* 1. Restore kernel CR3 */
    mov rax, [rip + KERNEL_SAVED_CR3]
    mov cr3, rax

    /* 2. Restore kernel segment selectors: KERNEL_DS = 0x10 */
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax

    /* 3. Restore kernel RSP */
    mov rsp, [rip + KERNEL_SAVED_RSP]

    /* 4. Move exit_code into rax (return value) */
    mov rax, rdi

    /* 5. Restore kernel caller's callee-saved registers */
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbp
    pop rbx

    /* 6. Return cleanly to the caller of enter_user_mode_asm */
    ret
