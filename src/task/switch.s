/*
 * CosmosKernel - Cooperative & Preemptive Context Switch Routine
 *
 * void cosmos_context_switch(uint64_t *prev_rsp, uint64_t next_rsp);
 * System V AMD64 ABI:
 *   RDI: prev_rsp (pointer to store current thread's RSP)
 *   RSI: next_rsp (new thread's RSP to switch to)
 */

.code64
.section .text
.global cosmos_context_switch
.type cosmos_context_switch, @function

cosmos_context_switch:
    /* Save callee-saved registers of previous task */
    push rbx
    push rbp
    push r12
    push r13
    push r14
    push r15
    pushfq

    /* Save old stack pointer into *prev_rsp */
    mov [rdi], rsp

    /* Load new stack pointer from next_rsp */
    mov rsp, rsi

    /* Restore callee-saved registers of next task */
    popfq
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbp
    pop rbx

    /* Return into next task's execution point */
    ret
