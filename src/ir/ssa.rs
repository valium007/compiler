use hashbrown::{HashMap, HashSet};
use std::collections::BTreeSet;
use std::fmt::{self, Debug};

use crate::ir::brilir::{LivenessInfo, Op};
use crate::ir::id::*;

#[derive(Copy, Clone, Eq, PartialEq, PartialOrd, Ord, Hash)]
pub struct Variable {
    pub id: VariableId,
    pub index: ValueId,
}

impl Debug for Variable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "%v{}_{}", self.id, self.index)
    }
}

#[derive(Copy, Clone, Eq, PartialEq, PartialOrd, Ord)]
pub enum Value {
    Variable(Variable),
    Immediate(i64),
    Bool(bool),
}

impl Value {
    pub fn variable_mut(&mut self) -> &mut Variable {
        match self {
            Value::Variable(v) => v,
            _ => panic!("Expected variable"),
        }
    }

    pub fn variable(&self) -> &Variable {
        match self {
            Value::Variable(v) => v,
            _ => panic!("Expected variable"),
        }
    }

    pub fn immediate(self) -> i64 {
        match self {
            Value::Immediate(i) => i,
            _ => panic!("Expected immediate"),
        }
    }

    pub fn bool(self) -> bool {
        match self {
            Value::Bool(b) => b,
            _ => panic!("Expected bool"),
        }
    }
}

impl Debug for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Variable(v) => write!(f, "{:?}", v),
            Value::Immediate(i) => write!(f, "{}", i),
            Value::Bool(b) => write!(f, "{}", b),
        }
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Lt,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct BinOp {
    pub op: BinaryOp,
    pub dst: Value,
    pub lhs: Value,
    pub rhs: Value,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Load {
    pub dst: Value,
    pub src: Value,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Jmp {
    pub bb: BasicBlockId,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Br {
    pub dst: Value,
    pub truthy: BasicBlockId,
    pub falsy: BasicBlockId,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Phi {
    pub var: Variable,
    pub operands: Vec<(Value, BasicBlockId)>,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Print {
    pub src: Value,
}

#[derive(Clone, Eq, PartialEq)]
pub enum IrInstruction {
    IrLoad(Load),
    IrBinOp(BinOp),
    IrJmp(Jmp),
    IrBr(Br),
    IrPhi(Phi),
    IrPrint(Print),
    Nop,
}

impl IrInstruction {
    pub fn get_rhs_mut(&mut self) -> Vec<&mut Variable> {
        let mut ssa_rhs = Vec::new();

        match self {
            IrInstruction::IrBinOp(BinOp { lhs, rhs, .. }) => {
                ssa_rhs.push(lhs.variable_mut());
                ssa_rhs.push(rhs.variable_mut());
            }
            IrInstruction::IrBr(Br { dst, .. }) => ssa_rhs.push(dst.variable_mut()),
            IrInstruction::IrPrint(Print { src }) => ssa_rhs.push(src.variable_mut()),
            _ => {}
        }
        ssa_rhs
    }

    pub fn get_lhs_mut(&mut self) -> Vec<&mut Variable> {
        let mut ssa_lhs = Vec::new();

        match self {
            IrInstruction::IrBinOp(BinOp { dst, .. }) => {
                ssa_lhs.push(dst.variable_mut());
            }
            IrInstruction::IrLoad(Load { dst, .. }) => ssa_lhs.push(dst.variable_mut()),
            _ => {}
        }
        ssa_lhs
    }
}

impl Debug for IrInstruction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IrInstruction::IrLoad(Load { dst, src }) => {
                write!(f, "{:?} = load {:?} ", dst, src)
            }

            IrInstruction::IrBinOp(BinOp { op, dst, lhs, rhs }) => {
                write!(f, "{:?} = {:?} {:?} {:?} ", dst, op, lhs, rhs)
            }
            IrInstruction::IrJmp(Jmp { bb }) => {
                write!(f, "jmp {:?} ", bb)
            }
            IrInstruction::IrBr(Br { dst, truthy, falsy }) => {
                write!(f, "br {:?} {:?} {:?} ", dst, truthy, falsy)
            }

            IrInstruction::Nop => {
                write!(f, "nop")
            }

            IrInstruction::IrPhi(Phi { var, operands }) => {
                write!(f, "{:?} = phi {:?} ", var, operands)
            }

            IrInstruction::IrPrint(Print { src }) => {
                write!(f, "print {:?}", src)
            }
        }
    }
}

pub struct BasicBlock {
    pub id: BasicBlockId,
    pub instructions: Vec<IrInstruction>,
    pub preds: Vec<BasicBlockId>,
    pub succs: Vec<BasicBlockId>,
    pub defs: HashSet<Variable>,
    pub uses: HashSet<Variable>,
}

impl BasicBlock {
    pub fn new(id: BasicBlockId) -> Self {
        BasicBlock {
            id: id,
            instructions: Vec::new(),
            preds: Vec::new(),
            succs: Vec::new(),
            defs: HashSet::new(),
            uses: HashSet::new(),
        }
    }

    pub fn phis_mut(&mut self) -> Vec<&mut Phi> {
        let mut phis: Vec<&mut Phi> = Vec::new();
        for inst in self.instructions.iter_mut() {
            match inst {
                IrInstruction::IrPhi(phi) => phis.push(phi),
                _ => {}
            }
        }
        phis
    }

    pub fn phi(&self) -> Vec<&Phi> {
        let mut phis: Vec<&Phi> = Vec::new();
        for inst in self.instructions.iter() {
            match inst {
                IrInstruction::IrPhi(phi) => phis.push(phi),
                _ => {}
            }
        }
        phis
    }

    pub fn statements_mut(&mut self) -> Vec<&mut IrInstruction> {
        let mut statements = Vec::new();

        for inst in self.instructions.iter_mut() {
            match inst {
                IrInstruction::IrPhi(..) => {}
                _ => {
                    statements.push(inst);
                }
            }
        }
        statements
    }

    pub fn compute_def_use(&mut self) {
        let mut defs: HashSet<Variable> = HashSet::new();
        let mut uses: HashSet<Variable> = HashSet::new();

        for inst in self.instructions.iter_mut() {
            for def in inst.get_lhs_mut() {
                defs.insert(*def);
            }

            for use_ in inst.get_rhs_mut() {
                if !defs.contains(use_) {
                    uses.insert(*use_);
                }
            }
        }
        self.defs = defs;
        self.uses = uses;
    }
}

pub struct SSA {
    pub blocks: Vec<BasicBlock>,
    pub stacks: Vec<Vec<ValueId>>,
    pub counters: Vec<ValueId>,
}

impl SSA {
    pub fn new() -> Self {
        Self {
            blocks: Vec::new(),
            stacks: Vec::new(),
            counters: Vec::new(),
        }
    }

    pub fn inst_phi(&mut self, id: BasicBlockId, phi: IrInstruction) {
        self.blocks[id].instructions.insert(0, phi)
    }

    pub fn get_vars(&mut self) -> BTreeSet<Variable> {
        let mut vars = BTreeSet::new();

        for block in self.blocks.iter() {
            for inst in block.instructions.iter() {
                match inst {
                    IrInstruction::IrLoad(Load { dst, .. }) => match dst {
                        Value::Variable(dst) => {
                            vars.insert(*dst);
                        }
                        _ => unreachable!(),
                    },
                    IrInstruction::IrBinOp(BinOp { dst, .. }) => match dst {
                        Value::Variable(dst) => {
                            vars.insert(*dst);
                        }
                        _ => unreachable!(),
                    },
                    _ => {}
                }
            }
        }

        vars
    }

    pub fn get_var_defs(&mut self, var: &Variable) -> Vec<BasicBlockId> {
        let mut defs: Vec<BasicBlockId> = Vec::new();
        for block in self.blocks.iter() {
            for inst in block.instructions.iter() {
                match inst {
                    IrInstruction::IrLoad(Load { dst, .. }) => match dst {
                        Value::Variable(dst) => {
                            if dst == var {
                                defs.push(block.id);
                            }
                        }
                        _ => unreachable!(),
                    },
                    IrInstruction::IrBinOp(BinOp { dst, .. }) => match dst {
                        Value::Variable(dst) => {
                            if dst == var {
                                defs.push(block.id);
                            }
                        }
                        _ => unreachable!(),
                    },
                    _ => {}
                }
            }
        }
        defs
    }

    pub fn insert_phi(&mut self, df: &Vec<HashSet<BasicBlockId>>, liveness_info : &LivenessInfo, var_to_id: &HashMap<String,Variable>) {
        let vars = self.get_vars();

        let mut has_already: BTreeSet<BasicBlockId> = BTreeSet::new();
        let mut ever_on_worklist: BTreeSet<BasicBlockId> = BTreeSet::new();
        let mut worklist: BTreeSet<BasicBlockId> = BTreeSet::new();

        for var in vars.iter() {
            let mut varname = "";
            for (name,v) in var_to_id.iter() {
                if v == var {
                    varname = name;
                    break;
                }
            }
            for bb in self.get_var_defs(var) {
                ever_on_worklist.insert(bb);
                worklist.insert(bb);
            }
            while !worklist.is_empty() {
                let x = worklist.pop_last();
                for y in df[x.unwrap()].iter() {
                    if !has_already.contains(y) && liveness_info.live_in.get(y).unwrap().contains(varname) {
                        self.inst_phi(
                            *y,
                            IrInstruction::IrPhi(Phi {
                                var: *var,
                                operands: Vec::new(),
                            }),
                        );

                        has_already.insert(*y);
                        if !ever_on_worklist.contains(y) {
                            ever_on_worklist.insert(*y);
                            worklist.insert(*y);
                        }
                    }
                }
            }
            has_already.clear();
            ever_on_worklist.clear();
            worklist.clear();
        }
    }

    pub fn run_rename(
        &mut self,
    ) {
        let vars = self.get_vars().len();
        for v in 0..vars {
            self.counters.insert(v, 0);
            self.stacks.insert(v, Vec::new());
        }

        let mut visited: BTreeSet<BasicBlockId> = BTreeSet::new();
        rename(
            0,
            &mut self.blocks,
            &mut visited,
            &mut self.stacks,
            &mut self.counters,
        );
    }
}

pub fn rename(
    block_id: BasicBlockId,
    blocks: &mut Vec<BasicBlock>,
    visited: &mut BTreeSet<BasicBlockId>,
    stacks: &mut Vec<Vec<ValueId>>,
    counters: &mut Vec<ValueId>,
) {
    if visited.contains(&block_id) {
        return;
    }
    visited.insert(block_id);

    let succs = blocks[block_id].succs.clone();

    for phi in blocks[block_id].phis_mut() {
        gen_name(&mut phi.var, stacks, counters);
    }

    for statement in blocks[block_id].statements_mut() {
        for var in statement.get_rhs_mut() {
            var.index = *stacks[var.id].last().unwrap();
        }

        for var in statement.get_lhs_mut() {
            gen_name(var, stacks, counters);
        }
    }

    for succ in succs.iter() {
        let mut j = 0;
        for (i, pred) in blocks[*succ].preds.iter().enumerate() {
            if *pred == block_id {
                j = i;
            }
        }

        for phi in blocks[*succ].phis_mut() {
            let top = stacks[phi.var.id].last().unwrap();
            let new_var = Variable {
                id: phi.var.id,
                index: *top,
            };
            phi.operands.insert(j, (Value::Variable(new_var), block_id));
        }
    }

    for succ in succs.iter() {
        rename(*succ, blocks, visited, stacks, counters);
    }

    for inst in blocks[block_id].instructions.iter_mut() {
        match inst {
            IrInstruction::IrPhi(Phi { var, .. }) => {
                stacks[var.id].pop();
            }
            _ => {
                for var in inst.get_lhs_mut() {
                    stacks[var.id].pop();
                }
            }
        }
    }
}


pub fn gen_name(var: &mut Variable, stacks: &mut Vec<Vec<ValueId>>, counters: &mut Vec<ValueId>) {
    let i = counters[var.id];
    var.index = i;
    stacks[var.id].push(i);
    counters[var.id] = i + 1;
}