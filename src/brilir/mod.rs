pub mod builder;
pub mod id;
pub mod instruction;
pub mod liveness;
pub use liveness::compute_liveness;

use crate::{
    bril_frontend::{
        self,
        json::{Instruction, Op},
    },
    brilir::{
        builder::Builder,
        instruction::{BinaryOp, Immediate, IrInstruction, Variable},
    },
};
use anyhow::Result;
use hashbrown::HashMap;

pub fn compile_bril() -> Result<Builder> {
    let mut program = bril_frontend::parse_json()?;
    let mut instrs: Vec<Instruction> = Vec::new();

    let data = build_data(&program.functions[0].instrs)?;
    println!("data: {:?}", data);
    instrs = std::mem::take(&mut program.functions[0].instrs);
    let mut builder = build_basic_blocks(instrs, data)?;
    build_edges(&mut builder);
    for block in builder.blocks.iter() {
        println!("{:?}", block);
    }
    builder.liveness = liveness::compute_liveness(&mut builder)?;
    Ok(builder)
}

pub fn build_basic_blocks(
    instrs: Vec<Instruction>,
    data: (HashMap<String, usize>, HashMap<String, usize>, usize),
) -> Result<Builder> {
    let mut builder = builder::Builder::new();

    for instr in instrs {
        match instr {
            Instruction::Label { label } => {
                let bb_id = data.1.get(&label).unwrap();
                builder.add_block(*bb_id);
            }

            Instruction::Op(op) => match op {
                Op::Const { dest, typ, value } => {
                    if typ == "int" {
                        let dest = data.0.get(&dest).unwrap();
                        builder.add_instr(IrInstruction::Load(
                            Variable {
                                id: *dest,
                                index: 0,
                            },
                            Immediate::Int(value.as_int()?),
                        ));
                    } else if typ == "bool" {
                        let dest = data.0.get(&dest).unwrap();
                        builder.add_instr(IrInstruction::Load(
                            Variable {
                                id: *dest,
                                index: 0,
                            },
                            Immediate::Bool(value.as_bool()?),
                        ));
                    }
                }

                Op::Id { dest, args, typ: _ } => {
                    let src = data.0.get(&args[0]).unwrap();
                    let dest = data.0.get(&dest).unwrap();
                    builder.add_instr(IrInstruction::Mov(
                        Variable {
                            id: *dest,
                            index: 0,
                        },
                        Variable { id: *src, index: 0 },
                    ));
                }

                Op::Add { dest, args, typ: _ } => {
                    let dest = data.0.get(&dest).unwrap();
                    let arg_ids: Vec<usize> =
                        args.iter().map(|arg| *data.0.get(arg).unwrap()).collect();
                    builder.add_instr(IrInstruction::Binary(
                        BinaryOp::Add,
                        Variable {
                            id: *dest,
                            index: 0,
                        },
                        Variable {
                            id: arg_ids[0],
                            index: 0,
                        },
                        Variable {
                            id: arg_ids[1],
                            index: 0,
                        },
                    ));
                }

                Op::Sub { dest, args, typ: _ } => {
                    let dest = data.0.get(&dest).unwrap();
                    let arg_ids: Vec<usize> =
                        args.iter().map(|arg| *data.0.get(arg).unwrap()).collect();
                    builder.add_instr(IrInstruction::Binary(
                        BinaryOp::Sub,
                        Variable {
                            id: *dest,
                            index: 0,
                        },
                        Variable {
                            id: arg_ids[0],
                            index: 0,
                        },
                        Variable {
                            id: arg_ids[1],
                            index: 0,
                        },
                    ));
                }

                Op::Mul { dest, args, typ: _ } => {
                    let dest = data.0.get(&dest).unwrap();
                    let arg_ids: Vec<usize> =
                        args.iter().map(|arg| *data.0.get(arg).unwrap()).collect();
                    builder.add_instr(IrInstruction::Binary(
                        BinaryOp::Mul,
                        Variable {
                            id: *dest,
                            index: 0,
                        },
                        Variable {
                            id: arg_ids[0],
                            index: 0,
                        },
                        Variable {
                            id: arg_ids[1],
                            index: 0,
                        },
                    ));
                }

                Op::Div { dest, args, typ: _ } => {
                    let dest = data.0.get(&dest).unwrap();
                    let arg_ids: Vec<usize> =
                        args.iter().map(|arg| *data.0.get(arg).unwrap()).collect();
                    builder.add_instr(IrInstruction::Binary(
                        BinaryOp::Div,
                        Variable {
                            id: *dest,
                            index: 0,
                        },
                        Variable {
                            id: arg_ids[0],
                            index: 0,
                        },
                        Variable {
                            id: arg_ids[1],
                            index: 0,
                        },
                    ));
                }

                Op::Eq { dest, args, typ: _ } => {
                    let dest = data.0.get(&dest).unwrap();
                    let arg_ids: Vec<usize> =
                        args.iter().map(|arg| *data.0.get(arg).unwrap()).collect();
                    builder.add_instr(IrInstruction::Binary(
                        BinaryOp::Eq,
                        Variable {
                            id: *dest,
                            index: 0,
                        },
                        Variable {
                            id: arg_ids[0],
                            index: 0,
                        },
                        Variable {
                            id: arg_ids[1],
                            index: 0,
                        },
                    ));
                }

                Op::Lt { dest, args, typ: _ } => {
                    let dest = data.0.get(&dest).unwrap();
                    let arg_ids: Vec<usize> =
                        args.iter().map(|arg| *data.0.get(arg).unwrap()).collect();
                    builder.add_instr(IrInstruction::Binary(
                        BinaryOp::Lt,
                        Variable {
                            id: *dest,
                            index: 0,
                        },
                        Variable {
                            id: arg_ids[0],
                            index: 0,
                        },
                        Variable {
                            id: arg_ids[1],
                            index: 0,
                        },
                    ));
                }

                Op::Le { dest, args, typ: _ } => {
                    let dest = data.0.get(&dest).unwrap();
                    let arg_ids: Vec<usize> =
                        args.iter().map(|arg| *data.0.get(arg).unwrap()).collect();
                    builder.add_instr(IrInstruction::Binary(
                        BinaryOp::Le,
                        Variable {
                            id: *dest,
                            index: 0,
                        },
                        Variable {
                            id: arg_ids[0],
                            index: 0,
                        },
                        Variable {
                            id: arg_ids[1],
                            index: 0,
                        },
                    ));
                }

                Op::Gt { dest, args, typ: _ } => {
                    let dest = data.0.get(&dest).unwrap();
                    let arg_ids: Vec<usize> =
                        args.iter().map(|arg| *data.0.get(arg).unwrap()).collect();
                    builder.add_instr(IrInstruction::Binary(
                        BinaryOp::Gt,
                        Variable {
                            id: *dest,
                            index: 0,
                        },
                        Variable {
                            id: arg_ids[0],
                            index: 0,
                        },
                        Variable {
                            id: arg_ids[1],
                            index: 0,
                        },
                    ));
                }

                Op::Ge { dest, args, typ: _ } => {
                    let dest = data.0.get(&dest).unwrap();
                    let arg_ids: Vec<usize> =
                        args.iter().map(|arg| *data.0.get(arg).unwrap()).collect();
                    builder.add_instr(IrInstruction::Binary(
                        BinaryOp::Ge,
                        Variable {
                            id: *dest,
                            index: 0,
                        },
                        Variable {
                            id: arg_ids[0],
                            index: 0,
                        },
                        Variable {
                            id: arg_ids[1],
                            index: 0,
                        },
                    ));
                }

                Op::And { dest, args } => {
                    let dest = data.0.get(&dest).unwrap();
                    let arg_ids: Vec<usize> =
                        args.iter().map(|arg| *data.0.get(arg).unwrap()).collect();
                    builder.add_instr(IrInstruction::Binary(
                        BinaryOp::And,
                        Variable {
                            id: *dest,
                            index: 0,
                        },
                        Variable {
                            id: arg_ids[0],
                            index: 0,
                        },
                        Variable {
                            id: arg_ids[1],
                            index: 0,
                        },
                    ));
                }

                Op::Or { dest, args } => {
                    let dest = data.0.get(&dest).unwrap();
                    let arg_ids: Vec<usize> =
                        args.iter().map(|arg| *data.0.get(arg).unwrap()).collect();
                    builder.add_instr(IrInstruction::Binary(
                        BinaryOp::Or,
                        Variable {
                            id: *dest,
                            index: 0,
                        },
                        Variable {
                            id: arg_ids[0],
                            index: 0,
                        },
                        Variable {
                            id: arg_ids[1],
                            index: 0,
                        },
                    ));
                }

                Op::Not { dest, args } => {
                    let dest = data.0.get(&dest).unwrap();
                    let src = data.0.get(&args[0]).unwrap();
                    builder.add_instr(IrInstruction::Not(
                        Variable {
                            id: *dest,
                            index: 0,
                        },
                        Variable { id: *src, index: 0 },
                    ));
                }

                Op::Print { args } => {
                    let src = data.0.get(&args[0]).unwrap();
                    builder.add_instr(IrInstruction::Print(Variable { id: *src, index: 0 }));
                }

                Op::Jmp { labels } => {
                    let bb = *data.1.get(&labels[0]).unwrap();
                    builder.add_instr(IrInstruction::Jmp(bb));
                }

                Op::Br { args, labels } => {
                    let src = data.0.get(&args[0]).unwrap();
                    let block_ids: Vec<usize> = labels
                        .iter()
                        .map(|label| *data.1.get(label).unwrap())
                        .collect();
                    builder.add_instr(IrInstruction::Br(
                        Variable { id: *src, index: 0 },
                        block_ids[0],
                        block_ids[1],
                    ))
                }

                Op::Ret { args } => {
                    let src = data.0.get(&args[0]).unwrap();
                    builder.add_instr(IrInstruction::Ret(Variable { id: *src, index: 0 }));
                }

                Op::Nop => {}
                _ => {}
            },
        }
    }
    if let Some(last_block) = builder.blocks.last() {
        if let Some(last_instr) = last_block.instrs.last() {
            match last_instr {
                IrInstruction::Ret(var) => {}
                _ => {
                    builder.add_instr(IrInstruction::Load(
                        Variable {
                            id: data.2,
                            index: 0,
                        },
                        Immediate::Int(0),
                    ));
                    builder.add_instr(IrInstruction::Ret(Variable {
                        id: data.2,
                        index: 0,
                    }));
                }
            }
        }
    }
    Ok(builder)
}

pub fn build_data(
    instrs: &Vec<Instruction>,
) -> Result<(HashMap<String, usize>, HashMap<String, usize>, usize)> {
    let mut next_variable_id = 0usize;
    let mut next_block_id = 0usize;

    let mut var_to_id: HashMap<String, usize> = HashMap::new();
    let mut label_to_block: HashMap<String, usize> = HashMap::new();

    for instr in instrs.iter() {
        match instr {
            Instruction::Label { label } => {
                if let Some(_block) = label_to_block.get(label) {
                } else {
                    label_to_block.insert(label.clone(), next_block_id);
                    next_block_id += 1;
                }
            }
            Instruction::Op(op) => match op {
                Op::Add { dest, .. }
                | Op::Sub { dest, .. }
                | Op::Mul { dest, .. }
                | Op::Div { dest, .. }
                | Op::Eq { dest, .. }
                | Op::Lt { dest, .. }
                | Op::Gt { dest, .. }
                | Op::Le { dest, .. }
                | Op::Ge { dest, .. }
                | Op::Not { dest, .. }
                | Op::And { dest, .. }
                | Op::Or { dest, .. }
                | Op::Const { dest, .. }
                | Op::Id { dest, .. } => {
                    if let Some(_var) = var_to_id.get(dest) {
                    } else {
                        var_to_id.insert(dest.clone(), next_variable_id);
                        next_variable_id += 1;
                    }
                }
                _ => {}
            },
        }
    }
    Ok((var_to_id, label_to_block, next_variable_id))
}

pub fn build_edges(builder: &mut Builder) {
    let mut blocks = builder.blocks.clone();
    for block in blocks.iter_mut() {
        let last_inst = block.instrs.last();
        if let Some(inst) = last_inst {
            match inst {
                IrInstruction::Jmp(bb) => {
                    let successor = *bb;
                    builder.add_edge(block.id, successor);
                }
                IrInstruction::Br(_, then_block, else_block) => {
                    let then_successor = *then_block;
                    let else_successor = *else_block;
                    builder.add_edge(block.id, then_successor);
                    builder.add_edge(block.id, else_successor);
                }
                _ => {
                    if block.id + 1 < builder.blocks.len() {
                        builder.add_edge(block.id, block.id + 1);
                    }
                }
            }
        }
    }
}
