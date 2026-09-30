//! Undirected cut searches over the directed store.

use std::collections::{HashMap, HashSet};

use petgraph::Directed;
use petgraph::stable_graph::{NodeIndex, StableGraph};
use petgraph::visit::{EdgeRef, IntoEdgeReferences};

use crate::store::{EdgeData, NodeData};

type Graph = StableGraph<NodeData, EdgeData, Directed>;

type UndirectedView = (Vec<NodeIndex>, HashMap<NodeIndex, Vec<(NodeIndex, usize)>>);

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
