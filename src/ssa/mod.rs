pub mod cytron;
use crate::brilir::builder::Builder;
use crate::ssa::cytron::{compute_df, insert_phi, run_rename};
use anyhow::Result;

pub fn build_ssa(builder: &mut Builder) -> Result<()> {
    let df = compute_df(builder)?;
    insert_phi(builder, df)?;
    run_rename(builder, 0)?;

    Ok(())
}
