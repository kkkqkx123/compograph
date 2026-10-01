//! Compound container hit testing and toggle detection.

use cg_graph::{GraphStore, NodeIndex, Positions};
use cg_types::Point2;

/// True when `node` may fold or unfold as a compound container.
pub fn compound_toggle_target(store: &GraphStore, node: NodeIndex) -> bool {
    store.is_container(node)
}

/// Compound-aware hit: deep children win over containers, hidden nodes never hit.
///
/// Falls back to none when the store holds no hierarchy, letting the caller
/// use the shaped path instead.
pub fn press_hit_compound(
    world_point: Point2,
    store: &GraphStore,
    positions: &Positions,
    half_extent: f32,
    tolerance: f32,
) -> Option<NodeIndex> {
    if !store.has_compound() {
        return None;
    }
    cg_render::pick_compound_node(store, positions, world_point, half_extent, tolerance)
}
