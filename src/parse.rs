use crate::ir::{bril_to_ssa::{to_ssa, vars_to_ids}, brilir::*, dominance::Dominance, id::BasicBlockId, passes::liveness::Liveness};

pub fn parse_instructions() -> Result<(), serde_json::Error> {
    let data = r#"
[
        {
          "label": "entry"
        },
        {
          "dest": "x",
          "op": "const",
          "type": "int",
          "value": 0
        },
        {
          "dest": "i",
          "op": "const",
          "type": "int",
          "value": 0
        },
        {
          "dest": "one",
          "op": "const",
          "type": "int",
          "value": 1
        },
        {
          "labels": [
            "loop"
          ],
          "op": "jmp"
        },
        {
          "label": "loop"
        },
        {
          "dest": "max",
          "op": "const",
          "type": "int",
          "value": 10
        },
        {
          "args": [
            "i",
            "max"
          ],
          "dest": "cond",
          "op": "lt",
          "type": "bool"
        },
        {
          "args": [
            "cond"
          ],
          "labels": [
            "body",
            "exit"
          ],
          "op": "br"
        },
        {
          "label": "body"
        },
        {
          "dest": "mid",
          "op": "const",
          "type": "int",
          "value": 5
        },
        {
          "args": [
            "i",
            "mid"
          ],
          "dest": "cond",
          "op": "lt",
          "type": "bool"
        },
        {
          "args": [
            "cond"
          ],
          "labels": [
            "then",
            "endif"
          ],
          "op": "br"
        },
        {
          "label": "then"
        },
        {
          "args": [
            "x",
            "one"
          ],
          "dest": "x",
          "op": "add",
          "type": "int"
        },
        {
          "labels": [
            "endif"
          ],
          "op": "jmp"
        },
        {
          "label": "endif"
        },
        {
          "dest": "factor",
          "op": "const",
          "type": "int",
          "value": 2
        },
        {
          "args": [
            "x",
            "factor"
          ],
          "dest": "x",
          "op": "mul",
          "type": "int"
        },
        {
          "args": [
            "i",
            "one"
          ],
          "dest": "i",
          "op": "add",
          "type": "int"
        },
        {
          "labels": [
            "loop"
          ],
          "op": "jmp"
        },
        {
          "label": "exit"
        },
        {
          "args": [
            "x"
          ],
          "op": "print"
        }
      ]
    "#;

    let parsed: Vec<Inst> = serde_json::from_str(&data).unwrap();
    let mut builder = BrilBuilder::new();

    builder.instructions = parsed;
    create_basic_blocks(&mut builder);
    fill_preds(&mut builder);

    let cfg: Vec<Vec<BasicBlockId>> = builder.blocks.iter().map(|b| b.succs.clone()).collect();

    println!("cfg: {:?}",cfg);

    let liveness_info = compute_liveness_iterative(&builder);
    let mut dom = Dominance::new();
    
    dom.compute_rpo(&cfg);
    let df = dom.compute_df(&cfg, &builder.blocks);
    println!("df: {:?}",df);

    let mut ssa_builder = to_ssa(&builder);

    ssa_builder.blocks.retain_mut(|b| {
      b.succs = builder.blocks[b.id].succs.clone();
      b.preds = builder.blocks[b.id].preds.clone();
      true
    });

    let var_to_id = vars_to_ids(&builder.blocks);

    ssa_builder.insert_phi(&df, &liveness_info, &var_to_id);
    ssa_builder.run_rename();
    

    for block in ssa_builder.blocks.iter_mut() {
      block.compute_def_use();
      println!("bb_{:?} succs: {:?} preds: {:?}",block.id,block.succs,block.preds);
      for inst in block.instructions.iter() {
        println!("  {:?}",inst);
      }
    }

    let liveness = Liveness::compute_liveness(&ssa_builder.blocks);
    println!("livein:{:?} \nliveout:{:?}", liveness.0, liveness.1);


    Ok(())
}
