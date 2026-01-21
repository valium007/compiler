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

    pub succs: HashMap<BasicBlockId, BTreeSet<BasicBlockId>>,
    pub preds: HashMap<BasicBlockId, BTreeSet<BasicBlockId>>,
    pub label_to_id: HashMap<String, BasicBlockId>,
}

impl BrilBuilder {
    pub fn new() -> Self {
        Self {
            blocks: Vec::new(),
            instructions: Vec::new(),
            succs: HashMap::new(),
            preds: HashMap::new(),
            label_to_id: HashMap::new(),
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
        self.succs.entry(from).or_default().insert(to);
        self.preds.entry(to).or_default().insert(from);
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
    builder.preds.insert(0, BTreeSet::new());
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

    if builder.blocks.last().unwrap().succs.len() == 0 {
        builder
            .succs
            .insert(builder.blocks.len() - 1, BTreeSet::new());
    }
}
