//! Combinatorial searches delegated to petgraph through compact mirrors.
//!
//! The store owns a stable index graph that may hold holes after removals,
//! while several upstream searches require a compact index bound. Every
//! function here therefore builds a temporary compact mirror with the same
//! node set and translates results back to store identifiers. Directed edges
//! are read as undirected for matching, coloring and cliques, with parallel
//! edges collapsed and self loops ignored, matching the undirected searches
//! elsewhere. Flow keeps parallel edges and drops self loops, with negative
//! weights clamped to zero capacity.

use std::collections::{HashMap, HashSet};

use petgraph::Directed;
use petgraph::algo::{
    all_simple_paths, dsatur_coloring, greedy_feedback_arc_set, greedy_matching, maximal_cliques,
    maximum_matching,
};
use petgraph::algo::maximum_flow::dinics;
use petgraph::graph::{Graph, NodeIndex as CompactIndex};
use petgraph::stable_graph::{NodeIndex, StableGraph};
use petgraph::Undirected;
use petgraph::visit::{EdgeRef, IntoEdgeReferences};

use crate::store::{EdgeData, NodeData};

type GraphStore = StableGraph<NodeData, EdgeData, Directed>;

type FlowMirror = (
    Graph<(), f32, Directed>,
    HashMap<NodeIndex, CompactIndex>,
    HashMap<CompactIndex, NodeIndex>,
    Vec<(NodeIndex, NodeIndex)>,
);

type UndirectedMirror = (
    Graph<(), (), Undirected>,
    HashMap<NodeIndex, CompactIndex>,
    HashMap<CompactIndex, NodeIndex>,
);

/// Maximum directed flow value from `source` to `goal` with per edge flows.
///
/// Capacities are the stored weights clamped at zero; self loops are skipped
/// because they never carry flow from source to goal. Each flow entry follows
/// the store edge order. Missing endpoints or a missing path yield zero with
/// no entries.
pub fn maximum_flow_value(
    graph: &GraphStore,
    source: NodeIndex,
    goal: NodeIndex,
) -> (f32, Vec<(NodeIndex, NodeIndex, f32)>) {
    if graph.node_weight(source).is_none()
        || graph.node_weight(goal).is_none()
        || source == goal
    {
        return (0.0, Vec::new());
    }
    let (mirror, forward, _, _) = flow_mirror(graph);
    let (Some(mirror_source), Some(mirror_goal)) =
        (forward.get(&source).copied(), forward.get(&goal).copied())
    else {
        return (0.0, Vec::new());
    };
    let (value, flows) = dinics(&mirror, mirror_source, mirror_goal);
    if value <= 0.0 {
        return (0.0, Vec::new());
    }
    let mut detailed = Vec::new();
    for (ordinal, reference) in graph.edge_references().enumerate() {
        let amount = flows.get(ordinal).copied().unwrap_or(0.0);
        if amount > 0.0 {
            detailed.push((reference.source(), reference.target(), amount));
        }
    }
    detailed.sort_unstable_by_key(|(from, to, _)| (from.index(), to.index()));
    (value, detailed)
}

/// Greedy matching pairs under undirected semantics.
///
/// Each pair holds the smaller endpoint first and pairs arrive sorted. Empty
/// graphs yield no pairs.
pub fn greedy_matching_pairs(graph: &GraphStore) -> Vec<(NodeIndex, NodeIndex)> {
    let (mirror, _, backward) = undirected_mirror(graph);
    let matched: Vec<(CompactIndex, CompactIndex)> =
        greedy_matching(&mirror).edges().collect();
    matching_pairs(matched, &backward)
}

/// Maximum matching pairs under undirected semantics.
///
/// Shares the input contract and output ordering of [`greedy_matching_pairs`].
pub fn maximum_matching_pairs(graph: &GraphStore) -> Vec<(NodeIndex, NodeIndex)> {
    let (mirror, _, backward) = undirected_mirror(graph);
    let matched: Vec<(CompactIndex, CompactIndex)> =
        maximum_matching(&mirror).edges().collect();
    matching_pairs(matched, &backward)
}

/// Groups by DSatur color with the color count.
///
/// Groups are ordered by their smallest member and members arrive sorted, so
/// repeated runs agree. Empty graphs yield no groups and zero colors.
pub fn dsatur_groups(graph: &GraphStore) -> (Vec<Vec<NodeIndex>>, usize) {
    let (mirror, _, backward) = undirected_mirror(graph);
    let (colors, count) = dsatur_coloring(&mirror);
    let mut buckets: HashMap<usize, Vec<NodeIndex>> = HashMap::new();
    for (mirror_node, color) in colors {
        if let Some(node) = backward.get(&mirror_node).copied() {
            buckets.entry(color).or_default().push(node);
        }
    }
    let mut groups: Vec<Vec<NodeIndex>> = buckets.into_values().collect();
    for group in groups.iter_mut() {
        group.sort_unstable_by_key(|node| node.index());
    }
    groups.sort_unstable_by_key(|group| group.first().map(|node| node.index()).unwrap_or(usize::MAX));
    (groups, count)
}

/// Maximal cliques under undirected semantics.
///
/// Singletons appear only for isolated nodes; every larger clique is maximal
/// by inclusion. Groups arrive sorted by members with deterministic order.
pub fn maximal_clique_groups(graph: &GraphStore) -> Vec<Vec<NodeIndex>> {
    let (mirror, _, backward) = undirected_mirror(graph);
    let mut groups: Vec<Vec<NodeIndex>> = maximal_cliques(&mirror)
        .into_iter()
        .map(|members| {
            let mut group: Vec<NodeIndex> = members
                .into_iter()
                .filter_map(|mirror_node| backward.get(&mirror_node).copied())
                .collect();
            group.sort_unstable_by_key(|node| node.index());
            group
        })
        .collect();
    groups.sort_unstable_by(|a, b| a.len().cmp(&b.len()).then(a.cmp(b)));
    groups
}

/// Edges whose removal makes the directed graph acyclic, via the greedy rule.
///
/// Each entry is a store endpoint pair in edge order with duplicates kept, so
/// parallel edges are reported per physical edge. Empty and acyclic graphs
/// yield no entries.
pub fn feedback_arc_edges(graph: &GraphStore) -> Vec<(NodeIndex, NodeIndex)> {
    let doomed: HashSet<(NodeIndex, NodeIndex)> = greedy_feedback_arc_set(graph)
        .map(|reference| (reference.source(), reference.target()))
        .collect();
    let mut remaining: HashMap<(NodeIndex, NodeIndex), usize> = HashMap::new();
    for pair in &doomed {
        *remaining.entry(*pair).or_insert(0) += 1;
    }
    let mut kept = Vec::new();
    for reference in graph.edge_references() {
        let pair = (reference.source(), reference.target());
        if let Some(budget) = remaining.get_mut(&pair) {
            if *budget > 0 {
                *budget -= 1;
                kept.push(pair);
            }
        }
    }
    kept
}

/// Simple directed paths from `from` to `to`, truncated to `limit`.
///
/// Paths hold node sequences without repeated nodes. A missing endpoint or a
/// zero limit yields no paths. Enumeration stops after `limit` paths, so
/// callers stay safe on dense graphs.
pub fn simple_paths_limited(
    graph: &GraphStore,
    from: NodeIndex,
    to: NodeIndex,
    limit: usize,
) -> Vec<Vec<NodeIndex>> {
    if limit == 0 || graph.node_weight(from).is_none() || graph.node_weight(to).is_none() {
        return Vec::new();
    }
    all_simple_paths::<Vec<NodeIndex>, _, std::collections::hash_map::RandomState>(
        graph,
        from,
        to,
        0,
        None,
    )
    .take(limit)
    .collect()
}

/// Translates upstream matched edge endpoints back to store pairs.
fn matching_pairs(
    matched: Vec<(CompactIndex, CompactIndex)>,
    backward: &HashMap<CompactIndex, NodeIndex>,
) -> Vec<(NodeIndex, NodeIndex)> {
    let mut pairs = Vec::new();
    for (from_mirror, to_mirror) in matched {
        if let (Some(from), Some(to)) = (
            backward.get(&from_mirror).copied(),
            backward.get(&to_mirror).copied(),
        ) {
            if from.index() <= to.index() {
                pairs.push((from, to));
            } else {
                pairs.push((to, from));
            }
        }
    }
    pairs.sort_unstable_by_key(|(from, to)| (from.index(), to.index()));
    pairs
}

/// Compact directed mirror preserving one edge per store edge in order.
///
/// Capacities clamp negative weights to zero and self loops are skipped.
fn flow_mirror(graph: &GraphStore) -> FlowMirror {
    let mut mirror: Graph<(), f32, Directed> = Graph::default();
    let mut forward = HashMap::new();
    let mut backward = HashMap::new();
    for node in graph.node_indices() {
        let mirror_node = mirror.add_node(());
        forward.insert(node, mirror_node);
        backward.insert(mirror_node, node);
    }
    let mut order = Vec::new();
    for reference in graph.edge_references() {
        let source = reference.source();
        let target = reference.target();
        if source == target {
            continue;
        }
        if let (Some(from), Some(to)) = (forward.get(&source).copied(), forward.get(&target).copied())
        {
            mirror.add_edge(from, to, reference.weight().weight.max(0.0));
            order.push((source, target));
        }
    }
    (mirror, forward, backward, order)
}

/// Compact undirected mirror with parallel edges collapsed.
fn undirected_mirror(graph: &GraphStore) -> UndirectedMirror {
    let mut mirror: Graph<(), (), Undirected> = Graph::default();
    let mut forward = HashMap::new();
    let mut backward = HashMap::new();
    for node in graph.node_indices() {
        let mirror_node = mirror.add_node(());
        forward.insert(node, mirror_node);
        backward.insert(mirror_node, node);
    }
    let mut seen: HashSet<(usize, usize)> = HashSet::new();
    for reference in graph.edge_references() {
        let source = reference.source();
        let target = reference.target();
        if source == target {
            continue;
        }
        let (low, high) = if source.index() <= target.index() {
            (source.index(), target.index())
        } else {
            (target.index(), source.index())
        };
        if !seen.insert((low, high)) {
            continue;
        }
        if let (Some(from), Some(to)) = (forward.get(&source).copied(), forward.get(&target).copied())
        {
            mirror.add_edge(from, to, ());
        }
    }
    (mirror, forward, backward)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labelled(label: &str) -> NodeData {
        NodeData { label: label.into() }
    }

    fn weighted(weight: f32) -> EdgeData {
        EdgeData { weight }
    }

    fn diamond() -> (GraphStore, NodeIndex, NodeIndex, NodeIndex, NodeIndex) {
        let mut graph: GraphStore = StableGraph::default();
        let a = graph.add_node(labelled("a"));
        let b = graph.add_node(labelled("b"));
        let c = graph.add_node(labelled("c"));
        let d = graph.add_node(labelled("d"));
        graph.add_edge(a, b, weighted(1.0));
        graph.add_edge(b, d, weighted(1.0));
        graph.add_edge(a, c, weighted(1.0));
        graph.add_edge(c, d, weighted(1.0));
        (graph, a, b, c, d)
    }

    #[test]
    fn flow_sums_parallel_capacity() {
        let (graph, a, _, _, d) = diamond();
        let (value, detailed) = maximum_flow_value(&graph, a, d);
        assert!((value - 2.0).abs() < 1e-5);
        assert_eq!(detailed.len(), 4);
        assert!(maximum_flow_value(&graph, d, a).0 >= 0.0);
        assert!(maximum_flow_value(&graph, a, NodeIndex::new(99)).0 == 0.0);
    }

    #[test]
    fn matchings_pair_every_node_of_a_pair() {
        let mut graph: GraphStore = StableGraph::default();
        let a = graph.add_node(labelled("a"));
        let b = graph.add_node(labelled("b"));
        graph.add_edge(a, b, weighted(1.0));
        assert_eq!(greedy_matching_pairs(&graph), vec![(a, b)]);
        assert_eq!(maximum_matching_pairs(&graph), vec![(a, b)]);
        assert!(greedy_matching_pairs(&StableGraph::default()).is_empty());
    }

    #[test]
    fn coloring_and_cliques_cover_a_triangle() {
        let mut graph: GraphStore = StableGraph::default();
        let nodes: Vec<NodeIndex> = (0..3).map(|ordinal| graph.add_node(labelled(&ordinal.to_string()))).collect();
        graph.add_edge(nodes[0], nodes[1], weighted(1.0));
        graph.add_edge(nodes[1], nodes[2], weighted(1.0));
        graph.add_edge(nodes[2], nodes[0], weighted(1.0));
        let (groups, count) = dsatur_groups(&graph);
        assert_eq!(count, 3);
        assert_eq!(groups.len(), 3);
        let cliques = maximal_clique_groups(&graph);
        assert!(cliques.iter().any(|group| group.len() == 3));
    }

    #[test]
    fn feedback_and_simple_paths_cover_a_cycle() {
        let mut graph: GraphStore = StableGraph::default();
        let a = graph.add_node(labelled("a"));
        let b = graph.add_node(labelled("b"));
        graph.add_edge(a, b, weighted(1.0));
        graph.add_edge(b, a, weighted(1.0));
        assert!(!feedback_arc_edges(&graph).is_empty());
        let paths = simple_paths_limited(&graph, a, b, 5);
        assert!(!paths.is_empty());
        assert!(simple_paths_limited(&graph, a, NodeIndex::new(99), 5).is_empty());
        assert!(simple_paths_limited(&graph, a, b, 0).is_empty());
    }
}
