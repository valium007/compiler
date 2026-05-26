.intel_syntax noprefix
.global main
.text

.section .rodata
.Lfmt_int:
    .asciz "%lld\n"
.text


spillme:
.Lbb_0:
    push rbp
    mov rbp, rsp
    sub rsp, 248
    push rbx
    push r12
    push r13
    push r14
    push r15

    mov QWORD PTR [rbp - 232], rdi
    mov QWORD PTR [rbp - 80], 7
    mov QWORD PTR [rbp - 88], 11
    mov QWORD PTR [rbp - 96], 13
    mov QWORD PTR [rbp - 104], 17
    mov QWORD PTR [rbp - 112], 19
    mov QWORD PTR [rbp - 120], 23
    mov QWORD PTR [rbp - 128], 29
    mov QWORD PTR [rbp - 136], 31
    mov QWORD PTR [rbp - 144], 37
    mov QWORD PTR [rbp - 152], 41
    mov QWORD PTR [rbp - 160], 43
    mov QWORD PTR [rbp - 8], 47
    mov QWORD PTR [rbp - 16], 53
    mov QWORD PTR [rbp - 24], 59
    mov QWORD PTR [rbp - 32], 61
    mov QWORD PTR [rbp - 40], 67
    mov QWORD PTR [rbp - 48], 71
    mov QWORD PTR [rbp - 56], 73
    mov QWORD PTR [rbp - 64], 79
    mov QWORD PTR [rbp - 72], 83
    mov r11, QWORD PTR [rbp - 232]
    mov r10, QWORD PTR [rbp - 80]
    mov rax, r11
    imul rax, r10
    mov r11, QWORD PTR [rbp - 232]
    mov r10, QWORD PTR [rbp - 88]
    mov rcx, r11
    imul rcx, r10
    mov r11, QWORD PTR [rbp - 232]
    mov r10, QWORD PTR [rbp - 96]
    mov rdx, r11
    imul rdx, r10
    mov r11, QWORD PTR [rbp - 232]
    mov r10, QWORD PTR [rbp - 104]
    mov rsi, r11
    imul rsi, r10
    mov r11, QWORD PTR [rbp - 232]
    mov r10, QWORD PTR [rbp - 112]
    mov rdi, r11
    imul rdi, r10
    mov r11, QWORD PTR [rbp - 232]
    mov r10, QWORD PTR [rbp - 120]
    mov r8, r11
    imul r8, r10
    mov r11, QWORD PTR [rbp - 232]
    mov r10, QWORD PTR [rbp - 128]
    mov r9, r11
    imul r9, r10
    mov r11, QWORD PTR [rbp - 232]
    mov r10, QWORD PTR [rbp - 136]
    mov rbx, r11
    imul rbx, r10
    mov r11, QWORD PTR [rbp - 232]
    mov r10, QWORD PTR [rbp - 144]
    mov r12, r11
    imul r12, r10
    mov r11, QWORD PTR [rbp - 232]
    mov r10, QWORD PTR [rbp - 152]
    mov r13, r11
    imul r13, r10
    mov r11, QWORD PTR [rbp - 232]
    mov r10, QWORD PTR [rbp - 160]
    mov r14, r11
    imul r14, r10
    mov r10, QWORD PTR [rbp - 232]
    mov r11, r10
    imul r11, QWORD PTR [rbp - 8]
    mov QWORD PTR [rbp - 168], r11
    mov r10, QWORD PTR [rbp - 232]
    mov r11, r10
    imul r11, QWORD PTR [rbp - 16]
    mov QWORD PTR [rbp - 176], r11
    mov r10, QWORD PTR [rbp - 232]
    mov r11, r10
    imul r11, QWORD PTR [rbp - 24]
    mov QWORD PTR [rbp - 184], r11
    mov r10, QWORD PTR [rbp - 232]
    mov r11, r10
    imul r11, QWORD PTR [rbp - 32]
    mov QWORD PTR [rbp - 192], r11
    mov r10, QWORD PTR [rbp - 232]
    mov r11, r10
    imul r11, QWORD PTR [rbp - 40]
    mov QWORD PTR [rbp - 200], r11
    mov r10, QWORD PTR [rbp - 232]
    mov r11, r10
    imul r11, QWORD PTR [rbp - 48]
    mov QWORD PTR [rbp - 208], r11
    mov r10, QWORD PTR [rbp - 232]
    mov r11, r10
    imul r11, QWORD PTR [rbp - 56]
    mov QWORD PTR [rbp - 216], r11
    mov r10, QWORD PTR [rbp - 232]
    mov r11, r10
    imul r11, QWORD PTR [rbp - 64]
    mov QWORD PTR [rbp - 224], r11
    mov r11, QWORD PTR [rbp - 232]
    mov r10, QWORD PTR [rbp - 72]
    mov r15, r11
    imul r15, r10
    mov r11, rax
    add r11, rcx
    mov QWORD PTR [rbp - 240], r11
    mov r11, QWORD PTR [rbp - 240]
    mov rax, r11
    add rax, rdx
    add rax, rsi
    add rax, rdi
    add rax, r8
    add rax, r9
    add rax, rbx
    add rax, r12
    add rax, r13
    add rax, r14
    mov r11, QWORD PTR [rbp - 168]
    add rax, r11
    mov r11, QWORD PTR [rbp - 176]
    add rax, r11
    mov r11, QWORD PTR [rbp - 184]
    add rax, r11
    mov r11, QWORD PTR [rbp - 192]
    add rax, r11
    mov r11, QWORD PTR [rbp - 200]
    add rax, r11
    mov r11, QWORD PTR [rbp - 208]
    add rax, r11
    mov r11, QWORD PTR [rbp - 216]
    add rax, r11
    mov r11, QWORD PTR [rbp - 224]
    add rax, r11
    add rax, r15
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbx
    mov rsp, rbp
    pop rbp
    ret


main:
.Lbb_1:
    push rbp
    mov rbp, rsp
    sub rsp, 8
    push rbx
    push r12
    push r13
    push r14
    push r15

    mov rdi, 10
    call .Lbb_0
    mov rsi, rax
    lea rdi, [rip + .Lfmt_int]
    xor eax, eax
    call printf
    mov rax, 0
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbx
    mov rsp, rbp
    pop rbp
    ret

