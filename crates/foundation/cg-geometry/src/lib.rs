//! Framework-agnostic geometry for hit testing and edge shapes.

pub mod curves;
pub mod picking;

pub use curves::{
    bezier_control_for_edge, parallel_offsets, quadratic_bezier, sample_quadratic_bezier,
};
pub use picking::{
    BEZIER_HIT_SAMPLES, EDGE_HIT_TOLERANCE, NODE_HIT_TOLERANCE, bezier_hit, distance_to_bezier,
    distance_to_polyline, distance_to_segment, edge_hit, nearest_point_index, point_hits_node,
};
pub use picking::{segment_intersects_rect, segments_intersect};
