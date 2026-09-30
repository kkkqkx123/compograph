//! Thin wrappers over petgraph algorithms.
//!
//! The wrappers keep call sites free of petgraph generics: each function takes
//! the graph the store exposes and returns plain, owned Rust data. They stay
//! free of gpui types so the algorithms can be exercised in headless tests.

use std::collections::{HashMap, HashSet};

use petgraph::Directed;
use petgraph::algo::{
    astar, bellman_ford, dijkstra, dominators::simple_fast, find_negative_cycle, min_spanning_tree,
    min_spanning_tree_prim, page_rank, tarjan_scc, toposort,
};
use petgraph::data::Element;
use petgraph::stable_graph::{EdgeIndex, NodeIndex, StableGraph};
use petgraph::visit::{Bfs, Dfs, EdgeRef, IntoEdgeReferences, NodeIndexable};

use crate::positions::Positions;
use crate::store::{EdgeData, NodeData};

/// Directed graph type the store owns, named here to keep wrapper signatures
/// readable.
type Graph = StableGraph<NodeData, EdgeData, Directed>;

/// Weight-only mirror of the store graph with identifier maps both ways.
type WeightMirror = (
    StableGraph<(), f32, Directed>,
    HashMap<NodeIndex, NodeIndex>,
    HashMap<NodeIndex, NodeIndex>,
);

/// Undirected adjacency with per-edge identities for cut searches.
type UndirectedView = (Vec<NodeIndex>, HashMap<NodeIndex, Vec<(NodeIndex, usize)>>);

/// Distances plus predecessors of one single-source search.
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

/// Strongly connected components, each as a list of node indices.
///
/// Components are returned in reverse topological order, matching petgraph.
pub fn strongly_connected_components(graph: &Graph) -> Vec<Vec<NodeIndex>> {
    tarjan_scc(graph)
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

/// Minimum spanning forest edges via Kruskal's algorithm.
///
/// Each entry carries its endpoints and weight. Disconnected components each
/// contribute their own tree, so a forest with `c` components holds
/// `nodes - c` edges. The graph is treated as undirected.
pub fn minimum_spanning_forest(graph: &Graph) -> Vec<(NodeIndex, NodeIndex, f32)> {
    let order: Vec<NodeIndex> = graph.node_indices().collect();
    min_spanning_tree(graph)
        .filter_map(|element| match element {
            Element::Edge {
                source,
                target,
                weight,
            } => {
                let endpoints = order.get(source).zip(order.get(target));
                endpoints.map(|(source, target)| (*source, *target, weight.weight))
            }
            Element::Node { .. } => None,
        })
        .collect()
}

/// Minimum spanning tree edges of one component via Prim's algorithm.
///
/// Only the component holding the first node is covered; remaining components
/// contribute no edges. Prefer [`minimum_spanning_forest`] when every
/// component must be represented. The graph is treated as undirected.
pub fn minimum_spanning_tree_single(graph: &Graph) -> Vec<(NodeIndex, NodeIndex, f32)> {
    let order: Vec<NodeIndex> = graph.node_indices().collect();
    min_spanning_tree_prim(graph)
        .filter_map(|element| match element {
            Element::Edge {
                source,
                target,
                weight,
            } => {
                let endpoints = order.get(source).zip(order.get(target));
                endpoints.map(|(source, target)| (*source, *target, weight.weight))
            }
            Element::Node { .. } => None,
        })
        .collect()
}

/// Nodes in topological order, or the first node closing a cycle.
///
/// Returns an empty vector for an empty graph. Self loops count as cycles.
pub fn topological_order(graph: &Graph) -> Result<Vec<NodeIndex>, NodeIndex> {
    toposort(graph, None).map_err(|cycle| cycle.node_id())
}

/// Immediate dominator of every node reachable from `root`, keyed by node.
///
/// The root itself is absent from the map, as are nodes unreachable from it.
pub fn immediate_dominators(graph: &Graph, root: NodeIndex) -> HashMap<NodeIndex, NodeIndex> {
    let relations = simple_fast(graph, root);
    graph
        .node_indices()
        .filter(|node| *node != root)
        .filter_map(|node| {
            relations
                .immediate_dominator(node)
                .map(|parent| (node, parent))
        })
        .collect()
}

/// PageRank scores, parallel to the node order of [`Graph::node_indices`].
///
/// Returns an empty vector for an empty graph. `damping` must lie in `[0, 1]`.
pub fn rank_nodes(graph: &Graph, damping: f32, iterations: usize) -> Vec<f32> {
    page_rank(graph, damping, iterations)
}

/// Edges of the transitive reduction, or the first node closing a cycle.
///
/// An edge is dropped when its target stays reachable from its source without
/// it, so the kept set is minimal while preserving reachability. Only defined
/// for acyclic graphs; cyclic input reports a member of a cycle, matching
/// [`topological_order`]. Results arrive sorted by endpoint indices.
pub fn transitive_reduction(graph: &Graph) -> Result<Vec<(NodeIndex, NodeIndex)>, NodeIndex> {
    toposort(graph, None).map_err(|cycle| cycle.node_id())?;
    let mut kept = Vec::new();
    for reference in graph.edge_references() {
        if !reachable_skipping(
            graph,
            reference.source(),
            reference.target(),
            reference.id(),
        ) {
            kept.push((reference.source(), reference.target()));
        }
    }
    kept.sort_unstable_by_key(|(source, target)| (source.index(), target.index()));
    Ok(kept)
}

/// All-pairs shortest-path distances, keyed by endpoint pair.
///
/// Only reachable pairs are present; unreachable pairs are absent rather than
/// carrying a large sentinel, matching [`shortest_paths`]. The diagonal holds
/// a zero entry per node. Returns a member of a negative cycle when one
/// exists, matching the [`topological_order`] error style.
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
/// failure, matching the [`topological_order`] error style. A missing source
/// yields empty maps rather than an error.
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

/// Articulation points under undirected connectivity.
///
/// The directed store is read as an undirected graph: each directed edge
/// links its endpoints both ways and self loops are ignored. Results arrive
/// sorted by node index. An empty graph yields no points.
pub fn articulation_points(graph: &Graph) -> Vec<NodeIndex> {
    let (order, adjacency) = undirected_adjacency(graph);
    let mut search = ArticulationSearch {
        adjacency: &adjacency,
        discovery: HashMap::new(),
        low: HashMap::new(),
        clock: 0,
        points: HashSet::new(),
    };
    for root in &order {
        if search.discovery.contains_key(root) {
            continue;
        }
        search.visit_root(*root);
    }
    let mut result: Vec<NodeIndex> = search.points.into_iter().collect();
    result.sort_unstable_by_key(|node| node.index());
    result
}

/// Bridges under undirected connectivity.
///
/// The directed store is read as an undirected graph; parallel edges between
/// one pair keep each other from becoming bridges, and self loops never are.
/// Each bridge is reported with the smaller endpoint first, and results
/// arrive sorted. An empty graph yields no bridges.
pub fn bridges(graph: &Graph) -> Vec<(NodeIndex, NodeIndex)> {
    let (order, adjacency) = undirected_adjacency(graph);
    let mut search = BridgeSearch {
        adjacency: &adjacency,
        discovery: HashMap::new(),
        low: HashMap::new(),
        clock: 0,
        found: Vec::new(),
    };
    for root in &order {
        if search.discovery.contains_key(root) {
            continue;
        }
        search.visit(*root, None);
    }
    search
        .found
        .sort_unstable_by_key(|(source, target)| (source.index(), target.index()));
    search.found
}

/// Nodes in breadth-first order from `start` along outgoing edges.
///
/// A missing source yields an empty order rather than an error.
pub fn breadth_first_order(graph: &Graph, start: NodeIndex) -> Vec<NodeIndex> {
    if graph.node_weight(start).is_none() {
        return Vec::new();
    }
    let mut search = Bfs::new(graph, start);
    let mut order = Vec::new();
    while let Some(node) = search.next(graph) {
        order.push(node);
    }
    order
}

/// Nodes in depth-first order from `start` along outgoing edges.
///
/// A missing source yields an empty order rather than an error.
pub fn depth_first_order(graph: &Graph, start: NodeIndex) -> Vec<NodeIndex> {
    if graph.node_weight(start).is_none() {
        return Vec::new();
    }
    let mut search = Dfs::new(graph, start);
    let mut order = Vec::new();
    while let Some(node) = search.next(graph) {
        order.push(node);
    }
    order
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

/// Sorted node order plus undirected adjacency with per-edge identities.
///
/// Parallel edges keep distinct identities so they never report as bridges;
/// self loops are skipped because they affect neither articulation nor
/// bridges.
fn undirected_adjacency(graph: &Graph) -> UndirectedView {
    let mut order: Vec<NodeIndex> = graph.node_indices().collect();
    order.sort_unstable_by_key(|node| node.index());
    let mut adjacency: HashMap<NodeIndex, Vec<(NodeIndex, usize)>> = HashMap::new();
    for node in &order {
        adjacency.insert(*node, Vec::new());
    }
    for (ordinal, reference) in graph.edge_references().enumerate() {
        let source = reference.source();
        let target = reference.target();
        if source == target {
            continue;
        }
        if let Some(neighbours) = adjacency.get_mut(&source) {
            neighbours.push((target, ordinal));
        }
        if let Some(neighbours) = adjacency.get_mut(&target) {
            neighbours.push((source, ordinal));
        }
    }
    for neighbours in adjacency.values_mut() {
        neighbours.sort_unstable_by_key(|(node, ordinal)| (node.index(), *ordinal));
    }
    (order, adjacency)
}

/// Tarjan lowlink search collecting articulation points.
///
/// The undirected adjacency carries the whole topology, so the store graph
/// itself is consumed during adjacency construction and plays no further
/// role here. Discovery and lowlink state lives on the search, leaving the
/// recursive visit with only the node and its arrival edge.
struct ArticulationSearch<'a> {
    adjacency: &'a HashMap<NodeIndex, Vec<(NodeIndex, usize)>>,
    discovery: HashMap<NodeIndex, usize>,
    low: HashMap<NodeIndex, usize>,
    clock: usize,
    points: HashSet<NodeIndex>,
}

impl ArticulationSearch<'_> {
    fn visit_root(&mut self, root: NodeIndex) {
        self.discovery.insert(root, self.clock);
        self.low.insert(root, self.clock);
        self.clock += 1;
        let mut children = 0usize;
        for (next, ordinal) in incident_neighbors(self.adjacency, root, None) {
            if self.discovery.contains_key(&next) {
                let current = self.low.get(&root).copied().unwrap_or(usize::MAX);
                let known = self.discovery.get(&next).copied().unwrap_or(usize::MAX);
                self.low.insert(root, current.min(known));
            } else {
                children += 1;
                self.visit(next, Some(ordinal));
                let child_low = self.low.get(&next).copied().unwrap_or(usize::MAX);
                let current = self.low.get(&root).copied().unwrap_or(usize::MAX);
                self.low.insert(root, current.min(child_low));
            }
        }
        if children > 1 {
            self.points.insert(root);
        }
    }

    fn visit(&mut self, node: NodeIndex, parent_edge: Option<usize>) {
        self.discovery.insert(node, self.clock);
        self.low.insert(node, self.clock);
        self.clock += 1;
        for (next, ordinal) in incident_neighbors(self.adjacency, node, parent_edge) {
            if let Some(known) = self.discovery.get(&next).copied() {
                let current = self.low.get(&node).copied().unwrap_or(usize::MAX);
                self.low.insert(node, current.min(known));
            } else {
                self.visit(next, Some(ordinal));
                let child_low = self.low.get(&next).copied().unwrap_or(usize::MAX);
                let current = self.low.get(&node).copied().unwrap_or(usize::MAX);
                self.low.insert(node, current.min(child_low));
                let entered = self.discovery.get(&node).copied().unwrap_or(0);
                if child_low >= entered {
                    self.points.insert(node);
                }
            }
        }
    }
}

/// Tarjan lowlink search collecting bridges.
///
/// Shares the adjacency-driven structure of [`ArticulationSearch`]: the
/// store graph is consumed during adjacency construction, and the recursive
/// visit carries only the node and its arrival edge. Parallel edges keep
/// distinct identities, so the second edge to the parent reads as a back
/// edge rather than a bridge.
struct BridgeSearch<'a> {
    adjacency: &'a HashMap<NodeIndex, Vec<(NodeIndex, usize)>>,
    discovery: HashMap<NodeIndex, usize>,
    low: HashMap<NodeIndex, usize>,
    clock: usize,
    found: Vec<(NodeIndex, NodeIndex)>,
}

impl BridgeSearch<'_> {
    fn visit(&mut self, node: NodeIndex, parent_edge: Option<usize>) {
        self.discovery.insert(node, self.clock);
        self.low.insert(node, self.clock);
        self.clock += 1;
        for (next, ordinal) in incident_neighbors(self.adjacency, node, parent_edge) {
            if let Some(known) = self.discovery.get(&next).copied() {
                let current = self.low.get(&node).copied().unwrap_or(usize::MAX);
                self.low.insert(node, current.min(known));
            } else {
                self.visit(next, Some(ordinal));
                let child_low = self.low.get(&next).copied().unwrap_or(usize::MAX);
                let current = self.low.get(&node).copied().unwrap_or(usize::MAX);
                self.low.insert(node, current.min(child_low));
                let entered = self.discovery.get(&node).copied().unwrap_or(0);
                if child_low > entered {
                    if node.index() <= next.index() {
                        self.found.push((node, next));
                    } else {
                        self.found.push((next, node));
                    }
                }
            }
        }
    }
}

/// Incident undirected edges of `node`, excluding the arrival edge.
///
/// Edge identities distinguish a parallel edge back to the parent from the
/// arrival edge itself, so parallel pairs never report as cuts.
fn incident_neighbors(
    adjacency: &HashMap<NodeIndex, Vec<(NodeIndex, usize)>>,
    node: NodeIndex,
    parent_edge: Option<usize>,
) -> Vec<(NodeIndex, usize)> {
    adjacency
        .get(&node)
        .map(|list| {
            list.iter()
                .filter(|(_, ordinal)| Some(*ordinal) != parent_edge)
                .copied()
                .collect()
        })
        .unwrap_or_default()
}

/// True when `goal` is reachable from `start` without traversing `skip`.
fn reachable_skipping(graph: &Graph, start: NodeIndex, goal: NodeIndex, skip: EdgeIndex) -> bool {
    let mut visited: HashSet<NodeIndex> = HashSet::from([start]);
    let mut stack = vec![start];
    while let Some(node) = stack.pop() {
        for outgoing in graph.edges(node) {
            if outgoing.id() == skip {
                continue;
            }
            let next = outgoing.target();
            if next == goal {
                return true;
            }
            if visited.insert(next) {
                stack.push(next);
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds the diamond used across these tests: a -> b -> d, a -> c -> d,
    /// with the two paths costing 3 and 5 respectively.
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
    fn scc_groups_a_cycle_and_leaves_singletons() {
        let mut graph: Graph = StableGraph::default();
        let a = graph.add_node(NodeData { label: "a".into() });
        let b = graph.add_node(NodeData { label: "b".into() });
        let c = graph.add_node(NodeData { label: "c".into() });
        graph.add_edge(a, b, EdgeData { weight: 1.0 });
        graph.add_edge(b, a, EdgeData { weight: 1.0 });
        graph.add_edge(b, c, EdgeData { weight: 1.0 });

        let components = strongly_connected_components(&graph);
        let cycle = components
            .iter()
            .find(|component| component.len() == 2)
            .expect("a two-node cycle exists");
        assert!(cycle.contains(&a) && cycle.contains(&b));
        assert!(components.iter().any(|component| component == &vec![c]));
    }

    #[test]
    fn scc_of_an_empty_graph_is_empty() {
        let graph: Graph = StableGraph::default();
        assert!(strongly_connected_components(&graph).is_empty());
    }

    #[test]
    fn rank_nodes_returns_one_score_per_node() {
        let (graph, _, _, _, _) = diamond();
        let ranks = rank_nodes(&graph, 0.85, 20);
        assert_eq!(ranks.len(), graph.node_count());
        assert!(ranks.iter().all(|rank| rank.is_finite() && *rank >= 0.0));
    }

    #[test]
    fn ranking_an_empty_graph_yields_no_scores() {
        let graph: Graph = StableGraph::default();
        assert!(rank_nodes(&graph, 0.85, 10).is_empty());
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
    fn kruskal_forest_spans_every_component() {
        let (mut graph, _, _, _, _) = diamond();
        let lonely = graph.add_node(NodeData {
            label: "lonely".into(),
        });
        let forest = minimum_spanning_forest(&graph);
        assert_eq!(forest.len(), graph.node_count() - 2);
        assert!(
            !forest
                .iter()
                .any(|(source, target, _)| { *source == lonely || *target == lonely })
        );
        let weights: f32 = forest.iter().map(|(_, _, weight)| weight).sum();
        assert!((weights - 4.0).abs() < 1e-5);
    }

    #[test]
    fn prim_tree_covers_a_single_component() {
        let (graph, _, _, _, _) = diamond();
        let tree = minimum_spanning_tree_single(&graph);
        assert_eq!(tree.len(), graph.node_count() - 1);
        let empty: Graph = StableGraph::default();
        assert!(minimum_spanning_forest(&empty).is_empty());
        assert!(minimum_spanning_tree_single(&empty).is_empty());
    }

    #[test]
    fn topological_order_respects_every_edge() {
        use petgraph::visit::EdgeRef;
        use petgraph::visit::IntoEdgeReferences;

        let (graph, _, _, _, _) = diamond();
        let order = topological_order(&graph).expect("the diamond is acyclic");
        assert_eq!(order.len(), graph.node_count());
        let rank: HashMap<NodeIndex, usize> = order
            .iter()
            .enumerate()
            .map(|(position, node)| (*node, position))
            .collect();
        for edge in graph.edge_references() {
            assert!(rank[&edge.source()] < rank[&edge.target()]);
        }
        assert!(
            topological_order(&StableGraph::default())
                .expect("empty")
                .is_empty()
        );
    }

    #[test]
    fn topological_order_reports_a_cycle_member() {
        let mut graph: Graph = StableGraph::default();
        let a = graph.add_node(NodeData { label: "a".into() });
        let b = graph.add_node(NodeData { label: "b".into() });
        graph.add_edge(a, b, EdgeData { weight: 1.0 });
        graph.add_edge(b, a, EdgeData { weight: 1.0 });
        let member = topological_order(&graph).expect_err("a cycle exists");
        assert!(member == a || member == b);
    }

    #[test]
    fn dominators_follow_a_chain_and_skip_the_unreachable() {
        let mut graph: Graph = StableGraph::default();
        let a = graph.add_node(NodeData { label: "a".into() });
        let b = graph.add_node(NodeData { label: "b".into() });
        let c = graph.add_node(NodeData { label: "c".into() });
        let lonely = graph.add_node(NodeData {
            label: "lonely".into(),
        });
        graph.add_edge(a, b, EdgeData { weight: 1.0 });
        graph.add_edge(b, c, EdgeData { weight: 1.0 });
        let parents = immediate_dominators(&graph, a);
        assert_eq!(parents.get(&b), Some(&a));
        assert_eq!(parents.get(&c), Some(&b));
        assert!(!parents.contains_key(&a));
        assert!(!parents.contains_key(&lonely));
    }

    #[test]
    fn transitive_reduction_drops_a_shortcut_edge() {
        let (mut graph, a, b, c, d) = diamond();
        graph.add_edge(a, d, EdgeData { weight: 10.0 });
        let reduced = transitive_reduction(&graph).expect("the diamond stays acyclic");
        assert_eq!(reduced.len(), 4);
        assert!(!reduced.contains(&(a, d)));
        assert!(reduced.contains(&(a, b)));
        assert!(reduced.contains(&(b, d)));
        assert!(reduced.contains(&(a, c)));
        assert!(reduced.contains(&(c, d)));
    }

    #[test]
    fn transitive_reduction_reports_cycles_and_empty_graphs() {
        let mut graph: Graph = StableGraph::default();
        let a = graph.add_node(NodeData { label: "a".into() });
        let b = graph.add_node(NodeData { label: "b".into() });
        graph.add_edge(a, b, EdgeData { weight: 1.0 });
        graph.add_edge(b, a, EdgeData { weight: 1.0 });
        let member = transitive_reduction(&graph).expect_err("a cycle exists");
        assert!(member == a || member == b);
        assert!(
            transitive_reduction(&StableGraph::default())
                .expect("empty")
                .is_empty()
        );
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

    /// Builds a directed chain a -> b -> c for cut searches, which read the
    /// store as undirected.
    fn chain3() -> (Graph, NodeIndex, NodeIndex, NodeIndex) {
        let mut graph: Graph = StableGraph::default();
        let a = graph.add_node(NodeData { label: "a".into() });
        let b = graph.add_node(NodeData { label: "b".into() });
        let c = graph.add_node(NodeData { label: "c".into() });
        graph.add_edge(a, b, EdgeData { weight: 1.0 });
        graph.add_edge(b, c, EdgeData { weight: 1.0 });
        (graph, a, b, c)
    }

    #[test]
    fn articulation_marks_the_chain_middle_but_not_a_ring() {
        let (chain, _, middle, _) = chain3();
        assert_eq!(articulation_points(&chain), vec![middle]);
        let mut ring: Graph = StableGraph::default();
        let nodes: Vec<NodeIndex> = (0..3)
            .map(|ordinal| {
                ring.add_node(NodeData {
                    label: ordinal.to_string(),
                })
            })
            .collect();
        ring.add_edge(nodes[0], nodes[1], EdgeData { weight: 1.0 });
        ring.add_edge(nodes[1], nodes[2], EdgeData { weight: 1.0 });
        ring.add_edge(nodes[2], nodes[0], EdgeData { weight: 1.0 });
        assert!(articulation_points(&ring).is_empty());
        assert!(articulation_points(&StableGraph::default()).is_empty());
    }

    #[test]
    fn bridges_span_the_chain_but_skip_rings_and_parallel_pairs() {
        let (chain, a, b, c) = chain3();
        let mut ordered = bridges(&chain);
        ordered.sort_unstable_by_key(|(source, target)| (source.index(), target.index()));
        assert_eq!(ordered, vec![(a, b), (b, c)]);
        let mut ring: Graph = StableGraph::default();
        let nodes: Vec<NodeIndex> = (0..3)
            .map(|ordinal| {
                ring.add_node(NodeData {
                    label: ordinal.to_string(),
                })
            })
            .collect();
        ring.add_edge(nodes[0], nodes[1], EdgeData { weight: 1.0 });
        ring.add_edge(nodes[1], nodes[2], EdgeData { weight: 1.0 });
        ring.add_edge(nodes[2], nodes[0], EdgeData { weight: 1.0 });
        assert!(bridges(&ring).is_empty());
        let mut parallel: Graph = StableGraph::default();
        let x = parallel.add_node(NodeData { label: "x".into() });
        let y = parallel.add_node(NodeData { label: "y".into() });
        parallel.add_edge(x, y, EdgeData { weight: 1.0 });
        parallel.add_edge(x, y, EdgeData { weight: 2.0 });
        assert!(bridges(&parallel).is_empty());
        assert!(bridges(&StableGraph::default()).is_empty());
    }

    #[test]
    fn traversals_start_at_the_source_and_cover_reachable_nodes() {
        let (graph, a, _, _, d) = diamond();
        let breadth = breadth_first_order(&graph, a);
        assert_eq!(breadth.first(), Some(&a));
        assert_eq!(breadth.len(), graph.node_count());
        assert!(breadth.contains(&d));
        let depth = depth_first_order(&graph, a);
        assert_eq!(depth.first(), Some(&a));
        assert_eq!(depth.len(), graph.node_count());
        assert!(breadth_first_order(&graph, NodeIndex::new(99)).is_empty());
        assert!(depth_first_order(&graph, NodeIndex::new(99)).is_empty());
    }
}
