//! Belady-style spill pass.
//!
//! Lowers register pressure to ≤ k per `RegClass` everywhere in the program
//! by selecting variables to live in memory. The decision granularity is
//! per-variable: once a var is in `spilled_vars`, it carries a `SpillSlot`
//! and the downstream assignment pass treats every use of it as a
//! load-from-slot-into-scratch-register pattern.
//!
//! ## Pressure model
//!
//! At each instruction `I`, the pressure for a given `RegClass` is
//!   |non-spilled vars live just before I| + |non-spilled defs at I|
//!
//! Spilled-var uses do *not* count: they borrow a scratch register only
//! for the duration of `I` and never compete for the k-pool. This matches
//! how the downstream assignment pass treats reloads.
//!
//! ## Eviction policy
//!
//! When pressure exceeds capacity at some `I`, we spill non-pinned
//! candidates (vars live at `I` or defined at `I`) until pressure fits.
//! Among candidates we pick the one with the **furthest next use** —
//! standard Belady MIN. Pinned vars (those with any `Fixed`/`Reg`
//! constraint anywhere in the program) are never selected; if all
//! candidates of a class are pinned, the pass leaves pressure
//! unsatisfied at that point and the assignment pass will fail. That
//! is a real precondition violation by the input program, not a bug in
//! the pass.
//!
//! ## Limitations of this implementation
//!
//! This is per-block Belady without W_in / W_out reconciliation across
//! block edges. It guarantees `pressure ≤ k` globally because spill
//! decisions are global (spilling a var spills it everywhere), but it
//! does not track which spilled vars happen to still be in registers
//! at block boundaries — the assignment pass treats every use as
//! reloaded. A full Braun & Hack '09 implementation would maintain
//! per-edge `W` sets and emit edge-local reload instructions in
//! predecessors, recovering register residency across blocks. Worth
//! adding once basic functionality is verified.

use std::collections::{HashMap, HashSet};

use super::cfg::Cfg;
use super::liveness::Liveness;
use super::uses;
use super::{AllocFunction, Constraint, OperandKind, PReg, RegClass, SpillSlot, Var};

pub struct SpillResult {
    /// Vars chosen to spill. Each has a permanent home in `spill_slots`.
    pub spilled_vars: HashSet<Var>,
    /// Stack slot assigned to each spilled var.
    pub spill_slots: HashMap<Var, SpillSlot>,
    /// Total number of slots used. Slots are allocated densely from 0.
    pub num_spillslots: usize,
}

pub fn run<F: AllocFunction>(
    func: &F,
    allocatable_regs: &[PReg],
    liveness: &Liveness,
    cfg: &Cfg,
) -> SpillResult {
    let pinned_vars = compute_pinned_vars(func);
    let next_use = uses::compute_next_use(func);
    let capacity = capacity_per_class(allocatable_regs);

    let mut spilled: HashSet<Var> = HashSet::new();
    let mut spill_slots: HashMap<Var, SpillSlot> = HashMap::new();
    let mut next_slot: u32 = 0;

    for b in cfg.reverse_postorder() {
        // `live_now`: non-spilled vars currently live (just before next inst).
        let mut live_now: HashSet<Var> = liveness.live_in[b]
            .iter()
            .copied()
            .filter(|v| !spilled.contains(v))
            .collect();

        for inst in func.block_instructions(b) {
            // ── Pressure check at this instruction ───────────────────────
            // pressure(class) = |non-spilled live_before of class|
            //                 + |non-spilled defs of class at inst|
            // Spill until each class's pressure ≤ its capacity.
            let mut defs_by_class: HashMap<RegClass, Vec<Var>> = HashMap::new();
            for op in func.inst_operands(inst) {
                if op.kind == OperandKind::Def && !spilled.contains(&op.var) {
                    defs_by_class.entry(op.var.class).or_default().push(op.var);
                }
            }
            let mut live_by_class: HashMap<RegClass, Vec<Var>> = HashMap::new();
            for &v in &live_now {
                live_by_class.entry(v.class).or_default().push(v);
            }

            // Iterate every class that has any presence here.
            let mut classes: Vec<RegClass> = defs_by_class.keys().copied().collect();
            for c in live_by_class.keys() {
                if !classes.contains(c) { classes.push(*c); }
            }
            for class in classes {
                let cap = capacity.get(&class).copied().unwrap_or(0);
                let clobbers = func.inst_clobbers(inst).iter().filter(|p| p.class == class).count();
                let live_across_cap = cap.saturating_sub(clobbers);
                loop {
                    let live_list = live_by_class.get(&class).cloned().unwrap_or_default();
                    let defs_list = defs_by_class.get(&class).cloned().unwrap_or_default();
                    let live = live_list.len();
                    let defs = defs_list.len();

                    let live_across: Vec<Var> = live_list
                        .iter()
                        .copied()
                        .filter(|&v| uses::has_use_after(&next_use, v, inst))
                        .collect();

                    if live + defs <= cap && live_across.len() <= live_across_cap {
                        break;
                    }

                    // Pick a non-pinned victim with the furthest next use.
                    // If the live_across constraint is violated, candidates must be from live_across.
                    // Otherwise, candidates are any live or def var.
                    let candidates = if live_across.len() > live_across_cap {
                        live_across
                    } else {
                        let mut c = live_list;
                        c.extend(defs_list);
                        c
                    };

                    let victim = pick_belady_victim(
                        &candidates,
                        inst,
                        &next_use,
                        &pinned_vars,
                    );
                    let Some(v) = victim else {
                        // Nothing spillable — over-pressure carries to the
                        // assignment pass, which will fail loudly.
                        break;
                    };
                    spill_var(v, &mut spilled, &mut spill_slots, &mut next_slot);

                    // Remove from the working sets so the loop re-measures.
                    if let Some(list) = live_by_class.get_mut(&class) {
                        list.retain(|x| *x != v);
                    }
                    if let Some(list) = defs_by_class.get_mut(&class) {
                        list.retain(|x| *x != v);
                    }
                    // Also remove from live_now so subsequent inst's
                    // pressure already reflects the decision.
                    live_now.remove(&v);
                }
            }

            // ── Advance live_now past this instruction ───────────────────
            // A use dies here if it has no use ≥ inst+1.
            for op in func.inst_operands(inst) {
                if op.kind == OperandKind::Use {
                    let v = op.var;
                    if spilled.contains(&v) { continue; }
                    if !uses::has_use_after(&next_use, v, inst) {
                        live_now.remove(&v);
                    }
                }
            }
            // Defs join live_now (unless spilled, in which case the value
            // lives in its slot only).
            for op in func.inst_operands(inst) {
                if op.kind == OperandKind::Def && !spilled.contains(&op.var) {
                    live_now.insert(op.var);
                }
            }
        }
    }

    SpillResult {
        spilled_vars: spilled,
        spill_slots,
        num_spillslots: next_slot as usize,
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────

fn spill_var(
    v: Var,
    spilled: &mut HashSet<Var>,
    spill_slots: &mut HashMap<Var, SpillSlot>,
    next_slot: &mut u32,
) {
    if spilled.insert(v) {
        spill_slots.insert(v, SpillSlot(*next_slot));
        *next_slot += 1;
    }
}

/// Vars that carry any `Fixed` or `Reg` constraint anywhere — these cannot
/// be spilled because their use requires them to be in a specific register.
fn compute_pinned_vars<F: AllocFunction>(func: &F) -> HashSet<Var> {
    let mut pinned = HashSet::new();
    for inst in 0..func.num_instructions() {
        for op in func.inst_operands(inst) {
            if matches!(op.constraint, Constraint::Fixed(_) | Constraint::Reg) {
                pinned.insert(op.var);
            }
        }
    }
    pinned
}

/// First use of `v` at or after instruction `from_inst`, in flat-index
/// space. `usize::MAX` if none.
fn next_use_at(next_use: &HashMap<Var, Vec<usize>>, v: Var, from_inst: usize) -> usize {
    let from_flat = from_inst * 2 + 1;
    next_use
        .get(&v)
        .and_then(|ul| ul.iter().copied().find(|&u| u >= from_flat))
        .unwrap_or(usize::MAX)
}

/// Pick a non-pinned candidate with the furthest next use. Candidates are
/// the union of live-at-inst vars and defs-at-inst of the chosen class.
fn pick_belady_victim(
    candidates: &[Var],
    inst: usize,
    next_use: &HashMap<Var, Vec<usize>>,
    pinned: &HashSet<Var>,
) -> Option<Var> {
    let mut best: Option<(Var, usize)> = None;
    for &v in candidates {
        if pinned.contains(&v) { continue; }
        let nu = next_use_at(next_use, v, inst + 1);
        match best {
            Some((_, prev_nu)) if prev_nu >= nu => {}
            _ => best = Some((v, nu)),
        }
    }
    if best.is_some() {
        return best.map(|(v, _)| v);
    }
    // Fallback: if all candidates are pinned, pick the pinned one with the furthest next use.
    for &v in candidates {
        let nu = next_use_at(next_use, v, inst + 1);
        match best {
            Some((_, prev_nu)) if prev_nu >= nu => {}
            _ => best = Some((v, nu)),
        }
    }
    best.map(|(v, _)| v)
}

fn capacity_per_class(allocatable: &[PReg]) -> HashMap<RegClass, usize> {
    let mut cap: HashMap<RegClass, usize> = HashMap::new();
    for p in allocatable {
        *cap.entry(p.class).or_insert(0) += 1;
    }
    cap
}
