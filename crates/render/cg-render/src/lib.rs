//! Graph canvas rendering on top of gpui paint primitives.

pub mod camera;
pub mod refresh;
pub mod spatial;
pub mod style;
pub mod view;

pub use camera::{Camera, MAX_ZOOM, MIN_ZOOM};
pub use refresh::subscribe_repaint;
pub use spatial::SpatialIndex;
pub use style::{
    BypassStore, DEFAULT_EDGE_TINT, DEFAULT_NODE_FILL, DEFAULT_NODE_STROKE, EdgeMapper,
    EdgePredicate, EdgeStyle, EdgeStylePatch, HIGHLIGHT_EDGE_TINT, HOVER_NODE_FILL, MAX_RANK_SCALE,
    MIN_RANK_SCALE, NodePredicate, NodeStyle, NodeStylePatch, SCC_GROUP_FILLS, SCC_OVERFLOW_FILL,
    SELECTED_NODE_FILL, StyleMapper, StyleSheet, scale_for_rank, scc_fill,
};
pub use view::{
    NODE_SIDE, PARALLEL_STEP, PaintedArrow, PaintedEdge, PaintedNode, PaintedRubberBand,
    graph_view, paint_arrows, paint_edges, paint_nodes, painted_edge_hits,
};
