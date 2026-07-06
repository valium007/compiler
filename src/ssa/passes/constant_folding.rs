// src/ssa/passes/constant_folding.rs

use crate::ssa::ir::{Builder, IrInstruction, SsaValue, SsaVariable, BinaryOp};
use std::collections::HashMap;

pub struct ConstantFolding;

impl ConstantFolding {
    pub fn run(builder: &mut Builder) {
        // Maps each SSA variable to its known constant value (e.g. Int(x) or Bool(y))
        let mut const_map: HashMap<SsaVariable, SsaValue> = HashMap::new();
        let mut changed = true;

        // Iterate to a fixed point in case folds propagate through phis or cycles
        while changed {
            changed = false;

            // Gather all block IDs to avoid borrow checker issues with builder.blocks mutability
            let block_ids: Vec<usize> = builder.blocks.iter().map(|b| b.id).collect();

            for bb_id in block_ids {
                let mut instrs_to_replace = Vec::new(); // (inst_index, new_instruction)
                {
                    let block = builder.get_block(bb_id);
                    for (inst_idx, inst) in block.instrs.iter().enumerate() {
                        let mut updated_inst = inst.clone();
                        
                        // 1. Rewrite any operand uses with their known constant values
                        updated_inst.for_each_use_mut(|use_val| {
                            if let SsaValue::Var(v) = use_val {
                                if let Some(c) = const_map.get(v) {
                                    *use_val = c.clone();
                                }
                            }
                        });

                        // 2. Try to fold the updated instruction
                        match &updated_inst {
                            IrInstruction::Const(SsaValue::Var(dst), val) => {
                                if const_map.insert(*dst, val.clone()).is_none() {
                                    changed = true;
                                }
                            }
                            IrInstruction::Mov(SsaValue::Var(dst), src) => {
                                if !src.is_var() {
                                    instrs_to_replace.push((inst_idx, IrInstruction::Const(SsaValue::Var(*dst), src.clone())));
                                    if const_map.insert(*dst, src.clone()).is_none() {
                                        changed = true;
                                    }
                                }
                            }
                            IrInstruction::Binary(op, SsaValue::Var(dst), lhs, rhs) => {
                                if let Some(folded) = fold_binary(*op, lhs, rhs) {
                                    instrs_to_replace.push((inst_idx, IrInstruction::Const(SsaValue::Var(*dst), folded.clone())));
                                    if const_map.insert(*dst, folded).is_none() {
                                        changed = true;
                                    }
                                }
                            }
                            IrInstruction::Not(SsaValue::Var(dst), src) => {
                                if let SsaValue::Bool(b) = src {
                                    let folded = SsaValue::Bool(!*b);
                                    instrs_to_replace.push((inst_idx, IrInstruction::Const(SsaValue::Var(*dst), folded.clone())));
                                    if const_map.insert(*dst, folded).is_none() {
                                        changed = true;
                                    }
                                }
                            }
                            
                            _ => {}
                        }
                    }
                }

                // Apply instruction updates to the block
                if !instrs_to_replace.is_empty() {
                    let block = builder.get_block_mut(bb_id);
                    for (idx, new_inst) in instrs_to_replace {
                        block.instrs[idx] = new_inst;
                    }
                }
            }
        }
    }

    pub fn name() -> &'static str {
        "constant_folding"
    }
}

/// Constant folding evaluator for binary operations
fn fold_binary(op: BinaryOp, lhs: &SsaValue, rhs: &SsaValue) -> Option<SsaValue> {
    match (lhs, rhs) {
        (SsaValue::Int(a), SsaValue::Int(b)) => match op {
            BinaryOp::Add => Some(SsaValue::Int(a + b)),
            BinaryOp::Sub => Some(SsaValue::Int(a - b)),
            BinaryOp::Mul => Some(SsaValue::Int(a * b)),
            BinaryOp::Div => {
                if *b != 0 { Some(SsaValue::Int(a / b)) } else { None } // Avoid division by zero compile error
            }
            BinaryOp::Eq => Some(SsaValue::Bool(a == b)),
            BinaryOp::Lt => Some(SsaValue::Bool(a < b)),
            BinaryOp::Gt => Some(SsaValue::Bool(a > b)),
            BinaryOp::Le => Some(SsaValue::Bool(a <= b)),
            BinaryOp::Ge => Some(SsaValue::Bool(a >= b)),
            _ => None,
        },
        (SsaValue::Bool(a), SsaValue::Bool(b)) => match op {
            BinaryOp::Eq => Some(SsaValue::Bool(a == b)),
            BinaryOp::And => Some(SsaValue::Bool(*a && *b)),
            BinaryOp::Or => Some(SsaValue::Bool(*a || *b)),
            _ => None,
        },
        
        (SsaValue::Var(x), SsaValue::Var(y)) if x == y => match op {
            BinaryOp::Sub => Some(SsaValue::Int(0)),
            BinaryOp::Eq => Some(SsaValue::Bool(true)),
            BinaryOp::Le => Some(SsaValue::Bool(true)),
            BinaryOp::Ge => Some(SsaValue::Bool(true)),
            BinaryOp::Lt => Some(SsaValue::Bool(false)),
            BinaryOp::Gt => Some(SsaValue::Bool(false)),
            _ => None,
        },
        _ => None,
    }
}
