use crate::ir::{brilir::*, dominator::Dominator, hir::to_hir, to_ssa};
use crate::ir::passes::liveness::*;

pub fn parse_instructions() -> Result<(), serde_json::Error> {
    let data = r#"
    [
    {
          "label": "entry"
        },
     {
          "dest": "a",
          "op": "const",
          "type": "int",
          "value": 47
        },
        {
          "dest": "b",
          "op": "const",
          "type": "int",
          "value": 42
        },
        {
          "dest": "cond",
          "op": "const",
          "type": "bool",
          "value": true
        },
        {
          "args": [
            "cond"
          ],
          "labels": [
            "left",
            "right"
          ],
          "op": "br"
        },
        {
          "label": "left"
        },
        {
          "dest": "b",
          "op": "const",
          "type": "int",
          "value": 1
        },
        {
          "dest": "c",
          "op": "const",
          "type": "int",
          "value": 5
        },
        {
          "labels": [
            "end"
          ],
          "op": "jmp"
        },
        {
          "label": "right"
        },
        {
          "dest": "a",
          "op": "const",
          "type": "int",
          "value": 2
        },
        {
          "dest": "c",
          "op": "const",
          "type": "int",
          "value": 10
        },
        {
          "labels": [
            "end"
          ],
          "op": "jmp"
        },
        {
          "label": "end"
        },
        {
          "args": [
            "a",
            "c"
          ],
          "dest": "d",
          "op": "sub",
          "type": "int"
        }
      ]
    "#;

    let parsed: Vec<Inst> = serde_json::from_str(&data).unwrap();
    let mut builder = Builder::new(); //brilir
    builder.instructions = parsed;
    create_basic_blocks(&mut builder);
    fill_preds(&mut builder);

    let hir = to_hir(&builder); //brilir to hir
    for block in hir.blocks.iter() {
        println!("bb_{} succs: {:?}", block.id, block.succs);
        for inst in block.body.iter() {
            println!("  {:?}", inst);
        }
    }

    let mut dom = Dominator::new(); //dominator compute
    let df = dom.compute(
        &(0..builder.blocks.len()).collect(),
        0,
        &builder.succs,
        &builder.preds,
    );

    let mut ssa_builder = to_ssa(&hir.blocks, &builder.label_to_id); //ssa form

    for block in ssa_builder.blocks.iter_mut() {
      block.succs = builder.succs.get(&block.id).unwrap().clone();
      block.preds = builder.preds.get(&block.id).unwrap().clone();
    }

    ssa_builder.insert_phi(df);
    ssa_builder.run_rename(&builder.succs, &builder.preds);
    for block in ssa_builder.blocks.iter_mut() {
      block.compute_def_use();
    }
    for block in ssa_builder.blocks.iter() {
        println!("bb_{}", block.id);
        for inst in block.instructions.iter() {
            println!("  {:?}", inst);
        }
    }

    let mut liveness = Liveness2::new();
    let vars = ssa_builder.get_vars();
    liveness.compute_livesets(&ssa_builder.blocks,vars);

    println!("{:?} {:?}",liveness.live_in, liveness.live_out);

    Ok(())
}
