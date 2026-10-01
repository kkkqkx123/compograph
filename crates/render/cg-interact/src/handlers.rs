//! Pure translation of pointer positions into graph state changes.
//!
//! These helpers stay free of gpui event types so they can be unit tested
//! headlessly; the application converts platform events into the model-space
//! points these functions consume.
//!
//! Concrete helpers live in flat sibling modules grouped by interaction
//! semantics; this module only re-exports them so existing `handlers::`
//! paths keep working.

pub use super::compound_hit::{compound_toggle_target, press_hit_compound};
pub use super::edge_select::{
    edge_hits_rect, edges_in_rect, edges_in_rect_for_styles, edges_in_rect_with_options,
    loop_hits_rect,
};
pub use super::hits::{
    NODE_GRAB_TOLERANCE, NODE_HALF_EXTENT, drag_position, hover_node_shaped, normalize_drag,
    press_hit_shaped,
};
pub use super::neighborhood::{expand_neighborhood, neighborhood_edges};
pub use super::node_select::{nodes_in_rect, nodes_in_rect_with_labels};
pub use super::policy::{
    apply_point_select, can_begin_drag, can_grab_node, should_clear_on_blank, wheel_zoom_factor,
};
