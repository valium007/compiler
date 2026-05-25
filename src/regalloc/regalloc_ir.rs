use std::fmt;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct PhysReg(pub usize);

impl PhysReg {
    pub const fn new(hw: usize) -> Self { Self(hw) }
    pub fn hw_enc(self) -> usize { self.0 }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct SpillSlot(pub usize);

impl SpillSlot {
    pub const fn new(index: usize) -> Self { Self(index) }
    pub fn index(self) -> usize { self.0 }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Allocation {
    None,
    Reg(PhysReg),
    Stack(SpillSlot),
}

impl Allocation {
    pub fn reg(p: PhysReg) -> Self { Self::Reg(p) }
    pub fn stack(s: SpillSlot) -> Self { Self::Stack(s) }
    pub fn none() -> Self { Self::None }

    pub fn is_none(&self) -> bool { matches!(self, Self::None) }
    pub fn is_reg(&self) -> bool { matches!(self, Self::Reg(_)) }
    pub fn is_stack(&self) -> bool { matches!(self, Self::Stack(_)) }

    pub fn as_reg(&self) -> Option<PhysReg> { if let Self::Reg(p) = self { Some(*p) } else { None } }
    pub fn as_stack(&self) -> Option<SpillSlot> { if let Self::Stack(s) = self { Some(*s) } else { None } }
}

/// Instruction after register allocation — all references are physical
/// `Allocation`s (registers or spill slots).
#[derive(Clone, Debug)]
pub enum LoweredInst {
    /// mov dst, src   (register or stack copy)
    Mov(Allocation, Allocation),
    /// mov dst, imm64
    LoadConst(Allocation, i64),
    /// dst = op lhs, rhs  (two-address: dst == lhs after isel)
    BinaryOp(crate::ssa::ir::BinaryOp, Allocation, Allocation, Allocation),
    /// dst = not src
    Not(Allocation, Allocation),
    /// print src
    Print(Allocation),
    /// call target
    Call(usize),
    /// jmp target_block
    Jmp(usize),
    /// br cond, then_block, else_block
    Br(Allocation, usize, usize),
    /// ret src
    Ret(Allocation),
    /// no-op (padding / removed instruction)
    Nop,
}

impl fmt::Display for LoweredInst {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoweredInst::Mov(dst, src) => write!(f, "mov {:?}, {:?}", dst, src),
            LoweredInst::LoadConst(dst, imm) => write!(f, "mov {:?}, {}", dst, imm),
            LoweredInst::BinaryOp(op, dst, lhs, rhs) => {
                write!(f, "{:?} {:?}, {:?}, {:?}", op, dst, lhs, rhs)
            }
            LoweredInst::Not(dst, src) => write!(f, "not {:?}, {:?}", dst, src),
            LoweredInst::Print(src) => write!(f, "print {:?}", src),
            LoweredInst::Call(bb) => write!(f, "call .bb_{}", bb),
            LoweredInst::Jmp(bb) => write!(f, "jmp bb_{}", bb),
            LoweredInst::Br(cond, t, e) => write!(f, "br {:?}, bb_{}, bb_{}", cond, t, e),
            LoweredInst::Ret(src) => write!(f, "ret {:?}", src),
            LoweredInst::Nop => write!(f, "nop"),
        }
    }
}
