use crate::brilir::builder::Builder;
use crate::ssa::cytron::{compute_df, insert_phi, run_rename};
use anyhow::Result;

pub mod cytron;

pub fn build_ssa(builder: &mut Builder) -> Result<()> {
    let df = compute_df(builder)?;
    println!("df: {:?}", df);

    insert_phi(builder, df)?;
    run_rename(builder, 0)?;

    for block in builder.blocks.iter() {
        println!("{:?}", block);
    }

    Ok(())
}
