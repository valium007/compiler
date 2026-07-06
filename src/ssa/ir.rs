use std::fmt::Debug;

use std::collections::{HashMap, HashSet};

use petgraph::Graph;
use petgraph::algo::tarjan_scc;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::dot::{Dot, Config};

pub type InstId = usize;
pub type ValueId = usize;
pub type VariableId = usize;
pub type BasicBlockId = usize;

#[derive(Copy, Clone, Default, Eq, PartialEq, Hash)]
pub struct SsaVariable {
    pub id: VariableId,
    pub index: ValueId,
}

impl Debug for SsaVariable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "%v{}_{}", self.id, self.index)
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Hash)]
pub enum SsaValue {
    Var(SsaVariable),
    Int(i64),
    Bool(bool),
    Undef,
}

impl Debug for SsaValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SsaValue::Var(v) => write!(f, "{:?}", v),
            SsaValue::Int(i) => write!(f, "{}", i),
            SsaValue::Bool(b) => write!(f, "{}", b),
            SsaValue::Undef => write!(f, "undef"),
        }
    }
}

impl SsaValue {
    /// Extract the underlying `SsaVariable`. Panics if `self` is not a Var —
    /// use this at def sites, where the type allows `SsaValue` but the
    /// runtime invariant is that defs hold `Var`.
    pub fn expect_var(&self) -> SsaVariable {
        match self {
            SsaValue::Var(v) => *v,
            other => panic!("expected SsaValue::Var, got {:?}", other),
        }
    }

    /// Non-panicking variant.
    pub fn as_var(&self) -> Option<SsaVariable> {
        match self {
            SsaValue::Var(v) => Some(*v),
            _ => None,
        }
    }

    pub fn is_var(&self) -> bool { matches!(self, SsaValue::Var(_)) }
}

#[derive(Copy, Clone, Eq, PartialEq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Eq,
    Lt,
    Gt,
    Le,
    Ge,
    And,
    Or,
}

#[derive(Clone, Eq, PartialEq)]
pub struct Phi {
    pub block: BasicBlockId,
    /// Phi destination. Statically `SsaValue` for type uniformity with
    /// `IrInstruction`'s operand positions; runtime invariant: always Var.
    pub var: SsaValue,
    /// Phi operands. Statically `SsaValue`; runtime invariant: always Var.
    pub operands: Vec<(SsaValue, BasicBlockId)>,
}

impl Debug for Phi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?} = phi ", self.var)?;
        if self.operands.is_empty() {
            write!(f, "[ ]")
        } else {
            write!(f, "[ ")?;
            for (i, (var, bb)) in self.operands.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "{:?} from bb_{}", var, bb)?;
            }
            write!(f, " ]")
        }
    }
}

/// SSA instructions. Every operand position is typed `SsaValue` for
/// uniform pass-authoring. Defs (first `SsaValue` in `Const`/`Mov`/`Binary`/
/// `Not`, the dest of `Call`, and `Phi::var`) are statically `SsaValue` but
/// must hold `SsaValue::Var(_)` at runtime — use `SsaValue::expect_var()`
/// at the consumer to recover the underlying `SsaVariable`.
#[derive(Clone, Eq, PartialEq)]
pub enum IrInstruction {
    Const(SsaValue, SsaValue),
    Mov(SsaValue, SsaValue),
    Binary(BinaryOp, SsaValue, SsaValue, SsaValue),
    PhiAssign(Phi),
    Not(SsaValue, SsaValue),
    Print(SsaValue),
    Jmp(BasicBlockId),
    Br(SsaValue, BasicBlockId, BasicBlockId),
    Ret(SsaValue),
    Call {
        callee_bb: usize,
        args: Vec<SsaValue>,
        dest: Option<SsaValue>,
    },
    Nop,
}

impl Debug for IrInstruction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IrInstruction::Const(dst, val) => write!(f, "{:?} = const {:?}", dst, val),
            IrInstruction::Mov(dst, src) => write!(f, "{:?} = mov {:?}", dst, src),
            IrInstruction::Binary(op, dst, lhs, rhs) => {
                write!(f, "{:?} = {:?} {:?} {:?}", dst, lhs, op, rhs)
            }
            IrInstruction::PhiAssign(phi) => write!(f, "{:?}", phi),
            IrInstruction::Not(dst, src) => write!(f, "{:?} = not {:?}", dst, src),
            IrInstruction::Print(val) => write!(f, "print {:?}", val),
            IrInstruction::Jmp(bb) => write!(f, "jmp bb_{}", bb),
            IrInstruction::Br(cond, then_bb, else_bb) => {
                write!(f, "br {:?} ? bb_{}:bb_{}", cond, then_bb, else_bb)
            }
            IrInstruction::Ret(val) => write!(f, "ret {:?}", val),
            IrInstruction::Call { callee_bb, args, dest } => {
                if let Some(d) = dest {
                    write!(f, "{:?} = call .bb_{}", d, callee_bb)?;
                } else {
                    write!(f, "call .bb_{}", callee_bb)?;
                }
                for arg in args {
                    write!(f, " {:?}", arg)?;
                }
                Ok(())
            }
            IrInstruction::Nop => Ok(()),
        }
    }
}

impl IrInstruction {
    pub fn phi_mut(&mut self) -> &mut Phi {
        match self {
            IrInstruction::PhiAssign(phi) => phi,
            _ => panic!("Expected phi"),
        }
    }

    pub fn phi(&self) -> &Phi {
        match self {
            IrInstruction::PhiAssign(phi) => phi,
            _ => panic!("Expected phi"),
        }
    }

    pub fn is_phi(&self) -> bool {
        matches!(self, IrInstruction::PhiAssign(_))
    }

    /// Operands that this instruction defines (writes). Returns 0 or 1
    /// element except for instructions that don't define anything.
    pub fn defs(&self) -> Vec<&SsaValue> {
        match self {
            IrInstruction::Const(d, _)
            | IrInstruction::Mov(d, _)
            | IrInstruction::Binary(_, d, _, _)
            | IrInstruction::Not(d, _) => vec![d],
            IrInstruction::PhiAssign(p) => vec![&p.var],
            IrInstruction::Call { dest: Some(d), .. } => vec![d],
            IrInstruction::Call { dest: None, .. }
            | IrInstruction::Print(_)
            | IrInstruction::Jmp(_)
            | IrInstruction::Br(_, _, _)
            | IrInstruction::Ret(_)
            | IrInstruction::Nop => vec![],
        }
    }

    /// Operands that this instruction uses (reads), in left-to-right order.
    /// For `PhiAssign`, returns each phi-operand value (without its predecessor
    /// label — use `inst.phi().operands` if you need both).
    pub fn uses(&self) -> Vec<&SsaValue> {
        match self {
            IrInstruction::Const(_, s)
            | IrInstruction::Mov(_, s)
            | IrInstruction::Not(_, s)
            | IrInstruction::Print(s)
            | IrInstruction::Br(s, _, _)
            | IrInstruction::Ret(s) => vec![s],
            IrInstruction::Binary(_, _, l, r) => vec![l, r],
            IrInstruction::PhiAssign(p) => p.operands.iter().map(|(o, _)| o).collect(),
            IrInstruction::Call { args, .. } => args.iter().collect(),
            IrInstruction::Jmp(_) | IrInstruction::Nop => vec![],
        }
    }

    pub fn for_each_def_mut(&mut self, mut f: impl FnMut(&mut SsaValue)) {
        match self {
            IrInstruction::Const(d, _)
            | IrInstruction::Mov(d, _)
            | IrInstruction::Binary(_, d, _, _)
            | IrInstruction::Not(d, _) => f(d),
            IrInstruction::PhiAssign(p) => f(&mut p.var),
            IrInstruction::Call { dest: Some(d), .. } => f(d),
            _ => {}
        }
    }

    pub fn for_each_use_mut(&mut self, mut f: impl FnMut(&mut SsaValue)) {
        match self {
            IrInstruction::Const(_, s)
            | IrInstruction::Mov(_, s)
            | IrInstruction::Not(_, s)
            | IrInstruction::Print(s)
            | IrInstruction::Br(s, _, _)
            | IrInstruction::Ret(s) => f(s),
            IrInstruction::Binary(_, _, l, r) => { f(l); f(r); }
            IrInstruction::PhiAssign(p) => {
                for (o, _) in p.operands.iter_mut() { f(o); }
            }
            IrInstruction::Call { args, .. } => {
                for a in args.iter_mut() { f(a); }
            }
            IrInstruction::Jmp(_) | IrInstruction::Nop => {}
        }
    }
}

impl Debug for BinaryOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BinaryOp::Add => write!(f, "add"),
            BinaryOp::Sub => write!(f, "sub"),
            BinaryOp::Mul => write!(f, "mul"),
            BinaryOp::Div => write!(f, "div"),
            BinaryOp::Eq => write!(f, "eq"),
            BinaryOp::Lt => write!(f, "lt"),
            BinaryOp::Gt => write!(f, "gt"),
            BinaryOp::Le => write!(f, "le"),
            BinaryOp::Ge => write!(f, "ge"),
            BinaryOp::And => write!(f, "and"),
            BinaryOp::Or => write!(f, "or"),
        }
    }
}

#[derive(Clone)]
pub struct BasicBlock {
    pub id: BasicBlockId,
    pub instrs: Vec<IrInstruction>,
    pub successors: Vec<BasicBlockId>,
    pub predecessors: Vec<BasicBlockId>,
    pub definitions: HashMap<VariableId, SsaVariable>,
    pub incomplete_phis: HashMap<VariableId, InstId>,
}
 
impl Debug for BasicBlock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "bb_{}:", self.id)?;
        write!(f, "  ")?;
        if !self.predecessors.is_empty() {
            write!(f, "preds: ")?;
            for (i, pred) in self.predecessors.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "bb_{}", pred)?;
            }
        } else {
            write!(f, "preds: none")?;
        }
        write!(f, " | ")?;
        if !self.successors.is_empty() {
            write!(f, "succs: ")?;
            for (i, succ) in self.successors.iter().enumerate() {
                if i > 0 {
                    write!(f, ", ")?;
                }
                write!(f, "bb_{}", succ)?;
            }
        } else {
            write!(f, "succs: none")?;
        }
        writeln!(f)?;
        for instr in self.instrs.iter() {
            writeln!(f, "  {:?}", instr)?;
        }
        Ok(())
    }
}
 
#[derive(Clone)]
pub struct Builder {
    pub blocks: Vec<BasicBlock>,
    pub variables: HashMap<VariableId, SsaVariable>,
    pub sealed: HashSet<BasicBlockId>,
    pub next_var_id: VariableId,
    pub name: String,
    pub params: Vec<SsaVariable>,
}

impl Builder {
    pub fn new() -> Self {
        Self {
            blocks: Vec::new(),
            variables: HashMap::new(),
            sealed: HashSet::new(),
            next_var_id: 0,
            name: String::new(),
            params: Vec::new(),
        }
    }
    
    pub fn write_variable(&mut self, var: VariableId, bb: BasicBlockId) -> SsaVariable {
        if let Some(v) = self.variables.get_mut(&var) {
            v.index += 1;
        } else {
            let v = SsaVariable { id: var, index: 0 };
            self.variables.insert(var, v);
        }
        let v = self.variables[&var];
        self.write_variable_internal(var, bb, v);
        v
    }

    fn write_variable_internal(&mut self, var: VariableId, bb: BasicBlockId, value: SsaVariable) {
        self.get_block_mut(bb).definitions.insert(var, value);
    }

    pub fn read_variable(&mut self, var: VariableId, bb: BasicBlockId) -> SsaVariable {
        if let Some(v) = self.blocks[bb].definitions.get(&var) {
            return *v;
        }
        return self.read_variable_recursive(var, bb);
    }

    pub fn read_variable_recursive(&mut self, var: VariableId, bb: BasicBlockId) -> SsaVariable {
        let variable;
        if !self.sealed.contains(&bb) {
            variable = self.get_fresh_var();
            let phi = self.inst_phi(variable, bb);
            *self
                .get_block_mut(bb)
                .incomplete_phis
                .entry(var)
                .or_default() = phi;
        } else if self.get_block(bb).predecessors.len() == 1 {
            // Single pred — no phi needed.  Memoize to match the Braun
            // paper (writeVariable is called in every branch).
            variable = self.read_variable(var, self.get_block(bb).predecessors[0]);
        } else {
            variable = self.get_fresh_var();
            let phi = self.inst_phi(variable, bb);
            self.write_variable_internal(var, bb, variable);
            // add_phi_operands returns None only when the phi was already
            // tombstoned by a prior call in the same chain — which cannot
            // happen here because `phi` was just created.  The .unwrap_or
            // is purely defensive.
            let resolved = self.add_phi_operands(var, bb, phi).unwrap_or(variable);
            self.write_variable_internal(var, bb, resolved);
            return resolved;
        }
        self.write_variable_internal(var, bb, variable);
        variable
    }

    pub fn add_phi_operands(
        &mut self,
        var: VariableId,
        bb: BasicBlockId,
        phi: InstId,
    ) -> Option<SsaVariable> {
        let preds = self.get_block(bb).predecessors.clone();
        for pred in preds.iter() {
            let pred_var = self.read_variable(var, *pred);
            self.get_block_mut(bb).instrs[phi]
                .phi_mut()
                .operands
                .push((SsaValue::Var(pred_var), *pred));
        }
        // All operands now present — try_remove_trivial_phi's partial-operand
        // guard will pass and the full check will run.
        self.try_remove_trivial_phi(phi, bb)
    }

    /// Try to remove a trivial phi (Algorithm 3 from Braun et al.).
    ///
    /// Returns `Some(var)` — the variable that should be used in place of this
    /// phi (may equal `phi_var` if the phi is non-trivial and was kept).
    /// Returns `None` if the instruction slot was already tombstoned by an
    /// earlier call in the same recursive chain; callers should treat this as
    /// "already handled" and ignore the result.
    pub fn try_remove_trivial_phi(&mut self, phi: InstId, bb: BasicBlockId) -> Option<SsaVariable> {
        // Already tombstoned by a prior call in the same chain.
        if !self.get_block(bb).instrs[phi].is_phi() {
            return None;
        }

        // Phi.var is statically SsaValue but always holds Var.
        let phi_var_val: SsaValue = self.get_block(bb).instrs[phi].phi().var.clone();
        let phi_var: SsaVariable = phi_var_val.expect_var();

        let n_preds = self.get_block(bb).predecessors.len();
        let n_ops   = self.get_block(bb).instrs[phi].phi().operands.len();
        if n_ops < n_preds {
            return Some(phi_var);
        }

        // `same` tracks the candidate single operand; phi operands are always
        // Var, so we track them as SsaVariable for downstream use.
        let mut same: Option<SsaVariable> = None;
        for (op, _) in self.get_block(bb).instrs[phi].phi().operands.clone().iter() {
            let op_var = op.expect_var();
            if Some(op_var) == same || op_var == phi_var {
                continue;
            }
            if same.is_some() {
                return Some(phi_var);
            }
            same = Some(op_var);
        }

        if same.is_none() {
            // Unreachable phi — all operands are self-references, or there
            // are no operands at all.  Per Algorithm 3 this becomes Undef.
            // Replace every use of phi_var with Undef, then tombstone the slot.
            self.replace_phi_uses(phi_var, SsaValue::Undef);
            self.get_block_mut(bb).instrs[phi] = IrInstruction::Nop;
            return Some(phi_var);
        }
        let resolved = same.unwrap();

        // Snapshot users of phi_var BEFORE replacement (Algorithm 3: collect
        // phi.users first, then replaceBy(same), then recurse on those users).
        // Collecting after replacement gives get_phi_users(resolved) which is
        // a superset — it includes phis that already held `resolved` before
        // this elimination, causing unnecessary extra recursive checks.
        let users_to_recheck = self.get_phi_users(phi_var);

        self.replace_phi_uses(phi_var, SsaValue::Var(resolved));
        self.get_block_mut(bb).instrs[phi] = IrInstruction::Nop;

        for block in self.blocks.iter_mut() {
            for (_, def) in block.definitions.iter_mut() {
                if *def == phi_var {
                    *def = resolved;
                }
            }
        }

        for (block, inst) in users_to_recheck {
            if self.get_block(block).instrs[inst].is_phi() {
                let _ = self.try_remove_trivial_phi(inst, block);
            }
        }

        Some(resolved)
    }

    fn get_phi_users(&self, var: SsaVariable) -> Vec<(BasicBlockId, InstId)> {
        let target = SsaValue::Var(var);
        let mut users = Vec::new();
        for block in self.blocks.iter() {
            for (i, inst) in block.instrs.iter().enumerate() {
                if let IrInstruction::PhiAssign(phi) = inst {
                    if phi.operands.iter().any(|(op, _)| *op == target) {
                        users.push((block.id, i));
                    }
                }
            }
        }
        users
    }

    pub fn replace_phi_uses(&mut self, old: SsaVariable, new: SsaValue) {
        let old_val = SsaValue::Var(old);
        let new_is_var = new.is_var();
        for block in self.blocks.iter_mut() {
            for inst in block.instrs.iter_mut() {
                // Phi operands carry an invariant: always Var. Skip substitution
                // when `new` is not a Var — the caller will tombstone the phi
                // immediately after (e.g. Undef path in try_remove_trivial_phi).
                if let IrInstruction::PhiAssign(phi) = inst {
                    if new_is_var {
                        for (op, _) in phi.operands.iter_mut() {
                            if *op == old_val {
                                *op = new.clone();
                            }
                        }
                    }
                    continue;
                }
                // For every other instruction, rewrite each use position
                // uniformly via the visitor helper.
                inst.for_each_use_mut(|u| {
                    if *u == old_val {
                        *u = new.clone();
                    }
                });
            }
        }
    }

    pub fn get_fresh_var(&mut self) -> SsaVariable {
        let var = self.next_var_id;
        let v = SsaVariable { id: var, index: 0 };
        self.variables.insert(var, v);
        self.next_var_id += 1;
        v
    }

    pub fn get_block(&self, id: BasicBlockId) -> &BasicBlock {
        &self.blocks[id]
    }

    pub fn get_block_mut(&mut self, id: BasicBlockId) -> &mut BasicBlock {
        &mut self.blocks[id]
    }
    pub fn inst_phi(&mut self, var: SsaVariable, bb: BasicBlockId) -> InstId {
        let i = self.blocks[bb].instrs.len();

        self.blocks[bb].instrs.push(IrInstruction::PhiAssign(Phi {
            block: bb,
            var: SsaValue::Var(var),
            operands: Vec::new(),
        }));

        i
    }

    pub fn add_block(
        &mut self,
        id: BasicBlockId,
        predecessors: Vec<BasicBlockId>,
        successors: Vec<BasicBlockId>,
    ) {
        let block = BasicBlock {
            id,
            instrs: Vec::new(),
            successors,
            predecessors,
            definitions: HashMap::new(),
            incomplete_phis: HashMap::new(),
        };
        self.blocks.push(block);
    }

    pub fn current_block_id(&self) -> BasicBlockId {
        self.blocks.last().expect("No blocks created").id
    }

    pub fn add_instr(&mut self, instr: IrInstruction) {
        self.blocks
            .last_mut()
            .expect("No blocks created")
            .instrs
            .push(instr);
    }

    pub fn seal_block(&mut self, bb: BasicBlockId) {
        let incomplete = self.blocks[bb].incomplete_phis.clone();
        for (var, phi) in incomplete.iter() {
            let _ = self.add_phi_operands(*var, bb, *phi);
        }
        self.sealed.insert(bb);
    }

    // ── Algorithm 5: Remove superfluous φ functions ───────────────────────────
    //
    // A non-empty set P of φ functions is redundant iff every φ in P has
    // operands exclusively from P itself or from a single external value v.
    // We detect redundant SCCs in the induced φ-subgraph and replace or
    // recursively simplify them.

    /// Entry point.  Call this after `build_ssa` to reach minimal SSA form
    /// in the presence of irreducible control flow.
    pub fn remove_redundant_phis(&mut self) {
        // Collect every live phi: (block_id, inst_idx).
        let phi_refs: Vec<(BasicBlockId, InstId)> = self
            .blocks
            .iter()
            .flat_map(|b| {
                b.instrs
                    .iter()
                    .enumerate()
                    .filter(|(_, i)| i.is_phi())
                    .map(move |(idx, _)| (b.id, idx))
            })
            .collect();

        self.remove_redundant_phis_set(&phi_refs);
    }

    /// Core recursive procedure that operates on an arbitrary subset of phis.
    fn remove_redundant_phis_set(&mut self, phi_refs: &[(BasicBlockId, InstId)]) {
        if phi_refs.is_empty() {
            return;
        }

        // Build a map: SsaVariable → (block, inst_idx) for quick lookup.
        let phi_var_map: HashMap<SsaVariable, (BasicBlockId, InstId)> = phi_refs
            .iter()
            .filter_map(|&(bb, idx)| {
                let instr = &self.blocks[bb].instrs[idx];
                if instr.is_phi() {
                    Some((instr.phi().var.expect_var(), (bb, idx)))
                } else {
                    None // already replaced with Nop
                }
            })
            .collect();

        // Build adjacency list for Tarjan (node = index into phi_refs).
        // An edge u → w means φ_u has φ_w as an operand.
        let live_refs: Vec<(BasicBlockId, InstId)> = phi_refs
            .iter()
            .copied()
            .filter(|&(bb, idx)| self.blocks[bb].instrs[idx].is_phi())
            .collect();

        let n = live_refs.len();
        if n == 0 {
            return;
        }

        // index by SsaVariable for O(1) node lookup
        let node_index: HashMap<SsaVariable, usize> = live_refs
            .iter()
            .enumerate()
            .map(|(i, &(bb, idx))| (self.blocks[bb].instrs[idx].phi().var.expect_var(), i))
            .collect();

        // Build a petgraph DiGraph — node weight = index into live_refs.
        // An edge u → w means φ_u has φ_w as an operand.
        let mut phi_graph: DiGraph<usize, ()> = DiGraph::with_capacity(n, n * 2);
        let nodes: Vec<NodeIndex> = (0..n).map(|i| phi_graph.add_node(i)).collect();
        for (i, &(bb, idx)) in live_refs.iter().enumerate() {
            let phi = self.blocks[bb].instrs[idx].phi();
            for (op, _) in phi.operands.iter() {
                // Phi operands are always Var by invariant.
                let op_var = op.expect_var();
                if let Some(&j) = node_index.get(&op_var) {
                    phi_graph.add_edge(nodes[i], nodes[j], ());
                }
            }
        }

        // petgraph::algo::tarjan_scc returns SCCs in topological order of the
        // condensation DAG (leaves/sinks first) — process forward, no .rev() needed.
        let sccs = tarjan_scc(&phi_graph);

        for scc_nodes in &sccs {
            let scc_indices: Vec<usize> = scc_nodes.iter().map(|&nx| phi_graph[nx]).collect();
            let scc_vars: HashSet<SsaVariable> = scc_indices
                .iter()
                .map(|&i| {
                    let (bb, idx) = live_refs[i];
                    self.blocks[bb].instrs[idx].phi().var.expect_var()
                })
                .collect();

            // Skip single-element SCCs — trivial-phi removal already handled them.
            if scc_vars.len() == 1 {
                continue;
            }

            self.process_scc(&live_refs, &scc_indices, &scc_vars, &phi_var_map);
        }
    }

    /// Process one SCC from Algorithm 5.
    fn process_scc(
        &mut self,
        live_refs: &[(BasicBlockId, InstId)],
        scc_indices: &[usize],
        scc_vars: &HashSet<SsaVariable>,
        _phi_var_map: &HashMap<SsaVariable, (BasicBlockId, InstId)>,
    ) {
        let mut inner_refs: Vec<(BasicBlockId, InstId)> = Vec::new();
        let mut outer_ops: HashSet<SsaVariable> = HashSet::new();

        for &i in scc_indices {
            let (bb, idx) = live_refs[i];
            let instr = &self.blocks[bb].instrs[idx];
            if !instr.is_phi() {
                continue; // already Nop-ified
            }
            let phi = instr.phi();
            let mut is_inner = true;
            for (op, _) in phi.operands.iter() {
                let op_var = op.expect_var();
                if !scc_vars.contains(&op_var) {
                    outer_ops.insert(op_var);
                    is_inner = false;
                }
            }
            if is_inner {
                inner_refs.push((bb, idx));
            }
        }

        if outer_ops.is_empty() {
            // All predecessors are unreachable — skip this SCC.
            return;
        }

        if outer_ops.len() == 1 {
            // Every φ in the SCC resolves to the same single external value.
            let replacement = SsaValue::Var(*outer_ops.iter().next().unwrap());
            // Replace all vars in this SCC with the replacement.
            let scc_live: Vec<SsaVariable> = scc_indices
                .iter()
                .filter_map(|&i| {
                    let (bb, idx) = live_refs[i];
                    if self.blocks[bb].instrs[idx].is_phi() {
                        Some(self.blocks[bb].instrs[idx].phi().var.expect_var())
                    } else {
                        None
                    }
                })
                .collect();

            for phi_var in scc_live {
                self.replace_phi_uses(phi_var, replacement.clone());
            }
            // Tombstone all phis in this SCC.
            for &i in scc_indices {
                let (bb, idx) = live_refs[i];
                if self.blocks[bb].instrs[idx].is_phi() {
                    self.blocks[bb].instrs[idx] = IrInstruction::Nop;
                }
            }
        } else {
            // Multiple outer operands: phis that reach the outside are necessary.
            // Recursively try to eliminate the purely-inner ones.
            self.remove_redundant_phis_set(&inner_refs);
        }
    }

    pub fn dump_cfg_dot(&self) -> String {
        dump_program_ssa_dot(std::slice::from_ref(self))
    }
}

#[derive(Debug)]
pub struct SsaNodeData {
    pub label: String,
    pub xlabel: String,
}

pub fn dump_program_ssa_dot(builders: &[Builder]) -> String {
    let mut graph = Graph::<SsaNodeData, ()>::new();
    let mut node_map = HashMap::new();

    for (fn_idx, builder) in builders.iter().enumerate() {
        for block in &builder.blocks {
            let mut label = String::new();
            for (idx, instr) in block.instrs.iter().enumerate() {
                if idx > 0 {
                    label.push_str("\n");
                }
                label.push_str(&format!("{:?}", instr));
            }
            let xlabel = format!("@{}_bb_{}", builder.name, block.id);
            let node_idx = graph.add_node(SsaNodeData { label, xlabel });
            node_map.insert((fn_idx, block.id), node_idx);
        }
    }

    for (fn_idx, builder) in builders.iter().enumerate() {
        for block in &builder.blocks {
            let u = node_map[&(fn_idx, block.id)];
            for &succ_id in &block.successors {
                if let Some(&v) = node_map.get(&(fn_idx, succ_id)) {
                    graph.add_edge(u, v, ());
                }
            }
        }
    }

    let dot = Dot::with_attr_getters(
        &graph,
        &[Config::EdgeNoLabel, Config::NodeNoLabel],
        &|_, _| "".to_string(),
        &|_, (_, node_data)| {
            let escaped_label = node_data.label.replace("\"", "\\\"");
            format!(
                r#"shape=box, label="{}", xlabel="{}""#,
                escaped_label, node_data.xlabel
            )
        },
    );

    format!("{:?}", dot)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dump_cfg_dot() {
        let mut builder = Builder::new();
        builder.name = "main".to_string();
        builder.add_block(0, vec![], vec![1]);
        let v0 = SsaVariable { id: 0, index: 0 };
        builder.blocks[0].instrs.push(IrInstruction::Const(SsaValue::Var(v0), SsaValue::Int(42)));
        builder.add_block(1, vec![0], vec![]);
        builder.blocks[1].instrs.push(IrInstruction::Ret(SsaValue::Var(v0)));

        let dot = builder.dump_cfg_dot();
        println!("DOT:\n{}", dot);
        assert!(dot.contains("shape=box"));
        assert!(dot.contains("label=\"%v0_0 = const 42\""));
        assert!(dot.contains("xlabel=\"@main_bb_0\""));
        assert!(dot.contains("label=\"ret %v0_0\""));
        assert!(dot.contains("xlabel=\"@main_bb_1\""));
        assert!(dot.contains("0 -> 1 [ ]"));
    }
}

impl Debug for Builder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "SSA IR Program:")?;
        for block in self.blocks.iter() {
            writeln!(f, "{:?}", block)?;
        }
        Ok(())
    }
}
