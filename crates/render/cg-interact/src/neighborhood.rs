//! Interactive neighborhood expansion around seed nodes.

use std::collections::{BTreeSet, HashMap, VecDeque};

use cg_graph::{GraphView, NodeIndex};

/// Nodes within `hops` undirected steps of `seeds`, seeds included.
///
/// The expansion reads only the neighbor queries, so folded or hidden nodes
/// are filtered by the caller. Zero hops return the seeds alone, and an empty
/// seed set stays empty. This is the interactive fringe primitive with hop
/// semantics; the closed one-hop and transitive closures used by algorithms
/// live in `cg-graph` collection queries.
pub fn expand_neighborhood(
    graph: &dyn GraphView,
    seeds: impl IntoIterator<Item = NodeIndex>,
    hops: usize,
) -> BTreeSet<NodeIndex> {
    let mut visited: BTreeSet<NodeIndex> = seeds.into_iter().collect();
    if hops == 0 || visited.is_empty() {
        return visited;
    }
    let mut frontier: VecDeque<NodeIndex> = visited.iter().copied().collect();
    let mut depth: HashMap<NodeIndex, usize> = visited.iter().map(|node| (*node, 0)).collect();
    while let Some(node) = frontier.pop_front() {
        let current = depth.get(&node).copied().unwrap_or(0);
        if current >= hops {
            continue;
        }
        for neighbour in graph.neighbors(node) {
            if visited.insert(neighbour) {
                depth.insert(neighbour, current + 1);
                frontier.push_back(neighbour);
            }
        }
    }
    visited
}

/// Directed edges with both endpoints inside `expanded`, in sorted order.
///
/// Restricting to internal edges keeps highlighted edges anchored at tinted
/// nodes on both ends, for any hop depth.
pub fn neighborhood_edges(
    graph: &dyn GraphView,
    expanded: &BTreeSet<NodeIndex>,
) -> Vec<(NodeIndex, NodeIndex)> {
    if expanded.is_empty() {
        return Vec::new();
    }
    let mut found: Vec<(NodeIndex, NodeIndex)> = graph
        .edges()
        .into_iter()
        .filter(|(source, target)| expanded.contains(source) && expanded.contains(target))
        .collect();
    found.sort_unstable_by_key(|(source, target)| (source.index(), target.index()));
    found.dedup();
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use cg_graph::MockGraph;

    #[test]
    fn neighborhood_expands_by_hops_and_clears_with_seeds() {
        let graph = MockGraph::chain(4);
        let seed = [NodeIndex::new(1)];
        let none = expand_neighborhood(&graph, seed, 0);
        assert_eq!(none, BTreeSet::from([NodeIndex::new(1)]));
        let one = expand_neighborhood(&graph, [NodeIndex::new(1)], 1);
        assert_eq!(
            one,
            BTreeSet::from([NodeIndex::new(0), NodeIndex::new(1), NodeIndex::new(2)])
        );
        let two = expand_neighborhood(&graph, [NodeIndex::new(1)], 2);
        assert_eq!(
            two,
            BTreeSet::from([
                NodeIndex::new(0),
                NodeIndex::new(1),
                NodeIndex::new(2),
                NodeIndex::new(3)
            ])
        );
        let empty = expand_neighborhood(&graph, [], 2);
        assert!(empty.is_empty());
        let edges = neighborhood_edges(&graph, &one);
        assert_eq!(
            edges,
            vec![
                (NodeIndex::new(0), NodeIndex::new(1)),
                (NodeIndex::new(1), NodeIndex::new(2))
            ]
        );
        assert!(neighborhood_edges(&graph, &BTreeSet::new()).is_empty());
    }
}
