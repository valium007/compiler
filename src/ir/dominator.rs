use crate::ir::id::BasicBlockId;
use hashbrown::HashMap;
use std::collections::BTreeSet;

pub struct Dominator {
    rpo: Vec<BasicBlockId>,
    po_map: HashMap<BasicBlockId, usize>,
    dom_tree: Vec<Option<BasicBlockId>>,
}

impl Dominator {
    pub fn new() -> Self {
        Self {
            rpo: Vec::new(),
            po_map: HashMap::new(),
            dom_tree: Vec::new(),
        }
    }

    pub fn compute(
        &mut self,
        nodes: &Vec<BasicBlockId>,
        start_node: BasicBlockId,
        succs: &HashMap<BasicBlockId, BTreeSet<BasicBlockId>>,
        preds: &HashMap<BasicBlockId, BTreeSet<BasicBlockId>>,
    ) -> Vec<BTreeSet<BasicBlockId>> {
        self.rpo(start_node, succs);
        self.compute_idom(nodes, start_node, preds);
        self.compute_df(nodes, preds)
    }

    pub fn processed_pred(
        &mut self,
        preds: &HashMap<BasicBlockId, BTreeSet<BasicBlockId>>,
        doms: &Vec<Option<BasicBlockId>>,
        node: BasicBlockId,
    ) -> (Option<BasicBlockId>, BTreeSet<usize>) {
        let mut new_idom: Option<BasicBlockId> = None;
        let mut current_preds = preds.get(&node).unwrap().clone();
        for p in current_preds.iter() {
            if doms[*p] != None {
                new_idom = Some(*p);
                break;
            }
        }
        if new_idom != None {
            current_preds.remove(&new_idom.unwrap());
        }

        (new_idom, current_preds)
    }

    pub fn compute_idom(
        &mut self,
        nodes: &Vec<BasicBlockId>,
        start_node: BasicBlockId,
        preds: &HashMap<BasicBlockId, BTreeSet<BasicBlockId>>,
    ) {
        let mut doms: Vec<Option<BasicBlockId>> = vec![None; nodes.len()];

        let mut worklist = self.rpo.clone();

        if let Some(index) = worklist.iter().position(|&r| r == start_node) {
            worklist.remove(index);
        }

        doms[start_node] = Some(start_node);

        let mut changed = true;
        while changed {
            changed = false;

            for b in worklist.iter() {
                let data = self.processed_pred(preds, &doms, *b);
                let pp = data.0;
                let other_preds = data.1;

                if pp == None {
                    continue;
                }

                let mut new_idom = pp;

                for p in other_preds {
                    if doms[p] != None {
                        new_idom = self.intersect(Some(p), new_idom, &doms);
                    }
                }

                if doms[*b] != new_idom {
                    doms[*b] = new_idom;
                    changed = true
                }
            }
        }

        self.dom_tree = doms
    }

    pub fn compute_df(
        &mut self,
        nodes: &Vec<BasicBlockId>,
        preds: &HashMap<BasicBlockId, BTreeSet<BasicBlockId>>,
    ) -> Vec<BTreeSet<BasicBlockId>> {
        let mut df: Vec<BTreeSet<BasicBlockId>> = vec![BTreeSet::new(); nodes.len()];

        for b in nodes {
            if preds.get(b).unwrap().len() >= 2 {
                for p in preds.get(b).unwrap().iter() {
                    let mut runner = *p;
                    while runner != self.dom_tree[*b].unwrap() {
                        df[runner].insert(*b);
                        runner = self.dom_tree[runner].unwrap();
                    }
                }
            }
        }
        df
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
            while self.po_map.get(&finger1.unwrap()) < self.po_map.get(&finger2.unwrap()) {
                finger1 = doms[finger1.unwrap()];
            }

            while self.po_map.get(&finger2.unwrap()) < self.po_map.get(&finger1.unwrap()) {
                finger2 = doms[finger2.unwrap()]
            }
        }
        return finger1;
    }

    pub fn rpo(
        &mut self,
        start_node: BasicBlockId,
        succs: &HashMap<BasicBlockId, BTreeSet<BasicBlockId>>,
    ) {
        let mut visited: BTreeSet<BasicBlockId> = BTreeSet::new();
        let mut postorder: Vec<BasicBlockId> = Vec::new();

        fn postorder_dfs(
            node: BasicBlockId,
            visited: &mut BTreeSet<BasicBlockId>,
            postorder: &mut Vec<BasicBlockId>,
            succs: &HashMap<BasicBlockId, BTreeSet<BasicBlockId>>,
        ) {
            visited.insert(node);

            if let Some(children) = succs.get(&node) {
                for &child in children {
                    if !visited.contains(&child) {
                        postorder_dfs(child, visited, postorder, succs);
                    }
                }
            }
            postorder.push(node);
        }

        postorder_dfs(start_node, &mut visited, &mut postorder, succs);

        let mut po_map: HashMap<BasicBlockId, usize> = HashMap::new();
        for (i, &node) in postorder.iter().enumerate() {
            po_map.insert(node, i);
        }

        let mut rpo_list = postorder.clone();
        rpo_list.reverse();

        self.rpo = rpo_list;
        self.po_map = po_map;

        //println!("rpo:{:?} po_map:{:?}", self.rpo, self.po_map);
    }
}
