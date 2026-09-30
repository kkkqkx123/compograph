//! Paint plans and canvas element for the graph.
//!
//! Each plan family lives in its own file under `view/`; this module only
//! declares the families and re-exports their public items so existing
//! `view::` paths keep working.
//!
//! Layout: `plans` holds the shared screen pixel vocabulary, `culling`
//! holds viewport math, `nodes`/`edges`/`heads` build node/edge/arrowhead
//! plans headlessly, and `canvas` owns the gpui element. Edge planning shares
//! `bundles` for parallel grouping, `routing` for per-edge geometry and
//! `hits` for box selection. Tests live with the family they cover.

pub mod bundles;
pub mod canvas;
pub mod culling;
pub mod edges;
pub mod heads;
pub mod hits;
pub mod nodes;
pub mod plans;
pub mod routing;

pub use bundles::{bundle_slot, edge_ordinals_for, loop_ordinal};
pub use canvas::graph_view;
pub use culling::world_viewport_rect;
pub use edges::{
    paint_edges, paint_edges_for, paint_edges_for_with_waypoints, paint_edges_with_options,
    paint_single_edge, paint_single_edge_with_waypoints,
};
pub use heads::{paint_arrows, paint_arrows_for_level, paint_single_arrow};
pub use hits::painted_edge_hits;
pub use nodes::{
    paint_nodes, paint_nodes_for, paint_nodes_for_level, paint_single_node,
    paint_single_node_for_level, visible_node_ids,
};
pub use plans::{
    ARROW_HALF_WIDTH, ARROW_LENGTH, EDGE_AGGREGATION_THRESHOLD, EdgePaintOptions, NODE_SIDE,
    PARALLEL_STEP, PaintedArrow, PaintedEdge, PaintedNode, PaintedRubberBand, RUBBER_BAND_STROKE,
};
