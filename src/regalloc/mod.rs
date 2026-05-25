pub mod parallel_move;
pub mod regalloc_ir;
pub mod machine_env;

use crate::ssa::ir::{Builder as SsaBuilder, IrInstruction, SsaValue, SsaVariable};
pub use machine_env::Target;
use regalloc_ir::LoweredInst;
use crate::xregalloc;
use std::collections::{HashMap};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct VarCopy {
    src: SsaVariable,
    dst: SsaVariable,
}

fn sequentialize_vars(parallel_copies: &[VarCopy], get_temp: impl FnMut() -> SsaVariable) -> Vec<VarCopy> {
    let pairs: Vec<(SsaVariable, SsaVariable)> = parallel_copies.iter()
        .map(|c| (c.src, c.dst))
        .collect();
    let seq = parallel_move::sequentialize(&pairs, get_temp);
    seq.into_iter()
        .map(|(src, dst)| VarCopy { src, dst })
        .collect()
}

pub fn lower_phis_to_parallel_moves(ssa: &mut SsaBuilder) {
    let num_blocks = ssa.blocks.len();
    let mut block_copies: Vec<Vec<VarCopy>> = vec![Vec::new(); num_blocks];
    
    for b in 0..num_blocks {
        let block = &ssa.blocks[b];
        let mut phis = Vec::new();
        for inst in &block.instrs {
            if let IrInstruction::PhiAssign(phi) = inst {
                phis.push(phi.clone());
            }
        }
        
        if phis.is_empty() {
            continue;
        }
        
        let predecessors = block.predecessors.clone();
        for pred in predecessors {
            let mut parallel_copies = Vec::new();
            for phi in &phis {
                if let Some(&(src_var, _)) = phi.operands.iter().find(|&&(_, from_b)| from_b == pred) {
                    parallel_copies.push(VarCopy {
                        src: src_var,
                        dst: phi.var,
                    });
                }
            }
            
            let seq = sequentialize_vars(&parallel_copies, || ssa.get_fresh_var());
            block_copies[pred].extend(seq);
        }
    }
    
    for b in 0..num_blocks {
        let copies = std::mem::take(&mut block_copies[b]);
        if copies.is_empty() {
            continue;
        }
        
        let block = &mut ssa.blocks[b];
        let has_terminator = block.instrs.last().map_or(false, |inst| {
            matches!(inst, IrInstruction::Jmp(_) | IrInstruction::Br(_, _, _) | IrInstruction::Ret(_))
        });
        
        let new_instrs: Vec<IrInstruction> = copies.into_iter()
            .map(|c| IrInstruction::Mov(c.dst, SsaValue::Var(c.src)))
            .collect();
            
        if has_terminator {
            let term = block.instrs.pop().unwrap();
            block.instrs.extend(new_instrs);
            block.instrs.push(term);
        } else {
            block.instrs.extend(new_instrs);
        }
    }
    
    for b in 0..num_blocks {
        ssa.blocks[b].instrs.retain(|inst| !inst.is_phi());
    }
}

/// Run the custom register allocator pipeline directly from SSA form:
///   SSA IR → xregalloc → lowered IR
pub fn run_regalloc(ssa: &SsaBuilder, target: Target) -> (usize, Vec<Vec<LoweredInst>>) {
    // Phase 1: Build the agnostic function adapter directly from SsaBuilder.
    let (agnostic_func, _mapper) = AgnosticFunc::new(ssa, target);

    // Phase 2: Run our custom machine-agnostic register allocator.
    let mut allocatable_regs = Vec::new();
    match target {
        Target::X86_64 => {
            for &r in machine_env::x86::CALLER_SAVED {
                allocatable_regs.push(xregalloc::PReg(r as u32));
            }
            for &r in machine_env::x86::CALLEE_SAVED {
                allocatable_regs.push(xregalloc::PReg(r as u32));
            }
        }
        Target::Aarch64 => {
            for &r in machine_env::aarch64::CALLER_SAVED {
                allocatable_regs.push(xregalloc::PReg(r as u32));
            }
            for &r in machine_env::aarch64::CALLEE_SAVED {
                allocatable_regs.push(xregalloc::PReg(r as u32));
            }
        }
    }

    let result = xregalloc::allocate(&agnostic_func, &allocatable_regs)
        .expect("register allocation failed");

    // Phase 3: Lower using AllocationResult.
    let mut lowered = Vec::new();
    for b in 0..ssa.blocks.len() {
        let mut block_insts = Vec::new();
        let range = agnostic_func.block_inst_ranges[b].clone();

        for flat_idx in range {
            // Edits before
            if let Some(edits) = result.edits_before.get(&flat_idx) {
                for edit in edits {
                    let from = map_alloc(edit.from);
                    let to = map_alloc(edit.to);
                    if from != to {
                        block_insts.push(LoweredInst::Mov(to, from));
                    }
                }
            }

            // The instruction itself (if not flat index 0)
            if flat_idx > 0 {
                let Some((block_idx, orig_idx)) = agnostic_func.flat_inst_map[flat_idx] else {
                    continue;
                };
                let orig_inst = &ssa.blocks[block_idx].instrs[orig_idx];
                if !orig_inst.is_phi() {
                    let allocs: Vec<regalloc_ir::Allocation> = result.inst_allocs[flat_idx]
                        .iter()
                        .map(|&a| map_alloc(a))
                        .collect();
                    let lowered_inst = lower_ssa_inst(orig_inst, &allocs);
                    block_insts.push(lowered_inst);
                }
            }

            // Edits after
            if let Some(edits) = result.edits_after.get(&flat_idx) {
                for edit in edits {
                    let from = map_alloc(edit.from);
                    let to = map_alloc(edit.to);
                    if from != to {
                        block_insts.push(LoweredInst::Mov(to, from));
                    }
                }
            }
        }

        lowered.push(block_insts);
    }

    (result.num_spillslots, lowered)
}

struct VarMapper {
    map: HashMap<SsaVariable, u32>,
    next: u32,
}

impl VarMapper {
    fn new() -> Self {
        Self {
            map: HashMap::new(),
            next: 0,
        }
    }

    fn get(&mut self, var: SsaVariable) -> u32 {
        *self.map.entry(var).or_insert_with(|| {
            let id = self.next;
            self.next += 1;
            id
        })
    }
}

struct AgnosticFunc {
    num_blocks: usize,
    block_inst_ranges: Vec<std::ops::Range<usize>>,
    succs: Vec<Vec<usize>>,
    preds: Vec<Vec<usize>>,
    inst_operands: Vec<Vec<xregalloc::Operand>>,
    inst_clobbers: Vec<Vec<xregalloc::PReg>>,
    scratch_regs: Vec<xregalloc::PReg>,
    num_vregs: usize,
    target: Target,
    flat_inst_map: Vec<Option<(usize, usize)>>,
    phi_info: Vec<Option<HashMap<usize, xregalloc::Var>>>,
    copy_info: Vec<bool>,
}

impl AgnosticFunc {
    fn new(ssa: &SsaBuilder, target: Target) -> (Self, VarMapper) {
        let mut mapper = VarMapper::new();
        let num_blocks = ssa.blocks.len();

        // Pre-register parameters
        for &param in &ssa.params {
            mapper.get(param);
        }

        // Pre-register all variables in the SSA builder
        for block in &ssa.blocks {
            for instr in &block.instrs {
                match instr {
                    IrInstruction::PhiAssign(phi) => {
                        mapper.get(phi.var);
                        for &(op, _) in &phi.operands {
                            mapper.get(op);
                        }
                    }
                    IrInstruction::Const(dst, _) => {
                        mapper.get(*dst);
                    }
                    IrInstruction::Mov(dst, src) => {
                        mapper.get(*dst);
                        if let SsaValue::Var(v) = src {
                            mapper.get(*v);
                        }
                    }
                    IrInstruction::Binary(_, dst, lhs, rhs) => {
                        mapper.get(*dst);
                        if let SsaValue::Var(v) = lhs { mapper.get(*v); }
                        if let SsaValue::Var(v) = rhs { mapper.get(*v); }
                    }
                    IrInstruction::Not(dst, src) => {
                        mapper.get(*dst);
                        if let SsaValue::Var(v) = src { mapper.get(*v); }
                    }
                    IrInstruction::Print(src) | IrInstruction::Ret(src) => {
                        if let SsaValue::Var(v) = src { mapper.get(*v); }
                    }
                    IrInstruction::Br(cond, _, _) => {
                        if let SsaValue::Var(v) = cond { mapper.get(*v); }
                    }
                    IrInstruction::Call { dest, args, .. } => {
                        if let Some(d) = dest {
                            mapper.get(*d);
                        }
                        for arg in args {
                            if let SsaValue::Var(v) = arg {
                                mapper.get(*v);
                            }
                        }
                    }
                    IrInstruction::Jmp(_) | IrInstruction::Nop => {}
                }
            }
        }

        // Build flat instructions list
        let mut flat_inst_map = Vec::new();
        let mut block_inst_ranges = vec![0..0; num_blocks];

        // Flat instruction 0 is the synthetic parameter definition Nop
        flat_inst_map.push(None);

        let mut inst_operands = Vec::new();
        let mut inst_clobbers = Vec::new();
        let mut phi_info = Vec::new();
        phi_info.push(None);
        let mut copy_info = Vec::new();
        copy_info.push(false);

        // Synthetic parameter instruction
        let mut param_ops = Vec::new();
        let arg_regs: Vec<xregalloc::PReg> = match target {
            Target::X86_64 => machine_env::x86::ARG_REGS.iter()
                .map(|&r| xregalloc::PReg(r as u32)).collect(),
            Target::Aarch64 => machine_env::aarch64::ARG_REGS.iter()
                .map(|&r| xregalloc::PReg(r as u32)).collect(),
        };
        for (i, &param) in ssa.params.iter().enumerate() {
            let var = xregalloc::Var(mapper.get(param));
            let constraint = if i < arg_regs.len() {
                xregalloc::Constraint::Fixed(arg_regs[i])
            } else {
                xregalloc::Constraint::Any
            };
            param_ops.push(xregalloc::Operand {
                var,
                constraint,
                kind: xregalloc::OperandKind::Def,
            });
        }
        inst_operands.push(param_ops);
        inst_clobbers.push(Vec::new());

        let mut current_flat_idx = 1;

        for b in 0..num_blocks {
            let block = &ssa.blocks[b];
            let start = current_flat_idx;

            // If it is block 0, it includes the synthetic parameter instruction at index 0!
            let range_start = if b == 0 { 0 } else { start };

            for (i, instr) in block.instrs.iter().enumerate() {
                flat_inst_map.push(Some((b, i)));
                let ops = get_inst_operands(instr, &mut mapper, target);
                inst_operands.push(ops);

                // Build clobbers
                let mut clobs = Vec::new();
                match instr {
                    IrInstruction::Print(_) => {
                        let caller_saved = match target {
                            Target::X86_64 => machine_env::x86::CALLER_SAVED,
                            Target::Aarch64 => machine_env::aarch64::CALLER_SAVED,
                        };
                        for &r in caller_saved {
                            clobs.push(xregalloc::PReg(r as u32));
                        }
                    }
                    IrInstruction::Call { dest, .. } => {
                        let caller_saved = match target {
                            Target::X86_64 => machine_env::x86::CALLER_SAVED,
                            Target::Aarch64 => machine_env::aarch64::CALLER_SAVED,
                        };
                        let ret_reg = match target {
                            Target::X86_64 => 0, // RAX
                            Target::Aarch64 => 0, // x0
                        };
                        for &r in caller_saved {
                            if dest.is_some() && r == ret_reg {
                                continue;
                            }
                            clobs.push(xregalloc::PReg(r as u32));
                        }
                    }
                    IrInstruction::Binary(crate::ssa::ir::BinaryOp::Div, _, _, _) if target == Target::X86_64 => {
                        clobs.push(xregalloc::PReg(2)); // RDX clobbered by idiv
                    }
                    _ => {}
                }
                inst_clobbers.push(clobs);

                // Build phi info
                if let IrInstruction::PhiAssign(phi) = instr {
                    let mut op_map = HashMap::new();
                    for &(op_var, pred_b) in &phi.operands {
                        op_map.insert(pred_b, xregalloc::Var(mapper.get(op_var)));
                    }
                    phi_info.push(Some(op_map));
                } else {
                    phi_info.push(None);
                }

                let is_mov = matches!(instr, IrInstruction::Mov(_, SsaValue::Var(_)));
                copy_info.push(is_mov);

                current_flat_idx += 1;
            }

            block_inst_ranges[b] = range_start..current_flat_idx;
        }

        let succs: Vec<Vec<usize>> = ssa.blocks.iter()
            .map(|b| b.successors.clone())
            .collect();

        let preds: Vec<Vec<usize>> = ssa.blocks.iter()
            .map(|b| b.predecessors.clone())
            .collect();

        let scratch_regs = match target {
            Target::X86_64 => vec![xregalloc::PReg(11), xregalloc::PReg(10)], // r11, r10
            Target::Aarch64 => vec![xregalloc::PReg(14), xregalloc::PReg(15)], // x14, x15
        };

        (
            Self {
                num_blocks,
                block_inst_ranges,
                succs,
                preds,
                inst_operands,
                inst_clobbers,
                scratch_regs,
                num_vregs: mapper.next as usize,
                target,
                flat_inst_map,
                phi_info,
                copy_info,
            },
            mapper,
        )
    }
}

impl xregalloc::AllocFunction for AgnosticFunc {
    fn num_blocks(&self) -> usize {
        self.num_blocks
    }

    fn block_instructions(&self, block: usize) -> std::ops::Range<usize> {
        self.block_inst_ranges[block].clone()
    }

    fn block_successors(&self, block: usize) -> &[usize] {
        &self.succs[block]
    }

    fn block_predecessors(&self, block: usize) -> &[usize] {
        &self.preds[block]
    }

    fn num_instructions(&self) -> usize {
        self.inst_operands.len()
    }

    fn inst_operands(&self, inst: usize) -> &[xregalloc::Operand] {
        &self.inst_operands[inst]
    }

    fn inst_clobbers(&self, inst: usize) -> &[xregalloc::PReg] {
        &self.inst_clobbers[inst]
    }

    fn num_vregs(&self) -> usize {
        self.num_vregs
    }

    fn scratch_regs(&self) -> &[xregalloc::PReg] {
        &self.scratch_regs
    }

    fn is_valid_combination(&self, inst: usize, allocs: &[xregalloc::Allocation]) -> bool {
        if self.target == Target::X86_64 {
            if inst == 0 {
                return true;
            }
            let original_info = &self.flat_inst_map[inst];
            let Some((_b, _i)) = original_info else { return true; };
            let mut stack_count = 0;
            for alloc in allocs {
                if matches!(alloc, xregalloc::Allocation::Stack(_)) {
                    stack_count += 1;
                }
            }
            stack_count <= 1
        } else {
            true
        }
    }

    fn is_phi(&self, inst: usize) -> bool {
        self.phi_info[inst].is_some()
    }

    fn phi_op(&self, inst: usize, pred: usize) -> xregalloc::Var {
        self.phi_info[inst].as_ref().unwrap()[&pred]
    }

    fn is_copy(&self, inst: usize) -> bool {
        self.copy_info[inst]
    }
}

fn get_inst_operands(inst: &IrInstruction, mapper: &mut VarMapper, target: Target) -> Vec<xregalloc::Operand> {
    let mut ops = Vec::new();
    match inst {
        IrInstruction::Const(dst, _) => {
            ops.push(xregalloc::Operand {
                var: xregalloc::Var(mapper.get(*dst)),
                constraint: xregalloc::Constraint::Any,
                kind: xregalloc::OperandKind::Def,
            });
        }
        IrInstruction::Mov(dst, src) => {
            ops.push(xregalloc::Operand {
                var: xregalloc::Var(mapper.get(*dst)),
                constraint: xregalloc::Constraint::Any,
                kind: xregalloc::OperandKind::Def,
            });
            if let SsaValue::Var(v) = src {
                ops.push(xregalloc::Operand {
                    var: xregalloc::Var(mapper.get(*v)),
                    constraint: xregalloc::Constraint::Any,
                    kind: xregalloc::OperandKind::Use,
                });
            }
        }
        IrInstruction::Binary(op, dst, lhs, rhs) => {
            let is_div = target == Target::X86_64 && *op == crate::ssa::ir::BinaryOp::Div;
            let is_mul = target == Target::X86_64 && *op == crate::ssa::ir::BinaryOp::Mul;

            let dst_constraint = if is_div {
                xregalloc::Constraint::Fixed(xregalloc::PReg(0)) // RAX
            } else if is_mul {
                // 2-operand `imul` requires a register destination.
                xregalloc::Constraint::Reg
            } else {
                xregalloc::Constraint::Any
            };

            ops.push(xregalloc::Operand {
                var: xregalloc::Var(mapper.get(*dst)),
                constraint: dst_constraint,
                kind: xregalloc::OperandKind::Def,
            });
            if let SsaValue::Var(v) = lhs {
                ops.push(xregalloc::Operand {
                    var: xregalloc::Var(mapper.get(*v)),
                    constraint: if is_div { xregalloc::Constraint::Fixed(xregalloc::PReg(0)) } else { xregalloc::Constraint::Any }, // RAX
                    kind: xregalloc::OperandKind::Use,
                });
            }
            if let SsaValue::Var(v) = rhs {
                ops.push(xregalloc::Operand {
                    var: xregalloc::Var(mapper.get(*v)),
                    constraint: xregalloc::Constraint::Any,
                    kind: xregalloc::OperandKind::Use,
                });
            }
        }
        IrInstruction::Not(dst, src) => {
            ops.push(xregalloc::Operand {
                var: xregalloc::Var(mapper.get(*dst)),
                constraint: xregalloc::Constraint::Any,
                kind: xregalloc::OperandKind::Def,
            });
            if let SsaValue::Var(v) = src {
                ops.push(xregalloc::Operand {
                    var: xregalloc::Var(mapper.get(*v)),
                    constraint: xregalloc::Constraint::Any,
                    kind: xregalloc::OperandKind::Use,
                });
            }
        }
        IrInstruction::Print(src) => {
            if let SsaValue::Var(v) = src {
                ops.push(xregalloc::Operand {
                    var: xregalloc::Var(mapper.get(*v)),
                    constraint: xregalloc::Constraint::Any,
                    kind: xregalloc::OperandKind::Use,
                });
            }
        }
        IrInstruction::Br(cond, _, _) => {
            if let SsaValue::Var(v) = cond {
                ops.push(xregalloc::Operand {
                    var: xregalloc::Var(mapper.get(*v)),
                    constraint: xregalloc::Constraint::Any,
                    kind: xregalloc::OperandKind::Use,
                });
            }
        }
        IrInstruction::Ret(src) => {
            if let SsaValue::Var(v) = src {
                ops.push(xregalloc::Operand {
                    var: xregalloc::Var(mapper.get(*v)),
                    constraint: xregalloc::Constraint::Any,
                    kind: xregalloc::OperandKind::Use,
                });
            }
        }
        IrInstruction::Call { dest, args, .. } => {
            let ret_reg = match target {
                Target::X86_64 => xregalloc::PReg(0), // RAX
                Target::Aarch64 => xregalloc::PReg(0), // x0
            };
            if let Some(d) = dest {
                ops.push(xregalloc::Operand {
                    var: xregalloc::Var(mapper.get(*d)),
                    constraint: xregalloc::Constraint::Fixed(ret_reg),
                    kind: xregalloc::OperandKind::Def,
                });
            }

            let arg_regs: Vec<xregalloc::PReg> = match target {
                Target::X86_64 => machine_env::x86::ARG_REGS.iter()
                    .map(|&r| xregalloc::PReg(r as u32)).collect(),
                Target::Aarch64 => machine_env::aarch64::ARG_REGS.iter()
                    .map(|&r| xregalloc::PReg(r as u32)).collect(),
            };

            let mut arg_idx = 0;
            for arg in args {
                if let SsaValue::Var(v) = arg {
                    let constraint = if arg_idx < arg_regs.len() {
                        xregalloc::Constraint::Fixed(arg_regs[arg_idx])
                    } else {
                        xregalloc::Constraint::Any
                    };
                    ops.push(xregalloc::Operand {
                        var: xregalloc::Var(mapper.get(*v)),
                        constraint,
                        kind: xregalloc::OperandKind::Use,
                    });
                    arg_idx += 1;
                }
            }
        }
        IrInstruction::PhiAssign(phi) => {
            ops.push(xregalloc::Operand {
                var: xregalloc::Var(mapper.get(phi.var)),
                constraint: xregalloc::Constraint::Any,
                kind: xregalloc::OperandKind::Def,
            });
        }
        IrInstruction::Jmp(_) | IrInstruction::Nop => {}
    }
    ops
}

fn map_alloc(alloc: xregalloc::Allocation) -> regalloc_ir::Allocation {
    match alloc {
        xregalloc::Allocation::Reg(xregalloc::PReg(hw)) => {
            regalloc_ir::Allocation::Reg(regalloc_ir::PhysReg::new(hw as usize))
        }
        xregalloc::Allocation::Stack(xregalloc::SpillSlot(idx)) => {
            regalloc_ir::Allocation::Stack(regalloc_ir::SpillSlot::new(idx as usize))
        }
    }
}

fn lower_ssa_inst(
    inst: &IrInstruction,
    allocs: &[regalloc_ir::Allocation],
) -> LoweredInst {
    match inst {
        IrInstruction::Const(_, val) => {
            let imm = match val {
                SsaValue::Int(i) => *i,
                SsaValue::Bool(b) => if *b { 1 } else { 0 },
                _ => 0,
            };
            LoweredInst::LoadConst(allocs[0], imm)
        }
        IrInstruction::Mov(_, src) => {
            match src {
                SsaValue::Var(_) => {
                    LoweredInst::Mov(allocs[0], allocs[1])
                }
                SsaValue::Int(imm) => {
                    LoweredInst::LoadConst(allocs[0], *imm)
                }
                SsaValue::Bool(b) => {
                    LoweredInst::LoadConst(allocs[0], if *b { 1 } else { 0 })
                }
                SsaValue::Undef => {
                    LoweredInst::Nop
                }
            }
        }
        IrInstruction::Binary(op, _, _, _) => {
            LoweredInst::BinaryOp(*op, allocs[0], allocs[1], allocs[2])
        }
        IrInstruction::Not(_, _) => {
            LoweredInst::Not(allocs[0], allocs[1])
        }
        IrInstruction::Print(_) => {
            LoweredInst::Print(allocs[0])
        }
        IrInstruction::Jmp(target) => {
            LoweredInst::Jmp(*target)
        }
        IrInstruction::Br(_, then_bb, else_bb) => {
            LoweredInst::Br(allocs[0], *then_bb, *else_bb)
        }
        IrInstruction::Ret(_) => {
            LoweredInst::Ret(allocs[0])
        }
        IrInstruction::Call { callee_bb, .. } => {
            LoweredInst::Call(*callee_bb)
        }
        IrInstruction::Nop => {
            LoweredInst::Nop
        }
        IrInstruction::PhiAssign(_) => {
            unreachable!("Phi should not be lowered directly")
        }
    }
}
