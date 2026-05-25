//! Machine environment GPR sets for the register allocator.
//!
//! Supports x86-64 and AArch64 (ARM64) targets.

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
    pub const RAX: u32 = 0;
    pub const RCX: u32 = 1;
    pub const RDX: u32 = 2;
    pub const RSI: u32 = 4;
    pub const RDI: u32 = 5;
    pub const RBX: u32 = 6;
    pub const R8:  u32 = 8;
    pub const R9:  u32 = 9;
    pub const R10: u32 = 10;
    pub const R11: u32 = 11;
    pub const R12: u32 = 12;
    pub const R13: u32 = 13;
    pub const R14: u32 = 14;
    pub const R15: u32 = 15;

    /// Caller-saved GPRs (SysV ABI).
    pub const CALLER_SAVED: &[u32] = &[RAX, RCX, RDX, RSI, RDI, R8, R9, R10, R11];
    /// Callee-saved GPRs.
    pub const CALLEE_SAVED: &[u32] = &[RBX, R12, R13, R14, R15];

    /// Argument registers in SysV order: 1st→rdi, 2nd→rsi, 3rd→rdx, 4th→rcx, 5th→r8, 6th→r9.
    pub const ARG_REGS: &[u32] = &[RDI, RSI, RDX, RCX, R8, R9];

    /// Return-value register: rax.
    pub const RETURN_REG: u32 = RAX;
}

// ═══════════════════════════════════════════════════════════════════════════
//  AArch64 (ARM64)
// ═══════════════════════════════════════════════════════════════════════════
pub mod aarch64 {
    /// Caller-saved GPRs (AAPCS64): x0–x15 (excluding x16–x18).
    pub const CALLER_SAVED: &[u32] = &[
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
    ];

    /// Callee-saved GPRs (AAPCS64): x19–x28.
    pub const CALLEE_SAVED: &[u32] = &[
        19, 20, 21, 22, 23, 24, 25, 26, 27, 28,
    ];

    /// Argument / result registers: x0–x7 (AAPCS64 §6.4.2).
    pub const ARG_REGS: &[u32] = &[
        0, 1, 2, 3, 4, 5, 6, 7,
    ];

    /// Return value register: x0.
    pub const RETURN_REG: u32 = 0;
}
