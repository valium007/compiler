//! Trace-based block coloring order (paper §4 / Algorithm 4).
//!
//! Color hot paths first, building each chain backward to entry so that
//! every block is colored after at least one non-back-edge predecessor.
//! This respects the SSA-allocator dominance requirement (entry is always
//! first; every other block has a colored forward predecessor by the time
//! it's processed).
//!
//! Step-by-step:
//!   1. `trace[b] = max(trace[non-back-edge preds]) + freq(b)`, computed
//!      via RPO.
//!   2. Sort blocks by `trace[b]` descending.
//!   3. For each block in that order, recursively prepend its highest-
//!      `trace` non-back-edge predecessor that isn't already in the
//!      output. The chain runs entry → ... → block.

use super::cfg::Cfg;
use super::AllocFunction;

/// Compute a trace-based ordering of blocks.
///
/// `freqs[b]` is the relative execution frequency of block b
/// (loop_depth + 1 is a fine choice when no profile data exists).
/// Blocks unreachable from entry are appended at the end in numeric
/// order so the assignment pass can still process them.
pub fn trace_order<F: AllocFunction>(func: &F, cfg: &Cfg, freqs: &[u32]) -> Vec<usize> {
    let n = func.num_blocks();
    let dom = cfg.dominators();

    // Step 1: trace values via RPO over the reachable part of the CFG.
    // Back edges (u → v with v dominating u) are skipped in the max.
    let rpo = cfg.reverse_postorder();
    let mut trace = vec![0u64; n];
    for &b in &rpo {
        let mut t: u64 = 0;
        for &p in func.block_predecessors(b) {
            // Skip back edges.
            if dom.dominates(b, p) {
                continue;
            }
            if trace[p] > t {
                t = trace[p];
            }
        }
        trace[b] = t + freqs.get(b).copied().unwrap_or(1) as u64;
    }

    // Step 2: sort reachable blocks by trace descending.
    let reachable_set: std::collections::HashSet<usize> = rpo.iter().copied().collect();
    let mut sorted: Vec<usize> = rpo.clone();
    sorted.sort_by(|a, b| trace[*b].cmp(&trace[*a]));

    // Step 3: build the order chain-by-chain.
    let mut order: Vec<usize> = Vec::with_capacity(n);
    let mut in_order = vec![false; n];
    for &b in &sorted {
        add_chain(b, func, &dom, &trace, &mut in_order, &mut order);
    }

    // Append unreachable blocks at the end (rare but defensive).
    for b in 0..n {
        if !reachable_set.contains(&b) && !in_order[b] {
            in_order[b] = true;
            order.push(b);
        }
    }
    order
}

fn add_chain<F: AllocFunction>(
    block: usize,
    func: &F,
    dom: &super::cfg::CfgDominators,
    trace: &[u64],
    in_order: &mut [bool],
    order: &mut Vec<usize>,
) {
    if in_order[block] {
        return;
    }
    // Find best non-back-edge predecessor.
    let mut best_trace: u64 = 0;
    let mut best_pred: Option<usize> = None;
    for &p in func.block_predecessors(block) {
        if dom.dominates(block, p) {
            continue; // back-edge
        }
        if trace[p] >= best_trace {
            best_trace = trace[p];
            best_pred = Some(p);
        }
    }
    if let Some(p) = best_pred {
        add_chain(p, func, dom, trace, in_order, order);
    }
    if !in_order[block] {
        in_order[block] = true;
        order.push(block);
    }
}

/// Convenience: derive `freqs` from `func.loop_depth(b)` as `2^depth`, so
/// each level of nesting multiplies weight by 2. Capped to avoid u32
/// overflow for absurdly nested loops.
pub fn freqs_from_loop_depth<F: AllocFunction>(func: &F) -> Vec<u32> {
    let n = func.num_blocks();
    let mut freqs = vec![1u32; n];
    for b in 0..n {
        let d = func.loop_depth(b).min(20);
        freqs[b] = 1u32 << d;
    }
    freqs
}
