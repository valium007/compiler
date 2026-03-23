use crate::brilir::instruction::IrInstruction;
use crate::brilir::instruction::{Variable,BasicBlockId};
use hashbrown::HashSet;
use std::fmt::Debug;

#[derive(Clone)]
pub struct BasicBlock {
    pub id: BasicBlockId,
    pub instrs: Vec<IrInstruction>,
    pub successors: HashSet<BasicBlockId>,
    pub predecessors: HashSet<BasicBlockId>,
}

impl Debug for BasicBlock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "");
        writeln!(
            f,
            "bb_{} succs: {:?} preds: {:?}",
            self.id, self.successors, self.predecessors
        );
        for instr in self.instrs.iter() {
            writeln!(f, "  {:?}", instr);
        }
        write!(f, "")
    }
}

impl BasicBlock {
    pub fn new(id: BasicBlockId) -> Self {
        Self {
            id: id,
            instrs: Vec::new(),
            successors: HashSet::new(),
            predecessors: HashSet::new(),
        }
    }
}

pub struct Builder {
    pub blocks: Vec<BasicBlock>,
    pub liveness: (Vec<HashSet<Variable>>, Vec<HashSet<Variable>>),
    pub next_var_id: usize,
}

impl Builder {
    pub fn new() -> Self {
        Self {
            blocks: Vec::new(),
            liveness: (Vec::new(), Vec::new()),
            next_var_id: 0,
        }
    }

    pub fn add_block(&mut self, id: BasicBlockId) -> &mut BasicBlock {
        let block = BasicBlock::new(id);
        self.blocks.push(block);
        self.blocks.last_mut().unwrap()
    }

    pub fn get_current_block_mut(&mut self) -> &mut BasicBlock {
        self.blocks.last_mut().unwrap()
    }

    pub fn add_instr(&mut self, instr: IrInstruction) {
        self.blocks.last_mut().unwrap().instrs.push(instr);
    }

    pub fn add_edge(&mut self, from: BasicBlockId, to: BasicBlockId) {
        self.blocks[from].successors.insert(to);
        self.blocks[to].predecessors.insert(from);
    }
}
