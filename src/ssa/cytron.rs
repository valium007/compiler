use anyhow::Result;
use hashbrown::{HashMap, HashSet};
use petgraph::algo::dominators::simple_fast;
use petgraph::prelude::GraphMap;

use crate::brilir::builder::{BasicBlock, BasicBlockId, Builder, Phi};
use crate::brilir::id::ValueId;
use crate::brilir::instruction::Variable;

pub fn insert_phi(builder: &mut Builder, df: Vec<Vec<BasicBlockId>>) -> Result<()> {
    let livein = builder.liveness.0.clone();

    let vars = get_vars_assignment(&builder.blocks)?;
    for var in vars.keys() {
        let mut worklist: Vec<BasicBlockId> = Vec::new();
        let mut ever_on_worklist: HashSet<BasicBlockId> = HashSet::new();
        let mut already_has_phi: HashSet<BasicBlockId> = HashSet::new();

        for n in vars[var].iter() {
            worklist.push(*n);
            ever_on_worklist.insert(*n);
        }

        while !worklist.is_empty() {
            if let Some(block) = worklist.pop() {
                for d in df[block].iter() {
                    if !already_has_phi.contains(d) && livein[*d].contains(var) {
                        //insert phi
                        builder.blocks[*d].phis.push(Phi {
                            var: Variable {
                                id: var.id,
                                index: 0,
                            },
                            operands: Vec::new(),
                        });

                        already_has_phi.insert(*d);

                        if !ever_on_worklist.contains(d) {
                            worklist.push(*d);
                            ever_on_worklist.insert(*d);
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

pub fn run_rename(builder: &mut Builder, bb: BasicBlockId) -> Result<()> {
    let n = get_max_variable_id(&builder.blocks).map_or(0, |id| id + 1);
    let mut stacks: Vec<Vec<ValueId>> = vec![Vec::new(); n];
    let mut counters: Vec<ValueId> = vec![0; n];

    let map = get_graph(builder)?;
    let idom = simple_fast(&map, bb);
    let mut dom_children: Vec<Vec<BasicBlockId>> = vec![Vec::new(); builder.blocks.len()];

    for block in builder.blocks.iter() {
        if block.id == bb {
            continue;
        }
        if let Some(parent) = idom.immediate_dominator(block.id)
            && parent != block.id
        {
            dom_children[parent].push(block.id);
        }
    }

    rename(builder, bb, &dom_children, &mut stacks, &mut counters)?;
    Ok(())
}

pub fn rename(
    builder: &mut Builder,
    bb: BasicBlockId,
    dom_children: &[Vec<BasicBlockId>],
    stacks: &mut Vec<Vec<ValueId>>,
    counters: &mut Vec<ValueId>,
) -> Result<()> {
    let succs = builder.blocks[bb].successors.clone();
    let mut pushed: Vec<usize> = Vec::new();

    for phi in builder.blocks[bb].phis.iter_mut() {
        gen_name(&mut phi.var, stacks, counters)?;
        pushed.push(phi.var.id);
    }

    for instr in builder.blocks[bb].instrs.iter_mut() {
        for var in instr.get_rhs_mut()? {
            var.index = current_name(var.id, stacks)?;
        }
        for var in instr.get_lhs_mut()? {
            gen_name(var, stacks, counters)?;
            pushed.push(var.id);
        }
    }

    for succ in succs.iter() {
        for phi in builder.blocks[*succ].phis.iter_mut() {
            let new_var = Variable {
                id: phi.var.id,
                index: current_name(phi.var.id, stacks)?,
            };

            if let Some((var, _)) = phi.operands.iter_mut().find(|(_, pred)| *pred == bb) {
                *var = new_var;
            } else {
                phi.operands.push((new_var, bb));
            }
        }
    }

    for child in dom_children[bb].iter() {
        rename(builder, *child, dom_children, stacks, counters)?;
    }

    for var_id in pushed.into_iter().rev() {
        stacks[var_id].pop();
    }
    Ok(())
}

pub fn gen_name(
    var: &mut Variable,
    stacks: &mut [Vec<ValueId>],
    counters: &mut [ValueId],
) -> Result<()> {
    let i = counters[var.id];
    var.index = i;
    stacks[var.id].push(i);
    counters[var.id] = i + 1;
    Ok(())
}

fn current_name(var_id: usize, stacks: &[Vec<ValueId>]) -> Result<ValueId> {
    stacks
        .get(var_id)
        .and_then(|stack| stack.last())
        .copied()
        .ok_or_else(|| anyhow::anyhow!("variable v{} has no SSA name", var_id))
}

fn get_max_variable_id(blocks: &[BasicBlock]) -> Option<usize> {
    let mut max_id = None;
    for block in blocks {
        for phi in block.phis.iter() {
            max_id = Some(max_id.map_or(phi.var.id, |id: usize| id.max(phi.var.id)));
            for (var, _) in phi.operands.iter() {
                max_id = Some(max_id.map_or(var.id, |id: usize| id.max(var.id)));
            }
        }
        for instr in block.instrs.iter() {
            for var in instr.get_def().unwrap_or_default() {
                max_id = Some(max_id.map_or(var.id, |id: usize| id.max(var.id)));
            }
            for var in instr.get_use().unwrap_or_default() {
                max_id = Some(max_id.map_or(var.id, |id: usize| id.max(var.id)));
            }
        }
    }
    max_id
}

pub fn get_vars_assignment(blocks: &[BasicBlock]) -> Result<HashMap<Variable, Vec<BasicBlockId>>> {
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
    let map = get_graph(builder)?;

    let idom = simple_fast(&map, 0);

    let mut df: Vec<Vec<BasicBlockId>> = vec![Vec::new(); n];

    for block in builder.blocks.iter() {
        if block.predecessors.len() >= 2 {
            let Some(block_idom) = idom.immediate_dominator(block.id) else {
                continue;
            };

            for pred in block.predecessors.iter() {
                let mut runner = *pred;
                while runner != block_idom {
                    df[runner].push(block.id);
                    let Some(next_runner) = idom.immediate_dominator(runner) else {
                        break;
                    };
                    if next_runner == runner {
                        break;
                    }
                    runner = next_runner;
                }
            }
        }
    }
    Ok(df)
}

fn get_graph(builder: &Builder) -> Result<GraphMap<BasicBlockId, (), petgraph::Directed>> {
    let mut map = GraphMap::<BasicBlockId, (), petgraph::Directed>::new();
    for block in builder.blocks.iter() {
        map.add_node(block.id);
    }
    for (from, to) in get_edges(builder)? {
        map.add_edge(from, to, ());
    }
    Ok(map)
}
