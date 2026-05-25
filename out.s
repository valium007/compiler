.intel_syntax noprefix
.global main
.text

.section .rodata
.Lfmt_int:
    .asciz "%lld\n"
.text


leaf1:
.Lbb_0:
    push rbp
    mov rbp, rsp
    sub rsp, 8
    push rbx
    push r12
    push r13
    push r14
    push r15

    mov rax, 972
    mov rcx, 130
    mov r11, rax
    imul r11, rcx
    mov rcx, r11
    mov rsi, 3
    mov rax, rcx
    cqo
    idiv rsi
    mov rsi, rax
    mov r11, rsi
    imul r11, rcx
    mov rcx, r11
    mov rsi, 3
    mov rax, rcx
    cqo
    idiv rsi
    mov rsi, rax
    mov rax, rsi
    imul rax, rcx
    mov rcx, 8
    cqo
    idiv rcx
    mov rcx, rax
    mov rsi, 2
    mov rax, rcx
    cqo
    idiv rsi
    mov rcx, rax
    mov rsi, 2
    mov rax, rcx
    cqo
    idiv rsi
    mov rcx, rax
    mov rax, rcx
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbx
    mov rsp, rbp
    pop rbp
    ret


leaf2:
.Lbb_1:
    push rbp
    mov rbp, rsp
    sub rsp, 8
    push rbx
    push r12
    push r13
    push r14
    push r15

    mov rax, rdi
    mov rcx, 507
    mov rsi, 969
    mov rdx, rax
    imul rdx, rcx
    mov r11, rdx
    imul r11, rax
    mov rax, r11
    mov rdi, 8
    cqo
    idiv rdi
    mov rdi, rax
    mov r8, 5
    mov rax, rdi
    cqo
    idiv r8
    mov rdi, rax
    mov r8, 9
    mov rax, rdi
    cqo
    idiv r8
    mov r8, rax
    mov rax, r8
    imul rax, rdi
    mov rdi, 6
    cqo
    idiv rdi
    mov rdi, rax
    mov r8, 2
    mov rax, rdi
    cqo
    idiv r8
    mov r8, rax
    mov rax, r8
    imul rax, rdi
    mov rax, rcx
    sub rax, rsi
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbx
    mov rsp, rbp
    pop rbp
    ret


leaf3:
.Lbb_2:
    push rbp
    mov rbp, rsp
    sub rsp, 8
    push rbx
    push r12
    push r13
    push r14
    push r15

    mov rax, 357
    mov rcx, 119
    mov rcx, 5
    cqo
    idiv rcx
    mov rcx, rax
    mov rsi, 7
    mov rax, rcx
    cqo
    idiv rsi
    mov rcx, rax
    mov rsi, 3
    mov rax, rcx
    cqo
    idiv rsi
    mov rsi, rax
    mov rax, rsi
    imul rax, rcx
    imul rax, rsi
    mov rcx, 3
    cqo
    idiv rcx
    mov rcx, rax
    mov rax, rcx
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbx
    mov rsp, rbp
    pop rbp
    ret


mid1:
.Lbb_3:
    push rbp
    mov rbp, rsp
    sub rsp, 8
    push rbx
    push r12
    push r13
    push r14
    push r15

    mov rbx, rdi
    mov rax, 862
    mov rcx, 7
    cqo
    idiv rcx
    mov rcx, rax
    mov rax, 933
    mov rsi, 3
    cqo
    idiv rsi
    mov rsi, rax
    mov rax, rcx
    imul rax, rsi
    mov rsi, 2
    cqo
    idiv rsi
    mov rsi, rax
    mov rax, rsi
    imul rax, rcx
    mov rcx, 5
    cqo
    idiv rcx
    mov r12, rax
    mov rax, 201
    mov rcx, 6
    cqo
    idiv rcx
    mov rcx, rax
    mov rax, rcx
    imul rax, rsi
    mov rdi, rax
    sub rdi, rbx
    call .Lbb_0
    mov r13, rax
    mov rax, 545
    mov rcx, 7
    cqo
    idiv rcx
    mov rcx, rax
    mov rax, rcx
    imul rax, r12
    mov rsi, 2
    cqo
    idiv rsi
    mov rsi, rax
    mov rax, rsi
    imul rax, rcx
    mov rdi, rax
    add rdi, rbx
    call .Lbb_1
    mov rbx, rax
    mov rax, rbx
    add rax, r13
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbx
    mov rsp, rbp
    pop rbp
    ret


mid2:
.Lbb_4:
    push rbp
    mov rbp, rsp
    sub rsp, 8
    push rbx
    push r12
    push r13
    push r14
    push r15

    mov rcx, rdi
    mov rax, 429
    mov rsi, 950
    mov rdi, 6
    cqo
    idiv rdi
    mov rbx, rax
    mov rax, rbx
    imul rax, rsi
    mov rsi, 3
    cqo
    idiv rsi
    mov r12, rax
    mov rax, r12
    imul rax, rbx
    mov rsi, 7
    cqo
    idiv rsi
    mov rsi, rax
    mov rax, rsi
    imul rax, r12
    mov rdi, 2
    cqo
    idiv rdi
    mov r13, rax
    mov rdi, rcx
    add rdi, rsi
    call .Lbb_1
    mov r14, rax
    mov rax, 912
    mov rcx, 4
    cqo
    idiv rcx
    mov rcx, rax
    mov rax, rcx
    imul rax, r13
    mov rsi, 8
    cqo
    idiv rsi
    mov rsi, rax
    mov rax, rsi
    imul rax, rcx
    mov rcx, 3
    cqo
    idiv rcx
    mov r13, rax
    mov rdi, r13
    add rdi, rbx
    call .Lbb_2
    mov rbx, rax
    mov rdi, 87
    mov rcx, 5
    mov rax, rdi
    cqo
    idiv rcx
    mov rcx, rax
    mov rax, rcx
    imul rax, r13
    imul rax, r12
    call .Lbb_0
    mov r12, rax
    mov rax, r12
    add rax, r14
    add rax, rbx
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbx
    mov rsp, rbp
    pop rbp
    ret


top1:
.Lbb_5:
    push rbp
    mov rbp, rsp
    sub rsp, 8
    push rbx
    push r12
    push r13
    push r14
    push r15

    mov rbx, rdi
    mov rcx, 957
    mov rsi, 3
    mov rax, rcx
    cqo
    idiv rsi
    mov rsi, rax
    mov rax, 898
    mov r11, rsi
    imul r11, rax
    mov rax, r11
    mov rdi, 4
    cqo
    idiv rdi
    mov rdi, rax
    mov r8, 7
    mov rax, rcx
    cqo
    idiv r8
    mov rcx, rax
    mov rax, rcx
    imul rax, rdi
    mov rdi, 5
    cqo
    idiv rdi
    mov r12, rax
    mov rax, r12
    imul rax, rcx
    mov rcx, 6
    cqo
    idiv rcx
    mov rcx, rax
    mov r13, rcx
    imul r13, rsi
    mov rdi, r13
    add rdi, rbx
    call .Lbb_3
    mov r14, rax
    mov rax, 738
    mov rcx, 4
    cqo
    idiv rcx
    mov rcx, rax
    mov rax, rcx
    imul rax, r12
    mov rsi, 3
    cqo
    idiv rsi
    mov rsi, rax
    mov rax, rsi
    imul rax, rcx
    mov rcx, 2
    cqo
    idiv rcx
    mov rcx, rax
    mov rax, rcx
    imul rax, r13
    mov rdi, rax
    add rdi, rbx
    call .Lbb_4
    mov rbx, rax
    mov rax, rbx
    add rax, r14
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbx
    mov rsp, rbp
    pop rbp
    ret


top2:
.Lbb_6:
    push rbp
    mov rbp, rsp
    sub rsp, 8
    push rbx
    push r12
    push r13
    push r14
    push r15

    mov rbx, rdi
    mov rax, 906
    mov rcx, 955
    mov rsi, 4
    cqo
    idiv rsi
    mov rsi, rax
    mov rdi, 8
    mov rax, rcx
    cqo
    idiv rdi
    mov rcx, rax
    mov rax, rsi
    imul rax, rcx
    mov rcx, 7
    cqo
    idiv rcx
    mov rcx, rax
    mov rax, rcx
    imul rax, rsi
    mov rsi, 2
    cqo
    idiv rsi
    mov r12, rax
    mov rsi, 5
    mov rax, r12
    cqo
    idiv rsi
    mov rsi, rax
    mov r13, rsi
    imul r13, rcx
    mov rdi, r13
    add rdi, rbx
    call .Lbb_4
    mov r14, rax
    mov rax, 670
    imul rax, r12
    mov rcx, 6
    cqo
    idiv rcx
    mov r12, rax
    mov rax, r12
    imul rax, r13
    mov rcx, 3
    cqo
    idiv rcx
    mov rcx, rax
    mov rdi, rcx
    add rdi, rbx
    call .Lbb_2
    mov rbx, rax
    mov rdi, 63
    mov rcx, 4
    mov rax, rdi
    cqo
    idiv rcx
    mov rcx, rax
    mov rax, rcx
    imul rax, r12
    mov rsi, 3
    cqo
    idiv rsi
    mov rsi, rax
    mov r8, 2
    mov rax, rsi
    cqo
    idiv r8
    mov rsi, rax
    mov rax, rsi
    imul rax, rcx
    call .Lbb_3
    mov r12, rax
    mov rax, r12
    add rax, r14
    add rax, rbx
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbx
    mov rsp, rbp
    pop rbp
    ret


main:
.Lbb_7:
    push rbp
    mov rbp, rsp
    sub rsp, 8
    push rbx
    push r12
    push r13
    push r14
    push r15

    mov rbx, 240
    mov rdi, rbx
    call .Lbb_5
    mov r12, rax
    mov rdi, rbx
    call .Lbb_6
    mov rbx, rax
    mov r13, r12
    add r13, rbx
    mov rsi, r12
    lea rdi, [rip + .Lfmt_int]
    xor eax, eax
    call printf
    mov rsi, rbx
    lea rdi, [rip + .Lfmt_int]
    xor eax, eax
    call printf
    mov rsi, r13
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

