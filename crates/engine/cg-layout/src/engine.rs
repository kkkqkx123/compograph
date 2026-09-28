//! The layout engine contract: turning graph structure into node positions.

use cg_graph::{GraphStore, Positions};

/// Produces model-space coordinates for every node of the graph.
///
/// Implementations may use `previous` as a starting point so that incremental
/// edits keep the rest of the graph stable instead of reshuffling everything.
pub trait LayoutEngine {
    fn layout(&self, store: &GraphStore, previous: &Positions) -> Positions;

    /// Identifier used by the layout registry and user-facing pickers.
    fn name(&self) -> &'static str;
}
