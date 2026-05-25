//! Convert phi-based SSA IR into regalloc2's block-parameter form.
//!
//! The conversion:
//! 1. Assigns a unique `VReg` to every `SsaVariable`.
//! 2. Extracts phis into block parameters and branch arguments.
//! 3. Lowers remaining instructions into `RegAllocInst` with `Operand` lists.

use std::collections::HashMap;

use regalloc2::{Block, Operand, OperandConstraint, OperandKind, OperandPos, PReg, PRegSet, RegClass, VReg};

use crate::ssa::ir::{
    BasicBlockId, Builder as SsaBuilder, IrInstruction, SsaValue, SsaVariable,
};

use super::machine_env::Target;
use super::regalloc_ir::{RegAllocBlock, RegAllocFunction, RegAllocInst};

/// Map an `SsaVariable` to a `VReg`.
///
/// We encode `(id, index)` into a single flat number.  Because
/// `SsaVariable` pairs `(id, index)` are unique after SSA construction
/// we keep a HashMap for the mapping and assign sequential VReg numbers.
struct VRegMapper {
    map: HashMap<(usize, usize), VReg>,
    next: usize,
}

impl VRegMapper {
    fn new() -> Self {
        Self {
            map: HashMap::new(),
            next: 0,
        }
    }

    fn get_or_insert(&mut self, var: SsaVariable) -> VReg {
        let key = (var.id, var.index);
        if let Some(&vreg) = self.map.get(&key) {
            return vreg;
        }
        let vreg = VReg::new(self.next, RegClass::Int);
        self.map.insert(key, vreg);
        self.next += 1;
        vreg
    }

    fn get(&self, var: SsaVariable) -> VReg {
        let key = (var.id, var.index);
        *self.map.get(&key).unwrap_or_else(|| {
            panic!("VReg not found for {:?}", var);
        })
    }

    fn total(&self) -> usize {
        self.next
    }
}

/// Convert an `SsaValue` operand to a `VReg`, if it is a variable.
/// Constants are handled at the instruction level (they have no input VReg).
fn value_vreg(mapper: &mut VRegMapper, val: &SsaValue) -> VReg {
    match val {
        SsaValue::Var(v) => mapper.get_or_insert(*v),
        _ => panic!("expected SsaValue::Var, got {:?}", val),
    }
}

/// Build a regalloc2 `Operand` for a use (read).
fn use_operand(vreg: VReg) -> Operand {
    Operand::new(vreg, OperandConstraint::Reg, OperandKind::Use, OperandPos::Early)
}

/// Build a regalloc2 `Operand` for a def (write).
fn def_operand(vreg: VReg) -> Operand {
    Operand::new(vreg, OperandConstraint::Reg, OperandKind::Def, OperandPos::Late)
}

/// Build a fixed-register use operand.
fn fixed_use(vreg: VReg, preg: PReg) -> Operand {
    Operand::reg_fixed_use(vreg, preg)
}

/// Build a fixed-register def operand.
fn fixed_def(vreg: VReg, preg: PReg) -> Operand {
    Operand::reg_fixed_def(vreg, preg)
}

/// Convert the SSA IR into a `RegAllocFunction`.
pub fn convert(ssa: &SsaBuilder, target: Target) -> RegAllocFunction {
    let mut mapper = VRegMapper::new();

    // ─── Compute reachable block set (BFS from bb_0) ───────────────
    // Both regalloc2 and regalloc3 expect all blocks to be reachable from
    // the entry.  We walk only reachable blocks in the pre-pass below and
    // emit empty Nop-only blocks for unreachable ones.
    let n_blocks = ssa.blocks.len();
    let mut reachable = vec![false; n_blocks];
    if n_blocks > 0 {
        reachable[0] = true;
        let mut queue: Vec<usize> = vec![0];
        while let Some(b) = queue.pop() {
            for &s in &ssa.blocks[b].successors {
                if !reachable[s] {
                    reachable[s] = true;
                    queue.push(s);
                }
            }
        }
    }

    // ─── Pre-pass: assign VRegs to all variables we will encounter ────
    // Pre-register function parameters — they have no defining instruction
    // (the calling convention defines them at entry), so they must be seeded
    // into the mapper before any use site registers them.
    for &param in &ssa.params {
        mapper.get_or_insert(param);
    }

    // Walk every REACHABLE instruction so the mapper sees every SsaVariable.
    for block in ssa.blocks.iter() {
        if !reachable[block.id] { continue; }
        for instr in block.instrs.iter() {
            match instr {
                IrInstruction::PhiAssign(phi) => {
                    mapper.get_or_insert(phi.var);
                    for (op, _) in phi.operands.iter() {
                        mapper.get_or_insert(*op);
                    }
                }
                IrInstruction::Const(dst, _) => {
                    mapper.get_or_insert(*dst);
                }
                IrInstruction::Mov(dst, src) => {
                    mapper.get_or_insert(*dst);
                    if let SsaValue::Var(v) = src {
                        mapper.get_or_insert(*v);
                    }
                }
                IrInstruction::Binary(_, dst, lhs, rhs) => {
                    mapper.get_or_insert(*dst);
                    if let SsaValue::Var(v) = lhs { mapper.get_or_insert(*v); }
                    if let SsaValue::Var(v) = rhs { mapper.get_or_insert(*v); }
                }
                IrInstruction::Not(dst, src) => {
                    mapper.get_or_insert(*dst);
                    if let SsaValue::Var(v) = src { mapper.get_or_insert(*v); }
                }
                IrInstruction::Print(src) | IrInstruction::Ret(src) => {
                    if let SsaValue::Var(v) = src { mapper.get_or_insert(*v); }
                }
                IrInstruction::Br(cond, _, _) => {
                    if let SsaValue::Var(v) = cond { mapper.get_or_insert(*v); }
                }
                IrInstruction::Jmp(_) | IrInstruction::Nop => {}
                IrInstruction::Call { dest, args, .. } => {
                    if let Some(d) = dest {
                        mapper.get_or_insert(*d);
                    }
                    for arg in args {
                        if let SsaValue::Var(v) = arg { mapper.get_or_insert(*v); }
                    }
                }
            }
        }
    }

    // ─── Phase 1: Extract phis → block params + branch args ──────────

    // block_params[block_id] = list of VRegs defined by phis in that block
    let mut block_params: Vec<Vec<VReg>> = vec![Vec::new(); ssa.blocks.len()];

    // For each block, for each predecessor, the arguments that predecessor
    // must pass.  Keyed: phi_args[target_block][(pred_block)] = Vec<VReg>
    // We will reorder them to match successor indexing later.
    let mut phi_args_by_pred: Vec<HashMap<BasicBlockId, Vec<VReg>>> =
        vec![HashMap::new(); ssa.blocks.len()];

    for block in ssa.blocks.iter() {
        if !reachable[block.id] { continue; }
        for instr in block.instrs.iter() {
            if let IrInstruction::PhiAssign(phi) = instr {
                let dst_vreg = mapper.get(phi.var);
                block_params[block.id].push(dst_vreg);

                for (operand_var, from_block) in phi.operands.iter() {
                    let src_vreg = mapper.get(*operand_var);
                    phi_args_by_pred[block.id]
                        .entry(*from_block)
                        .or_default()
                        .push(src_vreg);
                }
            }
        }
    }

    // ─── Phase 2: Lower instructions, build flattened inst list ──────

    let mut insts: Vec<RegAllocInst> = Vec::new();
    let mut operands: Vec<Vec<Operand>> = Vec::new();
    let mut clobbers: Vec<PRegSet> = Vec::new();
    let mut ra_blocks: Vec<RegAllocBlock> = Vec::new();
    let mut branch_blockparams: Vec<Vec<Vec<VReg>>> = Vec::new();

    for block in ssa.blocks.iter() {
        let first_inst = insts.len();

        if reachable[block.id] {
            // ── Param defs at function entry ─────────────────────────────
            // Inject a synthetic Nop at the top of block 0 with fixed-reg def
            // operands for each function parameter.  This tells the allocator
            // that params arrive in x0..x7 (AAPCS64 §6.4.2).
            if block.id == 0 && !ssa.params.is_empty() {
                let arg_regs = match target {
                    Target::X86_64  => &[] as &[PReg],
                    Target::Aarch64 => super::machine_env::aarch64::ARG_REGS,
                };
                let mut param_ops: Vec<Operand> = Vec::new();
                for (i, &param) in ssa.params.iter().enumerate() {
                    let vreg = mapper.get(param);
                    if i < arg_regs.len() {
                        param_ops.push(fixed_def(vreg, arg_regs[i]));
                    } else {
                        param_ops.push(def_operand(vreg));
                    }
                }
                insts.push(RegAllocInst::Nop);
                operands.push(param_ops);
                clobbers.push(PRegSet::empty());
            }

            for instr in block.instrs.iter() {
                // Skip phis — they are now block params.
                if instr.is_phi() {
                    continue;
                }

                let (inst, ops, clob) = lower_instruction(instr, &mut mapper, target);

                insts.push(inst);
                operands.push(ops);
                clobbers.push(clob);
            }
        }

        // Every block must have at least one instruction for regalloc2.
        // Unreachable blocks (where we skipped the body) get a Nop too so
        // the block is still well-formed even if dead.
        if insts.len() == first_inst {
            insts.push(RegAllocInst::Nop);
            operands.push(Vec::new());
            clobbers.push(PRegSet::empty());
        }

        let last_inst_plus_one = insts.len();

        // Build successor / predecessor lists using regalloc2::Block.
        let succs: Vec<Block> = block.successors.iter().map(|&id| Block::new(id)).collect();
        let preds: Vec<Block> = block.predecessors.iter().map(|&id| Block::new(id)).collect();

        // Build branch_blockparams for this block.
        // For each successor (in order), look up what args this block passes.
        let mut bbp_for_block: Vec<Vec<VReg>> = Vec::new();
        for &succ_id in block.successors.iter() {
            let args = phi_args_by_pred[succ_id]
                .get(&block.id)
                .cloned()
                .unwrap_or_default();
            bbp_for_block.push(args);
        }
        branch_blockparams.push(bbp_for_block);

        ra_blocks.push(RegAllocBlock {
            succs,
            preds,
            first_inst,
            last_inst_plus_one,
        });
    }

    RegAllocFunction {
        insts,
        operands,
        clobbers,
        blocks: ra_blocks,
        block_params,
        branch_blockparams,
        num_vregs: mapper.total(),
    }
}

/// Lower a single SSA instruction into a `RegAllocInst` + operand list.
fn lower_instruction(
    instr: &IrInstruction,
    mapper: &mut VRegMapper,
    target: Target,
) -> (RegAllocInst, Vec<Operand>, PRegSet) {
    let empty_clobber = PRegSet::empty();

    match instr {
        IrInstruction::Const(dst, val) => {
            let imm = match val {
                SsaValue::Int(i) => *i,
                SsaValue::Bool(b) => if *b { 1 } else { 0 },
                _ => 0,
            };
            let dst_vreg = mapper.get(*dst);
            (
                RegAllocInst::Const(imm),
                vec![def_operand(dst_vreg)],
                empty_clobber,
            )
        }

        IrInstruction::Mov(dst, src) => {
            let dst_vreg = mapper.get(*dst);
            let src_vreg = value_vreg(mapper, src);
            (
                RegAllocInst::Mov,
                vec![def_operand(dst_vreg), use_operand(src_vreg)],
                empty_clobber,
            )
        }

        IrInstruction::Binary(op, dst, lhs, rhs) => {
            let dst_vreg = mapper.get(*dst);
            let lhs_vreg = value_vreg(mapper, lhs);
            let rhs_vreg = value_vreg(mapper, rhs);
            (
                RegAllocInst::BinaryOp(*op),
                vec![def_operand(dst_vreg), use_operand(lhs_vreg), use_operand(rhs_vreg)],
                empty_clobber,
            )
        }

        IrInstruction::Not(dst, src) => {
            let dst_vreg = mapper.get(*dst);
            let src_vreg = value_vreg(mapper, src);
            (
                RegAllocInst::Not,
                vec![def_operand(dst_vreg), use_operand(src_vreg)],
                empty_clobber,
            )
        }

        IrInstruction::Print(src) => {
            let src_vreg = value_vreg(mapper, src);
            // Print is a "call" — it clobbers caller-saved registers.
            let mut clob = PRegSet::empty();
            use super::machine_env;
            let caller_saved = match target {
                Target::X86_64  => machine_env::x86::CALLER_SAVED,
                Target::Aarch64 => machine_env::aarch64::CALLER_SAVED,
            };
            for &preg in caller_saved.iter() {
                clob.add(preg);
            }
            (
                RegAllocInst::Print,
                vec![use_operand(src_vreg)],
                clob,
            )
        }

        IrInstruction::Jmp(target) => {
            (RegAllocInst::Jmp(*target), Vec::new(), empty_clobber)
        }

        IrInstruction::Br(cond, then_bb, else_bb) => {
            let cond_vreg = value_vreg(mapper, cond);
            (
                RegAllocInst::Br(*then_bb, *else_bb),
                vec![use_operand(cond_vreg)],
                empty_clobber,
            )
        }

        IrInstruction::Ret(src) => {
            let src_vreg = value_vreg(mapper, src);
            (RegAllocInst::Ret, vec![use_operand(src_vreg)], empty_clobber)
        }

        IrInstruction::Call { callee_bb, dest, args, .. } => {
            use super::machine_env::aarch64;

            let caller_saved = match target {
                Target::X86_64  => super::machine_env::x86::CALLER_SAVED,
                Target::Aarch64 => aarch64::CALLER_SAVED,
            };
            let arg_regs = match target {
                Target::X86_64  => &[] as &[_],  // TODO: x86-64 ABI
                Target::Aarch64 => aarch64::ARG_REGS,
            };
            let ret_reg = match target {
                Target::X86_64  => super::machine_env::x86::RAX,
                Target::Aarch64 => aarch64::RETURN_REG,
            };

            let mut clob = PRegSet::empty();
            for &preg in caller_saved {
                // If this call has a return value fixed to ret_reg, don't mark
                // ret_reg as clobbered — the def operand accounts for it.
                if dest.is_some() && preg == ret_reg { continue; }
                clob.add(preg);
            }

            let mut ops: Vec<Operand> = Vec::new();

            // Optional return-value def (fixed to return register).
            if let Some(d) = dest {
                let dst_vreg = mapper.get(*d);
                ops.push(fixed_def(dst_vreg, ret_reg));
            }

            // Register arguments (fixed to x0..x7 / rdi, rsi...).
            let ssa_args: Vec<_> = args.iter()
                .filter_map(|a| if let SsaValue::Var(v) = a { Some(*v) } else { None })
                .collect();
            for (i, v) in ssa_args.iter().enumerate() {
                let vreg = mapper.get(*v);
                if i < arg_regs.len() {
                    ops.push(fixed_use(vreg, arg_regs[i]));
                } else {
                    // Stack arg: treated as a regular use for now; codegen
                    // emits the stack store in Phase 4.
                    ops.push(use_operand(vreg));
                }
            }

            (RegAllocInst::Call(*callee_bb), ops, clob)
        }

        IrInstruction::Nop => {
            (RegAllocInst::Nop, Vec::new(), empty_clobber)
        }

        IrInstruction::PhiAssign(_) => {
            unreachable!("phis should have been stripped before lowering")
        }
    }
}
