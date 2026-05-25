use std::fmt::Debug;

use std::collections::{HashMap, HashSet};

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

#[derive(Clone, Eq, PartialEq)]
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
    pub fn variable(&self) -> SsaVariable {
        match self {
            SsaValue::Var(v) => *v,
            _ => panic!("Expected variable"),
        }
    }
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
    pub var: SsaVariable,
    pub operands: Vec<(SsaVariable, BasicBlockId)>,
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

#[derive(Clone, Eq, PartialEq)]
pub enum IrInstruction {
    Const(SsaVariable, SsaValue),
    Mov(SsaVariable, SsaValue),
    Binary(BinaryOp, SsaVariable, SsaValue, SsaValue),
    PhiAssign(Phi),
    Not(SsaVariable, SsaValue),
    Print(SsaValue),
    Jmp(BasicBlockId),
    Br(SsaValue, BasicBlockId, BasicBlockId),
    Ret(SsaValue),
    Call {
        callee_bb: usize,
        args: Vec<SsaValue>,
        dest: Option<SsaVariable>,
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
            IrInstruction::Nop => write!(f, "nop"),
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
        match self {
            IrInstruction::PhiAssign(_) => true,
            _ => false,
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
            let resolved = self.add_phi_operands(var, bb, phi);
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
    ) -> SsaVariable {
        let preds = self.get_block(bb).predecessors.clone();
        for pred in preds.iter() {
            let pred_var = self.read_variable(var, *pred);
            self.get_block_mut(bb).instrs[phi].phi_mut().operands.push((pred_var, *pred));
        }
        // All operands now present — try_remove_trivial_phi's partial-operand
        // guard will pass and the full check will run.
        self.try_remove_trivial_phi(phi, bb)
    }

    pub fn try_remove_trivial_phi(&mut self, phi: InstId, bb: BasicBlockId) -> SsaVariable {
        // Already tombstoned by a prior call in the same chain.
        if !self.get_block(bb).instrs[phi].is_phi() {
            return SsaVariable { id: usize::MAX, index: 0 };
        }

        let phi_var = self.get_block(bb).instrs[phi].phi().var;

        let n_preds = self.get_block(bb).predecessors.len();
        let n_ops   = self.get_block(bb).instrs[phi].phi().operands.len();
        if n_ops < n_preds {
            return phi_var;
        }

        let mut same: Option<SsaVariable> = None;
        for (op, _) in self.get_block(bb).instrs[phi].phi().operands.clone().iter() {
            if Some(*op) == same || *op == phi_var {
                continue;
            }
            if same.is_some() {
                return phi_var;
            }
            same = Some(*op);
        }

        if same.is_none() {
            // Unreachable phi (block has 0 preds, or only self-references).
            // Recycle phi_var by replacing the phi slot in place with a
            // `Const 0` that defines phi_var.  Existing uses keep their
            // references; phi_var now resolves to a concrete SSA value
            // instead of an undef phi (avoids SsaValue::Undef leaking into
            // instruction operands downstream).
            self.get_block_mut(bb).instrs[phi] =
                IrInstruction::Const(phi_var, SsaValue::Int(0));
            return phi_var;
        }
        let resolved = same.unwrap();

        self.replace_phi_uses(phi_var, SsaValue::Var(resolved));
        self.get_block_mut(bb).instrs[phi] = IrInstruction::Nop;

        for block in self.blocks.iter_mut() {
            for (_, def) in block.definitions.iter_mut() {
                if *def == phi_var {
                    *def = resolved;
                }
            }
        }

        let users = self.get_phi_users(resolved);
        for (block, inst) in users {
            if self.get_block(block).instrs[inst].is_phi() {
                self.try_remove_trivial_phi(inst, block);
            }
        }

        resolved
    }

    fn get_phi_users(&self, var: SsaVariable) -> Vec<(BasicBlockId, InstId)> {
        let mut users = Vec::new();
        for block in self.blocks.iter() {
            for (i, inst) in block.instrs.iter().enumerate() {
                if let IrInstruction::PhiAssign(phi) = inst {
                    if phi.operands.iter().any(|(op, _)| *op == var) {
                        users.push((block.id, i));
                    }
                }
            }
        }
        users
    }

    pub fn replace_phi_uses(&mut self, old: SsaVariable, new: SsaValue) {
        let old_val = SsaValue::Var(old);
        for block in self.blocks.iter_mut() {
            for inst in block.instrs.iter_mut() {
                match inst {
                    IrInstruction::PhiAssign(phi) => {
                        // Phi operands are SsaVariable — they cannot represent
                        // Undef.  Only replace when the new value is a Var.
                        if let SsaValue::Var(new_var) = &new {
                            for (op, _) in phi.operands.iter_mut() {
                                if *op == old {
                                    *op = *new_var;
                                }
                            }
                        }
                        // If new is Undef the phi operand stays as-is;
                        // the phi itself will be tombstoned right after this call.
                    }
                    IrInstruction::Binary(_, _, src1, src2) => {
                        if *src1 == old_val {
                            *src1 = new.clone();
                        }
                        if *src2 == old_val {
                            *src2 = new.clone();
                        }
                    }
                    IrInstruction::Const(_, src)
                    | IrInstruction::Mov(_, src)
                    | IrInstruction::Not(_, src) => {
                        if *src == old_val {
                            *src = new.clone();
                        }
                    }
                    IrInstruction::Print(src) | IrInstruction::Ret(src) => {
                        if *src == old_val {
                            *src = new.clone();
                        }
                    }
                    IrInstruction::Br(cond, _, _) => {
                        if *cond == old_val {
                            *cond = new.clone();
                        }
                    }
                    IrInstruction::Call { args, .. } => {
                        for arg in args.iter_mut() {
                            if *arg == old_val {
                                *arg = new.clone();
                            }
                        }
                    }
                    _ => {}
                }
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
            var,
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
            self.add_phi_operands(*var, bb, *phi);
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
                    Some((instr.phi().var, (bb, idx)))
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
            .map(|(i, &(bb, idx))| (self.blocks[bb].instrs[idx].phi().var, i))
            .collect();

        let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
        for (i, &(bb, idx)) in live_refs.iter().enumerate() {
            let phi = self.blocks[bb].instrs[idx].phi();
            for (op, _) in phi.operands.iter() {
                if let Some(&j) = node_index.get(op) {
                    adj[i].push(j);
                }
            }
        }

        // Tarjan's SCC — returns SCCs in *reverse* topological order (leaves last).
        let sccs = Self::tarjan_scc(n, &adj);

        // Process in topological order = iterate sccs in reverse.
        for scc_indices in sccs.iter().rev() {
            let scc_vars: HashSet<SsaVariable> = scc_indices
                .iter()
                .map(|&i| {
                    let (bb, idx) = live_refs[i];
                    self.blocks[bb].instrs[idx].phi().var
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
                if !scc_vars.contains(op) {
                    outer_ops.insert(*op);
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
                        Some(self.blocks[bb].instrs[idx].phi().var)
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

    /// Tarjan's strongly connected components algorithm.
    /// Returns SCCs in reverse topological order (first SCC = a root in the DAG).
    fn tarjan_scc(n: usize, adj: &[Vec<usize>]) -> Vec<Vec<usize>> {
        let mut index_counter = 0usize;
        let mut stack: Vec<usize> = Vec::new();
        let mut on_stack = vec![false; n];
        let mut index = vec![usize::MAX; n];
        let mut lowlink = vec![0usize; n];
        let mut sccs: Vec<Vec<usize>> = Vec::new();

        // Iterative Tarjan to avoid stack-overflow on large graphs.
        // Each stack frame: (node, iterator position in adj[node]).
        let mut call_stack: Vec<(usize, usize)> = Vec::new();

        for start in 0..n {
            if index[start] != usize::MAX {
                continue;
            }

            call_stack.push((start, 0));

            while let Some((v, ei)) = call_stack.last_mut() {
                let v = *v;
                if index[v] == usize::MAX {
                    // First visit.
                    index[v] = index_counter;
                    lowlink[v] = index_counter;
                    index_counter += 1;
                    stack.push(v);
                    on_stack[v] = true;
                }

                let edges = &adj[v];
                if *ei < edges.len() {
                    let w = edges[*ei];
                    *ei += 1;
                    if index[w] == usize::MAX {
                        call_stack.push((w, 0));
                    } else if on_stack[w] {
                        let lv = lowlink[v];
                        lowlink[v] = lv.min(index[w]);
                    }
                } else {
                    // Done with v's children.
                    call_stack.pop();

                    if let Some(&(parent, _)) = call_stack.last() {
                        let lv = lowlink[parent];
                        lowlink[parent] = lv.min(lowlink[v]);
                    }

                    // Is v a root of an SCC?
                    if lowlink[v] == index[v] {
                        let mut scc = Vec::new();
                        loop {
                            let w = stack.pop().unwrap();
                            on_stack[w] = false;
                            scc.push(w);
                            if w == v {
                                break;
                            }
                        }
                        sccs.push(scc);
                    }
                }
            }
        }

        sccs
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
