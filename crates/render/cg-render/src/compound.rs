//! Compound containers: bounds, hit priority, and edge clipping.
//!
//! Bounds derive from live child coordinates on every call; nothing is
//! cached. Hit order prefers deep children over their containers, and hidden
//! descendants never hit.

use cg_graph::{GraphStore, NodeIndex, Positions};
use cg_types::{Point2, Rect, Vec2};

/// Padding added around the child extent, in model units.
pub const COMPOUND_PADDING: f32 = 16.0;

/// Bounds of `container` from its visible descendants.
///
/// Returns none for leaves, hidden subtrees, and containers without placed
/// descendants.
pub fn compound_bounds(
    store: &GraphStore,
    positions: &Positions,
    container: NodeIndex,
    half_extent: f32,
) -> Option<Rect> {
    if !store.is_container(container) || !store.is_visible(container) {
        return None;
    }
    let mut points: Vec<Point2> = Vec::new();
    for member in store.descendants_of(container) {
        if !store.is_visible(member) {
            continue;
        }
        if store.is_container(member) {
            continue;
        }
        if let Some(point) = positions.get(&member).copied() {
            points.push(point);
        }
    }
    if points.is_empty() {
        return None;
    }
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for point in points {
        min_x = min_x.min(point.x);
        min_y = min_y.min(point.y);
        max_x = max_x.max(point.x);
        max_y = max_y.max(point.y);
    }
    let pad = half_extent + COMPOUND_PADDING;
    Some(Rect::from_corners(
        Point2::new(min_x - pad, min_y - pad),
        Point2::new(max_x + pad, max_y + pad),
    ))
}

/// Bounds for every visible container, keyed by container.
pub fn all_compound_bounds(
    store: &GraphStore,
    positions: &Positions,
    half_extent: f32,
) -> Vec<(NodeIndex, Rect)> {
    let mut ids = store.visible_node_ids();
    ids.sort_unstable_by_key(|node| node.index());
    let mut bounds = Vec::new();
    for node in ids {
        if let Some(rect) = compound_bounds(store, positions, node, half_extent) {
            bounds.push((node, rect));
        }
    }
    bounds
}

/// Depth of `node` in the compound tree; roots sit at zero.
fn compound_depth(store: &GraphStore, node: NodeIndex) -> usize {
    store.ancestors_of(node).len()
}

/// Node under `point` with children preferred over containers.
///
/// Visible leaves and deep children win over their ancestors; collapsed
/// descendants never participate. Containers hit through their bounds.
pub fn pick_compound_node(
    store: &GraphStore,
    positions: &Positions,
    point: Point2,
    half_extent: f32,
    tolerance: f32,
) -> Option<NodeIndex> {
    let mut candidates: Vec<(usize, f32, NodeIndex)> = Vec::new();
    for node in store.visible_node_ids() {
        if store.is_container(node) {
            continue;
        }
        let Some(center) = positions.get(&node).copied() else {
            continue;
        };
        let half = half_extent + tolerance;
        if (point.x - center.x).abs() <= half && (point.y - center.y).abs() <= half {
            let distance = (point - center).length_squared();
            candidates.push((compound_depth(store, node), distance, node));
        }
    }
    if let Some((_, _, node)) = candidates
        .iter()
        .max_by(|a, b| {
            a.0.cmp(&b.0)
                .then_with(|| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal))
        })
        .copied()
    {
        return Some(node);
    }
    let mut boxes = all_compound_bounds(store, positions, half_extent);
    boxes.sort_by_key(|(node, _)| std::cmp::Reverse(compound_depth(store, *node)));
    for (node, rect) in boxes {
        let grown = Rect::new(
            Point2::new(rect.origin.x - tolerance, rect.origin.y - tolerance),
            Vec2::new(rect.size.x + tolerance * 2.0, rect.size.y + tolerance * 2.0),
        );
        if grown.contains(point) {
            return Some(node);
        }
    }
    None
}

/// Clips a ray from `center` along `delta` to the container bounds.
pub fn clip_to_compound_bounds(center: Point2, delta: Vec2, bounds: Rect) -> Point2 {
    if delta.x == 0.0 && delta.y == 0.0 {
        return center;
    }
    let mut best: Option<(f32, Point2)> = None;
    let min = bounds.origin;
    let max = Point2::new(
        bounds.origin.x + bounds.size.x,
        bounds.origin.y + bounds.size.y,
    );
    let corners = [
        min,
        Point2::new(max.x, min.y),
        max,
        Point2::new(min.x, max.y),
    ];
    for pair in corners.iter().zip(corners.iter().cycle().skip(1)).take(4) {
        if let Some(hit) = ray_hits_segment(center, delta, *pair.0, *pair.1) {
            let replace = best.map(|(known, _)| hit.0 < known).unwrap_or(true);
            if replace {
                best = Some(hit);
            }
        }
    }
    best.map(|(_, point)| point).unwrap_or(center)
}

fn ray_hits_segment(origin: Point2, delta: Vec2, a: Point2, b: Point2) -> Option<(f32, Point2)> {
    let edge = b - a;
    let denom = delta.x * edge.y - delta.y * edge.x;
    if denom.abs() < 1e-6 {
        return None;
    }
    let dx = a.x - origin.x;
    let dy = a.y - origin.y;
    let ray_t = (dx * edge.y - dy * edge.x) / denom;
    let edge_t = (dx * delta.y - dy * delta.x) / denom;
    if ray_t >= 0.0 && edge_t >= 0.0 && edge_t <= 1.0 {
        Some((
            ray_t,
            Point2::new(origin.x + delta.x * ray_t, origin.y + delta.y * ray_t),
        ))
    } else {
        None
    }
}

/// Screen-space edge with its endpoints clipped to container bounds.
///
/// Endpoints inside a container move to the boundary along the edge
/// direction; loop edges and endpoints outside every container stay put.
/// Bends themselves are untouched, only the two ends move.
pub fn clip_painted_edge(
    edge: &crate::view::PaintedEdge,
    containers: &[crate::view::PaintedContainer],
) -> crate::view::PaintedEdge {
    if edge.loop_ctrls.is_some() || containers.is_empty() {
        return edge.clone();
    }
    let mut clipped = edge.clone();
    let first = edge.bends.first().copied().unwrap_or(edge.end);
    let last = edge.bends.last().copied().unwrap_or(edge.start);
    if let Some(rect) = smallest_containing(containers, edge.start) {
        let delta = first - edge.start;
        clipped.start = clip_to_compound_bounds(edge.start, delta, rect);
    }
    if let Some(rect) = smallest_containing(containers, edge.end) {
        let delta = last - edge.end;
        clipped.end = clip_to_compound_bounds(edge.end, delta, rect);
    }
    clipped
}

fn smallest_containing(
    containers: &[crate::view::PaintedContainer],
    point: Point2,
) -> Option<Rect> {
    let mut best: Option<Rect> = None;
    let mut best_area = f32::INFINITY;
    for container in containers {
        if container.rect.contains(point) {
            let area = (container.rect.size.x.max(0.0)) * (container.rect.size.y.max(0.0));
            if area < best_area {
                best_area = area;
                best = Some(container.rect);
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_bounds_have_no_container() {
        let store = GraphStore::new();
        assert!(compound_bounds(&store, &Positions::new(), NodeIndex::new(0), 12.0).is_none());
        assert!(pick_compound_node(&store, &Positions::new(), Point2::ZERO, 12.0, 2.0).is_none());
    }

    #[test]
    fn ray_clip_stays_on_the_bounds() {
        let bounds = Rect::new(Point2::new(-10.0, -10.0), Vec2::new(20.0, 20.0));
        let clipped = clip_to_compound_bounds(Point2::ZERO, Vec2::new(100.0, 0.0), bounds);
        assert!((clipped.x - 10.0).abs() < 1e-3);
        assert!(clipped.y.abs() < 1e-3);
        let idle = clip_to_compound_bounds(Point2::ZERO, Vec2::ZERO, bounds);
        assert_eq!(idle, Point2::ZERO);
    }
}
