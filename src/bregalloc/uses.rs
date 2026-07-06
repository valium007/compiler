//! Shared next-use list computation for bregalloc passes.
//!
//! Uses flat index space `inst * 2 + 1` (the "use slot" of each instruction).
//! Phi operands are recorded at the predecessor block's terminator flat index,
//! modelling the SSA convention that phi inputs are consumed on the incoming edge.

use std::collections::HashMap;

use super::{AllocFunction, OperandKind, Var};

/// For each variable, a sorted, deduplicated list of flat indices
/// (`inst * 2 + 1`) at which the variable is used.
///
/// Phi operands count as uses at the flat index of the last instruction of
/// their predecessor block — not at the phi instruction itself.
pub fn compute_next_use<F: AllocFunction>(func: &F) -> HashMap<Var, Vec<usize>> {
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
        ul.dedup();
    }
    uses
}

/// Returns `true` if `v` has any use strictly after `inst` (flat index space).
pub fn has_use_after(next_use: &HashMap<Var, Vec<usize>>, v: Var, inst: usize) -> bool {
    let after = inst * 2 + 2;
    next_use.get(&v).map_or(false, |ul| ul.iter().any(|&u| u >= after))
}
