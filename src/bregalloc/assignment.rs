//! Assignment pass — Algorithm 1 of the paper, augmented with preferences,
//! affinity propagation, and optimistic move insertion (Algorithm 3).
//!
//! Pipeline:
//!   1. Walk blocks in `block_order` (trace-based, dominance-respecting).
//!   2. For each block b:
//!        - Initialize `occupied` from live-in vars (already colored in a
//!          dominator).
//!        - Color φ-defs at b's entry, biased by affinity preferences.
//!        - Walk instructions: free dying uses, then for each Def call
//!          `get_register`. Spilled vars are skipped — they don't compete
//!          for registers (downstream output-builder loads them through a
//!          scratch).
//!   3. After all blocks are colored, phi-resolution emits parallel-copy
//!      moves in pred-block tails (handled in `phi_resolve`).

use std::collections::{HashMap, HashSet};

/// Affinity broadcast weight: how many times `block_freq` each preference
/// boost is worth. Higher values make coalescing more aggressive.
const AFFINITY_BOOST_FACTOR: i32 = 4;

use super::affinity::AffinityChunks;
use super::forbidden::Forbidden;
use super::liveness::Liveness;
use super::preference::Preferences;
use super::spill::SpillResult;
use super::{AllocFunction, Allocation, OperandKind, PReg, RegClass, Var};

pub struct Assignment {
    pub vreg_alloc: HashMap<Var, Allocation>,
}

pub struct AssignInput<'a, F: AllocFunction> {
    pub func: &'a F,
    pub liveness: &'a Liveness,
    pub spill: &'a SpillResult,
    pub block_order: &'a [usize],
    pub freqs: &'a [u32],
    pub allocatable_by_class: &'a HashMap<RegClass, Vec<PReg>>,
}

pub fn run<F: AllocFunction>(
    input: AssignInput<F>,
    prefs: &mut Preferences,
    affinity: &mut AffinityChunks,
    forbidden: &Forbidden,
    all_vars: &[Var],
) -> Result<Assignment, Var> {
    let mut vreg_alloc: HashMap<Var, Allocation> = HashMap::new();

    // Spilled vars get their stack-slot home up front.
    for (&v, &slot) in &input.spill.spill_slots {
        vreg_alloc.insert(v, Allocation::Stack(slot));
    }

    // Pre-compute chunk members index for O(chunk_size) broadcast.
    let chunk_index = affinity.chunk_members_index(all_vars);

    // Pre-compute next-use lists for the dies-here test inside blocks.
    let next_use = compute_next_use(input.func);

    for &b in input.block_order {
        let mut occupied: HashMap<PReg, Var> = HashMap::new();
        let block_freq = input.freqs.get(b).copied().unwrap_or(1);

        // Initialize occupied from live-in vars that already have a Reg
        // allocation (set by dominators we've already processed).
        for &v in &input.liveness.live_in[b] {
            if let Some(&Allocation::Reg(p)) = vreg_alloc.get(&v) {
                occupied.insert(p, v);
            }
        }

        // Color phis at block entry. Phi.operand assignments are NOT
        // known yet (operands belong to predecessors). We only color the
        // phi.dst here; the predecessor moves get emitted post-pass.
        for inst in input.func.block_instructions(b) {
            if !input.func.is_phi(inst) {
                continue;
            }
            let dst = match input.func.inst_operands(inst).first() {
                Some(op) if op.kind == OperandKind::Def => op.var,
                _ => continue,
            };
            if input.spill.spilled_vars.contains(&dst) {
                continue; // spilled — no preg assignment
            }
            if vreg_alloc.contains_key(&dst) {
                continue; // already colored (shouldn't happen normally)
            }
            let chosen = get_register(
                dst,
                &mut occupied,
                prefs,
                forbidden,
                input.allocatable_by_class,
                &mut vreg_alloc,
                block_freq,
            );
            if let Some(p) = chosen {
                vreg_alloc.insert(dst, Allocation::Reg(p));
                occupied.insert(p, dst);
                broadcast_chunk_preference(
                    dst, p, affinity, prefs, &chunk_index, &vreg_alloc, block_freq,
                    input.liveness, b,
                );

                let inst_flat = inst * 2 + 1;
                let has_later_use = input.liveness.live_out[b].contains(&dst) || next_use
                    .get(&dst)
                    .map_or(false, |ul| ul.iter().any(|&u| u > inst_flat));
                if !has_later_use {
                    occupied.remove(&p);
                }
            } else {
                return Err(dst);
            }
        }

        // Walk non-phi instructions in order.
        for inst in input.func.block_instructions(b) {
            if input.func.is_phi(inst) {
                continue;
            }

            // (a) Free dying uses (uses that have no later use in this
            //     instruction's flat-index range or beyond).
            let inst_flat = inst * 2 + 1;
            for op in input.func.inst_operands(inst) {
                if op.kind == OperandKind::Use {
                    let v = op.var;
                    if input.spill.spilled_vars.contains(&v) {
                        continue;
                    }
                    let dies_here = !input.liveness.live_out[b].contains(&v) && next_use
                        .get(&v)
                        .map_or(true, |ul| !ul.iter().any(|&u| u > inst_flat));
                    if dies_here {
                        if let Some(&Allocation::Reg(p)) = vreg_alloc.get(&v) {
                            if occupied.get(&p) == Some(&v) {
                                occupied.remove(&p);
                            }
                        }
                    }
                }
            }

            // (b) For each Def, allocate a register.
            for op in input.func.inst_operands(inst) {
                if op.kind == OperandKind::Def {
                    let v = op.var;
                    if input.spill.spilled_vars.contains(&v) {
                        continue;
                    }
                    if vreg_alloc.contains_key(&v) {
                        continue;
                    }
                    let chosen = get_register(
                        v,
                        &mut occupied,
                        prefs,
                        forbidden,
                        input.allocatable_by_class,
                        &mut vreg_alloc,
                        block_freq,
                    );
                    if let Some(p) = chosen {
                        vreg_alloc.insert(v, Allocation::Reg(p));
                        occupied.insert(p, v);
                        broadcast_chunk_preference(
                            v, p, affinity, prefs, &chunk_index, &vreg_alloc, block_freq,
                            input.liveness, b,
                        );

                        let has_later_use = input.liveness.live_out[b].contains(&v) || next_use
                            .get(&v)
                            .map_or(false, |ul| ul.iter().any(|&u| u > inst_flat));
                        if !has_later_use {
                            occupied.remove(&p);
                        }
                    } else {
                        return Err(v);
                    }
                }
            }
        }
    }

    Ok(Assignment { vreg_alloc })
}

/// Choose a physical register for `v` — Algorithm 2 + Algorithm 3 from the
/// paper, extended with:
///   - (#3) Alg 3 preemption: before settling for a free but lower-ranked
///     preg, evaluate whether displacing the occupant of the top-ranked preg
///     is cost-effective.
///   - (#6) Two-level chain: if ovar has no free target, try evicting ovar to
///     a preg whose occupant (wvar) itself has a free escape route.
fn get_register(
    v: Var,
    occupied: &mut HashMap<PReg, Var>,
    prefs: &Preferences,
    forbidden: &Forbidden,
    allocatable_by_class: &HashMap<RegClass, Vec<PReg>>,
    vreg_alloc: &mut HashMap<Var, Allocation>,
    block_freq: u32,
) -> Option<PReg> {
    let pool = allocatable_by_class
        .get(&v.class)
        .map(|x| x.as_slice())
        .unwrap_or(&[]);
    let ranked = prefs.sorted_pregs(v, pool);

    // Scan once: find the top non-forbidden preg (occupied or free) and the
    // best free non-forbidden preg.
    let mut top: Option<PReg> = None;
    let mut first_free: Option<PReg> = None;
    for &p in &ranked {
        if forbidden.is_forbidden(v, p) { continue; }
        if top.is_none() { top = Some(p); }
        if first_free.is_none() && !occupied.contains_key(&p) { first_free = Some(p); }
    }

    let desired = match top {
        None => return first_free, // entire pool forbidden
        Some(d) => d,
    };

    // If the top-ranked preg is already free, take it immediately (Alg 2).
    if !occupied.contains_key(&desired) {
        return Some(desired);
    }

    // `desired` is occupied. Compute how much v gains by landing there vs
    // settling for `first_free` (or nothing if everything is occupied).
    let v_desired_pref = prefs.score(v, desired);
    let v_fallback_pref = first_free.map_or(0, |p| prefs.score(v, p));
    let v_gain = v_desired_pref - v_fallback_pref;

    let ovar = *occupied.get(&desired).unwrap();
    let ovar_ranked = prefs.sorted_pregs(ovar, pool);
    let ovar_current_pref = prefs.score(ovar, desired);

    // ── Single-level Alg 3 ────────────────────────────────────────────────
    if let Some(oreg) = ovar_ranked.iter().copied().find(|&op| {
        op != desired && !occupied.contains_key(&op) && !forbidden.is_forbidden(ovar, op)
    }) {
        let ovar_loss = ovar_current_pref - prefs.score(ovar, oreg);
        if v_gain - ovar_loss > block_freq as i32 {
            vreg_alloc.insert(ovar, Allocation::Reg(oreg));
            occupied.remove(&desired);
            occupied.insert(oreg, ovar);
            return Some(desired);
        }
    }

    // ── Two-level chain (#6) ─────────────────────────────────────────────
    // ovar can't find a free slot; try: ovar → oreg (occupied by wvar),
    // wvar → wreg (free). If both displacements are net-profitable, do chain.
    'chain: for &oreg in &ovar_ranked {
        if oreg == desired || forbidden.is_forbidden(ovar, oreg) { continue; }
        let &wvar = match occupied.get(&oreg) {
            Some(w) => w,
            None => continue, // free — would have been taken by single-level above
        };
        let wvar_ranked = prefs.sorted_pregs(wvar, pool);
        let wreg = wvar_ranked.iter().copied().find(|&wp| {
            wp != oreg && wp != desired
                && !occupied.contains_key(&wp)
                && !forbidden.is_forbidden(wvar, wp)
        });
        let Some(wreg) = wreg else { continue 'chain; };

        let ovar_loss = ovar_current_pref - prefs.score(ovar, oreg);
        let wvar_loss = prefs.score(wvar, oreg) - prefs.score(wvar, wreg);
        if v_gain - ovar_loss - wvar_loss > block_freq as i32 {
            vreg_alloc.insert(wvar, Allocation::Reg(wreg));
            occupied.remove(&oreg);
            occupied.insert(wreg, wvar);
            vreg_alloc.insert(ovar, Allocation::Reg(oreg));
            occupied.remove(&desired);
            occupied.insert(oreg, ovar);
            return Some(desired);
        }
    }

    // Optimistic displacement wasn't worth it. Fall back to first_free.
    if let Some(p) = first_free {
        return Some(p);
    }

    // Everything occupied. Try single-level Alg 3 for lower-ranked pregs.
    for &p in ranked.iter().skip(1) {
        if forbidden.is_forbidden(v, p) { continue; }
        let &ovar2 = match occupied.get(&p) {
            Some(o) => o,
            None => return Some(p), // shouldn't happen but take it
        };
        let ovar2_ranked = prefs.sorted_pregs(ovar2, pool);
        let ovar2_cur = prefs.score(ovar2, p);
        let Some(oreg2) = ovar2_ranked.iter().copied().find(|&op| {
            op != p && !occupied.contains_key(&op) && !forbidden.is_forbidden(ovar2, op)
        }) else { continue; };
        let loss2 = ovar2_cur - prefs.score(ovar2, oreg2);
        let gain2 = prefs.score(v, p); // no free fallback, baseline 0
        if gain2 - loss2 > block_freq as i32 {
            vreg_alloc.insert(ovar2, Allocation::Reg(oreg2));
            occupied.remove(&p);
            occupied.insert(oreg2, ovar2);
            return Some(p);
        }
    }

    None
}

/// After coloring `v` to `p`, broadcast a preference for `p` to all
/// uncolored chunk members that don't interfere with `v` (paper §3.3).
///
/// Interference check: in SSA, `w` interferes with `v` if `w` is live at
/// `v`'s definition point — i.e. `w ∈ live_in[block]`. Already-colored
/// vars implicitly handled: they're skipped by the `vreg_alloc` check.
fn broadcast_chunk_preference(
    v: Var,
    p: PReg,
    affinity: &mut AffinityChunks,
    prefs: &mut Preferences,
    chunk_index: &HashMap<u32, Vec<Var>>,
    vreg_alloc: &HashMap<Var, Allocation>,
    block_freq: u32,
    liveness: &super::liveness::Liveness,
    block: usize,
) {
    let root = affinity.find(v);
    if let Some(members) = chunk_index.get(&root) {
        for &w in members {
            if w == v { continue; }
            if vreg_alloc.contains_key(&w) { continue; }
            if w.class != p.class { continue; }
            // Skip vars live at v's def point — they interfere and can't
            // share p anyway; boosting them just pollutes their pref vectors.
            if liveness.live_in[block].contains(&w) { continue; }
            prefs.boost(w, p, block_freq as i32 * AFFINITY_BOOST_FACTOR);
        }
    }
}

pub fn collect_all_vars<F: AllocFunction>(func: &F) -> Vec<Var> {
    let mut seen: HashSet<Var> = HashSet::new();
    let mut out = Vec::new();
    for inst in 0..func.num_instructions() {
        for op in func.inst_operands(inst) {
            if seen.insert(op.var) {
                out.push(op.var);
            }
        }
    }
    out
}

fn compute_next_use<F: AllocFunction>(func: &F) -> HashMap<Var, Vec<usize>> {
    let mut uses: HashMap<Var, Vec<usize>> = HashMap::new();
    for inst in 0..func.num_instructions() {
        let flat = inst * 2 + 1;
        for op in func.inst_operands(inst) {
            if op.kind == OperandKind::Use {
                uses.entry(op.var).or_default().push(flat);
            }
        }
    }
    for b in 0..func.num_blocks() {
        let term = func.block_instructions(b).end.saturating_sub(1);
        let flat = term * 2 + 1;
        for &succ in func.block_successors(b) {
            for inst in func.block_instructions(succ) {
                if func.is_phi(inst) {
                    uses.entry(func.phi_op(inst, b)).or_default().push(flat);
                }
            }
        }
    }
    for ul in uses.values_mut() {
        ul.sort_unstable();
    }
    uses
}
