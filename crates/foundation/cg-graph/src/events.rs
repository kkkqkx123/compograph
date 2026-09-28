//! Change notifications emitted when the graph structure is edited.

use petgraph::stable_graph::{EdgeIndex, NodeIndex};

use crate::store::GraphStore;

/// Describes a single structural mutation of the graph store.
///
/// Layout and rendering layers subscribe to these events to decide which
/// part of the pipeline needs to run again.
#[derive(Clone, Copy, Debug)]
pub enum GraphChangeEvent {
    NodeAdded(NodeIndex),
    NodeRemoved(NodeIndex),
    EdgeAdded(EdgeIndex),
    EdgeRemoved(EdgeIndex),
    StructureReset,
}

impl gpui::EventEmitter<GraphChangeEvent> for GraphStore {}
