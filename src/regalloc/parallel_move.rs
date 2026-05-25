//! Parallel-copy sequentialization (Boissinot Algorithm 13 — Briggs/Bovet).
//!
//! Given a set of parallel copies (all sources read before any destination
//! is written), produce an equivalent sequence of individual copies.
//! A `spare` allocation is used to break dependency cycles.

use std::collections::HashMap;

/// Sequentialize a set of parallel copies into an ordered list of
/// sequential copies that produces the same result.
///
/// `get_temp` returns a temporary register/variable to break cycle dependencies.
pub fn sequentialize<T>(
    parallel_copies: &[(T, T)],
    mut get_temp: impl FnMut() -> T,
) -> Vec<(T, T)>
where
    T: Clone + Copy + std::hash::Hash + Eq,
{
    if parallel_copies.is_empty() {
        return Vec::new();
    }

    // Strip self-copies.
    let copies: Vec<(T, T)> = parallel_copies.iter()
        .filter(|&&(src, dst)| src != dst)
        .copied()
        .collect();

    if copies.is_empty() {
        return Vec::new();
    }

    // `pred[d]` — the source that writes to destination `d`.
    let mut pred = HashMap::new();
    // `resource[v]` — current location of `v`'s original value.
    let mut resource = HashMap::new();
    let mut todo = Vec::new();

    for &(src, dst) in &copies {
        pred.insert(dst, src);
        resource.insert(src, src);
        todo.push(dst);
    }

    // A destination is available if it's NOT also a source — i.e. nothing
    // else needs to read its old value.
    let mut available = Vec::new();
    for &(_, dst) in &copies {
        if !copies.iter().any(|&(s, _)| s == dst) {
            available.push(dst);
        }
    }

    let mut sequentialized = Vec::new();

    while !todo.is_empty() {
        // Drain everything that can be emitted directly.
        while let Some(b) = available.pop() {
            let Some(&p) = pred.get(&b) else { continue; };
            let a = *resource.get(&p).unwrap_or(&p);
            if a != b {
                sequentialized.push((a, b));
            }
            resource.insert(p, b);
            pred.remove(&b);
            if p != a {
                if pred.contains_key(&p) {
                    available.push(p);
                }
            } else if pred.contains_key(&a) {
                available.push(a);
            }
        }

        // Find the next live destination that still has pending work.
        let mut b = None;
        while let Some(cand) = todo.pop() {
            if pred.contains_key(&cand) {
                b = Some(cand);
                break;
            }
        }
        let Some(b) = b else { break; };

        let p = pred[&b];
        let cur = *resource.get(&p).unwrap_or(&p);
        if b == cur {
            pred.remove(&b);
            continue;
        }

        // Cycle break: save the value currently at `b` into a temporary location.
        let temp = get_temp();
        sequentialized.push((b, temp));

        // Anyone whose value currently sits at `b` must now find it at `temp`.
        let mut owners = Vec::new();
        for (&v, &loc) in &resource {
            if loc == b {
                owners.push(v);
            }
        }
        for v in owners {
            resource.insert(v, temp);
        }
        available.push(b);
    }

    sequentialized
}

// ─────────────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    /// Apply `copies` as a *true* parallel copy to a register file model.
    fn apply_parallel(initial: &[(usize, i64)], copies: &[(usize, usize)]) -> HashMap<usize, i64> {
        let mut state = HashMap::new();
        for &(reg, v) in initial { state.insert(reg, v); }
        let reads: Vec<(usize, i64)> = copies.iter()
            .map(|&(src, dst)| (dst, *state.get(&src).unwrap_or(&0)))
            .collect();
        for (dst, v) in reads { state.insert(dst, v); }
        state
    }

    /// Apply `copies` SEQUENTIALLY.
    fn apply_sequential(initial: &[(usize, i64)], copies: &[(usize, usize)]) -> HashMap<usize, i64> {
        let mut state = HashMap::new();
        for &(reg, v) in initial { state.insert(reg, v); }
        for &(src, dst) in copies {
            let v = *state.get(&src).unwrap_or(&0);
            state.insert(dst, v);
        }
        state
    }

    #[test]
    fn no_copies() {
        let result = sequentialize(&[], || 63);
        assert!(result.is_empty());
    }

    #[test]
    fn single_copy() {
        let copies = vec![(0, 1)];
        let result = sequentialize(&copies, || 63);
        assert_eq!(result, copies);
    }

    #[test]
    fn independent_copies() {
        let copies = vec![(0, 1), (2, 3)];
        let result = sequentialize(&copies, || 63);
        let initial = vec![(0, 10), (1, 20), (2, 30), (3, 40)];
        let par = apply_parallel(&initial, &copies);
        let seq = apply_sequential(&initial, &result);
        assert_eq!(par.get(&1), seq.get(&1));
        assert_eq!(par.get(&3), seq.get(&3));
    }

    #[test]
    fn chain() {
        // 0→1, 1→2 — must emit 1→2 first, then 0→1
        let copies = vec![(0, 1), (1, 2)];
        let result = sequentialize(&copies, || 63);
        let initial = vec![(0, 10), (1, 20), (2, 30)];
        let par = apply_parallel(&initial, &copies);
        let seq = apply_sequential(&initial, &result);
        assert_eq!(par.get(&1), seq.get(&1));
        assert_eq!(par.get(&2), seq.get(&2));
    }

    #[test]
    fn swap_cycle_semantics() {
        // 0→1, 1→0 — must end with values swapped.
        let copies = vec![(0, 1), (1, 0)];
        let result = sequentialize(&copies, || 63);
        let initial = vec![(0, 100), (1, 200)];
        let par = apply_parallel(&initial, &copies);
        let seq = apply_sequential(&initial, &result);
        assert_eq!(par.get(&0), Some(&200));
        assert_eq!(par.get(&1), Some(&100));
        assert_eq!(seq.get(&0), par.get(&0));
        assert_eq!(seq.get(&1), par.get(&1));
    }

    #[test]
    fn three_cycle_semantics() {
        // 0→1, 1→2, 2→0
        let copies = vec![(0, 1), (1, 2), (2, 0)];
        let result = sequentialize(&copies, || 63);
        let initial = vec![(0, 1), (1, 2), (2, 3)];
        let par = apply_parallel(&initial, &copies);
        let seq = apply_sequential(&initial, &result);
        for reg in 0..3 {
            assert_eq!(seq.get(&reg), par.get(&reg));
        }
    }

    #[test]
    fn cycle_with_tree_tail() {
        // 0→1, 1→0 plus 0→2.
        let copies = vec![(0, 1), (1, 0), (0, 2)];
        let result = sequentialize(&copies, || 63);
        let initial = vec![(0, 10), (1, 20), (2, 30)];
        let par = apply_parallel(&initial, &copies);
        let seq = apply_sequential(&initial, &result);
        for reg in 0..3 {
            assert_eq!(seq.get(&reg), par.get(&reg));
        }
    }

    #[test]
    fn self_copy_filtered() {
        let copies = vec![(0, 0)];
        let result = sequentialize(&copies, || 63);
        assert!(result.is_empty());
    }
}
