//! x86-64 instruction selection and assembly emission.
//!
//! Translates `LoweredInst` (post-regalloc) into AT&T-syntax x86-64
//! assembly text.

use crate::regalloc::regalloc_ir::{Allocation, PhysReg};

use crate::regalloc::regalloc_ir::LoweredInst;
use crate::ssa::ir::BinaryOp;
use crate::codegen::aarch64::FunctionOutput;

// ── Register names ───────────────────────────────────────────────────────

/// Map a `PhysReg` index to its 64-bit Intel-syntax name.
fn reg_name(preg: PhysReg) -> &'static str {
    match preg.hw_enc() {
        0 => "rax",
        1 => "rcx",
        2 => "rdx",
        3 => "rsp",
        4 => "rsi",
        5 => "rdi",
        6 => "rbx",
        7 => "rbp",
        8 => "r8",
        9 => "r9",
        10 => "r10",
        11 => "r11",
        12 => "r12",
        13 => "r13",
        14 => "r14",
        15 => "r15",
        n => panic!("unknown PhysReg hw_enc {}", n),
    }
}

/// Map a `PhysReg` index to its 8-bit low-byte Intel-syntax name (for setcc).
fn reg_name_8(preg: PhysReg) -> &'static str {
    match preg.hw_enc() {
        0 => "al",
        1 => "cl",
        2 => "dl",
        4 => "sil",
        5 => "dil",
        6 => "bl",
        8 => "r8b",
        9 => "r9b",
        10 => "r10b",
        11 => "r11b",
        12 => "r12b",
        13 => "r13b",
        14 => "r14b",
        15 => "r15b",
        n => panic!("unknown PhysReg hw_enc {} for 8-bit", n),
    }
}

/// Format an `Allocation` as an assembly operand.
fn alloc_operand(alloc: Allocation) -> String {
    match alloc {
        Allocation::Reg(preg) => reg_name(preg).to_string(),
        Allocation::Stack(slot) => {
            // Each spill slot is 8 bytes; slot indices start at 0.
            // Stack grows downward from RBP.
            let offset = (slot.index() + 1) * 8;
            format!("QWORD PTR [rbp - {}]", offset)
        }
        _ => panic!("unexpected allocation kind"),
    }
}

/// Get the PhysReg from an allocation (panics if it's a stack slot).
fn alloc_preg(alloc: Allocation) -> PhysReg {
    alloc.as_reg().expect("expected register allocation")
}

// ── Assembly emission ────────────────────────────────────────────────────

/// Emit a complete x86-64 program (one or more functions).
pub fn emit_program(fns: &[FunctionOutput<'_>]) -> String {
    let mut asm = String::new();

    // Global header (once per program).
    asm.push_str(".intel_syntax noprefix\n");
    asm.push_str(".global main\n");
    asm.push_str(".text\n\n");

    // Format string in .rodata (used by Print).
    asm.push_str(".section .rodata\n");
    asm.push_str(".Lfmt_int:\n");
    asm.push_str("    .asciz \"%lld\\n\"\n");
    asm.push_str(".text\n\n");

    for f in fns {
        emit_one_fn(&mut asm, f.name, f.entry_bb, f.blocks, f.num_spillslots);
    }

    asm
}

/// Emit a single function (backward-compatible wrapper).
pub fn emit_function(blocks: &[Vec<LoweredInst>], num_spillslots: usize) -> String {
    emit_program(&[FunctionOutput {
        name: "main",
        entry_bb: 0,
        blocks,
        num_spillslots,
    }])
}

/// Emit one function body into `asm`.
fn emit_one_fn(asm: &mut String, fn_name: &str, entry_bb: usize, blocks: &[Vec<LoweredInst>], num_spillslots: usize) {
    // Entry rsp ≡ 8 (mod 16) per SysV.  Prologue pushes 6 qwords (rbp + 5
    // callee-saved) = 48 bytes (≡ 0 mod 16), so after the pushes rsp is still
    // 8 mod 16.  Pad `stack_size` by an extra 8 so that rsp at our call sites
    // is 0 mod 16 (which becomes 8 mod 16 in the callee, matching ABI).
    let raw = num_spillslots * 8;
    let aligned = (raw + 15) & !15;
    let stack_size = aligned + 8;

    // Function entry: both the named symbol and the entry-block label sit
    // at the same address, before the prologue, so `call .Lbb_N` enters
    // the callee at the start of its frame setup.
    asm.push_str(&format!("\n{}:\n", fn_name));
    asm.push_str(&format!(".Lbb_{}:\n", entry_bb));

    asm.push_str("    push rbp\n");
    asm.push_str("    mov rbp, rsp\n");
    asm.push_str(&format!("    sub rsp, {}\n", stack_size));

    // Save callee-saved registers we might use.
    asm.push_str("    push rbx\n");
    asm.push_str("    push r12\n");
    asm.push_str("    push r13\n");
    asm.push_str("    push r14\n");
    asm.push_str("    push r15\n");
    asm.push_str("\n");

    // ── Block bodies ──────────────────────────────────────────────────
    for (local_idx, block_insts) in blocks.iter().enumerate() {
        // Block 0's label was already emitted above (merged with fn entry).
        if local_idx > 0 {
            let global_bb = entry_bb + local_idx;
            asm.push_str(&format!(".Lbb_{}:\n", global_bb));
        }

        for inst in block_insts.iter() {
            emit_instruction(asm, inst, entry_bb);
        }

        asm.push('\n');
    }
}

/// Emit a single lowered instruction as x86-64 assembly.
fn emit_instruction(asm: &mut String, inst: &LoweredInst, entry_bb: usize) {
    match inst {
        LoweredInst::Mov(dst, src) => {
            let d = alloc_operand(*dst);
            let s = alloc_operand(*src);
            if d != s {
                // If both are memory (spill slot), we need a temp register.
                // regalloc2 should not produce mem→mem moves, but just in case:
                if dst.is_stack() && src.is_stack() {
                    asm.push_str(&format!("    mov r11, {}\n", s));
                    asm.push_str(&format!("    mov {}, r11\n", d));
                } else {
                    asm.push_str(&format!("    mov {}, {}\n", d, s));
                }
            }
        }

        LoweredInst::LoadConst(dst, imm) => {
            let d = alloc_operand(*dst);
            if dst.is_stack() {
                // mov mem64, imm is limited to sign-extended imm32.
                // Always route through a scratch register for safety.
                asm.push_str(&format!("    movabs r11, {}\n", imm));
                asm.push_str(&format!("    mov {}, r11\n", d));
            } else if *imm < i32::MIN as i64 || *imm > i32::MAX as i64 {
                // imm doesn't fit in 32 bits → need movabs for a register.
                asm.push_str(&format!("    movabs {}, {}\n", d, imm));
            } else {
                asm.push_str(&format!("    mov {}, {}\n", d, imm));
            }
        }

        LoweredInst::BinaryOp(op, dst, lhs, rhs) => {
            emit_binary(asm, *op, *dst, *lhs, *rhs);
        }

        LoweredInst::Not(dst, src) => {
            let d = alloc_operand(*dst);
            let s = alloc_operand(*src);
            if d != s {
                asm.push_str(&format!("    mov {}, {}\n", d, s));
            }
            asm.push_str(&format!("    xor {}, 1\n", d));
        }

        LoweredInst::Print(src) => {
            let s = alloc_operand(*src);
            if s != "rsi" {
                asm.push_str(&format!("    mov rsi, {}\n", s));
            }
            asm.push_str("    lea rdi, [rip + .Lfmt_int]\n");
            asm.push_str("    xor eax, eax\n");
            asm.push_str("    call printf\n");
        }

        LoweredInst::Jmp(target) => {
            asm.push_str(&format!("    jmp .Lbb_{}\n", entry_bb + target));
        }

        LoweredInst::Br(cond, then_bb, else_bb) => {
            let c = alloc_operand(*cond);
            asm.push_str(&format!("    test {}, {}\n", c, c));
            asm.push_str(&format!("    jnz .Lbb_{}\n", entry_bb + then_bb));
            asm.push_str(&format!("    jmp .Lbb_{}\n", entry_bb + else_bb));
        }

        LoweredInst::Ret(src) => {
            let s = alloc_operand(*src);
            if s != "rax" {
                asm.push_str(&format!("    mov rax, {}\n", s));
            }
            // Restore callee-saved.
            asm.push_str("    pop r15\n");
            asm.push_str("    pop r14\n");
            asm.push_str("    pop r13\n");
            asm.push_str("    pop r12\n");
            asm.push_str("    pop rbx\n");
            asm.push_str("    mov rsp, rbp\n");
            asm.push_str("    pop rbp\n");
            asm.push_str("    ret\n");
        }

        LoweredInst::Nop => {
            // Emit nothing for nops.
        }

        LoweredInst::Call(target_bb) => {
            asm.push_str(&format!("    call .Lbb_{}\n", target_bb));
        }
    }
}

/// Emit a binary operation.
fn emit_binary(asm: &mut String, op: BinaryOp, dst: Allocation, lhs: Allocation, rhs: Allocation) {
    let d = alloc_operand(dst);
    let l = alloc_operand(lhs);
    let r = alloc_operand(rhs);

    match op {
        BinaryOp::Add | BinaryOp::Sub | BinaryOp::And | BinaryOp::Or => {
            let mnemonic = match op {
                BinaryOp::Add => "add",
                BinaryOp::Sub => "sub",
                BinaryOp::And => "and",
                BinaryOp::Or => "or",
                _ => unreachable!(),
            };
            if d == r && d != l {
                asm.push_str(&format!("    mov r11, {}\n", l));
                asm.push_str(&format!("    {} r11, {}\n", mnemonic, r));
                asm.push_str(&format!("    mov {}, r11\n", d));
            } else {
                if d != l {
                    asm.push_str(&format!("    mov {}, {}\n", d, l));
                }
                asm.push_str(&format!("    {} {}, {}\n", mnemonic, d, r));
            }
        }

        BinaryOp::Mul => {
            // Safety net: the regalloc constrains imul's destination to a
            // register (Constraint::Reg), so `dst.is_stack()` should never
            // hold here.  Kept as a fallback in case the constraint is
            // weakened or the allocator's pin-fallback (select_spill_candidate)
            // is ever forced to spill an otherwise-pinned var.
            if dst.is_stack() {
                asm.push_str(&format!("    mov r11, {}\n", l));
                asm.push_str(&format!("    imul r11, {}\n", r));
                asm.push_str(&format!("    mov {}, r11\n", d));
            } else if d == r && d != l {
                asm.push_str(&format!("    mov r11, {}\n", l));
                asm.push_str(&format!("    imul r11, {}\n", r));
                asm.push_str(&format!("    mov {}, r11\n", d));
            } else {
                if d != l {
                    asm.push_str(&format!("    mov {}, {}\n", d, l));
                }
                asm.push_str(&format!("    imul {}, {}\n", d, r));
            }
        }

        BinaryOp::Div => {
            
            asm.push_str("    cqo\n");
            asm.push_str(&format!("    idiv {}\n", r));
            if d != "rax" {
                asm.push_str(&format!("    mov {}, rax\n", d));
            }
        }

        BinaryOp::Eq | BinaryOp::Lt | BinaryOp::Gt | BinaryOp::Le | BinaryOp::Ge => {
            // cmp lhs, rhs; setCC low-byte-of-dst; movzx dst, low-byte
            asm.push_str(&format!("    cmp {}, {}\n", l, r));
            let dst_preg = alloc_preg(dst);
            let low = reg_name_8(dst_preg);
            let setcc = match op {
                BinaryOp::Eq => "sete",
                BinaryOp::Lt => "setl",
                BinaryOp::Gt => "setg",
                BinaryOp::Le => "setle",
                BinaryOp::Ge => "setge",
                _ => unreachable!(),
            };
            asm.push_str(&format!("    {} {}\n", setcc, low));
            asm.push_str(&format!("    movzx {}, {}\n", d, low));
        }
    }
}
