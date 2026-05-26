//! CFG wrapper backed by petgraph.
//!
//! Lifts an `AllocFunction`'s block-successor/predecessor lists into a
//! `petgraph::DiGraph` and exposes the graph utilities the rest of
//! bregalloc relies on: reverse-postorder traversal and the dominator tree.
//!
//! Node weights are block ids (the indices the AllocFunction trait uses);
//! edge weights are unit. The graph is built once per allocation.

use petgraph::algo::dominators::{self, Dominators};
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::DfsPostOrder;

use super::AllocFunction;

pub struct Cfg {
    graph: DiGraph<usize, ()>,
    /// block_id -> NodeIndex
    node_of: Vec<NodeIndex>,
    /// Entry block id (assumed to be 0).
    entry: NodeIndex,
}

impl Cfg {
    pub fn build<F: AllocFunction>(func: &F) -> Self {
        let n = func.num_blocks();
        let succs: Vec<Vec<usize>> = (0..n).map(|b| func.block_successors(b).to_vec()).collect();
        Self::build_from_slices(n, &succs)
    }

    /// Construct from raw block/successor data, without requiring the
    /// full AllocFunction trait. Useful for adapter code that has the
    /// CFG in plain Vec<Vec<usize>> form.
    pub fn build_from_slices(num_blocks: usize, succs: &[Vec<usize>]) -> Self {
        let mut graph: DiGraph<usize, ()> = DiGraph::with_capacity(num_blocks, num_blocks * 2);
        let mut node_of = Vec::with_capacity(num_blocks);
        for b in 0..num_blocks {
            node_of.push(graph.add_node(b));
        }
        for b in 0..num_blocks {
            for &s in &succs[b] {
                graph.add_edge(node_of[b], node_of[s], ());
            }
        }
        let entry = node_of[0];
        Self { graph, node_of, entry }
    }

    pub fn entry_block(&self) -> usize { 0 }

    pub fn node_of(&self, block: usize) -> NodeIndex { self.node_of[block] }

    pub fn block_of(&self, node: NodeIndex) -> usize {
        self.graph[node]
    }

    pub fn graph(&self) -> &DiGraph<usize, ()> { &self.graph }

    /// Reverse-postorder traversal of blocks reachable from entry. Blocks
    /// unreachable from entry are omitted — the caller must decide whether
    /// to process them separately or ignore them.
    pub fn reverse_postorder(&self) -> Vec<usize> {
        let mut dfs = DfsPostOrder::new(&self.graph, self.entry);
        let mut post: Vec<usize> = Vec::new();
        while let Some(nx) = dfs.next(&self.graph) {
            post.push(self.graph[nx]);
        }
        post.reverse();
        post
    }

    /// Dominator tree rooted at entry. Use `dominators.dominators(block)`
    /// to walk a block's ancestors, or `immediate_dominator(block)` for
    /// the parent.
    pub fn dominators(&self) -> CfgDominators {
        CfgDominators {
            doms: dominators::simple_fast(&self.graph, self.entry),
            node_of: self.node_of.clone(),
        }
    }

    /// Loop nesting depth per block, derived from back-edges in the DFS
    /// tree. Edges (u → v) where v dominates u count as back-edges and
    /// contribute to the depth of every block on the natural-loop body.
    /// This is a coarse estimate, good enough for frequency weighting.
    pub fn loop_depths(&self) -> Vec<u32> {
        let n = self.graph.node_count();
        let mut depth = vec![0u32; n];
        let doms = dominators::simple_fast(&self.graph, self.entry);

        // Collect back edges: (u → v) with v dominating u.
        for u_idx in self.graph.node_indices() {
            for v_idx in self.graph.neighbors(u_idx) {
                if dominates(&doms, v_idx, u_idx) {
                    // v dominates u; (u → v) is a back-edge. Walk natural
                    // loop body: blocks that reach u without going through v.
                    self.bump_loop_body(&mut depth, v_idx, u_idx);
                }
            }
        }
        // Re-map to block-id-indexed.
        let mut by_block = vec![0u32; n];
        for (i, &node) in self.node_of.iter().enumerate() {
            by_block[i] = depth[node.index()];
        }
        by_block
    }

    fn bump_loop_body(&self, depth: &mut [u32], header: NodeIndex, tail: NodeIndex) {
        // Standard "find natural loop body": reverse BFS from tail, stopping at header.
        let mut stack = vec![tail];
        let mut seen = vec![false; self.graph.node_count()];
        seen[header.index()] = true;
        seen[tail.index()] = true;
        depth[header.index()] += 1;
        if header != tail {
            depth[tail.index()] += 1;
        }
        while let Some(u) = stack.pop() {
            for pred in self.graph.neighbors_directed(u, petgraph::Direction::Incoming) {
                if !seen[pred.index()] {
                    seen[pred.index()] = true;
                    depth[pred.index()] += 1;
                    stack.push(pred);
                }
            }
        }
    }
}

fn dominates(doms: &Dominators<NodeIndex>, dominator: NodeIndex, of: NodeIndex) -> bool {
    if dominator == of { return true; }
    let mut cur = of;
    while let Some(idom) = doms.immediate_dominator(cur) {
        if idom == dominator { return true; }
        if idom == cur { return false; }
        cur = idom;
    }
    false
}

pub struct CfgDominators {
    doms: Dominators<NodeIndex>,
    node_of: Vec<NodeIndex>,
}

impl CfgDominators {
    pub fn immediate_dominator(&self, block: usize) -> Option<usize> {
        let node = self.node_of[block];
        self.doms.immediate_dominator(node).map(|n| {
            // Find which block this NodeIndex corresponds to.
            self.node_of.iter().position(|&nx| nx == n).expect("dominator must be a known block")
        })
    }

    pub fn dominates(&self, a: usize, b: usize) -> bool {
        let na = self.node_of[a];
        let nb = self.node_of[b];
        dominates(&self.doms, na, nb)
    }
}
