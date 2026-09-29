//! Thin wrappers over petgraph algorithms.
//!
//! The wrappers keep call sites free of petgraph generics: each function takes
//! the graph the store exposes and returns plain, owned Rust data. They stay
//! free of gpui types so the algorithms can be exercised in headless tests.

use std::collections::{HashMap, HashSet};

use petgraph::Directed;
use petgraph::algo::{
    astar, dijkstra, dominators::simple_fast, min_spanning_tree, min_spanning_tree_prim, page_rank,
    tarjan_scc, toposort,
};
use petgraph::data::Element;
use petgraph::stable_graph::{EdgeIndex, NodeIndex, StableGraph};
use petgraph::visit::{EdgeRef, IntoEdgeReferences};

use crate::positions::Positions;
use crate::store::{EdgeData, NodeData};

/// Directed graph type the store owns, named here to keep wrapper signatures
/// readable.
type Graph = StableGraph<NodeData, EdgeData, Directed>;

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
}
