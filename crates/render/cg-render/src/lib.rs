//! Graph canvas rendering on top of gpui paint primitives.

pub mod arrows;
pub mod camera;
pub mod export;
pub mod lod;
pub mod metrics;
pub mod png;
pub mod raster;
pub mod refresh;
pub mod retained;
pub mod shapes;
pub mod spatial;
pub mod style;
pub mod synth;
pub mod text;
pub mod view;

pub use arrows::ArrowKind;
pub use camera::{Camera, MAX_ZOOM, MIN_ZOOM};
pub use export::{
    ExportRequest, ExportScope, ExportSnapshot, export_pixels, graph_bounds, scale_arrows,
    scale_edges, scale_nodes,
};
pub use png::encode_png;
pub use raster::{rasterize, solid_background};
pub use lod::{DetailLevel, LodParams};
pub use metrics::{FrameMetrics, FrameSample, PlanCounts, measure_ms};
pub use refresh::subscribe_repaint;
pub use retained::{
    CacheVersions, CameraSnapshot, RefreshInput, RetainedCache, StoredPlans, incident_edges,
};
pub use shapes::{NodeShape, node_polygon, point_hits_shape, shape_hits_rect};
pub use spatial::{ADAPTIVE_CELL_MULTIPLE, SpatialIndex, adaptive_cell};
pub use style::{
    BypassStore, DEFAULT_EDGE_TINT, DEFAULT_NODE_FILL, DEFAULT_NODE_STROKE, EdgeMapper,
    EdgePredicate, EdgeStyle, EdgeStylePatch, HIGHLIGHT_EDGE_TINT, HOVER_NODE_FILL, MAX_RANK_SCALE,
    MIN_RANK_SCALE, NodePredicate, NodeStyle, NodeStylePatch, SCC_GROUP_FILLS, SCC_OVERFLOW_FILL,
    SELECTED_NODE_FILL, StyleMapper, StyleSheet, scale_for_rank, scc_fill,
};
pub use synth::{SynthGraph, chain_with_cross_edges, grid_with_random_edges};
pub use text::{
    DEFAULT_EDGE_LABEL_COLOR, DEFAULT_EDGE_LABEL_SIZE, DEFAULT_LABEL_COLOR, DEFAULT_LABEL_SIZE,
    EDGE_LABEL_GAP, LABEL_GAP, LABEL_LINE_HEIGHT_SCALE, MAX_LABEL_CHARS_PER_LINE, PaintedEdgeLabel,
    PaintedLabel, draws_labels, edge_label_anchor, line_height, paint_edge_labels_for,
    paint_labels_for, split_label_lines,
};
pub use view::{
    ARROW_HALF_WIDTH, ARROW_LENGTH, EDGE_AGGREGATION_THRESHOLD, EdgePaintOptions, NODE_SIDE,
    PARALLEL_STEP, PaintedArrow, PaintedEdge, PaintedNode, PaintedRubberBand, bundle_slot,
    edge_ordinals_for, graph_view, loop_ordinal, paint_arrows, paint_arrows_for_level, paint_edges,
    paint_edges_for, paint_edges_with_options, paint_nodes, paint_nodes_for, paint_nodes_for_level,
    paint_single_arrow, paint_single_edge, paint_single_node, paint_single_node_for_level,
    painted_edge_hits, visible_node_ids, world_viewport_rect,
};
