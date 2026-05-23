use anyhow::{Result, anyhow};
use hashbrown::{HashMap, HashSet};

use crate::brilir::{
    builder::{BasicBlockId, Builder},
    instruction::Variable,
};

type LiveSets = (Vec<HashSet<Variable>>, Vec<HashSet<Variable>>);

#[derive(Debug, Clone)]
pub struct RegisterAllocation {
    registers: HashMap<Variable, usize>,
    register_count: usize,
}

impl RegisterAllocation {
    pub fn register(&self, var: Variable) -> Result<usize> {
        self.registers
            .get(&var)
            .copied()
            .ok_or_else(|| anyhow!("missing register allocation for {:?}", var))
    }

    pub fn register_count(&self) -> usize {
        self.register_count
    }
}

#[derive(Debug, Clone)]
struct LivenessInfo {
    live_ins: Vec<HashSet<Variable>>,
    live_outs: Vec<HashSet<Variable>>,
    defs: HashMap<Variable, (BasicBlockId, usize)>,
    uses: Vec<Vec<(usize, Variable)>>,
}

pub fn allocate_registers(builder: &Builder) -> Result<RegisterAllocation> {
    let (live_ins, live_outs) = compute_ssa_liveness(builder)?;
    let liveness = compute_liveness_info(builder, live_ins, live_outs)?;
    let mut uses_remaining = count_uses(builder)?;

    let mut registers: HashMap<Variable, usize> = HashMap::new();
    let mut active: HashMap<usize, Variable> = HashMap::new();
    let mut future_active: HashMap<usize, HashSet<Variable>> = HashMap::new();
    let mut register_count = 0usize;
    let mut previous_live_out: HashSet<Variable> = HashSet::new();

    for block in builder.blocks.iter() {
        expire_between_blocks(
            &previous_live_out,
            &liveness.live_ins[block.id],
            &mut uses_remaining,
            &registers,
            &mut active,
            &mut future_active,
        );

        let mut block_live_ins: Vec<Variable> = liveness.live_ins[block.id]
            .difference(&previous_live_out)
            .copied()
            .collect();
        block_live_ins.sort();
        for var in block_live_ins {
            allocate_register(
                var,
                &liveness,
                &mut registers,
                &mut active,
                &mut future_active,
                &mut register_count,
            )?;
        }

        for phi in block.phis.iter() {
            allocate_register(
                phi.var,
                &liveness,
                &mut registers,
                &mut active,
                &mut future_active,
                &mut register_count,
            )?;
            free_if_dead(
                phi.var,
                &uses_remaining,
                &registers,
                &mut active,
                &mut future_active,
            );
        }

        for instr in block.instrs.iter() {
            for var in instr.get_use()? {
                allocate_register(
                    *var,
                    &liveness,
                    &mut registers,
                    &mut active,
                    &mut future_active,
                    &mut register_count,
                )?;
            }

            for var in instr.get_use()? {
                consume_use(
                    *var,
                    &mut uses_remaining,
                    &registers,
                    &mut active,
                    &mut future_active,
                );
            }

            for var in instr.get_def()? {
                allocate_register(
                    *var,
                    &liveness,
                    &mut registers,
                    &mut active,
                    &mut future_active,
                    &mut register_count,
                )?;
                free_if_dead(
                    *var,
                    &uses_remaining,
                    &registers,
                    &mut active,
                    &mut future_active,
                );
            }
        }

        let mut edge_uses = phi_uses_from_block(builder, block.id);
        edge_uses.sort();
        for var in edge_uses.iter() {
            allocate_register(
                *var,
                &liveness,
                &mut registers,
                &mut active,
                &mut future_active,
                &mut register_count,
            )?;
        }
        for var in edge_uses {
            consume_use(
                var,
                &mut uses_remaining,
                &registers,
                &mut active,
                &mut future_active,
            );
        }

        previous_live_out = liveness.live_outs[block.id].clone();
    }

    Ok(RegisterAllocation {
        registers,
        register_count,
    })
}

fn allocate_register(
    var: Variable,
    liveness: &LivenessInfo,
    registers: &mut HashMap<Variable, usize>,
    active: &mut HashMap<usize, Variable>,
    future_active: &mut HashMap<usize, HashSet<Variable>>,
    register_count: &mut usize,
) -> Result<usize> {
    if let Some(reg) = registers.get(&var).copied() {
        if active.get(&reg) == Some(&var) {
            return Ok(reg);
        }
        if let Some(paused) = future_active.get_mut(&reg)
            && paused.remove(&var)
        {
            active.insert(reg, var);
            return Ok(reg);
        }
        if !active.contains_key(&reg) {
            active.insert(reg, var);
        }
        return Ok(reg);
    }

    for reg in 0..=*register_count {
        if active.contains_key(&reg) {
            continue;
        }

        let conflicts_with_future = future_active
            .get(&reg)
            .map(|vars| {
                vars.iter()
                    .any(|future| live_at_same_time(var, *future, liveness))
            })
            .unwrap_or(false);

        if conflicts_with_future {
            continue;
        }

        registers.insert(var, reg);
        active.insert(reg, var);
        *register_count = (*register_count).max(reg + 1);
        return Ok(reg);
    }

    Err(anyhow!("failed to allocate a register for {:?}", var))
}

fn expire_between_blocks(
    previous_live_out: &HashSet<Variable>,
    live_in: &HashSet<Variable>,
    uses_remaining: &mut HashMap<Variable, usize>,
    registers: &HashMap<Variable, usize>,
    active: &mut HashMap<usize, Variable>,
    future_active: &mut HashMap<usize, HashSet<Variable>>,
) {
    let mut expired: Vec<Variable> = previous_live_out.difference(live_in).copied().collect();
    expired.sort();

    for var in expired {
        if uses_remaining.get(&var).copied().unwrap_or(0) == 0 {
            free_register(var, registers, active, future_active);
        } else {
            pause_register(var, registers, active, future_active);
        }
    }
}

fn consume_use(
    var: Variable,
    uses_remaining: &mut HashMap<Variable, usize>,
    registers: &HashMap<Variable, usize>,
    active: &mut HashMap<usize, Variable>,
    future_active: &mut HashMap<usize, HashSet<Variable>>,
) {
    if let Some(count) = uses_remaining.get_mut(&var) {
        *count = count.saturating_sub(1);
    }
    free_if_dead(var, uses_remaining, registers, active, future_active);
}

fn free_if_dead(
    var: Variable,
    uses_remaining: &HashMap<Variable, usize>,
    registers: &HashMap<Variable, usize>,
    active: &mut HashMap<usize, Variable>,
    future_active: &mut HashMap<usize, HashSet<Variable>>,
) {
    if uses_remaining.get(&var).copied().unwrap_or(0) == 0 {
        free_register(var, registers, active, future_active);
    }
}

fn pause_register(
    var: Variable,
    registers: &HashMap<Variable, usize>,
    active: &mut HashMap<usize, Variable>,
    future_active: &mut HashMap<usize, HashSet<Variable>>,
) {
    if let Some(reg) = registers.get(&var).copied() {
        active.remove(&reg);
        future_active.entry(reg).or_default().insert(var);
    }
}

fn free_register(
    var: Variable,
    registers: &HashMap<Variable, usize>,
    active: &mut HashMap<usize, Variable>,
    future_active: &mut HashMap<usize, HashSet<Variable>>,
) {
    if let Some(reg) = registers.get(&var).copied() {
        if active.get(&reg) == Some(&var) {
            active.remove(&reg);
        }
        if let Some(paused) = future_active.get_mut(&reg) {
            paused.remove(&var);
        }
    }
}

fn live_at_same_time(lhs: Variable, rhs: Variable, liveness: &LivenessInfo) -> bool {
    if lhs == rhs {
        return true;
    }

    let Some(&(lhs_block, _)) = liveness.defs.get(&lhs) else {
        return true;
    };
    let Some(&(rhs_block, _)) = liveness.defs.get(&rhs) else {
        return true;
    };

    if lhs_block == rhs_block {
        live_at_same_time_same_block(lhs, rhs, lhs_block, liveness)
    } else {
        live_at_same_time_in_block(lhs, rhs, lhs_block, liveness)
            || live_at_same_time_in_block(rhs, lhs, rhs_block, liveness)
    }
}

fn live_at_same_time_same_block(
    lhs: Variable,
    rhs: Variable,
    block: BasicBlockId,
    liveness: &LivenessInfo,
) -> bool {
    let lhs_live_out = liveness.live_outs[block].contains(&lhs);
    let rhs_live_out = liveness.live_outs[block].contains(&rhs);

    if lhs_live_out && rhs_live_out {
        return true;
    }

    if !lhs_live_out && !rhs_live_out {
        let lhs_pos = liveness.defs[&lhs].1;
        let rhs_pos = liveness.defs[&rhs].1;
        let (first, last_pos) = if lhs_pos <= rhs_pos {
            (lhs, rhs_pos)
        } else {
            (rhs, lhs_pos)
        };

        return liveness.uses[block]
            .iter()
            .any(|(pos, var)| *pos > last_pos && *var == first);
    }

    let (live_out, other) = if lhs_live_out { (lhs, rhs) } else { (rhs, lhs) };
    let live_out_pos = liveness.defs[&live_out].1;

    liveness.uses[block]
        .iter()
        .any(|(pos, var)| *pos > live_out_pos && *var == other)
}

fn live_at_same_time_in_block(
    lhs: Variable,
    rhs: Variable,
    lhs_block: BasicBlockId,
    liveness: &LivenessInfo,
) -> bool {
    if !liveness.live_ins[lhs_block].contains(&rhs) {
        return false;
    }

    if liveness.live_outs[lhs_block].contains(&rhs) {
        return true;
    }

    let lhs_pos = liveness.defs[&lhs].1;
    liveness.uses[lhs_block]
        .iter()
        .any(|(pos, var)| *pos > lhs_pos && *var == rhs)
}

fn compute_ssa_liveness(builder: &Builder) -> Result<LiveSets> {
    let n = builder.blocks.len();
    let mut live_ins: Vec<HashSet<Variable>> = vec![HashSet::new(); n];
    let mut live_outs: Vec<HashSet<Variable>> = vec![HashSet::new(); n];
    let mut uses: Vec<HashSet<Variable>> = vec![HashSet::new(); n];
    let mut defs: Vec<HashSet<Variable>> = vec![HashSet::new(); n];

    for block in builder.blocks.iter() {
        for phi in block.phis.iter() {
            defs[block.id].insert(phi.var);
        }

        for instr in block.instrs.iter() {
            for var in instr.get_use()? {
                if !defs[block.id].contains(var) {
                    uses[block.id].insert(*var);
                }
            }
            for var in instr.get_def()? {
                defs[block.id].insert(*var);
            }
        }
    }

    loop {
        let mut changed = false;

        for block in builder.blocks.iter().rev() {
            let old_in = live_ins[block.id].clone();
            let old_out = live_outs[block.id].clone();

            live_outs[block.id].clear();
            for succ in block.successors.iter() {
                live_outs[block.id].extend(live_ins[*succ].iter().copied());
                for phi in builder.blocks[*succ].phis.iter() {
                    if let Some((var, _)) = phi.operands.iter().find(|(_, pred)| *pred == block.id)
                    {
                        live_outs[block.id].insert(*var);
                    }
                }
            }

            live_ins[block.id].clear();
            live_ins[block.id].extend(uses[block.id].iter().copied());
            for var in live_outs[block.id].iter() {
                if !defs[block.id].contains(var) {
                    live_ins[block.id].insert(*var);
                }
            }

            if old_in != live_ins[block.id] || old_out != live_outs[block.id] {
                changed = true;
            }
        }

        if !changed {
            break;
        }
    }

    Ok((live_ins, live_outs))
}

fn compute_liveness_info(
    builder: &Builder,
    live_ins: Vec<HashSet<Variable>>,
    live_outs: Vec<HashSet<Variable>>,
) -> Result<LivenessInfo> {
    let mut defs = HashMap::new();
    let mut uses: Vec<Vec<(usize, Variable)>> = vec![Vec::new(); builder.blocks.len()];

    for block in builder.blocks.iter() {
        for (phi_index, phi) in block.phis.iter().enumerate() {
            defs.insert(phi.var, (block.id, phi_index));
        }

        let instr_base = block.phis.len();
        for (instr_index, instr) in block.instrs.iter().enumerate() {
            let position = instr_base + instr_index;
            for var in instr.get_use()? {
                uses[block.id].push((position, *var));
            }
            for var in instr.get_def()? {
                defs.insert(*var, (block.id, position));
            }
        }
    }

    for block in builder.blocks.iter() {
        let edge_position = block.phis.len() + block.instrs.len();
        for succ in block.successors.iter() {
            for phi in builder.blocks[*succ].phis.iter() {
                if let Some((var, _)) = phi.operands.iter().find(|(_, pred)| *pred == block.id) {
                    uses[block.id].push((edge_position, *var));
                }
            }
        }
    }

    Ok(LivenessInfo {
        live_ins,
        live_outs,
        defs,
        uses,
    })
}

fn count_uses(builder: &Builder) -> Result<HashMap<Variable, usize>> {
    let mut uses = HashMap::new();

    for block in builder.blocks.iter() {
        for instr in block.instrs.iter() {
            for var in instr.get_use()? {
                *uses.entry(*var).or_insert(0) += 1;
            }
        }

        for phi in block.phis.iter() {
            for (var, _) in phi.operands.iter() {
                *uses.entry(*var).or_insert(0) += 1;
            }
        }
    }

    Ok(uses)
}

fn phi_uses_from_block(builder: &Builder, block_id: BasicBlockId) -> Vec<Variable> {
    let mut uses = Vec::new();

    for succ in builder.blocks[block_id].successors.iter() {
        for phi in builder.blocks[*succ].phis.iter() {
            if let Some((var, _)) = phi.operands.iter().find(|(_, pred)| *pred == block_id) {
                uses.push(*var);
            }
        }
    }

    uses
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brilir::{
        compute_liveness,
        instruction::{Immediate, IrInstruction},
    };
    use crate::ssa::build_ssa;

    fn var(id: usize) -> Variable {
        Variable { id, index: 0 }
    }

    #[test]
    fn reuses_one_register_for_many_dead_variables() -> Result<()> {
        let mut builder = Builder::new();
        builder.add_block(0);

        for id in 0..500 {
            builder.add_instr(IrInstruction::Load(var(id), Immediate::Int(id as i64)));
        }
        builder.add_instr(IrInstruction::Print(var(499)));
        builder.add_instr(IrInstruction::Ret(var(499)));

        let allocation = allocate_registers(&builder)?;

        assert_eq!(allocation.register_count(), 1);
        Ok(())
    }

    #[test]
    fn allocates_one_hundred_values_live_across_join() -> Result<()> {
        let variable_count = 100;
        let mut builder = Builder::new();

        builder.add_block(0);
        builder.add_instr(IrInstruction::Load(
            var(variable_count),
            Immediate::Bool(true),
        ));
        builder.add_instr(IrInstruction::Br(var(variable_count), 1, 2));

        builder.add_block(1);
        for id in 0..variable_count {
            builder.add_instr(IrInstruction::Load(
                var(id),
                Immediate::Int((id + 1) as i64),
            ));
        }
        builder.add_instr(IrInstruction::Jmp(3));

        builder.add_block(2);
        for id in 0..variable_count {
            builder.add_instr(IrInstruction::Load(
                var(id),
                Immediate::Int((id + 2) as i64),
            ));
        }

        builder.add_block(3);
        builder.add_instr(IrInstruction::Binary(
            crate::brilir::instruction::BinaryOp::Add,
            var(variable_count + 1),
            var(0),
            var(1),
        ));
        for id in 2..variable_count {
            builder.add_instr(IrInstruction::Binary(
                crate::brilir::instruction::BinaryOp::Add,
                var(variable_count + 1),
                var(variable_count + 1),
                var(id),
            ));
        }
        builder.add_instr(IrInstruction::Ret(var(variable_count + 1)));

        builder.add_edge(0, 1);
        builder.add_edge(0, 2);
        builder.add_edge(1, 3);
        builder.add_edge(2, 3);

        builder.liveness = compute_liveness(&mut builder)?;
        build_ssa(&mut builder)?;
        let allocation = allocate_registers(&builder)?;

        assert_eq!(allocation.register_count(), variable_count);
        Ok(())
    }
}
