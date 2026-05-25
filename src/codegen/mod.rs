pub mod x86;
pub mod aarch64;

use crate::regalloc::regalloc_ir::LoweredInst;
use crate::regalloc::Target;
pub use aarch64::FunctionOutput;

/// Generate assembly text from lowered (post-regalloc) blocks for the
/// given target architecture.
pub fn generate(blocks: &[Vec<LoweredInst>], target: Target, num_spillslots: usize) -> String {
    match target {
        Target::X86_64  => x86::emit_function(blocks, num_spillslots),
        Target::Aarch64 => aarch64::emit_function(blocks, num_spillslots),
    }
}

/// Generate assembly for a multi-function program.
/// Each entry is `(fn_name, global_entry_bb, blocks, num_spillslots)`.
pub fn generate_program(
    fns: &[(String, usize, Vec<Vec<LoweredInst>>, usize)],
    target: Target,
) -> String {
    let descs: Vec<FunctionOutput<'_>> = fns.iter().map(|(name, entry, blocks, slots)| {
        FunctionOutput { name: name.as_str(), entry_bb: *entry, blocks, num_spillslots: *slots }
    }).collect();
    match target {
        Target::Aarch64 => aarch64::emit_program(&descs),
        Target::X86_64  => x86::emit_program(&descs),
    }
}
