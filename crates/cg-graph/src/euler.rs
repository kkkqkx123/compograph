//! Eulerian trails over the store graph.
//!
//! Both functions are self researched: petgraph offers no Eulerian trail
//! search, so Hierholzer traversal is implemented here against the concrete
//! store graph. The concrete type is required because parallel edges share
//! one endpoint pair and only edge identities keep them distinct; the
//! read-only view collapses them and cannot drive a trail covering every
//! edge exactly once.
//!
//! Directed semantics follow outgoing edges; undirected semantics read the
//! directed store as an undirected graph where each directed edge links its
//! endpoints both ways and self loops are single traversable edges. Results
//! are node sequences using every edge exactly once, so they feed the path
//! highlight outcome directly. Failures report the first relevant node in
//! index order, matching [`crate::algo::topological_order`], and never panic.
//! Empty graphs and graphs without edges yield an empty path.

use std::collections::{HashMap, HashSet, VecDeque};

use petgraph::Directed;
use petgraph::stable_graph::{NodeIndex, StableGraph};
use petgraph::visit::{EdgeRef, IntoEdgeReferences};

use crate::store::{EdgeData, NodeData};

/// Directed graph type the store owns, named here to keep signatures readable.
type Graph = StableGraph<NodeData, EdgeData, Directed>;

/// Directed Eulerian trail as a node sequence, or the first relevant node.
///
/// Exactly one start node may satisfy `out - inn == 1` and one end node
/// `in - out == 1`; every other active node must balance. All active nodes
/// must share one weakly connected component. Isolated nodes are ignored, so
/// an edgeless graph yields an empty path.
pub fn eulerian_path_directed(graph: &Graph) -> Result<Vec<NodeIndex>, NodeIndex> {
    let mut order: Vec<NodeIndex> = graph.node_indices().collect();
    order.sort_unstable_by_key(|node| node.index());
    if order.is_empty() {
        return Ok(Vec::new());
    }
    let mut outgoing: HashMap<NodeIndex, Vec<(NodeIndex, usize)>> = HashMap::new();
    let mut balance: HashMap<NodeIndex, i32> = HashMap::new();
    for node in &order {
        outgoing.insert(*node, Vec::new());
        balance.insert(*node, 0);
    }
    for reference in graph.edge_references() {
        let source = reference.source();
        let target = reference.target();
        if let Some(list) = outgoing.get_mut(&source) {
            list.push((target, reference.id().index()));
        }
        if let Some(entry) = balance.get_mut(&source) {
            *entry += 1;
        }
        if let Some(entry) = balance.get_mut(&target) {
            *entry -= 1;
        }
    }
    for list in outgoing.values_mut() {
        list.sort_unstable_by_key(|(target, ordinal)| (target.index(), *ordinal));
        list.reverse();
    }
    let mut imbalanced: Vec<NodeIndex> = order
        .iter()
        .copied()
        .filter(|node| balance.get(node).copied().unwrap_or(0) != 0)
        .collect();
    imbalanced.sort_unstable_by_key(|node| node.index());
    let start = match imbalanced.len() {
        0 => None,
        2 => {
            let first = imbalanced[0];
            let second = imbalanced[1];
            let first_balance = balance.get(&first).copied().unwrap_or(0);
            let second_balance = balance.get(&second).copied().unwrap_or(0);
            if first_balance == 1 && second_balance == -1 {
                Some(first)
            } else if first_balance == -1 && second_balance == 1 {
                Some(second)
            } else {
                return Err(first);
            }
        }
        _ => return Err(imbalanced.first().copied().unwrap_or(NodeIndex::new(0))),
    };
    let active: Vec<NodeIndex> = order
        .iter()
        .copied()
        .filter(|node| {
            let out = outgoing.get(node).map(Vec::len).unwrap_or(0);
            let inn = graph
                .edges_directed(*node, petgraph::Direction::Incoming)
                .count();
            out + inn > 0
        })
        .collect();
    if active.is_empty() {
        return Ok(Vec::new());
    }
    let origin = start.unwrap_or(active[0]);
    if let Some(outsider) = first_outside_weak_component(graph, &active) {
        return Err(outsider);
    }
    let mut stack = vec![origin];
    let mut circuit = Vec::new();
    while let Some(node) = stack.last().copied() {
        let next = outgoing
            .get_mut(&node)
            .and_then(|list| list.pop().map(|(target, _)| target));
        match next {
            Some(target) => stack.push(target),
            None => {
                stack.pop();
                circuit.push(node);
            }
        }
    }
    circuit.reverse();
    Ok(circuit)
}

/// Undirected Eulerian trail reading the directed store as undirected.
///
/// Each directed edge links its endpoints both ways and every self loop is
/// one traversable edge contributing two to its degree. Zero or two odd
/// degree nodes are required, and all active nodes must share one component.
/// Isolated nodes are ignored, so an edgeless graph yields an empty path.
pub fn eulerian_path_undirected(graph: &Graph) -> Result<Vec<NodeIndex>, NodeIndex> {
    let mut order: Vec<NodeIndex> = graph.node_indices().collect();
    order.sort_unstable_by_key(|node| node.index());
    if order.is_empty() {
        return Ok(Vec::new());
    }
    let mut adjacent: HashMap<NodeIndex, Vec<(NodeIndex, usize)>> = HashMap::new();
    let mut degree: HashMap<NodeIndex, usize> = HashMap::new();
    for node in &order {
        adjacent.insert(*node, Vec::new());
        degree.insert(*node, 0);
    }
    for reference in graph.edge_references() {
        let source = reference.source();
        let target = reference.target();
        let ordinal = reference.id().index();
        if source == target {
            if let Some(list) = adjacent.get_mut(&source) {
                list.push((target, ordinal));
            }
            if let Some(entry) = degree.get_mut(&source) {
                *entry += 2;
            }
        } else {
            if let Some(list) = adjacent.get_mut(&source) {
                list.push((target, ordinal));
            }
            if let Some(list) = adjacent.get_mut(&target) {
                list.push((source, ordinal));
            }
            if let Some(entry) = degree.get_mut(&source) {
                *entry += 1;
            }
            if let Some(entry) = degree.get_mut(&target) {
                *entry += 1;
            }
        }
    }
    for list in adjacent.values_mut() {
        list.sort_unstable_by_key(|(neighbour, ordinal)| (neighbour.index(), *ordinal));
        list.reverse();
    }
    let mut odd: Vec<NodeIndex> = order
        .iter()
        .copied()
        .filter(|node| degree.get(node).copied().unwrap_or(0) % 2 == 1)
        .collect();
    odd.sort_unstable_by_key(|node| node.index());
    if !odd.is_empty() && odd.len() != 2 {
        return Err(odd.first().copied().unwrap_or(NodeIndex::new(0)));
    }
    let active: Vec<NodeIndex> = order
        .iter()
        .copied()
        .filter(|node| degree.get(node).copied().unwrap_or(0) > 0)
        .collect();
    if active.is_empty() {
        return Ok(Vec::new());
    }
    let origin = odd.first().copied().unwrap_or(active[0]);
    if let Some(outsider) = first_outside_weak_component(graph, &active) {
        return Err(outsider);
    }
    let mut used: HashSet<usize> = HashSet::new();
    let mut stack = vec![origin];
    let mut circuit = Vec::new();
    while let Some(node) = stack.last().copied() {
        let mut next: Option<NodeIndex> = None;
        while let Some((neighbour, ordinal)) = adjacent.get_mut(&node).and_then(|list| list.pop()) {
            if used.insert(ordinal) {
                next = Some(neighbour);
                break;
            }
        }
        match next {
            Some(neighbour) => stack.push(neighbour),
            None => {
                stack.pop();
                circuit.push(node);
            }
        }
    }
    circuit.reverse();
    Ok(circuit)
}

/// Smallest active node outside the weak component of the smallest active node.
///
/// Returns `None` when every active node is reachable, so callers map the
/// result directly to the connectivity error. Neighbours are read in both
/// directions and self loops are skipped because they never connect nodes.
fn first_outside_weak_component(graph: &Graph, active: &[NodeIndex]) -> Option<NodeIndex> {
    let origin = active.first().copied()?;
    let mut seen: HashSet<NodeIndex> = HashSet::from([origin]);
    let mut queue = VecDeque::from([origin]);
    while let Some(node) = queue.pop_front() {
        let mut neighbours: Vec<NodeIndex> = graph
            .edges(node)
            .map(|edge| edge.target())
            .chain(
                graph
                    .edges_directed(node, petgraph::Direction::Incoming)
                    .map(|edge| edge.source()),
            )
            .filter(|neighbour| *neighbour != node)
            .collect();
        neighbours.sort_unstable_by_key(|neighbour| neighbour.index());
        for neighbour in neighbours {
            if seen.insert(neighbour) {
                queue.push_back(neighbour);
            }
        }
    }
    let mut ordered = active.to_vec();
    ordered.sort_unstable_by_key(|node| node.index());
    ordered.into_iter().find(|node| !seen.contains(node))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labelled(label: &str) -> NodeData {
        NodeData {
            label: label.into(),
        }
    }

    fn weighted(weight: f32) -> EdgeData {
        EdgeData { weight }
    }

    fn chain3() -> (Graph, Vec<NodeIndex>) {
        let mut graph: Graph = StableGraph::default();
        let a = graph.add_node(labelled("a"));
        let b = graph.add_node(labelled("b"));
        let c = graph.add_node(labelled("c"));
        graph.add_edge(a, b, weighted(1.0));
        graph.add_edge(b, c, weighted(1.0));
        (graph, vec![a, b, c])
    }

    fn uses_every_edge_once(graph: &Graph, path: &[NodeIndex], directed: bool) -> bool {
        if graph.edge_count() == 0 {
            return path.is_empty();
        }
        if path.len() != graph.edge_count() + 1 {
            return false;
        }
        let mut remaining: Vec<(NodeIndex, NodeIndex)> = graph
            .edge_references()
            .map(|edge| (edge.source(), edge.target()))
            .collect();
        for pair in path.windows(2) {
            let (from, to) = (pair[0], pair[1]);
            let slot = if directed {
                remaining.iter().position(|(s, t)| *s == from && *t == to)
            } else {
                remaining
                    .iter()
                    .position(|(s, t)| (*s == from && *t == to) || (*s == to && *t == from))
            };
            match slot {
                Some(index) => {
                    remaining.remove(index);
                }
                None => return false,
            }
        }
        remaining.is_empty()
    }

    #[test]
    fn directed_chain_yields_the_node_order() {
        let (graph, expected) = chain3();
        let path = eulerian_path_directed(&graph).expect("a chain has a trail");
        assert_eq!(path, expected);
    }

    #[test]
    fn directed_ring_yields_a_circuit_covering_every_edge() {
        let mut graph: Graph = StableGraph::default();
        let nodes: Vec<NodeIndex> = (0..3)
            .map(|ordinal| graph.add_node(labelled(&ordinal.to_string())))
            .collect();
        graph.add_edge(nodes[0], nodes[1], weighted(1.0));
        graph.add_edge(nodes[1], nodes[2], weighted(1.0));
        graph.add_edge(nodes[2], nodes[0], weighted(1.0));
        let path = eulerian_path_directed(&graph).expect("a ring has a circuit");
        assert_eq!(path.first(), path.last());
        assert!(uses_every_edge_once(&graph, &path, true));
    }

    #[test]
    fn directed_reports_degree_violations_without_panicking() {
        let (mut graph, _) = chain3();
        let extra = graph.add_node(labelled("extra"));
        let first = graph.node_indices().next().expect("a node exists");
        graph.add_edge(first, extra, weighted(1.0));
        graph.add_edge(first, extra, weighted(1.0));
        let member = eulerian_path_directed(&graph).expect_err("degrees forbid a trail");
        assert!(graph.node_weight(member).is_some());
    }

    #[test]
    fn directed_reports_disconnected_graphs() {
        let (mut graph, _) = chain3();
        let x = graph.add_node(labelled("x"));
        let y = graph.add_node(labelled("y"));
        graph.add_edge(x, y, weighted(1.0));
        let member = eulerian_path_directed(&graph).expect_err("disconnected");
        assert!(graph.node_weight(member).is_some());
    }

    #[test]
    fn directed_parallel_pair_with_return_forms_a_circuit() {
        let mut graph: Graph = StableGraph::default();
        let x = graph.add_node(labelled("x"));
        let y = graph.add_node(labelled("y"));
        graph.add_edge(x, y, weighted(1.0));
        graph.add_edge(x, y, weighted(2.0));
        graph.add_edge(y, x, weighted(1.0));
        graph.add_edge(y, x, weighted(1.5));
        let path = eulerian_path_directed(&graph).expect("balanced parallels have a trail");
        assert!(uses_every_edge_once(&graph, &path, true));
    }

    #[test]
    fn directed_self_loop_is_traversed() {
        let mut graph: Graph = StableGraph::default();
        let node = graph.add_node(labelled("only"));
        graph.add_edge(node, node, weighted(1.0));
        let path = eulerian_path_directed(&graph).expect("a loop is traversable");
        assert_eq!(path, vec![node, node]);
    }

    #[test]
    fn empty_and_isolated_graphs_yield_empty_paths() {
        let empty: Graph = StableGraph::default();
        assert!(eulerian_path_directed(&empty).expect("empty").is_empty());
        assert!(eulerian_path_undirected(&empty).expect("empty").is_empty());
        let mut lonely: Graph = StableGraph::default();
        lonely.add_node(labelled("lonely"));
        assert!(
            eulerian_path_directed(&lonely)
                .expect("isolated")
                .is_empty()
        );
        assert!(
            eulerian_path_undirected(&lonely)
                .expect("isolated")
                .is_empty()
        );
    }

    #[test]
    fn undirected_chain_and_ring_cover_every_edge() {
        let (graph, _) = chain3();
        let path = eulerian_path_undirected(&graph).expect("a chain has a trail");
        assert!(uses_every_edge_once(&graph, &path, false));
        let mut ring: Graph = StableGraph::default();
        let nodes: Vec<NodeIndex> = (0..3)
            .map(|ordinal| ring.add_node(labelled(&ordinal.to_string())))
            .collect();
        ring.add_edge(nodes[0], nodes[1], weighted(1.0));
        ring.add_edge(nodes[1], nodes[2], weighted(1.0));
        ring.add_edge(nodes[2], nodes[0], weighted(1.0));
        let circuit = eulerian_path_undirected(&ring).expect("a ring has a circuit");
        assert!(uses_every_edge_once(&ring, &circuit, false));
    }

    #[test]
    fn undirected_parallel_edges_are_kept_distinct() {
        let mut graph: Graph = StableGraph::default();
        let x = graph.add_node(labelled("x"));
        let y = graph.add_node(labelled("y"));
        graph.add_edge(x, y, weighted(1.0));
        graph.add_edge(x, y, weighted(2.0));
        let path = eulerian_path_undirected(&graph).expect("even parallels have a circuit");
        assert!(uses_every_edge_once(&graph, &path, false));
    }

    #[test]
    fn undirected_self_loop_counts_once_but_keeps_parity() {
        let mut graph: Graph = StableGraph::default();
        let node = graph.add_node(labelled("only"));
        graph.add_edge(node, node, weighted(1.0));
        let path = eulerian_path_undirected(&graph).expect("a loop is traversable");
        assert_eq!(path, vec![node, node]);
    }

    #[test]
    fn undirected_star_reports_the_first_odd_node() {
        let mut graph: Graph = StableGraph::default();
        let hub = graph.add_node(labelled("hub"));
        let mut leaves = Vec::new();
        for ordinal in 0..3 {
            let leaf = graph.add_node(labelled(&ordinal.to_string()));
            graph.add_edge(hub, leaf, weighted(1.0));
            leaves.push(leaf);
        }
        let member = eulerian_path_undirected(&graph).expect_err("three odds forbid a trail");
        assert!(graph.node_weight(member).is_some());
    }
}
