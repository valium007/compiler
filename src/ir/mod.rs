use std::collections::BTreeSet;

use hashbrown::HashMap;

use crate::ir::{
    id::BasicBlockId,
    ssa::{BasicBlock, BinOp, SSA, Variable},
};
pub mod bril_to_ssa;
pub mod brilir;
pub mod dominator;
pub mod id;
pub mod irfmt;
pub mod passes;
pub mod ssa;

