//! Liveness analysis.
//!
//! Classic backward dataflow fixpoint: `live_out[b] = ⋃_{s ∈ succ(b)} live_in[s]`
//! plus phi-operand contributions from φ-instructions in successors,
//! `live_in[b]` derived by walking instructions in reverse, removing defs
//! and adding uses.
//!
//! Phi operands count as uses *at the predecessor's terminator*, not as
//! uses inside the successor — this matches the semantics of φ as a
//! parallel copy on the edge.

use std::collections::HashSet;

use super::cfg::Cfg;
use super::{AllocFunction, OperandKind, Var};

pub struct Liveness {
    pub live_in: Vec<HashSet<Var>>,
    pub live_out: Vec<HashSet<Var>>,
}

impl Liveness {
    pub fn compute<F: AllocFunction>(func: &F) -> Self {
        let n = func.num_blocks();
        let mut live_in: Vec<HashSet<Var>> = vec![HashSet::new(); n];
        let mut live_out: Vec<HashSet<Var>> = vec![HashSet::new(); n];

        let cfg = Cfg::build(func);
        let rpo = cfg.reverse_postorder();
        let po: Vec<usize> = rpo.into_iter().rev().collect();

        let mut changed = true;
        while changed {
            changed = false;
            for &b in &po {
                let mut out: HashSet<Var> = HashSet::new();
                for &succ in func.block_successors(b) {
                    for &v in &live_in[succ] {
                        out.insert(v);
                    }
                    // Phi operands of successor's φs are live-out of this block.
                    for inst in func.block_instructions(succ) {
                        if func.is_phi(inst) {
                            out.insert(func.phi_op(inst, b));
                        }
                    }
                }
                live_out[b] = out;

                let mut cur = live_out[b].clone();
                for inst in func.block_instructions(b).rev() {
                    for op in func.inst_operands(inst) {
                        match op.kind {
                            OperandKind::Def => { cur.remove(&op.var); }
                            OperandKind::Use => { cur.insert(op.var); }
                        }
                    }
                }
                if cur != live_in[b] {
                    live_in[b] = cur;
                    changed = true;
                }
            }
        }
        Self { live_in, live_out }
    }
}
