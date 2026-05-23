use anyhow::Result;

use crate::brilir::{
    builder::{BasicBlock, BasicBlockId, Builder},
    instruction::{BinaryOp, Immediate, IrInstruction, Variable},
};
use crate::codegen::future_active::RegisterAllocation;

fn imm_to_llvm_int(imm: &Immediate) -> String {
    match imm {
        Immediate::Int(i) => i.to_string(),
        Immediate::Bool(b) => {
            if *b {
                "1".to_string()
            } else {
                "0".to_string()
            }
        }
    }
}

pub fn emit_llvm_ir(builder: &Builder, allocation: &RegisterAllocation) -> Result<String> {
    let mut out = String::new();

    out.push_str("@formatString = private constant [6 x i8] c\"%lld\\0A\\00\"\n");
    out.push_str("declare i32 @printf(ptr, ...)\n\n");

    out.push_str("define i32 @main() {\n");
    out.push_str("entry:\n");
    for reg in 0..allocation.register_count() {
        out.push_str(&format!("  %r{} = alloca i64\n", reg));
    }
    out.push_str("  br label %bb_0\n");

    for block in builder.blocks.iter() {
        emit_block(builder, block, allocation, &mut out)?;
    }

    out.push_str("}\n");
    Ok(out)
}

fn emit_block(
    builder: &Builder,
    block: &BasicBlock,
    allocation: &RegisterAllocation,
    out: &mut String,
) -> Result<()> {
    out.push_str(&format!("bb_{}:\n", block.id));

    for (instr_index, instr) in block.instrs.iter().enumerate() {
        match instr {
            IrInstruction::Load(dst, imm) => {
                store_var(out, allocation, *dst, &imm_to_llvm_int(imm))?;
            }

            IrInstruction::Mov(dst, src) => {
                let src_value = load_var(
                    out,
                    allocation,
                    *src,
                    &format!("mov_src_{}_{}", block.id, instr_index),
                )?;
                store_var(out, allocation, *dst, &src_value)?;
            }

            IrInstruction::Binary(op, dst, lhs, rhs) => {
                let lhs_value = load_var(
                    out,
                    allocation,
                    *lhs,
                    &format!("bin_lhs_{}_{}", block.id, instr_index),
                )?;
                let rhs_value = load_var(
                    out,
                    allocation,
                    *rhs,
                    &format!("bin_rhs_{}_{}", block.id, instr_index),
                )?;
                let opcode = binary_opcode(op);

                match op {
                    BinaryOp::Eq | BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
                        let cmp = format!("%cmp_{}_{}", block.id, instr_index);
                        let zext = format!("%cmp_i64_{}_{}", block.id, instr_index);
                        out.push_str(&format!(
                            "  {} = icmp {} i64 {}, {}\n",
                            cmp, opcode, lhs_value, rhs_value
                        ));
                        out.push_str(&format!("  {} = zext i1 {} to i64\n", zext, cmp));
                        store_var(out, allocation, *dst, &zext)?;
                    }
                    _ => {
                        let result = format!("%bin_{}_{}", block.id, instr_index);
                        out.push_str(&format!(
                            "  {} = {} i64 {}, {}\n",
                            result, opcode, lhs_value, rhs_value
                        ));
                        store_var(out, allocation, *dst, &result)?;
                    }
                }
            }

            IrInstruction::Not(dst, src) => {
                let src_value = load_var(
                    out,
                    allocation,
                    *src,
                    &format!("not_src_{}_{}", block.id, instr_index),
                )?;
                let cmp = format!("%not_cmp_{}_{}", block.id, instr_index);
                let zext = format!("%not_i64_{}_{}", block.id, instr_index);
                out.push_str(&format!("  {} = icmp eq i64 {}, 0\n", cmp, src_value));
                out.push_str(&format!("  {} = zext i1 {} to i64\n", zext, cmp));
                store_var(out, allocation, *dst, &zext)?;
            }

            IrInstruction::Print(src) => {
                let src_value = load_var(
                    out,
                    allocation,
                    *src,
                    &format!("print_src_{}_{}", block.id, instr_index),
                )?;
                out.push_str(&format!(
                    "  %fmt_ptr_{}_{} = getelementptr inbounds [6 x i8], ptr @formatString, i32 0, i32 0\n",
                    block.id, instr_index
                ));
                out.push_str(&format!(
                    "  %print_ret_{}_{} = call i32 (ptr, ...) @printf(ptr %fmt_ptr_{}_{}, i64 {})\n",
                    block.id, instr_index, block.id, instr_index, src_value
                ));
            }

            IrInstruction::Jmp(target) => {
                emit_phi_copies(builder, block.id, *target, allocation, out)?;
                out.push_str(&format!("  br label %bb_{}\n", target));
            }

            IrInstruction::Br(cond, then_bb, else_bb) => {
                let cond_value = load_var(
                    out,
                    allocation,
                    *cond,
                    &format!("br_src_{}_{}", block.id, instr_index),
                )?;
                let tmp = format!("%br_cond_{}_{}", block.id, instr_index);
                out.push_str(&format!("  {} = icmp ne i64 {}, 0\n", tmp, cond_value));

                let then_label = branch_target_label(builder, block.id, *then_bb);
                let else_label = branch_target_label(builder, block.id, *else_bb);
                out.push_str(&format!(
                    "  br i1 {}, label %{}, label %{}\n",
                    tmp, then_label, else_label
                ));

                emit_conditional_edge_block(builder, block.id, *then_bb, allocation, out)?;
                if else_bb != then_bb {
                    emit_conditional_edge_block(builder, block.id, *else_bb, allocation, out)?;
                }
            }

            IrInstruction::Ret(var) => {
                let ret_value = load_var(
                    out,
                    allocation,
                    *var,
                    &format!("ret_src_{}_{}", block.id, instr_index),
                )?;
                let tmp = format!("%ret32_{}_{}", block.id, instr_index);
                out.push_str(&format!("  {} = trunc i64 {} to i32\n", tmp, ret_value));
                out.push_str(&format!("  ret i32 {}\n", tmp));
            }

            IrInstruction::Call | IrInstruction::Nop => {}
        }
    }

    let is_terminated = block.instrs.last().is_some_and(|i| {
        matches!(
            i,
            IrInstruction::Jmp(_) | IrInstruction::Br(..) | IrInstruction::Ret(_)
        )
    });

    if !is_terminated && let Some(&succ) = block.successors.iter().next() {
        emit_phi_copies(builder, block.id, succ, allocation, out)?;
        out.push_str(&format!("  br label %bb_{}\n", succ));
    }

    Ok(())
}

fn branch_target_label(builder: &Builder, pred: BasicBlockId, succ: BasicBlockId) -> String {
    if builder.blocks[succ].phis.is_empty() {
        format!("bb_{}", succ)
    } else {
        format!("bb_{}_to_{}", pred, succ)
    }
}

fn emit_conditional_edge_block(
    builder: &Builder,
    pred: BasicBlockId,
    succ: BasicBlockId,
    allocation: &RegisterAllocation,
    out: &mut String,
) -> Result<()> {
    if builder.blocks[succ].phis.is_empty() {
        return Ok(());
    }

    out.push_str(&format!("bb_{}_to_{}:\n", pred, succ));
    emit_phi_copies(builder, pred, succ, allocation, out)?;
    out.push_str(&format!("  br label %bb_{}\n", succ));
    Ok(())
}

fn emit_phi_copies(
    builder: &Builder,
    pred: BasicBlockId,
    succ: BasicBlockId,
    allocation: &RegisterAllocation,
    out: &mut String,
) -> Result<()> {
    let mut copies = Vec::new();

    for (phi_index, phi) in builder.blocks[succ].phis.iter().enumerate() {
        if let Some((src, _)) = phi.operands.iter().find(|(_, block)| *block == pred) {
            let src_reg = allocation.register(*src)?;
            let dst_reg = allocation.register(phi.var)?;
            if src_reg == dst_reg {
                continue;
            }

            let tmp = format!("%phi_{}_{}_{}", pred, succ, phi_index);
            out.push_str(&format!("  {} = load i64, ptr %r{}\n", tmp, src_reg));
            copies.push((dst_reg, tmp));
        }
    }

    for (dst_reg, tmp) in copies {
        out.push_str(&format!("  store i64 {}, ptr %r{}\n", tmp, dst_reg));
    }

    Ok(())
}

fn load_var(
    out: &mut String,
    allocation: &RegisterAllocation,
    var: Variable,
    name: &str,
) -> Result<String> {
    let reg = allocation.register(var)?;
    let tmp = format!("%{}", name);
    out.push_str(&format!("  {} = load i64, ptr %r{}\n", tmp, reg));
    Ok(tmp)
}

fn store_var(
    out: &mut String,
    allocation: &RegisterAllocation,
    var: Variable,
    value: &str,
) -> Result<()> {
    let reg = allocation.register(var)?;
    out.push_str(&format!("  store i64 {}, ptr %r{}\n", value, reg));
    Ok(())
}

fn binary_opcode(op: &BinaryOp) -> &'static str {
    match op {
        BinaryOp::Add => "add",
        BinaryOp::Sub => "sub",
        BinaryOp::Mul => "mul",
        BinaryOp::Div => "sdiv",
        BinaryOp::And => "and",
        BinaryOp::Or => "or",
        BinaryOp::Eq => "eq",
        BinaryOp::Lt => "slt",
        BinaryOp::Le => "sle",
        BinaryOp::Gt => "sgt",
        BinaryOp::Ge => "sge",
    }
}
