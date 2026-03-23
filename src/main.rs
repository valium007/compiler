use anyhow::Result;
use std::fs;

pub mod bril_frontend;
pub mod brilir;
pub mod ssa;

use crate::brilir::compile_bril;

fn main() -> Result<()> {
    let bril_builder = compile_bril()?;
    let ssa_builder = ssa::build_ssa(&bril_builder);
    println!("{:?}", ssa_builder);

    Ok(())
}
