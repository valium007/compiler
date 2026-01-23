use std::{collections::BTreeSet, fmt::Debug, hash::Hash};

use hashbrown::HashMap;
use paste::paste;
use serde::Deserialize;

use crate::ir::id::BasicBlockId;

#[derive(Clone, Deserialize, Eq, Hash, PartialEq, PartialOrd, Ord)]
#[serde(untagged)]
pub enum BrilValue {
    Variable(String),
    Int(i64),
    Bool(bool),
}
impl Debug for BrilValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Variable(v) => write!(f, "{}", v),
            Self::Int(i) => write!(f, "{}", i),
            Self::Bool(b) => write!(f, "{}", b),
        }
    }
}

#[derive(Clone, Deserialize, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub struct Load {
    pub dest: String,
    pub value: BrilValue,
    #[serde(rename = "type")]
    pub typ: String,
}

impl Debug for Load {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {} = load {:?}", self.dest, self.typ, self.value)
    }
}

#[derive(Clone, Deserialize, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub struct Jmp {
    pub labels: Vec<String>,
}

impl Debug for Jmp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "jmp {}", self.labels[0])
    }
}

#[derive(Clone, Deserialize, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub struct Br {
    pub args: Vec<String>,
    pub labels: [String; 2],
}

impl Debug for Br {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "br {} {} {}",
            self.args[0], self.labels[0], self.labels[1]
        )
    }
}

#[derive(Clone, Deserialize, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub struct Print {
    pub args: Vec<String>,
}

impl Debug for Print {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "print {:?}", self.args[0])
    }
}

macro_rules! bin_op {
    ($name:ident) => {
        // Define a binaryop struct. This expands to:
        //
        //     pub struct bin_op {
        //         dest: String,
        //         args: [String; 2],
        //         typ: String,
        //     }
        #[derive(Clone, Deserialize, Eq, Hash, PartialEq, PartialOrd, Ord)]
        pub struct $name {
            pub dest: String,
            pub args: [String; 2],
            #[serde(rename = "type")]
            pub typ: String,
        }

        paste! {
            impl Debug for $name {
                    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(f,"{}: {} = {} {} {}",self.dest,self.typ,stringify!([<$name:lower>]),self.args[0],self.args[1])
                    }
            }
        }


    };
}

bin_op!(Add);
bin_op!(Sub);
bin_op!(Mul);
bin_op!(Lt);

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, PartialOrd, Ord)]
#[serde(tag = "op")]
pub enum Op {
    #[serde(rename = "const")]
    Load(Load),

    #[serde(rename = "add")]
    Add(Add),

    #[serde(rename = "sub")]
    Sub(Sub),

    #[serde(rename = "mul")]
    Mul(Mul),

    #[serde(rename = "lt")]
    Lt(Lt),

    #[serde(rename = "jmp")]
    Jmp(Jmp),

    #[serde(rename = "br")]
    Br(Br),

    #[serde(rename = "print")]
    Print(Print),
}

#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, PartialOrd, Ord)]
#[serde(untagged)]
pub enum Inst {
    Label { label: String },
    Op(Op),
}

#[derive(Clone, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub struct BasicBlock {
    pub label: String,
    pub body: Vec<Inst>,
    pub preds: Vec<BasicBlockId>,
    pub succs: Vec<BasicBlockId>,
}

pub struct BrilBuilder {
    pub blocks: Vec<BasicBlock>,
    pub instructions: Vec<Inst>,
    pub label_to_id: HashMap<String, BasicBlockId>,
    pub cfg: Vec<Vec<BasicBlockId>>
}

impl BrilBuilder {
    pub fn new() -> Self {
        Self {
            blocks: Vec::new(),
            instructions: Vec::new(),
            label_to_id: HashMap::new(),
            cfg: Vec::new()
        }
    }

    pub fn new_block(&mut self, label: &str) -> BasicBlockId {
        let current_id = self.blocks.len();

        self.blocks.push(BasicBlock {
            label: label.to_string(),
            body: Vec::new(),
            preds: Vec::new(),
            succs: Vec::new(),
        });

        self.label_to_id.insert(label.to_string(), current_id);
        current_id
    }

    pub fn add_inst(&mut self, inst: Inst, bb_id: BasicBlockId) {
        self.blocks[bb_id].body.push(inst);
    }

    pub fn add_edge(&mut self, from: BasicBlockId, to: BasicBlockId) {
        self.blocks[from].succs.push(to);
        self.blocks[to].preds.push(from);
    }
}

pub fn create_basic_blocks(builder: &mut BrilBuilder) {
    let mut current_id = 0;
    let instrs = std::mem::take(&mut builder.instructions);

    for instr in instrs {
        match instr {
            Inst::Label { label } => {
                current_id = builder.new_block(&label);
            }
            other => {
                builder.add_inst(other, current_id);
            }
        }
    }
}

pub fn fill_preds(builder: &mut BrilBuilder) {
    let num_blocks = builder.blocks.len();
    for block_id in 0..num_blocks {
        let last_inst = builder.blocks[block_id].body.last();

        match last_inst {
            Some(Inst::Op(Op::Jmp(jmp))) => {
                let target_id = *builder.label_to_id.get(&jmp.labels[0]).unwrap();
                builder.add_edge(block_id, target_id);
            }
            Some(Inst::Op(Op::Br(br))) => {
                let truthy_id = *builder.label_to_id.get(&br.labels[0]).unwrap();
                let falsy_id = *builder.label_to_id.get(&br.labels[1]).unwrap();
                builder.add_edge(block_id, truthy_id);
                builder.add_edge(block_id, falsy_id);
            }

            _ => {
                if block_id + 1 < num_blocks {
                    builder.add_edge(block_id, block_id + 1);
                }
            }
        }
    }
}

use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct LivenessInfo {
    pub live_in: HashMap<BasicBlockId, BTreeSet<String>>,
    pub live_out: HashMap<BasicBlockId, BTreeSet<String>>,
}

impl LivenessInfo {
    pub fn new() -> Self {
        Self {
            live_in: HashMap::new(),
            live_out: HashMap::new(),
        }
    }
}

/// Helper to extract variables defined and used by an instruction
/// Returns (definition, uses)
fn analyze_inst(inst: &Inst) -> (Option<String>, Vec<String>) {
    match inst {
        Inst::Op(op) => match op {
            Op::Load(load) => {
                let mut uses = Vec::new();
                if let BrilValue::Variable(v) = &load.value {
                    uses.push(v.clone());
                }
                (Some(load.dest.clone()), uses)
            }
            Op::Add(add) => (
                Some(add.dest.clone()),
                vec![add.args[0].clone(), add.args[1].clone()],
            ),
            Op::Sub(sub) => (
                Some(sub.dest.clone()),
                vec![sub.args[0].clone(), sub.args[1].clone()],
            ),
            Op::Mul(mul) => (
                Some(mul.dest.clone()),
                vec![mul.args[0].clone(), mul.args[1].clone()],
            ),
            Op::Lt(lt) => (
                Some(lt.dest.clone()),
                vec![lt.args[0].clone(), lt.args[1].clone()],
            ),
            Op::Br(br) => (None, vec![br.args[0].clone()]),
            Op::Print(print) => (None, print.args.clone()),
            Op::Jmp(_) => (None, Vec::new()),
        },
        Inst::Label { .. } => (None, Vec::new()),
    }
}

// Optional: Simple iterative dataflow for comparison
pub fn compute_liveness_iterative(builder: &BrilBuilder) -> LivenessInfo {
    let mut liveness = LivenessInfo::new();

    // Initialize
    for block_id in 0..builder.blocks.len() {
        liveness.live_in.insert(block_id, BTreeSet::new());
        liveness.live_out.insert(block_id, BTreeSet::new());
    }

    // Compute upward-exposed uses and defs for each block
    let mut ue_uses: HashMap<BasicBlockId, BTreeSet<String>> = HashMap::new();
    let mut def_vars: HashMap<BasicBlockId, BTreeSet<String>> = HashMap::new();

    for (block_id, block) in builder.blocks.iter().enumerate() {
        let mut local_defs = HashSet::new();
        let mut local_ue = BTreeSet::new();

        for inst in &block.body {
            let (def, uses) = analyze_inst(inst);

            // Upward-exposed uses
            for var in uses {
                if !local_defs.contains(&var) {
                    local_ue.insert(var);
                }
            }

            // Definitions
            if let Some(d) = def {
                local_defs.insert(d);
            }
        }

        ue_uses.insert(block_id, local_ue);
        def_vars.insert(block_id, local_defs.into_iter().collect());
    }

    // Iterate until fixed point
    let mut changed = true;
    while changed {
        changed = false;

        // Process blocks in reverse order (better for backward analysis)
        for block_id in (0..builder.blocks.len()).rev() {
            // LiveOut(B) = Union of LiveIn(S) for all successors S
            let mut new_live_out = BTreeSet::new();
            for &succ_id in &builder.blocks[block_id].succs {
                new_live_out.extend(liveness.live_in.get(&succ_id).unwrap().clone());
            }

            // LiveIn(B) = UEUses(B) ∪ (LiveOut(B) \ Defs(B))
            let mut new_live_in = ue_uses.get(&block_id).unwrap().clone();
            let defs = def_vars.get(&block_id).unwrap();
            for var in &new_live_out {
                if !defs.contains(var) {
                    new_live_in.insert(var.clone());
                }
            }

            // Check if changed
            if new_live_in != *liveness.live_in.get(&block_id).unwrap()
                || new_live_out != *liveness.live_out.get(&block_id).unwrap()
            {
                changed = true;
                liveness.live_in.insert(block_id, new_live_in);
                liveness.live_out.insert(block_id, new_live_out);
            }
        }
    }

    liveness
}
