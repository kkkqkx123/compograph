//! Snapshot-backed graph view for background layout tasks.
//!
//! Background executors cannot borrow the live store, so static engines run
//! over an owned node and edge snapshot instead. This view replays the
//! snapshot through the read-only graph contract.

use cg_graph::{GraphView, NodeIndex};

use crate::force::ForceSnapshot;

/// Owned structural view a background task can read without borrowing live state.
pub(crate) struct StaticView {
    snapshot: ForceSnapshot,
}

impl StaticView {
    pub(crate) fn new(snapshot: ForceSnapshot) -> Self {
        Self { snapshot }
    }
}

impl GraphView for StaticView {
    fn node_ids(&self) -> Vec<NodeIndex> {
        self.snapshot.nodes.clone()
    }

    fn node_count(&self) -> usize {
        self.snapshot.nodes.len()
    }

    fn edge_count(&self) -> usize {
        self.snapshot.edges.len()
    }

    fn edges(&self) -> Vec<(NodeIndex, NodeIndex)> {
        self.snapshot.edges.clone()
    }

    fn degree(&self, node: NodeIndex) -> usize {
        self.snapshot
            .edges
            .iter()
            .filter(|(source, target)| *source == node || *target == node)
            .count()
    }

    fn neighbors(&self, node: NodeIndex) -> Vec<NodeIndex> {
        collect_neighbors(&self.snapshot.edges, node, true, true)
    }

    fn successors(&self, node: NodeIndex) -> Vec<NodeIndex> {
        collect_neighbors(&self.snapshot.edges, node, true, false)
    }

    fn predecessors(&self, node: NodeIndex) -> Vec<NodeIndex> {
        collect_neighbors(&self.snapshot.edges, node, false, true)
    }
}

fn collect_neighbors(
    edges: &[(NodeIndex, NodeIndex)],
    node: NodeIndex,
    outgoing: bool,
    incoming: bool,
) -> Vec<NodeIndex> {
    let mut out: Vec<NodeIndex> = edges
        .iter()
        .filter_map(|(source, target)| {
            if outgoing && *source == node {
                Some(*target)
            } else if incoming && *target == node {
                Some(*source)
            } else {
                None
            }
        })
        .collect();
    out.sort_unstable_by_key(|node| node.index());
    out.dedup_by_key(|node| node.index());
    out
}

#[cfg(test)]
mod tests {
    use super::collect_neighbors;
    use cg_graph::NodeIndex;

    #[test]
    fn neighbor_directions_stay_directed() {
        let edges = vec![(NodeIndex::new(0), NodeIndex::new(1))];
        assert_eq!(
            collect_neighbors(&edges, NodeIndex::new(0), true, true),
            vec![NodeIndex::new(1)]
        );
        assert_eq!(
            collect_neighbors(&edges, NodeIndex::new(1), true, true),
            vec![NodeIndex::new(0)]
        );
        assert_eq!(
            collect_neighbors(&edges, NodeIndex::new(0), true, false),
            vec![NodeIndex::new(1)]
        );
        assert!(collect_neighbors(&edges, NodeIndex::new(0), false, true).is_empty());
        assert_eq!(
            collect_neighbors(&edges, NodeIndex::new(1), false, true),
            vec![NodeIndex::new(0)]
        );
    }
}
