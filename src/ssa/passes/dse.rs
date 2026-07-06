use crate::ssa::ir::*;


pub struct DeadStoreElimination;

impl DeadStoreElimination {
    pub fn run(builder: &mut Builder) {
        let mut defs: Vec<InstId> = Vec::new();
        let mut i = 0usize;
        for block in builder.blocks.iter() {
            for insn in block.instrs.iter() {
                match insn {
                    IrInstruction::Const(_,_) => {
                        defs.push(i);
                    }
                    _ => {}
                }
                i+=1
            }
        }

        while !defs.is_empty() {
            let s = defs.pop().unwrap();
            if store_has_users(s, builder)  {
                continue;
            }
            delete_store(s, builder);
        }
    }
}

pub fn delete_store(inst_id: InstId, builder: &mut Builder) {
    let mut i = 0usize;
    for block in builder.blocks.iter_mut() {
        for insn in block.instrs.iter_mut() {
            if i == inst_id {
                *insn = IrInstruction::Nop;
            }
            i+=1;
        }
    }
}

pub fn store_has_users(inst_id: InstId, builder: &Builder) -> bool {
    let mut i = 0usize;
    let mut val: &SsaValue = &SsaValue::Undef;
    
    for block in builder.blocks.iter() {
        for insn in block.instrs.iter() {
            if i == inst_id && matches!(insn, IrInstruction::Const(_,_)) {
                match insn {
                    IrInstruction::Const(lhs, _) => val = lhs,
                    _ => {}
                }
            }
            if insn.uses().contains(&val) {
                return true;
            }
            i+=1;
        }
    }
    false
}