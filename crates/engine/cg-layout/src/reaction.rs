//! Reaction to graph mutations: deciding what a layout run has to recompute.
//!
//! The engine trait itself stays free of gpui types so layouts can be unit
//! tested headlessly. Deciding *when* to run lives here, driven by
//! [`crate::driver::LayoutDriver`].

use cg_graph::{ChangeFilter, GraphChangeEvent};

/// The work a layout run must perform after a mutation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutWork {
    /// Nothing to do; positions stay valid as they are.
    None,
    /// Place the newly added nodes, leaving existing positions untouched.
    PlaceNew,
    /// Recompute edge-dependent geometry while keeping node positions.
    RefreshEdgeGeometry,
    /// Run the engine over the whole graph.
    Full,
}

/// Maps a graph change to the layout work it implies.
///
/// Node additions only need placement, because
/// [`cg_graph::ChangeFilter`] keeps removals cheap: dropping a node cannot move
/// its neighbours in a position-preserving layout.
pub fn work_for(event: &GraphChangeEvent) -> LayoutWork {
    match event {
        GraphChangeEvent::NodeAdded(_) => LayoutWork::PlaceNew,
        GraphChangeEvent::NodeRemoved(_) => LayoutWork::RefreshEdgeGeometry,
        GraphChangeEvent::EdgeAdded(_) | GraphChangeEvent::EdgeRemoved(_) => {
            LayoutWork::RefreshEdgeGeometry
        }
        GraphChangeEvent::NodeAttrChanged(_)
        | GraphChangeEvent::EdgeAttrChanged(_)
        | GraphChangeEvent::NodeClassChanged(_)
        | GraphChangeEvent::EdgeClassChanged(_) => LayoutWork::None,
        GraphChangeEvent::ParentChanged(_) | GraphChangeEvent::CollapsedChanged(_) => {
            LayoutWork::Full
        }
        GraphChangeEvent::StructureReset => LayoutWork::Full,
    }
}

/// Filter used by layout drivers: data edits never arrive, while hierarchy
/// edits do so the driver can re-run past the folding boundary.
pub const LAYOUT_FILTER: ChangeFilter = ChangeFilter {
    structure: true,
    topology: true,
    reset: true,
    data: false,
    hierarchy: true,
};

#[cfg(test)]
mod tests {
    use cg_graph::{EdgeIndex, NodeIndex};

    use super::*;

    #[test]
    fn added_nodes_only_need_placement() {
        assert_eq!(
            work_for(&GraphChangeEvent::NodeAdded(NodeIndex::new(0))),
            LayoutWork::PlaceNew
        );
    }

    #[test]
    fn edge_edits_keep_node_positions() {
        assert_eq!(
            work_for(&GraphChangeEvent::EdgeAdded(EdgeIndex::new(0))),
            LayoutWork::RefreshEdgeGeometry
        );
        assert_eq!(
            work_for(&GraphChangeEvent::EdgeRemoved(EdgeIndex::new(0))),
            LayoutWork::RefreshEdgeGeometry
        );
    }

    #[test]
    fn reset_reruns_the_engine() {
        assert_eq!(
            work_for(&GraphChangeEvent::StructureReset),
            LayoutWork::Full
        );
    }

    #[test]
    fn data_edits_leave_positions_alone_while_hierarchy_reruns() {
        assert_eq!(
            work_for(&GraphChangeEvent::NodeAttrChanged(NodeIndex::new(0))),
            LayoutWork::None
        );
        assert_eq!(
            work_for(&GraphChangeEvent::EdgeAttrChanged(EdgeIndex::new(0))),
            LayoutWork::None
        );
        assert_eq!(
            work_for(&GraphChangeEvent::NodeClassChanged(NodeIndex::new(0))),
            LayoutWork::None
        );
        assert_eq!(
            work_for(&GraphChangeEvent::ParentChanged(NodeIndex::new(0))),
            LayoutWork::Full
        );
        assert_eq!(
            work_for(&GraphChangeEvent::CollapsedChanged(NodeIndex::new(0))),
            LayoutWork::Full
        );
        assert!(!LAYOUT_FILTER.accepts(&GraphChangeEvent::NodeAttrChanged(NodeIndex::new(0))));
        assert!(LAYOUT_FILTER.accepts(&GraphChangeEvent::ParentChanged(NodeIndex::new(0))));
    }
}
