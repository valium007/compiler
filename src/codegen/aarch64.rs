//! AArch64 (ARM64) instruction selection and assembly emission.
//!
//! Translates `LoweredInst` (post-regalloc) into GNU-as AArch64 assembly
//! targeting Linux (AAPCS64 calling convention).

use crate::regalloc::regalloc_ir::{Allocation, PhysReg};

use crate::regalloc::regalloc_ir::LoweredInst;
use crate::ssa::ir::BinaryOp;

// ── Register names ───────────────────────────────────────────────────────

/// Map a `PhysReg` hw_enc (0..28) to its 64-bit register name.
fn reg_name(preg: PhysReg) -> &'static str {
    match preg.hw_enc() {
        0  => "x0",   1  => "x1",   2  => "x2",   3  => "x3",
        4  => "x4",   5  => "x5",   6  => "x6",   7  => "x7",
        8  => "x8",   9  => "x9",   10 => "x10",  11 => "x11",
        12 => "x12",  13 => "x13",  14 => "x14",  15 => "x15",
        16 => "x16",  17 => "x17",  18 => "x18",  19 => "x19",
        20 => "x20",  21 => "x21",  22 => "x22",  23 => "x23",
        24 => "x24",  25 => "x25",  26 => "x26",  27 => "x27",
        28 => "x28",  29 => "x29",  30 => "x30",
        n  => panic!("unknown AArch64 PhysReg hw_enc {}", n),
    }
}

/// Map a `PhysReg` hw_enc to its 32-bit (w-register) name (for cset, etc.).
fn reg_name_32(preg: PhysReg) -> &'static str {
    match preg.hw_enc() {
        0  => "w0",   1  => "w1",   2  => "w2",   3  => "w3",
        4  => "w4",   5  => "w5",   6  => "w6",   7  => "w7",
        8  => "w8",   9  => "w9",   10 => "w10",  11 => "w11",
        12 => "w12",  13 => "w13",  14 => "w14",  15 => "w15",
        16 => "w16",  17 => "w17",  18 => "w18",  19 => "w19",
        20 => "w20",  21 => "w21",  22 => "w22",  23 => "w23",
        24 => "w24",  25 => "w25",  26 => "w26",  27 => "w27",
        28 => "w28",  29 => "w29",  30 => "w30",
        n  => panic!("unknown AArch64 PhysReg hw_enc {} for 32-bit", n),
    }
}

/// Format an `Allocation` as an assembly operand.
fn alloc_operand(alloc: Allocation) -> String {
    match alloc {
        Allocation::Reg(preg) => {
            reg_name(preg).to_string()
        }
        Allocation::Stack(slot) => {
            // Each spill slot is 8 bytes, addressed relative to sp.
            let offset = slot.index() * 8;
            format!("[sp, #{}]", offset)
        }
        _ => panic!("unexpected allocation kind"),
    }
}

/// Get the PhysReg from an allocation (panics if it's a stack slot).
fn alloc_preg(alloc: Allocation) -> PhysReg {
    alloc.as_reg().expect("expected register allocation")
}

// ── Assembly emission ────────────────────────────────────────────────────

/// Descriptor for one function in a multi-function program.
pub struct FunctionOutput<'a> {
    pub name:          &'a str,
    pub entry_bb:      usize,
    pub blocks:        &'a [Vec<LoweredInst>],
    pub num_spillslots: usize,
}

/// Emit a complete AArch64 program (one or more functions).
pub fn emit_program(fns: &[FunctionOutput<'_>]) -> String {
    let mut asm = String::new();

    // Global header (once per program).
    asm.push_str(".arch armv8-a\n");
    asm.push_str(".global main\n");
    asm.push_str(".type main, %function\n");
    asm.push_str(".text\n\n");
    emit_print_helper(&mut asm);

    for f in fns {
        emit_one_fn(&mut asm, f.name, f.entry_bb, f.blocks, f.num_spillslots);
    }

    asm
}

/// Emit the full function as AArch64 assembly (GNU as syntax, Linux).
/// Kept for backward compatibility with the single-function pipelines.
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
    let stack_size = (num_spillslots * 8 + 15) & !15;

    // ── Function entry labels ─────────────────────────────────────────
    // Both the function name AND the first block's global label must be at
    // the same address, BEFORE the prologue.  Calls use `bl .Lbb_N` so if
    // the prologue came first the callee would be entered without a frame.
    asm.push_str(&format!("\n.type {}, %function\n", fn_name));
    asm.push_str(&format!("{}:\n", fn_name));
    asm.push_str(&format!(".Lbb_{}:\n", entry_bb));   // ← before prologue

    // ── Function prologue ─────────────────────────────────────────────
    asm.push_str("    stp x29, x30, [sp, #-16]!\n");
    asm.push_str("    mov x29, sp\n");
    asm.push_str("    stp x19, x20, [sp, #-16]!\n");
    asm.push_str("    stp x21, x22, [sp, #-16]!\n");
    asm.push_str("    stp x23, x24, [sp, #-16]!\n");
    asm.push_str("    stp x25, x26, [sp, #-16]!\n");
    asm.push_str("    stp x27, x28, [sp, #-16]!\n");
    asm.push_str(&format!("    mov x16, #{}\n", stack_size));
    asm.push_str("    sub sp, sp, x16\n\n");

    // ── Block bodies ──────────────────────────────────────────────────
    for (local_idx, block_insts) in blocks.iter().enumerate() {
        // Block 0's label was already emitted above (merged with fn entry).
        if local_idx > 0 {
            let global_bb = entry_bb + local_idx;
            asm.push_str(&format!(".Lbb_{}:\n", global_bb));
        }

        let mut i = 0;
        while i < block_insts.len() {
            // Peephole: fuse (comparison BinaryOp, Br) → cmp + b.<cond>.
            if i + 1 < block_insts.len() {
                if let LoweredInst::BinaryOp(op, dst, lhs, rhs) = &block_insts[i] {
                    if is_comparison(*op) {
                        if let LoweredInst::Br(cond, then_bb, else_bb) = &block_insts[i + 1] {
                            if cond == dst {
                                emit_cmp_branch(asm, *op, *lhs, *rhs,
                                    entry_bb + then_bb, entry_bb + else_bb);
                                i += 2;
                                continue;
                            }
                        }
                    }
                }
            }
            emit_instruction(asm, &block_insts[i], stack_size, entry_bb);
            i += 1;
        }
        asm.push('\n');
    }
}

/// Emit a single lowered instruction as AArch64 assembly.
fn emit_instruction(asm: &mut String, inst: &LoweredInst, stack_size: usize, entry_bb: usize) {
    match inst {
        LoweredInst::Mov(dst, src) => {
            let d = alloc_operand(*dst);
            let s = alloc_operand(*src);
            if d != s {
                match (dst, src) {
                    // reg ← reg
                    (Allocation::Reg(_), Allocation::Reg(_)) => {
                        asm.push_str(&format!("    mov {}, {}\n", d, s));
                    }
                    // reg ← stack (load)
                    (Allocation::Reg(_), Allocation::Stack(_)) => {
                        asm.push_str(&format!("    ldr {}, {}\n", d, s));
                    }
                    // stack ← reg (store)
                    (Allocation::Stack(_), Allocation::Reg(_)) => {
                        asm.push_str(&format!("    str {}, {}\n", s, d));
                    }
                    // stack ← stack (via scratch x16)
                    (Allocation::Stack(_), Allocation::Stack(_)) => {
                        asm.push_str(&format!("    ldr x16, {}\n", s));
                        asm.push_str(&format!("    str x16, {}\n", d));
                    }
                    _ => panic!("unexpected allocation kinds in Mov"),
                }
            }
        }

        LoweredInst::LoadConst(dst, imm) => {
            let d = alloc_operand(*dst);
            if dst.is_stack() {
                // Load immediate into scratch, then store to stack.
                emit_load_imm(asm, "x16", *imm);
                asm.push_str(&format!("    str x16, {}\n", d));
            } else {
                emit_load_imm(asm, &d, *imm);
            }
        }

        LoweredInst::BinaryOp(op, dst, lhs, rhs) => {
            emit_binary(asm, *op, *dst, *lhs, *rhs);
        }

        LoweredInst::Not(dst, src) => {
            let d = alloc_operand(*dst);
            let s = alloc_operand(*src);
            // not(bool) = xor with 1
            if d != s {
                asm.push_str(&format!("    mov {}, {}\n", d, s));
            }
            asm.push_str(&format!("    eor {}, {}, #1\n", d, d));
        }

        LoweredInst::Print(src) => {
            // AAPCS64: first argument in x0.
            let s = alloc_operand(*src);
            if s != "x0" {
                asm.push_str(&format!("    mov x0, {}\n", s));
            }
            asm.push_str("    bl print_int\n");
        }

        LoweredInst::Jmp(target) => {
            asm.push_str(&format!("    b .Lbb_{}\n", entry_bb + target));
        }

        LoweredInst::Br(cond, then_bb, else_bb) => {
            let c = alloc_operand(*cond);
            // Generic boolean branch: cond is a 0/1 integer value.
            asm.push_str(&format!("    cbnz {}, .Lbb_{}\n", c, entry_bb + then_bb));
            asm.push_str(&format!("    b .Lbb_{}\n", entry_bb + else_bb));
        }

        LoweredInst::Ret(src) => {
            let s = alloc_operand(*src);
            if s != "x0" {
                asm.push_str(&format!("    mov x0, {}\n", s));
            }
            // Deallocate spill slots.
            asm.push_str(&format!("    mov x16, #{}\n", stack_size));
            asm.push_str("    add sp, sp, x16\n");
            // Restore callee-saved registers (reverse order of saves).
            asm.push_str("    ldp x27, x28, [sp], #16\n");
            asm.push_str("    ldp x25, x26, [sp], #16\n");
            asm.push_str("    ldp x23, x24, [sp], #16\n");
            asm.push_str("    ldp x21, x22, [sp], #16\n");
            asm.push_str("    ldp x19, x20, [sp], #16\n");
            // Restore frame pointer and link register.
            asm.push_str("    ldp x29, x30, [sp], #16\n");
            asm.push_str("    ret\n");
        }

        LoweredInst::Nop => {
            // Emit nothing.
        }

        LoweredInst::Call(target_bb) => {
            asm.push_str(&format!("    bl .Lbb_{}\n", target_bb));
        }
    }
}

/// Returns true if `op` is a comparison that sets flags (eq/lt/gt/le/ge).
fn is_comparison(op: BinaryOp) -> bool {
    matches!(op, BinaryOp::Eq | BinaryOp::Lt | BinaryOp::Gt | BinaryOp::Le | BinaryOp::Ge)
}

/// Emit a fused compare-and-branch: `cmp lhs, rhs` + `b.<cond> then` + `b else`.
///
/// This replaces the three-instruction sequence:
///   cmp lhs, rhs
///   cset w<tmp>, <cond>
///   cbnz w<tmp>, .Lbb_then
///   b .Lbb_else
fn emit_cmp_branch(
    asm: &mut String,
    op: BinaryOp,
    lhs: Allocation,
    rhs: Allocation,
    then_bb: usize,
    else_bb: usize,
) {
    let l = alloc_operand(lhs);
    let r = alloc_operand(rhs);
    let bcond = match op {
        BinaryOp::Eq => "beq",
        BinaryOp::Lt => "blt",
        BinaryOp::Gt => "bgt",
        BinaryOp::Le => "ble",
        BinaryOp::Ge => "bge",
        _ => unreachable!("emit_cmp_branch called with non-comparison op"),
    };
    asm.push_str(&format!("    cmp {}, {}\n", l, r));
    asm.push_str(&format!("    {} .Lbb_{}\n", bcond, then_bb));
    asm.push_str(&format!("    b .Lbb_{}\n", else_bb));
}

/// Emit a binary operation.
fn emit_binary(asm: &mut String, op: BinaryOp, dst: Allocation, lhs: Allocation, rhs: Allocation) {
    let d = alloc_operand(dst);
    let l = alloc_operand(lhs);
    let r = alloc_operand(rhs);

    match op {
        BinaryOp::Add => {
            asm.push_str(&format!("    add {}, {}, {}\n", d, l, r));
        }
        BinaryOp::Sub => {
            asm.push_str(&format!("    sub {}, {}, {}\n", d, l, r));
        }
        BinaryOp::Mul => {
            asm.push_str(&format!("    mul {}, {}, {}\n", d, l, r));
        }
        BinaryOp::Div => {
            // AArch64 has a proper sdiv instruction — no rdx:rax gymnastics!
            asm.push_str(&format!("    sdiv {}, {}, {}\n", d, l, r));
        }
        BinaryOp::And => {
            asm.push_str(&format!("    and {}, {}, {}\n", d, l, r));
        }
        BinaryOp::Or => {
            asm.push_str(&format!("    orr {}, {}, {}\n", d, l, r));
        }

        BinaryOp::Eq | BinaryOp::Lt | BinaryOp::Gt | BinaryOp::Le | BinaryOp::Ge => {
            // cmp lhs, rhs; cset dst, <cond>
            asm.push_str(&format!("    cmp {}, {}\n", l, r));
            let dst_preg = alloc_preg(dst);
            let cond = match op {
                BinaryOp::Eq => "eq",
                BinaryOp::Lt => "lt",
                BinaryOp::Gt => "gt",
                BinaryOp::Le => "le",
                BinaryOp::Ge => "ge",
                _ => unreachable!(),
            };
            // cset writes to a 32-bit (w) register and zero-extends.
            let w = reg_name_32(dst_preg);
            asm.push_str(&format!("    cset {}, {}\n", w, cond));
        }
    }
}

/// Emit an immediate load.  ARM64 `mov` can only handle 16-bit immediates
/// directly.  For larger values we use `movz` + `movk` sequences.
fn emit_load_imm(asm: &mut String, dst: &str, imm: i64) {
    let val = imm as u64;

    if imm >= 0 && imm <= 0xFFFF {
        // Simple case: fits in 16 bits.
        asm.push_str(&format!("    mov {}, #{}\n", dst, imm));
        return;
    }

    if imm >= -0x10000 && imm < 0 {
        // Small negative: use movn (move wide with NOT).
        let inverted = !val & 0xFFFF;
        asm.push_str(&format!("    movn {}, #{}\n", dst, inverted));
        return;
    }

    // General case: up to 4 × 16-bit chunks via movz + movk.
    // Find the first non-zero chunk for movz, then use movk for the rest.
    let mut emitted_movz = false;
    for shift in (0..64).step_by(16) {
        let chunk = (val >> shift) & 0xFFFF;
        if chunk == 0 {
            continue;
        }
        if !emitted_movz {
            asm.push_str(&format!("    movz {}, {:#x}, lsl #{}\n", dst, chunk, shift));
            emitted_movz = true;
        } else {
            asm.push_str(&format!("    movk {}, {:#x}, lsl #{}\n", dst, chunk, shift));
        }
    }
    // If val was somehow 0 but we got here (shouldn't happen — caught above),
    // fall back to mov.
    if !emitted_movz {
        asm.push_str(&format!("    mov {}, #0\n", dst));
    }
}

/// Emit a small `print_int` helper that calls printf.
/// AAPCS64: x0 = format string, x1 = value.
fn emit_print_helper(asm: &mut String) {
    asm.push_str(".section .rodata\n");
    asm.push_str(".Lfmt_int:\n");
    asm.push_str("    .asciz \"%lld\\n\"\n");
    asm.push_str(".text\n\n");

    asm.push_str(".type print_int, %function\n");
    asm.push_str("print_int:\n");
    // Save lr (x30) — we're going to bl printf.
    asm.push_str("    stp x29, x30, [sp, #-16]!\n");
    asm.push_str("    mov x29, sp\n");
    // Move the value (x0) to x1 (second arg for printf).
    asm.push_str("    mov x1, x0\n");
    // Load format string address into x0 (first arg).
    asm.push_str("    adrp x0, .Lfmt_int\n");
    asm.push_str("    add x0, x0, :lo12:.Lfmt_int\n");
    // Call printf.
    asm.push_str("    bl printf\n");
    // Restore and return.
    asm.push_str("    ldp x29, x30, [sp], #16\n");
    asm.push_str("    ret\n\n");
}
