use crate::ssa::ir::{Builder as SsaBuilder, IrInstruction, SsaValue, SsaVariable};
use crate::ssa::parallel_move;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct VarCopy {
    src: SsaVariable,
    dst: SsaVariable,
}

fn sequentialize_vars(parallel_copies: &[VarCopy], get_temp: impl FnMut() -> SsaVariable) -> Vec<VarCopy> {
    let pairs: Vec<(SsaVariable, SsaVariable)> = parallel_copies.iter()
        .map(|c| (c.src, c.dst))
        .collect();
    let seq = parallel_move::sequentialize(&pairs, get_temp);
    seq.into_iter()
        .map(|(src, dst)| VarCopy { src, dst })
        .collect()
}

pub fn lower_phis_to_parallel_moves(ssa: &mut SsaBuilder) {
    let num_blocks = ssa.blocks.len();
    let mut block_copies: Vec<Vec<VarCopy>> = vec![Vec::new(); num_blocks];
    
    for b in 0..num_blocks {
        let block = &ssa.blocks[b];
        let mut phis = Vec::new();
        for inst in &block.instrs {
            if let IrInstruction::PhiAssign(phi) = inst {
                phis.push(phi.clone());
            }
        }
        
        if phis.is_empty() {
            continue;
        }
        
        let predecessors = block.predecessors.clone();
        for pred in predecessors {
            let mut parallel_copies = Vec::new();
            for phi in &phis {
                // Phi operands are always Var by invariant.
                if let Some((src_val, _)) =
                    phi.operands.iter().find(|(_, from_b)| *from_b == pred)
                {
                    parallel_copies.push(VarCopy {
                        src: src_val.expect_var(),
                        dst: phi.var.expect_var(),
                    });
                }
            }
            
            let seq = sequentialize_vars(&parallel_copies, || ssa.get_fresh_var());
            block_copies[pred].extend(seq);
        }
    }
    
    for b in 0..num_blocks {
        let copies = std::mem::take(&mut block_copies[b]);
        if copies.is_empty() {
            continue;
        }
        
        let block = &mut ssa.blocks[b];
        let has_terminator = block.instrs.last().map_or(false, |inst| {
            matches!(inst, IrInstruction::Jmp(_) | IrInstruction::Br(_, _, _) | IrInstruction::Ret(_))
        });
        
        let new_instrs: Vec<IrInstruction> = copies.into_iter()
            .map(|c| IrInstruction::Mov(SsaValue::Var(c.dst), SsaValue::Var(c.src)))
            .collect();
            
        if has_terminator {
            let term = block.instrs.pop().unwrap();
            block.instrs.extend(new_instrs);
            block.instrs.push(term);
        } else {
            block.instrs.extend(new_instrs);
        }
    }
    
    for b in 0..num_blocks {
        ssa.blocks[b].instrs.retain(|inst| !inst.is_phi());
    }
}
