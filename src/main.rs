use anyhow::Result;
use std::collections::HashMap;
use std::fs;

pub mod bregalloc;
pub mod bril_frontend;
pub mod brilir;
pub mod codegen;
pub mod regalloc;
pub mod ssa;
pub mod xregalloc;

use crate::brilir::compile_bril;
use crate::regalloc::Target;

fn main() -> Result<()> {
    // ── Parse target from CLI args using clap ────────────────────────
    let matches = clap::Command::new("compiler")
        .version("0.1.0")
        .about("Bril compiler targeting AArch64 (default) and x86-64")
        .arg(
            clap::Arg::new("experimental-x86")
                .long("experimental-x86")
                .action(clap::ArgAction::SetTrue)
                .help("Force targeting experimental x86_64 backend instead of AArch64"),
        )
        .arg(
            clap::Arg::new("braun")
                .long("braun")
                .action(clap::ArgAction::SetTrue)
                .help("Use the Braun-style SSA register allocator instead of xregalloc"),
        )
        .arg(
            clap::Arg::new("input")
                .help("Path to the input BRIL JSON file (reads from stdin if omitted)")
                .required(false)
                .index(1),
        )
        .get_matches();

    let target = if matches.get_flag("experimental-x86") {
        Target::X86_64
    } else {
        Target::Aarch64
    };

    let use_braun = matches.get_flag("braun");

    let input_path = matches.get_one::<String>("input");
    let json_content = match input_path {
        Some(path) => fs::read_to_string(path)?,
        None => {
            use std::io::{self, Read};
            let mut buffer = String::new();
            io::stdin().read_to_string(&mut buffer)?;
            buffer
        }
    };

    // ── Frontend: Bril → non-SSA IR (one Builder per function) ─────────
    let bril_builders = compile_bril(&json_content)?;
    println!("=== Compiled {} function(s) ===", bril_builders.len());

    // ── SSA construction (Braun's algorithm) — per function ──────────────
    let ssa_builders: Vec<ssa::ir::Builder> = bril_builders
        .iter()
        .map(|bril_fn| {
            println!("function {}", bril_fn.name);
            let mut s = ssa::build_ssa(bril_fn);
            ssa::critical_edge::split_critical_edges(&mut s);
            // Phi-lowering is deferred to after register allocation so the
            // allocator can coalesce phi operands with their destination,
            // turning what would be Mov instructions into self-copies.
            ssa::prune_unreachable(&mut s);
            s
        })
        .collect();

    println!("=== SSA IR ===");
    for sb in &ssa_builders {
        println!("@{}:", sb.name);
        println!("{:?}", sb);
    }

    // For the rest of the pipeline, operate on @main (or the first function).
    let main_idx = ssa_builders
        .iter()
        .position(|b| b.name == "main")
        .unwrap_or(0);
    let ssa_builder = &ssa_builders[main_idx];
    println!("=== After Critical Edge Splitting ===");
    println!("{:?}", ssa_builder);

    // ── Global block-ID map (entry block ID for each function) ──────────
    // Used for multi-function codegen so `.Lbb_N` labels are globally unique.
    let fn_entry_map: HashMap<String, usize> = {
        let mut map = HashMap::new();
        let mut offset = 0usize;
        for sb in &ssa_builders {
            map.insert(sb.name.clone(), offset);
            offset += sb.blocks.len();
        }
        map
    };

    //


    // ── Rogers pipeline (Ian Rogers 2020, phi-based, no regalloc2) ───
    let mut fn_lowered_info = Vec::new();
    for sb in &ssa_builders {
        let (num_spillslots, lowered) = if use_braun {
            crate::regalloc::badapter::run_regalloc_b(sb, target)
        } else {
            crate::regalloc::run_regalloc(sb, target)
        };
        let entry_id = fn_entry_map[&sb.name];
        fn_lowered_info.push((sb.name.clone(), entry_id, lowered, num_spillslots));
    }

    let assembly = crate::codegen::generate_program(&fn_lowered_info, target);
    println!("{}", assembly);
    fs::write("out.s", &assembly)?;

    Ok(())
}
