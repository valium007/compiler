use anyhow::Result;
use std::fs;

pub mod bril_frontend;
pub mod brilir;
pub mod codegen;
pub mod ssa;

use crate::brilir::compile_bril;
use crate::codegen::llvm_isel::emit_llvm_ir;
use crate::ssa::build_ssa;

fn main() -> Result<()> {
    let mut builder = compile_bril()?;
    build_ssa(&mut builder)?;
    let ir = emit_llvm_ir(&builder);
    fs::write("out.ll", ir)?;
    Ok(())
}
