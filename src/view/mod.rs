//! Interactive SSA graph viewer.
//!
//! Entry point: [`run`]. Lays out the supplied `Builder`s with
//! [`graph::layout`], converts the result to a ferrum-flow `Graph`,
//! and opens a window with the full plugin stack (pan, zoom,
//! select, clipboard, minimap, context menu, undo/redo, etc.).

pub mod graph;

mod flow;

pub use flow::run;
