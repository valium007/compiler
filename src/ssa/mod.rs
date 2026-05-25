pub mod critical_edge;
pub mod ir;
pub mod parallel_move;
pub mod phi_to_move;
use crate::brilir;

use brilir::builder::Builder as BrilBuilder;
use brilir::instruction::{
    BinaryOp as BrilBinaryOp, Value, IrInstruction as BrilIrInstruction, Variable,
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
fn immediate_to_ssa_value(imm: Value) -> SsaValue {
    match imm {
        Value::Int(i) => SsaValue::Int(i),
        Value::Bool(b) => SsaValue::Bool(b),
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
    builder.name = bril.name.clone();

    for bril_block in bril.blocks.iter() {
        let preds: Vec<usize> = bril_block.predecessors.iter().copied().collect();
        let succs: Vec<usize> = bril_block.successors.iter().copied().collect();

        builder.add_block(bril_block.id, preds, succs);

        // Register function parameters as definitions in the entry block so
        // any read inside the function body resolves to these SSA variables.
        if bril_block.id == 0 {
            for &param_var in &bril.params {
                let ssa_var = builder.write_variable(param_var.0, 0);
                builder.params.push(ssa_var);
            }
        }

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
                    if truthy == falsy {
                        // `br cond X X` — both arms target the same block.
                        // Lower to unconditional Jmp and DROP the cond read
                        // entirely (skipping `as_ssa_value` avoids creating
                        // a dangling phi for the cond if it requires a
                        // recursive lookup).
                        let _ = cond;
                        builder.add_instr(IrInstruction::Jmp(*truthy));
                    } else {
                        let cond_val = as_ssa_value(&mut builder, *cond);
                        builder.add_instr(IrInstruction::Br(cond_val, *truthy, *falsy));
                    }
                }

                BrilIrInstruction::Ret(src) => {
                    let src_val = as_ssa_value(&mut builder, *src);
                    builder.add_instr(IrInstruction::Ret(src_val));
                }

                BrilIrInstruction::Call { callee_bb, args, dest } => {
                    let mut arg_vals = Vec::new();
                    for &arg_var in args.iter() {
                        arg_vals.push(as_ssa_value(&mut builder, arg_var));
                    }
                    let ssa_dest = if let Some(dst_var) = dest {
                        let bb = builder.current_block_id();
                        let ssa_var = builder.write_variable(dst_var.0, bb);
                        Some(ssa_var)
                    } else {
                        None
                    };
                    builder.add_instr(IrInstruction::Call {
                        callee_bb: *callee_bb,
                        args: arg_vals,
                        dest: ssa_dest,
                    });
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

    // ── Algorithm 5: remove superfluous φ SCCs (irreducible control flow) ────
    builder.remove_redundant_phis();

    // Second Nop-cleanup: Algorithm 5 replaces eliminated phis with Nop.
    for block in builder.blocks.iter_mut() {
        let mut phis = Vec::new();
        let mut rest = Vec::new();
        for instr in block.instrs.drain(..) {
            match instr {
                IrInstruction::Nop => {}
                IrInstruction::PhiAssign(_) => phis.push(instr),
                _ => rest.push(instr),
            }
        }
        phis.append(&mut rest);
        block.instrs = phis;
    }


    builder
}

/// Drop blocks that are unreachable from the entry block (bb_0).
///
/// After SSA construction the IR may contain blocks with no predecessors
/// (other than the entry).  These are dead — they have no incoming control
/// flow — but regalloc2 will still try to observe the classes of any vregs
/// it sees, and panic on vregs that only appear in unreachable code.
///
/// This pass:
///   1. Computes the reachable set via BFS from bb_0.
///   2. For each unreachable block, clears its instructions and successor list.
///   3. Removes the unreachable block from its successors' predecessor lists.
///   4. Drops phi operands that came from unreachable predecessors.
///   5. Re-runs trivial-phi removal on phis that lost operands.
pub fn prune_unreachable(builder: &mut Builder) {
    use ir::IrInstruction;
    let n = builder.blocks.len();
    if n == 0 { return; }

    let mut reachable = vec![false; n];
    reachable[0] = true;
    let mut queue: Vec<usize> = vec![0];
    while let Some(b) = queue.pop() {
        let succs = builder.blocks[b].successors.clone();
        for s in succs {
            if !reachable[s] {
                reachable[s] = true;
                queue.push(s);
            }
        }
    }

    let mut phis_to_recheck: Vec<(usize, usize)> = Vec::new();

    for b in 0..n {
        if reachable[b] { continue; }
        // Leave the unreachable block's body intact (so vregs defined here
        // still have a def in the SSA, even though the block is dead).
        // Just sever its outgoing edges so it can't pollute successors'
        // liveness via phi operands.
        let old_succs = builder.blocks[b].successors.clone();
        builder.blocks[b].successors.clear();

        for s in old_succs {
            builder.blocks[s].predecessors.retain(|&p| p != b);
            for (idx, inst) in builder.blocks[s].instrs.iter_mut().enumerate() {
                if let IrInstruction::PhiAssign(phi) = inst {
                    let before = phi.operands.len();
                    phi.operands.retain(|(_, from)| *from != b);
                    if phi.operands.len() != before {
                        phis_to_recheck.push((s, idx));
                    }
                }
            }
        }
    }

    // Recheck phis that lost operands — they may now be trivial.
    for (block, inst) in phis_to_recheck {
        if builder.blocks[block].instrs.get(inst)
            .map(|i| i.is_phi()).unwrap_or(false)
        {
            builder.try_remove_trivial_phi(inst, block);
        }
    }

    // Drop Nops that try_remove_trivial_phi may have left behind.
    for block in builder.blocks.iter_mut() {
        block.instrs.retain(|i| !matches!(i, IrInstruction::Nop));
    }
}
