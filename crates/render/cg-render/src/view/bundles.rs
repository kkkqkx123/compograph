//! Unordered edge bundle context for parallel edge spreading.
//!
//! The pair list doubles as the bundling context: offsets and haystack slots
//! derive from the full list, so bulk planning builds one context while the
//! single-edge rebuild queries the same grouping with a linear scan.

use std::collections::HashMap;

use cg_graph::NodeIndex;

use super::culling::unordered_key;
use super::plans::PaintedEdge;

/// Grouped parallel edges sharing one unordered endpoint key.
pub(crate) struct BundleContext {
    members_of: HashMap<(usize, usize), Vec<usize>>,
}

impl BundleContext {
    pub(crate) fn build(pairs: &[(NodeIndex, NodeIndex)]) -> Self {
        let mut members_of: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
        for (ordinal, (source, target)) in pairs.iter().enumerate() {
            members_of
                .entry(unordered_key(*source, *target))
                .or_default()
                .push(ordinal);
        }
        Self { members_of }
    }

    pub(crate) fn bundle_len(&self, key: (usize, usize)) -> usize {
        self.members_of.get(&key).map(Vec::len).unwrap_or(1)
    }

    pub(crate) fn slot(&self, key: (usize, usize), ordinal: usize) -> usize {
        self.members_of
            .get(&key)
            .and_then(|members| members.iter().position(|member| *member == ordinal))
            .unwrap_or(0)
    }
}

/// Bundle slot and size of `pairs[ordinal]` within its unordered bundle.
///
/// The retained cache uses this to rebuild one edge exactly as the bulk path
/// would; unknown ordinals report a lone edge instead of failing.
pub fn bundle_slot(pairs: &[(NodeIndex, NodeIndex)], ordinal: usize) -> (usize, usize) {
    let (source, target) = match pairs.get(ordinal) {
        Some(pair) => *pair,
        None => return (0, 1),
    };
    let key = unordered_key(source, target);
    let mut slot = 0usize;
    let mut len = 0usize;
    for (member, pair) in pairs.iter().enumerate() {
        if unordered_key(pair.0, pair.1) == key {
            if member == ordinal {
                slot = len;
            }
            len += 1;
        }
    }
    (slot, len.max(1))
}

/// Count of earlier self loops on the same node before `pairs[ordinal]`.
pub fn loop_ordinal(pairs: &[(NodeIndex, NodeIndex)], ordinal: usize) -> usize {
    let (source, target) = match pairs.get(ordinal) {
        Some(pair) => *pair,
        None => return 0,
    };
    if source != target {
        return 0;
    }
    pairs
        .iter()
        .take(ordinal)
        .filter(|(a, b)| *a == source && *b == target)
        .count()
}

/// Pair-list ordinals behind `edges`, in plan order.
///
/// The bulk builder preserves pair order, so the k-th painted edge of one
/// directed pair maps to the k-th pair entry. Used to key retained entries
/// without changing the plan functions' return shapes.
pub fn edge_ordinals_for(edges: &[PaintedEdge], pairs: &[(NodeIndex, NodeIndex)]) -> Vec<usize> {
    let mut ordinal_of: HashMap<(usize, usize, usize), usize> = HashMap::new();
    let mut occurrence: HashMap<(usize, usize), usize> = HashMap::new();
    for (ordinal, (source, target)) in pairs.iter().enumerate() {
        let key = (source.index(), target.index());
        let seen = occurrence.get(&key).copied().unwrap_or(0);
        occurrence.insert(key, seen + 1);
        ordinal_of.insert((key.0, key.1, seen), ordinal);
    }
    let mut next: HashMap<(usize, usize), usize> = HashMap::new();
    edges
        .iter()
        .map(|edge| {
            let key = (edge.source.index(), edge.target.index());
            let seen = next.get(&key).copied().unwrap_or(0);
            next.insert(key, seen + 1);
            ordinal_of
                .get(&(key.0, key.1, seen))
                .copied()
                .unwrap_or(usize::MAX)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_slots_match_linear_scan() {
        let pairs = vec![
            (NodeIndex::new(0), NodeIndex::new(1)),
            (NodeIndex::new(1), NodeIndex::new(0)),
            (NodeIndex::new(0), NodeIndex::new(1)),
        ];
        let context = BundleContext::build(&pairs);
        for (ordinal, (source, target)) in pairs.iter().enumerate() {
            let key = unordered_key(*source, *target);
            let (slot, len) = bundle_slot(&pairs, ordinal);
            assert_eq!(context.slot(key, ordinal), slot);
            assert_eq!(context.bundle_len(key), len);
        }
        assert_eq!(bundle_slot(&pairs, 99), (0, 1));
        assert_eq!(loop_ordinal(&pairs, 0), 0);
    }

    #[test]
    fn loop_ordinals_count_earlier_self_loops() {
        let pairs = vec![
            (NodeIndex::new(2), NodeIndex::new(2)),
            (NodeIndex::new(2), NodeIndex::new(2)),
            (NodeIndex::new(0), NodeIndex::new(1)),
        ];
        assert_eq!(loop_ordinal(&pairs, 0), 0);
        assert_eq!(loop_ordinal(&pairs, 1), 1);
        assert_eq!(loop_ordinal(&pairs, 2), 0);
        assert_eq!(loop_ordinal(&pairs, 99), 0);
    }
}
