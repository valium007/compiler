use crate::ir::{
    id::BasicBlockId,
    ssa::{BasicBlock, Variable},
};
use hashbrown::{HashMap, HashSet};
use std::collections::BTreeSet;

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

pub struct Liveness2 {
    pub live_in: HashMap<BasicBlockId, Vec<Variable>>,
    pub live_out: HashMap<BasicBlockId, Vec<Variable>>,
}

impl Liveness2 {
    pub fn new() -> Self {
        Liveness2 {
            live_in: HashMap::new(),
            live_out: HashMap::new(),
        }
    }

    pub fn compute_livesets(&mut self, blocks: &Vec<BasicBlock>, vars: BTreeSet<Variable>) {
        for block in blocks.iter() {
            self.live_in.insert(block.id, Vec::new());
            self.live_out.insert(block.id, Vec::new());
        }

        for var in vars.iter() {
            for block in blocks.iter() {
                if !block.uses.contains(var) && !phi_uses(blocks, block).contains(var) {
                    continue;
                }
                if phi_uses(blocks, block).contains(var) {
                    self.live_out.get_mut(&block.id).unwrap().push(*var);
                }
                self.up_and_mark(blocks, block, var);
            }
        }
    }

    fn up_and_mark(&mut self, blocks: &Vec<BasicBlock>, block: &BasicBlock, var: &Variable) {
        if block.defs.contains(var) {
            return;
        }

        if self.live_in.get(&block.id).unwrap().last() == Some(var) {
            return;
        }

        self.live_in.get_mut(&block.id).unwrap().push(*var);

        if phi_defs(block).contains(var) {
            return;
        }

        for pred in block.preds.iter() {
            if self.live_out.get(pred).unwrap().last() != Some(var) {
                self.live_out.get_mut(pred).unwrap().push(*var);
            }
            self.up_and_mark(blocks, &blocks[*pred], var);
        }
    }
}

pub struct Liveness {
    pub livein: Vec<HashSet<Variable>>,
    pub liveout: Vec<HashSet<Variable>>,
}

impl Liveness {
    pub fn new() -> Self {
        Liveness {
            livein: Vec::new(),
            liveout: Vec::new(),
        }
    }

    pub fn compute_liveness(&mut self, blocks: &Vec<BasicBlock>) {
        for b in 0..blocks.len() {
            self.livein.push(HashSet::new());
            self.liveout.push(HashSet::new());
        }

        for block in blocks.iter() {
            for var in phi_uses(&blocks, &block).iter() {
                self.liveout[block.id].insert(*var);
                self.up_and_mark(blocks, block, var);
            }
            for var in block.uses.iter() {
                self.up_and_mark(blocks, block, var);
            }
        }
    }

    pub fn up_and_mark(&mut self, blocks: &Vec<BasicBlock>, block: &BasicBlock, var: &Variable) {
        if block.defs.contains(var) {
            return;
        }
        if self.livein[block.id].contains(var) {
            return;
        }

        if phi_defs(block).contains(var) {
            return;
        }

        self.livein[block.id].insert(*var);

        for pred in block.preds.iter() {
            self.liveout[*pred].insert(*var);
            self.up_and_mark(blocks, block, var);
        }
    }
}
