/*
 * KiteKernel - AMD64 fast syscall / sysret assembly entry
 */

.code64
.section .text
.global syscall_entry
.type syscall_entry, @function

syscall_entry:
    /*
     * Switch atomically to this CPU/task's kernel stack via swapgs.
     * IA32_FMASK already cleared IF/TF/DF on syscall entry, so this 5-instruction
     * window cannot be preempted and immediately restores IA32_KERNEL_GSBASE.
     * CpuSyscallContext layout: [0] = kernel_rsp, [8] = user_rsp scratch.
     */
    swapgs
    mov gs:[8], rsp
    mov rsp, gs:[0]
    push qword ptr gs:[8]   /* [1] user_rsp saved on kernel stack */
    swapgs

    /* RCX has user RIP, R11 has user RFLAGS */
    push r11                /* [2] user_rflags */
    push rcx                /* [3] user_rip */
    push rax                /* [4] syscall number (also keeps 16-byte ABI alignment) */

    /* Push all general purpose registers to preserve user state across syscall */
    push rbp                /* [5] */
    push rbx                /* [6] */
    push r12                /* [7] */
    push r13                /* [8] */
    push r14                /* [9] */
    push r15                /* [10] */
    push rdi                /* [11] */
    push rsi                /* [12] */
    push rdx                /* [13] */
    push r10                /* [14] */
    push r8                 /* [15] */
    push r9                 /* [16] SyscallFrame base = RSP (16 * 8 = 128 bytes) */

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
     * SysV ABI for syscall_dispatcher(num, a1, a2, a3, a4, a5, a6, frame):
     *   RDI: num
     *   RSI: a1
     *   RDX: a2
     *   RCX: a3
     *   R8:  a4
     *   R9:  a5
     *   [RSP+8]:  a6
     *   [RSP+16]: frame (*const SyscallFrame)
     */
    mov r11, rsp /* Snapshot SyscallFrame pointer before stack args */
    push r11     /* [17] 8th param: frame */
    push r9      /* [18] 7th param: a6 (144 bytes total -> 16-byte aligned!) */
    mov r9, r8   /* 6th param: a5 */
    mov r8, r10  /* 5th param: a4 */
    mov rcx, rdx /* 4th param: a3 */
    mov rdx, rsi /* 3rd param: a2 */
    mov rsi, rdi /* 2nd param: a1 */
    mov rdi, rax /* 1st param: num */

    call syscall_dispatcher
    add rsp, 16

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
    add rsp, 8  /* Discard saved syscall number (RAX holds return value) */

    pop rcx
    pop r11

    /* Disable interrupts across user RSP restore before sysretq re-enables IF via R11 */
    cli
    pop rsp

    /* Return to user space (RAX holds syscall return code) */
    sysretq
