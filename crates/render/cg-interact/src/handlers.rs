//! Pure translation of pointer positions into graph state changes.
//!
//! These helpers stay free of gpui event types so they can be unit tested
//! headlessly; the application converts platform events into the model-space
//! points these functions consume.

use std::collections::{BTreeSet, HashMap, VecDeque};

use cg_geometry::{
    BEZIER_HIT_SAMPLES, OrthoDirection, bezier_control_for_edge, manhattan_route, parallel_offsets,
    polyline_intersects_rect, sample_quadratic_bezier, segment_intersects_rect, self_loop_polyline,
};
use cg_graph::{GraphStore, GraphView, NodeIndex, Positions};
use cg_render::{
    NODE_SIDE, NodeShape, PARALLEL_STEP, SpatialIndex, point_hits_shape, shape_hits_rect,
};
use cg_types::{Point2, Rect, Vec2};

use super::input::{InteractLocks, SelectMode, SelectionState};

/// Half extent of a node body in model units, derived from the paint plan.
pub const NODE_HALF_EXTENT: f32 = NODE_SIDE / 2.0;

/// Pointer tolerance for node grabs, in model units.
pub const NODE_GRAB_TOLERANCE: f32 = 6.0;

/// Finds the node under `world_point` honoring per-node shapes.
///
/// Candidates are tested nearest first; the square body stays the conservative
/// outer envelope inside [`point_hits_shape`], and box selection shares the
/// same shape test, so taps and box selects agree.
pub fn press_hit_shaped(
    world_point: Point2,
    positions: &Positions,
    index: &SpatialIndex,
    radius: f32,
    shape_of: impl Fn(NodeIndex) -> NodeShape,
) -> Option<(NodeIndex, Vec2)> {
    let candidates = index.query_point(world_point, radius);
    let mut ordered: Vec<(f32, NodeIndex, Point2)> = Vec::new();
    for node in candidates {
        if let Some(center) = positions.get(&node).copied() {
            let distance = (world_point - center).length_squared();
            ordered.push((distance, node, center));
        }
    }
    ordered.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    for (_, node, center) in ordered {
        if point_hits_shape(
            shape_of(node),
            world_point,
            center,
            NODE_HALF_EXTENT,
            NODE_GRAB_TOLERANCE,
        ) {
            return Some((node, world_point - center));
        }
    }
    None
}

/// Node under `world_point` honoring per-node shapes, for hover highlighting.
pub fn hover_node_shaped(
    world_point: Point2,
    positions: &Positions,
    index: &SpatialIndex,
    radius: f32,
    shape_of: impl Fn(NodeIndex) -> NodeShape,
) -> Option<NodeIndex> {
    press_hit_shaped(world_point, positions, index, radius, shape_of).map(|(node, _)| node)
}

/// New center of a dragged node from the pointer and the initial grab offset.
pub fn drag_position(pointer_world: Point2, grab_offset: Vec2) -> Point2 {
    Point2::new(
        pointer_world.x - grab_offset.x,
        pointer_world.y - grab_offset.y,
    )
}

/// Normalized selection rectangle spanning a viewport drag.
///
/// Corners arrive in any order; the result always has non-negative size.
pub fn normalize_drag(start: Point2, current: Point2) -> Rect {
    Rect::from_corners(start, current)
}

/// Nodes whose bodies touch `rect`, in index order.
///
/// The spatial index narrows candidates and each body is tested with its own
/// shape, so box selection agrees with pointer hit testing: corners a tap
/// rejects stay unselected here as well. The query grows by the body extent
/// because the index keys on centers; the exact shape test still rejects
/// non-overlapping bodies, so growth only adds candidates.
pub fn nodes_in_rect(
    positions: &Positions,
    index: &SpatialIndex,
    rect: Rect,
    half_extent: f32,
    shape_of: impl Fn(NodeIndex) -> NodeShape,
) -> Vec<NodeIndex> {
    let grown = Rect::new(
        Point2::new(rect.origin.x - half_extent, rect.origin.y - half_extent),
        Vec2::new(
            rect.size.x + half_extent * 2.0,
            rect.size.y + half_extent * 2.0,
        ),
    );
    let mut found: Vec<NodeIndex> = index
        .query_rect(grown)
        .into_iter()
        .filter(|node| {
            positions
                .get(node)
                .map(|center| shape_hits_rect(shape_of(*node), *center, half_extent, rect))
                .unwrap_or(false)
        })
        .collect();
    found.sort_unstable_by_key(|node| node.index());
    found
}

/// True when the edge between `start` and `end` touches `rect`.
///
/// Curved edges are flattened with the same sampling as hit testing, so box
/// selection and pointer hits agree on curved shapes.
pub fn edge_hits_rect(start: Point2, ctrl: Option<Point2>, end: Point2, rect: Rect) -> bool {
    match ctrl {
        None => segment_intersects_rect(start, end, rect),
        Some(mid) => {
            let samples = sample_quadratic_bezier(start, mid, end, BEZIER_HIT_SAMPLES);
            polyline_intersects_rect(&samples, rect)
        }
    }
}

/// True when the self loop anchored at `center` touches `rect`.
///
/// The loop is flattened with the shared loop geometry, so selection matches
/// what the canvas paints.
pub fn loop_hits_rect(center: Point2, node_side: f32, ordinal: usize, rect: Rect) -> bool {
    polyline_intersects_rect(&self_loop_polyline(center, node_side, ordinal), rect)
}

/// Directed edges touching `rect`, in (source, target) order.
///
/// Straight and curved edges share the paint plan bundling, and self loops
/// reuse the painted loop geometry, so box selection matches the canvas.
/// Parallel edges in one direction collapse to a single entry.
pub fn edges_in_rect(
    graph: &dyn GraphView,
    positions: &Positions,
    rect: Rect,
    node_side: f32,
) -> Vec<(NodeIndex, NodeIndex)> {
    edges_in_rect_with_options(graph, positions, rect, node_side, None, None)
}

/// Directed edges touching `rect` under explicit Manhattan routing.
///
/// The route selector is the same one the paint plan uses, so taxi and
/// orthogonal edges test their routed polylines instead of the straight
/// chord. Callers pass the options the canvas paints with; the default entry
/// above covers the common unset case.
pub fn edges_in_rect_with_options(
    graph: &dyn GraphView,
    positions: &Positions,
    rect: Rect,
    node_side: f32,
    ortho: Option<OrthoDirection>,
    taxi: Option<OrthoDirection>,
) -> Vec<(NodeIndex, NodeIndex)> {
    let mut edges = graph.edges();
    edges.sort_unstable_by_key(|(source, target)| (source.index(), target.index()));
    let mut bundle_of: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    for (ordinal, (source, target)) in edges.iter().enumerate() {
        bundle_of
            .entry(bundle_key(*source, *target))
            .or_default()
            .push(ordinal);
    }
    let mut loops_seen: HashMap<usize, usize> = HashMap::new();
    let mut found = Vec::new();
    for (ordinal, (source, target)) in edges.iter().enumerate() {
        if source == target {
            let Some(center) = positions.get(source) else {
                continue;
            };
            let seen = loops_seen.get(&source.index()).copied().unwrap_or(0);
            loops_seen.insert(source.index(), seen + 1);
            if loop_hits_rect(*center, node_side, seen, rect) {
                found.push((*source, *target));
            }
            continue;
        }
        let (Some(start), Some(end)) = (positions.get(source), positions.get(target)) else {
            continue;
        };
        let routed = manhattan_route(*start, *end, ortho, taxi);
        if !routed.is_empty() {
            if polyline_intersects_rect(&routed, rect) {
                found.push((*source, *target));
            }
            continue;
        }
        let bundle = bundle_of
            .get(&bundle_key(*source, *target))
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let slot = bundle
            .iter()
            .position(|member| *member == ordinal)
            .unwrap_or(0);
        let offsets = parallel_offsets(bundle.len(), PARALLEL_STEP);
        let mut offset = offsets.get(slot).copied().unwrap_or(0.0);
        if source.index() > target.index() {
            offset = -offset;
        }
        let ctrl = if offset == 0.0 {
            None
        } else {
            Some(bezier_control_for_edge(*start, *end, offset))
        };
        if edge_hits_rect(*start, ctrl, *end, rect) {
            found.push((*source, *target));
        }
    }
    found.sort_unstable_by_key(|(source, target)| (source.index(), target.index()));
    found.dedup();
    found
}

fn bundle_key(source: NodeIndex, target: NodeIndex) -> (usize, usize) {
    let (a, b) = (source.index(), target.index());
    if a <= b { (a, b) } else { (b, a) }
}

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

/// Zoom factor for a scroll wheel line delta.
///
/// Positive deltas zoom in one notch per line; the factor compounds, so small
/// trackpad deltas stay smooth while notched wheels move visibly.
pub fn wheel_zoom_factor(line_delta: f32) -> f32 {
    if !line_delta.is_finite() {
        return 1.0;
    }
    1.15f32.powf(-line_delta)
}

/// Nodes within `hops` undirected steps of `seeds`, seeds included.
///
/// The expansion reads only the neighbor queries, so folded or hidden nodes
/// are filtered by the caller. Zero hops return the seeds alone, and an empty
/// seed set stays empty.
pub fn expand_neighborhood(
    graph: &dyn GraphView,
    seeds: impl IntoIterator<Item = NodeIndex>,
    hops: usize,
) -> BTreeSet<NodeIndex> {
    let mut visited: BTreeSet<NodeIndex> = seeds.into_iter().collect();
    if hops == 0 || visited.is_empty() {
        return visited;
    }
    let mut frontier: VecDeque<NodeIndex> = visited.iter().copied().collect();
    let mut depth: HashMap<NodeIndex, usize> = visited.iter().map(|node| (*node, 0)).collect();
    while let Some(node) = frontier.pop_front() {
        let current = depth.get(&node).copied().unwrap_or(0);
        if current >= hops {
            continue;
        }
        for neighbour in graph.neighbors(node) {
            if visited.insert(neighbour) {
                depth.insert(neighbour, current + 1);
                frontier.push_back(neighbour);
            }
        }
    }
    visited
}

/// Directed edges with both endpoints inside `expanded`, in sorted order.
///
/// Restricting to internal edges keeps highlighted edges anchored at tinted
/// nodes on both ends, for any hop depth.
pub fn neighborhood_edges(
    graph: &dyn GraphView,
    expanded: &BTreeSet<NodeIndex>,
) -> Vec<(NodeIndex, NodeIndex)> {
    if expanded.is_empty() {
        return Vec::new();
    }
    let mut found: Vec<(NodeIndex, NodeIndex)> = graph
        .edges()
        .into_iter()
        .filter(|(source, target)| expanded.contains(source) && expanded.contains(target))
        .collect();
    found.sort_unstable_by_key(|(source, target)| (source.index(), target.index()));
    found.dedup();
    found
}

/// Applies one point tap to `selection` under `mode` and the modifier key.
///
/// A held modifier always toggles the tapped node, preserving the existing
/// rubber-band path. Without a modifier, single mode replaces the set while
/// additive mode accumulates.
pub fn apply_point_select(
    selection: &mut SelectionState,
    mode: SelectMode,
    node: NodeIndex,
    additive_modifier: bool,
) {
    if additive_modifier {
        selection.toggle(node);
        return;
    }
    match mode {
        SelectMode::Single => selection.select(node),
        SelectMode::Additive => selection.add(node),
    }
}

/// True when a blank press clears the selection.
///
/// Locking deselect or holding the additive modifier both keep the set.
pub fn should_clear_on_blank(locks: &InteractLocks, additive_modifier: bool) -> bool {
    locks.can_deselect() && !additive_modifier
}

/// True when a node drag gesture may start under `locks`.
pub fn can_begin_drag(locks: &InteractLocks) -> bool {
    locks.can_drag()
}

/// True when a press may run node hit testing under `locks`.
pub fn can_grab_node(locks: &InteractLocks) -> bool {
    locks.can_grab()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn indexed(positions: &Positions) -> SpatialIndex {
        let mut index = SpatialIndex::new(32.0);
        index.rebuild(positions);
        index
    }

    #[test]
    fn press_finds_the_node_under_the_pointer() {
        use cg_render::NodeShape;

        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(0.0, 0.0));
        positions.insert(NodeIndex::new(1), Point2::new(100.0, 0.0));
        let index = indexed(&positions);
        let hit = press_hit_shaped(Point2::new(103.0, 1.0), &positions, &index, 24.0, |_| {
            NodeShape::Square
        });
        assert!(hit.map(|(node, _)| node) == Some(NodeIndex::new(1)));
    }

    #[test]
    fn press_misses_empty_space() {
        use cg_render::NodeShape;

        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(0.0, 0.0));
        let index = indexed(&positions);
        assert!(
            press_hit_shaped(Point2::new(200.0, 200.0), &positions, &index, 24.0, |_| {
                NodeShape::Square
            })
            .is_none()
        );
    }

    #[test]
    fn drag_keeps_the_grab_offset() {
        let moved = drag_position(Point2::new(10.0, 10.0), Vec2::new(2.0, 1.0));
        assert_eq!(moved, Point2::new(8.0, 9.0));
    }

    #[test]
    fn wheel_factor_zooms_in_on_negative_delta() {
        assert!(wheel_zoom_factor(-1.0) > 1.0);
        assert!(wheel_zoom_factor(1.0) < 1.0);
        assert_eq!(wheel_zoom_factor(f32::NAN), 1.0);
    }

    #[test]
    fn drag_normalization_orders_corners() {
        let rect = normalize_drag(Point2::new(9.0, 1.0), Point2::new(2.0, 7.0));
        assert_eq!(rect.origin, Point2::new(2.0, 1.0));
        assert_eq!(rect.size, Vec2::new(7.0, 6.0));
    }

    #[test]
    fn rect_select_picks_nodes_by_body_overlap() {
        use cg_render::NodeShape;

        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(0.0, 0.0));
        positions.insert(NodeIndex::new(1), Point2::new(100.0, 0.0));
        positions.insert(NodeIndex::new(2), Point2::new(200.0, 0.0));
        let index = indexed(&positions);
        let rect = Rect::from_corners(Point2::new(-20.0, -20.0), Point2::new(112.0, 20.0));
        assert_eq!(
            nodes_in_rect(&positions, &index, rect, NODE_HALF_EXTENT, |_| {
                NodeShape::Square
            }),
            vec![NodeIndex::new(0), NodeIndex::new(1)]
        );
    }

    #[test]
    fn rect_select_rejects_corners_a_tap_rejects() {
        use cg_render::NodeShape;

        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::ZERO);
        let index = indexed(&positions);
        let corner = Rect::from_corners(Point2::new(-12.0, -12.0), Point2::new(-8.0, -8.0));
        assert_eq!(
            nodes_in_rect(&positions, &index, corner, NODE_HALF_EXTENT, |_| {
                NodeShape::Square
            }),
            vec![NodeIndex::new(0)]
        );
        assert!(
            nodes_in_rect(&positions, &index, corner, NODE_HALF_EXTENT, |_| {
                NodeShape::Triangle
            })
            .is_empty()
        );
    }

    #[test]
    fn rect_select_judges_edges_by_intersection() {
        let rect = Rect::new(Point2::new(4.0, -1.0), Vec2::new(2.0, 2.0));
        assert!(edge_hits_rect(
            Point2::new(0.0, 0.0),
            None,
            Point2::new(10.0, 0.0),
            rect
        ));
        assert!(!edge_hits_rect(
            Point2::new(0.0, 5.0),
            None,
            Point2::new(10.0, 5.0),
            rect
        ));
        assert!(edge_hits_rect(
            Point2::new(0.0, 0.0),
            Some(Point2::new(5.0, 10.0)),
            Point2::new(10.0, 0.0),
            Rect::from_corners(Point2::new(3.0, 3.0), Point2::new(7.0, 7.0))
        ));
    }

    #[test]
    fn rect_select_catches_self_loops_above_the_node() {
        let center = Point2::new(0.0, 0.0);
        let above = Rect::from_corners(Point2::new(-30.0, -70.0), Point2::new(30.0, -14.0));
        assert!(loop_hits_rect(center, 24.0, 0, above));
        let below = Rect::from_corners(Point2::new(-30.0, 30.0), Point2::new(30.0, 70.0));
        assert!(!loop_hits_rect(center, 24.0, 0, below));
    }

    #[test]
    fn edges_in_rect_matches_straight_and_loop_edges() {
        use cg_graph::MockGraph;

        let mut graph = MockGraph::chain(3);
        graph.push_edge(2, 2);
        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(0.0, 0.0));
        positions.insert(NodeIndex::new(1), Point2::new(10.0, 0.0));
        positions.insert(NodeIndex::new(2), Point2::new(20.0, 0.0));
        let crossing = Rect::new(Point2::new(4.0, -1.0), Vec2::new(2.0, 2.0));
        assert_eq!(
            edges_in_rect(&graph, &positions, crossing, NODE_SIDE),
            vec![(NodeIndex::new(0), NodeIndex::new(1))]
        );
        let above_loop = Rect::from_corners(Point2::new(-10.0, -70.0), Point2::new(50.0, -14.0));
        assert_eq!(
            edges_in_rect(&graph, &positions, above_loop, NODE_SIDE),
            vec![(NodeIndex::new(2), NodeIndex::new(2))]
        );
        let far = Rect::new(Point2::new(200.0, 200.0), Vec2::new(10.0, 10.0));
        assert!(edges_in_rect(&graph, &positions, far, NODE_SIDE).is_empty());
    }

    #[test]
    fn routed_box_select_follows_the_taxi_corner() {
        use cg_graph::MockGraph;

        let graph = MockGraph::chain(2);
        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(0.0, 0.0));
        positions.insert(NodeIndex::new(1), Point2::new(100.0, 40.0));
        let taxi = Some(OrthoDirection::HorizontalFirst);
        let corner = Rect::from_corners(Point2::new(90.0, -10.0), Point2::new(110.0, 10.0));
        assert_eq!(
            edges_in_rect_with_options(&graph, &positions, corner, NODE_SIDE, None, taxi),
            vec![(NodeIndex::new(0), NodeIndex::new(1))]
        );
        let chord = Rect::from_corners(Point2::new(45.0, 15.0), Point2::new(55.0, 25.0));
        assert!(
            edges_in_rect_with_options(&graph, &positions, chord, NODE_SIDE, None, taxi).is_empty()
        );
        assert_eq!(
            edges_in_rect_with_options(&graph, &positions, chord, NODE_SIDE, None, None),
            vec![(NodeIndex::new(0), NodeIndex::new(1))]
        );
    }

    #[test]
    fn shaped_press_rejects_triangle_corners() {
        use cg_render::NodeShape;

        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(0.0, 0.0));
        let index = indexed(&positions);
        let corner = Point2::new(-11.0, -11.0);
        assert!(
            press_hit_shaped(corner, &positions, &index, 24.0, |_| NodeShape::Square).is_some()
        );
        assert!(
            press_hit_shaped(corner, &positions, &index, 24.0, |_| NodeShape::Triangle).is_none()
        );
        assert!(
            press_hit_shaped(Point2::ZERO, &positions, &index, 24.0, |_| {
                NodeShape::Triangle
            })
            .is_some()
        );
    }

    #[test]
    fn hover_agrees_with_press_hits() {
        use cg_render::NodeShape;

        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(0.0, 0.0));
        let index = indexed(&positions);
        let shape_of = |_| NodeShape::Square;
        assert_eq!(
            hover_node_shaped(Point2::new(2.0, 1.0), &positions, &index, 24.0, shape_of),
            Some(NodeIndex::new(0))
        );
        assert_eq!(
            hover_node_shaped(
                Point2::new(200.0, 200.0),
                &positions,
                &index,
                24.0,
                shape_of
            ),
            None
        );
    }

    #[test]
    fn neighborhood_expands_by_hops_and_clears_with_seeds() {
        use cg_graph::MockGraph;

        let graph = MockGraph::chain(4);
        let seed = [NodeIndex::new(1)];
        let none = expand_neighborhood(&graph, seed, 0);
        assert_eq!(none, BTreeSet::from([NodeIndex::new(1)]));
        let one = expand_neighborhood(&graph, [NodeIndex::new(1)], 1);
        assert_eq!(
            one,
            BTreeSet::from([
                NodeIndex::new(0),
                NodeIndex::new(1),
                NodeIndex::new(2)
            ])
        );
        let two = expand_neighborhood(&graph, [NodeIndex::new(1)], 2);
        assert_eq!(
            two,
            BTreeSet::from([
                NodeIndex::new(0),
                NodeIndex::new(1),
                NodeIndex::new(2),
                NodeIndex::new(3)
            ])
        );
        let empty = expand_neighborhood(&graph, [], 2);
        assert!(empty.is_empty());
        let edges = neighborhood_edges(&graph, &one);
        assert_eq!(
            edges,
            vec![
                (NodeIndex::new(0), NodeIndex::new(1)),
                (NodeIndex::new(1), NodeIndex::new(2))
            ]
        );
        assert!(neighborhood_edges(&graph, &BTreeSet::new()).is_empty());
    }

    #[test]
    fn point_select_modes_and_modifier_combine() {
        use super::super::input::SelectionState;

        let mut selection = SelectionState::default();
        apply_point_select(&mut selection, SelectMode::Single, NodeIndex::new(1), false);
        apply_point_select(&mut selection, SelectMode::Single, NodeIndex::new(2), false);
        assert_eq!(
            selection.iter().collect::<Vec<_>>(),
            vec![NodeIndex::new(2)]
        );
        let mut additive = SelectionState::default();
        apply_point_select(&mut additive, SelectMode::Additive, NodeIndex::new(1), false);
        apply_point_select(&mut additive, SelectMode::Additive, NodeIndex::new(2), false);
        assert_eq!(additive.len(), 2);
        apply_point_select(&mut additive, SelectMode::Single, NodeIndex::new(1), true);
        assert!(!additive.contains(NodeIndex::new(1)));
        assert!(additive.contains(NodeIndex::new(2)));
    }

    #[test]
    fn locks_gate_drag_grab_and_deselect() {
        let open = InteractLocks::default();
        assert!(can_begin_drag(&open));
        assert!(can_grab_node(&open));
        assert!(should_clear_on_blank(&open, false));
        assert!(!should_clear_on_blank(&open, true));
        let locked = InteractLocks {
            lock_drag: true,
            no_grab: true,
            no_deselect: true,
        };
        assert!(!can_begin_drag(&locked));
        assert!(!can_grab_node(&locked));
        assert!(!should_clear_on_blank(&locked, false));
    }
}
