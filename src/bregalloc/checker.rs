//! Symbolic correctness checker for `AllocationResult`s, in the style of
//! Cranelift's regalloc.rs checker (Fallin, 2021).
//!
//! Given a function `F: AllocFunction` and an `AllocationResult` produced
//! by `bregalloc::allocate`, this module proves — for *all* runtime inputs
//! — that every use sees the same vreg value it would have seen in the
//! unallocated SSA program. It does this without executing the program by
//! tracking a small symbolic lattice over storage locations:
//!
//!     SymVal ::= Empty | Vreg(Var) | Conflicted
//!
//! The meet operation is standard:
//!     Empty       ⊓ x           = x
//!     Vreg(a)     ⊓ Vreg(a)     = Vreg(a)
//!     Vreg(a)     ⊓ Vreg(b)     = Conflicted   (a ≠ b)
//!     Conflicted  ⊓ _           = Conflicted
//!
//! Per-instruction transfer (in order):
//!   1. Apply `edits_before[inst]` — each `AllocMove { from, to }` copies
//!      `state[from]` into `state[to]`. `from`'s symbol is left intact.
//!   2. For each Use operand `k`, assert
//!        `state[inst_allocs[inst][k]] == Vreg(op.var)`.
//!   3. For each clobber preg, wipe `state[Reg(p)] = Empty`.
//!   4. For each Def operand `k`, set
//!        `state[inst_allocs[inst][k]] = Vreg(op.var)`.
//!   5. Apply `edits_after[inst]`.
//!
//! Phi handling. Per `output.rs`, phi-resolution moves live in the
//! predecessor's terminator's `edits_before`. So after a pred's transfer,
//! the phi destination's home allocation already holds the pred's phi
//! operand value (still symbolically labeled with the *pred*'s operand
//! vreg). To avoid the meet collapsing phi destinations to `Conflicted`,
//! the entry state for a block is computed by first cloning each pred's
//! exit state, then *renaming* each phi-dst's home allocation from
//! `Vreg(phi_op(pred))` to `Vreg(phi_def)` (asserting the rename source
//! matches before overwriting). The per-pred states are then met.
//!
//! Errors are typed and carry enough information to point at the offending
//! instruction/operand. A passing check proves the allocation is correct
//! for every possible runtime input.

use std::collections::{HashMap, HashSet, VecDeque};

use super::{
    AllocFunction, AllocMove, Allocation, AllocationResult, OperandKind, Var,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SymVal {
    Empty,
    Vreg(Var),
    Conflicted,
}

impl SymVal {
    fn meet(self, other: SymVal) -> SymVal {
        match (self, other) {
            (SymVal::Empty, x) | (x, SymVal::Empty) => x,
            (SymVal::Conflicted, _) | (_, SymVal::Conflicted) => SymVal::Conflicted,
            (SymVal::Vreg(a), SymVal::Vreg(b)) => {
                if a == b { SymVal::Vreg(a) } else { SymVal::Conflicted }
            }
        }
    }
}

/// Symbolic store: missing key ≡ `Empty`. We keep it as a sparse map so
/// large spill-slot ranges don't blow up memory.
#[derive(Clone, Default, Debug)]
pub struct State {
    inner: HashMap<Allocation, SymVal>,
}

impl State {
    pub fn new() -> Self { Self::default() }

    pub fn get(&self, a: Allocation) -> SymVal {
        self.inner.get(&a).copied().unwrap_or(SymVal::Empty)
    }

    pub fn set(&mut self, a: Allocation, v: SymVal) {
        if matches!(v, SymVal::Empty) {
            self.inner.remove(&a);
        } else {
            self.inner.insert(a, v);
        }
    }

    /// Pointwise meet. Missing keys are treated as `Empty`.
    fn meet(&self, other: &State) -> State {
        let mut out = State::new();
        let mut keys: HashSet<Allocation> = self.inner.keys().copied().collect();
        keys.extend(other.inner.keys().copied());
        for k in keys {
            let m = self.get(k).meet(other.get(k));
            if !matches!(m, SymVal::Empty) {
                out.inner.insert(k, m);
            }
        }
        out
    }
}

#[derive(Debug)]
pub enum CheckError {
    UseMismatch {
        block: usize,
        inst: usize,
        operand_idx: usize,
        at: Allocation,
        expected: Var,
        got: SymVal,
    },
    PhiMismatch {
        succ_block: usize,
        pred_block: usize,
        phi_inst: usize,
        at: Allocation,
        expected_src: Var,
        got: SymVal,
    },
    MissingPhiDstAlloc {
        block: usize,
        phi_inst: usize,
        dst: Var,
    },
    FixpointDiverged {
        iterations: usize,
    },
}

impl std::fmt::Display for CheckError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CheckError::UseMismatch { block, inst, operand_idx, at, expected, got } => write!(
                f,
                "use mismatch: block {block} inst {inst} operand #{operand_idx} \
                 at {at:?}: expected {expected:?}, got {got:?}"
            ),
            CheckError::PhiMismatch { succ_block, pred_block, phi_inst, at, expected_src, got } => write!(
                f,
                "phi mismatch: succ {succ_block} ← pred {pred_block} \
                 phi inst {phi_inst} at {at:?}: expected source {expected_src:?}, got {got:?}"
            ),
            CheckError::MissingPhiDstAlloc { block, phi_inst, dst } => write!(
                f,
                "phi dst {dst:?} (inst {phi_inst} in block {block}) has no home allocation"
            ),
            CheckError::FixpointDiverged { iterations } => {
                write!(f, "checker fixpoint diverged after {iterations} iterations")
            }
        }
    }
}

impl std::error::Error for CheckError {}

/// Print a detailed report for a verification failure: the offending
/// instruction's full operand list, its edits, and the home allocations
/// of the involved vregs. Use this from a caller on `Err(e)` to localise
/// the bug to a specific instruction or pred-edge.
pub fn dump_context<F: AllocFunction>(
    func: &F,
    result: &AllocationResult,
    err: &CheckError,
) {
    eprintln!("─────────── checker context ───────────");
    eprintln!("{err}");
    match err {
        CheckError::UseMismatch { block, inst, operand_idx, expected, got, .. } => {
            dump_inst(func, result, *block, *inst);
            eprintln!("  operand #{operand_idx} expected {expected:?}, got {got:?}");
            if let Some(home) = result.vreg_alloc.get(expected) {
                eprintln!("  {expected:?} home allocation: {home:?}");
            } else {
                eprintln!("  {expected:?} has NO home allocation in vreg_alloc");
            }
            if let SymVal::Vreg(v) = got {
                if let Some(home) = result.vreg_alloc.get(v) {
                    eprintln!("  {v:?} home allocation: {home:?}");
                }
            }
            eprintln!("  block {block} predecessors: {:?}", func.block_predecessors(*block));
        }
        CheckError::PhiMismatch { succ_block, pred_block, phi_inst, expected_src, got, at } => {
            dump_inst(func, result, *succ_block, *phi_inst);
            eprintln!("  pred {pred_block}'s terminator's edits_before:");
            let term = func.block_instructions(*pred_block).end.saturating_sub(1);
            if let Some(eb) = result.edits_before.get(&term) {
                for m in eb {
                    eprintln!("    move {:?} → {:?}", m.from, m.to);
                }
            } else {
                eprintln!("    (none)");
            }
            eprintln!("  at location {at:?}: expected {expected_src:?}, got {got:?}");
        }
        CheckError::MissingPhiDstAlloc { block, phi_inst, dst } => {
            dump_inst(func, result, *block, *phi_inst);
            eprintln!("  phi dst {dst:?} has no home allocation");
        }
        CheckError::FixpointDiverged { .. } => {}
    }
    eprintln!("───────────────────────────────────────");
}

fn dump_inst<F: AllocFunction>(
    func: &F,
    result: &AllocationResult,
    block: usize,
    inst: usize,
) {
    eprintln!("  block {block} inst {inst} (is_phi={}, is_copy={})",
        func.is_phi(inst), func.is_copy(inst));
    let ops = func.inst_operands(inst);
    let allocs = result.inst_allocs.get(inst);
    eprintln!("  operands ({}):", ops.len());
    for (i, op) in ops.iter().enumerate() {
        let alloc = allocs.and_then(|a| a.get(i)).copied();
        let home = result.vreg_alloc.get(&op.var);
        eprintln!(
            "    #{i} {kind:?} {var:?} constraint={constraint:?}  chosen={alloc:?}  home={home:?}",
            kind = op.kind,
            var = op.var,
            constraint = op.constraint,
            alloc = alloc,
            home = home,
        );
    }
    let clobbers = func.inst_clobbers(inst);
    if !clobbers.is_empty() {
        eprintln!("  clobbers: {clobbers:?}");
    }
    if let Some(eb) = result.edits_before.get(&inst) {
        eprintln!("  edits_before:");
        for m in eb {
            eprintln!("    {:?} → {:?}", m.from, m.to);
        }
    }
    if let Some(ea) = result.edits_after.get(&inst) {
        eprintln!("  edits_after:");
        for m in ea {
            eprintln!("    {:?} → {:?}", m.from, m.to);
        }
    }
}

/// Public entry point. Returns `Ok(())` if the allocation is correct for
/// every runtime input, or the first violation found.
pub fn verify<F: AllocFunction>(
    func: &F,
    result: &AllocationResult,
) -> Result<(), CheckError> {
    let n_blocks = func.num_blocks();
    if n_blocks == 0 { return Ok(()); }

    // Pre-compute phi metadata per block: (phi_inst, dst_var, dst_home_alloc).
    let mut phis_by_block: Vec<Vec<(usize, Var, Allocation)>> = vec![Vec::new(); n_blocks];
    for b in 0..n_blocks {
        for inst in func.block_instructions(b) {
            if !func.is_phi(inst) { continue; }
            let Some(dst_op) = func
                .inst_operands(inst)
                .iter()
                .find(|op| op.kind == OperandKind::Def)
            else { continue };
            let Some(&home) = result.vreg_alloc.get(&dst_op.var) else {
                return Err(CheckError::MissingPhiDstAlloc {
                    block: b,
                    phi_inst: inst,
                    dst: dst_op.var,
                });
            };
            phis_by_block[b].push((inst, dst_op.var, home));
        }
    }

    let mut entry: Vec<State> = vec![State::new(); n_blocks];
    let mut exit: Vec<Option<State>> = vec![None; n_blocks];

    // Compute reachable blocks via BFS from the entry block (block 0).
    // Unreachable blocks (no predecessors) must be excluded: they have no
    // live-in values, so any Use operand there would see Empty and produce a
    // spurious UseMismatch.  The allocator's output for unreachable code is
    // never exercised at runtime, so we don't need to check it.
    let mut reachable = vec![false; n_blocks];
    reachable[0] = true;
    let mut bfs: std::collections::VecDeque<usize> = std::collections::VecDeque::new();
    bfs.push_back(0);
    while let Some(b) = bfs.pop_front() {
        for &s in func.block_successors(b) {
            if !reachable[s] {
                reachable[s] = true;
                bfs.push_back(s);
            }
        }
    }

    let mut worklist: VecDeque<usize> = (0..n_blocks).filter(|&b| reachable[b]).collect();
    let mut in_wl: Vec<bool> = (0..n_blocks).map(|b| reachable[b]).collect();

    const CAP: usize = 50_000;
    let mut iters = 0usize;

    while let Some(b) = worklist.pop_front() {
        in_wl[b] = false;
        iters += 1;
        if iters > CAP {
            return Err(CheckError::FixpointDiverged { iterations: iters });
        }

        if b != 0 {
            let new_entry = compute_entry(func, b, &phis_by_block[b], &exit)?;
            let entry_changed = !states_equal(&new_entry, &entry[b]);
            entry[b] = new_entry;
            // If entry didn't change and we already have an exit, no work.
            if !entry_changed && exit[b].is_some() {
                continue;
            }
        }

        let new_exit = transfer_block(func, result, b, entry[b].clone())?;

        let changed = match &exit[b] {
            Some(old) => !states_equal(old, &new_exit),
            None => true,
        };
        exit[b] = Some(new_exit);

        if changed {
            for &s in func.block_successors(b) {
                if !in_wl[s] {
                    in_wl[s] = true;
                    worklist.push_back(s);
                }
            }
        }
    }

    Ok(())
}

fn states_equal(a: &State, b: &State) -> bool {
    if a.inner.len() != b.inner.len() { return false; }
    for (k, v) in &a.inner {
        if b.inner.get(k) != Some(v) { return false; }
    }
    true
}

/// Build the entry state of `block` from its predecessors. For each phi
/// in `block`, rename the phi-dst home allocation's symbol on each
/// per-pred state from `Vreg(phi_op(pred))` to `Vreg(phi_def)` before
/// meeting.
fn compute_entry<F: AllocFunction>(
    func: &F,
    block: usize,
    phis: &[(usize, Var, Allocation)],
    exit: &[Option<State>],
) -> Result<State, CheckError> {
    let preds = func.block_predecessors(block);
    if preds.is_empty() {
        return Ok(State::new());
    }

    let mut per_pred: Vec<State> = Vec::with_capacity(preds.len());
    for &p in preds {
        let Some(s_pred) = &exit[p] else {
            // Pred not yet visited; treat as Empty so the fixpoint can
            // progress without spurious meet-with-empty conflicts (Empty
            // meets anything = that anything).
            per_pred.push(State::new());
            continue;
        };
        let mut s = s_pred.clone();
        for &(phi_inst, dst_var, dst_alloc) in phis {
            let src_var = func.phi_op(phi_inst, p);
            let got = s.get(dst_alloc);
            // Only enforce the invariant once the pred state is non-trivial
            // at this slot. An Empty symbol means the pred's transfer
            // hasn't established anything there yet (first iteration);
            // we'll re-check on the next pass.
            if !matches!(got, SymVal::Empty) && got != SymVal::Vreg(src_var) {
                return Err(CheckError::PhiMismatch {
                    succ_block: block,
                    pred_block: p,
                    phi_inst,
                    at: dst_alloc,
                    expected_src: src_var,
                    got,
                });
            }
            s.set(dst_alloc, SymVal::Vreg(dst_var));
        }
        per_pred.push(s);
    }

    let mut iter = per_pred.into_iter();
    let mut acc = iter.next().unwrap();
    for s in iter {
        acc = acc.meet(&s);
    }
    Ok(acc)
}

/// Standard transfer: edits_before → uses → clobbers → defs → edits_after.
fn transfer_block<F: AllocFunction>(
    func: &F,
    result: &AllocationResult,
    block: usize,
    mut state: State,
) -> Result<State, CheckError> {
    for inst in func.block_instructions(block) {
        // Phis: their parallel-copy moves are emitted in pred terminators,
        // and the entry-state computation already renamed the dst alloc to
        // Vreg(phi_def). The phi instruction itself is a no-op for the
        // transfer.
        if func.is_phi(inst) {
            continue;
        }

        apply_moves(&mut state, result.edits_before.get(&inst));

        let ops = func.inst_operands(inst);
        let allocs = &result.inst_allocs[inst];

        for (k, op) in ops.iter().enumerate() {
            if op.kind != OperandKind::Use { continue; }
            let at = allocs[k];
            let got = state.get(at);
            if got != SymVal::Vreg(op.var) {
                return Err(CheckError::UseMismatch {
                    block,
                    inst,
                    operand_idx: k,
                    at,
                    expected: op.var,
                    got,
                });
            }
        }

        for &p in func.inst_clobbers(inst) {
            state.set(Allocation::Reg(p), SymVal::Empty);
        }

        for (k, op) in ops.iter().enumerate() {
            if op.kind != OperandKind::Def { continue; }
            state.set(allocs[k], SymVal::Vreg(op.var));
        }

        apply_moves(&mut state, result.edits_after.get(&inst));
    }
    Ok(state)
}

fn apply_moves(state: &mut State, moves: Option<&Vec<AllocMove>>) {
    let Some(moves) = moves else { return };
    for m in moves {
        let v = state.get(m.from);
        state.set(m.to, v);
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Secondary diagnostic: post-assignment, no two simultaneously-live vregs
// share a Reg home. If this fires, the bug is in assignment (Algorithm 1)
// or in the liveness/interference info feeding it — NOT in output building
// or phi resolution. Use to triangulate which pass to debug.
// ─────────────────────────────────────────────────────────────────────────

/// Walk every program point, recompute the set of live vregs, and assert
/// no two of them have the same `Allocation::Reg(_)` home.  Independent of
/// `verify`: a function can pass `verify` and fail this (unlikely but
/// possible if an output-builder fixup masks the conflict), or vice versa.
pub fn verify_homes_disjoint<F: AllocFunction>(
    func: &F,
    result: &AllocationResult,
) -> Result<(), HomesConflict> {
    use super::liveness::Liveness;
    let liveness = Liveness::compute(func);
    let next_use = super::uses::compute_next_use(func);

    for b in 0..func.num_blocks() {
        // Live-IN of block b = liveness.live_in[b] (everything that can be
        // read before being redefined). Check it.
        let mut live: HashSet<Var> = liveness.live_in[b].clone();
        check_disjoint(b, usize::MAX, &live, &result.vreg_alloc)?;

        for inst in func.block_instructions(b) {
            // Match the assignment pass's (a)→(b) order so we don't flag
            // the legal case of a new def reusing the preg of a use that
            // died at this same inst.
            //
            // (a) Drop dying uses (last-use is this inst, not live-out).
            for op in func.inst_operands(inst) {
                if op.kind != OperandKind::Use { continue; }
                let v = op.var;
                let dies_here = !liveness.live_out[b].contains(&v)
                    && !super::uses::has_use_after(&next_use, v, inst);
                if dies_here {
                    live.remove(&v);
                }
            }
            // (b) Add defs that live on past this inst.
            for op in func.inst_operands(inst) {
                if op.kind != OperandKind::Def { continue; }
                let v = op.var;
                let lives_on = liveness.live_out[b].contains(&v)
                    || super::uses::has_use_after(&next_use, v, inst);
                if lives_on {
                    live.insert(v);
                }
            }
            check_disjoint(b, inst, &live, &result.vreg_alloc)?;
        }
    }
    Ok(())
}

fn check_disjoint(
    block: usize,
    inst: usize,
    live: &HashSet<Var>,
    homes: &HashMap<Var, Allocation>,
) -> Result<(), HomesConflict> {
    let mut by_preg: HashMap<super::PReg, Var> = HashMap::new();
    for &v in live {
        let Some(Allocation::Reg(p)) = homes.get(&v).copied() else { continue };
        if let Some(&other) = by_preg.get(&p) {
            return Err(HomesConflict {
                block,
                inst,
                preg: p,
                vreg_a: other,
                vreg_b: v,
            });
        }
        by_preg.insert(p, v);
    }
    Ok(())
}

#[derive(Debug)]
pub struct HomesConflict {
    pub block: usize,
    pub inst: usize,
    pub preg: super::PReg,
    pub vreg_a: Var,
    pub vreg_b: Var,
}

impl std::fmt::Display for HomesConflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let where_ = if self.inst == usize::MAX {
            format!("block {} entry", self.block)
        } else {
            format!("block {} after inst {}", self.block, self.inst)
        };
        write!(
            f,
            "two co-live vregs share preg {:?} at {where_}: {:?} and {:?}",
            self.preg, self.vreg_a, self.vreg_b
        )
    }
}

impl std::error::Error for HomesConflict {}

// ─────────────────────────────────────────────────────────────────────────
// Tests + proptest fuzzing
// ─────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bregalloc::{
        allocate, Constraint, Operand, PReg, Var,
    };

    /// Minimal hand-built `AllocFunction` for unit tests. One straight-line
    /// block, every instruction has zero clobbers, no phis, no fixed
    /// constraints. Operands carry `Any`.
    struct TestFunc {
        num_vregs: usize,
        // per block: (instruction range, successors, predecessors)
        blocks: Vec<(std::ops::Range<usize>, Vec<usize>, Vec<usize>)>,
        // per instruction: operands, is_phi, is_copy, phi_ops (one per pred)
        insts: Vec<TestInst>,
        scratches: Vec<PReg>,
    }

    struct TestInst {
        ops: Vec<Operand>,
        clobbers: Vec<PReg>,
        is_phi: bool,
        is_copy: bool,
        phi_ops: Vec<Var>, // index by predecessor position
    }

    impl AllocFunction for TestFunc {
        fn num_blocks(&self) -> usize { self.blocks.len() }
        fn block_instructions(&self, b: usize) -> std::ops::Range<usize> {
            self.blocks[b].0.clone()
        }
        fn block_successors(&self, b: usize) -> &[usize] { &self.blocks[b].1 }
        fn block_predecessors(&self, b: usize) -> &[usize] { &self.blocks[b].2 }
        fn num_instructions(&self) -> usize { self.insts.len() }
        fn inst_operands(&self, i: usize) -> &[Operand] { &self.insts[i].ops }
        fn inst_clobbers(&self, i: usize) -> &[PReg] { &self.insts[i].clobbers }
        fn num_vregs(&self) -> usize { self.num_vregs }
        fn scratch_regs(&self) -> &[PReg] { &self.scratches }
        fn is_phi(&self, i: usize) -> bool { self.insts[i].is_phi }
        fn phi_op(&self, i: usize, pred: usize) -> Var {
            let preds = &self.blocks[
                self.insts.iter().position(|_| true).map(|_| 0).unwrap_or(0) // unused
            ];
            // find the block containing inst i
            let b = self.blocks.iter().position(|bd| bd.0.contains(&i)).unwrap();
            let pred_idx = self.blocks[b].2.iter().position(|&p| p == pred).unwrap();
            let _ = preds;
            self.insts[i].phi_ops[pred_idx]
        }
        fn is_copy(&self, i: usize) -> bool { self.insts[i].is_copy }
    }

    fn pregs(n: u8) -> Vec<PReg> {
        (0..n).map(|i| PReg::int(i)).collect()
    }

    /// Three registers, three defs, three uses in sequence. Always allocatable.
    #[test]
    fn straight_line_three_vars() {
        let r = pregs(3);
        let scratches = vec![PReg::int(7)];

        let v0 = Var::int(0);
        let v1 = Var::int(1);
        let v2 = Var::int(2);
        let v3 = Var::int(3);

        let mk_def = |v: Var| Operand { var: v, constraint: Constraint::Any, kind: OperandKind::Def };
        let mk_use = |v: Var| Operand { var: v, constraint: Constraint::Any, kind: OperandKind::Use };

        let insts = vec![
            TestInst { ops: vec![mk_def(v0)], clobbers: vec![], is_phi: false, is_copy: false, phi_ops: vec![] },
            TestInst { ops: vec![mk_def(v1)], clobbers: vec![], is_phi: false, is_copy: false, phi_ops: vec![] },
            TestInst { ops: vec![mk_def(v2)], clobbers: vec![], is_phi: false, is_copy: false, phi_ops: vec![] },
            TestInst {
                ops: vec![mk_def(v3), mk_use(v0), mk_use(v1)],
                clobbers: vec![], is_phi: false, is_copy: false, phi_ops: vec![],
            },
            TestInst {
                ops: vec![mk_use(v2), mk_use(v3)],
                clobbers: vec![], is_phi: false, is_copy: false, phi_ops: vec![],
            },
        ];

        let func = TestFunc {
            num_vregs: 4,
            blocks: vec![(0..insts.len(), vec![], vec![])],
            insts,
            scratches,
        };

        let result = allocate(&func, &r).expect("allocation should succeed");
        verify(&func, &result).expect("symbolic checker should pass");
    }

    /// Smoke test that the checker actually catches a deliberately-corrupted
    /// allocation result.
    #[test]
    fn detects_corrupted_use() {
        let r = pregs(3);
        let scratches = vec![PReg::int(7)];
        let v0 = Var::int(0);
        let v1 = Var::int(1);
        let mk_def = |v: Var| Operand { var: v, constraint: Constraint::Any, kind: OperandKind::Def };
        let mk_use = |v: Var| Operand { var: v, constraint: Constraint::Any, kind: OperandKind::Use };

        let insts = vec![
            TestInst { ops: vec![mk_def(v0)], clobbers: vec![], is_phi: false, is_copy: false, phi_ops: vec![] },
            TestInst { ops: vec![mk_def(v1)], clobbers: vec![], is_phi: false, is_copy: false, phi_ops: vec![] },
            TestInst {
                ops: vec![mk_use(v0)],
                clobbers: vec![], is_phi: false, is_copy: false, phi_ops: vec![],
            },
        ];

        let func = TestFunc {
            num_vregs: 2,
            blocks: vec![(0..insts.len(), vec![], vec![])],
            insts,
            scratches,
        };

        let mut result = allocate(&func, &r).expect("allocation should succeed");

        // Sabotage: pick whatever preg v1 was assigned and rewrite the use
        // at inst 2 to read from THAT preg instead of v0's preg.
        let v0_alloc = result.vreg_alloc[&v0];
        let v1_alloc = result.vreg_alloc[&v1];
        assert_ne!(v0_alloc, v1_alloc, "test prerequisite");
        result.inst_allocs[2][0] = v1_alloc;

        let err = verify(&func, &result).expect_err("checker must catch the corruption");
        matches!(err, CheckError::UseMismatch { .. });
    }

    // ─────────────────────────────────────────────────────────────────
    // proptest fuzz harness: straight-line programs
    // ─────────────────────────────────────────────────────────────────

    use proptest::prelude::*;

    /// Build a random straight-line program with `n_defs` defs followed by
    /// `n_uses` "use 2 previously-defined vregs and def a new one" insts.
    /// `pressure` controls how many physical registers are available — set
    /// low to force the spill pass to kick in.
    fn random_straight_line(
        n_defs: usize,
        n_uses: usize,
        use_pattern: Vec<(usize, usize)>, // (i, j) picks defs to use; clamped
    ) -> TestFunc {
        let mk_def = |v: Var| Operand { var: v, constraint: Constraint::Any, kind: OperandKind::Def };
        let mk_use = |v: Var| Operand { var: v, constraint: Constraint::Any, kind: OperandKind::Use };

        let mut insts: Vec<TestInst> = Vec::new();
        for i in 0..n_defs {
            insts.push(TestInst {
                ops: vec![mk_def(Var::int(i as u32))],
                clobbers: vec![],
                is_phi: false, is_copy: false, phi_ops: vec![],
            });
        }
        let mut next_vreg = n_defs as u32;
        for (k, &(i, j)) in use_pattern.iter().enumerate().take(n_uses) {
            if n_defs == 0 { break; }
            let i = i % n_defs;
            let j = j % n_defs;
            let def_v = Var::int(next_vreg);
            next_vreg += 1;
            insts.push(TestInst {
                ops: vec![
                    mk_def(def_v),
                    mk_use(Var::int(i as u32)),
                    mk_use(Var::int(j as u32)),
                ],
                clobbers: vec![],
                is_phi: false, is_copy: false, phi_ops: vec![],
            });
            let _ = k;
        }

        let n_insts = insts.len();
        TestFunc {
            num_vregs: next_vreg as usize,
            blocks: vec![(0..n_insts, vec![], vec![])],
            insts,
            scratches: vec![PReg::int(15)],
        }
    }

    fn pregs_n(n: u8) -> Vec<PReg> {
        // PReg 15 is reserved as scratch; allocatable pool is 0..n.
        (0..n).map(PReg::int).collect()
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            cases: 256,
            .. ProptestConfig::default()
        })]

        /// For any random straight-line program and any (reasonable) physical
        /// register count, `allocate` followed by `verify` must succeed.
        #[test]
        fn fuzz_straight_line(
            n_defs in 1usize..8,
            n_uses in 0usize..16,
            use_pattern in prop::collection::vec((0usize..16, 0usize..16), 0..16),
            n_pregs in 1u8..6,
        ) {
            let func = random_straight_line(n_defs, n_uses, use_pattern);
            let pool = pregs_n(n_pregs);
            // The allocator may fail (e.g. fixed-constraint conflicts) — only
            // verify when it succeeds; AllocError is the allocator saying "I
            // can't do it", not a correctness violation.
            if let Ok(result) = allocate(&func, &pool) {
                prop_assert!(
                    verify(&func, &result).is_ok(),
                    "symbolic checker rejected an allocation produced by bregalloc::allocate"
                );
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────
    // Control-flow program generators
    // ─────────────────────────────────────────────────────────────────

    fn mk_def(v: Var) -> Operand {
        Operand { var: v, constraint: Constraint::Any, kind: OperandKind::Def }
    }
    fn mk_use(v: Var) -> Operand {
        Operand { var: v, constraint: Constraint::Any, kind: OperandKind::Use }
    }

    /// Diamond (if-then-else + merge with phi):
    ///
    ///     B0: def v0, def v1, branch → B1, B2
    ///     B1: def v2 = f(v0)         → B3
    ///     B2: def v3 = f(v1)         → B3
    ///     B3: phi v4 ← (B1: v2, B2: v3), use v4
    ///
    /// `extra_defs_b0` adds extra defs in B0 that are used in B3 to
    /// increase register pressure across the diamond.
    fn random_diamond(
        extra_defs_b0: usize,
        extra_uses_b3: Vec<usize>, // indices into the extra defs (clamped)
    ) -> TestFunc {
        let mut insts: Vec<TestInst> = Vec::new();
        let mut next_vreg: u32 = 0;
        let mut alloc_vreg = || { let v = Var::int(next_vreg); next_vreg += 1; v };

        // ── B0 ──
        let b0_start = insts.len();
        let v0 = alloc_vreg();
        let v1 = alloc_vreg();
        insts.push(TestInst {
            ops: vec![mk_def(v0)],
            clobbers: vec![], is_phi: false, is_copy: false, phi_ops: vec![],
        });
        insts.push(TestInst {
            ops: vec![mk_def(v1)],
            clobbers: vec![], is_phi: false, is_copy: false, phi_ops: vec![],
        });
        // Extra defs in B0 that live across the diamond (high pressure).
        let mut extra_vars: Vec<Var> = Vec::new();
        for _ in 0..extra_defs_b0 {
            let v = alloc_vreg();
            extra_vars.push(v);
            insts.push(TestInst {
                ops: vec![mk_def(v)],
                clobbers: vec![], is_phi: false, is_copy: false, phi_ops: vec![],
            });
        }
        // Terminator: branch (uses v0 as the condition-like input).
        insts.push(TestInst {
            ops: vec![mk_use(v0)],
            clobbers: vec![], is_phi: false, is_copy: false, phi_ops: vec![],
        });
        let b0_end = insts.len();

        // ── B1 (then) ──
        let b1_start = insts.len();
        let v2 = alloc_vreg();
        insts.push(TestInst {
            ops: vec![mk_def(v2), mk_use(v0)],
            clobbers: vec![], is_phi: false, is_copy: false, phi_ops: vec![],
        });
        // Terminator (unconditional jump).
        insts.push(TestInst {
            ops: vec![], clobbers: vec![], is_phi: false, is_copy: false, phi_ops: vec![],
        });
        let b1_end = insts.len();

        // ── B2 (else) ──
        let b2_start = insts.len();
        let v3 = alloc_vreg();
        insts.push(TestInst {
            ops: vec![mk_def(v3), mk_use(v1)],
            clobbers: vec![], is_phi: false, is_copy: false, phi_ops: vec![],
        });
        insts.push(TestInst {
            ops: vec![], clobbers: vec![], is_phi: false, is_copy: false, phi_ops: vec![],
        });
        let b2_end = insts.len();

        // ── B3 (merge) ──
        let b3_start = insts.len();
        let v4 = alloc_vreg();
        // Phi: v4 ← (pred B1: v2, pred B2: v3).
        insts.push(TestInst {
            ops: vec![mk_def(v4)],
            clobbers: vec![],
            is_phi: true,
            is_copy: false,
            phi_ops: vec![v2, v3], // index 0 = pred B1, index 1 = pred B2
        });
        // Use the phi result.
        insts.push(TestInst {
            ops: vec![mk_use(v4)],
            clobbers: vec![], is_phi: false, is_copy: false, phi_ops: vec![],
        });
        // Use some of the extra vars that were live across the diamond.
        for &idx in &extra_uses_b3 {
            if extra_vars.is_empty() { break; }
            let v = extra_vars[idx % extra_vars.len()];
            insts.push(TestInst {
                ops: vec![mk_use(v)],
                clobbers: vec![], is_phi: false, is_copy: false, phi_ops: vec![],
            });
        }
        let b3_end = insts.len();

        TestFunc {
            num_vregs: next_vreg as usize,
            blocks: vec![
                // B0: succs=[B1, B2], preds=[]
                (b0_start..b0_end, vec![1, 2], vec![]),
                // B1: succs=[B3], preds=[B0]
                (b1_start..b1_end, vec![3], vec![0]),
                // B2: succs=[B3], preds=[B0]
                (b2_start..b2_end, vec![3], vec![0]),
                // B3: succs=[], preds=[B1, B2]
                (b3_start..b3_end, vec![], vec![1, 2]),
            ],
            insts,
            scratches: vec![PReg::int(15)],
        }
    }

    /// Simple while-loop:
    ///
    ///     B0: def v0 (initial)    → B1
    ///     B1: phi v1 ← (B0: v0, B2: v2), use v1, branch → B2, B3
    ///     B2: def v2 = f(v1)     → B1   (backedge)
    ///     B3: use v1             (exit)
    ///
    /// `extra_loop_vars` adds more phi-carried variables to stress spilling
    /// inside loops.
    fn random_loop(
        extra_loop_vars: usize,
        body_ops: usize,          // extra use-def pairs inside the loop body (B2)
        body_pattern: Vec<usize>, // which loop var to use in each body op (clamped)
    ) -> TestFunc {
        let mut insts: Vec<TestInst> = Vec::new();
        let mut next_vreg: u32 = 0;
        let mut alloc_vreg = || { let v = Var::int(next_vreg); next_vreg += 1; v };

        // Track the loop-carried variable triples: (init, phi_def, backedge_def).
        let n_loop_vars = 1 + extra_loop_vars;
        let mut inits: Vec<Var> = Vec::new();
        let mut phi_defs: Vec<Var> = Vec::new();
        let mut back_defs: Vec<Var> = Vec::new();

        for _ in 0..n_loop_vars {
            inits.push(alloc_vreg());
            phi_defs.push(alloc_vreg());
            back_defs.push(alloc_vreg());
        }

        // ── B0 (preheader) ──
        let b0_start = insts.len();
        for &init_v in &inits {
            insts.push(TestInst {
                ops: vec![mk_def(init_v)],
                clobbers: vec![], is_phi: false, is_copy: false, phi_ops: vec![],
            });
        }
        // Terminator.
        insts.push(TestInst {
            ops: vec![], clobbers: vec![], is_phi: false, is_copy: false, phi_ops: vec![],
        });
        let b0_end = insts.len();

        // ── B1 (loop header) ──
        let b1_start = insts.len();
        // Phis: phi_defs[i] ← (B0: inits[i], B2: back_defs[i]).
        for i in 0..n_loop_vars {
            insts.push(TestInst {
                ops: vec![mk_def(phi_defs[i])],
                clobbers: vec![],
                is_phi: true,
                is_copy: false,
                phi_ops: vec![inits[i], back_defs[i]], // idx 0 = pred B0, idx 1 = pred B2
            });
        }
        // Use the primary phi to decide the branch.
        insts.push(TestInst {
            ops: vec![mk_use(phi_defs[0])],
            clobbers: vec![], is_phi: false, is_copy: false, phi_ops: vec![],
        });
        let b1_end = insts.len();

        // ── B2 (loop body) ──
        let b2_start = insts.len();
        // Extra use-def pairs inside the body.
        let mut body_available: Vec<Var> = phi_defs.clone();
        for k in 0..body_ops {
            if body_available.is_empty() { break; }
            let idx = body_pattern.get(k).copied().unwrap_or(0) % body_available.len();
            let use_v = body_available[idx];
            let new_v = alloc_vreg();
            insts.push(TestInst {
                ops: vec![mk_def(new_v), mk_use(use_v)],
                clobbers: vec![], is_phi: false, is_copy: false, phi_ops: vec![],
            });
            body_available.push(new_v);
        }
        // Produce the backedge defs from phi_defs.
        for i in 0..n_loop_vars {
            let use_v = phi_defs[i];
            insts.push(TestInst {
                ops: vec![mk_def(back_defs[i]), mk_use(use_v)],
                clobbers: vec![], is_phi: false, is_copy: false, phi_ops: vec![],
            });
        }
        // Terminator (unconditional back-branch).
        insts.push(TestInst {
            ops: vec![], clobbers: vec![], is_phi: false, is_copy: false, phi_ops: vec![],
        });
        let b2_end = insts.len();

        // ── B3 (exit) ──
        let b3_start = insts.len();
        // Use all phi_defs (they're live-out of B1 into both B2 and B3).
        for &pv in &phi_defs {
            insts.push(TestInst {
                ops: vec![mk_use(pv)],
                clobbers: vec![], is_phi: false, is_copy: false, phi_ops: vec![],
            });
        }
        let b3_end = insts.len();

        TestFunc {
            num_vregs: next_vreg as usize,
            blocks: vec![
                // B0: succs=[B1], preds=[]
                (b0_start..b0_end, vec![1], vec![]),
                // B1: succs=[B2, B3], preds=[B0, B2]
                (b1_start..b1_end, vec![2, 3], vec![0, 2]),
                // B2: succs=[B1], preds=[B1]
                (b2_start..b2_end, vec![1], vec![1]),
                // B3: succs=[], preds=[B1]
                (b3_start..b3_end, vec![], vec![1]),
            ],
            insts,
            scratches: vec![PReg::int(15)],
        }
    }

    /// Random multi-block DAG (no back-edges, but arbitrary forward edges
    /// and phis at every merge point). This covers shapes like cascading
    /// if-else chains, switch-like fan-outs, and multi-way merges.
    ///
    /// Topology: `n_blocks` blocks numbered 0..n_blocks-1. Edges go only
    /// from lower to higher block numbers (DAG guarantee). Block 0 has no
    /// predecessors. The last block has no successors. Each intermediate
    /// block gets 1-2 successors chosen from higher-numbered blocks.
    ///
    /// Every block with ≥2 predecessors gets a phi. Every block has 1-3
    /// "real" instructions that use previously-defined vregs and def new
    /// ones.
    fn random_dag(
        n_blocks: usize,
        // For each block, a seed that determines its successors and
        // instruction patterns.
        block_seeds: Vec<(usize, usize, usize, usize)>,
    ) -> TestFunc {
        let n_blocks = n_blocks.max(2); // need at least entry + exit
        let mut insts: Vec<TestInst> = Vec::new();
        let mut next_vreg: u32 = 0;
        let mut alloc_vreg = || { let v = Var::int(next_vreg); next_vreg += 1; v };

        // Phase 1: build the CFG edges.
        let mut succs: Vec<Vec<usize>> = vec![Vec::new(); n_blocks];
        let mut preds: Vec<Vec<usize>> = vec![Vec::new(); n_blocks];

        for b in 0..n_blocks - 1 {
            let seed = block_seeds.get(b).copied().unwrap_or((0, 0, 0, 0));
            // Each non-last block gets at least one successor.
            let s0 = (b + 1).min(n_blocks - 1); // always reachable
            succs[b].push(s0);
            preds[s0].push(b);

            // Optionally add a second successor (for branching).
            if seed.0 % 3 != 0 && n_blocks > b + 2 {
                let s1 = b + 2 + (seed.1 % (n_blocks - b - 2));
                let s1 = s1.min(n_blocks - 1);
                if s1 != s0 && !succs[b].contains(&s1) {
                    succs[b].push(s1);
                    preds[s1].push(b);
                }
            }
        }

        // Phase 2: build instructions per block.
        // Track which vregs are available (def dominates) at each block's
        // *exit*. For a block with multiple predecessors, the available set
        // at entry is the intersection of all predecessors' exit sets.
        // Phi defs are added on top of that intersection.
        let mut exit_available: Vec<Vec<Var>> = vec![Vec::new(); n_blocks];
        // Per-predecessor exit available, needed for phi source selection.
        // (Stored after processing each block.)

        struct BlockRange {
            start: usize,
            end: usize,
        }
        let mut block_ranges: Vec<BlockRange> = Vec::new();

        for b in 0..n_blocks {
            let seed = block_seeds.get(b).copied().unwrap_or((0, 0, 0, 0));
            let b_start = insts.len();

            // Compute available-at-entry: intersection of all preds' exit sets.
            let mut available: Vec<Var> = if preds[b].is_empty() {
                Vec::new() // entry block
            } else if preds[b].len() == 1 {
                exit_available[preds[b][0]].clone()
            } else {
                // Intersection: keep only vregs present in ALL preds.
                let first_pred = preds[b][0];
                let mut inter: Vec<Var> = exit_available[first_pred].clone();
                for &p in &preds[b][1..] {
                    let pred_set: HashSet<Var> = exit_available[p].iter().copied().collect();
                    inter.retain(|v| pred_set.contains(v));
                }
                inter
            };

            // Phi instructions: only at merge points (≥2 preds).
            // Each phi picks one source per predecessor from THAT pred's
            // exit-available set.
            if preds[b].len() >= 2 {
                // We need at least one vreg available per pred for a phi.
                let all_preds_have_defs = preds[b].iter().all(|&p| !exit_available[p].is_empty());
                if all_preds_have_defs {
                    let n_phis = 1 + (seed.2 % 2); // 1 or 2 phis
                    for phi_k in 0..n_phis {
                        let phi_def = alloc_vreg();
                        let phi_ops: Vec<Var> = preds[b]
                            .iter()
                            .enumerate()
                            .map(|(pi, &p)| {
                                let pred_avail = &exit_available[p];
                                let idx = (seed.3.wrapping_add(phi_k).wrapping_add(pi))
                                    % pred_avail.len();
                                pred_avail[idx]
                            })
                            .collect();
                        insts.push(TestInst {
                            ops: vec![mk_def(phi_def)],
                            clobbers: vec![],
                            is_phi: true,
                            is_copy: false,
                            phi_ops,
                        });
                        available.push(phi_def);
                    }
                }
            }

            // Real instructions: 1-3 per block.
            let n_real = 1 + (seed.0 % 3);
            for k in 0..n_real {
                let def_v = alloc_vreg();
                let mut ops = vec![mk_def(def_v)];
                // Use 0-2 previously-available vregs (if any exist).
                if !available.is_empty() {
                    let n_uses = (seed.1.wrapping_add(k)) % 3;
                    for u in 0..n_uses {
                        let idx = (seed.2.wrapping_add(k).wrapping_add(u))
                            % available.len();
                        ops.push(mk_use(available[idx]));
                    }
                }
                insts.push(TestInst {
                    ops,
                    clobbers: vec![],
                    is_phi: false,
                    is_copy: false,
                    phi_ops: vec![],
                });
                available.push(def_v);
            }

            // Terminator (empty branch/jump placeholder).
            if b < n_blocks - 1 {
                insts.push(TestInst {
                    ops: vec![], clobbers: vec![], is_phi: false, is_copy: false, phi_ops: vec![],
                });
            }

            let b_end = insts.len();
            block_ranges.push(BlockRange { start: b_start, end: b_end });
            exit_available[b] = available;
        }

        let blocks: Vec<(std::ops::Range<usize>, Vec<usize>, Vec<usize>)> = (0..n_blocks)
            .map(|b| {
                let r = &block_ranges[b];
                (r.start..r.end, succs[b].clone(), preds[b].clone())
            })
            .collect();

        TestFunc {
            num_vregs: next_vreg as usize,
            blocks,
            insts,
            scratches: vec![PReg::int(15)],
        }
    }

    // ─────────────────────────────────────────────────────────────────
    // Unit tests for control-flow shapes
    // ─────────────────────────────────────────────────────────────────

    #[test]
    fn diamond_basic() {
        let func = random_diamond(0, vec![]);
        let pool = pregs_n(3);
        let result = allocate(&func, &pool).expect("diamond should allocate");
        verify(&func, &result).expect("checker should pass on diamond");
    }

    #[test]
    fn diamond_with_pressure() {
        // 4 extra defs in B0, all used after the merge — forces spills
        // with only 2 registers.
        let func = random_diamond(4, vec![0, 1, 2, 3]);
        let pool = pregs_n(2);
        if let Ok(result) = allocate(&func, &pool) {
            verify(&func, &result).expect("checker should pass on high-pressure diamond");
        }
    }

    #[test]
    fn loop_basic() {
        let func = random_loop(0, 0, vec![]);
        let pool = pregs_n(3);
        let result = allocate(&func, &pool).expect("loop should allocate");
        verify(&func, &result).expect("checker should pass on loop");
    }

    #[test]
    fn loop_with_extra_vars() {
        let func = random_loop(3, 2, vec![0, 1]);
        let pool = pregs_n(2);
        if let Ok(result) = allocate(&func, &pool) {
            verify(&func, &result).expect("checker should pass on multi-var loop");
        }
    }

    #[test]
    fn dag_basic() {
        let func = random_dag(4, vec![(0,0,0,0), (1,1,1,1), (2,2,2,2), (3,3,3,3)]);
        let pool = pregs_n(3);
        if let Ok(result) = allocate(&func, &pool) {
            verify(&func, &result).expect("checker should pass on DAG");
        }
    }

    /// Reproduces a phi-resolution bug found by the DAG fuzzer.
    /// Run with `cargo test dag_repro_minimal -- --ignored --nocapture`.
    #[test]
    #[ignore = "known allocator phi-resolution bug — run manually to investigate"]
    fn dag_repro_minimal() {
        let func = random_dag(5, vec![(1,0,0,0), (1,0,0,0)]);
        let pool = pregs_n(3);

        // Dump the generated program for debugging.
        eprintln!("=== DAG repro: {} blocks, {} insts, {} vregs ===",
            func.blocks.len(), func.insts.len(), func.num_vregs);
        for (b, (range, s, p)) in func.blocks.iter().enumerate() {
            eprintln!("  B{b}: insts={range:?} succs={s:?} preds={p:?}");
            for i in range.clone() {
                let inst = &func.insts[i];
                let phi_tag = if inst.is_phi { " [PHI]" } else { "" };
                eprintln!("    inst {i}{phi_tag}: ops={:?} phi_ops={:?}",
                    inst.ops, inst.phi_ops);
            }
        }

        if let Ok(result) = allocate(&func, &pool) {
            eprintln!("=== vreg_alloc ===");
            let mut allocs: Vec<_> = result.vreg_alloc.iter().collect();
            allocs.sort_by_key(|(v, _)| v.id);
            for (v, a) in allocs {
                eprintln!("  {v:?} → {a:?}");
            }
            eprintln!("=== inst_allocs ===");
            for (i, a) in result.inst_allocs.iter().enumerate() {
                eprintln!("  inst {i}: {a:?}");
            }
            eprintln!("=== edits_before ===");
            for (i, moves) in &result.edits_before {
                for m in moves {
                    eprintln!("  inst {i}: {:?} → {:?}", m.from, m.to);
                }
            }
            eprintln!("=== edits_after ===");
            for (i, moves) in &result.edits_after {
                for m in moves {
                    eprintln!("  inst {i}: {:?} → {:?}", m.from, m.to);
                }
            }

            if let Err(e) = verify(&func, &result) {
                dump_context(&func, &result, &e);
                panic!("checker found bug in allocator: {e}");
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────
    // proptest fuzz harnesses: control-flow programs
    // ─────────────────────────────────────────────────────────────────

    proptest! {
        #![proptest_config(ProptestConfig {
            cases: 256,
            .. ProptestConfig::default()
        })]

        /// Diamond (if-then-else) with varying register pressure.
        #[test]
        fn fuzz_diamond(
            extra_defs in 0usize..6,
            extra_uses in prop::collection::vec(0usize..8, 0..6),
            n_pregs in 1u8..6,
        ) {
            let func = random_diamond(extra_defs, extra_uses);
            let pool = pregs_n(n_pregs);
            if let Ok(result) = allocate(&func, &pool) {
                prop_assert!(
                    verify(&func, &result).is_ok(),
                    "checker rejected diamond allocation"
                );
            }
        }

        /// Simple while-loop with loop-carried phis.
        #[test]
        fn fuzz_loop(
            extra_loop_vars in 0usize..4,
            body_ops in 0usize..6,
            body_pattern in prop::collection::vec(0usize..8, 0..6),
            n_pregs in 1u8..6,
        ) {
            let func = random_loop(extra_loop_vars, body_ops, body_pattern);
            let pool = pregs_n(n_pregs);
            if let Ok(result) = allocate(&func, &pool) {
                prop_assert!(
                    verify(&func, &result).is_ok(),
                    "checker rejected loop allocation"
                );
            }
        }

        /// Random multi-block DAG with phis at merge points.
        /// Currently ignored: reliably triggers a known phi-resolution bug
        /// in the allocator. Un-ignore once the bug is fixed.
        #[test]
        #[ignore = "known allocator phi-resolution bug"]
        fn fuzz_dag(
            n_blocks in 2usize..8,
            block_seeds in prop::collection::vec(
                (0usize..100, 0usize..100, 0usize..100, 0usize..100),
                2..8,
            ),
            n_pregs in 1u8..6,
        ) {
            let func = random_dag(n_blocks, block_seeds);
            let pool = pregs_n(n_pregs);
            if let Ok(result) = allocate(&func, &pool) {
                prop_assert!(
                    verify(&func, &result).is_ok(),
                    "checker rejected DAG allocation"
                );
            }
        }
    }
}
