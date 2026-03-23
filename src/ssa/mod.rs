pub mod ir;
use crate::brilir;

use brilir::builder::Builder as BrilBuilder;
use brilir::instruction::{
    BinaryOp as BrilBinaryOp, Immediate, IrInstruction as BrilIrInstruction, Variable,
};
use ir::Builder;
use ir::{BinaryOp as SsaBinaryOp, IrInstruction, SsaValue};

/// Convert a brilir BinaryOp to an SSA BinaryOp (1:1 mapping).
fn convert_binary_op(op: BrilBinaryOp) -> SsaBinaryOp {
    match op {
        BrilBinaryOp::Add => SsaBinaryOp::Add,
        BrilBinaryOp::Sub => SsaBinaryOp::Sub,
        BrilBinaryOp::Mul => SsaBinaryOp::Mul,
        BrilBinaryOp::Div => SsaBinaryOp::Div,
        BrilBinaryOp::Eq => SsaBinaryOp::Eq,
        BrilBinaryOp::Lt => SsaBinaryOp::Lt,
        BrilBinaryOp::Gt => SsaBinaryOp::Gt,
        BrilBinaryOp::Le => SsaBinaryOp::Le,
        BrilBinaryOp::Ge => SsaBinaryOp::Ge,
        BrilBinaryOp::And => SsaBinaryOp::And,
        BrilBinaryOp::Or => SsaBinaryOp::Or,
    }
}

/// Convert a brilir Immediate to an SsaValue (Int or Bool).
fn immediate_to_ssa_value(imm: Immediate) -> SsaValue {
    match imm {
        Immediate::Int(i) => SsaValue::Int(i),
        Immediate::Bool(b) => SsaValue::Bool(b),
    }
}

/// Read a brilir Variable through the SSA builder, returning an SsaValue::Var.
fn as_ssa_value(builder: &mut Builder, var: Variable) -> SsaValue {
    let bb = builder.current_block_id();
    let ssa_var = builder.read_variable(var.0, bb);
    SsaValue::Var(ssa_var)
}

/// Lift a brilir::Builder (non-SSA IR with CFG edges already built)
/// into SSA form using ssa::ir::Builder (Braun's algorithm).
pub fn build_ssa(bril: &BrilBuilder) -> Builder {
    let mut builder = Builder::new();

    // Use the brilir builder's tracked next_var_id (already accounts for the
    // synthetic ret variable if one was added) so get_fresh_var() doesn't
    // collide with original variable IDs.
    builder.next_var_id = bril.next_var_id;

    for bril_block in bril.blocks.iter() {
        let preds: Vec<usize> = bril_block.predecessors.iter().copied().collect();
        let succs: Vec<usize> = bril_block.successors.iter().copied().collect();

        builder.add_block(bril_block.id, preds, succs);

        for instr in bril_block.instrs.iter() {
            match instr {
                BrilIrInstruction::Load(dst, imm) => {
                    let bb = builder.current_block_id();
                    let dst_var = builder.write_variable(dst.0, bb);
                    let val = immediate_to_ssa_value(*imm);
                    builder.add_instr(IrInstruction::Const(dst_var, val));
                }

                BrilIrInstruction::Mov(dst, src) => {
                    let src_val = as_ssa_value(&mut builder, *src);
                    let bb = builder.current_block_id();
                    let dst_var = builder.write_variable(dst.0, bb);
                    builder.add_instr(IrInstruction::Mov(dst_var, src_val));
                }

                BrilIrInstruction::Binary(op, dst, lhs, rhs) => {
                    let lhs_val = as_ssa_value(&mut builder, *lhs);
                    let rhs_val = as_ssa_value(&mut builder, *rhs);
                    let bb = builder.current_block_id();
                    let dst_var = builder.write_variable(dst.0, bb);
                    let ssa_op = convert_binary_op(*op);
                    builder.add_instr(IrInstruction::Binary(ssa_op, dst_var, lhs_val, rhs_val));
                }

                BrilIrInstruction::Not(dst, src) => {
                    let src_val = as_ssa_value(&mut builder, *src);
                    let bb = builder.current_block_id();
                    let dst_var = builder.write_variable(dst.0, bb);
                    builder.add_instr(IrInstruction::Not(dst_var, src_val));
                }

                BrilIrInstruction::Print(src) => {
                    let src_val = as_ssa_value(&mut builder, *src);
                    builder.add_instr(IrInstruction::Print(src_val));
                }

                BrilIrInstruction::Jmp(target_bb) => {
                    builder.add_instr(IrInstruction::Jmp(*target_bb));
                }

                BrilIrInstruction::Br(cond, truthy, falsy) => {
                    let cond_val = as_ssa_value(&mut builder, *cond);
                    builder.add_instr(IrInstruction::Br(cond_val, *truthy, *falsy));
                }

                BrilIrInstruction::Ret(src) => {
                    let src_val = as_ssa_value(&mut builder, *src);
                    builder.add_instr(IrInstruction::Ret(src_val));
                }

                BrilIrInstruction::Call => {
                    builder.add_instr(IrInstruction::Call);
                }

                BrilIrInstruction::Nop => {
                    builder.add_instr(IrInstruction::Nop);
                }
            }
        }
    }

    // Seal all blocks after lifting is complete, so all definitions
    // are in place before phi resolution runs.
    let block_ids: Vec<usize> = builder.blocks.iter().map(|b| b.id).collect();
    for bb in block_ids {
        builder.seal_block(bb);
    }

    // Cleanup: reorder so phis come first, remove Nops.
    for block in builder.blocks.iter_mut() {
        let mut phis = Vec::new();
        let mut rest = Vec::new();
        for instr in block.instrs.drain(..) {
            match instr {
                IrInstruction::Nop => {} // drop
                IrInstruction::PhiAssign(_) => phis.push(instr),
                _ => rest.push(instr),
            }
        }
        phis.append(&mut rest);
        block.instrs = phis;
    }

    builder
}
