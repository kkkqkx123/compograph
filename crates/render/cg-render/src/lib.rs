//! Graph canvas rendering on top of gpui paint primitives.

pub mod camera;
pub mod export;
pub mod lod;
pub mod metrics;
pub mod refresh;
pub mod retained;
pub mod spatial;
pub mod style;
pub mod synth;
pub mod view;

pub use camera::{Camera, MAX_ZOOM, MIN_ZOOM};
pub use export::{
    ExportRequest, ExportScope, ExportSnapshot, encode_ppm, export_pixels, graph_bounds, rasterize,
    scale_arrows, scale_edges, scale_nodes, solid_background,
};
pub use lod::{DetailLevel, LodParams};
pub use metrics::{FrameMetrics, FrameSample, PlanCounts, measure_ms};
pub use refresh::subscribe_repaint;
pub use retained::{
    CacheVersions, CameraSnapshot, RefreshInput, RetainedCache, StoredPlans, incident_edges,
};
pub use spatial::{ADAPTIVE_CELL_MULTIPLE, SpatialIndex, adaptive_cell};
pub use style::{
    BypassStore, DEFAULT_EDGE_TINT, DEFAULT_NODE_FILL, DEFAULT_NODE_STROKE, EdgeMapper,
    EdgePredicate, EdgeStyle, EdgeStylePatch, HIGHLIGHT_EDGE_TINT, HOVER_NODE_FILL, MAX_RANK_SCALE,
    MIN_RANK_SCALE, NodePredicate, NodeStyle, NodeStylePatch, SCC_GROUP_FILLS, SCC_OVERFLOW_FILL,
    SELECTED_NODE_FILL, StyleMapper, StyleSheet, scale_for_rank, scc_fill,
};
pub use synth::{SynthGraph, chain_with_cross_edges, grid_with_random_edges};
pub use view::{
    EDGE_AGGREGATION_THRESHOLD, EdgePaintOptions, NODE_SIDE, PARALLEL_STEP, PaintedArrow,
    PaintedEdge, PaintedNode, PaintedRubberBand, bundle_slot, edge_ordinals_for, graph_view,
    loop_ordinal, paint_arrows, paint_arrows_for_level, paint_edges, paint_edges_for,
    paint_edges_with_options, paint_nodes, paint_nodes_for, paint_single_arrow, paint_single_edge,
    paint_single_node, painted_edge_hits, visible_node_ids, world_viewport_rect,
};
