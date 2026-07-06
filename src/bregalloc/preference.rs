//! Register preference vectors (paper §3.1–3.2).
//!
//! For each variable v, `pref[v]` maps a `PReg` to a signed score: positive
//! values are "v wants this preg", negative are "v dislikes it". The scores
//! drive `get_register` during assignment.
//!
//! The paper's constraint vector formula (§3.2):
//!
//!   c_l(v) = e_R - 1      if v has Fixed(R) at l   (boost R, penalise others)
//!          = -Σ e_R'       otherwise               (dislike pregs claimed here)
//!
//! Critically, the paper sums *use* constraints over program points where v is
//! alive **before** l (live_before), and *def* constraints over points where v
//! is alive **after** l (live_after).  We honour that distinction so that, e.g.,
//! a def at a call instruction doesn't incorrectly dislike the call's clobbers.

use std::collections::HashMap;

use super::liveness::Liveness;
use super::uses;
use super::{AllocFunction, Constraint, OperandKind, PReg, RegClass, Var};

#[derive(Clone)]
pub struct Preferences {
    pub pref: HashMap<Var, HashMap<PReg, i32>>,
}

impl Preferences {
    pub fn compute<F: AllocFunction>(
        func: &F,
        liveness: &Liveness,
        freqs: &[u32],
        allocatable_by_class: &HashMap<RegClass, Vec<PReg>>,
    ) -> Self {
        let mut pref: HashMap<Var, HashMap<PReg, i32>> = HashMap::new();
        let next_use = uses::compute_next_use(func);

        for b in 0..func.num_blocks() {
            let freq = freqs.get(b).copied().unwrap_or(1) as i32;
            let mut live_now = liveness.live_in[b].clone();

            for inst in func.block_instructions(b) {
                // Partition Fixed constraints into Use-kind and Def-kind owners.
                let mut use_owners: HashMap<PReg, Vec<Var>> = HashMap::new();
                let mut def_owners: HashMap<PReg, Vec<Var>> = HashMap::new();
                for op in func.inst_operands(inst) {
                    if let Constraint::Fixed(p) = op.constraint {
                        match op.kind {
                            OperandKind::Use => use_owners.entry(p).or_default().push(op.var),
                            OperandKind::Def => def_owners.entry(p).or_default().push(op.var),
                        }
                    }
                }

                // (A) USE constraints — apply to live_before = live_now.
                //     Paper: Σ_{l | v alive before l} f_l · c_l^use(v)
                if !use_owners.is_empty() {
                    apply_prefs(&live_now, &use_owners, freq, &mut pref, allocatable_by_class);
                }

                // Advance live_now to live_after: add defs then remove dying uses.
                for op in func.inst_operands(inst) {
                    if op.kind == OperandKind::Def {
                        live_now.insert(op.var);
                    }
                }
                for op in func.inst_operands(inst) {
                    if op.kind == OperandKind::Use
                        && !uses::has_use_after(&next_use, op.var, inst)
                    {
                        live_now.remove(&op.var);
                    }
                }

                // (B) DEF constraints — apply to live_after = live_now (post-advance).
                //     Paper: Σ_{l | v alive after l} f_l · c_l^def(v)
                if !def_owners.is_empty() {
                    apply_prefs(&live_now, &def_owners, freq, &mut pref, allocatable_by_class);
                }
            }
        }

        Self { pref }
    }
}

/// Apply preference contributions from a single instruction's Fixed constraints
/// to all vars in `alive`.
///
/// For each claimed preg p:
///   - If v is the var fixed to p: boost p by freq and penalise all other
///     allocatable regs of v's class (paper's `e_R − 1`).
///   - Otherwise: penalise p by freq (paper's `−Σ e_R'`).
fn apply_prefs(
    alive: &std::collections::HashSet<Var>,
    owners: &HashMap<PReg, Vec<Var>>,
    freq: i32,
    pref: &mut HashMap<Var, HashMap<PReg, i32>>,
    allocatable_by_class: &HashMap<RegClass, Vec<PReg>>,
) {
    for &v in alive {
        for (&p, own) in owners {
            if p.class != v.class {
                continue;
            }
            let e = pref.entry(v).or_default().entry(p).or_insert(0);
            if own.contains(&v) {
                *e += freq;
                if let Some(pool) = allocatable_by_class.get(&v.class) {
                    for &other in pool {
                        if other != p {
                            *pref.entry(v).or_default().entry(other).or_insert(0) -= freq;
                        }
                    }
                }
            } else {
                *e -= freq;
            }
        }
    }
}

impl Preferences {
    /// Add `amount` to v's preference for `p`. Used to propagate affinity
    /// preferences during assignment.
    pub fn boost(&mut self, v: Var, p: PReg, amount: i32) {
        *self.pref.entry(v).or_default().entry(p).or_insert(0) += amount;
    }

    /// Pregs sorted by preference score, descending. Pregs with no entry
    /// (score 0) come last in stable pool order.
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
