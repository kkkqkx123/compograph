//! The layout engine contract: turning graph structure into node positions.

use cg_graph::{FixedNodes, GraphView, Positions};

use crate::force::ForceOptions;

/// Produces model-space coordinates for every node of the graph.
///
/// Implementations may use `previous` as a starting point so that incremental
/// edits keep the rest of the graph stable instead of reshuffling everything.
/// Nodes in `fixed` keep their previous coordinates; engines still let them
/// attract their neighbours so the surroundings settle around them.
///
/// Dynamic dispatch is used because layouts are chosen at runtime from user
/// input, and the set of engines is open to future additions.
pub trait LayoutEngine {
    fn layout(&self, graph: &dyn GraphView, previous: &Positions, fixed: &FixedNodes) -> Positions;

    /// Identifier used by the layout registry and user-facing pickers.
    fn name(&self) -> &'static str;

    /// Force-directed options when this engine refines in the background.
    ///
    /// Only force-directed engines override this; the driver runs any other
    /// engine synchronously and ignores the background path.
    fn force_options(&self) -> Option<ForceOptions> {
        None
    }
}
