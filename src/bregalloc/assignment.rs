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
/// paper.
///
/// **Algorithm 2**: Walk preference-sorted pregs; pick the first that is free
/// and not forbidden.
///
/// **Fallback**: If no preferred preg qualifies, try *any* allocatable preg
/// of the right class that is free and not forbidden.
///
/// **Algorithm 3 (optimistic move insertion)**: If the most-preferred preg
/// is occupied but the occupying variable could be moved aside cheaply
/// (i.e. the preference gain exceeds the block's execution frequency),
/// relocate the occupier and take the preferred preg.
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

    // ── Algorithm 2: first free, non-forbidden preg by preference ──────
    for &p in &ranked {
        if occupied.contains_key(&p) {
            continue;
        }
        if forbidden.is_forbidden(v, p) {
            continue;
        }
        return Some(p);
    }

    // ── Fallback: any free preg not in ranked (score 0, unranked) ──────
    // `ranked` already contains all pool entries, so we scan pool again
    // just to be sure none was missed by the forbidden check pattern.
    // (ranked == pool sorted by pref — they have the same elements.)
    // If we got here, every preg is either occupied or forbidden.
    // Proceed to Algorithm 3.

    // ── Algorithm 3: optimistic move insertion ─────────────────────────
    // Try to relocate the occupying variable of the top-preferred preg.
    //
    // Paper pseudocode:
    //   ovar ← reg.current_variable
    //   find ovar's next-best free preg → oreg
    //   other_win ← opref − oreg.current_pref
    //   next_pref ← preference for next free register for v
    //   win ← next_pref − pref
    //   if win + other_win > block.execfreq: relocate, return reg
    for &desired in &ranked {
        if forbidden.is_forbidden(v, desired) {
            continue;
        }
        let Some(&ovar) = occupied.get(&desired) else {
            // Free! Shouldn't happen (we checked above), but take it.
            return Some(desired);
        };

        // Find ovar's next-best free, non-forbidden preg.
        let ovar_ranked = prefs.sorted_pregs(ovar, pool);
        let ovar_current_pref = prefs.score(ovar, desired);
        let relocate_target = ovar_ranked.iter().find(|&&op| {
            op != desired && !occupied.contains_key(&op) && !forbidden.is_forbidden(ovar, op)
        });
        let Some(&oreg) = relocate_target else {
            continue; // ovar has nowhere to go
        };
        let oreg_pref = prefs.score(ovar, oreg);
        let ovar_loss = ovar_current_pref - oreg_pref; // cost of displacing ovar (positive)

        // v's preference difference: what v gets (desired) vs next-best free.
        let v_desired_pref = prefs.score(v, desired);
        let v_next = ranked.iter().find(|&&p| {
            p != desired && !occupied.contains_key(&p) && !forbidden.is_forbidden(v, p)
        });
        let v_next_pref = v_next.map_or(0, |&p| prefs.score(v, p));
        let v_gain = v_desired_pref - v_next_pref; // benefit of picking desired (positive)

        // Paper criterion: move is worthwhile if net gain > block frequency.
        if v_gain - ovar_loss > block_freq as i32 {
            // Relocate ovar from `desired` to `oreg`.
            vreg_alloc.insert(ovar, Allocation::Reg(oreg));
            occupied.remove(&desired);
            occupied.insert(oreg, ovar);
            return Some(desired);
        }

        // Optimistic move was not cost-effective for this preg.
        // If there's a free preg for v (even if not the desired one),
        // we would have returned it in Algorithm 2. So we only continue
        // to try optimistic moves on the next-preferred occupied preg.
    }

    // Last resort: grab any free, non-forbidden preg (may have been
    // missed if ranked was empty or all forbidden). This shouldn't
    // normally trigger after Algorithm 3 attempts.
    for &p in pool {
        if !occupied.contains_key(&p) && !forbidden.is_forbidden(v, p) {
            return Some(p);
        }
    }

    None
}

/// After coloring `v` to `p`, broadcast a preference for `p` to all
/// uncolored chunk members. Uses the pre-computed chunk→members index
/// for O(chunk_size) instead of O(N). This is the affinity-propagation
/// step (paper §3.3) that drives coalescing.
fn broadcast_chunk_preference(
    v: Var,
    p: PReg,
    affinity: &mut AffinityChunks,
    prefs: &mut Preferences,
    chunk_index: &HashMap<u32, Vec<Var>>,
    vreg_alloc: &HashMap<Var, Allocation>,
    block_freq: u32,
) {
    let root = affinity.find(v);
    if let Some(members) = chunk_index.get(&root) {
        for &w in members {
            if w == v {
                continue;
            }
            if vreg_alloc.contains_key(&w) {
                continue;
            }
            if w.class != p.class {
                continue;
            }
            // Scale boost dynamically with block frequency
            prefs.boost(w, p, (block_freq * 4) as i32);
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
