//! Forbidden physical registers per variable.
//!
//! For each var v, the set of pregs that v's home register can never be —
//! because at some instruction along v's live range, the preg is either
//! clobbered or claimed by another var's `Fixed` constraint, and v's value
//! would not survive.
//!
//! Carve-out (matching xregalloc's `live_conflict` logic): if v itself
//! has a `Fixed(preg)` operand at the inst AND v has no use after the
//! inst, then v's role at that inst is precisely to flow through `preg`
//! being consumed/destroyed — not a corruption. The preg is not forbidden
//! for v in that case.

use std::collections::{HashMap, HashSet};

use super::liveness::Liveness;
use super::uses;
use super::{AllocFunction, Constraint, OperandKind, PReg, Var};

pub struct Forbidden {
    pub forbidden: HashMap<Var, HashSet<PReg>>,
}

impl Forbidden {
    pub fn compute<F: AllocFunction>(func: &F, liveness: &Liveness) -> Self {
        let n_inst = func.num_instructions();

        // For each var v, the flat-index of v's last use across the whole
        // function. Used by the dies-here carve-out.
        let last_use = compute_last_use(func);

        let mut forbidden: HashMap<Var, HashSet<PReg>> = HashMap::new();

        let live_at = LiveAtInst::build(func, liveness);

        for inst in 0..n_inst {
            // Gather claims (clobbers + Fixed at this inst), and for each
            // claimed preg, the set of vars that are "fixed to that preg
            // at this inst" (used by the carve-out).
            let mut claims: HashMap<PReg, HashSet<Var>> = HashMap::new();
            for &p in func.inst_clobbers(inst) {
                claims.entry(p).or_default();
            }
            for op in func.inst_operands(inst) {
                if let Constraint::Fixed(p) = op.constraint {
                    claims.entry(p).or_default().insert(op.var);
                }
            }
            if claims.is_empty() {
                continue;
            }

            // Vars to consider: everything live at this inst (live_before
            // + defs).
            let candidates: &HashSet<Var> = live_at.at(inst);

            let inst_flat = inst * 2 + 1;
            let implicit_reads: std::collections::HashSet<PReg> =
                func.inst_implicit_reads(inst).iter().copied().collect();

            for (preg, fixed_at_here) in &claims {
                for &v in candidates {
                    let skip = if fixed_at_here.contains(&v) {
                        // v is itself fixed to this preg: safe if it's a fresh
                        // def here, or if it's consumed and dies at this inst.
                        let is_def_here = func.inst_operands(inst).iter().any(|op| {
                            op.var == v && op.kind == OperandKind::Def
                                && op.constraint == Constraint::Fixed(*preg)
                        });
                        is_def_here || last_use.get(&v).map_or(true, |lu| *lu <= inst_flat)
                    } else if !implicit_reads.contains(preg) {
                        // Pure-write clobber: the preg is not read by this
                        // instruction before the write, so a var that dies here
                        // (its value is read before the clobber fires) is safe.
                        last_use.get(&v).map_or(true, |lu| *lu <= inst_flat)
                    } else {
                        false // implicit-read preg (e.g. RDX for idiv): no carve-out
                    };
                    if skip {
                        continue;
                    }
                    if v.class != preg.class {
                        continue;
                    }
                    forbidden.entry(v).or_default().insert(*preg);
                }
            }
        }

        Self { forbidden }
    }

    pub fn is_forbidden(&self, v: Var, p: PReg) -> bool {
        self.forbidden.get(&v).map_or(false, |s| s.contains(&p))
    }
}

/// Last-use flat index per var, across the whole function. Phi operands
/// count as uses at the predecessor terminator.
fn compute_last_use<F: AllocFunction>(func: &F) -> HashMap<Var, usize> {
    let mut last: HashMap<Var, usize> = HashMap::new();
    let bump = |v: Var, flat: usize, last: &mut HashMap<Var, usize>| {
        let e = last.entry(v).or_insert(0);
        if flat > *e {
            *e = flat;
        }
    };

    for inst in 0..func.num_instructions() {
        let flat = inst * 2 + 1;
        for op in func.inst_operands(inst) {
            if op.kind == OperandKind::Use {
                bump(op.var, flat, &mut last);
            }
        }
    }
    for b in 0..func.num_blocks() {
        let term = func.block_instructions(b).end.saturating_sub(1);
        let flat = term * 2 + 1;
        for &succ in func.block_successors(b) {
            for inst in func.block_instructions(succ) {
                if func.is_phi(inst) {
                    bump(func.phi_op(inst, b), flat, &mut last);
                }
            }
        }
    }
    last
}

/// "Live at instruction" oracle: for each inst, the set of vars live there
/// (live_before(inst) ∪ defs(inst)). Built once via a forward walk per
/// block.
struct LiveAtInst {
    per_inst: Vec<HashSet<Var>>,
}

impl LiveAtInst {
    fn build<F: AllocFunction>(func: &F, liveness: &Liveness) -> Self {
        let n_inst = func.num_instructions();
        let mut per_inst: Vec<HashSet<Var>> = vec![HashSet::new(); n_inst];

        let next_use = uses::compute_next_use(func);

        for b in 0..func.num_blocks() {
            // block_end_flat: first flat index belonging to a *different* block.
            // Valid flat indices in b are [block_start*2, block_end*2).
            let block_end_flat = func.block_instructions(b).end * 2;
            let mut live_now = liveness.live_in[b].clone();
            for inst in func.block_instructions(b) {
                let mut at = live_now.clone();
                for op in func.inst_operands(inst) {
                    if op.kind == OperandKind::Def {
                        at.insert(op.var);
                    }
                }
                per_inst[inst] = at;

                // Advance: remove dying uses, add defs.
                for op in func.inst_operands(inst) {
                    if op.kind == OperandKind::Use {
                        if !liveness.live_out[b].contains(&op.var)
                            && !has_use_in_block_after(&next_use, op.var, inst, block_end_flat)
                        {
                            live_now.remove(&op.var);
                        }
                    }
                }
                for op in func.inst_operands(inst) {
                    if op.kind == OperandKind::Def {
                        live_now.insert(op.var);
                    }
                }
            }
        }
        Self { per_inst }
    }

    fn at(&self, inst: usize) -> &HashSet<Var> {
        &self.per_inst[inst]
    }
}

/// Returns true if v has any use strictly after `inst` and before `block_end_flat`
/// (exclusive), all in flat-index space.
fn has_use_in_block_after(
    next_use: &HashMap<Var, Vec<usize>>,
    v: Var,
    inst: usize,
    block_end_flat: usize,
) -> bool {
    let after = inst * 2 + 2;
    next_use
        .get(&v)
        .map_or(false, |ul| ul.iter().any(|&u| u >= after && u < block_end_flat))
}
