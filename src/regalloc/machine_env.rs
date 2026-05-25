//! Machine environment register sets for the allocator.
//!
//! Exposes typed `xregalloc::PReg` constants (with `RegClass`) so callers
//! don't have to wrap raw u32 hardware encodings at every use site.
//!
//! Supports x86-64 and AArch64 (ARM64) targets.

use crate::xregalloc::PReg;

/// Target architecture for code generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    X86_64,
    Aarch64,
}

// ═══════════════════════════════════════════════════════════════════════════
//  x86-64
// ═══════════════════════════════════════════════════════════════════════════
pub mod x86 {
    use super::PReg;
    use crate::xregalloc::RegClass;

    pub const RAX: PReg = PReg::new(0,  RegClass::Int);
    pub const RCX: PReg = PReg::new(1,  RegClass::Int);
    pub const RDX: PReg = PReg::new(2,  RegClass::Int);
    pub const RSI: PReg = PReg::new(4,  RegClass::Int);
    pub const RDI: PReg = PReg::new(5,  RegClass::Int);
    pub const RBX: PReg = PReg::new(6,  RegClass::Int);
    pub const R8:  PReg = PReg::new(8,  RegClass::Int);
    pub const R9:  PReg = PReg::new(9,  RegClass::Int);
    pub const R10: PReg = PReg::new(10, RegClass::Int);
    pub const R11: PReg = PReg::new(11, RegClass::Int);
    pub const R12: PReg = PReg::new(12, RegClass::Int);
    pub const R13: PReg = PReg::new(13, RegClass::Int);
    pub const R14: PReg = PReg::new(14, RegClass::Int);
    pub const R15: PReg = PReg::new(15, RegClass::Int);

    /// Caller-saved GPRs (SysV ABI).
    pub const CALLER_SAVED: &[PReg] = &[RAX, RCX, RDX, RSI, RDI, R8, R9, R10, R11];
    /// Callee-saved GPRs.
    pub const CALLEE_SAVED: &[PReg] = &[RBX, R12, R13, R14, R15];

    /// Argument registers in SysV order: 1st→rdi, 2nd→rsi, 3rd→rdx, 4th→rcx, 5th→r8, 6th→r9.
    pub const ARG_REGS: &[PReg] = &[RDI, RSI, RDX, RCX, R8, R9];

    /// Return-value register: rax.
    pub const RETURN_REG: PReg = RAX;

    /// Scratch registers reserved from allocation. Used by xregalloc for
    /// spill-shuttle moves and by the post-allocation phi resolver as
    /// cycle-break temps in parallel-copy sequentialization. xregalloc
    /// filters these out of the allocatable pool before allocation runs,
    /// so they are guaranteed free at any program point.
    pub const SCRATCH_REGS: &[PReg] = &[R11, R10];
}

// ═══════════════════════════════════════════════════════════════════════════
//  AArch64 (ARM64)
// ═══════════════════════════════════════════════════════════════════════════
pub mod aarch64 {
    use super::PReg;
    use crate::xregalloc::RegClass;

    pub const X0:  PReg = PReg::new(0,  RegClass::Int);
    pub const X1:  PReg = PReg::new(1,  RegClass::Int);
    pub const X2:  PReg = PReg::new(2,  RegClass::Int);
    pub const X3:  PReg = PReg::new(3,  RegClass::Int);
    pub const X4:  PReg = PReg::new(4,  RegClass::Int);
    pub const X5:  PReg = PReg::new(5,  RegClass::Int);
    pub const X6:  PReg = PReg::new(6,  RegClass::Int);
    pub const X7:  PReg = PReg::new(7,  RegClass::Int);
    pub const X8:  PReg = PReg::new(8,  RegClass::Int);
    pub const X9:  PReg = PReg::new(9,  RegClass::Int);
    pub const X10: PReg = PReg::new(10, RegClass::Int);
    pub const X11: PReg = PReg::new(11, RegClass::Int);
    pub const X12: PReg = PReg::new(12, RegClass::Int);
    pub const X13: PReg = PReg::new(13, RegClass::Int);
    pub const X14: PReg = PReg::new(14, RegClass::Int);
    pub const X15: PReg = PReg::new(15, RegClass::Int);
    pub const X19: PReg = PReg::new(19, RegClass::Int);
    pub const X20: PReg = PReg::new(20, RegClass::Int);
    pub const X21: PReg = PReg::new(21, RegClass::Int);
    pub const X22: PReg = PReg::new(22, RegClass::Int);
    pub const X23: PReg = PReg::new(23, RegClass::Int);
    pub const X24: PReg = PReg::new(24, RegClass::Int);
    pub const X25: PReg = PReg::new(25, RegClass::Int);
    pub const X26: PReg = PReg::new(26, RegClass::Int);
    pub const X27: PReg = PReg::new(27, RegClass::Int);
    pub const X28: PReg = PReg::new(28, RegClass::Int);

    /// Caller-saved GPRs (AAPCS64): x0–x15.
    pub const CALLER_SAVED: &[PReg] =
        &[X0, X1, X2, X3, X4, X5, X6, X7, X8, X9, X10, X11, X12, X13, X14, X15];

    /// Callee-saved GPRs (AAPCS64): x19–x28.
    pub const CALLEE_SAVED: &[PReg] =
        &[X19, X20, X21, X22, X23, X24, X25, X26, X27, X28];

    /// Argument / result registers: x0–x7 (AAPCS64 §6.4.2).
    pub const ARG_REGS: &[PReg] = &[X0, X1, X2, X3, X4, X5, X6, X7];

    /// Return value register: x0.
    pub const RETURN_REG: PReg = X0;

    /// Scratch registers reserved from allocation. See `x86::SCRATCH_REGS`.
    pub const SCRATCH_REGS: &[PReg] = &[X14, X15];
}
