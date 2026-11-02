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

    /* Push general purpose registers */
    push rbp
    push rbx
    push r12
    push r13
    push r14
    push r15

    /*
     * AMD64 syscall calling convention:
     *   RAX: syscall number
     *   RDI: arg1
     *   RSI: arg2
     *   RDX: arg3
     *   R10: arg4 (syscall changes RCX to user RIP, so Linux uses R10 instead of RCX)
     *   R8:  arg5
     *   R9:  arg6
     * Pass to syscall_dispatcher(num, a1, a2, a3, a4, a5, a6):
     */
    mov rcx, r10 /* arg4 */
    /* System V ABI: rdi=a1, rsi=a2, rdx=a3, rcx=a4, r8=a5, r9=a6 */
    /* We push num or pass in rdi: */
    push r9      /* arg6 */
    push r8      /* arg5 */
    mov r9, rcx  /* arg4 */
    mov r8, rdx  /* arg3 */
    mov rcx, rsi /* arg2 */
    mov rdx, rdi /* arg1 */
    mov rsi, rax /* num */

    call syscall_dispatcher
    add rsp, 16

    /* Restore registers */
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbx
    pop rbp

    pop rcx
    pop r11

    /* Return to user space */
    sysretq
