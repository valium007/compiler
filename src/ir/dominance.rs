use crate::ir::brilir::BasicBlock;
use crate::ir::id::*;
use hashbrown::HashSet;

pub struct Dominance {
    rpo: Vec<BasicBlockId>,
    po_map: Vec<BasicBlockId>,
}

impl Dominance {
    pub fn new() -> Self {
        Dominance {
            rpo: Vec::new(),
            po_map: Vec::new(),
        }
    }

    pub fn compute_df(
        &mut self,
        cfg: &Vec<Vec<BasicBlockId>>,
        blocks: &Vec<BasicBlock>,
    ) -> Vec<HashSet<BasicBlockId>> {
        let mut df: Vec<HashSet<BasicBlockId>> = vec![HashSet::new(); cfg.len()];

        let mut runner = None;
        let mut doms = self.compute_idom(cfg, blocks);
        for node in 0..cfg.len() {
            if blocks[node].preds.len() >= 2 {
                for pred in blocks[node].preds.iter() {
                    runner = Some(*pred);
                    while runner != doms[node] {
                        df[runner.unwrap()].insert(node);
                        runner = doms[runner.unwrap()]
                    }
                }
            }
        }
        df
    }

    pub fn compute_idom(
        &mut self,
        cfg: &Vec<Vec<BasicBlockId>>,
        blocks: &Vec<BasicBlock>,
    ) -> Vec<Option<BasicBlockId>> {
        let mut doms: Vec<Option<BasicBlockId>> = vec![None; cfg.len()];

        doms[0] = Some(0); //start node

        let mut changed = true;

        while changed {
            changed = false;
            for b in self.rpo.iter().filter(|node| **node != 0) {
                let processed = processed_pred(&blocks[*b].preds, &doms);
                //println!("node: {} processed pred: {:?} remaining pred: {:?}", b, processed.0, processed.1);

                let mut new_idom = processed.0;

                for p in processed.1 {
                    if doms[p] != None {
                        new_idom = self.intersect(Some(p), new_idom, &doms)
                    }
                }

                if doms[*b] != new_idom {
                    doms[*b] = new_idom;
                    changed = true;
                }
            }
        }

        println!("idoms: {:?}", doms);
        doms
    }

    pub fn intersect(
        &self,
        b1: Option<BasicBlockId>,
        b2: Option<BasicBlockId>,
        doms: &Vec<Option<BasicBlockId>>,
    ) -> Option<BasicBlockId> {
        let mut finger1 = b1;
        let mut finger2 = b2;

        while finger1 != finger2 {
            while self.po_map[finger1.unwrap()] < self.po_map[finger2.unwrap()] {
                finger1 = doms[finger1.unwrap()];
            }
            while self.po_map[finger2.unwrap()] < self.po_map[finger1.unwrap()] {
                finger2 = doms[finger2.unwrap()];
            }
        }

        finger1
    }

    pub fn compute_rpo(&mut self, cfg: &Vec<Vec<BasicBlockId>>) {
        let mut visited: HashSet<BasicBlockId> = HashSet::new();
        let mut po_list: Vec<BasicBlockId> = Vec::new();

        dfs(0, cfg, &mut visited, &mut po_list);

        let mut po_map = vec![0; cfg.len()];

        for (i, node) in po_list.iter().enumerate() {
            po_map[*node] = i;
        }

        po_list.reverse();
        //println!("rpo_list: {:?}", po_list);

        self.po_map = po_map.clone();
        self.rpo = po_list.clone();
    }
}

pub fn processed_pred(
    preds: &Vec<BasicBlockId>,
    doms: &Vec<Option<BasicBlockId>>,
) -> (Option<BasicBlockId>, Vec<BasicBlockId>) {
    let mut idom = None;
    let mut remaining_preds: Vec<BasicBlockId> = Vec::new();
    for p in preds {
        if doms[*p] != None {
            idom = Some(*p);
            break;
        }
    }

    if idom != None {
        remaining_preds = preds
            .iter()
            .filter(|pred| Some(**pred) != idom)
            .map(|pred| pred.to_owned())
            .collect();
    }

    (idom, remaining_preds)
}

pub fn dfs(
    node: BasicBlockId,
    cfg: &Vec<Vec<BasicBlockId>>,
    visited: &mut HashSet<BasicBlockId>,
    po_list: &mut Vec<BasicBlockId>,
) {
    visited.insert(node);

    for child in cfg[node].iter() {
        if !visited.contains(child) {
            dfs(*child, cfg, visited, po_list);
        }
    }
    po_list.push(node);
}
