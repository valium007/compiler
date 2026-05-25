use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct Var(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct PReg(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct SpillSlot(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Allocation {
    Reg(PReg),
    Stack(SpillSlot),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Constraint {
    Any,
    /// Any physical register, but never a spill slot.  Use for operands whose
    /// instruction encoding forbids a memory operand (e.g. x86 2-operand
    /// `imul` destination).
    Reg,
    Fixed(PReg),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OperandKind {
    Use,
    Def,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Operand {
    pub var: Var,
    pub constraint: Constraint,
    pub kind: OperandKind,
}

pub trait AllocFunction {
    fn num_blocks(&self) -> usize;
    fn block_instructions(&self, block: usize) -> std::ops::Range<usize>;
    fn block_successors(&self, block: usize) -> &[usize];
    fn block_predecessors(&self, block: usize) -> &[usize];
    fn num_instructions(&self) -> usize;
    fn inst_operands(&self, inst: usize) -> &[Operand];
    fn inst_clobbers(&self, inst: usize) -> &[PReg];
    fn num_vregs(&self) -> usize;
    fn scratch_regs(&self) -> &[PReg];
    fn is_valid_combination(&self, inst: usize, allocs: &[Allocation]) -> bool;

    fn is_phi(&self, inst: usize) -> bool;
    fn phi_op(&self, inst: usize, pred: usize) -> Var;
    fn is_copy(&self, inst: usize) -> bool;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AllocMove {
    pub from: Allocation,
    pub to: Allocation,
}

#[derive(Debug)]
pub enum AllocError {
    AllocationFailed(String),
}

pub struct AllocationResult {
    pub vreg_alloc: HashMap<Var, Allocation>,
    pub inst_allocs: Vec<Vec<Allocation>>,
    pub edits_before: HashMap<usize, Vec<AllocMove>>,
    pub edits_after: HashMap<usize, Vec<AllocMove>>,
    pub num_spillslots: usize,
}

struct AllocationAttemptResult {
    vreg_alloc: HashMap<Var, Allocation>,
    inst_allocs: Vec<Vec<Allocation>>,
    edits_before: HashMap<usize, Vec<AllocMove>>,
    edits_after: HashMap<usize, Vec<AllocMove>>,
}

struct VarToSpill(Var);

pub fn allocate<F: AllocFunction>(
    func: &F,
    allocatable_regs: &[PReg],
) -> Result<AllocationResult, AllocError> {
    let mut spilled_vars: HashMap<Var, SpillSlot> = HashMap::new();
    let mut next_spill_slot = 0;

    let scratch_regs: HashSet<PReg> = func.scratch_regs().iter().copied().collect();
    let filtered_allocatable: Vec<PReg> = allocatable_regs
        .iter()
        .copied()
        .filter(|r| !scratch_regs.contains(r))
        .collect();

    // Map each register to the instructions that clobber it
    let mut clobbered_insts: HashMap<PReg, Vec<usize>> = HashMap::new();
    for inst in 0..func.num_instructions() {
        for &preg in func.inst_clobbers(inst) {
            clobbered_insts.entry(preg).or_default().push(inst);
        }
    }

    // Identify all pinned variables (cannot be spilled)
    let mut pinned_vars = HashSet::new();
    for inst in 0..func.num_instructions() {
        for op in func.inst_operands(inst) {
            if matches!(op.constraint, Constraint::Fixed(_) | Constraint::Reg) {
                pinned_vars.insert(op.var);
            }
        }
    }

    loop {
        match allocate_attempt(
            func,
            &filtered_allocatable,
            &spilled_vars,
            &clobbered_insts,
            &pinned_vars,
        ) {
            Ok(result) => {
                return Ok(AllocationResult {
                    vreg_alloc: result.vreg_alloc,
                    inst_allocs: result.inst_allocs,
                    edits_before: result.edits_before,
                    edits_after: result.edits_after,
                    num_spillslots: next_spill_slot,
                });
            }
            Err(VarToSpill(v)) => {
                spilled_vars.insert(v, SpillSlot(next_spill_slot as u32));
                next_spill_slot += 1;
            }
        }
    }
}

/// `live_conflict(v, preg)` — would assigning `preg` as `v`'s home register
/// corrupt `v` (or be corrupted by `v`) at some instruction along its lifetime?
///
/// Returns true if there exists an instruction `I` such that:
///   - `I` requires `preg` (either `I` clobbers `preg`, or `I` has a Fixed(preg)
///     operand for *some* variable), AND
///   - `v` is live at `I`, AND
///   - `v` itself does NOT have a Fixed(preg) operand at `I`. (If it does, `v`'s
///     role at `I` is precisely to flow through `preg` — handled by the fixed
///     constraint, not a conflict.)
///
/// Used both for the pre-allocation invariant from the paper (§3.4) and for
/// the per-register conflict check inside the main `allocate_register` loop,
/// so that any variable — Fixed or not — is kept out of a register whose value
/// would be destroyed across the variable's live range.
fn live_conflict<F: AllocFunction>(
    v: Var,
    preg: PReg,
    func: &F,
    live_ins: &[HashSet<Var>],
    live_outs: &[HashSet<Var>],
    uses: &HashMap<Var, Vec<usize>>,
    var_defs: &HashMap<Var, Vec<usize>>,
    inst_block: &[usize],
    clobbered_insts: &HashMap<PReg, Vec<usize>>,
    fixed_insts: &HashMap<PReg, Vec<usize>>,
) -> bool {
    let v_is_fixed_to_preg_at = |inst: usize| -> bool {
        func.inst_operands(inst)
            .iter()
            .any(|op| op.var == v && op.constraint == Constraint::Fixed(preg))
    };

    // Skipping a clobber/fixed instruction `I` because `v` is itself fixed to
    // `preg` at `I` is only safe when `v` does not live past `I`. If `v` is
    // used again later, its value must survive `I` — but `I` clobbers `preg`
    // after consuming the use, so `preg` cannot be `v`'s home.
    let v_dies_at = |inst: usize| -> bool {
        let inst_flat = inst * 2 + 1;
        uses.get(&v).map_or(true, |ul| {
            !ul.iter().any(|&uf| uf > inst_flat)
        })
    };

    if let Some(clob_insts) = clobbered_insts.get(&preg) {
        for &ci in clob_insts {
            if v_is_fixed_to_preg_at(ci) && v_dies_at(ci) {
                continue;
            }
            if is_live_at(v, ci, func, live_ins, live_outs, uses, var_defs, inst_block) {
                return true;
            }
        }
    }

    if let Some(fi) = fixed_insts.get(&preg) {
        for &ii in fi {
            if v_is_fixed_to_preg_at(ii) && v_dies_at(ii) {
                continue;
            }
            if is_live_at(v, ii, func, live_ins, live_outs, uses, var_defs, inst_block) {
                return true;
            }
        }
    }

    false
}

fn allocate_attempt<F: AllocFunction>(
    func: &F,
    allocatable_regs: &[PReg],
    spilled_vars: &HashMap<Var, SpillSlot>,
    clobbered_insts: &HashMap<PReg, Vec<usize>>,
    pinned_vars: &HashSet<Var>,
) -> Result<AllocationAttemptResult, VarToSpill> {
    let num_blocks = func.num_blocks();
    let num_insts = func.num_instructions();

    // ─── 1. Liveness Analysis ───
    let mut live_ins = vec![HashSet::new(); num_blocks];
    let mut live_outs = vec![HashSet::new(); num_blocks];
    let mut changed = true;
    while changed {
        changed = false;
        for b in (0..num_blocks).rev() {
            let mut out_vars = HashSet::new();
            for &succ in func.block_successors(b) {
                for &v in &live_ins[succ] {
                    out_vars.insert(v);
                }
                for inst in func.block_instructions(succ) {
                    if func.is_phi(inst) {
                        let op_v = func.phi_op(inst, b);
                        out_vars.insert(op_v);
                    }
                }
            }
            live_outs[b] = out_vars;

            let mut current = live_outs[b].clone();
            let range = func.block_instructions(b);
            for inst in range.clone().rev() {
                for op in func.inst_operands(inst) {
                    match op.kind {
                        OperandKind::Def => {
                            current.remove(&op.var);
                        }
                        OperandKind::Use => {
                            current.insert(op.var);
                        }
                    }
                }
            }

            if current != live_ins[b] {
                live_ins[b] = current;
                changed = true;
            }
        }
    }

    // ─── 2. Definition and Use Mappings ───
    let mut var_defs: HashMap<Var, Vec<usize>> = HashMap::new();
    let mut inst_block = vec![0; num_insts];

    for b in 0..num_blocks {
        let range = func.block_instructions(b);
        for inst in range {
            inst_block[inst] = b;
            let inst_flat = inst * 2 + 1;
            for op in func.inst_operands(inst) {
                if matches!(op.kind, OperandKind::Def) {
                    var_defs.entry(op.var).or_default().push(inst_flat);
                }
            }
        }
    }

    let mut uses: HashMap<Var, Vec<usize>> = HashMap::new();
    for inst in 0..num_insts {
        let flat = inst * 2 + 1;
        for op in func.inst_operands(inst) {
            if matches!(op.kind, OperandKind::Use) {
                uses.entry(op.var).or_default().push(flat);
            }
        }
    }
    for b in 0..num_blocks {
        let term_inst = func.block_instructions(b).end.saturating_sub(1);
        let flat = term_inst * 2 + 1;
        for &succ in func.block_successors(b) {
            for inst in func.block_instructions(succ) {
                if func.is_phi(inst) {
                    let op_v = func.phi_op(inst, b);
                    uses.entry(op_v).or_default().push(flat);
                }
            }
        }
    }
    for use_list in uses.values_mut() {
        use_list.sort_unstable();
    }

    // ─── 2b. Coalescing Equivalence Groups ───
    let mut coalesce_groups = CoalesceGroups::new(func.num_vregs());
    let mut group_alloc = HashMap::new();

    for inst in 0..num_insts {
        if func.is_copy(inst) {
            let ops = func.inst_operands(inst);
            if ops.len() >= 2 {
                let dst = ops[0].var;
                let src = ops[1].var;
                coalesce_groups.union(dst.0 as usize, src.0 as usize);
            }
        } else if func.is_phi(inst) {
            let ops = func.inst_operands(inst);
            if let Some(first_op) = ops.first() {
                if matches!(first_op.kind, OperandKind::Def) {
                    let dst = first_op.var;
                    let b_dst = inst_block[inst];
                    for &pred_b in func.block_predecessors(b_dst) {
                        let src = func.phi_op(inst, pred_b);
                        coalesce_groups.union(dst.0 as usize, src.0 as usize);
                    }
                }
            }
        }
    }

    // ─── 3. Allocation Core State ───
    let mut active = HashMap::new();
    let mut future_active: HashMap<PReg, HashSet<Var>> = HashMap::new();
    let mut vreg_alloc = HashMap::new();

    for (&v, &slot) in spilled_vars {
        vreg_alloc.insert(v, Allocation::Stack(slot));
    }

    // Precompute fixed_insts: for each PReg, the instructions where some operand
    // has a Fixed(preg) constraint. Used by `live_conflict` to detect that a
    // register is implicitly "in use" at those points by the fixup mov sequence.
    let mut fixed_insts: HashMap<PReg, Vec<usize>> = HashMap::new();
    for inst in 0..num_insts {
        let mut seen: HashSet<PReg> = HashSet::new();
        for op in func.inst_operands(inst) {
            if let Constraint::Fixed(p) = op.constraint {
                if seen.insert(p) {
                    fixed_insts.entry(p).or_default().push(inst);
                }
            }
        }
    }

    // Pre-allocate fixed constraints into future_active so AllocateRegister can
    // pull the assignment for free (paper §3.5). The paper's §3.4 invariant is
    // enforced two ways:
    //   1. `live_conflict` ensures preg is genuinely safe for v's lifetime —
    //      no foreign clobber/fixed-use of preg crosses v.
    //   2. We also refuse to add v if anything already in future_active[preg]
    //      lives at the same time as v.
    for inst in 0..num_insts {
        for op in func.inst_operands(inst) {
            if let Constraint::Fixed(preg) = op.constraint {
                if spilled_vars.contains_key(&op.var) {
                    continue;
                }
                if future_active
                    .get(&preg)
                    .map_or(false, |s| s.contains(&op.var))
                {
                    continue;
                }
                if live_conflict(
                    op.var,
                    preg,
                    func,
                    &live_ins,
                    &live_outs,
                    &uses,
                    &var_defs,
                    &inst_block,
                    clobbered_insts,
                    &fixed_insts,
                ) {
                    continue;
                }
                let overlaps_existing = future_active
                    .get(&preg)
                    .map(|occs| {
                        occs.iter().any(|&u| {
                            live_at_the_same_time(
                                op.var,
                                u,
                                func,
                                &live_ins,
                                &live_outs,
                                &uses,
                                &var_defs,
                                &inst_block,
                            )
                        })
                    })
                    .unwrap_or(false);
                if overlaps_existing {
                    continue;
                }
                future_active.entry(preg).or_default().insert(op.var);
            }
        }
    }

    let mut visited = HashSet::new();

    for cur in 0..num_blocks {
        let live_ins_cur = &live_ins[cur];

        // Expire active registers
        let mut to_pause = Vec::new();
        let mut to_free = Vec::new();
        for (&preg, &v) in &active {
            if !live_ins_cur.contains(&v) {
                let is_live_in_future = (0..num_blocks)
                    .filter(|&b| !visited.contains(&b))
                    .any(|b| live_ins[b].contains(&v));
                if is_live_in_future {
                    to_pause.push((preg, v));
                } else {
                    to_free.push((preg, v));
                }
            }
        }
        for (preg, v) in to_pause {
            active.remove(&preg);
            future_active.entry(preg).or_default().insert(v);
        }
        for (preg, _) in to_free {
            active.remove(&preg);
        }

        // Start intervals for live-ins
        for &v in live_ins_cur {
            if spilled_vars.contains_key(&v) {
                continue;
            }
            if active.values().any(|&x| x == v) {
                continue;
            }
            allocate_register(
                cur,
                v,
                &mut active,
                &mut future_active,
                &mut vreg_alloc,
                allocatable_regs,
                func,
                &live_ins,
                &live_outs,
                &uses,
                &var_defs,
                &inst_block,
                clobbered_insts,
                &fixed_insts,
                pinned_vars,
                func.block_instructions(cur).start * 2,
                &mut coalesce_groups,
                &mut group_alloc,
            )?;
        }

        // Iterate instructions inside the block
        for inst in func.block_instructions(cur) {
            let inst_flat = inst * 2 + 1;

            // Expire inputs
            for op in func.inst_operands(inst) {
                if matches!(op.kind, OperandKind::Use) {
                    let v = op.var;
                    if spilled_vars.contains_key(&v) {
                        continue;
                    }
                    if let Some(&preg) = active.iter().find(|&(_, &x)| x == v).map(|(r, _)| r) {
                        let has_uses_after = uses.get(&v).map_or(false, |ul| {
                            ul.iter().any(|&uf| uf >= inst_flat + 1)
                        });
                        let is_live_in_future = (0..num_blocks)
                            .filter(|&b| !visited.contains(&b))
                            .any(|b| live_ins[b].contains(&v));

                        if has_uses_after || is_live_in_future {
                            active.remove(&preg);
                            future_active.entry(preg).or_default().insert(v);
                        } else {
                            active.remove(&preg);
                        }
                    }
                }
            }

            // Allocate registers for defs
            for op in func.inst_operands(inst) {
                if matches!(op.kind, OperandKind::Def) {
                    let v = op.var;
                    if spilled_vars.contains_key(&v) {
                        continue;
                    }
                    allocate_register(
                        cur,
                        v,
                        &mut active,
                        &mut future_active,
                        &mut vreg_alloc,
                        allocatable_regs,
                        func,
                        &live_ins,
                        &live_outs,
                        &uses,
                        &var_defs,
                        &inst_block,
                        clobbered_insts,
                        &fixed_insts,
                        pinned_vars,
                        inst_flat,
                        &mut coalesce_groups,
                        &mut group_alloc,
                    )?;
                }
            }
        }

        visited.insert(cur);
    }

    // ─── 4. Build Output and Insert Spill/Fill moves (Option B) ───
    let mut inst_allocs = vec![Vec::new(); num_insts];
    let mut edits_before: HashMap<usize, Vec<AllocMove>> = HashMap::new();
    let mut edits_after: HashMap<usize, Vec<AllocMove>> = HashMap::new();

    let scratch_regs_list = func.scratch_regs();

    for inst in 0..num_insts {
        let mut allocs = Vec::new();
        for op in func.inst_operands(inst) {
            let alloc = vreg_alloc.get(&op.var).copied().unwrap_or(Allocation::Reg(PReg(0)));
            allocs.push(alloc);
        }

        let mut scratch_idx = 0;
        let mut target_regs = HashSet::new();
        for (i, op) in func.inst_operands(inst).iter().enumerate() {
            if op.kind == OperandKind::Use {
                if let Constraint::Fixed(preg) = op.constraint {
                    if allocs[i] != Allocation::Reg(preg) {
                        target_regs.insert(preg);
                    }
                }
            }
        }

        for (i, op) in func.inst_operands(inst).iter().enumerate() {
            if op.kind == OperandKind::Use {
                if let Allocation::Reg(curr_preg) = allocs[i] {
                    if target_regs.contains(&curr_preg) {
                        if scratch_idx < scratch_regs_list.len() {
                            let scratch = scratch_regs_list[scratch_idx];
                            scratch_idx += 1;
                            
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

        for (i, op) in func.inst_operands(inst).iter().enumerate() {
            if let Constraint::Fixed(preg) = op.constraint {
                let curr_alloc = allocs[i];
                if curr_alloc != Allocation::Reg(preg) {
                    allocs[i] = Allocation::Reg(preg);
                    match op.kind {
                        OperandKind::Use => {
                            edits_before.entry(inst).or_default().push(AllocMove {
                                from: curr_alloc,
                                to: Allocation::Reg(preg),
                            });
                        }
                        OperandKind::Def => {
                            edits_after.entry(inst).or_default().push(AllocMove {
                                from: Allocation::Reg(preg),
                                to: curr_alloc,
                            });
                        }
                    }
                }
            }
        }

        if !func.is_valid_combination(inst, &allocs) {
            for (i, op) in func.inst_operands(inst).iter().enumerate() {
                if let Allocation::Stack(slot) = allocs[i] {
                    if scratch_idx < scratch_regs_list.len() {
                        let scratch = scratch_regs_list[scratch_idx];
                        scratch_idx += 1;
                        
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
                }
            }
        }

        inst_allocs[inst] = allocs;
    }


    Ok(AllocationAttemptResult {
        vreg_alloc,
        inst_allocs,
        edits_before,
        edits_after,
    })
}

fn allocate_register<F: AllocFunction>(
    _cur_block: usize,
    v: Var,
    active: &mut HashMap<PReg, Var>,
    future_active: &mut HashMap<PReg, HashSet<Var>>,
    vreg_alloc: &mut HashMap<Var, Allocation>,
    allocatable_regs: &[PReg],
    func: &F,
    live_ins: &[HashSet<Var>],
    live_outs: &[HashSet<Var>],
    uses: &HashMap<Var, Vec<usize>>,
    var_defs: &HashMap<Var, Vec<usize>>,
    inst_block: &[usize],
    clobbered_insts: &HashMap<PReg, Vec<usize>>,
    fixed_insts: &HashMap<PReg, Vec<usize>>,
    pinned_vars: &HashSet<Var>,
    current_inst_flat: usize,
    coalesce_groups: &mut CoalesceGroups,
    group_alloc: &mut HashMap<usize, PReg>,
) -> Result<(), VarToSpill> {
    // 0. Reuse existing register assignment (global consistency: a paused or
    //    previously-allocated var must come back into the same preg).
    if let Some(&Allocation::Reg(preg)) = vreg_alloc.get(&v) {
        if let Some(occs) = future_active.get_mut(&preg) {
            occs.remove(&v);
        }
        active.insert(preg, v);
        let rep = coalesce_groups.find(v.0 as usize);
        group_alloc.insert(rep, preg);
        return Ok(());
    }

    // 1. Pulled from future-active (paper Algorithm 4).
    let mut pulled = None;
    for (&preg, vars) in future_active.iter_mut() {
        if vars.contains(&v) {
            vars.remove(&v);
            pulled = Some(preg);
            break;
        }
    }
    if let Some(preg) = pulled {
        active.insert(preg, v);
        vreg_alloc.insert(v, Allocation::Reg(preg));
        let rep = coalesce_groups.find(v.0 as usize);
        group_alloc.insert(rep, preg);
        return Ok(());
    }

    let preg_conflict = |preg: PReg,
                         active: &HashMap<PReg, Var>,
                         future_active: &HashMap<PReg, HashSet<Var>>|
     -> bool {
        // Paper §3.4 invariant: cannot place v in preg if any current
        // occupant of active[preg] ∪ future_active[preg] is live at the
        // same time as v.
        if let Some(&u) = active.get(&preg) {
            if live_at_the_same_time(v, u, func, live_ins, live_outs, uses, var_defs, inst_block) {
                return true;
            }
        }
        if let Some(f_vars) = future_active.get(&preg) {
            for &u in f_vars {
                if live_at_the_same_time(v, u, func, live_ins, live_outs, uses, var_defs, inst_block) {
                    return true;
                }
            }
        }
        // Also: preg may be implicitly required (clobber or fixed-op fixup) at
        // some instruction along v's lifetime, even when no other variable is
        // pre-allocated to it. live_conflict catches that.
        if live_conflict(
            v,
            preg,
            func,
            live_ins,
            live_outs,
            uses,
            var_defs,
            inst_block,
            clobbered_insts,
            fixed_insts,
        ) {
            return true;
        }
        false
    };

    // 2. Prefer the coalesce group's existing register, when safe.
    let rep = coalesce_groups.find(v.0 as usize);
    if let Some(&pref_preg) = group_alloc.get(&rep) {
        if allocatable_regs.contains(&pref_preg)
            && !preg_conflict(pref_preg, active, future_active)
        {
            active.insert(pref_preg, v);
            vreg_alloc.insert(v, Allocation::Reg(pref_preg));
            return Ok(());
        }
    }

    // 3. Pick the lowest-cost free register.
    let mut best_preg = None;
    let mut best_cost = i32::MAX;
    for &preg in allocatable_regs {
        if preg_conflict(preg, active, future_active) {
            continue;
        }
        let cost = 0;
        if cost < best_cost {
            best_cost = cost;
            best_preg = Some(preg);
        }
    }

    if let Some(preg) = best_preg {
        active.insert(preg, v);
        vreg_alloc.insert(v, Allocation::Reg(preg));
        group_alloc.insert(rep, preg);
        Ok(())
    } else {
        let spill_cand = select_spill_candidate(active, v, pinned_vars, uses, current_inst_flat);
        Err(VarToSpill(spill_cand))
    }
}

fn select_spill_candidate(
    active: &HashMap<PReg, Var>,
    failing_var: Var,
    pinned_vars: &HashSet<Var>,
    uses: &HashMap<Var, Vec<usize>>,
    current_inst_flat: usize,
) -> Var {
    let mut candidates = Vec::new();
    if !pinned_vars.contains(&failing_var) {
        candidates.push(failing_var);
    }
    for &v in active.values() {
        if !pinned_vars.contains(&v) {
            candidates.push(v);
        }
    }

    if candidates.is_empty() {
        // Fallback in case everything is pinned
        return failing_var;
    }

    let mut best_cand = candidates[0];
    let mut furthest_use = 0;

    for cand in candidates {
        let next_use = uses.get(&cand).map_or(usize::MAX, |ul| {
            ul.iter().copied().find(|&uf| uf >= current_inst_flat).unwrap_or(usize::MAX)
        });
        if next_use > furthest_use {
            furthest_use = next_use;
            best_cand = cand;
        }
    }

    best_cand
}

fn is_live_at<F: AllocFunction>(
    v: Var,
    inst: usize,
    func: &F,
    live_ins: &[HashSet<Var>],
    live_outs: &[HashSet<Var>],
    uses: &HashMap<Var, Vec<usize>>,
    var_defs: &HashMap<Var, Vec<usize>>,
    inst_block: &[usize],
) -> bool {
    if inst >= inst_block.len() {
        return false;
    }
    let b = inst_block[inst];
    let range = func.block_instructions(b);

    if live_outs[b].contains(&v) {
        if let Some(defs) = var_defs.get(&v) {
            if let Some(&def_flat) = defs.iter().find(|&&df| inst_block[df / 2] == b) {
                let def_inst = def_flat / 2;
                return inst >= def_inst;
            }
        }
        return true;
    } else {
        let def_in_b = var_defs.get(&v).and_then(|defs| {
            defs.iter().copied().find(|&def_flat| {
                let def_inst = def_flat / 2;
                def_inst >= range.start && def_inst < range.end
            })
        });

        if let Some(def_flat) = def_in_b {
            let def_inst = def_flat / 2;
            if inst < def_inst {
                return false;
            }
        } else if !live_ins[b].contains(&v) {
            return false;
        }

        // Check if there is a use at or after inst in this block
        if let Some(uses_list) = uses.get(&v) {
            let start_flat = inst * 2 + 1;
            let end_flat = (range.end - 1) * 2 + 1;
            return uses_list.iter().any(|&uf| uf >= start_flat && uf <= end_flat);
        }
    }
    false
}

fn live_at_the_same_time(
    lhs: Var,
    rhs: Var,
    _func: &impl AllocFunction,
    live_ins: &[HashSet<Var>],
    live_outs: &[HashSet<Var>],
    uses: &HashMap<Var, Vec<usize>>,
    var_defs: &HashMap<Var, Vec<usize>>,
    inst_block: &[usize],
) -> bool {
    let empty = Vec::new();
    let lhs_defs = var_defs.get(&lhs).unwrap_or(&empty);
    let rhs_defs = var_defs.get(&rhs).unwrap_or(&empty);

    if lhs_defs.is_empty() || rhs_defs.is_empty() {
        return false;
    }

    // Check if they share any definition block
    for &def_l in lhs_defs {
        let b_l = inst_block[def_l / 2];
        for &def_r in rhs_defs {
            let b_r = inst_block[def_r / 2];
            if b_l == b_r {
                if live_at_the_same_time_same_block(lhs, rhs, b_l, live_outs, uses, def_l, def_r) {
                    return true;
                }
            }
        }
    }

    // Check across different blocks
    for &def_l in lhs_defs {
        let b_l = inst_block[def_l / 2];
        if live_at_the_same_time_in_block(lhs, rhs, b_l, live_ins, live_outs, uses, def_l) {
            return true;
        }
    }

    for &def_r in rhs_defs {
        let b_r = inst_block[def_r / 2];
        if live_at_the_same_time_in_block(rhs, lhs, b_r, live_ins, live_outs, uses, def_r) {
            return true;
        }
    }

    false
}

fn live_at_the_same_time_same_block(
    lhs: Var,
    rhs: Var,
    block: usize,
    live_outs: &[HashSet<Var>],
    uses: &HashMap<Var, Vec<usize>>,
    lhs_def_flat: usize,
    rhs_def_flat: usize,
) -> bool {
    let lhs_live_out = live_outs[block].contains(&lhs);
    let rhs_live_out = live_outs[block].contains(&rhs);

    if lhs_live_out && rhs_live_out {
        return true;
    } else if !lhs_live_out && !rhs_live_out {
        let (first, last_def_flat) = if lhs_def_flat < rhs_def_flat {
            (lhs, rhs_def_flat)
        } else {
            (rhs, lhs_def_flat)
        };
        if let Some(first_uses) = uses.get(&first) {
            for &u_flat in first_uses {
                if u_flat > last_def_flat {
                    return true;
                }
            }
        }
    } else {
        let (non_live_out, live_out_def_flat) = if lhs_live_out {
            (rhs, lhs_def_flat)
        } else {
            (lhs, rhs_def_flat)
        };
        if let Some(uses_list) = uses.get(&non_live_out) {
            for &u_flat in uses_list {
                if u_flat > live_out_def_flat {
                    return true;
                }
            }
        }
    }
    false
}

fn live_at_the_same_time_in_block(
    _lhs: Var,
    rhs: Var,
    lhs_block: usize,
    live_ins: &[HashSet<Var>],
    live_outs: &[HashSet<Var>],
    uses: &HashMap<Var, Vec<usize>>,
    lhs_def_flat: usize,
) -> bool {
    if live_ins[lhs_block].contains(&rhs) {
        if live_outs[lhs_block].contains(&rhs) {
            return true;
        }
        if let Some(rhs_uses) = uses.get(&rhs) {
            for &u_flat in rhs_uses {
                if u_flat > lhs_def_flat {
                    return true;
                }
            }
        }
    }
    false
}

struct CoalesceGroups {
    parent: Vec<usize>,
}

impl CoalesceGroups {
    fn new(size: usize) -> Self {
        Self {
            parent: (0..size).collect(),
        }
    }

    fn find(&mut self, i: usize) -> usize {
        let mut root = i;
        while root != self.parent[root] {
            root = self.parent[root];
        }
        let mut curr = i;
        while curr != root {
            let nxt = self.parent[curr];
            self.parent[curr] = root;
            curr = nxt;
        }
        root
    }

    fn union(&mut self, i: usize, j: usize) {
        let root_i = self.find(i);
        let root_j = self.find(j);
        if root_i != root_j {
            self.parent[root_i] = root_j;
        }
    }
}

