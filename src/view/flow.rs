//! SSA graph viewer: `Layout` -> `ferrum_flow::Graph` -> window.
//!
//! Hand-rolled GPUI app code used to live in `app.rs`. That file is
//! gone now; this is the entire UI surface. ferrum-flow gives us
//! pan, zoom, select, clipboard, undo/redo, minimap, context menu,
//! fit-all, etc. for free via `default_plugins()`, so we only have
//! to convert our `Layout` into a `Graph`, register a custom node
//! renderer for our `"ssa-block"` kind, and open a window.

use ferrum_flow::{FlowCanvas, Graph, Node, NodeRenderer, PortId, RenderContext};
use gpui::{
    AppContext as _, Application, Element as _, ParentElement as _, Styled, WindowOptions, div, px,
    rgb,
};
use serde_json::json;

use super::graph::Layout;

/// Open a window and display the SSA CFG. Blocks until the window
/// is closed.
pub fn run(builders: &[crate::ssa::ir::Builder]) {
    let layout = super::graph::layout(builders);
    let graph = build_graph(&layout);
    Application::new().run(move |cx| {
        cx.open_window(WindowOptions::default(), |window, cx| {
            cx.new(|ctx| {
                FlowCanvas::builder(graph, ctx, window)
                    .default_plugins()
                    .node_renderer("ssa-block", SsaBlockRenderer)
                    .build()
            })
        })
        .expect("failed to open SSA graph window");
    });
}

/// Convert our `Layout` into a ferrum-flow `Graph`.
///
/// One `Node` per `LaidOutBlock` (kind `"ssa-block"`), one `Edge`
/// per `LaidOutEdge`. Each node carries its header, body lines,
/// and `is_entry` flag in the `data` JSON, which the
/// `SsaBlockRenderer` reads back when drawing the card.
///
/// Positions and sizes are passed through directly: our layout
/// pass already produces sensible x/y/w/h in ferrum-flow's
/// pixel-from-top-left coordinate system.
///
/// Each block gets one input and one output port so edges can
/// connect. We use a two-pass build: the first pass records the
/// `(block_id, input_port, output_port)` triple for every block;
/// the second pass uses that map to wire edges.
fn build_graph(layout: &Layout) -> Graph {
    /// Vertical gap between functions, matching the value used by
    /// `graph::layout` for its `FUNCTION_GAP`. Keeping these in
    /// sync ensures the viewer places nodes where the layout
    /// said.
    const FUNCTION_GAP: f32 = 80.0;

    Graph::build(|g| {
        // Track per-block port handles so the edge pass can find
        // them by id. The map is keyed by (function_index, block_id)
        // because block ids are local to a function.
        let mut inputs: std::collections::HashMap<(usize, usize), PortId> =
            std::collections::HashMap::new();
        let mut outputs: std::collections::HashMap<(usize, usize), PortId> =
            std::collections::HashMap::new();

        let mut y_offset: f32 = 0.0;
        for (fi, func) in layout.functions.iter().enumerate() {
            for block in &func.blocks {
                let x = block.x;
                let y = block.y + y_offset;
                let w = block.width;
                let h = block.height;
                let data = json!({
                    "label": block.header,
                    "header": block.header,
                    "body": block.body,
                    "is_entry": block.is_entry,
                });
                let (_node, ins, outs) = g
                    .create_node("ssa-block")
                    .position(x, y)
                    .size(w, h)
                    .input()
                    .output()
                    .data(data)
                    .build_with_ports();
                // Each block has exactly one input and one output
                // port; record both for the edge pass.
                inputs.insert((fi, block.block_id), ins[0]);
                outputs.insert((fi, block.block_id), outs[0]);
            }
            y_offset += func.height + FUNCTION_GAP;
        }

        // Edge pass: for each `LaidOutEdge`, look up the source
        // block's output port and the target block's input port
        // and create an edge between them.
        for (fi, func) in layout.functions.iter().enumerate() {
            for edge in &func.edges {
                let source = outputs.get(&(fi, edge.from_block)).copied();
                let target = inputs.get(&(fi, edge.to_block)).copied();
                if let (Some(source), Some(target)) = (source, target) {
                    g.create_edge().source(source).target(target).build();
                }
            }
        }
    })
}

/// Custom node renderer for kind `"ssa-block"`.
///
/// Reads `data["header"]` (the block name, e.g. `entry bb_0`),
/// `data["body"]` (a list of SSA instruction text lines), and
/// `data["is_entry"]` (a bool that tints the card background).
/// Renders the standard ferrum-flow card shell with the body
/// text inside.
///
/// Without this, ferrum-flow's default behavior for an unknown
/// kind is to draw an "undefined node" placeholder with a
/// warning message instead of the actual block content. We
/// need a real renderer to see the SSA instructions.
pub struct SsaBlockRenderer;

impl NodeRenderer for SsaBlockRenderer {
    fn render(&self, node: &Node, ctx: &mut RenderContext) -> gpui::AnyElement {
        let data = node.data_ref();
        let header = data
            .get("header")
            .and_then(|v| v.as_str())
            .unwrap_or("?")
            .to_string();
        let is_entry = data
            .get("is_entry")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let body: Vec<String> = data
            .get("body")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        // Dark-theme colors, lifted from the prior hand-rolled
        // viewer. Entry blocks get a slightly bluer card so the
        // user can spot them at a glance.
        let (bg, border) = if is_entry {
            (rgb(0x2a3344), rgb(0x4a6da8))
        } else {
            (rgb(0x25252f), rgb(0x3a3a48))
        };
        let fg = rgb(0xe6e6f0);
        let dim = rgb(0x6d6d8a);

        let shell = ctx
            .node_card_shell_custom(node)
            .rounded(px(6.0))
            .bg(bg)
            .border_1()
            .border_color(border);

        // Stack header + body. We use `flex_col` so the header
        // sits on top of the body. Each body line is one text
        // child. We do not yet color tokens individually — the
        // full per-token highlighter can be wired up here later
        // using the same `tokenize` data we already serialize.
        let mut inner = div().flex().flex_col().size_full().p(px(6.0)).gap(px(2.0));
        inner = inner.child(
            div()
                .text_xs()
                .text_color(dim)
                .child(header),
        );
        for line in &body {
            inner = inner.child(
                div()
                    .text_xs()
                    .text_color(fg)
                    .child(line.clone()),
            );
        }

        shell.child(inner).into_any()
    }
}

