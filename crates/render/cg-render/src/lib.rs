//! Graph canvas rendering on top of gpui paint primitives.

pub mod appearance;
pub mod arrows;
pub mod bypass;
pub mod camera;
pub mod compound;
pub mod edge_rules;
pub mod export;
pub mod fill;
pub mod glyph;
pub mod image;
pub mod label_envelope;
pub mod label_plan;
pub mod label_style;
pub mod label_wrap;
pub mod lod;
pub mod mapping;
pub mod metrics;
pub mod node_rules;
pub mod palette;
pub mod png;
pub mod raster;
pub mod refresh;
pub mod retained;
pub mod selector;
pub mod shapes;
pub mod spatial;
pub mod style;
pub mod synth;
pub mod text;
pub mod transition;
pub mod view;
pub mod waypoints;

pub use arrows::ArrowKind;
pub use camera::{Camera, MAX_ZOOM, MIN_ZOOM};
pub use compound::{
    COMPOUND_PADDING, all_compound_bounds, clip_painted_edge, clip_to_compound_bounds,
    compound_bounds, pick_compound_node,
};
pub use export::{
    ExportRequest, ExportScope, ExportSnapshot, export_pixels, graph_bounds, scale_arrows,
    scale_edge_labels, scale_edges, scale_labels, scale_nodes,
};
pub use glyph::{GLYPH_ADVANCE, GLYPH_HEIGHT, GLYPH_WIDTH, glyph_rows};
pub use image::{ImageCache, ImageEntry, ImageFit, ImageStatus, NodeImage, fit_rect, node_bounds};
pub use lod::{DetailLevel, LodParams};
pub use metrics::{FrameMetrics, FrameSample, PlanCounts, measure_ms};
pub use png::encode_png;
pub use raster::{LabelOverlays, rasterize, rasterize_with_labels, solid_background};
pub use refresh::subscribe_repaint;
pub use retained::{
    CacheVersions, CameraSnapshot, RefreshInput, RetainedCache, StoredPlans, incident_edges,
};
pub use selector::{ElementSelector, SelectorSheet, SelectorTarget};
pub use shapes::{NodeShape, node_polygon, point_hits_shape, shape_hits_rect};
pub use spatial::{ADAPTIVE_CELL_MULTIPLE, SpatialIndex, adaptive_cell};
pub use style::{
    BypassStore, DEFAULT_EDGE_TINT, DEFAULT_NODE_FILL, DEFAULT_NODE_STROKE, EdgeColorMap,
    EdgeCurve, EdgeMapper, EdgeNumberMap, EdgePredicate, EdgeStyle, EdgeStylePatch,
    HIGHLIGHT_EDGE_TINT, HOVER_NODE_FILL, MAX_RANK_SCALE, MIN_RANK_SCALE, NodeColorMap,
    NodeDataTables, NodeFill,
    NodeNumberMap, NodePredicate, NodeStyle, NodeStylePatch, SCC_GROUP_FILLS, SCC_OVERFLOW_FILL,
    SELECTED_NODE_FILL, StyleMapper, StyleSheet, color_map, linear_map, scale_for_rank, scc_fill,
};
pub use synth::{SynthGraph, chain_with_cross_edges, grid_with_random_edges};
pub use text::{
    DEFAULT_EDGE_LABEL_COLOR, DEFAULT_EDGE_LABEL_SIZE, DEFAULT_LABEL_COLOR, DEFAULT_LABEL_SIZE,
    EDGE_LABEL_GAP, LABEL_BACKGROUND_FILL, LABEL_BACKGROUND_PAD, LABEL_CORNER_RADIUS, LABEL_GAP,
    LABEL_LINE_HEIGHT_SCALE, LabelAlign, LabelBackground, LabelStyle, MAX_LABEL_CHARS_PER_LINE,
    PaintedEdgeLabel, PaintedLabel, draws_labels, edge_label_anchor, edge_label_angle,
    estimate_label_block, label_envelope, label_envelope_styled, line_height, node_label_origin, paint_edge_labels_for,
    paint_edge_labels_for_with_style, paint_labels_for, paint_labels_for_with_style,
    split_label_lines,
};
pub use transition::{
    EdgeStyleTransition, NodeStyleTransition, blend_edge_style, blend_label_style,
    blend_node_style,
};
pub use view::{
    ARROW_HALF_WIDTH, ARROW_LENGTH, EDGE_AGGREGATION_THRESHOLD, BundleSlot, EdgeOrdinal,
    EdgePaintOptions, NODE_SIDE,
    PARALLEL_STEP, PaintedArrow, PaintedContainer, PaintedEdge, PaintedNode, PaintedRubberBand,
    bundle_slot, curve_control, edge_ordinals_for, graph_view, loop_ordinal, paint_arrows,
    paint_arrows_for_level, paint_edges, paint_edges_for, paint_edges_for_with_waypoints,
    paint_edges_with_options, paint_nodes, paint_nodes_for, paint_nodes_for_level,
    paint_single_arrow, paint_single_edge, paint_single_edge_with_waypoints, paint_single_node,
    paint_single_node_for_level, painted_edge_hits, routed_options, visible_node_ids,
    world_viewport_rect,
};
pub use waypoints::WaypointStore;
