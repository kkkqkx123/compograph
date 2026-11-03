//! Change notifications emitted when the graph structure is edited.

use petgraph::stable_graph::{EdgeIndex, NodeIndex};

use crate::store::GraphStore;

/// Describes a single structural mutation of the graph store.
///
/// Layout and rendering layers subscribe to these events to decide which
/// part of the pipeline needs to run again. Endpoint moves and payload edits
/// travel as their own variants so subscribers never confuse them with
/// additions or removals. Batched work collapses to one [`GraphChangeEvent::BatchCommitted`].
#[derive(Clone, Copy, Debug)]
pub enum GraphChangeEvent {
    NodeAdded(NodeIndex),
    NodeRemoved(NodeIndex),
    EdgeAdded(EdgeIndex),
    EdgeRemoved(EdgeIndex),
    NodeAttrChanged(NodeIndex),
    EdgeAttrChanged(EdgeIndex),
    NodeClassChanged(NodeIndex),
    EdgeClassChanged(EdgeIndex),
    NodeDataChanged(NodeIndex),
    EdgeDataChanged(EdgeIndex),
    EdgeEndpointsChanged(EdgeIndex),
    ParentChanged(NodeIndex),
    CollapsedChanged(NodeIndex),
    StructureReset,
    BatchCommitted,
}

impl gpui::EventEmitter<GraphChangeEvent> for GraphStore {}
