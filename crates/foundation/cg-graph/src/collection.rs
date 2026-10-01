//! Ordered element sets with union, intersection and closure queries.
//!
//! Sets enter and leave as sorted vectors, so query results stay
//! deterministic. Neighborhood, successor and predecessor closures share the
//! undirected and directed readings of the algorithm bridge, keeping both
//! paths in agreement. This module is the algorithmic set ground; the
//! hop-limited fringe expansion used for interactive highlighting lives in
//! `cg-interact` and keeps its own hop semantics on purpose.

use std::collections::BTreeSet;

use petgraph::stable_graph::NodeIndex;

use crate::algo::undirected_connected_components;
use crate::store::GraphStore;
use crate::view::GraphView;

/// Sorted node set with union, intersection and difference.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NodeSet(BTreeSet<NodeIndex>);

/// Sorted directed edge set keyed by endpoint pairs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EdgeSet(BTreeSet<(NodeIndex, NodeIndex)>);

impl NodeSet {
    pub fn new() -> Self {
        Self(BTreeSet::new())
    }

    /// Set holding `members` in sorted order.
    pub fn from_slice(members: &[NodeIndex]) -> Self {
        Self(members.iter().copied().collect())
    }

    /// Every visible node of `store`.
    pub fn universe(store: &GraphStore) -> Self {
        Self(store.visible_node_ids().into_iter().collect())
    }

    pub fn contains(&self, node: NodeIndex) -> bool {
        self.0.contains(&node)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Members in index order.
    pub fn sorted(&self) -> Vec<NodeIndex> {
        self.0.iter().copied().collect()
    }

    pub fn union(&self, other: &Self) -> Self {
        Self(self.0.union(&other.0).copied().collect())
    }

    pub fn intersection(&self, other: &Self) -> Self {
        Self(self.0.intersection(&other.0).copied().collect())
    }

    pub fn difference(&self, other: &Self) -> Self {
        Self(self.0.difference(&other.0).copied().collect())
    }

    /// Visible nodes outside the set.
    pub fn complement(&self, store: &GraphStore) -> Self {
        Self::universe(store).difference(self)
    }

    /// Members satisfying `keep`, in index order.
    pub fn filter(&self, mut keep: impl FnMut(NodeIndex) -> bool) -> Self {
        Self(self.0.iter().copied().filter(|node| keep(*node)).collect())
    }
}

impl EdgeSet {
    pub fn new() -> Self {
        Self(BTreeSet::new())
    }

    /// Set holding `members` in endpoint order.
    pub fn from_slice(members: &[(NodeIndex, NodeIndex)]) -> Self {
        Self(members.iter().copied().collect())
    }

    /// Every visible edge of `store`.
    pub fn universe(store: &GraphStore) -> Self {
        Self(store.visible_edges().into_iter().collect())
    }

    pub fn contains(&self, source: NodeIndex, target: NodeIndex) -> bool {
        self.0.contains(&(source, target))
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Members in endpoint order.
    pub fn sorted(&self) -> Vec<(NodeIndex, NodeIndex)> {
        self.0.iter().copied().collect()
    }

    pub fn union(&self, other: &Self) -> Self {
        Self(self.0.union(&other.0).copied().collect())
    }

    pub fn intersection(&self, other: &Self) -> Self {
        Self(self.0.intersection(&other.0).copied().collect())
    }

    pub fn difference(&self, other: &Self) -> Self {
        Self(self.0.difference(&other.0).copied().collect())
    }

    /// Visible edges outside the set.
    pub fn complement(&self, store: &GraphStore) -> Self {
        Self::universe(store).difference(self)
    }

    /// Members satisfying `keep`, in endpoint order.
    pub fn filter(&self, mut keep: impl FnMut(NodeIndex, NodeIndex) -> bool) -> Self {
        Self(
            self.0
                .iter()
                .copied()
                .filter(|(source, target)| keep(*source, *target))
                .collect(),
        )
    }
}

/// Closed neighborhood of `seeds`: the seeds plus every visible neighbor.
pub fn neighborhood(store: &GraphStore, seeds: &[NodeIndex]) -> Vec<NodeIndex> {
    let mut closed = BTreeSet::new();
    for seed in seeds {
        if !store.is_visible(*seed) {
            continue;
        }
        closed.insert(*seed);
        for next in GraphView::neighbors(store, *seed) {
            closed.insert(next);
        }
    }
    closed.into_iter().collect()
}

/// Transitive successors of `seeds`; seeds recur only when reachable again.
pub fn successors_closure(store: &GraphStore, seeds: &[NodeIndex]) -> Vec<NodeIndex> {
    closure_over(store, seeds, false)
}

/// Transitive predecessors of `seeds`; seeds recur only when reachable again.
pub fn predecessors_closure(store: &GraphStore, seeds: &[NodeIndex]) -> Vec<NodeIndex> {
    closure_over(store, seeds, true)
}

fn closure_over(store: &GraphStore, seeds: &[NodeIndex], incoming: bool) -> Vec<NodeIndex> {
    let mut seen = BTreeSet::new();
    let mut stack: Vec<NodeIndex> = seeds
        .iter()
        .copied()
        .filter(|seed| store.is_visible(*seed))
        .collect();
    while let Some(node) = stack.pop() {
        let next: Vec<NodeIndex> = if incoming {
            GraphView::predecessors(store, node)
        } else {
            GraphView::successors(store, node)
        };
        for member in next {
            if seen.insert(member) {
                stack.push(member);
            }
        }
    }
    seen.into_iter().collect()
}

/// Undirected component holding `seed`, or nothing when it is hidden.
pub fn component_of(store: &GraphStore, seed: NodeIndex) -> Vec<NodeIndex> {
    if !store.is_visible(seed) {
        return Vec::new();
    }
    let mut seen = BTreeSet::from([seed]);
    let mut stack = vec![seed];
    while let Some(node) = stack.pop() {
        for next in GraphView::neighbors(store, node) {
            if seen.insert(next) {
                stack.push(next);
            }
        }
    }
    seen.into_iter().collect()
}

/// Every undirected component, matching the algorithm bridge grouping.
pub fn all_components(store: &GraphStore) -> Vec<Vec<NodeIndex>> {
    undirected_connected_components(store.graph())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{EdgeData, NodeData};
    use petgraph::Directed;
    use petgraph::stable_graph::StableGraph;

    fn chain_store() -> (GraphStore, [NodeIndex; 3]) {
        let mut graph: StableGraph<NodeData, EdgeData, Directed> = StableGraph::default();
        let a = graph.add_node(NodeData { label: "a".into() });
        let b = graph.add_node(NodeData { label: "b".into() });
        let c = graph.add_node(NodeData { label: "c".into() });
        graph.add_edge(a, b, EdgeData { weight: 1.0 });
        graph.add_edge(b, c, EdgeData { weight: 1.0 });
        let store = GraphStore {
            graph,
            ..GraphStore::new()
        };
        (store, [a, b, c])
    }

    #[test]
    fn set_algebra_covers_union_intersection_difference_complement() {
        let (store, [a, b, c]) = chain_store();
        let left = NodeSet::from_slice(&[a, b]);
        let right = NodeSet::from_slice(&[b, c]);
        assert_eq!(left.union(&right).sorted(), vec![a, b, c]);
        assert_eq!(left.intersection(&right).sorted(), vec![b]);
        assert_eq!(left.difference(&right).sorted(), vec![a]);
        assert_eq!(left.complement(&store).sorted(), vec![c]);
        assert_eq!(right.filter(|node| node == b).sorted(), vec![b]);
        assert!(NodeSet::new().is_empty());
        let edges = EdgeSet::universe(&store);
        assert_eq!(edges.len(), 2);
        assert!(edges.contains(a, b));
        let one = EdgeSet::from_slice(&[(a, b)]);
        assert_eq!(one.complement(&store).sorted(), vec![(b, c)]);
        assert_eq!(one.filter(|source, _| source == b).sorted(), Vec::new());
    }

    #[test]
    fn closures_follow_directed_and_undirected_readings() {
        let (store, [a, b, c]) = chain_store();
        assert_eq!(neighborhood(&store, &[b]), vec![a, b, c]);
        assert_eq!(successors_closure(&store, &[a]), vec![b, c]);
        assert_eq!(predecessors_closure(&store, &[c]), vec![a, b]);
        assert_eq!(component_of(&store, a), vec![a, b, c]);
        assert_eq!(all_components(&store), vec![vec![a, b, c]]);
        assert!(successors_closure(&store, &[]).is_empty());
        assert!(component_of(&store, NodeIndex::new(99)).is_empty());
    }

    #[test]
    fn cycles_revisit_their_seeds_while_chains_do_not() {
        let mut graph: StableGraph<NodeData, EdgeData, Directed> = StableGraph::default();
        let a = graph.add_node(NodeData { label: "a".into() });
        let b = graph.add_node(NodeData { label: "b".into() });
        graph.add_edge(a, b, EdgeData { weight: 1.0 });
        graph.add_edge(b, a, EdgeData { weight: 1.0 });
        let store = GraphStore {
            graph,
            ..GraphStore::new()
        };
        assert_eq!(successors_closure(&store, &[a]), vec![a, b]);
        let (chain, [head, _, _]) = chain_store();
        assert!(!successors_closure(&chain, &[head]).contains(&head));
    }
}
