//! Output building: turn the `vreg_alloc` map into the final
//! `AllocationResult` (inst_allocs + edits_before/after) and emit the
//! phi-resolution parallel-copy moves at predecessor-block tails.
//!
//! Conceptually three sub-phases:
//!   - Per-instruction lowering: for each operand, fill in `inst_allocs`
//!     with the var's home preg. Spilled vars borrow a scratch register
//!     and emit a Stack→Reg edit_before (Use) or Reg→Stack edit_after
//!     (Def). Fixed-constraint operands whose home preg differs from the
//!     constraint preg emit fix-up moves.
//!   - Stack→Stack mov expansion is *not* needed here because we always
//!     route spills through a per-instruction scratch.
//!   - Phi resolution: for each successor with phis, collect
//!     (operand_alloc, dst_alloc) pairs per predecessor and sequentialize
//!     via `ssa::parallel_move::sequentialize`. Emit the resulting moves
//!     at the predecessor's tail (as an `AllocMove` list attached to the
//!     pred's terminator's `edits_before`).

use std::collections::{HashMap, HashSet};

use crate::ssa::parallel_move;

use super::{
    AllocFunction, AllocMove, Allocation, AllocationResult, Constraint, OperandKind, PReg,
    RegClass, SpillSlot, Var,
};

pub struct OutputInput<'a, F: AllocFunction> {
    pub func: &'a F,
    pub vreg_alloc: &'a HashMap<Var, Allocation>,
    pub num_spillslots: usize,
}

pub fn build<F: AllocFunction>(input: OutputInput<F>) -> AllocationResult {
    let num_insts = input.func.num_instructions();
    let mut inst_allocs: Vec<Vec<Allocation>> = vec![Vec::new(); num_insts];
    let mut edits_before: HashMap<usize, Vec<AllocMove>> = HashMap::new();
    let mut edits_after: HashMap<usize, Vec<AllocMove>> = HashMap::new();

    // Partition scratch regs by class.
    let mut scratch_by_class: HashMap<RegClass, Vec<PReg>> = HashMap::new();
    for &p in input.func.scratch_regs() {
        scratch_by_class.entry(p.class).or_default().push(p);
    }



    for inst in 0..num_insts {
        // Skip phis — their "operand" allocations live in pred terminators,
        // not at the phi instruction itself.
        if input.func.is_phi(inst) {
            // But the phi's def alloc still has to populate inst_allocs[0]
            // so downstream code that reads them doesn't see an empty vec.
            inst_allocs[inst] = input
                .func
                .inst_operands(inst)
                .iter()
                .map(|op| home_alloc(input.vreg_alloc, op.var))
                .collect();
            continue;
        }

        let ops = input.func.inst_operands(inst);
        let mut allocs: Vec<Allocation> =
            ops.iter().map(|op| home_alloc(input.vreg_alloc, op.var)).collect();
        let mut scratch_used: HashMap<RegClass, usize> = HashMap::new();

        // (1) Fixed-constraint fixups: if a use is supposed to be in preg
        //     P but actually sits elsewhere, shuttle. Same for defs (other
        //     direction).
        //
        //     Before emitting the Use→P fixups, any other use currently in
        //     P must be shuttled out to a scratch — otherwise the Use→P
        //     mov would clobber it.
        let target_regs: HashSet<PReg> = ops
            .iter()
            .enumerate()
            .filter_map(|(i, op)| match (op.kind, op.constraint) {
                (OperandKind::Use, Constraint::Fixed(p)) if allocs[i] != Allocation::Reg(p) => {
                    Some(p)
                }
                _ => None,
            })
            .collect();

        for (i, op) in ops.iter().enumerate() {
            if op.kind == OperandKind::Use {
                if let Allocation::Reg(curr) = allocs[i] {
                    if target_regs.contains(&curr) {
                        if let Some(scratch) =
                            take_scratch(&mut scratch_used, &scratch_by_class, op.var.class)
                        {
                            edits_before.entry(inst).or_default().push(AllocMove {
                                from: allocs[i],
                                to: Allocation::Reg(scratch),
                            });
                            allocs[i] = Allocation::Reg(scratch);
                        }
                    }
                }
            }
        }

        for (i, op) in ops.iter().enumerate() {
            if let Constraint::Fixed(p) = op.constraint {
                let curr = allocs[i];
                if curr != Allocation::Reg(p) {
                    allocs[i] = Allocation::Reg(p);
                    match op.kind {
                        OperandKind::Use => {
                            edits_before.entry(inst).or_default().push(AllocMove {
                                from: curr,
                                to: Allocation::Reg(p),
                            });
                        }
                        OperandKind::Def => {
                            edits_after.entry(inst).or_default().push(AllocMove {
                                from: Allocation::Reg(p),
                                to: curr,
                            });
                        }
                    }
                }
            }
        }

        // (2) Spill/fill: every Stack operand goes through a scratch.
        //     Use-deduped by slot, just like xregalloc's output-builder.
        let mut spill_scratch: HashMap<SpillSlot, PReg> = HashMap::new();
        for (i, op) in ops.iter().enumerate() {
            let Allocation::Stack(slot) = allocs[i] else { continue };
            if op.kind == OperandKind::Def && (input.func.is_copy(inst) || input.func.is_const(inst)) {
                continue;
            }
            let scratch = if let Some(&existing) = spill_scratch.get(&slot) {
                existing
            } else {
                let Some(s) = take_scratch(&mut scratch_used, &scratch_by_class, op.var.class)
                else {
                    continue;
                };
                spill_scratch.insert(slot, s);
                s
            };
            allocs[i] = Allocation::Reg(scratch);
            match op.kind {
                OperandKind::Use => {
                    edits_before.entry(inst).or_default().push(AllocMove {
                        from: Allocation::Stack(slot),
                        to: Allocation::Reg(scratch),
                    });
                }
                OperandKind::Def => {
                    edits_after.entry(inst).or_default().push(AllocMove {
                        from: Allocation::Reg(scratch),
                        to: Allocation::Stack(slot),
                    });
                }
            }
        }

        inst_allocs[inst] = allocs;
    }

    // (3) Phi resolution.
    emit_phi_resolution(
        input.func,
        input.vreg_alloc,
        &mut edits_before,
    );

    let mut vreg_alloc = input.vreg_alloc.clone();
    // For any var that has no alloc (defensive), use a class-0 default.
    let _ = &mut vreg_alloc;

    AllocationResult {
        vreg_alloc,
        inst_allocs,
        edits_before,
        edits_after,
        num_spillslots: input.num_spillslots,
    }
}

fn home_alloc(vreg_alloc: &HashMap<Var, Allocation>, v: Var) -> Allocation {
    vreg_alloc
        .get(&v)
        .copied()
        .unwrap_or(Allocation::Reg(PReg::new(0, v.class)))
}

fn take_scratch(
    used: &mut HashMap<RegClass, usize>,
    pool: &HashMap<RegClass, Vec<PReg>>,
    class: RegClass,
) -> Option<PReg> {
    let idx = used.entry(class).or_insert(0);
    let regs = pool.get(&class)?;
    if *idx >= regs.len() {
        return None;
    }
    let p = regs[*idx];
    *idx += 1;
    Some(p)
}



/// Emit phi-resolution moves in each predecessor's terminator's
/// edits_before slot. The moves are sequentialized via the SSA
/// parallel_move::sequentialize helper so swaps and cycles resolve
/// correctly.
///
/// Critical-edge splitting (done upstream by the caller) guarantees
/// that any block with phis has only single-successor predecessors,
/// so emitting at the pred's tail is unambiguous.
fn emit_phi_resolution<F: AllocFunction>(
    func: &F,
    vreg_alloc: &HashMap<Var, Allocation>,
    edits_before: &mut HashMap<usize, Vec<AllocMove>>,
) {
    let n_blocks = func.num_blocks();

    // Gather scratches for cycle-break temps.
    let mut scratch_by_class: HashMap<RegClass, Vec<PReg>> = HashMap::new();
    for &p in func.scratch_regs() {
        scratch_by_class.entry(p.class).or_default().push(p);
    }

    for b in 0..n_blocks {
        // Collect per-successor copies, then sequentialize each.
        for &succ in func.block_successors(b) {
            let mut copies: Vec<(Allocation, Allocation)> = Vec::new();
            for inst in func.block_instructions(succ) {
                if !func.is_phi(inst) {
                    continue;
                }
                let Some(dst_op) = func.inst_operands(inst).first() else { continue };
                if dst_op.kind != OperandKind::Def {
                    continue;
                }
                let dst_alloc = home_alloc(vreg_alloc, dst_op.var);
                let src_var = func.phi_op(inst, b);
                let src_alloc = home_alloc(vreg_alloc, src_var);
                copies.push((src_alloc, dst_alloc));
            }
            if copies.is_empty() {
                continue;
            }

            // Use the first scratch of each class on demand. Stack→Stack
            // pairs get expanded post-sequentialize.
            let int_scratches: Vec<Allocation> = scratch_by_class
                .get(&RegClass::Int)
                .map(|v| v.iter().map(|p| Allocation::Reg(*p)).collect())
                .unwrap_or_default();
            let mut cycle_idx = 0usize;
            let get_temp = || -> Allocation {
                // For now we only handle Int-class cycle temps. A more
                // general impl would pick by class of the copy involved.
                let t = int_scratches
                    .get(cycle_idx)
                    .copied()
                    .expect("phi resolution exhausted scratch pool");
                cycle_idx += 1;
                t
            };

            let seq = parallel_move::sequentialize(&copies, get_temp);

            // Expand Stack→Stack pairs via the first available class-Int
            // scratch (reused across all such pairs because the sequence
            // is already sequential — no parallel-copy semantics post-
            // sequentialize).
            let stack_shuttle = int_scratches.first().copied();
            let term_inst = func.block_instructions(b).end.saturating_sub(1);
            let emit_at = edits_before.entry(term_inst).or_default();
            for (src, dst) in seq {
                if src.is_stack() && dst.is_stack() {
                    if let Some(shuttle) = stack_shuttle {
                        emit_at.push(AllocMove { from: src, to: shuttle });
                        emit_at.push(AllocMove { from: shuttle, to: dst });
                        continue;
                    }
                }
                emit_at.push(AllocMove { from: src, to: dst });
            }
        }
    }
}

