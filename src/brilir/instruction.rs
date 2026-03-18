use anyhow::Result;
use std::fmt::Debug;
use crate::brilir::id::*;

#[derive(Copy, Clone, Eq, PartialEq, PartialOrd, Ord, Hash)]
pub struct Variable {
    pub id: VariableId,
    pub index: ValueId,
}

#[derive(Copy, Clone, Eq, PartialEq, PartialOrd, Ord, Hash)]
pub enum Immediate {
    Int(i64),
    Bool(bool),
}

impl Debug for Immediate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Immediate::Int(i) => write!(f, "{}", i),
            Immediate::Bool(b) => write!(f, "{}", b),
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
pub enum IrInstruction {
    Load(Variable, Immediate),
    Mov(Variable, Variable),
    Binary(BinaryOp, Variable, Variable, Variable),
    Not(Variable, Variable),
    Print(Variable),
    Jmp(BasicBlockId),
    Br(Variable, BasicBlockId, BasicBlockId),
    Ret(Variable),
    Call,
    Nop,
}

impl IrInstruction {
    pub fn get_def(&self) -> Result<Vec<&Variable>> {
        match self {
            IrInstruction::Load(dest, _) => Ok(vec![dest]),
            IrInstruction::Mov(dest, _) => Ok(vec![dest]),
            IrInstruction::Binary(_, dest, _, _) => Ok(vec![dest]),
            IrInstruction::Not(dest, _) => Ok(vec![dest]),
            _ => Ok(vec![]),
        }
    }

    pub fn get_use(&self) -> Result<Vec<&Variable>> {
        match self {
            IrInstruction::Mov(_, src) => Ok(vec![src]),
            IrInstruction::Binary(_, _, src1, src2) => Ok(vec![src1, src2]),
            IrInstruction::Not(_, src) => Ok(vec![src]),
            IrInstruction::Print(src) => Ok(vec![src]),
            IrInstruction::Br(cond, _, _) => Ok(vec![cond]),
            IrInstruction::Ret(var) => Ok(vec![var]),
            _ => Ok(vec![]),
        }
    }

    pub fn get_rhs_mut(&mut self) -> Result<Vec<&mut Variable>> {
        match self {
            IrInstruction::Mov(_, src) => Ok(vec![src]),
            IrInstruction::Binary(_, _, src1, src2) => Ok(vec![src1, src2]),
            IrInstruction::Not(_, src) => Ok(vec![src]),
            IrInstruction::Print(src) => Ok(vec![src]),
            IrInstruction::Br(cond, _, _) => Ok(vec![cond]),
            IrInstruction::Ret(var) => Ok(vec![var]),
            _ => Ok(vec![]),
        }
    }

    pub fn get_lhs_mut(&mut self) -> Result<Vec<&mut Variable>> {
        match self {
            IrInstruction::Load(dst, _) => Ok(vec![dst]),
            IrInstruction::Mov(dst, _) => Ok(vec![dst]),
            IrInstruction::Binary(_, dst, _, _) => Ok(vec![dst]),
            IrInstruction::Not(dst, _) => Ok(vec![dst]),
            _ => Ok(vec![]),
        }
    }
}

impl Debug for Variable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        return write!(f, "v{}_{}", self.id, self.index);
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

impl Debug for IrInstruction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IrInstruction::Load(dst, imm) => {
                write!(f, "{:?} = load {:?}", dst, imm)
            }
            IrInstruction::Mov(dst, src) => {
                write!(f, "{:?} = mov {:?}", dst, src)
            }
            IrInstruction::Binary(op, dst, lhs, rhs) => {
                write!(f, "{:?} = {:?} {:?} {:?}", dst, lhs, op, rhs)
            }
            IrInstruction::Not(dst, src) => {
                write!(f, "{:?} = !{:?}", dst, src)
            }
            IrInstruction::Print(src) => {
                write!(f, "print {:?}", src)
            }
            IrInstruction::Jmp(bb) => {
                write!(f, "jmp bb_{}", bb)
            }
            IrInstruction::Br(cond, truthy, falsy) => {
                write!(f, "br {:?} ? bb_{} : bb_{}", cond, truthy, falsy)
            }
            IrInstruction::Ret(var) => {
                write!(f, "ret {:?}", var)
            }
            IrInstruction::Call => {
                write!(f, "call")
            }
            IrInstruction::Nop => {
                write!(f, "nop")
            }
        }
    }
}
