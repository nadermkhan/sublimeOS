/*
 * CosmosKernel - AMD64 fast syscall / sysret assembly entry
 */

.code64
.section .text
.global syscall_entry
.type syscall_entry, @function

syscall_entry:
    /* RCX has user RIP, R11 has user RFLAGS */
    push r11
    push rcx

    /* Push all general purpose registers to preserve user state across syscall */
    push rbp
    push rbx
    push r12
    push r13
    push r14
    push r15
    push rdi
    push rsi
    push rdx
    push r10
    push r8
    push r9

    /*
     * AMD64 syscall calling convention:
     *   RAX: syscall number (num)
     *   RDI: arg1
     *   RSI: arg2
     *   RDX: arg3
     *   R10: arg4 (syscall changes RCX to user RIP, so R10 carries arg4)
     *   R8:  arg5
     *   R9:  arg6
     *
     * SysV ABI for syscall_dispatcher(num, a1, a2, a3, a4, a5, a6):
     *   RDI: num
     *   RSI: a1
     *   RDX: a2
     *   RCX: a3
     *   R8:  a4
     *   R9:  a5
     *   [RSP]: a6
     */
    push r9      /* 7th param: a6 */
    mov r9, r8  /* 6th param: a5 */
    mov r8, r10 /* 5th param: a4 */
    mov rcx, rdx /* 4th param: a3 */
    mov rdx, rsi /* 3rd param: a2 */
    mov rsi, rdi /* 2nd param: a1 */
    mov rdi, rax /* 1st param: num */

    call syscall_dispatcher
    add rsp, 8

    /* Restore all user general purpose registers */
    pop r9
    pop r8
    pop r10
    pop rdx
    pop rsi
    pop rdi
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbx
    pop rbp

    pop rcx
    pop r11

    /* Return to user space (RAX holds syscall return code) */
    sysretq
