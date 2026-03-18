use anyhow::{Ok, Result};
use hashbrown::HashSet;

use crate::brilir::{Builder, builder::BasicBlock, instruction::Variable};

pub fn compute_liveness(
    builder: &mut Builder,
) -> Result<(Vec<HashSet<Variable>>, Vec<HashSet<Variable>>)> {
    let n = builder.blocks.len();

    let mut live_out: Vec<HashSet<Variable>> = vec![HashSet::new(); n];
    let mut live_in: Vec<HashSet<Variable>> = vec![HashSet::new(); n];
    let mut uses: Vec<HashSet<Variable>> = vec![HashSet::new(); n];
    let mut defs: Vec<HashSet<Variable>> = vec![HashSet::new(); n];

    for (i, block) in builder.blocks.iter().enumerate() {
        let (d, u) = compute_def_use(block)?;
        println!("use: {:?} def: {:?}", u, d);
        uses[i] = u;
        defs[i] = d;
    }

    loop {
        let mut changed = false;

        for b in (0..n).rev() {
            let old_in = live_in[b].clone();
            let old_out = live_out[b].clone();

            live_out[b].clear();
            for &s in &builder.blocks[b].successors {
                live_out[b].extend(live_in[s].iter().cloned());
            }

            let mut differences: HashSet<Variable> = HashSet::new();
            for var in &live_out[b] {
                if defs[b].contains(var) {
                    continue;
                }
                differences.insert(var.clone());
            }

            live_in[b].clear();
            live_in[b].extend(uses[b].iter().cloned());
            live_in[b].extend(differences.iter().cloned());

            if old_in != live_in[b] || old_out != live_out[b] {
                changed = true;
            }
        }

        if !changed {
            break;
        }
    }

    Ok((live_in, live_out))
}

pub fn compute_def_use(bb: &BasicBlock) -> Result<(HashSet<Variable>, HashSet<Variable>)> {
    let mut defs = HashSet::new();
    let mut uses = HashSet::new();

    for inst in &bb.instrs {
        for def in inst.get_def()? {
            defs.insert(def.clone());
        }
        for u in inst.get_use()? {
            if !defs.contains(u) {
                uses.insert(u.clone());
            }
        }
    }
    Ok((defs, uses))
}
