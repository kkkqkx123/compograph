//! Ordering relations over directed graphs.

use std::collections::{HashMap, HashSet};

use petgraph::Directed;
use petgraph::algo::{dominators::simple_fast, tarjan_scc, toposort};
use petgraph::stable_graph::{EdgeIndex, NodeIndex, StableGraph};
use petgraph::visit::{EdgeRef, IntoEdgeReferences};

use crate::store::{EdgeData, NodeData};

type Graph = StableGraph<NodeData, EdgeData, Directed>;

/// Strongly connected components, each as a list of node indices.
///
/// Components are returned in reverse topological order, matching petgraph.
pub fn strongly_connected_components(graph: &Graph) -> Vec<Vec<NodeIndex>> {
    tarjan_scc(graph)
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
    fn topological_order_respects_every_edge() {
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
