//! Register preference vectors (paper §3.1–3.2).
//!
//! For each variable v, `pref[v]` maps a `PReg` to a signed score: positive
//! values are "v wants this preg", negative are "v dislikes it". The scores
//! drive `get_register` during assignment: at each def, we sort the
//! per-var preference map by score and pick the highest-scoring preg that
//! is free and not forbidden.
//!
//! The vector is initialized by walking the program once: at every inst
//! where v is live, accumulate the constraint vector weighted by the
//! block's execution frequency. The paper's constraint vector is:
//!
//!   c_l(v) = e_R - 1      if v has Fixed(R) at l   (boost R, slight dislike of others)
//!          = -Σ e_R'       otherwise               (dislike pregs claimed by others)
//!
//! In our implementation we keep `e_R - 1` simpler: just boost R by freq,
//! no penalty on others. The "dislike others" effect is provided by the
//! Σ -e_R' contribution from instructions where some OTHER var is fixed.

use std::collections::HashMap;

use super::liveness::Liveness;
use super::{AllocFunction, Constraint, OperandKind, PReg, Var};

#[derive(Clone)]
pub struct Preferences {
    pub pref: HashMap<Var, HashMap<PReg, i32>>,
}

impl Preferences {
    pub fn compute<F: AllocFunction>(
        func: &F,
        liveness: &Liveness,
        freqs: &[u32],
    ) -> Self {
        let mut pref: HashMap<Var, HashMap<PReg, i32>> = HashMap::new();

        let next_use = compute_next_use_lists(func);

        // Per-block forward walk: maintain live_now = live_before(inst).
        for b in 0..func.num_blocks() {
            let freq = freqs.get(b).copied().unwrap_or(1) as i32;
            let mut live_now = liveness.live_in[b].clone();
            for inst in func.block_instructions(b) {
                // Vars live at this inst: live_now (live_before) + defs.
                let mut defs_here: Vec<Var> = Vec::new();
                for op in func.inst_operands(inst) {
                    if op.kind == OperandKind::Def {
                        defs_here.push(op.var);
                    }
                }

                // Gather Fixed claims at this inst: (preg, var-fixed-here).
                let mut claims: Vec<(PReg, Var)> = Vec::new();
                for op in func.inst_operands(inst) {
                    if let Constraint::Fixed(p) = op.constraint {
                        claims.push((p, op.var));
                    }
                }

                // Update preferences for all vars alive at this inst.
                let mut alive_here: Vec<Var> = live_now.iter().copied().collect();
                for &d in &defs_here {
                    if !alive_here.contains(&d) {
                        alive_here.push(d);
                    }
                }

                // Group fixed claims by register.
                let mut preg_owners: HashMap<PReg, Vec<Var>> = HashMap::new();
                for &(p, owner) in &claims {
                    preg_owners.entry(p).or_default().push(owner);
                }

                for &v in &alive_here {
                    for (&p, owners) in &preg_owners {
                        if p.class != v.class {
                            continue;
                        }
                        let entry = pref.entry(v).or_default().entry(p).or_insert(0);
                        if owners.contains(&v) {
                            // v itself wants p at this inst: boost.
                            *entry += freq;
                        } else {
                            // Someone else claims p here: v dislikes p.
                            *entry -= freq;
                        }
                    }
                }

                // Advance live_now: add defs, remove dying uses.
                for &d in &defs_here {
                    live_now.insert(d);
                }
                for op in func.inst_operands(inst) {
                    if op.kind == OperandKind::Use {
                        if !has_use_after(&next_use, op.var, inst) {
                            live_now.remove(&op.var);
                        }
                    }
                }
            }
        }

        Self { pref }
    }
}

fn compute_next_use_lists<F: AllocFunction>(func: &F) -> HashMap<Var, Vec<usize>> {
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

fn has_use_after(next_use: &HashMap<Var, Vec<usize>>, v: Var, inst: usize) -> bool {
    let after_flat = inst * 2 + 1 + 1;
    next_use.get(&v).map_or(false, |ul| ul.iter().any(|&u| u >= after_flat))
}

impl Preferences {
    /// Add `amount` to v's preference for `p`. Used to propagate affinity
    /// preferences during assignment.
    pub fn boost(&mut self, v: Var, p: PReg, amount: i32) {
        *self.pref.entry(v).or_default().entry(p).or_insert(0) += amount;
    }

    /// Pregs sorted by preference score, descending. Pregs with no entry
    /// (score 0) come last in arbitrary order. Used by get_register.
    pub fn sorted_pregs(&self, v: Var, pool: &[PReg]) -> Vec<PReg> {
        let empty: HashMap<PReg, i32> = HashMap::new();
        let scores = self.pref.get(&v).unwrap_or(&empty);
        let mut ranked: Vec<(PReg, i32)> = pool
            .iter()
            .map(|&p| (p, scores.get(&p).copied().unwrap_or(0)))
            .collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1));
        ranked.into_iter().map(|(p, _)| p).collect()
    }

    pub fn score(&self, v: Var, p: PReg) -> i32 {
        self.pref.get(&v).and_then(|m| m.get(&p)).copied().unwrap_or(0)
    }
}
