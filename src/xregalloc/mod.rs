use std::collections::{HashMap, HashSet};

/// Round `n` up to the next multiple of `align`. `align` must be >= 1.
fn align_up(n: usize, align: usize) -> usize {
    ((n + align - 1) / align) * align
}

/// The category of a value, dictating which physical register file it can
/// live in. Allocation is partitioned by class: an `Int` var can never be
/// considered for a `Float` preg, and vice versa.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum RegClass {
    Int = 0,
    Float = 1,
    Vector = 2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct Var {
    pub class: RegClass,
    pub id: u32,
}

impl Var {
    pub const fn new(id: u32, class: RegClass) -> Self {
        Self { class, id }
    }
    pub const fn int(id: u32) -> Self {
        Self { class: RegClass::Int, id }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct PReg {
    pub class: RegClass,
    pub index: u8,
}

impl PReg {
    pub const fn new(index: u8, class: RegClass) -> Self {
        Self { class, index }
    }
    pub const fn int(index: u8) -> Self {
        Self { class: RegClass::Int, index }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct SpillSlot(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Allocation {
    Reg(PReg),
    Stack(SpillSlot),
}

/// What occupies a physical register in `future_active`.
///
/// Per the paper, only real variables live in the future-active set. We also
/// pre-load *point reservations* (`Clobber` and `Fixed`) so the §3.4
/// LiveAtTheSameTime check alone is enough to detect every interference: there
/// is no separate `live_conflict` side-table for instructions that implicitly
/// require the register (e.g. `idiv` clobbering EDX, or a fixed EAX use).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Occupant {
    /// A real variable queued to occupy this preg later in the scan.
    Var(Var),
    /// The preg is reserved at this single instruction by a clobber.
    Clobber(usize),
    /// The preg is reserved at this single instruction by a fixed operand
    /// (some variable has `Constraint::Fixed(preg)` at this instruction).
    Fixed(usize),
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

    fn is_phi(&self, inst: usize) -> bool;
    fn phi_op(&self, inst: usize, pred: usize) -> Var;
    fn is_copy(&self, inst: usize) -> bool;

    /// How many logical spill slots does the given regclass require?
    ///
    /// E.g., on a 64-bit machine, spill slots may nominally be 64-bit words,
    /// but a 128-bit vector value will require two slots. The regalloc will
    /// always align on this size. Slot indices returned in `SpillSlot(_)` are
    /// in units of these logical slots.
    ///
    /// (Design and doc derive from `regalloc.rs`' trait of the same name.)
    fn spillslot_size(&self, regclass: RegClass) -> usize {
        // Default: every class fits in one nominal slot. Backends that have
        // wider classes (vectors, long/double pairs) override.
        let _ = regclass;
        1
    }
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

    let mut clobbered_insts: HashMap<PReg, Vec<usize>> = HashMap::new();
    for inst in 0..func.num_instructions() {
        for &preg in func.inst_clobbers(inst) {
            clobbered_insts.entry(preg).or_default().push(inst);
        }
    }

    let mut pinned_vars: HashSet<Var> = HashSet::new();
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
                // Slot indices and `next_spill_slot` are in *logical slots*
                // (the unit `spillslot_size` returns). A class needing N slots
                // is also aligned on N — so a width-2 vector slot never
                // straddles a width-1 int slot's pair-boundary.
                let size = func.spillslot_size(v.class).max(1);
                next_spill_slot = align_up(next_spill_slot, size);
                spilled_vars.insert(v, SpillSlot(next_spill_slot as u32));
                next_spill_slot += size;
            }
        }
    }
}

/// Immutable per-attempt analysis state.
///
/// All read-only inputs to the inner allocator (CFG/IR via `func`, the
/// liveness fixpoint, def/use maps, and the cached classification of pinned
/// and spilled vars) live here. Mutable allocation state (`active`,
/// `future_active`, `vreg_alloc`, coalesce groups) is intentionally kept
/// outside so methods on `Ctx` borrow `&self` while the caller mutates the
/// allocation state alongside.
///
/// Use-/def-list flat indices are `inst * 2 + 1`. The +1 keeps def and use of
/// the *same* instruction comparable as distinct half-steps, matching the
/// "before / at / after" ordering implied by the paper's algorithms.
struct Ctx<'a, F: AllocFunction> {
    func: &'a F,
    allocatable_by_class: HashMap<RegClass, Vec<PReg>>,
    pinned_vars: &'a HashSet<Var>,
    live_ins: Vec<HashSet<Var>>,
    live_outs: Vec<HashSet<Var>>,
    uses: HashMap<Var, Vec<usize>>,
    var_defs: HashMap<Var, Vec<usize>>,
    inst_block: Vec<usize>,
}

impl<'a, F: AllocFunction> Ctx<'a, F> {
    fn build(
        func: &'a F,
        allocatable_regs: &'a [PReg],
        pinned_vars: &'a HashSet<Var>,
    ) -> Self {
        let mut allocatable_by_class: HashMap<RegClass, Vec<PReg>> = HashMap::new();
        for &p in allocatable_regs {
            allocatable_by_class.entry(p.class).or_default().push(p);
        }
        let num_blocks = func.num_blocks();
        let num_insts = func.num_instructions();

        // ─── Liveness analysis (fixpoint over reverse block order) ───
        let mut live_ins: Vec<HashSet<Var>> = vec![HashSet::new(); num_blocks];
        let mut live_outs: Vec<HashSet<Var>> = vec![HashSet::new(); num_blocks];
        let mut changed = true;
        while changed {
            changed = false;
            for b in (0..num_blocks).rev() {
                let mut out_vars: HashSet<Var> = HashSet::new();
                for &succ in func.block_successors(b) {
                    for &v in &live_ins[succ] {
                        out_vars.insert(v);
                    }
                    for inst in func.block_instructions(succ) {
                        if func.is_phi(inst) {
                            out_vars.insert(func.phi_op(inst, b));
                        }
                    }
                }
                live_outs[b] = out_vars;

                let mut current = live_outs[b].clone();
                for inst in func.block_instructions(b).rev() {
                    for op in func.inst_operands(inst) {
                        match op.kind {
                            OperandKind::Def => { current.remove(&op.var); }
                            OperandKind::Use => { current.insert(op.var); }
                        }
                    }
                }

                if current != live_ins[b] {
                    live_ins[b] = current;
                    changed = true;
                }
            }
        }

        // ─── Definition map and inst→block map ───
        let mut var_defs: HashMap<Var, Vec<usize>> = HashMap::new();
        let mut inst_block = vec![0usize; num_insts];
        for b in 0..num_blocks {
            for inst in func.block_instructions(b) {
                inst_block[inst] = b;
                let inst_flat = inst * 2 + 1;
                for op in func.inst_operands(inst) {
                    if matches!(op.kind, OperandKind::Def) {
                        var_defs.entry(op.var).or_default().push(inst_flat);
                    }
                }
            }
        }

        // ─── Use map: explicit operand uses + phi operands at predecessor terminators ───
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
                        uses.entry(func.phi_op(inst, b)).or_default().push(flat);
                    }
                }
            }
        }
        for use_list in uses.values_mut() {
            use_list.sort_unstable();
        }

        Self {
            func,
            allocatable_by_class,
            pinned_vars,
            live_ins,
            live_outs,
            uses,
            var_defs,
            inst_block,
        }
    }

    fn num_blocks(&self) -> usize { self.func.num_blocks() }
    fn num_insts(&self) -> usize { self.func.num_instructions() }

    /// Is `v` live at instruction `inst`?
    fn is_live_at(&self, v: Var, inst: usize) -> bool {
        if inst >= self.inst_block.len() {
            return false;
        }
        let b = self.inst_block[inst];
        let range = self.func.block_instructions(b);

        if self.live_outs[b].contains(&v) {
            if let Some(defs) = self.var_defs.get(&v) {
                if let Some(&def_flat) = defs.iter().find(|&&df| self.inst_block[df / 2] == b) {
                    return inst >= def_flat / 2;
                }
            }
            return true;
        }

        let def_in_b = self.var_defs.get(&v).and_then(|defs| {
            defs.iter().copied().find(|&df| {
                let di = df / 2;
                di >= range.start && di < range.end
            })
        });
        if let Some(def_flat) = def_in_b {
            if inst < def_flat / 2 {
                return false;
            }
        } else if !self.live_ins[b].contains(&v) {
            return false;
        }

        if let Some(uses_list) = self.uses.get(&v) {
            let start_flat = inst * 2 + 1;
            let end_flat = (range.end - 1) * 2 + 1;
            return uses_list.iter().any(|&uf| uf >= start_flat && uf <= end_flat);
        }
        false
    }

    /// Paper Algorithm 5/6: are `lhs` and `rhs` simultaneously live at some
    /// program point?
    fn live_at_the_same_time(&self, lhs: Var, rhs: Var) -> bool {
        let empty: Vec<usize> = Vec::new();
        let lhs_defs = self.var_defs.get(&lhs).unwrap_or(&empty);
        let rhs_defs = self.var_defs.get(&rhs).unwrap_or(&empty);
        if lhs_defs.is_empty() || rhs_defs.is_empty() {
            return false;
        }

        for &def_l in lhs_defs {
            let b_l = self.inst_block[def_l / 2];
            for &def_r in rhs_defs {
                let b_r = self.inst_block[def_r / 2];
                if b_l == b_r
                    && self.live_at_the_same_time_same_block(lhs, rhs, b_l, def_l, def_r)
                {
                    return true;
                }
            }
        }
        for &def_l in lhs_defs {
            let b_l = self.inst_block[def_l / 2];
            if self.live_at_the_same_time_in_block(rhs, b_l, def_l) {
                return true;
            }
        }
        for &def_r in rhs_defs {
            let b_r = self.inst_block[def_r / 2];
            if self.live_at_the_same_time_in_block(lhs, b_r, def_r) {
                return true;
            }
        }
        false
    }

    fn live_at_the_same_time_same_block(
        &self,
        lhs: Var,
        rhs: Var,
        block: usize,
        lhs_def_flat: usize,
        rhs_def_flat: usize,
    ) -> bool {
        let lhs_live_out = self.live_outs[block].contains(&lhs);
        let rhs_live_out = self.live_outs[block].contains(&rhs);

        if lhs_live_out && rhs_live_out {
            true
        } else if !lhs_live_out && !rhs_live_out {
            let (first, last_def_flat) = if lhs_def_flat < rhs_def_flat {
                (lhs, rhs_def_flat)
            } else {
                (rhs, lhs_def_flat)
            };
            self.uses
                .get(&first)
                .map_or(false, |us| us.iter().any(|&u| u > last_def_flat))
        } else {
            let (non_live_out, non_live_out_def_flat, live_out_def_flat) = if lhs_live_out {
                (rhs, rhs_def_flat, lhs_def_flat)
            } else {
                (lhs, lhs_def_flat, rhs_def_flat)
            };
            // non_live_out lives [def_nlo, last_use_nlo]; live_out lives
            // [def_lo, ∞). If def_nlo >= def_lo then live_out is still in its
            // register when non_live_out is born — they conflict even if
            // non_live_out has no uses (a dead def still occupies the def
            // point). Otherwise, they overlap iff non_live_out has a use
            // past live_out's def.
            if non_live_out_def_flat >= live_out_def_flat {
                return true;
            }
            self.uses
                .get(&non_live_out)
                .map_or(false, |us| us.iter().any(|&u| u > live_out_def_flat))
        }
    }

    /// Is `other` live across `def_block`, where the reference variable is
    /// defined at `def_flat` (flat index) within `def_block`?
    fn live_at_the_same_time_in_block(
        &self,
        other: Var,
        def_block: usize,
        def_flat: usize,
    ) -> bool {
        if !self.live_ins[def_block].contains(&other) {
            return false;
        }
        if self.live_outs[def_block].contains(&other) {
            return true;
        }
        self.uses
            .get(&other)
            .map_or(false, |us| us.iter().any(|&u| u > def_flat))
    }

    /// Would putting `v` in `preg` conflict with the existing occupant `occ`?
    ///
    /// Carve-out: if `v` itself has a `Fixed(preg)` operand at the reservation
    /// site and has no use past it, the "conflict" is exactly `v` flowing
    /// through `preg` as planned by the constraint — not corruption — so it is
    /// not a conflict.
    fn occupant_conflicts_var(&self, v: Var, preg: PReg, occ: Occupant) -> bool {
        match occ {
            Occupant::Var(u) => {
                if u == v {
                    false
                } else {
                    self.live_at_the_same_time(v, u)
                }
            }
            Occupant::Clobber(inst) | Occupant::Fixed(inst) => {
                let v_fixed_here = self.func.inst_operands(inst).iter().any(|op| {
                    op.var == v && op.constraint == Constraint::Fixed(preg)
                });
                if v_fixed_here {
                    let inst_flat = inst * 2 + 1;
                    let v_dies_here = self.uses.get(&v).map_or(true, |ul| {
                        !ul.iter().any(|&uf| uf > inst_flat)
                    });
                    if v_dies_here {
                        return false;
                    }
                }
                self.is_live_at(v, inst)
            }
        }
    }

    /// Paper §3.4 invariant: would assigning `preg` to `v` violate any
    /// existing reservation in `active[preg] ∪ future_active[preg]`?
    fn preg_conflict(
        &self,
        v: Var,
        preg: PReg,
        active: &HashMap<PReg, Var>,
        future_active: &HashMap<PReg, HashSet<Occupant>>,
    ) -> bool {
        if let Some(&u) = active.get(&preg) {
            if u != v && self.live_at_the_same_time(v, u) {
                return true;
            }
        }
        if let Some(f_occs) = future_active.get(&preg) {
            for &occ in f_occs {
                if self.occupant_conflicts_var(v, preg, occ) {
                    return true;
                }
            }
        }
        false
    }

    /// Pick the spill candidate with the furthest next use (Belady-ish).
    fn select_spill_candidate(
        &self,
        failing_var: Var,
        active: &HashMap<PReg, Var>,
        current_inst_flat: usize,
    ) -> Var {
        let mut candidates: Vec<Var> = Vec::new();
        if !self.pinned_vars.contains(&failing_var) {
            candidates.push(failing_var);
        }
        for &v in active.values() {
            if !self.pinned_vars.contains(&v) {
                candidates.push(v);
            }
        }
        if candidates.is_empty() {
            // Everything pinned — caller will see this as forward progress
            // failing; bubble up the original failing var.
            return failing_var;
        }

        let mut best = candidates[0];
        let mut furthest_use = 0usize;
        for cand in candidates {
            let next_use = self.uses.get(&cand).map_or(usize::MAX, |ul| {
                ul.iter()
                    .copied()
                    .find(|&uf| uf >= current_inst_flat)
                    .unwrap_or(usize::MAX)
            });
            if next_use > furthest_use {
                furthest_use = next_use;
                best = cand;
            }
        }
        best
    }

    /// Place `v` into a register, updating `active`/`future_active`/`vreg_alloc`.
    /// On failure, returns the var the outer loop should spill.
    fn allocate_register(
        &self,
        v: Var,
        current_inst_flat: usize,
        active: &mut HashMap<PReg, Var>,
        future_active: &mut HashMap<PReg, HashSet<Occupant>>,
        vreg_alloc: &mut HashMap<Var, Allocation>,
        coalesce_groups: &mut CoalesceGroups,
        group_alloc: &mut HashMap<usize, PReg>,
    ) -> Result<(), VarToSpill> {
        // 0. Reuse existing assignment: a paused or previously-allocated var
        //    must come back into the same preg.
        if let Some(&Allocation::Reg(preg)) = vreg_alloc.get(&v) {
            if let Some(occs) = future_active.get_mut(&preg) {
                occs.remove(&Occupant::Var(v));
            }
            active.insert(preg, v);
            let rep = coalesce_groups.find(v.id as usize);
            group_alloc.insert(rep, preg);
            return Ok(());
        }

        // 1. Pull from future-active (paper Algorithm 4). Only `Var` occupants
        //    represent real variables to be allocated; `Clobber`/`Fixed`
        //    entries are point-reservations and stay put.
        let mut pulled = None;
        for (&preg, occs) in future_active.iter_mut() {
            if occs.remove(&Occupant::Var(v)) {
                pulled = Some(preg);
                break;
            }
        }
        if let Some(preg) = pulled {
            active.insert(preg, v);
            vreg_alloc.insert(v, Allocation::Reg(preg));
            let rep = coalesce_groups.find(v.id as usize);
            group_alloc.insert(rep, preg);
            return Ok(());
        }

        // 2. Prefer the coalesce group's register, when safe.
        let rep = coalesce_groups.find(v.id as usize);
        let class_pool = self
            .allocatable_by_class
            .get(&v.class)
            .map(|v| v.as_slice())
            .unwrap_or(&[]);
        if let Some(&pref_preg) = group_alloc.get(&rep) {
            if pref_preg.class == v.class
                && class_pool.contains(&pref_preg)
                && !self.preg_conflict(v, pref_preg, active, future_active)
            {
                active.insert(pref_preg, v);
                vreg_alloc.insert(v, Allocation::Reg(pref_preg));
                return Ok(());
            }
        }

        // 2b. Two-address hint (paper §3.7): when allocating a Def at an
        //     instruction, try the preg that holds the first Use operand of
        //     the same instruction. For an x86 binary op `add dst, lhs, rhs`,
        //     reusing lhs's preg as dst turns the lowering into the in-place
        //     `add dst, rhs` and elides one `mov`.
        //
        //     The current_inst_flat encoding is `inst*2 + 1` for in-block
        //     allocations and `block_start*2` for live-in starts, so an odd
        //     value means "we're allocating a Def at this instruction".
        if current_inst_flat & 1 == 1 {
            let inst = current_inst_flat / 2;
            for op in self.func.inst_operands(inst) {
                if op.kind == OperandKind::Use && op.var.class == v.class {
                    if let Some(Allocation::Reg(use_preg)) =
                        vreg_alloc.get(&op.var).copied()
                    {
                        if class_pool.contains(&use_preg)
                            && !self.preg_conflict(v, use_preg, active, future_active)
                        {
                            active.insert(use_preg, v);
                            vreg_alloc.insert(v, Allocation::Reg(use_preg));
                            group_alloc.insert(rep, use_preg);
                            return Ok(());
                        }
                    }
                    break; // Only the first class-matching Use is interesting.
                }
            }
        }

        // 3. First non-conflicting register of the matching class.
        let chosen = class_pool.iter().copied().find(|&preg| {
            !self.preg_conflict(v, preg, active, future_active)
        });

        if let Some(preg) = chosen {
            active.insert(preg, v);
            vreg_alloc.insert(v, Allocation::Reg(preg));
            group_alloc.insert(rep, preg);
            Ok(())
        } else {
            Err(VarToSpill(self.select_spill_candidate(v, active, current_inst_flat)))
        }
    }
}

fn allocate_attempt<F: AllocFunction>(
    func: &F,
    allocatable_regs: &[PReg],
    spilled_vars: &HashMap<Var, SpillSlot>,
    clobbered_insts: &HashMap<PReg, Vec<usize>>,
    pinned_vars: &HashSet<Var>,
) -> Result<AllocationAttemptResult, VarToSpill> {
    let ctx = Ctx::build(func, allocatable_regs, pinned_vars);
    let num_blocks = ctx.num_blocks();
    let num_insts = ctx.num_insts();

    // ─── Coalescing equivalence groups ───
    let mut coalesce_groups = CoalesceGroups::new(func.num_vregs());
    let mut group_alloc: HashMap<usize, PReg> = HashMap::new();
    for inst in 0..num_insts {
        if func.is_copy(inst) {
            let ops = func.inst_operands(inst);
            if ops.len() >= 2 && ops[0].var.class == ops[1].var.class {
                coalesce_groups.union(ops[0].var.id as usize, ops[1].var.id as usize);
            }
        } else if func.is_phi(inst) {
            let ops = func.inst_operands(inst);
            if let Some(first_op) = ops.first() {
                if matches!(first_op.kind, OperandKind::Def) {
                    let dst = first_op.var;
                    let b_dst = ctx.inst_block[inst];
                    for &pred_b in func.block_predecessors(b_dst) {
                        let src = func.phi_op(inst, pred_b);
                        if src.class == dst.class {
                            coalesce_groups.union(dst.id as usize, src.id as usize);
                        }
                    }
                }
            }
        }
    }

    // ─── Allocation core state ───
    let mut active: HashMap<PReg, Var> = HashMap::new();
    let mut future_active: HashMap<PReg, HashSet<Occupant>> = HashMap::new();
    let mut vreg_alloc: HashMap<Var, Allocation> = HashMap::new();

    for (&v, &slot) in spilled_vars {
        vreg_alloc.insert(v, Allocation::Stack(slot));
    }

    // Pre-allocate every clobber site as a point-reservation on its preg.
    for (&preg, clob_insts) in clobbered_insts {
        let set = future_active.entry(preg).or_default();
        for &ci in clob_insts {
            set.insert(Occupant::Clobber(ci));
        }
    }

    // Pre-allocate every fixed-operand site, and additionally try to enqueue
    // the variable itself (§3.5) so allocate_register can pull the register
    // assignment for free when the var's turn comes.
    for inst in 0..num_insts {
        let mut seen: HashSet<PReg> = HashSet::new();
        for op in func.inst_operands(inst) {
            if let Constraint::Fixed(preg) = op.constraint {
                if seen.insert(preg) {
                    future_active.entry(preg).or_default().insert(Occupant::Fixed(inst));
                }
            }
        }
    }
    for inst in 0..num_insts {
        for op in func.inst_operands(inst) {
            if let Constraint::Fixed(preg) = op.constraint {
                let v = op.var;
                if spilled_vars.contains_key(&v) {
                    continue;
                }
                if future_active
                    .get(&preg)
                    .map_or(false, |s| s.contains(&Occupant::Var(v)))
                {
                    continue;
                }
                let conflicts = future_active.get(&preg).map_or(false, |occs| {
                    occs.iter().any(|&occ| ctx.occupant_conflicts_var(v, preg, occ))
                });
                if conflicts {
                    continue;
                }
                future_active.entry(preg).or_default().insert(Occupant::Var(v));
            }
        }
    }

    let mut visited: HashSet<usize> = HashSet::new();

    for cur in 0..num_blocks {
        let live_ins_cur = &ctx.live_ins[cur];

        // Expire / pause active registers across the block boundary.
        let mut to_pause: Vec<(PReg, Var)> = Vec::new();
        let mut to_free: Vec<(PReg, Var)> = Vec::new();
        for (&preg, &v) in &active {
            if !live_ins_cur.contains(&v) {
                let is_live_in_future = (0..num_blocks)
                    .filter(|&b| !visited.contains(&b))
                    .any(|b| ctx.live_ins[b].contains(&v));
                if is_live_in_future {
                    to_pause.push((preg, v));
                } else {
                    to_free.push((preg, v));
                }
            }
        }
        for (preg, v) in to_pause {
            active.remove(&preg);
            future_active.entry(preg).or_default().insert(Occupant::Var(v));
        }
        for (preg, _) in to_free {
            active.remove(&preg);
        }

        // Start intervals for live-ins.
        for &v in live_ins_cur {
            if spilled_vars.contains_key(&v) {
                continue;
            }
            if active.values().any(|&x| x == v) {
                continue;
            }
            ctx.allocate_register(
                v,
                func.block_instructions(cur).start * 2,
                &mut active,
                &mut future_active,
                &mut vreg_alloc,
                &mut coalesce_groups,
                &mut group_alloc,
            )?;
        }

        for inst in func.block_instructions(cur) {
            let inst_flat = inst * 2 + 1;

            // Expire / pause inputs.
            for op in func.inst_operands(inst) {
                if matches!(op.kind, OperandKind::Use) {
                    let v = op.var;
                    if spilled_vars.contains_key(&v) {
                        continue;
                    }
                    if let Some(&preg) = active.iter().find(|&(_, &x)| x == v).map(|(r, _)| r) {
                        let has_uses_after = ctx.uses.get(&v).map_or(false, |ul| {
                            ul.iter().any(|&uf| uf >= inst_flat + 1)
                        });
                        let is_live_in_future = (0..num_blocks)
                            .filter(|&b| !visited.contains(&b))
                            .any(|b| ctx.live_ins[b].contains(&v));
                        if has_uses_after || is_live_in_future {
                            active.remove(&preg);
                            future_active.entry(preg).or_default().insert(Occupant::Var(v));
                        } else {
                            active.remove(&preg);
                        }
                    }
                }
            }

            // Allocate registers for defs.
            for op in func.inst_operands(inst) {
                if matches!(op.kind, OperandKind::Def) {
                    let v = op.var;
                    if spilled_vars.contains_key(&v) {
                        continue;
                    }
                    ctx.allocate_register(
                        v,
                        inst_flat,
                        &mut active,
                        &mut future_active,
                        &mut vreg_alloc,
                        &mut coalesce_groups,
                        &mut group_alloc,
                    )?;
                }
            }
        }

        visited.insert(cur);
    }

    // ─── Build output and insert spill/fill + fixup moves ───
    let mut inst_allocs: Vec<Vec<Allocation>> = vec![Vec::new(); num_insts];
    let mut edits_before: HashMap<usize, Vec<AllocMove>> = HashMap::new();
    let mut edits_after: HashMap<usize, Vec<AllocMove>> = HashMap::new();

    // Partition scratch regs by class so shuttling preserves operand class.
    let mut scratch_by_class: HashMap<RegClass, Vec<PReg>> = HashMap::new();
    for &p in func.scratch_regs() {
        scratch_by_class.entry(p.class).or_default().push(p);
    }
    // Take the next available scratch of `class`, or None if exhausted.
    let take_scratch = |used: &mut HashMap<RegClass, usize>, class: RegClass| -> Option<PReg> {
        let idx = used.entry(class).or_insert(0);
        let pool = scratch_by_class.get(&class)?;
        if *idx >= pool.len() {
            return None;
        }
        let preg = pool[*idx];
        *idx += 1;
        Some(preg)
    };

    for inst in 0..num_insts {
        let mut allocs: Vec<Allocation> = func
            .inst_operands(inst)
            .iter()
            .map(|op| {
                vreg_alloc
                    .get(&op.var)
                    .copied()
                    .unwrap_or(Allocation::Reg(PReg::new(0, op.var.class)))
            })
            .collect();

        let mut scratch_used: HashMap<RegClass, usize> = HashMap::new();

        // Pregs that a Fixed-use intends to occupy via a fixup move.
        let target_regs: HashSet<PReg> = func
            .inst_operands(inst)
            .iter()
            .enumerate()
            .filter_map(|(i, op)| match (op.kind, op.constraint) {
                (OperandKind::Use, Constraint::Fixed(preg))
                    if allocs[i] != Allocation::Reg(preg) =>
                {
                    Some(preg)
                }
                _ => None,
            })
            .collect();

        // Shuttle any use currently sitting in one of those target pregs
        // out to a scratch so the fixup move below doesn't clobber it.
        for (i, op) in func.inst_operands(inst).iter().enumerate() {
            if op.kind == OperandKind::Use {
                if let Allocation::Reg(curr_preg) = allocs[i] {
                    if target_regs.contains(&curr_preg) {
                        if let Some(scratch) = take_scratch(&mut scratch_used, op.var.class) {
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

        // Emit Fixed-constraint fixup moves.
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

        // Spill/fill: every stack-resident operand goes through a scratch
        // register so codegen always sees `inst_allocs[i] = Reg(_)`. A use
        // gets a Stack→Reg load before the instruction; a def gets a
        // Reg→Stack store after.
        //
        // Phi instructions are EXCLUDED: their Def is set up entirely by the
        // phi-resolution parallel copies emitted at predecessor tails.
        // Generating a scratch→stack store here would overwrite the correct
        // value that the predecessor already deposited in the slot.
        //
        // When the same spilled var appears multiple times in one
        // instruction, reuse the scratch — one load suffices for repeated
        // uses, and a use+def of the same var shares one slot↔reg pair.
        let mut spill_scratch: HashMap<SpillSlot, PReg> = HashMap::new();
        if !func.is_phi(inst) {
        for (i, op) in func.inst_operands(inst).iter().enumerate() {
            let Allocation::Stack(slot) = allocs[i] else { continue };
            let scratch = if let Some(&existing) = spill_scratch.get(&slot) {
                existing
            } else {
                let Some(s) = take_scratch(&mut scratch_used, op.var.class) else {
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
