//! Affinity chunks (paper §3.3).
//!
//! Union-find over `Var.id`. Vars connected by a move (copy) instruction
//! or by a phi (phi.dst ↔ each phi.operand) end up in the same chunk.
//! Cross-class unions are refused — coalescing across `RegClass` is
//! nonsense.
//!
//! The paper notes that affinity components can contain *internal*
//! interferences; properly splitting them is NP-complete. We don't split:
//! the chunks are used only to propagate register preferences when one
//! member is colored. The worst case is that we propagate a preference
//! to a var that interferes with the originator — which is harmless,
//! just sub-optimal.

use std::collections::HashMap;

use super::{AllocFunction, Constraint, OperandKind, PReg, Var};
use super::preference::Preferences;

#[derive(Clone)]
pub struct AffinityChunks {
    parent: Vec<u32>,
}

impl AffinityChunks {
    pub fn new(num_vregs: usize) -> Self {
        Self {
            parent: (0..num_vregs as u32).collect(),
        }
    }

    /// Build chunks from the function's copy and phi structure.
    pub fn build<F: AllocFunction>(func: &F) -> Self {
        let mut chunks = Self::new(func.num_vregs());

        // Precompute inst → block to keep phi processing O(n_inst).
        let inst_block = compute_inst_block(func);

        for inst in 0..func.num_instructions() {
            if func.is_copy(inst) {
                let ops = func.inst_operands(inst);
                if let Some((dst, src)) = copy_pair(ops) {
                    if dst.class == src.class {
                        chunks.union(dst, src);
                    }
                }
            } else if func.is_phi(inst) {
                let ops = func.inst_operands(inst);
                if let Some(dst) = phi_dst(ops) {
                    let b = inst_block[inst];
                    for &pred in func.block_predecessors(b) {
                        let src = func.phi_op(inst, pred);
                        if src.class == dst.class {
                            chunks.union(dst, src);
                        }
                    }
                }
            }
        }
        chunks
    }

    pub fn find(&mut self, v: Var) -> u32 {
        self.find_id(v.id)
    }

    pub fn find_id(&mut self, mut i: u32) -> u32 {
        let mut root = i;
        while self.parent[root as usize] != root {
            root = self.parent[root as usize];
        }
        while self.parent[i as usize] != root {
            let next = self.parent[i as usize];
            self.parent[i as usize] = root;
            i = next;
        }
        root
    }

    pub fn union(&mut self, a: Var, b: Var) {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra != rb {
            self.parent[ra as usize] = rb;
        }
    }

    /// Build a map from chunk-root → list of member `Var`s.
    /// `all_vars` must contain every var in the function.
    pub fn chunk_members_index(&mut self, all_vars: &[Var]) -> HashMap<u32, Vec<Var>> {
        let mut index: HashMap<u32, Vec<Var>> = HashMap::new();
        for &v in all_vars {
            let root = self.find(v);
            index.entry(root).or_default().push(v);
        }
        index
    }

    /// Paper §3.3: propagate Fixed-constraint preferences from chunk
    /// members to all other members, weighted by execution frequency.
    ///
    /// If variable `y` in a chunk has `Fixed(R0)` at some instruction in
    /// block `b`, then every other member of the chunk receives a boost
    /// for `R0` proportional to `freqs[b]`.
    pub fn propagate_constraints<F: AllocFunction>(
        &mut self,
        func: &F,
        prefs: &mut Preferences,
        freqs: &[u32],
        all_vars: &[Var],
    ) {
        let inst_block = compute_inst_block(func);
        // Collect (var, preg, freq) triples for every Fixed constraint.
        let mut fixed_claims: Vec<(Var, PReg, u32)> = Vec::new();
        for inst in 0..func.num_instructions() {
            let b = inst_block[inst];
            let freq = freqs.get(b).copied().unwrap_or(1);
            for op in func.inst_operands(inst) {
                if let Constraint::Fixed(p) = op.constraint {
                    fixed_claims.push((op.var, p, freq));
                }
            }
        }

        // Build chunk index.
        let index = self.chunk_members_index(all_vars);

        // For each fixed claim, boost all OTHER members of the same chunk.
        for &(v, p, freq) in &fixed_claims {
            let root = self.find(v);
            if let Some(members) = index.get(&root) {
                for &m in members {
                    if m == v { continue; }
                    if m.class != p.class { continue; }
                    prefs.boost(m, p, freq as i32);
                }
            }
        }
    }
}

fn copy_pair(ops: &[super::Operand]) -> Option<(Var, Var)> {
    if ops.len() >= 2
        && ops[0].kind == OperandKind::Def
        && ops[1].kind == OperandKind::Use
    {
        Some((ops[0].var, ops[1].var))
    } else {
        None
    }
}

fn phi_dst(ops: &[super::Operand]) -> Option<Var> {
    ops.first().and_then(|op| {
        if op.kind == OperandKind::Def {
            Some(op.var)
        } else {
            None
        }
    })
}

fn compute_inst_block<F: AllocFunction>(func: &F) -> Vec<usize> {
    let n_inst = func.num_instructions();
    let mut map = vec![0usize; n_inst];
    for b in 0..func.num_blocks() {
        for inst in func.block_instructions(b) {
            map[inst] = b;
        }
    }
    map
}
