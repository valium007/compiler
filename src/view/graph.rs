//! Layout the SSA control-flow graph for the viewer.
//!
//! Pure data: takes `&[ssa::ir::Builder]`, returns positioned blocks and
//! edge polylines. No GPUI types here — this is the easy half to test
//! deterministically and the part that should outlive any UI rewrite.
//!
//! # Algorithm
//!
//! 1. Layer the DAG by longest path: every node's layer is one more than
//!    the max layer of its predecessors, with back-edges (cycles) treated
//!    as if they pointed back to the source — a coarse approximation that
//!    still gives a useful top-to-bottom flow for CFGs (where cycles are
//!    `br` to a loop header).
//! 2. Within a layer, sort by barycenter of predecessors to reduce edge
//!    crossings. Iterate the order once; one pass is enough for our scale
//!    (tens of blocks, not thousands).
//! 3. Position nodes on a grid: row = layer index, column = order within
//!    layer. Block width grows with the longest instruction line; block
//!    height grows with the number of instructions.
//!
//! The result is intentionally simple: a top-to-bottom flow where the
//! first block sits at the top and successors cascade downward. The
//! viewer lets you pan/zoom if a function gets large.

use crate::ssa::ir::{Builder, IrInstruction};
use std::collections::HashMap;

/// One rendered block: its position in the canvas, its size, and the text
/// lines that go inside the card.
#[derive(Debug, Clone)]
pub struct LaidOutBlock {
    pub fn_index: usize,
    pub block_id: usize,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// Header line ("@name_bb_3" or "entry bb_0").
    pub header: String,
    /// Body lines (one per instruction, phis first). Each line is the
    /// raw SSA `Debug` text — kept around so the detail panel and any
    /// future textual consumer can reuse it without re-formatting.
    pub body: Vec<String>,
    /// Same lines, split into typed tokens for the canvas painter.
    /// The painter uses `body_runs` to color each token by kind
    /// (variable, block label, integer, opcode, punctuation).
    pub body_runs: Vec<Vec<Token>>,
    /// True if this block has no predecessors (entry or unreachable).
    pub is_entry: bool,
}

/// A single token of an SSA instruction's Debug text, classified for
/// syntax highlighting. The painter in `app.rs` maps each variant to
/// a theme color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    /// SSA value: `%v0_0`, `%v12_3`, etc.
    Variable,
    /// Block label: `bb_0`, `bb_21`.
    Block,
    /// Integer literal: `0`, `981`, `-3` (we never produce negative in
    /// this IR, but the tokenizer is permissive).
    Integer,
    /// Boolean literal: `true` / `false`.
    Boolean,
    /// Undefined value marker: `undef`.
    Undef,
    /// SSA opcode: `const`, `mov`, `add`, `sub`, `mul`, `div`, `eq`,
    /// `lt`, `gt`, `le`, `ge`, `and`, `or`, `not`, `print`, `jmp`,
    /// `br`, `ret`, `call`, `phi`, `from`.
    Opcode,
    /// Punctuation: `=`, `?`, `:`, `,`, `(`, `)`, `[`, `]`, `.`, `-`.
    Punct,
    /// Whitespace and everything else.
    Other,
}

/// A token: a piece of the original text plus its classified kind.
/// Adjacent tokens of the same kind are not merged at the source
/// level — the painter is the only consumer and merging there is
/// trivial.
#[derive(Debug, Clone)]
pub struct Token {
    pub text: String,
    pub kind: TokenKind,
}

/// One edge between two blocks. We store both endpoints and a hint about
/// whether this was a back-edge in the source CFG (so the UI can render
/// it as a curved self-loop rather than a straight backward line).
#[derive(Debug, Clone)]
pub struct LaidOutEdge {
    pub fn_index: usize,
    pub from_block: usize,
    pub to_block: usize,
    pub is_back_edge: bool,
    /// Text label for the edge (e.g. "true", "false", or "").
    pub label: String,
}

/// One function's layout, kept together so the UI can render them in
/// separate sub-canvases or stack them vertically.
#[derive(Debug, Clone)]
pub struct LaidOutFunction {
    pub name: String,
    pub blocks: Vec<LaidOutBlock>,
    pub edges: Vec<LaidOutEdge>,
    pub width: f32,
    pub height: f32,
}

/// Top-level layout result. Functions are stacked vertically with a gap
/// so the user can scroll through a multi-function program.
#[derive(Debug, Clone)]
pub struct Layout {
    pub functions: Vec<LaidOutFunction>,
    pub total_width: f32,
    pub total_height: f32,
}

/// Visual constants. Tuned for an SSA CFG dump on a 1080p screen: cards
/// need to be wide enough for "phi [ %v1_0 from bb_0, %v2_0 from bb_2 ]"
/// without wrapping.
const BLOCK_PADDING_X: f32 = 16.0;
const BLOCK_PADDING_Y: f32 = 10.0;
const LINE_HEIGHT: f32 = 18.0;
const HEADER_HEIGHT: f32 = 22.0;
const MIN_BLOCK_WIDTH: f32 = 180.0;
/// Per-character width approximation at the body font. We measure the
/// longest line in pixels and use it for the card width. 7.2 px per
/// monospace glyph is a good fit for GPUI's default mono face.
const CHAR_WIDTH: f32 = 7.2;
const COLUMN_GAP: f32 = 160.0;
const ROW_GAP: f32 = 40.0;
const FUNCTION_GAP: f32 = 80.0;

pub fn layout(builders: &[Builder]) -> Layout {
    let mut functions = Vec::with_capacity(builders.len());
    let mut y_offset = 0.0_f32;
    let mut max_width: f32 = 0.0;

    for (fn_idx, builder) in builders.iter().enumerate() {
        let lo = layout_function(fn_idx, builder, y_offset);
        max_width = max_width.max(lo.width);
        y_offset += lo.height + FUNCTION_GAP;
        functions.push(lo);
    }

    Layout {
        functions,
        total_width: max_width,
        total_height: y_offset,
    }
}

fn layout_function(fn_idx: usize, builder: &Builder, y_offset: f32) -> LaidOutFunction {
    // ── Step 1: build block text + measure ───────────────────────────
    let n = builder.blocks.len();
    let mut blocks: Vec<LaidOutBlock> = Vec::with_capacity(n);
    for block in &builder.blocks {
        // Width is measured per block, not per function. A block with
        // only `const` instructions would otherwise inherit the
        // width of a long `call` elsewhere in the function and
        // look like a wide card with a small line centered in
        // lots of empty space.
        let mut max_body_width_chars = 0usize;
        let mut max_header_width_chars = 0usize;
        // Phis first (they always come at the top of a block in this IR),
        // then the rest of the instructions in original order. We skip
        // `Nop` — they are tombstones left by phi elimination and not
        // useful to display.
        let mut body: Vec<String> = Vec::with_capacity(block.instrs.len());
        let mut body_runs: Vec<Vec<Token>> = Vec::with_capacity(block.instrs.len());
        for instr in &block.instrs {
            if matches!(instr, IrInstruction::Nop) {
                continue;
            }
            // `IrInstruction` implements Debug — reuse the same text
            // format the DOT dumper uses, so the viewer matches the dump
            // the user already has on disk.
            let line = format!("{:?}", instr);
            max_body_width_chars = max_body_width_chars.max(line.chars().count());
            let runs = tokenize(&line);
            body.push(line);
            body_runs.push(runs);
        }

        let header = if block.predecessors.is_empty() {
            format!("entry bb_{}", block.id)
        } else {
            format!("@{} bb_{}", builder.name, block.id)
        };
        max_header_width_chars = max_header_width_chars.max(header.chars().count());

        let longest_chars = max_body_width_chars.max(max_header_width_chars);
        let width = (longest_chars as f32 * CHAR_WIDTH + 2.0 * BLOCK_PADDING_X)
            .max(MIN_BLOCK_WIDTH);
        let height = HEADER_HEIGHT + LINE_HEIGHT * (body.len() as f32) + BLOCK_PADDING_Y;

        blocks.push(LaidOutBlock {
            fn_index: fn_idx,
            block_id: block.id,
            x: 0.0,
            y: 0.0,
            width,
            height,
            header,
            body,
            body_runs,
            is_entry: block.predecessors.is_empty(),
        });
    }

    if blocks.is_empty() {
        return LaidOutFunction {
            name: builder.name.clone(),
            blocks,
            edges: Vec::new(),
            width: 0.0,
            height: 0.0,
        };
    }

    // ── Step 2: assign layers (longest path) ─────────────────────────
    // For a DAG this is a topological longest-path. The CFG can have
    // cycles (loops), so we cap iterations and treat any remaining
    // back-edge as a layer increment from its source — close enough for
    // a readable picture. The cap of `n + 1` iterations means we give up
    // on truly cyclic SCCs and lay them out in declaration order.
    let mut layer: Vec<usize> = vec![0; n];
    let mut predecessor_idx: Vec<Vec<usize>> = vec![Vec::new(); n];
    // For each node: list of (predecessor_id, succ_index_in_pred).
    // Used as a tie-breaker when two siblings have the same
    // barycenter — a `Br(cond, then_bb, else_bb)` puts the
    // `then_bb` successor at index 0 (we want it on the left) and
    // `else_bb` at index 1 (right). Without this, siblings under a
    // single common predecessor collapse to declaration order and
    // the diamond looks like a stair.
    let mut predecessor_succ_index: Vec<Vec<(usize, usize)>> = vec![Vec::new(); n];
    for (i, b) in builder.blocks.iter().enumerate() {
        for &p in &b.predecessors {
            if p < n {
                predecessor_idx[i].push(p);
                // The succ_index of block `i` in predecessor `p` is
                // derived from `p`'s terminator, NOT from the
                // successor list. The BrilBuilder stores successors
                // in a HashSet, so the list order is non-deterministic
                // and would flip the bias between runs. The
                // terminator is the source of truth: `Br(cond, then,
                // else)` puts the `then` arm at index 0 (left) and
                // the `else` arm at index 1 (right). `Jmp(target)`
                // has a single successor at index 0. `Ret` has no
                // successors.
                let s_idx = match builder.blocks[p].instrs.last() {
                    Some(IrInstruction::Br(_, then_bb, else_bb)) => {
                        if *then_bb == i {
                            0
                        } else if *else_bb == i {
                            1
                        } else {
                            // The terminator doesn't mention us —
                            // shouldn't happen if the CFG is
                            // consistent, but be defensive.
                            0
                        }
                    }
                    Some(IrInstruction::Jmp(_)) => 0,
                    _ => 0,
                };
                predecessor_succ_index[i].push((p, s_idx));
            }
        }
    }
    // converges in `n` iterations; for cyclic graphs it falls back to
    // declaration order.
    for _ in 0..=n {
        let mut changed = false;
        for i in 0..n {
            // Use the *minimum* predecessor layer, not the maximum.
            // A back-edge (cycle) has its target in a later layer
            // than the source — if we used max, the back-edge
            // target would be pushed down to the source's layer
            // and the loop header would be drawn far from the
            // entry. With min, a back-edge target stays at the
            // earliest reachable layer (right after the entry,
            // for a typical loop), and the back-edge is drawn
            // as the curve it is. This matches Graphviz's `dot`
            // default: loop headers sit just below their entry,
            // and the exit (if any) is on the same row as the
            // loop body.
            let best_pred = predecessor_idx[i]
                .iter()
                .map(|&p| layer[p])
                .min()
                .unwrap_or(0);
            let proposed = if predecessor_idx[i].is_empty() {
                0
            } else {
                best_pred + 1
            };
            if proposed != layer[i] {
                layer[i] = proposed;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let n_layers = layer.iter().copied().max().unwrap_or(0) + 1;
    let mut layer_nodes: Vec<Vec<usize>> = vec![Vec::new(); n_layers];
    for (i, &l) in layer.iter().enumerate() {
        layer_nodes[l].push(i);
    }



    // ── Step 5: position blocks (top-to-bottom) ──────────────────────
    // Y is the flow direction: layer index maps to row, and a layer's
    // y is the sum of previous layers' heights plus gaps. X uses a
    // barycenter-from-predecessors scheme: each block's *center* x is
    // the average of its predecessors' center x's (tie-broken by
    // successor index so the `then` arm sits left of `else`). After
    // sorting a layer by desired center, we place blocks left-to-
    // right and push any block that overlaps the previous one further
    // right. The result is a true diamond when blocks fan out: both
    // arms sit symmetrically about the parent's centerline.
    let mut layer_height: Vec<f32> = vec![0.0; n_layers];
    let mut y_offsets: Vec<f32> = vec![0.0; n_layers];
    for (l, ln) in layer_nodes.iter().enumerate() {
        for &node in ln {
            layer_height[l] = layer_height[l].max(blocks[node].height);
        }
    }
    for l in 1..n_layers {
        y_offsets[l] = y_offsets[l - 1] + layer_height[l - 1] + ROW_GAP;
    }

    // Per-node center x. Layer 0 starts each block at the function's
    // left margin (center = block.width/2). After placement, every
    // block's center is `block.x + block.width / 2`.
    let mut center_x: Vec<f32> = vec![0.0; n];
    let mut left_margin: Vec<f32> = vec![0.0; n_layers];

    for (l, ln) in layer_nodes.iter().enumerate() {
        if l == 0 {
            // First layer: lay blocks out left-to-right starting at
            // the function's left edge. Most CFGs have a single
            // entry, but multi-entry functions are possible (e.g.
            // un-dominable blocks from the front-end) and we just
            // stack them with a gap.
            let mut cursor = 0.0_f32;
            for &node in ln {
                blocks[node].x = cursor;
                center_x[node] = cursor + blocks[node].width * 0.5;
                cursor += blocks[node].width + COLUMN_GAP;
            }
            left_margin[l] = 0.0;
        } else {
            // For each block, compute its desired center. A
            // block's center is the average of its predecessors'
            // centers, *plus* a symmetric offset for the
            // position it holds among the siblings of the same
            // parent in this layer. The offset is computed as
            // `(k - (N-1)/2) * BIAS` where k is the successor
            // index from the parent's terminator and N is the
            // number of siblings in this layer.
            //
            // The symmetric bias is *only* applied when the
            // parent is in layer 0 (i.e. an entry block). For
            // deeper layers we use the standard barycenter
            // (centered on the parent), because cascading
            // symmetric offsets push the descendants of the
            // left arm further and further left, making the
            // diamond look like a fan rather than a tree. The
            // first-generation diamond gives the right visual
            // (then-arm on the left, else-arm on the right);
            // deeper layers then settle naturally around their
            // parent's centerline.
            const SUCC_INDEX_BIAS: f32 = 220.0;
            // For each parent, count how many of its successors
            // land in this layer.
            let mut parent_succ_count: HashMap<usize, usize> = HashMap::new();
            for &node in ln {
                for &(p, _) in &predecessor_succ_index[node] {
                    *parent_succ_count.entry(p).or_insert(0) += 1;
                }
            }
            // Set of blocks whose parent is in layer 0 — these
            // are the ones that get the symmetric bias.
            let mut bias_eligible: std::collections::HashSet<usize> = std::collections::HashSet::new();
            for &node in ln {
                for &(p, _) in &predecessor_succ_index[node] {
                    if layer[p] == 0 {
                        bias_eligible.insert(node);
                    }
                }
            }
            let mut bary: Vec<(f32, f32, usize)> = ln
                .iter()
                .map(|&node| {
                    let preds = &predecessor_succ_index[node];
                    if preds.is_empty() {
                        (f32::NAN, usize::MAX as f32, node)
                    } else if bias_eligible.contains(&node) && preds.len() == 1 {
                        // First-generation child of an entry:
                        // symmetric about the parent.
                        let (p, succ_idx) = preds[0];
                        let n_siblings = *parent_succ_count.get(&p).unwrap_or(&1);
                        let center_offset = (succ_idx as f32)
                            - (n_siblings as f32 - 1.0) / 2.0;
                        let desired = center_x[p] + center_offset * SUCC_INDEX_BIAS;
                        (desired, succ_idx as f32, node)
                    } else {
                        // Deeper layer or multi-parent: average
                        // of predecessor centers, no offset.
                        let avg = preds
                            .iter()
                            .map(|&(p, _)| center_x[p])
                            .sum::<f32>()
                            / preds.len() as f32;
                        (avg, 0.0, node)
                    }
                })
                .collect();
            bary.sort_by(|a, b| {
                let an = a.0.is_nan();
                let bn = b.0.is_nan();
                match (an, bn) {
                    (true, true) => a.2.cmp(&b.2),
                    (true, false) => std::cmp::Ordering::Greater,
                    (false, true) => std::cmp::Ordering::Less,
                    (false, false) => a
                        .0
                        .partial_cmp(&b.0)
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then_with(|| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
                        .then_with(|| a.2.cmp(&b.2)),
                }
            });

            // Place left-to-right with overlap resolution.
            // The sibling gap is tight for blocks that share a
            // parent in this layer; the inter-column gap is
            // wider for blocks in different parent chains.
            const SIBLING_GAP: f32 = 24.0;
            let mut cursor_right = f32::NEG_INFINITY;
            let mut prev_parent: Option<usize> = None;
            for &(bary_val, _, node) in &bary {
                let desired_center = if bary_val.is_nan() {
                    cursor_right + blocks[node].width * 0.5
                } else {
                    bary_val
                };
                let block_w = blocks[node].width;
                let curr_parent = if predecessor_succ_index[node].len() == 1 {
                    Some(predecessor_succ_index[node][0].0)
                } else {
                    None
                };
                let is_sibling_of_prev = curr_parent.is_some()
                    && curr_parent == prev_parent;
                let gap = if is_sibling_of_prev {
                    SIBLING_GAP
                } else {
                    COLUMN_GAP
                };
                let left = (desired_center - block_w * 0.5).max(cursor_right + gap);
                cursor_right = left + block_w;
                blocks[node].x = left;
                center_x[node] = left + block_w * 0.5;
                prev_parent = curr_parent;
            }
            left_margin[l] = 0.0;
        }
        // All blocks in this layer share the same Y, computed from
        // cumulative layer heights (precomputed in `y_offsets`).
        for &node in ln {
            blocks[node].y = y_offset + y_offsets[l];
        }
    }

    // ── Step 6: build edges, marking back-edges (cycles) ────────────
    let mut edges = Vec::new();
    for block in &builder.blocks {
        for (succ_idx, &succ_id) in block.successors.iter().enumerate() {
            if succ_id >= n {
                continue;
            }
            // A back-edge is either a self-loop on the source
            // block, or a successor in an *earlier* layer
            // (the strict-less-than handles the common loop
            // case where the body is one layer below the
            // header). Same-layer successors are not
            // back-edges: they are forward edges within a
            // layer (e.g. a `jmp` between two blocks that
            // happened to land in the same row under the
            // min-layer scheme).
            let is_back = succ_id == block.id || layer[succ_id] < layer[block.id];
            // Best-effort label: the last instruction in the block is
            // usually the terminator. We attach a label to the first
            // successor of a `Br` and "fallthrough" to the rest.
            let label = match block.instrs.last() {
                Some(IrInstruction::Br(cond, then_bb, else_bb)) => {
                    if succ_idx == 0 && *then_bb == succ_id {
                        format!("{:?}", cond)
                    } else if *else_bb == succ_id {
                        "else".to_string()
                    } else {
                        String::new()
                    }
                }
                Some(IrInstruction::Jmp(_)) if succ_idx == 0 => "jmp".to_string(),
                _ => String::new(),
            };
            edges.push(LaidOutEdge {
                fn_index: fn_idx,
                from_block: block.id,
                to_block: succ_id,
                is_back_edge: is_back,
                label,
            });
        }
    }

    // Compute the function's bounding box from the placed blocks.
    // Each block is at `(x, y)` with size `(width, height)`; the
    // function extends from the leftmost/topmost edge to the
    // rightmost/bottom-most. Empty function: zero size.
    //
    // First, shift the function so the leftmost block sits at
    // x=0. The symmetric placement allows blocks to have
    // negative x (e.g. the then-arm of a diamond sits to the
    // left of the parent). Without this shift, the function
    // would have a left "bump" with empty space before the
    // first block.
    let min_x = blocks
        .iter()
        .map(|b| b.x)
        .fold(f32::INFINITY, f32::min);
    if min_x.is_finite() && min_x < 0.0 {
        for b in &mut blocks {
            b.x -= min_x;
        }
    }
    let function_width = blocks
        .iter()
        .map(|b| b.x + b.width)
        .fold(0.0_f32, f32::max);
    let function_height = if blocks.is_empty() {
        0.0
    } else {
        y_offsets[n_layers - 1] + layer_height[n_layers - 1]
    };
    LaidOutFunction {
        name: builder.name.clone(),
        blocks,
        edges,
        width: function_width,
        height: function_height,
    }
}

/// Tokenize a single line of SSA `Debug` text into typed runs.
///
/// The SSA Debug format is regular: identifiers (`%v0_0`, `add`),
/// keywords (`const`, `mov`, `phi`, ...), block labels (`bb_3`),
/// integer and boolean literals, and punctuation (`=`, `?`, `:`,
/// `,`, `(`, `)`, `[`, `]`, `.`). The tokenizer walks the string
/// once, splitting on word/punctuation boundaries, and classifies
/// each piece. The painter maps the kind to a theme color.
///
/// We don't need to handle every possible edge case — the input
/// comes from our own `Debug` impl, not from the user — but the
/// token rules are loose enough to survive minor format changes
/// (e.g. an extra space, a new operator name) without crashing.
pub fn tokenize(line: &str) -> Vec<Token> {
    let mut out: Vec<Token> = Vec::new();
    let mut buf = String::new();
    for c in line.chars() {
        match c {
            // Word characters and digits accumulate as one identifier.
            'a'..='z' | 'A'..='Z' | '0'..='9' | '_' => buf.push(c),
            // `-` is part of a negative integer literal.
            '-' if buf.chars().all(|b| b.is_ascii_digit()) && !buf.is_empty() => {
                buf.push(c);
            }
            '%' => {
                // `%` always begins a variable. Flush, then start a new
                // run; the rest of the token will be the digits/underscores.
                flush(&mut buf, &mut out);
                buf.push('%');
            }
            // Punctuation: flush the current word, then push a punct
            // token for this single character.
            '=' | '?' | ':' | ',' | '(' | ')' | '[' | ']' | '.' => {
                flush(&mut buf, &mut out);
                let mut single = String::new();
                single.push(c);
                out.push(Token { text: single, kind: TokenKind::Punct });
            }
            // Whitespace: flush the word, emit a single-space Other
            // token for the gap. The painter collapses adjacent
            // Other runs.
            _ => {
                flush(&mut buf, &mut out);
                if c == ' ' {
                    out.push(Token { text: " ".to_string(), kind: TokenKind::Other });
                }
            }
        }
    }
    flush(&mut buf, &mut out);
    out
}

/// Move `buf` into a classified `Token` if non-empty. Standalone
/// function so the closure form doesn't fight the borrow checker.
fn flush(buf: &mut String, out: &mut Vec<Token>) {
    if buf.is_empty() {
        return;
    }
    let text = std::mem::take(buf);
    let kind = classify(&text);
    out.push(Token { text, kind });
}

/// Classify a single non-punctuation, non-whitespace token. Called
/// only by `tokenize` with the text of one accumulated run.
fn classify(text: &str) -> TokenKind {
    match text {
        "true" | "false" => TokenKind::Boolean,
        "undef" => TokenKind::Undef,
        "const" | "mov" | "add" | "sub" | "mul" | "div" | "eq" | "lt" | "gt"
        | "le" | "ge" | "and" | "or" | "not" | "print" | "jmp" | "br" | "ret"
        | "call" | "phi" | "from" => TokenKind::Opcode,
        // SSA variables start with `%`; block labels start with `bb_`.
        t if t.starts_with('%') => TokenKind::Variable,
        t if t.starts_with("bb_") => TokenKind::Block,
        // Pure digit run (with optional leading `-`): integer literal.
        t if t.chars().all(|c| c.is_ascii_digit() || c == '-') && !t.is_empty() => {
            TokenKind::Integer
        }
        _ => TokenKind::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssa::ir::{Builder, IrInstruction, SsaValue, SsaVariable};

    fn var(id: usize) -> SsaVariable {
        SsaVariable { id, index: 0 }
    }

    /// Build a 3-block diamond: bb_0 -> {bb_1, bb_2} -> bb_3.
    fn diamond() -> Builder {
        let mut b = Builder::new();
        b.name = "diamond".to_string();
        b.add_block(0, vec![], vec![1, 2]);
        b.blocks[0]
            .instrs
            .push(IrInstruction::Const(SsaValue::Var(var(0)), SsaValue::Int(1)));
        b.blocks[0]
            .instrs
            .push(IrInstruction::Br(SsaValue::Var(var(0)), 1, 2));
        b.add_block(1, vec![0], vec![3]);
        b.add_block(2, vec![0], vec![3]);
        b.add_block(3, vec![1, 2], vec![]);
        b.blocks[3]
            .instrs
            .push(IrInstruction::Ret(SsaValue::Var(var(0))));
        b
    }

    #[test]
    fn empty_program_is_empty_layout() {
        let layout = layout(&[]);
        assert!(layout.functions.is_empty());
        assert_eq!(layout.total_width, 0.0);
        assert_eq!(layout.total_height, 0.0);
    }

    #[test]
    fn single_block_has_one_node_no_edges() {
        let mut b = Builder::new();
        b.name = "trivial".to_string();
        b.add_block(0, vec![], vec![]);
        let layout = layout(&[b]);
        assert_eq!(layout.functions.len(), 1);
        assert_eq!(layout.functions[0].blocks.len(), 1);
        assert_eq!(layout.functions[0].edges.len(), 0);
        assert!(layout.functions[0].blocks[0].is_entry);
    }

    #[test]
    fn diamond_assigns_three_layers() {
        let b = diamond();
        let layout = layout(&[b]);
        let f = &layout.functions[0];
        assert_eq!(f.blocks.len(), 4);
        assert_eq!(f.edges.len(), 4);

        // bb_0 layer 0, bb_1/bb_2 layer 1, bb_3 layer 2.
        // Top-to-bottom: layer index maps to Y. Sibling-in-layer maps
        // to X (bb_1 and bb_2 share a row).
        let y_of = |id: usize| {
            f.blocks
                .iter()
                .find(|b| b.block_id == id)
                .map(|b| b.y)
                .unwrap()
        };
        let x_of = |id: usize| {
            f.blocks
                .iter()
                .find(|b| b.block_id == id)
                .map(|b| b.x)
                .unwrap()
        };
        let y0 = y_of(0);
        let y1 = y_of(1);
        let y2 = y_of(2);
        let y3 = y_of(3);
        assert!(y0 < y1);
        assert!(y0 < y2);
        assert!(y1 < y3);
        assert!(y2 < y3);
        // Siblings in the same layer sit on the same row.
        assert_eq!(y_of(1), y_of(2));
        // ... and are horizontally separated.
        assert_ne!(x_of(1), x_of(2));
    }

    #[test]
    fn diamond_then_left_of_else() {
        // The terminator is `Br(cond, then_bb=1, else_bb=2)`. The
        // tie-breaker in the barycenter sort should put the `then`
        // arm (bb_1) on the left and the `else` arm (bb_2) on the
        // right. Without it they collapse to declaration order and
        // the diamond looks like a stair.
        let b = diamond();
        let layout = layout(&[b]);
        let f = &layout.functions[0];
        let x_then = f.blocks.iter().find(|b| b.block_id == 1).unwrap().x;
        let x_else = f.blocks.iter().find(|b| b.block_id == 2).unwrap().x;
        assert!(x_then < x_else, "expected then (bb_1) left of else (bb_2); got {x_then} >= {x_else}");
    }

    #[test]
    fn three_way_branch_keeps_source_order() {
        // bb_0 -> {bb_1, bb_2} and bb_1 -> {bb_3} then
        // bb_0/1/2 all converge to bb_4. The successor-index
        // tie-breaker should put bb_1 leftmost, bb_2 rightmost.
        let mut b = Builder::new();
        b.name = "three".to_string();
        b.add_block(0, vec![], vec![1, 2]);
        b.blocks[0]
            .instrs
            .push(IrInstruction::Br(SsaValue::Var(var(0)), 1, 2));
        b.add_block(1, vec![0], vec![3, 4]);
        b.add_block(2, vec![0], vec![4]);
        b.add_block(3, vec![1], vec![4]);
        b.add_block(4, vec![1, 2, 3], vec![]);
        let layout = layout(&[b]);
        let f = &layout.functions[0];
        let xs: Vec<_> = [1, 2, 3]
            .iter()
            .map(|id| f.blocks.iter().find(|b| b.block_id == *id).unwrap().x)
            .collect();
        assert!(xs[0] < xs[1], "bb_1 should be left of bb_2");
    }

    #[test]
    fn diamond_marks_no_back_edges() {
        let b = diamond();
        let layout = layout(&[b]);
        assert!(layout.functions[0].edges.iter().all(|e| !e.is_back_edge));
    }

    #[test]
    fn loop_marks_back_edge() {
        // bb_0 -> bb_1, bb_1 -> bb_1, bb_1 -> bb_2
        let mut b = Builder::new();
        b.name = "loop".to_string();
        b.add_block(0, vec![], vec![1]);
        b.add_block(1, vec![0, 1], vec![1, 2]);
        b.add_block(2, vec![1], vec![]);
        let layout = layout(&[b]);
        let edges = &layout.functions[0].edges;
        // Exactly one back-edge: 1 -> 1 (self-loop on the loop body).
        let back: Vec<_> = edges.iter().filter(|e| e.is_back_edge).collect();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].from_block, 1);
        assert_eq!(back[0].to_block, 1);
    }

    #[test]
    fn block_width_grows_with_longest_line() {
        let mut b = Builder::new();
        b.name = "wide".to_string();
        b.add_block(0, vec![], vec![]);
        let long = "x".repeat(80);
        b.blocks[0]
            .instrs
            .push(IrInstruction::Print(SsaValue::Int(0)));
        // Force a long line by stuffing a comment-like value into a Const.
        b.blocks[0]
            .instrs
            .push(IrInstruction::Const(SsaValue::Var(var(7)), SsaValue::Int(0)));
        b.blocks[0]
            .instrs
            .push(IrInstruction::Call {
                callee_bb: 0,
                args: (0..10).map(|i| SsaValue::Var(var(i))).collect(),
                dest: Some(SsaValue::Var(var(10))),
            });
        // The Call with 10 args should produce the widest line.
        let _ = long; // suppress unused warning
        let layout = layout(&[b]);
        let block = &layout.functions[0].blocks[0];
        // Width ≈ 11 (var refs) * 7.2 + padding; well above the minimum.
        assert!(block.width > MIN_BLOCK_WIDTH);
    }

    #[test]
    fn multiple_functions_stack_vertically() {
        let mut a = Builder::new();
        a.name = "a".to_string();
        a.add_block(0, vec![], vec![]);
        let mut b = Builder::new();
        b.name = "b".to_string();
        b.add_block(0, vec![], vec![]);
        let layout = layout(&[a, b]);
        assert_eq!(layout.functions.len(), 2);
        assert!(layout.total_height > layout.functions[0].height);
        assert!(layout.functions[1].blocks[0].y > layout.functions[0].blocks[0].y);
    }

    #[test]
    fn tokenizer_classifies_const_line() {
        // `%v0_0 = const 1` should split into a variable, an `=`, the
        // opcode `const`, and the integer `1`, separated by spaces.
        let toks = tokenize("%v0_0 = const 1");
        let kinds: Vec<TokenKind> = toks.iter().map(|t| t.kind).collect();
        assert_eq!(
            kinds,
            vec![
                TokenKind::Variable,
                TokenKind::Other,
                TokenKind::Punct,
                TokenKind::Other,
                TokenKind::Opcode,
                TokenKind::Other,
                TokenKind::Integer,
            ]
        );
        // The variable text is the *whole* `%v0_0` (including the `%`).
        assert_eq!(toks[0].text, "%v0_0");
    }

    #[test]
    fn tokenizer_classifies_branch_line() {
        // `br %v11_0 ? bb_1 : bb_2` exercises opcode, variable, the
        // `?` / `:` punctuation, and two block labels.
        let toks = tokenize("br %v11_0 ? bb_1 : bb_2");
        assert!(toks.iter().any(|t| t.text == "br" && t.kind == TokenKind::Opcode));
        assert!(toks
            .iter()
            .any(|t| t.text == "%v11_0" && t.kind == TokenKind::Variable));
        assert!(toks.iter().any(|t| t.text == "?" && t.kind == TokenKind::Punct));
        assert!(toks.iter().any(|t| t.text == ":" && t.kind == TokenKind::Punct));
        assert!(toks
            .iter()
            .any(|t| t.text == "bb_1" && t.kind == TokenKind::Block));
        assert!(toks
            .iter()
            .any(|t| t.text == "bb_2" && t.kind == TokenKind::Block));
    }
    #[test]
    fn block_width_is_per_block_not_per_function() {
        // bb_0 has a long `call` line, bb_1 has a short `const`
        // line. Before the per-block fix, both blocks inherited
        // the function-wide max width (the call), so bb_1
        // looked like a wide card with a small line centered
        // in lots of empty space. Now each block's width is
        // driven by its own longest line.
        let mut b = Builder::new();
        b.name = "wide_and_narrow".to_string();
        b.add_block(0, vec![], vec![1]);
        // 12 args — a long call line.
        b.blocks[0].instrs.push(IrInstruction::Call {
            callee_bb: 99,
            args: (0..12).map(|i| SsaValue::Var(var(i))).collect(),
            dest: Some(SsaValue::Var(var(12))),
        });
        b.add_block(1, vec![0], vec![]);
        b.blocks[1]
            .instrs
            .push(IrInstruction::Const(SsaValue::Var(var(13)), SsaValue::Int(5)));
        let layout = layout(&[b]);
        let f = &layout.functions[0];
        let wide_w = f.blocks[0].width;
        let narrow_w = f.blocks[1].width;
        // The narrow block must be visibly narrower than the
        // wide one — at least 60px of difference (a long call
        // adds dozens of chars * 7.2 px/char ≈ 200+ px).
        assert!(
            narrow_w + 60.0 < wide_w,
            "expected narrow block ({narrow_w}) to be meaningfully narrower than wide block ({wide_w})"
        );
    }

    #[test]
    fn tokenizer_classifies_phi_operands() {
        // `%v0_0 = phi [ %v1_0 from bb_0, %v2_0 from bb_2 ]`
        let toks = tokenize("%v0_0 = phi [ %v1_0 from bb_0, %v2_0 from bb_2 ]");
        assert!(toks.iter().any(|t| t.text == "phi" && t.kind == TokenKind::Opcode));
        assert!(toks.iter().any(|t| t.text == "from" && t.kind == TokenKind::Opcode));
        assert!(toks
            .iter()
            .any(|t| t.text == "%v1_0" && t.kind == TokenKind::Variable));
        assert!(toks
            .iter()
            .any(|t| t.text == "%v2_0" && t.kind == TokenKind::Variable));
        assert!(toks
            .iter()
            .any(|t| t.text == "bb_0" && t.kind == TokenKind::Block));
    }

    #[test]
    fn tokenizer_classifies_call_and_dots() {
        // The call dest is followed by ` = call .bb_0`; the dot
        // starts the call target label. We classify the whole
        // `.bb_0` as `Other` today (the leading `.` is punctuation,
        // then `bb_0` is a block — but the dot interrupts the run,
        // so the two come out as separate tokens). Either way, no
        // token should be empty, and we should see one Block token
        // for `bb_0`.
        let toks = tokenize("%v0_0 = call .bb_0");
        assert!(toks
            .iter()
            .any(|t| t.text == "bb_0" && t.kind == TokenKind::Block));
        for t in &toks {
            assert!(!t.text.is_empty(), "empty token: {:?}", t);
        }
    }
}
