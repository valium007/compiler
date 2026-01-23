use std::collections::BTreeSet;

use crate::ir::ssa::SSA;
use crate::ir::id::*;

use crate::ir::{
    ssa::Variable,
    brilir::{self},
    ssa,
};
use hashbrown::HashMap;

pub fn to_ssa(builder: &brilir::BrilBuilder) -> SSA {
    let mut ssa_builder = SSA::new();
    let var_to_id = vars_to_ids(&builder.blocks);
    for block in builder.blocks.iter() {
        let mut current_block =
            ssa::BasicBlock::new(*builder.label_to_id.get(&block.label).unwrap());
        for bril_inst in block.body.iter() {
            let ssa_inst = inst_ssa(bril_inst, &var_to_id, &builder.label_to_id);
            current_block.instructions.push(ssa_inst);
        }
        ssa_builder.blocks.push(current_block);
    }
    ssa_builder
}

macro_rules! ssa_binop {
    ($binop:ident, $inst:ident, $var_to_id:ident) => {{
        let dst = ssa::Value::Variable(*$var_to_id.get(&$inst.dest).unwrap());
        let lhs = ssa::Value::Variable(*$var_to_id.get(&$inst.args[0]).unwrap());
        let rhs = ssa::Value::Variable(*$var_to_id.get(&$inst.args[1]).unwrap());

        return ssa::IrInstruction::IrBinOp(ssa::BinOp {
            op: ssa::BinaryOp::$binop,
            dst,
            lhs,
            rhs,
        });
    }};
}

pub fn inst_ssa(
    bril_inst: &brilir::Inst,
    var_to_id: &HashMap<String, Variable>,
    label_to_id: &HashMap<String, BasicBlockId>,
) -> ssa::IrInstruction {
    let ssa_inst: ssa::IrInstruction = match bril_inst {
        brilir::Inst::Op(op) => match op {
            brilir::Op::Load(inst) => {
                let dst = ssa::Value::Variable(*var_to_id.get(&inst.dest).unwrap());
                let src = as_ir_value(&inst.value);
                return ssa::IrInstruction::IrLoad(ssa::Load { dst, src });
            }

            brilir::Op::Add(inst) => ssa_binop!(Add, inst, var_to_id),
            brilir::Op::Sub(inst) => ssa_binop!(Sub, inst, var_to_id),
            brilir::Op::Mul(inst) => ssa_binop!(Mul, inst, var_to_id),
            brilir::Op::Lt(inst) => ssa_binop!(Lt, inst, var_to_id),

            brilir::Op::Br(inst) => {
                let dst = ssa::Value::Variable(*var_to_id.get(&inst.args[0]).unwrap());
                return ssa::IrInstruction::IrBr(ssa::Br {
                    dst,
                    truthy: *label_to_id.get(&inst.labels[0]).unwrap(),
                    falsy: *label_to_id.get(&inst.labels[1]).unwrap(),
                });
            }

            brilir::Op::Jmp(inst) => ssa::IrInstruction::IrJmp(ssa::Jmp {
                bb: *label_to_id.get(&inst.labels[0]).unwrap(),
            }),

            brilir::Op::Print(inst) => ssa::IrInstruction::IrPrint(ssa::Print {
                src: ssa::Value::Variable(*var_to_id.get(&inst.args[0]).unwrap()),
            }),
        },
        _ => ssa::IrInstruction::Nop,
    };
    ssa_inst
}

fn as_ir_value(value: &brilir::BrilValue) -> ssa::Value {
    match value {
        brilir::BrilValue::Int(i) => ssa::Value::Immediate(*i),
        brilir::BrilValue::Bool(b) => ssa::Value::Bool(*b),
        _ => unreachable!(),
    }
}

macro_rules! get_vars {
    ($inst:ident, $vars:ident) => {{
        $vars.insert($inst.dest.clone());
        $vars.insert($inst.args[0].clone());
        $vars.insert($inst.args[1].clone());
    }};
}

pub fn vars_to_ids(blocks: &Vec<brilir::BasicBlock>) -> HashMap<String, Variable> {
    let mut var_to_id: HashMap<String, Variable> = HashMap::new();
    let mut vars = BTreeSet::new();

    for block in blocks.iter() {
        for inst in block.body.iter() {
            match inst {
                brilir::Inst::Label { .. } => unreachable!(),
                brilir::Inst::Op(op) => match op {
                    brilir::Op::Load(inst) => {
                        vars.insert(inst.dest.clone());
                    }
                    brilir::Op::Add(inst) => get_vars!(inst, vars),
                    brilir::Op::Sub(inst) => get_vars!(inst, vars),
                    brilir::Op::Mul(inst) => get_vars!(inst, vars),
                    brilir::Op::Lt(inst) => get_vars!(inst, vars),
                    brilir::Op::Br(inst) => {
                        vars.insert(inst.args[0].clone());
                    }
                    brilir::Op::Jmp(..) => {}
                    brilir::Op::Print(inst) => {
                        vars.insert(inst.args[0].clone());
                    }
                },
            }
        }
    }

    for (i, var) in vars.iter().enumerate() {
        var_to_id.insert(var.clone(), Variable { id: i, index: 0 });
    }
    var_to_id
}
