pub mod builder;
pub mod instruction;

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

    instrs = std::mem::take(&mut program.functions[0].instrs);
    let mut builder = build_basic_blocks(instrs, data)?;
    build_edges(&mut builder);

    for block in builder.blocks.iter() {
        println!("{:?}", block);
    }

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
                let bb = data.1.get(&label).unwrap();

                if builder.blocks.len() != 0 {
                    let last = builder.get_current_block_mut().instrs.last();
                    let needs_jmp = !matches!(
                        last,
                        Some(IrInstruction::Jmp(_))
                            | Some(IrInstruction::Br(_, _, _))
                            | Some(IrInstruction::Ret(_))
                    );
                    if needs_jmp {
                        builder.add_instr(IrInstruction::Jmp(*bb));
                    }
                }

                builder.add_block(*bb);
            }

            Instruction::Op(op) => match op {
                Op::Const { dest, typ, value } => {
                    if typ == "int" {
                        let dest = data.0.get(&dest).unwrap();
                        builder.add_instr(IrInstruction::Load(
                            Variable(*dest),
                            Immediate::Int(value.as_int()?),
                        ));
                    } else if typ == "bool" {
                        let dest = data.0.get(&dest).unwrap();
                        builder.add_instr(IrInstruction::Load(
                            Variable(*dest),
                            Immediate::Bool(value.as_bool()?),
                        ));
                    }
                }

                Op::Id { dest, args, typ: _ } => {
                    let src = data.0.get(&args[0]).unwrap();
                    let dest = data.0.get(&dest).unwrap();
                    builder.add_instr(IrInstruction::Mov(Variable(*dest), Variable(*src)));
                }

                Op::Add { dest, args, typ: _ } => {
                    let dest = data.0.get(&dest).unwrap();
                    let arg_ids: Vec<usize> =
                        args.iter().map(|arg| *data.0.get(arg).unwrap()).collect();
                    builder.add_instr(IrInstruction::Binary(
                        BinaryOp::Add,
                        Variable(*dest),
                        Variable(arg_ids[0]),
                        Variable(arg_ids[1]),
                    ));
                }

                Op::Sub { dest, args, typ: _ } => {
                    let dest = data.0.get(&dest).unwrap();
                    let arg_ids: Vec<usize> =
                        args.iter().map(|arg| *data.0.get(arg).unwrap()).collect();
                    builder.add_instr(IrInstruction::Binary(
                        BinaryOp::Sub,
                        Variable(*dest),
                        Variable(arg_ids[0]),
                        Variable(arg_ids[1]),
                    ));
                }

                Op::Mul { dest, args, typ: _ } => {
                    let dest = data.0.get(&dest).unwrap();
                    let arg_ids: Vec<usize> =
                        args.iter().map(|arg| *data.0.get(arg).unwrap()).collect();
                    builder.add_instr(IrInstruction::Binary(
                        BinaryOp::Mul,
                        Variable(*dest),
                        Variable(arg_ids[0]),
                        Variable(arg_ids[1]),
                    ));
                }

                Op::Div { dest, args, typ: _ } => {
                    let dest = data.0.get(&dest).unwrap();
                    let arg_ids: Vec<usize> =
                        args.iter().map(|arg| *data.0.get(arg).unwrap()).collect();
                    builder.add_instr(IrInstruction::Binary(
                        BinaryOp::Div,
                        Variable(*dest),
                        Variable(arg_ids[0]),
                        Variable(arg_ids[1]),
                    ));
                }

                Op::Eq { dest, args, typ: _ } => {
                    let dest = data.0.get(&dest).unwrap();
                    let arg_ids: Vec<usize> =
                        args.iter().map(|arg| *data.0.get(arg).unwrap()).collect();
                    builder.add_instr(IrInstruction::Binary(
                        BinaryOp::Eq,
                        Variable(*dest),
                        Variable(arg_ids[0]),
                        Variable(arg_ids[1]),
                    ));
                }

                Op::Lt { dest, args, typ: _ } => {
                    let dest = data.0.get(&dest).unwrap();
                    let arg_ids: Vec<usize> =
                        args.iter().map(|arg| *data.0.get(arg).unwrap()).collect();
                    builder.add_instr(IrInstruction::Binary(
                        BinaryOp::Lt,
                        Variable(*dest),
                        Variable(arg_ids[0]),
                        Variable(arg_ids[1]),
                    ));
                }

                Op::Le { dest, args, typ: _ } => {
                    let dest = data.0.get(&dest).unwrap();
                    let arg_ids: Vec<usize> =
                        args.iter().map(|arg| *data.0.get(arg).unwrap()).collect();
                    builder.add_instr(IrInstruction::Binary(
                        BinaryOp::Le,
                        Variable(*dest),
                        Variable(arg_ids[0]),
                        Variable(arg_ids[1]),
                    ));
                }

                Op::Gt { dest, args, typ: _ } => {
                    let dest = data.0.get(&dest).unwrap();
                    let arg_ids: Vec<usize> =
                        args.iter().map(|arg| *data.0.get(arg).unwrap()).collect();
                    builder.add_instr(IrInstruction::Binary(
                        BinaryOp::Gt,
                        Variable(*dest),
                        Variable(arg_ids[0]),
                        Variable(arg_ids[1]),
                    ));
                }

                Op::Ge { dest, args, typ: _ } => {
                    let dest = data.0.get(&dest).unwrap();
                    let arg_ids: Vec<usize> =
                        args.iter().map(|arg| *data.0.get(arg).unwrap()).collect();
                    builder.add_instr(IrInstruction::Binary(
                        BinaryOp::Ge,
                        Variable(*dest),
                        Variable(arg_ids[0]),
                        Variable(arg_ids[1]),
                    ));
                }

                Op::And { dest, args } => {
                    let dest = data.0.get(&dest).unwrap();
                    let arg_ids: Vec<usize> =
                        args.iter().map(|arg| *data.0.get(arg).unwrap()).collect();
                    builder.add_instr(IrInstruction::Binary(
                        BinaryOp::And,
                        Variable(*dest),
                        Variable(arg_ids[0]),
                        Variable(arg_ids[1]),
                    ));
                }

                Op::Or { dest, args } => {
                    let dest = data.0.get(&dest).unwrap();
                    let arg_ids: Vec<usize> =
                        args.iter().map(|arg| *data.0.get(arg).unwrap()).collect();
                    builder.add_instr(IrInstruction::Binary(
                        BinaryOp::Or,
                        Variable(*dest),
                        Variable(arg_ids[0]),
                        Variable(arg_ids[1]),
                    ));
                }

                Op::Not { dest, args } => {
                    let dest = data.0.get(&dest).unwrap();
                    let src = data.0.get(&args[0]).unwrap();
                    builder.add_instr(IrInstruction::Not(Variable(*dest), Variable(*src)));
                }

                Op::Print { args } => {
                    let src = data.0.get(&args[0]).unwrap();
                    builder.add_instr(IrInstruction::Print(Variable(*src)));
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
                        Variable(*src),
                        block_ids[0],
                        block_ids[1],
                    ))
                }

                Op::Ret { args } => {
                    let src = data.0.get(&args[0]).unwrap();
                    builder.add_instr(IrInstruction::Ret(Variable(*src)));
                }

                Op::Nop => {}
                _ => {}
            },
        }
    }

    let last_block = builder.blocks.last().unwrap();

    if last_block.instrs.is_empty()
        || !matches!(last_block.instrs.last().unwrap(), IrInstruction::Ret(_))
    {
        builder.add_instr(IrInstruction::Load(Variable(data.2), Immediate::Int(0)));
        builder.add_instr(IrInstruction::Ret(Variable(data.2)));
        // data.2 was consumed as a variable, so next free id is data.2 + 1
        builder.next_var_id = data.2 + 1;
    } else {
        // No synthetic ret added; data.2 is already the next free id
        builder.next_var_id = data.2;
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
