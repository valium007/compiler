pub mod critical_edge;
pub mod ir;
pub mod parallel_move;
pub mod phi_to_move;
pub mod passes;
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
                    builder.add_instr(IrInstruction::Const(SsaValue::Var(dst_var), val));
                }

                BrilIrInstruction::Mov(dst, src) => {
                    let src_val = as_ssa_value(&mut builder, *src);
                    let bb = builder.current_block_id();
                    let dst_var = builder.write_variable(dst.0, bb);
                    builder.add_instr(IrInstruction::Mov(SsaValue::Var(dst_var), src_val));
                }

                BrilIrInstruction::Binary(op, dst, lhs, rhs) => {
                    let lhs_val = as_ssa_value(&mut builder, *lhs);
                    let rhs_val = as_ssa_value(&mut builder, *rhs);
                    let bb = builder.current_block_id();
                    let dst_var = builder.write_variable(dst.0, bb);
                    let ssa_op = convert_binary_op(*op);
                    builder.add_instr(IrInstruction::Binary(
                        ssa_op,
                        SsaValue::Var(dst_var),
                        lhs_val,
                        rhs_val,
                    ));
                }

                BrilIrInstruction::Not(dst, src) => {
                    let src_val = as_ssa_value(&mut builder, *src);
                    let bb = builder.current_block_id();
                    let dst_var = builder.write_variable(dst.0, bb);
                    builder.add_instr(IrInstruction::Not(SsaValue::Var(dst_var), src_val));
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
                        Some(SsaValue::Var(ssa_var))
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

/// Drop blocks that are unreachable from the entry block (bb_0) and compact
/// the block Vec, rewriting all block-ID references throughout the builder.
///
/// Steps:
///   1. BFS from bb_0 to compute the reachable set.
///   2. For each unreachable block, drop its phi operands from reachable
///      successors (and re-run trivial-phi removal on those phis).
///   3. Build a remapping table: old_id → new_id (None for removed blocks).
///   4. Filter builder.blocks to only reachable blocks.
///   5. Rewrite every block-ID reference: block.id, block.successors,
///      block.predecessors, phi.block, phi operand pred IDs.
pub fn prune_unreachable(builder: &mut Builder) {
    use ir::IrInstruction;
    let n = builder.blocks.len();
    if n == 0 { return; }

    // ── Step 1: BFS reachability ──────────────────────────────────────────
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

    // If everything is reachable there is nothing to do.
    if reachable.iter().all(|&r| r) { return; }

    // ── Step 2: sever dead predecessors from reachable successors ─────────
    let mut phis_to_recheck: Vec<(usize, usize)> = Vec::new();
    for b in 0..n {
        if reachable[b] { continue; }
        let old_succs = builder.blocks[b].successors.clone();
        for s in old_succs {
            if !reachable[s] { continue; }
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

    // Re-run trivial-phi removal on phis that lost operands.
    for (block, inst) in phis_to_recheck {
        if builder.blocks[block].instrs.get(inst)
            .map(|i| i.is_phi()).unwrap_or(false)
        {
            builder.try_remove_trivial_phi(inst, block);
        }
    }

    // Drop Nops from trivial-phi removal.
    for block in builder.blocks.iter_mut() {
        block.instrs.retain(|i| !matches!(i, IrInstruction::Nop));
    }

    // ── Step 3: build old_id → new_id remapping table ────────────────────
    // new_id[old] = Some(compacted index) for reachable blocks, None for dead.
    let mut new_id: Vec<Option<usize>> = vec![None; n];
    let mut counter = 0usize;
    for old in 0..n {
        if reachable[old] {
            new_id[old] = Some(counter);
            counter += 1;
        }
    }

    // Helper closure: remap a block ID, panicking if a reachable reference
    // points to a dead block (that would be a bug in the severing step above).
    let remap = |old: usize| -> usize {
        new_id[old].unwrap_or_else(|| panic!("reachable block references dead block {old}"))
    };

    // ── Step 4: filter to only reachable blocks ───────────────────────────
    let mut compacted: Vec<_> = builder.blocks
        .drain(..)
        .filter(|b| reachable[b.id])
        .collect();

    // ── Step 5: rewrite all block-ID references ───────────────────────────
    for block in &mut compacted {
        block.id = remap(block.id);
        for s in &mut block.successors  { *s = remap(*s); }
        for p in &mut block.predecessors { *p = remap(*p); }

        for inst in &mut block.instrs {
            if let IrInstruction::PhiAssign(phi) = inst {
                phi.block = remap(phi.block);
                for (_, pred) in &mut phi.operands {
                    *pred = remap(*pred);
                }
            }
            // Jmp / Br targets are just BasicBlockIds stored in the instruction.
            match inst {
                IrInstruction::Jmp(t) => *t = remap(*t),
                IrInstruction::Br(_, t, f) => { *t = remap(*t); *f = remap(*f); }
                _ => {}
            }
        }
    }

    builder.blocks = compacted;
}



