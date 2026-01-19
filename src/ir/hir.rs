use crate::ir::brilir::{self, BrilValue, Inst};
use crate::ir::id::*;

#[derive(Debug)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Lt,
}

#[derive(Debug)]
pub enum UnaryOp {
    Load,
}

#[derive(Debug)]
pub enum HIRValue {
    Variable(String),
    Int(i64),
    Bool(bool),
}

#[derive(Debug)]
pub enum HIRInstruction {
    BinOp {
        op: BinaryOp,
        dst: String,
        lhs: String,
        rhs: String,
    },

    Load {
        dst: String,
        value: HIRValue,
        typ: String,
    },
    Jmp {
        labels: Vec<String>,
    },
    Br {
        args: Vec<String>,
        labels: [String; 2],
    },
}

fn value_hir(value: &brilir::BrilValue) -> HIRValue {
    match value {
        BrilValue::Int(i) => HIRValue::Int(*i),
        BrilValue::Bool(b) => HIRValue::Bool(*b),
        BrilValue::Variable(v) => HIRValue::Variable(v.to_string()),
    }
}

fn binop_hir(op: &brilir::Op) -> HIRInstruction {
    match op {
        brilir::Op::Add(add) => HIRInstruction::BinOp {
            op: BinaryOp::Add,
            dst: add.dest.clone(),
            lhs: add.args[0].clone(),
            rhs: add.args[1].clone(),
        },

        brilir::Op::Sub(sub) => HIRInstruction::BinOp {
            op: BinaryOp::Sub,
            dst: sub.dest.clone(),
            lhs: sub.args[0].clone(),
            rhs: sub.args[1].clone(),
        },

        brilir::Op::Mul(mul) => HIRInstruction::BinOp {
            op: BinaryOp::Mul,
            dst: mul.dest.clone(),
            lhs: mul.args[0].clone(),
            rhs: mul.args[1].clone(),
        },

        brilir::Op::Lt(lt) => HIRInstruction::BinOp {
            op: BinaryOp::Lt,
            dst: lt.dest.clone(),
            lhs: lt.args[0].clone(),
            rhs: lt.args[1].clone(),
        },

        other => todo!("unknown binop: {:?}", other),
    }
}

pub struct BasicBlock {
    pub body: Vec<HIRInstruction>,
    pub id: BasicBlockId,
    pub preds: Vec<BasicBlockId>,
    pub succs: Vec<BasicBlockId>,
}

impl BasicBlock {
    pub fn new(id: BasicBlockId) -> Self {
        BasicBlock {
            body: Vec::new(),
            id: id,
            preds: Vec::new(),
            succs: Vec::new(),
        }
    }
}

pub struct HIRBuilder {
    pub blocks: Vec<BasicBlock>,
}

pub fn to_hir(builder: &brilir::Builder) -> HIRBuilder {
    let mut blocks: Vec<BasicBlock> = Vec::new();
    for block in builder.blocks.iter() {
        let mut current_block = BasicBlock::new(*builder.label_to_id.get(&block.label).unwrap());
        current_block.preds = block.preds.clone();
        current_block.succs = block.succs.clone();

        for inst in block.body.iter() {
            match inst {
                Inst::Label { .. } => unreachable!(),
                Inst::Op(op) => {
                    match op {
                        brilir::Op::Load(load) => current_block.body.push(HIRInstruction::Load {
                            dst: load.dest.clone(),
                            value: value_hir(&load.value),
                            typ: load.typ.clone(),
                        }),
                        brilir::Op::Jmp(jmp) => current_block.body.push(HIRInstruction::Jmp {
                            labels: jmp.labels.clone(),
                        }),
                        brilir::Op::Br(br) => current_block.body.push(HIRInstruction::Br {
                            args: br.args.clone(),
                            labels: br.labels.clone(),
                        }),
                        _ => current_block.body.push(binop_hir(op)),
                    };
                }
            }
        }
        blocks.push(current_block);
    }
    HIRBuilder { blocks }
}
