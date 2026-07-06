// Maximal SSA shares the SSA IR data structures verbatim — re-export rather
// than maintain a byte-identical duplicate. This makes `build_max_ssa` return
// the same `Builder` type the optimizer/regalloc pipeline consumes.
pub use crate::ssa::ir;

use std::collections::{BTreeSet, HashMap, VecDeque};

use crate::brilir::builder::Builder as BrilBuilder;
use crate::brilir::instruction::{
    BinaryOp as BrilBinaryOp, IrInstruction as BrilIrInstruction, Value,
};

use ir::{
    BinaryOp as SsaBinaryOp, Builder, InstId, IrInstruction, SsaValue, SsaVariable, VariableId,
};

/// Convert a brilir BinaryOp to a max-SSA BinaryOp (1:1 mapping).
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

/// Convert a brilir immediate to an SsaValue (Int or Bool).
fn immediate_to_ssa_value(imm: Value) -> SsaValue {
    match imm {
        Value::Int(i) => SsaValue::Int(i),
        Value::Bool(b) => SsaValue::Bool(b),
    }
}

/// Mint the next version of original variable `v` as `%v{v}_{index}`.
/// Versions are kept in the original variable's namespace (id == original
/// variable, index bumped per def) so the maximal-SSA output maps cleanly
/// back to source variables — useful for incremental lifting.
fn mint(next_index: &mut HashMap<VariableId, usize>, v: VariableId) -> SsaVariable {
    let idx = next_index.entry(v).or_insert(0);
    let var = SsaVariable { id: v, index: *idx };
    *idx += 1;
    var
}

/// Variables written by a brilir instruction (0 or 1).
fn bril_defs(inst: &BrilIrInstruction) -> Option<VariableId> {
    match inst {
        BrilIrInstruction::Load(d, _)
        | BrilIrInstruction::Mov(d, _)
        | BrilIrInstruction::Binary(_, d, _, _)
        | BrilIrInstruction::Not(d, _) => Some(d.0),
        BrilIrInstruction::Call { dest: Some(d), .. } => Some(d.0),
        _ => None,
    }
}

/// Variables read by a brilir instruction, pushed into `out`.
fn bril_uses(inst: &BrilIrInstruction, out: &mut Vec<VariableId>) {
    match inst {
        BrilIrInstruction::Mov(_, s)
        | BrilIrInstruction::Not(_, s)
        | BrilIrInstruction::Print(s)
        | BrilIrInstruction::Br(s, _, _)
        | BrilIrInstruction::Ret(s) => out.push(s.0),
        BrilIrInstruction::Binary(_, _, l, r) => {
            out.push(l.0);
            out.push(r.0);
        }
        BrilIrInstruction::Call { args, .. } => out.extend(args.iter().map(|a| a.0)),
        BrilIrInstruction::Load(_, _)
        | BrilIrInstruction::Jmp(_)
        | BrilIrInstruction::Nop => {}
    }
}

/// Lift a brilir::Builder into **maximal SSA** form.
///
/// Maximal SSA places a φ for *every* variable in the universe at the head of
/// *every* block (entry included) — as many φs as can possibly exist. Every use
/// resolves to either a local def or the block's own φ dest, which always
/// exists, so there is **no `Undef`/missing-value case**. No liveness, no
/// dominance frontiers, no sealing, no fixpoint during construction.
///
/// Passes:
///   1. Place one φ per (block, variable). The φ dest is that block's *entry*
///      version of the variable, seeding the block's version map. Params
///      override entry's map with real incoming defs.
///   2. BFS from entry. Rename each block once (uses read the running version,
///      defs mint a fresh one), then walk every successor edge — both arms of a
///      conditional — pushing this block's exit version as the successor's φ
///      operand for that variable.
///   3. Prune to fixpoint with our own rules (NOT Braun's): a φ with 0 operands
///      is dropped (e.g. entry φs); a φ with 1 distinct operand is replaced by
///      that value at every use. Cascades until a sweep makes no change.
pub fn build_max_ssa(bril: &BrilBuilder) -> Builder {
    let mut builder = Builder::new();
    builder.next_var_id = bril.next_var_id;
    builder.name = bril.name.clone();

    // ── Pass 0a: clone the CFG skeleton (sorted preds/succs for determinism).
    // Assumes brilir block.id == its index, matching the rest of the pipeline.
    for bril_block in bril.blocks.iter() {
        let mut preds: Vec<usize> = bril_block.predecessors.iter().copied().collect();
        let mut succs: Vec<usize> = bril_block.successors.iter().copied().collect();
        preds.sort_unstable();
        succs.sort_unstable();
        builder.add_block(bril_block.id, preds, succs);
    }

    // ── Pass 0b: variable universe = params ∪ every def/use across all blocks.
    let mut universe: BTreeSet<VariableId> = BTreeSet::new();
    for p in &bril.params {
        universe.insert(p.0);
    }
    let mut use_buf: Vec<VariableId> = Vec::new();
    for block in &bril.blocks {
        for inst in &block.instrs {
            if let Some(d) = bril_defs(inst) {
                universe.insert(d);
            }
            use_buf.clear();
            bril_uses(inst, &mut use_buf);
            universe.extend(use_buf.iter().copied());
        }
    }
    let universe: Vec<VariableId> = universe.into_iter().collect();

    let n = builder.blocks.len();
    let mut next_index: HashMap<VariableId, usize> = HashMap::new();
    // Each block's running version map. Seeded with the block's own φ dest for
    // every universe variable, so every read resolves to a Var — there is no
    // Undef case in maximal SSA. After renaming, this holds the block's exit
    // versions, read by successors to fill their φ operands.
    let mut version: Vec<HashMap<VariableId, SsaValue>> = vec![HashMap::new(); n];
    let mut phi_slot: Vec<HashMap<VariableId, InstId>> = vec![HashMap::new(); n];

    // ── Pass 1: place φs in EVERY block (entry included) — one per universe
    // variable. Maximal: as many φs as can exist. The φ dest is the block's
    // entry version of that variable. Entry-block φs simply end up with no
    // operands (no predecessors) and are pruned later.
    for b in 0..n {
        for &v in &universe {
            let dest = mint(&mut next_index, v);
            let slot = builder.inst_phi(dest, b);
            version[b].insert(v, SsaValue::Var(dest));
            phi_slot[b].insert(v, slot);
        }
    }
    // Function params are real incoming values, not φ results. Override entry's
    // version map for each param with a fresh real def so body reads resolve to
    // it; the (unused) entry param-φ created above ends up with 0 operands and
    // is pruned. This keeps φ-placement uniform without inventing an Undef.
    for p in &bril.params {
        let var = mint(&mut next_index, p.0);
        builder.params.push(var);
        version[0].insert(p.0, SsaValue::Var(var));
    }

    // ── Pass 2: BFS from entry. Rename each block's body once (uses read the
    // running version, defs mint a fresh version), then walk EVERY successor
    // edge — both arms of a conditional — pushing this block's exit version of
    // each variable as the operand of the successor's φ for that variable.
    let mut renamed = vec![false; n];
    let mut queue: VecDeque<usize> = VecDeque::new();
    queue.push_back(0);

    while let Some(b) = queue.pop_front() {
        if renamed[b] {
            continue;
        }
        renamed[b] = true;

        let mut cur = version[b].clone();
        // Every variable has an entry version (its φ dest), so reads never miss.
        let read = |cur: &HashMap<VariableId, SsaValue>, v: VariableId| cur[&v];

        for inst in bril.blocks[b].instrs.iter() {
            let out = match inst {
                BrilIrInstruction::Load(dst, imm) => {
                    let d = mint(&mut next_index, dst.0);
                    cur.insert(dst.0, SsaValue::Var(d));
                    IrInstruction::Const(SsaValue::Var(d), immediate_to_ssa_value(*imm))
                }
                BrilIrInstruction::Mov(dst, src) => {
                    let s = read(&cur, src.0);
                    let d = mint(&mut next_index, dst.0);
                    cur.insert(dst.0, SsaValue::Var(d));
                    IrInstruction::Mov(SsaValue::Var(d), s)
                }
                BrilIrInstruction::Binary(op, dst, lhs, rhs) => {
                    let l = read(&cur, lhs.0);
                    let r = read(&cur, rhs.0);
                    let d = mint(&mut next_index, dst.0);
                    cur.insert(dst.0, SsaValue::Var(d));
                    IrInstruction::Binary(convert_binary_op(*op), SsaValue::Var(d), l, r)
                }
                BrilIrInstruction::Not(dst, src) => {
                    let s = read(&cur, src.0);
                    let d = mint(&mut next_index, dst.0);
                    cur.insert(dst.0, SsaValue::Var(d));
                    IrInstruction::Not(SsaValue::Var(d), s)
                }
                BrilIrInstruction::Print(src) => IrInstruction::Print(read(&cur, src.0)),
                BrilIrInstruction::Jmp(target) => IrInstruction::Jmp(*target),
                BrilIrInstruction::Br(cond, truthy, falsy) => {
                    if truthy == falsy {
                        // `br cond X X` → unconditional jump; drop the cond read.
                        IrInstruction::Jmp(*truthy)
                    } else {
                        IrInstruction::Br(read(&cur, cond.0), *truthy, *falsy)
                    }
                }
                BrilIrInstruction::Ret(src) => IrInstruction::Ret(read(&cur, src.0)),
                BrilIrInstruction::Call { callee_bb, args, dest } => {
                    let arg_vals: Vec<SsaValue> =
                        args.iter().map(|a| read(&cur, a.0)).collect();
                    let ssa_dest = dest.map(|d| {
                        let v = mint(&mut next_index, d.0);
                        cur.insert(d.0, SsaValue::Var(v));
                        SsaValue::Var(v)
                    });
                    IrInstruction::Call { callee_bb: *callee_bb, args: arg_vals, dest: ssa_dest }
                }
                BrilIrInstruction::Nop => IrInstruction::Nop,
            };
            builder.get_block_mut(b).instrs.push(out);
        }
        version[b] = cur;

        let succs = builder.get_block(b).successors.clone();
        for &s in &succs {
            for &v in &universe {
                let op = version[b][&v];
                let slot = phi_slot[s][&v];
                builder.get_block_mut(s).instrs[slot]
                    .phi_mut()
                    .operands
                    .push((op, b));
            }
            if !renamed[s] {
                queue.push_back(s);
            }
        }
    }

    // ── Pass 3: prune to fixpoint with our own rules (NOT Braun's):
    //   • φ with 0 operands           → drop it (dead def, e.g. entry φs).
    //   • φ with 1 distinct operand v → replace every use of the φ dest with v,
    //                                    then drop the φ.
    // Replacements cascade (a removed φ can make another φ trivial), so we loop
    // until a full sweep makes no change.
    prune_phis(&mut builder);

    builder
}

/// Prune maximal-SSA φs to fixpoint. See `build_max_ssa` pass 3 for the rules.
fn prune_phis(builder: &mut Builder) {
    loop {
        // Find one prunable φ: (block, inst, dest, replacement-or-None).
        let mut found: Option<(usize, usize, SsaVariable, Option<SsaValue>)> = None;
        'scan: for b in 0..builder.blocks.len() {
            for i in 0..builder.blocks[b].instrs.len() {
                let inst = &builder.blocks[b].instrs[i];
                if !inst.is_phi() {
                    continue;
                }
                let phi = inst.phi();
                let dest = phi.var.expect_var();
                // Distinct operand values, excluding self-references (φ = …, φ, …).
                let mut distinct: Option<SsaValue> = None;
                let mut multiple = false;
                for (op, _) in phi.operands.iter() {
                    if *op == SsaValue::Var(dest) {
                        continue; // self-reference: ignore
                    }
                    match distinct {
                        None => distinct = Some(*op),
                        Some(d) if d == *op => {}
                        Some(_) => { multiple = true; break; }
                    }
                }
                if multiple {
                    continue;
                }
                // 0 distinct operands → drop; 1 distinct → replace by it.
                found = Some((b, i, dest, distinct));
                break 'scan;
            }
        }

        let Some((b, i, dest, replacement)) = found else { break };

        if let Some(val) = replacement {
            replace_all_uses(builder, dest, val);
        }
        builder.blocks[b].instrs[i] = IrInstruction::Nop;
    }

    // Drop the Nop placeholders left by pruning.
    for block in builder.blocks.iter_mut() {
        block.instrs.retain(|i| !matches!(i, IrInstruction::Nop));
    }
}

/// Replace every use of `old` (def or φ operand) with `val` across all blocks.
fn replace_all_uses(builder: &mut Builder, old: SsaVariable, val: SsaValue) {
    let old_val = SsaValue::Var(old);
    for block in builder.blocks.iter_mut() {
        for inst in block.instrs.iter_mut() {
            if let IrInstruction::PhiAssign(phi) = inst {
                for (op, _) in phi.operands.iter_mut() {
                    if *op == old_val {
                        *op = val;
                    }
                }
            } else {
                inst.for_each_use_mut(|u| {
                    if *u == old_val {
                        *u = val;
                    }
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brilir::builder::Builder as BrilBuilder;
    use crate::brilir::instruction::{IrInstruction as B, Value, Variable};

    fn total_phis(b: &Builder) -> usize {
        b.blocks.iter().flat_map(|bb| bb.instrs.iter()).filter(|i| i.is_phi()).count()
    }

    fn block_phis<'a>(b: &'a Builder, block: usize) -> Vec<&'a ir::Phi> {
        b.get_block(block).instrs.iter().filter(|i| i.is_phi()).map(|i| i.phi()).collect()
    }

    /// No surviving instruction may reference a value that was pruned away
    /// (every φ dest that still exists, plus every non-φ def, is a valid target).
    fn assert_no_dangling(b: &Builder) {
        use std::collections::HashSet;
        let mut defined: HashSet<SsaVariable> = HashSet::new();
        for block in &b.blocks {
            for inst in &block.instrs {
                for d in inst.defs() {
                    defined.insert(d.expect_var());
                }
            }
        }
        for v in &b.params {
            defined.insert(*v);
        }
        for block in &b.blocks {
            for inst in &block.instrs {
                for u in inst.uses() {
                    if let SsaValue::Var(v) = u {
                        assert!(defined.contains(v), "dangling use of {:?} in {:?}", v, inst);
                    }
                }
            }
        }
    }

    // Loop: bb0 -> bb1(header) -> bb2(body, back-edge to bb1); bb1 -> bb3(exit).
    // v0 is the loop counter (genuine header φ); v1 is loop-invariant (=1) so its
    // φs collapse. After pruning, exactly one φ survives: v0 in the header.
    #[test]
    fn loop_prunes_to_single_counter_phi() {
        let mut bril = BrilBuilder::new();
        bril.name = "loop".into();
        bril.next_var_id = 2;

        bril.add_block(0); // entry
        bril.add_instr(B::Load(Variable(0), Value::Int(0)));
        bril.add_instr(B::Load(Variable(1), Value::Int(1)));
        bril.add_instr(B::Jmp(1));

        bril.add_block(1); // header
        bril.add_instr(B::Br(Variable(0), 2, 3));

        bril.add_block(2); // body: v0 = v0 + v1; back to header
        bril.add_instr(B::Binary(
            crate::brilir::instruction::BinaryOp::Add,
            Variable(0),
            Variable(0),
            Variable(1),
        ));
        bril.add_instr(B::Jmp(1));

        bril.add_block(3); // exit
        bril.add_instr(B::Ret(Variable(0)));

        bril.add_edge(0, 1);
        bril.add_edge(1, 2);
        bril.add_edge(2, 1);
        bril.add_edge(1, 3);

        let ssa = build_max_ssa(&bril);
        assert_no_dangling(&ssa);

        // Exactly one φ survives, and it lives in the header (bb1) for v0.
        assert_eq!(total_phis(&ssa), 1, "only the loop-counter φ should remain");
        let header = block_phis(&ssa, 1);
        assert_eq!(header.len(), 1);
        let phi = header[0];
        assert_eq!(phi.var.expect_var().id, 0, "surviving φ is for v0");
        // Two operands: the entry init (from bb0) and the back-edge update (bb2).
        let preds: Vec<_> = phi.operands.iter().map(|(_, p)| *p).collect();
        assert_eq!(preds, vec![0, 2]);
        assert!(phi.operands.iter().all(|(v, _)| v.is_var()));
        // The two operands are genuinely distinct (init ≠ updated counter).
        assert_ne!(phi.operands[0].0, phi.operands[1].0);
    }

    // Diamond: v1 defined in BOTH arms with different values, used at the join.
    // The join φ for v1 has two distinct operands → kept. Everything else (entry
    // φs, dead arm φs, single-operand φs) is pruned. Result: one φ in bb3.
    #[test]
    fn diamond_prunes_to_single_join_phi() {
        let mut bril = BrilBuilder::new();
        bril.name = "diamond".into();
        bril.next_var_id = 2;

        bril.add_block(0);
        bril.add_instr(B::Load(Variable(0), Value::Bool(true)));
        bril.add_instr(B::Br(Variable(0), 1, 2));

        bril.add_block(1); // then: v1 = 7
        bril.add_instr(B::Load(Variable(1), Value::Int(7)));
        bril.add_instr(B::Jmp(3));

        bril.add_block(2); // else: v1 = 9
        bril.add_instr(B::Load(Variable(1), Value::Int(9)));
        bril.add_instr(B::Jmp(3));

        bril.add_block(3); // join: use v1
        bril.add_instr(B::Print(Variable(1)));
        bril.add_instr(B::Ret(Variable(1)));

        bril.add_edge(0, 1);
        bril.add_edge(0, 2);
        bril.add_edge(1, 3);
        bril.add_edge(2, 3);

        let ssa = build_max_ssa(&bril);
        assert_no_dangling(&ssa);

        assert_eq!(total_phis(&ssa), 1, "only the join φ for v1 should remain");
        let join = block_phis(&ssa, 3);
        assert_eq!(join.len(), 1);
        let phi = join[0];
        assert_eq!(phi.var.expect_var().id, 1, "surviving φ is for v1");
        let preds: Vec<_> = phi.operands.iter().map(|(_, p)| *p).collect();
        assert_eq!(preds, vec![1, 2], "join φ reads both arms");
        assert!(phi.operands.iter().all(|(v, _)| v.is_var()));
        assert_ne!(phi.operands[0].0, phi.operands[1].0, "arms carry distinct values");
    }

    // Straight-line code with no merges prunes to zero φs.
    #[test]
    fn straight_line_has_no_phis() {
        let mut bril = BrilBuilder::new();
        bril.name = "line".into();
        bril.next_var_id = 1;

        bril.add_block(0);
        bril.add_instr(B::Load(Variable(0), Value::Int(5)));
        bril.add_instr(B::Jmp(1));
        bril.add_block(1);
        bril.add_instr(B::Ret(Variable(0)));
        bril.add_edge(0, 1);

        let ssa = build_max_ssa(&bril);
        assert_no_dangling(&ssa);
        assert_eq!(total_phis(&ssa), 0, "no merges → no φs after pruning");
    }
}
