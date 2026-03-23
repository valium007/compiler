use std::fmt::Debug;

pub type BasicBlockId = usize;

#[derive(Copy, Clone, Eq, PartialEq, PartialOrd, Ord, Hash)]
pub struct Variable(pub usize);

#[derive(Copy, Clone, Eq, PartialEq, PartialOrd, Ord, Hash)]
pub enum Value {
    Int(i64),
    Bool(bool),
}

impl Debug for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Int(i) => write!(f, "{}", i),
            Value::Bool(b) => write!(f, "{}", b),
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
    Load(Variable, Value),
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

impl Debug for Variable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        return write!(f, "v{}", self.0);
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
