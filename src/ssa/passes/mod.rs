use anyhow::Result;
use crate::ssa::ir::Builder;
pub mod constant_folding;
pub mod sccp;
pub mod dse;


pub trait Pass {
    
    fn name() -> &'static str;
    fn run(builder: &mut Builder) -> Result<()>;
    
}

pub fn optimizer(builder: &mut Builder) -> Result<()> {
    sccp::SparseConditionalConstantPropagation::run(builder);
    dse::DeadStoreElimination::run(builder);
    Ok(())
}