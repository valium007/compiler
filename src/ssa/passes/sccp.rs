use crate::ssa::ir::{Builder, IrInstruction, SsaValue, SsaVariable, BinaryOp, BasicBlockId, Phi};
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lattice {
    Top,
    Int(i64),
    Bool(bool),
    Bottom,
}

impl Lattice {

    fn to_ssa_value(&self) -> Option<SsaValue> {
        match self {
            Lattice::Int(i) => Some(SsaValue::Int(*i)),
            Lattice::Bool(b) => Some(SsaValue::Bool(*b)),
            _ => None,
        }
    }
}

fn meet(v1: Lattice, v2: Lattice) -> Lattice {
    match (v1, v2) {
        (Lattice::Top, other) | (other, Lattice::Top) => other,
        (Lattice::Bottom, _) | (_, Lattice::Bottom) => Lattice::Bottom,
        (Lattice::Int(a), Lattice::Int(b)) if a == b => Lattice::Int(a),
        (Lattice::Bool(a), Lattice::Bool(b)) if a == b => Lattice::Bool(a),
        _ => Lattice::Bottom,
    }
}

pub struct SparseConditionalConstantPropagation {
    cfg_worklist: VecDeque<(BasicBlockId, BasicBlockId)>,
    ssa_worklist: VecDeque<SsaVariable>,
    executable_edges: HashSet<(BasicBlockId, BasicBlockId)>,
    executable_blocks: HashSet<BasicBlockId>,
    lat_vals: HashMap<SsaVariable, Lattice>,
    use_map: HashMap<SsaVariable, Vec<(BasicBlockId, usize)>>,
}

impl SparseConditionalConstantPropagation {
    pub fn run(builder: &mut Builder) {
        if builder.blocks.is_empty() {
            return;
        }

        let mut sccp = Self::new(builder);
        sccp.solve(builder);
        sccp.rewrite(builder);
    }

    fn new(builder: &Builder) -> Self {
        let mut lat_vals = HashMap::new();

        // Initialize all variables to Top
        for &v in builder.variables.values() {
            lat_vals.insert(v, Lattice::Top);
        }

        // Initialize parameters in the entry block to Bottom
        for &param in &builder.params {
            lat_vals.insert(param, Lattice::Bottom);
        }

        // Precompute the use map
        let mut use_map: HashMap<SsaVariable, Vec<(BasicBlockId, usize)>> = HashMap::new();
        for block in &builder.blocks {
            for (inst_idx, inst) in block.instrs.iter().enumerate() {
                for use_val in inst.uses() {
                    if let SsaValue::Var(v) = use_val {
                        use_map.entry(*v).or_default().push((block.id, inst_idx));
                    }
                }
            }
        }

        let mut cfg_worklist = VecDeque::new();
        // Push a synthetic "start → entry" edge so the CFG loop visits block 0
        // and evaluates its instructions on first-visit, exactly as Wegman-Zadeck
        // describes. usize::MAX is used as a sentinel for the virtual predecessor.
        cfg_worklist.push_back((usize::MAX, 0));

        Self {
            cfg_worklist,
            ssa_worklist: VecDeque::new(),
            executable_edges: HashSet::new(),
            executable_blocks: HashSet::new(), // block 0 inserted on first-visit below
            lat_vals,
            use_map,
        }
    }

    fn get_lattice(&self, val: &SsaValue) -> Lattice {
        match val {
            SsaValue::Var(v) => self.lat_vals.get(v).copied().unwrap_or(Lattice::Top),
            SsaValue::Int(i) => Lattice::Int(*i),
            SsaValue::Bool(b) => Lattice::Bool(*b),
            SsaValue::Undef => Lattice::Top,
        }
    }

    fn set_lattice(&mut self, var: SsaVariable, val: Lattice) {
        let old = self.lat_vals.entry(var).or_insert(Lattice::Top);
        if *old != val {
            *old = val;
            self.ssa_worklist.push_back(var);
        }
    }

    fn solve(&mut self, builder: &Builder) {
        while !self.cfg_worklist.is_empty() || !self.ssa_worklist.is_empty() {
            // Process CFG worklist
            while let Some(edge) = self.cfg_worklist.pop_front() {
                if !self.executable_edges.contains(&edge) {
                    self.executable_edges.insert(edge);
                    let to_bb = edge.1;
                    let first_visit = !self.executable_blocks.contains(&to_bb);
                    self.executable_blocks.insert(to_bb);

                    // Clone phi instructions out of the block before calling
                    // evaluate_phi, which mutably borrows self.
                    let phi_clones: Vec<Phi> = builder
                        .get_block(to_bb)
                        .instrs
                        .iter()
                        .filter_map(|i| {
                            if let IrInstruction::PhiAssign(p) = i { Some(p.clone()) } else { None }
                        })
                        .collect();
                    for phi in &phi_clones {
                        self.evaluate_phi(phi);
                    }

                    // If this is the first time we visit this block, evaluate all
                    // non-phi instructions. Clone each instruction so the block
                    // borrow doesn't overlap with the mutable self borrow in
                    // evaluate_instruction.
                    if first_visit {
                        let non_phi_instrs: Vec<(usize, IrInstruction)> = builder
                            .get_block(to_bb)
                            .instrs
                            .iter()
                            .enumerate()
                            .filter(|(_, i)| !i.is_phi())
                            .map(|(idx, i)| (idx, i.clone()))
                            .collect();
                        for (inst_idx, inst) in &non_phi_instrs {
                            self.evaluate_instruction(to_bb, *inst_idx, inst, builder);
                        }
                    }
                }
            }

            // Process SSA worklist.
            // Clone the use-list out first so the immutable borrow of self.use_map
            // doesn't conflict with the mutable borrow inside evaluate_instruction.
            while let Some(var) = self.ssa_worklist.pop_front() {
                let uses: Vec<(BasicBlockId, usize)> = self
                    .use_map
                    .get(&var)
                    .cloned()
                    .unwrap_or_default();

                for (bb_id, inst_idx) in uses {
                    if self.executable_blocks.contains(&bb_id) {
                        // Clone the instruction so the immutable borrow of builder
                        // doesn't survive into the mutable call below.
                        let inst = builder.get_block(bb_id).instrs[inst_idx].clone();
                        self.evaluate_instruction(bb_id, inst_idx, &inst, builder);
                    }
                }
            }
        }
    }

    fn evaluate_phi(&mut self, phi: &Phi) {
        let dst = phi.var.expect_var();
        let mut phi_val = Lattice::Top;

        for (op, pred) in &phi.operands {
            // Only consider operands from executable edges
            if self.executable_edges.contains(&(*pred, phi.block)) {
                let op_val = self.get_lattice(op);
                phi_val = meet(phi_val, op_val);
            }
        }

        self.set_lattice(dst, phi_val);
    }

    fn evaluate_instruction(&mut self, bb_id: BasicBlockId, _inst_idx: usize, inst: &IrInstruction, _builder: &Builder) {
        match inst {
            IrInstruction::Const(dst, val) => {
                let dst_var = dst.expect_var();
                let lat = self.get_lattice(val);
                self.set_lattice(dst_var, lat);
            }
            IrInstruction::Mov(dst, src) => {
                let dst_var = dst.expect_var();
                let lat = self.get_lattice(src);
                self.set_lattice(dst_var, lat);
            }
            IrInstruction::Not(dst, src) => {
                let dst_var = dst.expect_var();
                let src_lat = self.get_lattice(src);
                let lat = match src_lat {
                    Lattice::Top => Lattice::Top,
                    Lattice::Bottom => Lattice::Bottom,
                    Lattice::Bool(b) => Lattice::Bool(!b),
                    Lattice::Int(i) => Lattice::Int(!i),
                };
                self.set_lattice(dst_var, lat);
            }
            IrInstruction::Binary(op, dst, lhs, rhs) => {
                let dst_var = dst.expect_var();
                let lhs_lat = self.get_lattice(lhs);
                let rhs_lat = self.get_lattice(rhs);

                // Short-circuiting checks
                let lat = match op {
                    BinaryOp::And => {
                        if lhs_lat == Lattice::Bool(false) || rhs_lat == Lattice::Bool(false) {
                            Lattice::Bool(false)
                        } else if lhs_lat == Lattice::Int(0) || rhs_lat == Lattice::Int(0) {
                            Lattice::Int(0)
                        } else {
                            self.eval_binary_standard(*op, lhs_lat, rhs_lat)
                        }
                    }
                    BinaryOp::Or => {
                        if lhs_lat == Lattice::Bool(true) || rhs_lat == Lattice::Bool(true) {
                            Lattice::Bool(true)
                        } else {
                            self.eval_binary_standard(*op, lhs_lat, rhs_lat)
                        }
                    }
                    BinaryOp::Mul => {
                        if lhs_lat == Lattice::Int(0) || rhs_lat == Lattice::Int(0) {
                            Lattice::Int(0)
                        } else {
                            self.eval_binary_standard(*op, lhs_lat, rhs_lat)
                        }
                    }
                    _ => self.eval_binary_standard(*op, lhs_lat, rhs_lat),
                };

                self.set_lattice(dst_var, lat);
            }
            IrInstruction::Call { dest: Some(dst), .. } => {
                let dst_var = dst.expect_var();
                self.set_lattice(dst_var, Lattice::Bottom);
            }
            IrInstruction::Br(cond, then_bb, else_bb) => {
                let cond_lat = self.get_lattice(cond);
                match cond_lat {
                    Lattice::Bool(true) => {
                        self.cfg_worklist.push_back((bb_id, *then_bb));
                    }
                    Lattice::Bool(false) => {
                        self.cfg_worklist.push_back((bb_id, *else_bb));
                    }
                    Lattice::Bottom => {
                        self.cfg_worklist.push_back((bb_id, *then_bb));
                        self.cfg_worklist.push_back((bb_id, *else_bb));
                    }
                    Lattice::Top => {
                        // Do nothing until cond_lat is resolved
                    }
                    _ => {
                        // Fallback in case of int condition
                        self.cfg_worklist.push_back((bb_id, *then_bb));
                        self.cfg_worklist.push_back((bb_id, *else_bb));
                    }
                }
            }
            IrInstruction::Jmp(target) => {
                self.cfg_worklist.push_back((bb_id, *target));
            }
            IrInstruction::PhiAssign(phi) => {
                self.evaluate_phi(phi);
            }
            _ => {}
        }
    }

    fn eval_binary_standard(&self, op: BinaryOp, lhs: Lattice, rhs: Lattice) -> Lattice {
        if lhs == Lattice::Bottom || rhs == Lattice::Bottom {
            return Lattice::Bottom;
        }
        if lhs == Lattice::Top || rhs == Lattice::Top {
            return Lattice::Top;
        }

        match (lhs, rhs) {
            (Lattice::Int(a), Lattice::Int(b)) => match op {
                BinaryOp::Add => Lattice::Int(a.wrapping_add(b)),
                BinaryOp::Sub => Lattice::Int(a.wrapping_sub(b)),
                BinaryOp::Mul => Lattice::Int(a.wrapping_mul(b)),
                BinaryOp::Div => {
                    if b != 0 {
                        Lattice::Int(a / b)
                    } else {
                        Lattice::Bottom
                    }
                }
                BinaryOp::Eq => Lattice::Bool(a == b),
                BinaryOp::Lt => Lattice::Bool(a < b),
                BinaryOp::Gt => Lattice::Bool(a > b),
                BinaryOp::Le => Lattice::Bool(a <= b),
                BinaryOp::Ge => Lattice::Bool(a >= b),
                _ => Lattice::Bottom,
            },
            (Lattice::Bool(a), Lattice::Bool(b)) => match op {
                BinaryOp::Eq => Lattice::Bool(a == b),
                BinaryOp::And => Lattice::Bool(a && b),
                BinaryOp::Or => Lattice::Bool(a || b),
                _ => Lattice::Bottom,
            },
            _ => Lattice::Bottom,
        }
    }

    fn rewrite(&self, builder: &mut Builder) {
        let block_ids: Vec<usize> = builder.blocks.iter().map(|b| b.id).collect();

        for bb_id in block_ids {
            let mut instrs_to_replace = Vec::new();
            let mut edges_to_remove = Vec::new();

            {
                let block = builder.get_block(bb_id);
                for (inst_idx, inst) in block.instrs.iter().enumerate() {
                    let mut updated_inst = inst.clone();

                    // 1. Replace uses of constants
                    updated_inst.for_each_use_mut(|use_val| {
                        if let SsaValue::Var(v) = use_val {
                            if let Some(lat) = self.lat_vals.get(v) {
                                if let Some(const_val) = lat.to_ssa_value() {
                                    *use_val = const_val;
                                }
                            }
                        }
                    });

                    // 2. Perform folding based on lattice analysis
                    match &updated_inst {
                        IrInstruction::Const(SsaValue::Var(dst), _) => {
                            if let Some(lat) = self.lat_vals.get(dst) {
                                if let Some(const_val) = lat.to_ssa_value() {
                                    instrs_to_replace.push((inst_idx, IrInstruction::Const(SsaValue::Var(*dst), const_val)));
                                }
                            }
                        }
                        IrInstruction::Mov(SsaValue::Var(dst), src) => {
                            if !src.is_var() {
                                instrs_to_replace.push((inst_idx, IrInstruction::Const(SsaValue::Var(*dst), src.clone())));
                            } else if let Some(lat) = self.lat_vals.get(dst) {
                                if let Some(const_val) = lat.to_ssa_value() {
                                    instrs_to_replace.push((inst_idx, IrInstruction::Const(SsaValue::Var(*dst), const_val)));
                                }
                            }
                        }
                        IrInstruction::Binary(_op, SsaValue::Var(dst), _, _) => {
                            if let Some(lat) = self.lat_vals.get(dst) {
                                if let Some(const_val) = lat.to_ssa_value() {
                                    instrs_to_replace.push((inst_idx, IrInstruction::Const(SsaValue::Var(*dst), const_val)));
                                }
                            }
                        }
                        IrInstruction::Not(SsaValue::Var(dst), _) => {
                            if let Some(lat) = self.lat_vals.get(dst) {
                                if let Some(const_val) = lat.to_ssa_value() {
                                    instrs_to_replace.push((inst_idx, IrInstruction::Const(SsaValue::Var(*dst), const_val)));
                                }
                            }
                        }
                        IrInstruction::PhiAssign(phi) => {
                            if let SsaValue::Var(dst) = phi.var {
                                if let Some(lat) = self.lat_vals.get(&dst) {
                                    if let Some(const_val) = lat.to_ssa_value() {
                                        instrs_to_replace.push((inst_idx, IrInstruction::Const(SsaValue::Var(dst), const_val)));
                                    }
                                }
                            }
                        }
                        IrInstruction::Br(cond, then_bb, else_bb) => {
                            if let SsaValue::Bool(b) = cond {
                                let (taken, untaken) = if *b { (*then_bb, *else_bb) } else { (*else_bb, *then_bb) };
                                instrs_to_replace.push((inst_idx, IrInstruction::Jmp(taken)));
                                edges_to_remove.push(untaken);
                            }
                        }
                        _ => {}
                    }
                }
            }

            // Apply updates
            if !instrs_to_replace.is_empty() {
                let block = builder.get_block_mut(bb_id);
                for (idx, new_inst) in instrs_to_replace {
                    block.instrs[idx] = new_inst;
                }
            }

            // Handle pruned edges
            for untaken_bb in edges_to_remove {
                // 1. Remove from successors
                builder.get_block_mut(bb_id).successors.retain(|&s| s != untaken_bb);

                // 2. Remove from predecessors of untaken block
                builder.get_block_mut(untaken_bb).predecessors.retain(|&p| p != bb_id);

                // 3. Remove phi operands in untaken block coming from bb_id
                let untaken_block = builder.get_block_mut(untaken_bb);
                for inst in &mut untaken_block.instrs {
                    if let IrInstruction::PhiAssign(phi) = inst {
                        phi.operands.retain(|(_, from)| *from != bb_id);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_meet() {
        let t = Lattice::Top;
        let b = Lattice::Bottom;
        let c1 = Lattice::Int(1);
        let c2 = Lattice::Int(2);

        assert_eq!(meet(t, t), t);
        assert_eq!(meet(t, b), b);
        assert_eq!(meet(t, c1), c1);
        assert_eq!(meet(t, c2), c2);
        assert_eq!(meet(c1, b), b);
        assert_eq!(meet(c2, c1), b);
        assert_eq!(meet(c1, c1), c1);
    }

    #[test]
    fn test_sccp_basic() {
        let mut builder = Builder::new();
        builder.name = "main".to_string();

        // bb_0: entry
        // v0 = const 2
        // v1 = const 3
        // v2 = add v0, v1
        builder.add_block(0, vec![], vec![]);

        let v0 = SsaVariable { id: 0, index: 0 };
        let v1 = SsaVariable { id: 1, index: 0 };
        let v2 = SsaVariable { id: 2, index: 0 };

        builder.variables.insert(0, v0);
        builder.variables.insert(1, v1);
        builder.variables.insert(2, v2);

        builder.blocks[0].instrs.push(IrInstruction::Const(SsaValue::Var(v0), SsaValue::Int(2)));
        builder.blocks[0].instrs.push(IrInstruction::Const(SsaValue::Var(v1), SsaValue::Int(3)));
        builder.blocks[0].instrs.push(IrInstruction::Binary(BinaryOp::Add, SsaValue::Var(v2), SsaValue::Var(v0), SsaValue::Var(v1)));

        SparseConditionalConstantPropagation::run(&mut builder);

        // v2 should be folded to const 5
        let inst = &builder.blocks[0].instrs[2];
        match inst {
            IrInstruction::Const(dst, val) => {
                assert_eq!(dst.expect_var(), v2);
                assert_eq!(val, &SsaValue::Int(5));
            }
            _ => panic!("Expected const instruction for v2, got {:?}", inst),
        }
    }
}
