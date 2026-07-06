use anyhow::Result;
use std::collections::HashMap;
use std::fs;

pub mod bril_frontend;
pub mod brilir;
pub mod codegen;
pub mod max_ssa;
pub mod regalloc;
pub mod ssa;
pub mod xregalloc;

use crate::brilir::compile_bril;
use crate::regalloc::Target;

fn main() -> Result<()> {
    // ── Parse arguments using clap ────────────────────────
    let matches = clap::Command::new("compiler")
        .version("0.1.0")
        .about("Bril compiler targeting x86-64")
        .arg(
            clap::Arg::new("max-ssa")
                .long("max-ssa")
                .action(clap::ArgAction::SetTrue)
                .help("Build maximal SSA form (incremental-lifting experiment), print it, then exit"),
        )
        .arg(
            clap::Arg::new("ssa-dump")
                .long("ssa-dump")
                .num_args(0..=1)
                .default_missing_value("")
                .help("Dump the SSA CFG in DOT format using petgraph to a file before critical edge splitting"),
        )
        .arg(
            clap::Arg::new("input")
                .help("Path to the input BRIL JSON file")
                .required(true)
                .index(1),
        )
        .arg(
            clap::Arg::new("output")
                .help("Path to the output assembly file (not required with --view)")
                .required(false)
                .index(2),
        )
        .get_matches();


    let target = Target::X86_64;



    let input_path = matches.get_one::<String>("input").unwrap();
    let json_content = fs::read_to_string(input_path)?;

    // ── Frontend: Bril → non-SSA IR (one Builder per function) ─────────
    let bril_builders = compile_bril(&json_content)?;
    println!("=== Compiled {} function(s) ===", bril_builders.len());

    // ── SSA construction — per function. `--max-ssa` selects maximal SSA
    // (incremental-lifting experiment); otherwise Braun's algorithm. Both
    // produce ssa::ir::Builder and feed the same downstream pipeline.
    let use_max_ssa = matches.get_flag("max-ssa");
    let mut ssa_builders: Vec<ssa::ir::Builder> = bril_builders
        .iter()
        .map(|bril_fn| {
            println!("function {}", bril_fn.name);
            if use_max_ssa {
                max_ssa::build_max_ssa(bril_fn)
            } else {
                ssa::build_ssa(bril_fn)
            }
        })
        .collect();

    if let Some(dump_ssa_val) = matches.get_one::<String>("ssa-dump") {
        let path = if dump_ssa_val.is_empty() {
            std::path::Path::new(input_path)
                .with_extension(if use_max_ssa { "max_ssa.dot" } else { "ssa.dot" })
                .to_string_lossy()
                .into_owned()
        } else {
            dump_ssa_val.clone()
        };
        let content = ssa::ir::dump_program_ssa_dot(&ssa_builders);
        fs::write(&path, content)?;
        println!("Dumped SSA CFG to {}", path);
    }

    for s in &mut ssa_builders {
        ssa::passes::optimizer(s)?;
        ssa::critical_edge::split_critical_edges(s);
        ssa::prune_unreachable(s);
    }

    println!("=== SSA IR ===");
    for sb in &ssa_builders {
        println!("@{}:", sb.name);
        println!("{:?}", sb);
    }



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

    // ── Rogers pipeline (Ian Rogers 2020, phi-based, no regalloc2) ───
    let mut fn_lowered_info = Vec::new();
    for sb in &ssa_builders {
        let (num_spillslots, lowered) =  crate::regalloc::run_regalloc(sb, target);
        let entry_id = fn_entry_map[&sb.name];
        fn_lowered_info.push((sb.name.clone(), entry_id, lowered, num_spillslots));
    }

    let assembly = crate::codegen::generate_program(&fn_lowered_info, target);
    let output_path = matches
        .get_one::<String>("output")
        .ok_or_else(|| anyhow::anyhow!("an output path is required"))?;
    fs::write(output_path, &assembly)?;
    println!("assembly written to {}", output_path);
    Ok(())
}
