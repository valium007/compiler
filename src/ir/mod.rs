use std::collections::BTreeSet;

use hashbrown::HashMap;

use crate::ir::{
    id::BasicBlockId,
    ssa::{BasicBlock, BinOp, SSA, Variable},
};
pub mod brilir;
pub mod dominator;
pub mod hir;
pub mod id;
pub mod ssa;
pub mod passes;

pub fn to_ssa(blocks: &Vec<hir::BasicBlock>, label_to_id: &HashMap<String, BasicBlockId>) -> SSA {
    let mut ssa_builder = SSA::new();
    let var_to_id = vars_to_ids(blocks);

    for block in blocks.iter() {
        let mut bb = BasicBlock::new(block.id);
        for inst in block.body.iter() {
            let ssa_inst = inst_ssa(inst, &var_to_id, &label_to_id);
            bb.instructions.push(ssa_inst);
        }
        ssa_builder.blocks.push(bb);
    }
    ssa_builder
}

pub fn vars_to_ids(blocks: &Vec<hir::BasicBlock>) -> HashMap<String, Variable> {
    let mut set = BTreeSet::new();
    for block in blocks.iter() {
        for inst in block.body.iter() {
            match inst {
                hir::HIRInstruction::BinOp { dst, lhs, rhs, ..} => {
                    set.insert(dst.clone());
                    set.insert(lhs.clone());
                    set.insert(rhs.clone());
                }
                hir::HIRInstruction::Br { args,.. } => {
                    set.insert(args[0].clone());
                }
                hir::HIRInstruction::Load { dst, .. } => {
                    set.insert(dst.clone());
                }
                _ => {}
            }
        }
    }
    let mut var_to_id: HashMap<String, Variable> = HashMap::new();

    for (i, v) in set.iter().enumerate() {
        let var = Variable { id: i, index: 0 };
        var_to_id.insert(v.to_string(), var);
    }
    var_to_id
}

pub fn inst_ssa(
    hir_inst: &hir::HIRInstruction,
    var_to_id: &HashMap<String, Variable>,
    label_to_id: &HashMap<String, BasicBlockId>,
) -> ssa::IrInstruction {
    let inst: ssa::IrInstruction = match hir_inst {
        hir::HIRInstruction::BinOp { op, dst, lhs, rhs } => {
            let dst = ssa::Value::Variable(*var_to_id.get(dst).unwrap());
            let lhs = ssa::Value::Variable(*var_to_id.get(lhs).unwrap());
            let rhs = ssa::Value::Variable(*var_to_id.get(rhs).unwrap());

            let binop = match op {
                hir::BinaryOp::Add => ssa::BinaryOp::Add,
                hir::BinaryOp::Sub => ssa::BinaryOp::Sub,
                hir::BinaryOp::Mul => ssa::BinaryOp::Mul,
                hir::BinaryOp::Div => ssa::BinaryOp::Div,
                hir::BinaryOp::Lt => ssa::BinaryOp::Lt,
            };

            return ssa::IrInstruction::IrBinOp(BinOp {
                op: binop,
                dst,
                lhs,
                rhs,
            });
        }
        hir::HIRInstruction::Load { dst, value, .. } => {
            let dst = ssa::Value::Variable(*var_to_id.get(dst).unwrap());
            let src = as_ir_value(value);
            return ssa::IrInstruction::IrLoad(ssa::Load { dst, src });
        }

        hir::HIRInstruction::Br { args, labels } => {
            let dst = ssa::Value::Variable(*var_to_id.get(&args[0]).unwrap());
            return ssa::IrInstruction::IrBr(ssa::Br {
                dst,
                truthy: *label_to_id.get(&labels[0]).unwrap(),
                falsy: *label_to_id.get(&labels[1]).unwrap(),
            });
        }
        hir::HIRInstruction::Jmp { labels } => ssa::IrInstruction::IrJmp(ssa::Jmp {
            bb: *label_to_id.get(&labels[0]).unwrap(),
        }),
    };
    inst
}

fn as_ir_value(value: &hir::HIRValue) -> ssa::Value {
    match value {
        hir::HIRValue::Int(i) => ssa::Value::Immediate(*i),
        hir::HIRValue::Bool(b) => ssa::Value::Bool(*b),
        _ => unreachable!(),
    }
}
