.intel_syntax noprefix
.global main
.text

.section .rodata
.Lfmt_int:
    .asciz "%lld\n"
.text


square:
.Lbb_0:
    push rbp
    mov rbp, rsp
    sub rsp, 8
    push rbx
    push r12
    push r13
    push r14
    push r15

    mov rax, rdi
    imul rax, rax
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
    sub rsp, 56
    push rbx
    push r12
    push r13
    push r14
    push r15

    mov rbx, 1
    mov r12, 2
    mov r13, 3
    mov r14, 4
    mov r11, 5
    mov QWORD PTR [rbp - 48], r11
    mov r11, 6
    mov QWORD PTR [rbp - 8], r11
    mov r11, 7
    mov QWORD PTR [rbp - 16], r11
    mov r11, 8
    mov QWORD PTR [rbp - 24], r11
    mov r11, 9
    mov QWORD PTR [rbp - 32], r11
    mov r11, 10
    mov QWORD PTR [rbp - 40], r11
    mov r15, 100
    mov rdi, r15
    call .Lbb_0
    mov rdi, r15
    call .Lbb_0
    mov rdi, r15
    call .Lbb_0
    mov rdi, r15
    call .Lbb_0
    mov rdi, r15
    call .Lbb_0
    mov rdi, r15
    call .Lbb_0
    mov rdi, r15
    call .Lbb_0
    mov r15, rax
    add r15, rbx
    add r15, r12
    add r15, r13
    add r15, r14
    mov r11, QWORD PTR [rbp - 48]
    add r15, r11
    mov r11, QWORD PTR [rbp - 8]
    add r15, r11
    mov r11, QWORD PTR [rbp - 16]
    add r15, r11
    mov r11, QWORD PTR [rbp - 24]
    add r15, r11
    mov r11, QWORD PTR [rbp - 32]
    add r15, r11
    mov r11, QWORD PTR [rbp - 40]
    add r15, r11
    mov rsi, r15
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

