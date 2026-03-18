use anyhow::Result;
use hashbrown::{HashMap, HashSet};
use petgraph::algo::dominators::simple_fast;
use petgraph::prelude::GraphMap;

use crate::brilir::builder::{BasicBlock, Builder, Phi, BasicBlockId};
use crate::brilir::instruction::Variable;
use crate::brilir::id::ValueId;

pub fn insert_phi(builder: &mut Builder, df: Vec<Vec<BasicBlockId>>) -> Result<()> {
    let livein = builder.liveness.0.clone();

    let vars = get_vars_assignment(&builder.blocks)?;
    for var in vars.keys() {
        let mut worklist: Vec<BasicBlockId> = Vec::new();
        let mut ever_on_worklist: Vec<BasicBlockId> = Vec::new();
        let mut already_has_phi: Vec<BasicBlockId> = Vec::new();

        for n in vars[var].iter() {
            worklist.push(*n);
        }

        ever_on_worklist = worklist.clone();

        while !worklist.is_empty() {
            if let Some(block) = worklist.pop() {
                for d in df[block].iter() {
                    if !already_has_phi.contains(&d) && livein[*d].contains(var) {
                        //insert phi
                        builder.blocks[*d].phis.push(Phi {
                            var: Variable {
                                id: var.id,
                                index: 0,
                            },
                            operands: Vec::new(),
                        });

                        already_has_phi.push(*d);

                        if !ever_on_worklist.contains(d) {
                            worklist.push(*d);
                            ever_on_worklist.push(*d);
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

pub fn run_rename(builder: &mut Builder, bb: BasicBlockId) -> Result<()> {
    let n = get_vars_assignment(&builder.blocks)?.len();
    let mut stacks: Vec<Vec<ValueId>> = Vec::new();
    let mut counters: Vec<ValueId> = Vec::new();

    let mut visited: HashSet<BasicBlockId> = HashSet::new();

    for i in 0..n {
        counters.insert(i, 0);
        stacks.insert(i, Vec::new());
    }
    rename(builder, bb, &mut stacks, &mut counters, &mut visited);
    Ok(())
}

pub fn rename(
    builder: &mut Builder,
    bb: BasicBlockId,
    stacks: &mut Vec<Vec<ValueId>>,
    counters: &mut Vec<ValueId>,
    visited: &mut HashSet<BasicBlockId>,
) -> Result<()> {
    if visited.contains(&bb) {
        return Ok(());
    }

    visited.insert(bb);

    let succs = builder.blocks[bb].successors.clone();

    for phi in builder.blocks[bb].phis.iter_mut() {
        gen_name(&mut phi.var, stacks, counters);
    }

    for instr in builder.blocks[bb].instrs.iter_mut() {
        for var in instr.get_rhs_mut()? {
            var.index = *stacks[var.id].last().unwrap();
        }
        for var in instr.get_lhs_mut()? {
            gen_name(var, stacks, counters);
        }
    }

    for succ in succs.iter() {
        let mut j = 0;
        for (i, pred) in builder.blocks[*succ].predecessors.iter().enumerate() {
            if *pred == bb {
                j = i;
            }
        }

        for phi in builder.blocks[*succ].phis.iter_mut() {
            let top = stacks[phi.var.id].last().unwrap();
            let new_var = Variable {
                id: phi.var.id,
                index: *top,
            };
            if phi.operands.len() <= j {
                phi.operands.resize(
                    j + 1,
                    (
                        Variable {
                            id: phi.var.id,
                            index: 0,
                        },
                        0,
                    ),
                );
            }
            phi.operands[j] = (new_var, bb);
        }
    }

    for succ in succs.iter() {
        rename(builder, *succ, stacks, counters, visited);
    }
    for phi in builder.blocks[bb].phis.iter_mut() {
        stacks[phi.var.id].pop();
    }
    for instr in builder.blocks[bb].instrs.iter_mut() {
        for var in instr.get_lhs_mut()? {
            stacks[var.id].pop();
        }
    }
    Ok(())
}

pub fn gen_name(
    var: &mut Variable,
    stacks: &mut Vec<Vec<ValueId>>,
    counters: &mut Vec<ValueId>,
) -> Result<()> {
    let i = counters[var.id];
    var.index = i;
    stacks[var.id].push(i);
    counters[var.id] = i + 1;
    Ok(())
}

pub fn get_vars_assignment(
    blocks: &Vec<BasicBlock>,
) -> Result<HashMap<Variable, Vec<BasicBlockId>>> {
    let mut vars: HashMap<Variable, Vec<BasicBlockId>> = HashMap::new();
    for (i, block) in blocks.iter().enumerate() {
        for instr in block.instrs.iter() {
            let def = instr.get_def()?;
            for var in def {
                vars.entry(*var).or_default().push(i);
            }
        }
    }
    Ok(vars)
}

pub fn get_edges(builder: &Builder) -> Result<Vec<(BasicBlockId, BasicBlockId)>> {
    let mut edges = Vec::new();
    for block in builder.blocks.iter() {
        for succ in block.successors.iter() {
            edges.push((block.id, *succ));
        }
    }
    Ok(edges)
}

pub fn compute_df(builder: &Builder) -> Result<Vec<Vec<BasicBlockId>>> {
    let n = builder.blocks.len();
    let map = GraphMap::<BasicBlockId, (), petgraph::Directed>::from_edges(get_edges(builder)?);

    let idom = simple_fast(&map, 0);

    let mut df: Vec<Vec<BasicBlockId>> = vec![Vec::new(); n];

    for block in builder.blocks.iter() {
        if block.predecessors.len() >= 2 {
            for pred in block.predecessors.iter() {
                let mut runner = *pred;
                while runner != idom.immediate_dominator(block.id).unwrap() {
                    df[runner].push(block.id);
                    runner = idom.immediate_dominator(runner).unwrap();
                }
            }
        }
    }
    Ok(df)
}
