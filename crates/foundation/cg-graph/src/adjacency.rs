//! Materialized adjacency index over a read-only graph view.
//!
//! Layout engines and paint planners ask the same question once per node:
//! which neighbours does this node have, and how many edges touch it.
//! Answering through the live view costs one vector per question, and the
//! store rebuilds its whole visible edge table per question once the graph
//! carries containers.
//!
//! A snapshot answers the same questions from one materialized pass. The node
//! list, the edge list and the adjacency runs are built once, then read
//! without allocating: neighbours come back as slices of one flat array.

use std::collections::HashMap;

use petgraph::stable_graph::NodeIndex;

use crate::view::GraphView;

/// Adjacent nodes of every node, stored as one flat run per direction.
struct Runs {
    /// Start offset of every node's run; one extra entry closes the last run.
    start: Vec<u32>,
    /// Neighbour identifiers grouped by node.
    items: Vec<NodeIndex>,
}

impl Runs {
    /// Neighbours of the node at `position`.
    fn of(&self, position: u32) -> &[NodeIndex] {
        let from = self.start[position as usize] as usize;
        let to = self.start[position as usize + 1] as usize;
        &self.items[from..to]
    }
}

/// One materialized pass over a graph view.
///
/// The snapshot keeps the view's node order and orders edges by their
/// endpoints, so repeated runs over the same structure agree.
pub struct AdjacencySnapshot {
    nodes: Vec<NodeIndex>,
    edges: Vec<(NodeIndex, NodeIndex)>,
    /// Position of every node inside the node list.
    position: HashMap<NodeIndex, u32>,
    /// Incident edge count per node; a self loop counts once.
    degrees: Vec<u32>,
    outgoing: Runs,
    incoming: Runs,
    undirected: Runs,
}

impl AdjacencySnapshot {
    /// Captures the visible structure of `view`.
    ///
    /// The capture costs one pass over the nodes and one over the edges.
    /// Nodes missing from the view's node list drop out of the adjacency
    /// runs, so a view that reports an edge to an unknown node still yields
    /// a consistent index.
    pub fn capture(view: &dyn GraphView) -> Self {
        let nodes = view.node_ids();
        let mut edges = view.edges();
        edges.sort_unstable_by_key(|(source, target)| (source.index(), target.index()));

        let mut position = HashMap::with_capacity(nodes.len());
        for (index, node) in nodes.iter().enumerate() {
            position.insert(*node, index as u32);
        }

        let mut degrees = vec![0u32; nodes.len()];
        let mut outgoing: Vec<(u32, NodeIndex)> = Vec::with_capacity(edges.len());
        let mut incoming: Vec<(u32, NodeIndex)> = Vec::with_capacity(edges.len());
        let mut undirected: Vec<(u32, NodeIndex)> = Vec::with_capacity(edges.len() * 2);
        for (source, target) in &edges {
            let (Some(&source_position), Some(&target_position)) =
                (position.get(source), position.get(target))
            else {
                continue;
            };
            degrees[source_position as usize] += 1;
            degrees[target_position as usize] += 1;
            if source == target {
                degrees[source_position as usize] -= 1;
            }
            outgoing.push((source_position, *target));
            incoming.push((target_position, *source));
            undirected.push((source_position, *target));
            undirected.push((target_position, *source));
        }

        Self {
            outgoing: Runs::build(nodes.len(), outgoing),
            incoming: Runs::build(nodes.len(), incoming),
            undirected: Runs::build(nodes.len(), undirected),
            nodes,
            edges,
            position,
            degrees,
        }
    }

    /// Nodes of the captured structure, in the order the view reported them.
    pub fn nodes(&self) -> &[NodeIndex] {
        &self.nodes
    }

    /// Edges of the captured structure, ordered by their endpoints.
    pub fn edges(&self) -> &[(NodeIndex, NodeIndex)] {
        &self.edges
    }

    /// Number of captured nodes.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Number of captured edges.
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// Number of edges touching `node`; a self loop counts once.
    ///
    /// Parallel edges count separately, matching the incident edge count the
    /// graph view reports.
    pub fn degree(&self, node: NodeIndex) -> usize {
        match self.position.get(&node) {
            Some(position) => self.degrees[*position as usize] as usize,
            None => 0,
        }
    }

    /// Distinct out-neighbours of `node`, in index order.
    pub fn successors(&self, node: NodeIndex) -> &[NodeIndex] {
        self.run(&self.outgoing, node)
    }

    /// Distinct in-neighbours of `node`, in index order.
    pub fn predecessors(&self, node: NodeIndex) -> &[NodeIndex] {
        self.run(&self.incoming, node)
    }

    /// Distinct neighbours of `node` in either direction, in index order.
    pub fn neighbors(&self, node: NodeIndex) -> &[NodeIndex] {
        self.run(&self.undirected, node)
    }

    /// True when `node` belongs to the captured structure.
    pub fn contains(&self, node: NodeIndex) -> bool {
        self.position.contains_key(&node)
    }

    /// Neighbours of `node` inside one run, empty for unknown nodes.
    fn run<'a>(&self, runs: &'a Runs, node: NodeIndex) -> &'a [NodeIndex] {
        match self.position.get(&node) {
            Some(position) => runs.of(*position),
            None => &[],
        }
    }
}

impl Runs {
    /// Builds the flat runs from node-keyed neighbour entries.
    ///
    /// Entries are ordered by node and neighbour index, then duplicates
    /// collapse, so every run holds distinct neighbours in index order.
    fn build(node_count: usize, mut entries: Vec<(u32, NodeIndex)>) -> Self {
        entries.sort_unstable_by_key(|(position, node)| (*position, node.index()));
        entries.dedup_by_key(|(position, node)| (*position, node.index()));

        let mut start = vec![0u32; node_count + 1];
        for (position, _) in &entries {
            start[*position as usize + 1] += 1;
        }
        for index in 1..start.len() {
            start[index] += start[index - 1];
        }

        let mut items = vec![NodeIndex::new(0); start[node_count] as usize];
        let mut cursor = start.clone();
        for (position, node) in entries {
            let slot = cursor[position as usize] as usize;
            items[slot] = node;
            cursor[position as usize] += 1;
        }

        Self { start, items }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view::MockGraph;

    #[test]
    fn neighbours_follow_the_edge_direction() {
        let mut graph = MockGraph::empty();
        graph.push_edge(0, 1);
        graph.push_edge(2, 1);
        let snapshot = AdjacencySnapshot::capture(&graph);
        let one = NodeIndex::new(1);
        assert!(snapshot.successors(one).is_empty());
        assert_eq!(
            snapshot.predecessors(one),
            &[NodeIndex::new(0), NodeIndex::new(2)]
        );
        assert_eq!(
            snapshot.neighbors(one),
            &[NodeIndex::new(0), NodeIndex::new(2)]
        );
        assert_eq!(snapshot.successors(NodeIndex::new(0)), &[NodeIndex::new(1)]);
        assert!(snapshot.predecessors(NodeIndex::new(0)).is_empty());
    }

    #[test]
    fn parallel_edges_collapse_in_the_runs_but_count_in_the_degree() {
        let mut graph = MockGraph::empty();
        graph.push_edge(0, 1);
        graph.push_edge(0, 1);
        let snapshot = AdjacencySnapshot::capture(&graph);
        assert_eq!(snapshot.neighbors(NodeIndex::new(0)), &[NodeIndex::new(1)]);
        assert_eq!(snapshot.degree(NodeIndex::new(0)), 2);
    }

    #[test]
    fn self_loops_count_once_towards_the_degree() {
        let mut graph = MockGraph::isolated(1);
        graph.push_edge(0, 0);
        let snapshot = AdjacencySnapshot::capture(&graph);
        assert_eq!(snapshot.degree(NodeIndex::new(0)), 1);
        assert_eq!(snapshot.neighbors(NodeIndex::new(0)), &[NodeIndex::new(0)]);
    }

    #[test]
    fn unknown_nodes_answer_with_nothing() {
        let graph = MockGraph::isolated(2);
        let snapshot = AdjacencySnapshot::capture(&graph);
        let missing = NodeIndex::new(9);
        assert_eq!(snapshot.degree(missing), 0);
        assert!(snapshot.neighbors(missing).is_empty());
        assert!(!snapshot.contains(missing));
        assert!(snapshot.contains(NodeIndex::new(0)));
    }

    #[test]
    fn capture_keeps_the_node_order_of_the_view() {
        let graph = MockGraph::chain(3);
        let snapshot = AdjacencySnapshot::capture(&graph);
        assert_eq!(snapshot.node_count(), 3);
        assert_eq!(snapshot.edge_count(), 2);
        assert_eq!(
            snapshot.nodes(),
            &[NodeIndex::new(0), NodeIndex::new(1), NodeIndex::new(2)]
        );
    }
}
