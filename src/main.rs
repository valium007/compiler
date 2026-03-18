use anyhow::Result;
pub mod bril_frontend;
pub mod brilir;
pub mod ssa;

use crate::brilir::compile_bril;
use crate::ssa::{build_ssa};

fn main() -> Result<()> {
    let mut builder = compile_bril()?;
    build_ssa(&mut builder)?;
    Ok(())
}
