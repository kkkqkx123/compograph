//! Framework-agnostic geometry for hit testing and edge shapes.

pub mod aggregation;
pub mod curves;
pub mod picking;

pub use aggregation::{
    HAYSTACK_BUNDLE_THRESHOLD, HAYSTACK_RADIUS_SCALE, HAYSTACK_SPREAD, ORTHO_MAX_BENDS,
    OrthoDirection, haystack_angle, haystack_endpoints, manhattan_route, ortho_polyline,
    segmented_polyline, taxi_polyline, use_haystack,
};
pub use curves::{
    SELF_LOOP_HALF_WIDTH_SCALE, SELF_LOOP_HEIGHT_SCALE, SELF_LOOP_SAMPLES, bezier_control_for_edge,
    cubic_bezier, parallel_offsets, quadratic_bezier, sample_cubic_bezier, sample_quadratic_bezier,
    self_loop_controls, self_loop_polyline,
};
pub use picking::{
    BEZIER_HIT_SAMPLES, EDGE_HIT_TOLERANCE, NODE_HIT_TOLERANCE, bezier_hit, distance_to_bezier,
    distance_to_polyline, distance_to_segment, edge_hit, nearest_point_index, point_hits_node,
    point_in_polygon, polygon_intersects_rect, polyline_intersects_rect,
};
pub use picking::{segment_intersects_rect, segments_intersect};
