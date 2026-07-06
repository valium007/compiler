//! Adapter for the Braun-style allocator (`bregalloc`).
//!
//! Plug-and-play counterpart to the xregalloc adapter (the `AgnosticFunc`
//! struct in this module's parent). Uses **only** `bregalloc` types — no
//! reference to `xregalloc` anywhere.

use std::collections::HashMap;

use crate::bregalloc;
use crate::regalloc::machine_env;
use crate::regalloc::regalloc_ir::{self, LoweredInst};
use crate::regalloc::Target;
use crate::ssa::ir::{Builder as SsaBuilder, IrInstruction, SsaValue, SsaVariable};

pub fn run_regalloc_b(
    ssa: &SsaBuilder,
    target: Target,
    check: bool,
) -> (usize, Vec<Vec<LoweredInst>>) {
    let (agnostic, mapper) = BAgnosticFunc::new(ssa, target);

    // Build the allocatable pool by converting machine_env's bregalloc-typed
    // constants. machine_env uses bregalloc::PReg's xregalloc twin — both
    // are { class, index } with identical layout but distinct types, so we
    // convert via hardware encoding.
    let mut allocatable: Vec<bregalloc::PReg> = Vec::new();
    let (caller, callee) = match target {
        Target::X86_64 => (machine_env::x86::CALLER_SAVED, machine_env::x86::CALLEE_SAVED),
        Target::Aarch64 => (machine_env::aarch64::CALLER_SAVED, machine_env::aarch64::CALLEE_SAVED),
    };
    for &p in caller.iter().chain(callee.iter()) {
        allocatable.push(bregalloc::PReg::int(p.index));
    }

    let result = bregalloc::allocate(&agnostic, &allocatable)
        .expect("bregalloc allocation failed");

    if check {
        // Secondary check first: if two simultaneously-live vregs share a
        // Reg home, the bug is in assignment/liveness, not output. Report
        // separately for clarity.
        if let Err(e) = bregalloc::checker::verify_homes_disjoint(&agnostic, &result) {
            eprintln!("homes-disjoint check FAILED (assignment-phase bug): {e}");
        }
        if let Err(e) = bregalloc::checker::verify(&agnostic, &result) {
            bregalloc::checker::dump_context(&agnostic, &result, &e);
            panic!("bregalloc symbolic checker rejected allocation: {e}");
        }
        eprintln!("bregalloc symbolic checker: ok ({} insts)", result.inst_allocs.len());
    }

    // Lower using AllocationResult — identical shape to xregalloc's path.
    let mut lowered: Vec<Vec<LoweredInst>> = Vec::new();
    for b in 0..ssa.blocks.len() {
        let mut block_insts = Vec::new();
        for flat_idx in agnostic.block_inst_ranges[b].clone() {
            if let Some(edits) = result.edits_before.get(&flat_idx) {
                for edit in edits {
                    let from = map_alloc(edit.from);
                    let to = map_alloc(edit.to);
                    if from != to {
                        block_insts.push(LoweredInst::Mov(to, from));
                    }
                }
            }
            if flat_idx > 0 {
                let Some((block_idx, orig_idx)) = agnostic.flat_inst_map[flat_idx] else {
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

    let _ = mapper;
    (result.num_spillslots, lowered)
}

fn map_alloc(a: bregalloc::Allocation) -> regalloc_ir::Allocation {
    match a {
        bregalloc::Allocation::Reg(bregalloc::PReg { index, .. }) => {
            regalloc_ir::Allocation::Reg(regalloc_ir::PhysReg::new(index as usize))
        }
        bregalloc::Allocation::Stack(bregalloc::SpillSlot(idx)) => {
            regalloc_ir::Allocation::Stack(regalloc_ir::SpillSlot::new(idx as usize))
        }
    }
}

struct VarMapper {
    map: HashMap<SsaVariable, u32>,
    next: u32,
}

impl VarMapper {
    fn new() -> Self { Self { map: HashMap::new(), next: 0 } }
    fn get(&mut self, var: SsaVariable) -> u32 {
        *self.map.entry(var).or_insert_with(|| {
            let id = self.next;
            self.next += 1;
            id
        })
    }
}

struct BAgnosticFunc {
    num_blocks: usize,
    block_inst_ranges: Vec<std::ops::Range<usize>>,
    succs: Vec<Vec<usize>>,
    preds: Vec<Vec<usize>>,
    inst_operands: Vec<Vec<bregalloc::Operand>>,
    inst_clobbers: Vec<Vec<bregalloc::PReg>>,
    inst_implicit_reads: Vec<Vec<bregalloc::PReg>>,
    scratch_regs: Vec<bregalloc::PReg>,
    num_vregs: usize,
    flat_inst_map: Vec<Option<(usize, usize)>>,
    phi_info: Vec<Option<HashMap<usize, bregalloc::Var>>>,
    copy_info: Vec<bool>,
    const_info: Vec<bool>,
    loop_depths: Vec<u32>,
}

impl BAgnosticFunc {
    fn new(ssa: &SsaBuilder, target: Target) -> (Self, VarMapper) {
        let mut mapper = VarMapper::new();
        let num_blocks = ssa.blocks.len();

        for &param in &ssa.params {
            mapper.get(param);
        }
        for block in &ssa.blocks {
            for instr in &block.instrs {
                match instr {
                    // Generic operand walk via the new visitor helpers. Defs
                    // are statically SsaValue but always hold Var; uses can be
                    // Var/Int/Bool/Undef.
                    inst => {
                        for d in inst.defs() {
                            mapper.get(d.expect_var());
                        }
                        for u in inst.uses() {
                            if let SsaValue::Var(v) = u { mapper.get(*v); }
                        }
                    }
                }
            }
        }

        let mut inst_operands: Vec<Vec<bregalloc::Operand>> = Vec::new();
        let mut inst_clobbers: Vec<Vec<bregalloc::PReg>> = Vec::new();
        let mut inst_implicit_reads: Vec<Vec<bregalloc::PReg>> = Vec::new();
        let mut flat_inst_map: Vec<Option<(usize, usize)>> = Vec::new();
        let mut phi_info: Vec<Option<HashMap<usize, bregalloc::Var>>> = Vec::new();
        let mut copy_info: Vec<bool> = Vec::new();
        let mut const_info: Vec<bool> = Vec::new();
        let mut block_inst_ranges = vec![0..0usize; num_blocks];

        // Slot 0 is a synthetic parameter-def instruction (mirrors AgnosticFunc).
        let mut param_ops: Vec<bregalloc::Operand> = Vec::new();
        let arg_regs = match target {
            Target::X86_64 => machine_env::x86::ARG_REGS,
            Target::Aarch64 => machine_env::aarch64::ARG_REGS,
        };
        for (i, &param) in ssa.params.iter().enumerate() {
            let var = bregalloc::Var::int(mapper.get(param));
            let constraint = if i < arg_regs.len() {
                bregalloc::Constraint::Fixed(bregalloc::PReg::int(arg_regs[i].index))
            } else {
                bregalloc::Constraint::Any
            };
            param_ops.push(bregalloc::Operand {
                var,
                constraint,
                kind: bregalloc::OperandKind::Def,
            });
        }
        inst_operands.push(param_ops);
        inst_clobbers.push(Vec::new());
        inst_implicit_reads.push(Vec::new());
        flat_inst_map.push(None);
        phi_info.push(None);
        copy_info.push(false);
        const_info.push(false);

        let mut current_flat_idx = 1usize;
        for b in 0..num_blocks {
            let block = &ssa.blocks[b];
            let start = current_flat_idx;
            let range_start = if b == 0 { 0 } else { start };
            for (i, instr) in block.instrs.iter().enumerate() {
                flat_inst_map.push(Some((b, i)));
                let ops = build_operands(instr, &mut mapper, target);
                inst_operands.push(ops);

                let mut clobs = Vec::new();
                match instr {
                    IrInstruction::Print(_) => {
                        let caller_saved = match target {
                            Target::X86_64 => machine_env::x86::CALLER_SAVED,
                            Target::Aarch64 => machine_env::aarch64::CALLER_SAVED,
                        };
                        for &p in caller_saved {
                            clobs.push(bregalloc::PReg::int(p.index));
                        }
                    }
                    IrInstruction::Call { dest, .. } => {
                        let caller_saved = match target {
                            Target::X86_64 => machine_env::x86::CALLER_SAVED,
                            Target::Aarch64 => machine_env::aarch64::CALLER_SAVED,
                        };
                        let ret = match target {
                            Target::X86_64 => machine_env::x86::RETURN_REG,
                            Target::Aarch64 => machine_env::aarch64::RETURN_REG,
                        };
                        for &p in caller_saved {
                            if dest.is_some() && p == ret { continue; }
                            clobs.push(bregalloc::PReg::int(p.index));
                        }
                    }
                    IrInstruction::Binary(crate::ssa::ir::BinaryOp::Div, _, _, _)
                        if target == Target::X86_64 =>
                    {
                        clobs.push(bregalloc::PReg::int(machine_env::x86::RDX.index));
                    }
                    _ => {}
                }
                inst_clobbers.push(clobs);

                // Implicit reads: registers consumed by the instruction
                // before named operands take effect. x86 idiv reads RDX as the
                // high half of the dividend; cqo/xor-rdx setup runs before the
                // instruction reads any named operand.
                let impl_reads: Vec<bregalloc::PReg> = match instr {
                    IrInstruction::Binary(crate::ssa::ir::BinaryOp::Div, _, _, _)
                        if target == Target::X86_64 =>
                    {
                        vec![bregalloc::PReg::int(machine_env::x86::RDX.index)]
                    }
                    _ => Vec::new(),
                };
                inst_implicit_reads.push(impl_reads);

                if let IrInstruction::PhiAssign(phi) = instr {
                    let mut op_map = HashMap::new();
                    for (op_val, pred_b) in &phi.operands {
                        op_map.insert(
                            *pred_b,
                            bregalloc::Var::int(mapper.get(op_val.expect_var())),
                        );
                    }
                    phi_info.push(Some(op_map));
                } else {
                    phi_info.push(None);
                }
                copy_info.push(matches!(instr, IrInstruction::Mov(_, SsaValue::Var(_))));
                const_info.push(matches!(
                    instr,
                    IrInstruction::Const(_, _)
                        | IrInstruction::Mov(_, SsaValue::Int(_))
                        | IrInstruction::Mov(_, SsaValue::Bool(_))
                ));
                current_flat_idx += 1;
            }
            block_inst_ranges[b] = range_start..current_flat_idx;
        }

        let succs: Vec<Vec<usize>> = ssa.blocks.iter().map(|b| b.successors.clone()).collect();
        let preds: Vec<Vec<usize>> = ssa.blocks.iter().map(|b| b.predecessors.clone()).collect();
        let scratch_regs: Vec<bregalloc::PReg> = match target {
            Target::X86_64 => machine_env::x86::SCRATCH_REGS,
            Target::Aarch64 => machine_env::aarch64::SCRATCH_REGS,
        }
        .iter()
        .map(|p| bregalloc::PReg::int(p.index))
        .collect();

        // Loop depths: derived from petgraph's dominator-based back-edge
        // detection inside bregalloc::cfg::Cfg.
        let loop_depths = {
            let cfg = bregalloc::cfg::Cfg::build_from_slices(num_blocks, &succs);
            cfg.loop_depths()
        };

        (
            Self {
                num_blocks,
                block_inst_ranges,
                succs,
                preds,
                inst_operands,
                inst_clobbers,
                inst_implicit_reads,
                scratch_regs,
                num_vregs: mapper.next as usize,
                flat_inst_map,
                phi_info,
                copy_info,
                const_info,
                loop_depths,
            },
            mapper,
        )
    }
}

impl bregalloc::AllocFunction for BAgnosticFunc {
    fn num_blocks(&self) -> usize { self.num_blocks }
    fn block_instructions(&self, block: usize) -> std::ops::Range<usize> { self.block_inst_ranges[block].clone() }
    fn block_successors(&self, block: usize) -> &[usize] { &self.succs[block] }
    fn block_predecessors(&self, block: usize) -> &[usize] { &self.preds[block] }
    fn num_instructions(&self) -> usize { self.inst_operands.len() }
    fn inst_operands(&self, inst: usize) -> &[bregalloc::Operand] { &self.inst_operands[inst] }
    fn inst_clobbers(&self, inst: usize) -> &[bregalloc::PReg] { &self.inst_clobbers[inst] }
    fn inst_implicit_reads(&self, inst: usize) -> &[bregalloc::PReg] { &self.inst_implicit_reads[inst] }
    fn num_vregs(&self) -> usize { self.num_vregs }
    fn scratch_regs(&self) -> &[bregalloc::PReg] { &self.scratch_regs }
    fn is_phi(&self, inst: usize) -> bool { self.phi_info[inst].is_some() }
    fn phi_op(&self, inst: usize, pred: usize) -> bregalloc::Var {
        self.phi_info[inst].as_ref().unwrap()[&pred]
    }
    fn is_copy(&self, inst: usize) -> bool { self.copy_info[inst] }
    fn is_const(&self, inst: usize) -> bool { self.const_info[inst] }
    fn loop_depth(&self, block: usize) -> u32 { self.loop_depths[block] }
}

// Type alias removed — `machine_env`'s constants are `xregalloc::PReg`-typed,
// but the adapter only reads their `.index` field (a `u8`), so no xregalloc
// type bleeds into `bregalloc`.

fn build_operands(
    inst: &IrInstruction,
    mapper: &mut VarMapper,
    target: Target,
) -> Vec<bregalloc::Operand> {
    use bregalloc::{Constraint, Operand, OperandKind, PReg, Var};
    let mut ops = Vec::new();
    match inst {
        IrInstruction::Const(dst, _) => {
            ops.push(Operand {
                var: Var::int(mapper.get(dst.expect_var())),
                constraint: Constraint::Any,
                kind: OperandKind::Def,
            });
        }
        IrInstruction::Mov(dst, src) => {
            ops.push(Operand {
                var: Var::int(mapper.get(dst.expect_var())),
                constraint: Constraint::Any,
                kind: OperandKind::Def,
            });
            if let SsaValue::Var(v) = src {
                ops.push(Operand {
                    var: Var::int(mapper.get(*v)),
                    constraint: Constraint::Any,
                    kind: OperandKind::Use,
                });
            }
        }
        IrInstruction::Binary(op, dst, lhs, rhs) => {
            let is_div = matches!(op, crate::ssa::ir::BinaryOp::Div);
            let is_mul = matches!(op, crate::ssa::ir::BinaryOp::Mul);
            let dst_constraint = if is_div && target == Target::X86_64 {
                Constraint::Fixed(PReg::int(machine_env::x86::RAX.index))
            } else if is_mul && target == Target::X86_64 {
                Constraint::Reg
            } else {
                Constraint::Any
            };
            ops.push(Operand {
                var: Var::int(mapper.get(dst.expect_var())),
                constraint: dst_constraint,
                kind: OperandKind::Def,
            });
            if let SsaValue::Var(v) = lhs {
                ops.push(Operand {
                    var: Var::int(mapper.get(*v)),
                    constraint: if is_div && target == Target::X86_64 {
                        Constraint::Fixed(PReg::int(machine_env::x86::RAX.index))
                    } else {
                        Constraint::Any
                    },
                    kind: OperandKind::Use,
                });
            }
            if let SsaValue::Var(v) = rhs {
                ops.push(Operand {
                    var: Var::int(mapper.get(*v)),
                    constraint: Constraint::Any,
                    kind: OperandKind::Use,
                });
            }
        }
        IrInstruction::Not(dst, src) => {
            ops.push(Operand {
                var: Var::int(mapper.get(dst.expect_var())),
                constraint: Constraint::Any,
                kind: OperandKind::Def,
            });
            if let SsaValue::Var(v) = src {
                ops.push(Operand {
                    var: Var::int(mapper.get(*v)),
                    constraint: Constraint::Any,
                    kind: OperandKind::Use,
                });
            }
        }
        IrInstruction::Print(src) => {
            if let SsaValue::Var(v) = src {
                ops.push(Operand {
                    var: Var::int(mapper.get(*v)),
                    constraint: Constraint::Fixed(PReg::int(match target {
                        Target::X86_64 => machine_env::x86::RSI.index,
                        Target::Aarch64 => machine_env::aarch64::X0.index,
                    })),
                    kind: OperandKind::Use,
                });
            }
        }
        IrInstruction::Br(cond, _, _) => {
            if let SsaValue::Var(v) = cond {
                ops.push(Operand {
                    var: Var::int(mapper.get(*v)),
                    constraint: Constraint::Any,
                    kind: OperandKind::Use,
                });
            }
        }
        IrInstruction::Ret(src) => {
            if let SsaValue::Var(v) = src {
                ops.push(Operand {
                    var: Var::int(mapper.get(*v)),
                    constraint: Constraint::Fixed(PReg::int(match target {
                        Target::X86_64 => machine_env::x86::RETURN_REG.index,
                        Target::Aarch64 => machine_env::aarch64::RETURN_REG.index,
                    })),
                    kind: OperandKind::Use,
                });
            }
        }
        IrInstruction::Call { dest, args, .. } => {
            let ret_reg = match target {
                Target::X86_64 => machine_env::x86::RETURN_REG.index,
                Target::Aarch64 => machine_env::aarch64::RETURN_REG.index,
            };
            if let Some(d) = dest {
                ops.push(Operand {
                    var: Var::int(mapper.get(d.expect_var())),
                    constraint: Constraint::Fixed(PReg::int(ret_reg)),
                    kind: OperandKind::Def,
                });
            }
            let arg_regs = match target {
                Target::X86_64 => machine_env::x86::ARG_REGS,
                Target::Aarch64 => machine_env::aarch64::ARG_REGS,
            };
            let mut arg_idx = 0;
            for arg in args {
                if let SsaValue::Var(v) = arg {
                    let constraint = if arg_idx < arg_regs.len() {
                        Constraint::Fixed(PReg::int(arg_regs[arg_idx].index))
                    } else {
                        Constraint::Any
                    };
                    ops.push(Operand {
                        var: Var::int(mapper.get(*v)),
                        constraint,
                        kind: OperandKind::Use,
                    });
                    arg_idx += 1;
                }
            }
        }
        IrInstruction::PhiAssign(phi) => {
            ops.push(Operand {
                var: Var::int(mapper.get(phi.var.expect_var())),
                constraint: Constraint::Any,
                kind: OperandKind::Def,
            });
        }
        IrInstruction::Jmp(_) | IrInstruction::Nop => {}
    }
    ops
}

fn lower_ssa_inst(inst: &IrInstruction, allocs: &[regalloc_ir::Allocation]) -> LoweredInst {
    match inst {
        IrInstruction::Const(_, val) => {
            let imm = match val {
                SsaValue::Int(i) => *i,
                SsaValue::Bool(b) => if *b { 1 } else { 0 },
                _ => 0,
            };
            LoweredInst::LoadConst(allocs[0], imm)
        }
        IrInstruction::Mov(_, src) => match src {
            SsaValue::Var(_) => LoweredInst::Mov(allocs[0], allocs[1]),
            SsaValue::Int(imm) => LoweredInst::LoadConst(allocs[0], *imm),
            SsaValue::Bool(b) => LoweredInst::LoadConst(allocs[0], if *b { 1 } else { 0 }),
            SsaValue::Undef => LoweredInst::Nop,
        },
        IrInstruction::Binary(op, _, _, _) => LoweredInst::BinaryOp(*op, allocs[0], allocs[1], allocs[2]),
        IrInstruction::Not(_, _) => LoweredInst::Not(allocs[0], allocs[1]),
        IrInstruction::Print(_) => LoweredInst::Print(allocs[0]),
        IrInstruction::Jmp(target) => LoweredInst::Jmp(*target),
        IrInstruction::Br(_, then_bb, else_bb) => LoweredInst::Br(allocs[0], *then_bb, *else_bb),
        IrInstruction::Ret(_) => LoweredInst::Ret(allocs[0]),
        IrInstruction::Call { callee_bb, .. } => LoweredInst::Call(*callee_bb),
        IrInstruction::Nop => LoweredInst::Nop,
        IrInstruction::PhiAssign(_) => unreachable!("Phi should not be lowered directly"),
    }
}
