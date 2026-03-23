use std::fmt::Debug;

use std::collections::{HashMap, HashSet};

pub type InstId = usize;
pub type ValueId = usize;
pub type VariableId = usize;
pub type BasicBlockId = usize;

#[derive(Copy, Clone, Default, Eq, PartialEq)]
pub struct SsaVariable {
    pub id: VariableId,
    pub index: ValueId,
}

impl Debug for SsaVariable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "%v{}_{}", self.id, self.index)
    }
}

#[derive(Clone, Eq, PartialEq)]
pub enum SsaValue {
    Var(SsaVariable),
    Int(i64),
    Bool(bool),
    Undef,
}

impl Debug for SsaValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SsaValue::Var(v) => write!(f, "{:?}", v),
            SsaValue::Int(i) => write!(f, "{}", i),
            SsaValue::Bool(b) => write!(f, "{}", b),
            SsaValue::Undef => write!(f, "undef"),
        }
    }
}

impl SsaValue {
    pub fn variable(&self) -> SsaVariable {
        match self {
            SsaValue::Var(v) => *v,
            _ => panic!("Expected variable"),
        }
    }
}

#[derive(Copy, Clone, Eq, PartialEq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Eq,
    Lt,
    Gt,
    Le,
    Ge,
    And,
    Or,
}

#[derive(Clone, Eq, PartialEq)]
pub struct Phi {
    pub block: BasicBlockId,
    pub var: SsaVariable,
    pub operands: Vec<(SsaVariable, BasicBlockId)>,
}

impl Debug for Phi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?} = phi ", self.var)?;
        if self.operands.is_empty() {
            write!(f, "[ ]")
        } else {
            write!(f, "[ ")?;
            for (i, (var, bb)) in self.operands.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "{:?} from bb_{}", var, bb)?;
            }
            write!(f, " ]")
        }
    }
}

#[derive(Clone, Eq, PartialEq)]
pub enum IrInstruction {
    Const(SsaVariable, SsaValue),
    Mov(SsaVariable, SsaValue),
    Binary(BinaryOp, SsaVariable, SsaValue, SsaValue),
    PhiAssign(Phi),
    Not(SsaVariable, SsaValue),
    Print(SsaValue),
    Jmp(BasicBlockId),
    Br(SsaValue, BasicBlockId, BasicBlockId),
    Ret(SsaValue),
    Call,
    Nop,
}

impl Debug for IrInstruction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IrInstruction::Const(dst, val) => write!(f, "{:?} = const {:?}", dst, val),
            IrInstruction::Mov(dst, src) => write!(f, "{:?} = mov {:?}", dst, src),
            IrInstruction::Binary(op, dst, lhs, rhs) => {
                write!(f, "{:?} = {:?} {:?} {:?}", dst, lhs, op, rhs)
            }
            IrInstruction::PhiAssign(phi) => write!(f, "{:?}", phi),
            IrInstruction::Not(dst, src) => write!(f, "{:?} = not {:?}", dst, src),
            IrInstruction::Print(val) => write!(f, "print {:?}", val),
            IrInstruction::Jmp(bb) => write!(f, "jmp bb_{}", bb),
            IrInstruction::Br(cond, then_bb, else_bb) => {
                write!(f, "br {:?} ? bb_{}:bb_{}", cond, then_bb, else_bb)
            }
            IrInstruction::Ret(val) => write!(f, "ret {:?}", val),
            IrInstruction::Call => write!(f, "call"),
            IrInstruction::Nop => write!(f, "nop"),
        }
    }
}

impl IrInstruction {
    pub fn phi_mut(&mut self) -> &mut Phi {
        match self {
            IrInstruction::PhiAssign(phi) => phi,
            _ => panic!("Expected phi"),
        }
    }

    pub fn phi(&self) -> &Phi {
        match self {
            IrInstruction::PhiAssign(phi) => phi,
            _ => panic!("Expected phi"),
        }
    }

    pub fn is_phi(&self) -> bool {
        match self {
            IrInstruction::PhiAssign(_) => true,
            _ => false,
        }
    }
}

impl Debug for BinaryOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BinaryOp::Add => write!(f, "add"),
            BinaryOp::Sub => write!(f, "sub"),
            BinaryOp::Mul => write!(f, "mul"),
            BinaryOp::Div => write!(f, "div"),
            BinaryOp::Eq => write!(f, "eq"),
            BinaryOp::Lt => write!(f, "lt"),
            BinaryOp::Gt => write!(f, "gt"),
            BinaryOp::Le => write!(f, "le"),
            BinaryOp::Ge => write!(f, "ge"),
            BinaryOp::And => write!(f, "and"),
            BinaryOp::Or => write!(f, "or"),
        }
    }
}

pub struct BasicBlock {
    pub id: BasicBlockId,
    pub instrs: Vec<IrInstruction>,
    pub successors: Vec<BasicBlockId>,
    pub predecessors: Vec<BasicBlockId>,
    pub definitions: HashMap<VariableId, SsaVariable>,
    pub incomplete_phis: HashMap<VariableId, InstId>,
}

impl Debug for BasicBlock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "bb_{}:", self.id)?;
        write!(f, "  ")?;
        if !self.predecessors.is_empty() {
            write!(f, "preds: ")?;
            for (i, pred) in self.predecessors.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "bb_{}", pred)?;
            }
        } else {
            write!(f, "preds: none")?;
        }
        write!(f, " | ")?;
        if !self.successors.is_empty() {
            write!(f, "succs: ")?;
            for (i, succ) in self.successors.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "bb_{}", succ)?;
            }
        } else {
            write!(f, "succs: none")?;
        }
        writeln!(f)?;
        for instr in self.instrs.iter() {
            writeln!(f, "  {:?}", instr)?;
        }
        Ok(())
    }
}

pub struct Builder {
    pub blocks: Vec<BasicBlock>,
    pub variables: HashMap<VariableId, SsaVariable>,
    pub sealed: HashSet<BasicBlockId>,
    pub next_var_id: VariableId,
}

impl Builder {
    pub fn new() -> Self {
        Self {
            blocks: Vec::new(),
            variables: HashMap::new(),
            sealed: HashSet::new(),
            next_var_id: 0,
        }
    }

    pub fn write_variable(&mut self, var: VariableId, bb: BasicBlockId) -> SsaVariable {
        if let Some(v) = self.variables.get_mut(&var) {
            v.index += 1;
        } else {
            let v = SsaVariable { id: var, index: 0 };
            self.variables.insert(var, v);
        }
        let v = self.variables[&var];
        self.write_variable_internal(var, bb, v);
        v
    }

    fn write_variable_internal(&mut self, var: VariableId, bb: BasicBlockId, value: SsaVariable) {
        self.get_block_mut(bb).definitions.insert(var, value);
    }

    pub fn read_variable(&mut self, var: VariableId, bb: BasicBlockId) -> SsaVariable {
        if let Some(v) = self.blocks[bb].definitions.get(&var) {
            return *v;
        }
        return self.read_variable_recursive(var, bb);
    }

    pub fn read_variable_recursive(&mut self, var: VariableId, bb: BasicBlockId) -> SsaVariable {
        let mut variable;
        if !self.sealed.contains(&bb) {
            variable = self.get_fresh_var();

            let phi = self.inst_phi(variable, bb);

            *self
                .get_block_mut(bb)
                .incomplete_phis
                .entry(var)
                .or_default() = phi;
        } else if self.get_block(bb).predecessors.len() == 1 {
            return self.read_variable(var, self.get_block(bb).predecessors[0]);
        } else {
            variable = self.get_fresh_var();
            let phi = self.inst_phi(variable, bb);
            self.write_variable_internal(var, bb, variable);
            variable = self.add_phi_operands(var, bb, phi);
        }
        self.write_variable_internal(var, bb, variable);
        variable
    }

    pub fn add_phi_operands(
        &mut self,
        var: VariableId,
        bb: BasicBlockId,
        phi: InstId,
    ) -> SsaVariable {
        let preds = self.get_block(bb).predecessors.clone();

        for pred in preds.iter() {
            let pred_var = self.read_variable(var, *pred);
            let phi = self.get_block_mut(bb).instrs[phi].phi_mut();
            phi.operands.push((pred_var, *pred));
        }

        self.try_remove_trivial_phi(phi, bb)
    }

    pub fn try_remove_trivial_phi(&mut self, phi: InstId, bb: BasicBlockId) -> SsaVariable {
        let mut same: SsaValue = SsaValue::Undef;
        let phi_var = self.get_block(bb).instrs[phi].phi().var;

        for (op, _) in self.get_block(bb).instrs[phi].phi().operands.iter() {
            if SsaValue::Var(*op) == same || *op == phi_var {
                continue;
            }
            if same != SsaValue::Undef {
                return phi_var;
            }
            same = SsaValue::Var(*op);
        }
        if same == SsaValue::Undef {
            panic!("unhandled edge case")
        }

        self.replace_phi_uses(phi_var, same.clone());
        // Replace with Nop instead of remove to preserve instruction indices
        self.get_block_mut(bb).instrs[phi] = IrInstruction::Nop;

        // Update definitions maps so stale phi variables don't get
        // returned by future read_variable calls.
        let resolved = same.variable();
        for block in self.blocks.iter_mut() {
            for (_, def) in block.definitions.iter_mut() {
                if *def == phi_var {
                    *def = resolved;
                }
            }
        }

        let users = self.get_phi_users(&same);
        for (block, inst) in users {
            self.try_remove_trivial_phi(inst, block);
        }

        same.variable()
    }

    fn get_phi_users(&self, var: &SsaValue) -> Vec<(BasicBlockId, InstId)> {
        let mut users = Vec::new();

        for block in self.blocks.iter() {
            for (i, inst) in block.instrs.iter().enumerate() {
                match inst {
                    IrInstruction::PhiAssign(phi) => {
                        for (op, _) in phi.operands.iter() {
                            if *op == var.variable() {
                                users.push((block.id, i));
                                continue;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        users
    }

    pub fn replace_phi_uses(&mut self, old: SsaVariable, new: SsaValue) {
        let old_val = SsaValue::Var(old);
        for block in self.blocks.iter_mut() {
            for inst in block.instrs.iter_mut() {
                match inst {
                    IrInstruction::PhiAssign(phi) => {
                        for (op, _) in phi.operands.iter_mut() {
                            if *op == old {
                                *op = new.variable();
                            }
                        }
                    }
                    IrInstruction::Binary(_, _, src1, src2) => {
                        if *src1 == old_val {
                            *src1 = new.clone();
                        }
                        if *src2 == old_val {
                            *src2 = new.clone();
                        }
                    }
                    IrInstruction::Const(_, src)
                    | IrInstruction::Mov(_, src)
                    | IrInstruction::Not(_, src) => {
                        if *src == old_val {
                            *src = new.clone();
                        }
                    }
                    IrInstruction::Print(src) | IrInstruction::Ret(src) => {
                        if *src == old_val {
                            *src = new.clone();
                        }
                    }
                    IrInstruction::Br(cond, _, _) => {
                        if *cond == old_val {
                            *cond = new.clone();
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    fn get_fresh_var(&mut self) -> SsaVariable {
        let var = self.next_var_id;
        let v = SsaVariable { id: var, index: 0 };
        self.variables.insert(var, v);
        self.next_var_id += 1;
        v
    }

    pub fn get_block(&self, id: BasicBlockId) -> &BasicBlock {
        &self.blocks[id]
    }

    pub fn get_block_mut(&mut self, id: BasicBlockId) -> &mut BasicBlock {
        &mut self.blocks[id]
    }
    pub fn inst_phi(&mut self, var: SsaVariable, bb: BasicBlockId) -> InstId {
        let i = self.blocks[bb].instrs.len();

        self.blocks[bb].instrs.push(IrInstruction::PhiAssign(Phi {
            block: bb,
            var,
            operands: Vec::new(),
        }));

        i
    }

    pub fn add_block(
        &mut self,
        id: BasicBlockId,
        predecessors: Vec<BasicBlockId>,
        successors: Vec<BasicBlockId>,
    ) {
        let block = BasicBlock {
            id,
            instrs: Vec::new(),
            successors,
            predecessors,
            definitions: HashMap::new(),
            incomplete_phis: HashMap::new(),
        };
        self.blocks.push(block);
    }

    pub fn current_block_id(&self) -> BasicBlockId {
        self.blocks.last().expect("No blocks created").id
    }

    pub fn add_instr(&mut self, instr: IrInstruction) {
        self.blocks
            .last_mut()
            .expect("No blocks created")
            .instrs
            .push(instr);
    }

    pub fn seal_block(&mut self, bb: BasicBlockId) {
        let incomplete = self.blocks[bb].incomplete_phis.clone();
        for (var, phi) in incomplete.iter() {
            self.add_phi_operands(*var, bb, *phi);
        }
        self.sealed.insert(bb);
    }
}

impl Debug for Builder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "SSA IR Program:")?;
        for block in self.blocks.iter() {
            writeln!(f, "{:?}", block)?;
        }
        Ok(())
    }
}
