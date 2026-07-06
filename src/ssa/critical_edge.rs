use super::ir::{BasicBlock, BasicBlockId, Builder, IrInstruction};
use std::collections::HashMap;

/// Split all critical edges in the CFG.
///
/// A critical edge is an edge (A → B) where A has more than one successor
/// and B has more than one predecessor.  We insert a new empty block S on
/// the edge so that moves / copies can be placed unambiguously.
///
/// After this pass every `Br` target with multiple predecessors will have
/// been redirected through a fresh single-predecessor, single-successor
/// trampoline block.
pub fn split_critical_edges(builder: &mut Builder) {
    // Collect edges to split first to avoid mutating while iterating.
    // Each entry is (source_block, target_block, successor_index_in_source).
    let mut edges_to_split: Vec<(BasicBlockId, BasicBlockId, usize)> = Vec::new();

    for block in builder.blocks.iter() {
        if block.successors.len() <= 1 {
            continue;
        }
        for (succ_idx, &succ_id) in block.successors.iter().enumerate() {
            if builder.blocks[succ_id].predecessors.len() > 1 {
                edges_to_split.push((block.id, succ_id, succ_idx));
            }
        }
    }

    if edges_to_split.is_empty() {
        return;
    }

    for (src, dst, _succ_idx) in edges_to_split.iter() {
        let split_id = builder.blocks.len();

        // --- Create the new trampoline block ---
        let split_block = BasicBlock {
            id: split_id,
            instrs: vec![IrInstruction::Jmp(*dst)],
            successors: vec![*dst],
            predecessors: vec![*src],
            definitions: HashMap::new(),
            incomplete_phis: HashMap::new(),
        };
        builder.blocks.push(split_block);

        // --- Patch the source block's branch target(s) ---
        // Update successor list.
        let src_block = &mut builder.blocks[*src];
        for s in src_block.successors.iter_mut() {
            if *s == *dst {
                *s = split_id;
                break; // only patch one occurrence per edge
            }
        }
        // Update the actual branch instruction.
        if let Some(last) = src_block.instrs.last_mut() {
            match last {
                IrInstruction::Jmp(target) => {
                    if *target == *dst {
                        *target = split_id;
                    }
                }
                IrInstruction::Br(_, then_bb, else_bb) => {
                    if *then_bb == *dst {
                        *then_bb = split_id;
                    } else if *else_bb == *dst {
                        *else_bb = split_id;
                    }
                }
                _ => {}
            }
        }

        // --- Patch the destination block's predecessor list ---
        let dst_block = &mut builder.blocks[*dst];
        for p in dst_block.predecessors.iter_mut() {
            if *p == *src {
                *p = split_id;
                break;
            }
        }

        // --- Update phi operands in the destination block ---
        // Any phi that said "from src" must now say "from split_id".
        for instr in dst_block.instrs.iter_mut() {
            if let IrInstruction::PhiAssign(phi) = instr {
                for (_var, from_block) in phi.operands.iter_mut() {
                    if *from_block == *src {
                        *from_block = split_id;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssa::ir::{Phi, SsaValue, SsaVariable};

    /// Helper: build a minimal diamond CFG with a critical edge and a phi.
    ///
    /// ```text
    ///        bb_0
    ///       /    \
    ///     bb_1  bb_2
    ///       \    /
    ///        bb_3   (phi merges values from bb_1 and bb_2)
    /// ```
    ///
    /// bb_0 ends with `br cond bb_1 bb_2` → two successors.
    /// bb_1 ends with `jmp bb_3`          → one successor.
    /// bb_2 ends with `jmp bb_3`          → one successor.
    /// bb_3 has two predecessors.
    ///
    /// The edges bb_0→bb_1 and bb_0→bb_2 are NOT critical
    /// (bb_1 and bb_2 each have exactly one predecessor).
    ///
    /// Now if we change bb_0 to also jmp directly to bb_3 via a second
    /// edge, that edge (bb_0 → bb_3) would be critical.
    fn build_diamond_with_critical_edge() -> Builder {
        let mut b = Builder::new();

        let v0 = SsaVariable { id: 0, index: 0 };
        let v1 = SsaVariable { id: 1, index: 0 };
        let v2 = SsaVariable { id: 2, index: 0 };
        let v3 = SsaVariable { id: 3, index: 0 };

        // bb_0: br v0 bb_1 bb_3  (two successors, bb_3 has >1 pred → critical!)
        b.add_block(0, vec![], vec![1, 3]);
        b.blocks[0].instrs.push(IrInstruction::Const(SsaValue::Var(v0), SsaValue::Bool(true)));
        b.blocks[0].instrs.push(IrInstruction::Br(SsaValue::Var(v0), 1, 3));

        // bb_1: some work, then jmp bb_3
        b.add_block(1, vec![0], vec![3]);
        b.blocks[1].instrs.push(IrInstruction::Const(SsaValue::Var(v1), SsaValue::Int(10)));
        b.blocks[1].instrs.push(IrInstruction::Jmp(3));

        // bb_2: unused in this test but keeps IDs consistent
        b.add_block(2, vec![], vec![]);

        // bb_3: phi merging v1 (from bb_1) and v2 (from bb_0)
        b.add_block(3, vec![1, 0], vec![]);
        b.blocks[3].instrs.push(IrInstruction::PhiAssign(Phi {
            block: 3,
            var: SsaValue::Var(v3),
            operands: vec![(SsaValue::Var(v1), 1), (SsaValue::Var(v2), 0)],
        }));
        b.blocks[3].instrs.push(IrInstruction::Ret(SsaValue::Var(v3)));

        b
    }

    #[test]
    fn critical_edge_is_split() {
        let mut builder = build_diamond_with_critical_edge();
        assert_eq!(builder.blocks.len(), 4);

        split_critical_edges(&mut builder);

        // A new block should have been inserted.
        assert_eq!(builder.blocks.len(), 5, "expected one split block");

        let split = &builder.blocks[4];
        // The split block should jump to the original destination (bb_3).
        assert_eq!(split.successors, vec![3]);
        assert_eq!(split.predecessors, vec![0]);
        assert_eq!(split.instrs.len(), 1);
        assert!(matches!(split.instrs[0], IrInstruction::Jmp(3)));

        // bb_0 should now branch to bb_1 and the split block (4), not bb_3.
        let bb0 = &builder.blocks[0];
        assert!(bb0.successors.contains(&4));
        assert!(!bb0.successors.contains(&3));

        // bb_3's predecessors should now include the split block, not bb_0.
        let bb3 = &builder.blocks[3];
        assert!(bb3.predecessors.contains(&4));
        assert!(!bb3.predecessors.contains(&0));

        // The phi in bb_3 should reference the split block (4), not bb_0.
        if let IrInstruction::PhiAssign(phi) = &bb3.instrs[0] {
            let from_blocks: Vec<_> = phi.operands.iter().map(|(_, b)| *b).collect();
            assert!(from_blocks.contains(&4), "phi should reference split block");
            assert!(!from_blocks.contains(&0), "phi should no longer reference bb_0");
        } else {
            panic!("expected phi");
        }
    }

    #[test]
    fn no_critical_edges_is_noop() {
        let mut b = Builder::new();
        let v0 = SsaVariable { id: 0, index: 0 };

        // Simple linear: bb_0 → bb_1
        b.add_block(0, vec![], vec![1]);
        b.blocks[0].instrs.push(IrInstruction::Jmp(1));

        b.add_block(1, vec![0], vec![]);
        b.blocks[1].instrs.push(IrInstruction::Ret(SsaValue::Var(v0)));

        let before = b.blocks.len();
        split_critical_edges(&mut b);
        assert_eq!(b.blocks.len(), before, "no blocks should be added");
    }
}
