//! Shortest-path searches over the store graph.

use std::collections::HashMap;

use petgraph::Directed;
use petgraph::algo::{
    astar, bellman_ford, bidirectional_dijkstra, dijkstra, find_negative_cycle, johnson,
    k_shortest_path, spfa,
};
use petgraph::stable_graph::{NodeIndex, StableGraph};
use petgraph::visit::{EdgeRef, IntoEdgeReferences, NodeIndexable};

use crate::positions::Positions;
use crate::store::{EdgeData, NodeData};

type Graph = StableGraph<NodeData, EdgeData, Directed>;

type WeightMirror = (
    StableGraph<(), f32, Directed>,
    HashMap<NodeIndex, NodeIndex>,
    HashMap<NodeIndex, NodeIndex>,
);

type DistanceMaps = (
    HashMap<NodeIndex, f32>,
    HashMap<NodeIndex, Option<NodeIndex>>,
);

/// Shortest-path distances from `start`, using edge weights as costs.
///
/// Nodes unreachable from `start` are absent from the map. Distances use the
/// same sign convention as petgraph; negative weights are not supported by
/// Dijkstra and are the caller's responsibility.
pub fn shortest_paths(graph: &Graph, start: NodeIndex) -> HashMap<NodeIndex, f32> {
    dijkstra(graph, start, None, |edge| edge.weight().weight)
        .into_iter()
        .collect()
}

/// Shortest-path distance between two nodes, if one exists.
pub fn shortest_path_cost(graph: &Graph, start: NodeIndex, goal: NodeIndex) -> Option<f32> {
    dijkstra(graph, start, Some(goal), |edge| edge.weight().weight)
        .get(&goal)
        .copied()
}

/// Shortest-path distance searched from both endpoints at once.
///
/// Delegates directly to petgraph, which needs no compact index bound here.
/// Missing endpoints yield no cost rather than an error.
pub fn bidirectional_path_cost(
    graph: &Graph,
    start: NodeIndex,
    goal: NodeIndex,
) -> Option<f32> {
    if graph.node_weight(start).is_none() || graph.node_weight(goal).is_none() {
        return None;
    }
    bidirectional_dijkstra(graph, start, goal, |edge| edge.weight().weight)
}

/// Single-source distances via the queue based shortest path search.
///
/// Runs on the weight mirror so node removals cannot break the compact index
/// bound the upstream search requires. Returns a cycle member on failure and
/// empty maps for a missing source.
pub fn spfa_paths(graph: &Graph, source: NodeIndex) -> Result<DistanceMaps, NodeIndex> {
    if graph.node_weight(source).is_none() {
        return Ok((HashMap::new(), HashMap::new()));
    }
    let (mirror, forward, backward) = mirror_for_negative(graph);
    let Some(mirror_source) = forward.get(&source).copied() else {
        return Ok((HashMap::new(), HashMap::new()));
    };
    match spfa(&mirror, mirror_source, |edge| *edge.weight()) {
        Ok(paths) => {
            let mut distances = HashMap::new();
            let mut predecessors = HashMap::new();
            for node in graph.node_indices() {
                let Some(mirror_node) = forward.get(&node).copied() else {
                    continue;
                };
                let slot = mirror.to_index(mirror_node);
                let reached = paths.distances.get(slot).copied().unwrap_or(f32::INFINITY);
                if reached.is_finite() {
                    distances.insert(node, reached);
                    let parent = paths.predecessors.get(slot).copied().flatten().and_then(
                        |mirror_parent| backward.get(&mirror_parent).copied(),
                    );
                    predecessors.insert(node, parent);
                }
            }
            Ok((distances, predecessors))
        }
        Err(_) => Err(find_negative_cycle(&mirror, mirror_source)
            .and_then(|cycle| cycle.first().copied())
            .and_then(|mirror_member| backward.get(&mirror_member).copied())
            .unwrap_or(source)),
    }
}

/// All-pairs distances via reweighted single-source searches.
///
/// Runs on the weight mirror for the same compactness reason as the negative
/// weight searches. Only reachable pairs are present and the diagonal holds
/// zeros. Returns a cycle member when a negative cycle exists.
pub fn johnson_paths(
    graph: &Graph,
) -> Result<HashMap<(NodeIndex, NodeIndex), f32>, NodeIndex> {
    if graph.node_count() == 0 {
        return Ok(HashMap::new());
    }
    let (mirror, _, backward) = mirror_for_negative(graph);
    let reverse: HashMap<NodeIndex, NodeIndex> = backward.iter().map(|(a, b)| (*b, *a)).collect();
    match johnson(&mirror, |edge| *edge.weight()) {
        Ok(matrix) => {
            let mut sparse = HashMap::new();
            for ((from_mirror, to_mirror), cost) in matrix {
                if let (Some(from), Some(to)) = (
                    backward.get(&from_mirror).copied(),
                    backward.get(&to_mirror).copied(),
                ) {
                    sparse.insert((from, to), cost);
                }
            }
            for node in graph.node_indices() {
                sparse.entry((node, node)).or_insert(0.0);
            }
            let _ = reverse;
            Ok(sparse)
        }
        Err(_) => Err(negative_member(graph)),
    }
}

/// Cost of the k-th shortest walk to each node from `start`.
///
/// Runs on the weight mirror so removed indices cannot break the dense
/// counter the upstream search keeps. Nodes visited fewer than `k` times are
/// absent. A zero `k` or missing source yields an empty map.
pub fn kth_shortest_costs(
    graph: &Graph,
    start: NodeIndex,
    k: usize,
) -> HashMap<NodeIndex, f32> {
    if k == 0 || graph.node_weight(start).is_none() {
        return HashMap::new();
    }
    let (mirror, forward, backward) = mirror_for_negative(graph);
    let Some(mirror_start) = forward.get(&start).copied() else {
        return HashMap::new();
    };
    k_shortest_path(&mirror, mirror_start, None, k, |edge| *edge.weight())
        .into_iter()
        .filter_map(|(mirror_node, cost)| backward.get(&mirror_node).copied().map(|node| (node, cost)))
        .collect()
}

/// Cheapest node sequence from `start` to `goal`, with its total cost.
///
/// Runs A* with a zero heuristic, which behaves like Dijkstra while also
/// tracking predecessors. Returns `None` when `goal` is unreachable.
pub fn shortest_path(
    graph: &Graph,
    start: NodeIndex,
    goal: NodeIndex,
) -> Option<(f32, Vec<NodeIndex>)> {
    astar(
        graph,
        start,
        |node| node == goal,
        |edge| edge.weight().weight,
        |_| 0.0,
    )
}

/// Cheapest node sequence guided by straight-line distance to `goal`.
///
/// The heuristic reads `positions` and falls back to zero for nodes missing
/// coordinates, so callers may pass a partial map. Returns `None` when `goal`
/// is unreachable.
pub fn heuristic_shortest_path(
    graph: &Graph,
    positions: &Positions,
    start: NodeIndex,
    goal: NodeIndex,
) -> Option<(f32, Vec<NodeIndex>)> {
    let target = positions.get(&goal).copied().unwrap_or_default();
    astar(
        graph,
        start,
        |node| node == goal,
        |edge| edge.weight().weight,
        |node| {
            positions
                .get(&node)
                .map(|position| {
                    let delta = target - *position;
                    delta.length()
                })
                .unwrap_or(0.0)
        },
    )
}

/// All-pairs shortest-path distances, keyed by endpoint pair.
///
/// Only reachable pairs are present; unreachable pairs are absent rather than
/// carrying a large sentinel, matching [`shortest_paths`]. The diagonal holds
/// a zero entry per node. Returns a member of a negative cycle when one
/// exists, matching the topological-order error style.
///
/// Runs Floyd-Warshall over a dense ordinal space instead of delegating to
/// petgraph: the store owns a `StableGraph`, which cannot satisfy the
/// compact-index bound petgraph's own all-pairs search requires, so the
/// triple loop here reads edge weights directly and keeps unreachable pairs
/// absent by construction.
pub fn all_pairs_shortest_paths(
    graph: &Graph,
) -> Result<HashMap<(NodeIndex, NodeIndex), f32>, NodeIndex> {
    let mut order: Vec<NodeIndex> = graph.node_indices().collect();
    order.sort_unstable_by_key(|node| node.index());
    let size = order.len();
    let position: HashMap<NodeIndex, usize> = order
        .iter()
        .enumerate()
        .map(|(ordinal, node)| (*node, ordinal))
        .collect();
    let mut dist = vec![vec![f32::INFINITY; size]; size];
    for (diagonal, row) in dist.iter_mut().enumerate() {
        row[diagonal] = 0.0;
    }
    for reference in graph.edge_references() {
        if let (Some(&row), Some(&column)) = (
            position.get(&reference.source()),
            position.get(&reference.target()),
        ) {
            let weight = reference.weight().weight;
            if weight < dist[row][column] {
                dist[row][column] = weight;
            }
        }
    }
    for middle in 0..size {
        for row in 0..size {
            if !dist[row][middle].is_finite() {
                continue;
            }
            for column in 0..size {
                if !dist[middle][column].is_finite() {
                    continue;
                }
                let via = dist[row][middle] + dist[middle][column];
                if via < dist[row][column] {
                    dist[row][column] = via;
                }
            }
        }
    }
    if (0..size).any(|diagonal| dist[diagonal][diagonal] < 0.0) {
        return Err(negative_member(graph));
    }
    let mut sparse = HashMap::new();
    for row in 0..size {
        for column in 0..size {
            if dist[row][column].is_finite() {
                sparse.insert((order[row], order[column]), dist[row][column]);
            }
        }
    }
    Ok(sparse)
}

/// Single-source shortest paths supporting negative edge weights.
///
/// Returns distances for nodes reachable from `source` plus the predecessor
/// of each reached node (`None` for `source` itself). Unreachable nodes are
/// absent from both maps. Returns a member of a reachable negative cycle on
/// failure. A missing source yields empty maps rather than an error.
pub fn bellman_ford_paths(graph: &Graph, source: NodeIndex) -> Result<DistanceMaps, NodeIndex> {
    if graph.node_weight(source).is_none() {
        return Ok((HashMap::new(), HashMap::new()));
    }
    let (mirror, forward, backward) = mirror_for_negative(graph);
    let mirror_source = forward.get(&source).copied().unwrap_or(NodeIndex::new(0));
    match bellman_ford(&mirror, mirror_source) {
        Ok(paths) => {
            let mut distances = HashMap::new();
            let mut predecessors = HashMap::new();
            for node in graph.node_indices() {
                let mirror_node = match forward.get(&node) {
                    Some(mirror_node) => *mirror_node,
                    None => continue,
                };
                let slot = mirror.to_index(mirror_node);
                let reached = paths.distances.get(slot).copied().unwrap_or(f32::INFINITY);
                if reached.is_finite() {
                    distances.insert(node, reached);
                    let parent = paths
                        .predecessors
                        .get(slot)
                        .copied()
                        .flatten()
                        .and_then(|mirror_parent| backward.get(&mirror_parent).copied());
                    predecessors.insert(node, parent);
                }
            }
            Ok((distances, predecessors))
        }
        Err(_) => Err(find_negative_cycle(&mirror, mirror_source)
            .and_then(|cycle| cycle.first().copied())
            .and_then(|mirror_member| backward.get(&mirror_member).copied())
            .unwrap_or(source)),
    }
}

/// One negative cycle reachable from `source`, if any.
///
/// Returns the node sequence of the cycle without repeating the start node.
/// A missing source yields no cycle rather than an error.
pub fn negative_cycle_path(graph: &Graph, source: NodeIndex) -> Option<Vec<NodeIndex>> {
    graph.node_weight(source)?;
    let (mirror, forward, backward) = mirror_for_negative(graph);
    let mirror_source = forward.get(&source).copied()?;
    find_negative_cycle(&mirror, mirror_source).map(|cycle| {
        cycle
            .into_iter()
            .filter_map(|mirror_node| backward.get(&mirror_node).copied())
            .collect()
    })
}

/// First member of any negative cycle in the graph, for error reporting.
///
/// Every node is tried as a cycle source so cycles in any component are
/// found. Falls back to the first node, which only happens when the caller
/// already knows a cycle exists but no source reaches it.
fn negative_member(graph: &Graph) -> NodeIndex {
    let (mirror, _, backward) = mirror_for_negative(graph);
    for node in mirror.node_indices() {
        if let Some(cycle) = find_negative_cycle(&mirror, node)
            && let Some(mirror_member) = cycle.first().copied()
            && let Some(member) = backward.get(&mirror_member).copied()
        {
            return member;
        }
    }
    graph.node_indices().next().unwrap_or(NodeIndex::new(0))
}

/// Weight-only mirror of the store graph for negative-weight searches.
///
/// Bellman-Ford style searches require plain floating weights, while the
/// store carries payload structs, so this builds a temporary graph with one
/// unit node per store node and one `f32` edge per store edge. The maps
/// translate identifiers both ways.
fn mirror_for_negative(graph: &Graph) -> WeightMirror {
    let mut mirror: StableGraph<(), f32, Directed> = StableGraph::default();
    let mut forward = HashMap::new();
    let mut backward = HashMap::new();
    for node in graph.node_indices() {
        let mirror_node = mirror.add_node(());
        forward.insert(node, mirror_node);
        backward.insert(mirror_node, node);
    }
    for reference in graph.edge_references() {
        if let (Some(source), Some(target)) = (
            forward.get(&reference.source()).copied(),
            forward.get(&reference.target()).copied(),
        ) {
            mirror.add_edge(source, target, reference.weight().weight);
        }
    }
    (mirror, forward, backward)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diamond() -> (Graph, NodeIndex, NodeIndex, NodeIndex, NodeIndex) {
        let mut graph: Graph = StableGraph::default();
        let a = graph.add_node(NodeData { label: "a".into() });
        let b = graph.add_node(NodeData { label: "b".into() });
        let c = graph.add_node(NodeData { label: "c".into() });
        let d = graph.add_node(NodeData { label: "d".into() });
        graph.add_edge(a, b, EdgeData { weight: 1.0 });
        graph.add_edge(b, d, EdgeData { weight: 2.0 });
        graph.add_edge(a, c, EdgeData { weight: 4.0 });
        graph.add_edge(c, d, EdgeData { weight: 1.0 });
        (graph, a, b, c, d)
    }

    #[test]
    fn shortest_paths_pick_the_cheaper_route() {
        let (graph, a, b, c, d) = diamond();
        let distances = shortest_paths(&graph, a);
        assert_eq!(distances.get(&a), Some(&0.0));
        assert_eq!(distances.get(&b), Some(&1.0));
        assert_eq!(distances.get(&c), Some(&4.0));
        assert_eq!(distances.get(&d), Some(&3.0));
        assert_eq!(shortest_path_cost(&graph, a, d), Some(3.0));
    }

    #[test]
    fn unreachable_nodes_are_absent_from_the_distance_map() {
        let mut graph: Graph = StableGraph::default();
        let a = graph.add_node(NodeData { label: "a".into() });
        let lonely = graph.add_node(NodeData {
            label: "lonely".into(),
        });
        let distances = shortest_paths(&graph, a);
        assert!(!distances.contains_key(&lonely));
        assert_eq!(shortest_path_cost(&graph, a, lonely), None);
    }

    #[test]
    fn shortest_path_returns_the_cheaper_node_sequence() {
        let (graph, a, b, _, d) = diamond();
        let (cost, path) = shortest_path(&graph, a, d).expect("a path exists");
        assert_eq!(cost, 3.0);
        assert_eq!(path, vec![a, b, d]);
    }

    #[test]
    fn shortest_path_reports_unreachable_goals() {
        let mut graph: Graph = StableGraph::default();
        let a = graph.add_node(NodeData { label: "a".into() });
        let lonely = graph.add_node(NodeData {
            label: "lonely".into(),
        });
        assert!(shortest_path(&graph, a, lonely).is_none());
        assert!(shortest_path(&graph, a, NodeIndex::new(99)).is_none());
    }

    #[test]
    fn heuristic_path_matches_the_optimal_route() {
        use std::collections::HashMap;

        use cg_types::Point2;

        let (graph, a, _, _, d) = diamond();
        let positions: Positions = [(a, Point2::new(0.0, 0.0)), (d, Point2::new(3.0, 0.0))]
            .into_iter()
            .collect();
        let fallback: HashMap<NodeIndex, Point2> = HashMap::new();
        let guided = heuristic_shortest_path(&graph, &positions, a, d).expect("a path exists");
        let unguided = heuristic_shortest_path(&graph, &fallback, a, d).expect("a path exists");
        assert_eq!(guided.0, 3.0);
        assert_eq!(guided.1, unguided.1);
        assert!(heuristic_shortest_path(&graph, &positions, a, NodeIndex::new(99)).is_none());
    }

    #[test]
    fn all_pairs_covers_diagonal_and_skips_unreachable() {
        let (graph, a, _, _, d) = diamond();
        let matrix = all_pairs_shortest_paths(&graph).expect("the diamond has no cycle");
        assert_eq!(matrix.get(&(a, a)), Some(&0.0));
        assert_eq!(matrix.get(&(a, d)), Some(&3.0));
        assert!(!matrix.contains_key(&(d, a)));
    }

    #[test]
    fn all_pairs_stays_symmetric_on_bidirectional_pairs() {
        let mut graph: Graph = StableGraph::default();
        let a = graph.add_node(NodeData { label: "a".into() });
        let b = graph.add_node(NodeData { label: "b".into() });
        graph.add_edge(a, b, EdgeData { weight: 2.0 });
        graph.add_edge(b, a, EdgeData { weight: 2.0 });
        let matrix = all_pairs_shortest_paths(&graph).expect("no negative cycle");
        assert_eq!(matrix.get(&(a, b)), matrix.get(&(b, a)));
        assert!(
            all_pairs_shortest_paths(&StableGraph::default())
                .expect("empty")
                .is_empty()
        );
    }

    #[test]
    fn all_pairs_reports_a_negative_cycle_member() {
        let mut graph: Graph = StableGraph::default();
        let a = graph.add_node(NodeData { label: "a".into() });
        let b = graph.add_node(NodeData { label: "b".into() });
        graph.add_edge(a, b, EdgeData { weight: 1.0 });
        graph.add_edge(b, a, EdgeData { weight: -3.0 });
        let member = all_pairs_shortest_paths(&graph).expect_err("a negative cycle exists");
        assert!(member == a || member == b);
    }

    #[test]
    fn bellman_ford_supports_negative_weights() {
        let mut graph: Graph = StableGraph::default();
        let a = graph.add_node(NodeData { label: "a".into() });
        let b = graph.add_node(NodeData { label: "b".into() });
        let c = graph.add_node(NodeData { label: "c".into() });
        graph.add_edge(a, b, EdgeData { weight: -2.0 });
        graph.add_edge(b, c, EdgeData { weight: 1.0 });
        let (distances, predecessors) = bellman_ford_paths(&graph, a).expect("no cycle");
        assert_eq!(distances.get(&a), Some(&0.0));
        assert_eq!(distances.get(&b), Some(&-2.0));
        assert_eq!(distances.get(&c), Some(&-1.0));
        assert_eq!(predecessors.get(&a), Some(&None));
        assert_eq!(predecessors.get(&c), Some(&Some(b)));
        let missing = bellman_ford_paths(&graph, NodeIndex::new(99)).expect("missing source");
        assert!(missing.0.is_empty() && missing.1.is_empty());
    }

    #[test]
    fn bellman_ford_reports_negative_cycles_with_a_path() {
        let mut graph: Graph = StableGraph::default();
        let a = graph.add_node(NodeData { label: "a".into() });
        let b = graph.add_node(NodeData { label: "b".into() });
        graph.add_edge(a, b, EdgeData { weight: 1.0 });
        graph.add_edge(b, a, EdgeData { weight: -3.0 });
        let member = bellman_ford_paths(&graph, a).expect_err("a negative cycle exists");
        assert!(member == a || member == b);
        let cycle = negative_cycle_path(&graph, a).expect("a cycle is reachable");
        assert!(cycle.contains(&a) && cycle.contains(&b));
        assert!(negative_cycle_path(&graph, NodeIndex::new(99)).is_none());
        let (acyclic, start, _, _, _) = diamond();
        assert!(negative_cycle_path(&acyclic, start).is_none());
    }

    #[test]
    fn bidirectional_matches_dijkstra_and_rejects_missing() {
        let (graph, a, _, _, d) = diamond();
        assert_eq!(bidirectional_path_cost(&graph, a, d), Some(3.0));
        assert_eq!(bidirectional_path_cost(&graph, d, a), None);
        assert_eq!(
            bidirectional_path_cost(&graph, a, NodeIndex::new(99)),
            None
        );
    }

    #[test]
    fn spfa_matches_bellman_ford_on_negative_weights() {
        let mut graph: Graph = StableGraph::default();
        let a = graph.add_node(NodeData { label: "a".into() });
        let b = graph.add_node(NodeData { label: "b".into() });
        let c = graph.add_node(NodeData { label: "c".into() });
        graph.add_edge(a, b, EdgeData { weight: -2.0 });
        graph.add_edge(b, c, EdgeData { weight: 1.0 });
        let (distances, _) = spfa_paths(&graph, a).expect("no cycle");
        assert_eq!(distances.get(&c), Some(&-1.0));
        assert!(spfa_paths(&graph, NodeIndex::new(99)).expect("missing").0.is_empty());
    }

    #[test]
    fn johnson_matches_all_pairs_on_the_diamond() {
        let (graph, a, _, _, d) = diamond();
        let matrix = johnson_paths(&graph).expect("the diamond has no cycle");
        assert_eq!(matrix.get(&(a, d)), Some(&3.0));
        assert_eq!(matrix.get(&(a, a)), Some(&0.0));
        assert!(johnson_paths(&StableGraph::default()).expect("empty").is_empty());
    }

    #[test]
    fn kth_shortest_counts_revisits() {
        let (graph, a, _, _, d) = diamond();
        let first = kth_shortest_costs(&graph, a, 1);
        assert_eq!(first.get(&d), Some(&3.0));
        assert!(kth_shortest_costs(&graph, a, 0).is_empty());
        assert!(kth_shortest_costs(&graph, NodeIndex::new(99), 2).is_empty());
    }
}
