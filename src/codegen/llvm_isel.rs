use crate::brilir::{
    builder::{BasicBlock, Builder},
    instruction::{BinaryOp, Immediate, IrInstruction},
};

fn imm_to_llvm_int(imm: &Immediate) -> String {
    match imm {
        Immediate::Int(i) => i.to_string(),
        Immediate::Bool(b) => {
            if *b {
                "1".to_string()
            } else {
                "0".to_string()
            }
        }
    }
}

fn imm_to_llvm_i1(imm: &Immediate) -> String {
    match imm {
        Immediate::Bool(b) => {
            if *b {
                "1".to_string()
            } else {
                "0".to_string()
            }
        }
        Immediate::Int(i) => {
            if *i == 0 {
                "0".to_string()
            } else {
                "1".to_string()
            }
        }
    }
}

pub fn emit_llvm_ir(builder: &Builder) -> String {
    let mut out = String::new();

    out.push_str("@formatString = private constant [6 x i8] c\"%lld\\0A\\00\"\n");
    out.push_str("declare i32 @printf(ptr, ...)\n\n");

    out.push_str("define i32 @main() {\n");

    for block in builder.blocks.iter() {
        emit_block(block, &mut out);
    }

    out.push_str("}\n");
    out
}

fn emit_block(block: &BasicBlock, out: &mut String) {
    out.push_str(&format!("bb_{}:\n", block.id));

    for phi in block.phis.iter() {
        let var = format!("%v{}_{}", phi.var.id, phi.var.index);
        let operands: Vec<String> = phi
            .operands
            .iter()
            .map(|(v, pred)| format!("[ %v{}_{}, %bb_{} ]", v.id, v.index, pred))
            .collect();

        // NOTE:
        // If your IR tracks types per variable, switch this to i1 for bool vars.
        // For now we keep i64 to match existing pipeline expectations.
        out.push_str(&format!("  {} = phi i64 {}\n", var, operands.join(", ")));
    }

    for instr in block.instrs.iter() {
        match instr {
            IrInstruction::Load(dst, imm) => {
                // Keep values in i64 domain for now.
                out.push_str(&format!(
                    "  %v{}_{} = add i64 0, {}\n",
                    dst.id,
                    dst.index,
                    imm_to_llvm_int(imm)
                ));
            }

            IrInstruction::Mov(dst, src) => {
                out.push_str(&format!(
                    "  %v{}_{} = add i64 0, %v{}_{}\n",
                    dst.id, dst.index, src.id, src.index
                ));
            }

            IrInstruction::Binary(op, dst, lhs, rhs) => {
                let opcode = binary_opcode(op);
                let dst_s = format!("%v{}_{}", dst.id, dst.index);
                let lhs_s = format!("%v{}_{}", lhs.id, lhs.index);
                let rhs_s = format!("%v{}_{}", rhs.id, rhs.index);

                match op {
                    BinaryOp::Eq | BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
                        let tmp = format!("%cmp_{}_{}_{}", dst.id, dst.index, block.id);
                        out.push_str(&format!(
                            "  {} = icmp {} i64 {}, {}\n",
                            tmp, opcode, lhs_s, rhs_s
                        ));
                        out.push_str(&format!("  {} = zext i1 {} to i64\n", dst_s, tmp));
                    }
                    _ => {
                        out.push_str(&format!(
                            "  {} = {} i64 {}, {}\n",
                            dst_s, opcode, lhs_s, rhs_s
                        ));
                    }
                }
            }

            IrInstruction::Not(dst, src) => {
                let tmp = format!("%not_tmp_{}_{}_{}", dst.id, dst.index, block.id);
                out.push_str(&format!(
                    "  {} = icmp eq i64 %v{}_{}, 0\n",
                    tmp, src.id, src.index
                ));
                out.push_str(&format!(
                    "  %v{}_{} = zext i1 {} to i64\n",
                    dst.id, dst.index, tmp
                ));
            }

            IrInstruction::Print(src) => {
                let vid = src.id;
                let vidx = src.index;
                let bid = block.id;
                out.push_str(&format!(
                    "  %fmt_ptr_{vid}_{vidx}_{bid} = getelementptr inbounds [6 x i8], ptr @formatString, i32 0, i32 0\n"
                ));
                out.push_str(&format!(
                    "  %print_ret_{vid}_{vidx}_{bid} = call i32 (ptr, ...) @printf(ptr %fmt_ptr_{vid}_{vidx}_{bid}, i64 %v{vid}_{vidx})\n"
                ));
            }

            IrInstruction::Jmp(target) => {
                out.push_str(&format!("  br label %bb_{}\n", target));
            }

            IrInstruction::Br(cond, then_bb, else_bb) => {
                let tmp = format!("%br_cond_{}_{}_{}", cond.id, cond.index, block.id);
                out.push_str(&format!(
                    "  {} = icmp ne i64 %v{}_{}, 0\n",
                    tmp, cond.id, cond.index
                ));
                out.push_str(&format!(
                    "  br i1 {}, label %bb_{}, label %bb_{}\n",
                    tmp, then_bb, else_bb
                ));
            }

            IrInstruction::Ret(var) => {
                // Return type of @main is i32. Truncate i64 SSA value.
                let tmp = format!("%ret32_{}_{}_{}", var.id, var.index, block.id);
                out.push_str(&format!(
                    "  {} = trunc i64 %v{}_{} to i32\n",
                    tmp, var.id, var.index
                ));
                out.push_str(&format!("  ret i32 {}\n", tmp));
            }

            IrInstruction::Call | IrInstruction::Nop => {}
        }
    }

    let is_terminated = block.instrs.last().map_or(false, |i| {
        matches!(
            i,
            IrInstruction::Jmp(_) | IrInstruction::Br(..) | IrInstruction::Ret(_)
        )
    });

    if !is_terminated {
        if let Some(&succ) = block.successors.iter().next() {
            out.push_str(&format!("  br label %bb_{}\n", succ));
        }
    }
}

fn binary_opcode(op: &BinaryOp) -> &'static str {
    match op {
        BinaryOp::Add => "add",
        BinaryOp::Sub => "sub",
        BinaryOp::Mul => "mul",
        BinaryOp::Div => "sdiv",
        BinaryOp::And => "and",
        BinaryOp::Or => "or",
        BinaryOp::Eq => "eq",
        BinaryOp::Lt => "slt",
        BinaryOp::Le => "sle",
        BinaryOp::Gt => "sgt",
        BinaryOp::Ge => "sge",
    }
}
