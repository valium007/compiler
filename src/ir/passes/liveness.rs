use crate::ir::ssa::{BasicBlock, Variable};
use hashbrown::HashSet;

pub struct Liveness;
impl Liveness {
    pub fn compute_liveness(
    blocks: &Vec<BasicBlock>,
) -> (Vec<HashSet<Variable>>, Vec<HashSet<Variable>>) {
    let mut livein: Vec<HashSet<Variable>> = vec![HashSet::new(); blocks.len()];
    let mut liveout: Vec<HashSet<Variable>> = vec![HashSet::new(); blocks.len()];

    loop {
        let mut changed = false;

        for block in blocks.iter() {
            let old_in = livein[block.id].clone();
            let old_out = liveout[block.id].clone();

            // Compute LiveOut first
            let mut new_liveout = HashSet::new();
            
            for succ in block.succs.iter() {
                for var in livein[*succ].iter() {
                    if !phi_defs(&blocks[*succ]).contains(var) {
                        new_liveout.insert(var.clone());
                    }
                }
            }
            
            new_liveout.extend(phi_uses(blocks, block));
            
            // Compute LiveIn
            let mut diff = HashSet::new();
            for var in new_liveout.iter() {
                if !block.defs.contains(var) {
                    diff.insert(var.clone());
                }
            }

            let mut new_livein = phi_defs(block);
            new_livein.extend(block.uses.clone());
            new_livein.extend(diff);

            // Update sets
            livein[block.id] = new_livein;
            liveout[block.id] = new_liveout;

            if old_in != livein[block.id] || old_out != liveout[block.id] {
                changed = true;
            }
        }

        if !changed {
            break;
        }
    }
    (livein, liveout)
}
}

fn phi_defs(block: &BasicBlock) -> HashSet<Variable> {
    let mut phi_defs = HashSet::new();
    for phi in block.phi() {
        phi_defs.insert(phi.var);
    }
    phi_defs
}

fn phi_uses(blocks: &Vec<BasicBlock>, block: &BasicBlock) -> HashSet<Variable> {
    let mut uses = HashSet::new();
    for succ in block.succs.iter() {
        for phi in blocks[*succ].phi() {
            for operand in phi.operands.iter() {
                if operand.1 == block.id {
                    uses.insert(*operand.0.variable());
                }
            }
        }
    }
    uses
}