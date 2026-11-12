/*
 * KiteKernel - AMD64 Ring 3 User Transition and Return
 */

.code64
.section .text
.global enter_user_mode_asm
.type enter_user_mode_asm, @function

.global exit_user_mode_asm
.type exit_user_mode_asm, @function

.section .data
.align 8
NEST_DEPTH:
    .quad 0
SAVED_RSP_STACK:
    .quad 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
SAVED_CR3_STACK:
    .quad 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
SAVED_RFLAGS_STACK:
    .quad 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0

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

    /* 2. Save current kernel RSP, CR3, and RFLAGS to nested frame stack */
    mov r8, [rip + NEST_DEPTH]
    lea r9, [rip + SAVED_RSP_STACK]
    mov [r9 + r8 * 8], rsp

    mov rax, cr3
    lea r9, [rip + SAVED_CR3_STACK]
    mov [r9 + r8 * 8], rax

    pushfq
    pop rax
    lea r9, [rip + SAVED_RFLAGS_STACK]
    mov [r9 + r8 * 8], rax

    inc r8
    mov [rip + NEST_DEPTH], r8

    /* 3. Switch to user address space (CR3) */
    mov cr3, rdx

    /* 4. Set user data segment registers: USER_DS = 0x18 | 3 = 0x1B */
    mov ax, 0x1B
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax

    /* 5. Push 64-bit iretq stack frame:
     *    [SS, RSP, RFLAGS, CS, RIP]
     */
    push 0x1B        /* SS: User Data Segment (DPL 3) */
    push rsi         /* RSP: User Stack Pointer */
    push 0x202       /* RFLAGS: IF enabled (bit 9), reserved bit 1 = 1 */
    push 0x23        /* CS: User Code Segment (DPL 3, 0x20 | 3) */
    push rdi         /* RIP: User Entry Point */

    /* 6. Drop privilege level to Ring 3 */
    iretq

/*
 * void exit_user_mode_asm(int exit_code);
 * Argument (SysV ABI):
 *   rdi: exit_code (32-bit integer)
 */
exit_user_mode_asm:
    mov r8, [rip + NEST_DEPTH]
    test r8, r8
    jz .Lno_nest
    dec r8
    mov [rip + NEST_DEPTH], r8

    /* 1. Restore kernel CR3 */
    lea r9, [rip + SAVED_CR3_STACK]
    mov rax, [r9 + r8 * 8]
    mov cr3, rax

    /* 2. Restore kernel segment selectors: KERNEL_DS = 0x10 */
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov fs, ax
    mov gs, ax
    mov ss, ax

    /* 3. Restore kernel RSP */
    lea r9, [rip + SAVED_RSP_STACK]
    mov rsp, [r9 + r8 * 8]

    /* 4. Restore kernel RFLAGS (including IF=1 before sys_exit/fault interrupt gate cleared it) */
    lea r9, [rip + SAVED_RFLAGS_STACK]
    mov rax, [r9 + r8 * 8]
    push rax
    popfq

.Lno_nest:
    /* 5. Move exit_code into rax (return value) */
    mov rax, rdi

    /* 6. Restore kernel caller's callee-saved registers */
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbp
    pop rbx

    /* 7. Return cleanly to the caller of enter_user_mode_asm */
    ret
