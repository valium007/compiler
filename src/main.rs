use anyhow::Result;
use std::fs;

pub mod bril_frontend;
pub mod brilir;
pub mod ssa;
pub mod codegen;

use crate::brilir::compile_bril;
use crate::ssa::build_ssa;
use crate::codegen::llvm_isel::emit_llvm_ir;

fn main() -> Result<()> {
    let mut builder = compile_bril()?;
    build_ssa(&mut builder)?;
    let ir = emit_llvm_ir(&builder);
    fs::write("out.ll", ir)?;
    Ok(())
}
