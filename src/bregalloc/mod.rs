//! Braun-style preference-guided SSA register allocator.
//!
//! Decoupled from `xregalloc` — its own types, trait, and pipeline. Two
//! phases the paper describes are split here:
//!   - `spill`: Belady-style pressure-reducing pass. Lowers register
//!     pressure to ≤ k by selecting variables to live in memory.
//!   - `assignment`: dominance-order coloring with preference vectors
//!     and affinity chunks, per Braun, Mallon, Hack 2010.
//!
//! Graph operations (CFG traversal, reverse-postorder, dominators) are
//! delegated to `petgraph` rather than hand-rolled.

use std::collections::HashMap;

pub mod affinity;
pub mod assignment;
pub mod block_order;
pub mod cfg;
pub mod forbidden;
pub mod liveness;
pub mod output;
pub mod preference;
pub mod spill;

/// Register file partition. Allocation is partitioned by class: a value of
/// one class can never be considered for a physical register of another.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum RegClass {
    Int = 0,
    Float = 1,
    Vector = 2,
}

/// SSA virtual register.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct Var {
    pub class: RegClass,
    pub id: u32,
}

impl Var {
    pub const fn new(id: u32, class: RegClass) -> Self { Self { class, id } }
    pub const fn int(id: u32) -> Self { Self::new(id, RegClass::Int) }
}

/// Physical register.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct PReg {
    pub class: RegClass,
    pub index: u8,
}

impl PReg {
    pub const fn new(index: u8, class: RegClass) -> Self { Self { class, index } }
    pub const fn int(index: u8) -> Self { Self::new(index, RegClass::Int) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct SpillSlot(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Allocation {
    Reg(PReg),
    Stack(SpillSlot),
}

impl Allocation {
    pub fn is_stack(&self) -> bool {
        matches!(self, Allocation::Stack(_))
    }
    pub fn is_reg(&self) -> bool {
        matches!(self, Allocation::Reg(_))
    }
}

/// Constraint on an operand's allocation. `Any` accepts a register or a
/// spill slot; `Reg` rejects spill slots; `Fixed` pins the operand to a
/// specific physical register.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Constraint {
    Any,
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

/// Function interface for bregalloc. Mirrors xregalloc's `AllocFunction` in
/// shape so consumers can dual-implement, but the types in this signature
/// are bregalloc's own — no symbol from xregalloc appears.
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
    fn is_const(&self, _inst: usize) -> bool { false }

    /// Loop nesting depth of `block`. Used by the future assignment pass
    /// to estimate execution frequencies for preference weighting. The
    /// default implementation returns 0 — the allocator still works, it
    /// just loses the loop-aware weighting.
    fn loop_depth(&self, _block: usize) -> u32 { 0 }
}

/// Top-level entry point: full Braun-style preference-guided allocation.
///
/// Pipeline:
///   1. Liveness analysis.
///   2. CFG (petgraph) + dominators + loop depths → frequencies.
///   3. Belady-style spill pass to lower pressure to ≤ k per class.
///   4. Forbidden-preg analysis (clobbers + foreign Fixed crossings).
///   5. Preference analysis (paper §3.1–3.2), weighted by frequencies.
///   6. Affinity-chunk union-find (paper §3.3) over copies and phis.
///   7. Trace-based block coloring order (paper §4).
///   8. Assignment (paper Algorithm 1 with preference-sorted get_register
///      and affinity-broadcast updates).
///   9. Output building: per-instruction operand allocations + spill-fill
///      and Fixed-constraint fixup edits + phi-resolution moves at pred
///      terminators.
pub fn allocate<F: AllocFunction>(
    func: &F,
    allocatable_regs: &[PReg],
) -> Result<AllocationResult, AllocError> {
    use std::collections::HashMap;

    // 1–2. Liveness, CFG, frequencies.
    let liveness = liveness::Liveness::compute(func);
    let cfg = cfg::Cfg::build(func);
    let freqs = block_order::freqs_from_loop_depth(func);

    // Partition allocatable pregs by class. Scratches are removed from the
    // allocatable pool so they remain available as transients.
    let scratch_set: std::collections::HashSet<PReg> =
        func.scratch_regs().iter().copied().collect();
    let filtered_allocatable: Vec<PReg> = allocatable_regs
        .iter()
        .copied()
        .filter(|p| !scratch_set.contains(p))
        .collect();
    let mut allocatable_by_class: HashMap<RegClass, Vec<PReg>> = HashMap::new();
    for &p in &filtered_allocatable {
        allocatable_by_class.entry(p.class).or_default().push(p);
    }

    // 3. Spill pass.
    let mut spill_result = spill::run(func, &filtered_allocatable);

    // 4. Forbidden pregs.
    let forbidden_set = forbidden::Forbidden::compute(func, &liveness);

    // 5. Preferences.
    let mut prefs = preference::Preferences::compute(func, &liveness, &freqs);

    // 6. Affinity chunks + constraint propagation (paper §3.3).
    let mut affinity = affinity::AffinityChunks::build(func);
    let all_vars = assignment::collect_all_vars(func);
    affinity.propagate_constraints(func, &mut prefs, &freqs, &all_vars);

    // 7. Block coloring order.
    let order = block_order::trace_order(func, &cfg, &freqs);

    // 8. Assignment with spill-and-retry loop.
    let mut next_slot = spill_result.num_spillslots;
    let mut attempts = 0;
    let assigned = loop {
        attempts += 1;
        if attempts > 1000 {
            return Err(AllocError::AllocationFailed("Too many spill retries".to_string()));
        }
        let mut prefs_clone = prefs.clone();
        let mut affinity_clone = affinity.clone();
        let assign_input = assignment::AssignInput {
            func,
            liveness: &liveness,
            spill: &spill_result,
            block_order: &order,
            freqs: &freqs,
            allocatable_by_class: &allocatable_by_class,
        };
        match assignment::run(
            assign_input,
            &mut prefs_clone,
            &mut affinity_clone,
            &forbidden_set,
            &all_vars,
        ) {
            Ok(assigned) => break assigned,
            Err(v) => {
                if spill_result.spilled_vars.insert(v) {
                    spill_result.spill_slots.insert(v, SpillSlot(next_slot as u32));
                    next_slot += 1;
                    spill_result.num_spillslots = next_slot;
                }
            }
        }
    };

    // 9. Output build (inst_allocs, edits, phi resolution).
    let result = output::build(output::OutputInput {
        func,
        vreg_alloc: &assigned.vreg_alloc,
        num_spillslots: spill_result.num_spillslots,
    });

    Ok(result)
}
