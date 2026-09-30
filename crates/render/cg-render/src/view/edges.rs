//! Edge paint plans with bundling, routing and culling.
//!
//! The bulk builder and the single-edge rebuild share one geometry path so
//! the retained cache can refresh moved nodes without rebuilding every edge.
//! `pairs` doubles as the bundling context in both entries.

use std::collections::HashMap;

use cg_geometry::segmented_polyline;
use cg_graph::{GraphView, NodeIndex, Positions};
use cg_types::Vec2;

use crate::camera::Camera;
use crate::style::EdgeStyle;

use super::bundles::BundleContext;
pub use super::bundles::{bundle_slot, edge_ordinals_for, loop_ordinal};
use super::culling::{
    edge_visible, loop_visible, point_in_grown_rect, polyline_visible, segment_in_grown_rect,
    spread_for, unordered_key, world_margin_for, world_viewport_rect,
};
pub use super::hits::painted_edge_hits;
use super::plans::{EdgePaintOptions, NODE_SIDE, PaintedEdge};
use super::routing::{
    bezier_control, clean_waypoints, curved_edge, haystack_edge, haystack_span, loop_edge,
    manhattan_bends, manhattan_edge, should_use_haystack, should_use_manhattan, waypoint_edge,
};
use crate::waypoints::WaypointStore;

/// Transforms edges into a paint plan with parallel edges spread as curves.
///
/// This entry iterates every edge and rejects off-screen ones by bounding box,
/// which keeps it suitable as the scale-benchmark baseline. The product path
/// prefers [`paint_edges_for`] over a caller-narrowed visible set; long edges
/// crossing the viewport stay covered because rejection uses the bounding box
/// rather than endpoint visibility. Endpoints missing from `positions` are
/// skipped. Self loops become upward cubic loops sized by the node constant;
/// overlapping loops on one node spread outward in edge order. Opposite
/// directed edges share one bundle so they curve to opposite sides instead of
/// overlapping.
pub fn paint_edges(
    graph: &dyn GraphView,
    positions: &Positions,
    camera: &Camera,
    viewport: Vec2,
    edge_style: impl Fn(NodeIndex, NodeIndex) -> EdgeStyle,
) -> Vec<PaintedEdge> {
    paint_edges_with_options(
        graph,
        positions,
        camera,
        viewport,
        EdgePaintOptions::default(),
        edge_style,
    )
}

/// Transforms edges with detail and aggregation options applied.
///
/// Simplified and minimal levels drop Bezier controls to straight segments;
/// dense bundles fan out as haystack lines; an explicit orthogonal or taxi
/// direction routes edges as polylines with at most two bends. Hit testing
/// stays on the full-precision path so visual downgrades never change
/// selection.
///
/// Edges fully outside the world viewport are rejected before any control
/// point math, so dense off-screen bundles cost only a bounding-box test.
pub fn paint_edges_with_options(
    graph: &dyn GraphView,
    positions: &Positions,
    camera: &Camera,
    viewport: Vec2,
    options: EdgePaintOptions,
    edge_style: impl Fn(NodeIndex, NodeIndex) -> EdgeStyle,
) -> Vec<PaintedEdge> {
    let mut edges = graph.edges();
    edges.sort_unstable_by_key(|(source, target)| (source.index(), target.index()));
    paint_edges_for(&edges, positions, camera, viewport, options, edge_style)
}

/// Edge plan for an explicit sorted pair list.
///
/// `pairs` doubles as the bundling context: parallel offsets and haystack
/// slots derive from the full list, so callers pass every edge even when they
/// only expect a visible subset back.
pub fn paint_edges_for(
    pairs: &[(NodeIndex, NodeIndex)],
    positions: &Positions,
    camera: &Camera,
    viewport: Vec2,
    options: EdgePaintOptions,
    edge_style: impl Fn(NodeIndex, NodeIndex) -> EdgeStyle,
) -> Vec<PaintedEdge> {
    paint_edges_for_with_waypoints(
        pairs,
        positions,
        camera,
        viewport,
        options,
        &WaypointStore::new(),
        edge_style,
    )
}

/// Edge plan honoring user waypoints in world coordinates.
///
/// Edges with a stored waypoint sequence route directly through the cleaned
/// screen points, skipping parallel offsets, haystack simplification and
/// Manhattan derivation. Self loops ignore waypoints and keep loop geometry.
/// Edges without waypoints follow the default derivation unchanged.
pub fn paint_edges_for_with_waypoints(
    pairs: &[(NodeIndex, NodeIndex)],
    positions: &Positions,
    camera: &Camera,
    viewport: Vec2,
    options: EdgePaintOptions,
    waypoints: &WaypointStore,
    edge_style: impl Fn(NodeIndex, NodeIndex) -> EdgeStyle,
) -> Vec<PaintedEdge> {
    let context = BundleContext::build(pairs);
    let world_rect = world_viewport_rect(camera, viewport);
    let margin = world_margin_for(camera);
    let mut loops_seen: HashMap<usize, usize> = HashMap::new();
    let mut occurrence: HashMap<(usize, usize), usize> = HashMap::new();
    let mut painted = Vec::new();
    for (ordinal, (source, target)) in pairs.iter().enumerate() {
        let style = edge_style(*source, *target);
        let directed = (source.index(), target.index());
        let seen = occurrence.get(&directed).copied().unwrap_or(0);
        occurrence.insert(directed, seen + 1);
        if source == target {
            let Some(anchor) = positions.get(source) else {
                continue;
            };
            if !point_in_grown_rect(*anchor, world_rect, margin + NODE_SIDE) {
                continue;
            }
            let screen = camera.world_to_viewport(viewport, *anchor);
            let stack = loops_seen.get(&source.index()).copied().unwrap_or(0);
            loops_seen.insert(source.index(), stack + 1);
            let entry = loop_edge(*source, *target, screen, stack, style);
            if loop_visible(screen, entry.loop_ctrls.unwrap_or([screen; 2]), viewport) {
                painted.push(entry);
            }
            continue;
        }
        let (Some(a), Some(b)) = (positions.get(source), positions.get(target)) else {
            continue;
        };
        let raw = waypoints.get(*source, *target, seen);
        if !raw.is_empty() {
            let start = camera.world_to_viewport(viewport, *a);
            let end = camera.world_to_viewport(viewport, *b);
            let screen_raw: Vec<cg_types::Point2> = raw
                .iter()
                .map(|point| camera.world_to_viewport(viewport, *point))
                .collect();
            let bends = clean_waypoints(start, &screen_raw, end);
            let mut line = Vec::with_capacity(bends.len() + 2);
            line.push(start);
            line.extend_from_slice(&bends);
            line.push(end);
            if polyline_visible(&line, viewport) {
                painted.push(waypoint_edge(*source, *target, start, end, bends, style));
            }
            continue;
        }
        let key = unordered_key(*source, *target);
        let bundle_len = context.bundle_len(key);
        if !segment_in_grown_rect(*a, *b, world_rect, margin + spread_for(bundle_len)) {
            continue;
        }
        let start = camera.world_to_viewport(viewport, *a);
        let end = camera.world_to_viewport(viewport, *b);
        let slot = context.slot(key, ordinal);
        if should_use_haystack(options, bundle_len) {
            let (fanned_a, fanned_b) = haystack_span(start, end, *source, *target, slot);
            if edge_visible(fanned_a, fanned_b, None, viewport) {
                painted.push(haystack_edge(*source, *target, fanned_a, fanned_b, style));
            }
            continue;
        }
        if should_use_manhattan(options) {
            let (bend_a, bend_b) = manhattan_bends(start, end, options);
            let mut via = Vec::new();
            if let Some(bend) = bend_a {
                via.push(bend);
            }
            if let Some(bend) = bend_b {
                via.push(bend);
            }
            let line = segmented_polyline(start, &via, end);
            if polyline_visible(&line, viewport) {
                painted.push(manhattan_edge(*source, *target, start, end, via, style));
            }
            continue;
        }
        let ctrl = bezier_control(start, end, *source, *target, slot, bundle_len, options);
        if edge_visible(start, end, ctrl, viewport) {
            painted.push(curved_edge(*source, *target, start, end, ctrl, style));
        }
    }
    painted
}

/// Edge plan for one pair-list entry, or nothing when it is missing or culled.
///
/// The result agrees with [`paint_edges_for`] for the same ordinal, so the
/// retained cache can refresh moved nodes without rebuilding every edge.
pub fn paint_single_edge(
    pairs: &[(NodeIndex, NodeIndex)],
    ordinal: usize,
    positions: &Positions,
    camera: &Camera,
    viewport: Vec2,
    options: EdgePaintOptions,
    edge_style: impl Fn(NodeIndex, NodeIndex) -> EdgeStyle,
) -> Option<PaintedEdge> {
    paint_single_edge_with_waypoints(
        pairs,
        ordinal,
        positions,
        camera,
        viewport,
        options,
        &WaypointStore::new(),
        edge_style,
    )
}

/// Single-edge plan honoring user waypoints.
///
/// The result agrees with [`paint_edges_for_with_waypoints`] for the same
/// ordinal, so the retained cache can refresh moved nodes without rebuilding
/// every edge.
pub fn paint_single_edge_with_waypoints(
    pairs: &[(NodeIndex, NodeIndex)],
    ordinal: usize,
    positions: &Positions,
    camera: &Camera,
    viewport: Vec2,
    options: EdgePaintOptions,
    waypoints: &WaypointStore,
    edge_style: impl Fn(NodeIndex, NodeIndex) -> EdgeStyle,
) -> Option<PaintedEdge> {
    let (source, target) = *pairs.get(ordinal)?;
    let style = edge_style(source, target);
    let world_rect = world_viewport_rect(camera, viewport);
    let margin = world_margin_for(camera);
    if source == target {
        let anchor = positions.get(&source)?;
        if !point_in_grown_rect(*anchor, world_rect, margin + NODE_SIDE) {
            return None;
        }
        let screen = camera.world_to_viewport(viewport, *anchor);
        let entry = loop_edge(source, target, screen, loop_ordinal(pairs, ordinal), style);
        if !loop_visible(screen, entry.loop_ctrls.unwrap_or([screen; 2]), viewport) {
            return None;
        }
        return Some(entry);
    }
    let (a, b) = match (positions.get(&source), positions.get(&target)) {
        (Some(a), Some(b)) => (*a, *b),
        _ => return None,
    };
    let occurrence = pairs
        .iter()
        .take(ordinal)
        .filter(|(a, b)| *a == source && *b == target)
        .count();
    let raw = waypoints.get(source, target, occurrence);
    if !raw.is_empty() {
        let start = camera.world_to_viewport(viewport, a);
        let end = camera.world_to_viewport(viewport, b);
        let screen_raw: Vec<cg_types::Point2> = raw
            .iter()
            .map(|point| camera.world_to_viewport(viewport, *point))
            .collect();
        let bends = clean_waypoints(start, &screen_raw, end);
        let mut line = Vec::with_capacity(bends.len() + 2);
        line.push(start);
        line.extend_from_slice(&bends);
        line.push(end);
        if !polyline_visible(&line, viewport) {
            return None;
        }
        return Some(waypoint_edge(source, target, start, end, bends, style));
    }
    let (slot, bundle_len) = bundle_slot(pairs, ordinal);
    if !segment_in_grown_rect(a, b, world_rect, margin + spread_for(bundle_len)) {
        return None;
    }
    let start = camera.world_to_viewport(viewport, a);
    let end = camera.world_to_viewport(viewport, b);
    if should_use_haystack(options, bundle_len) {
        let (fanned_a, fanned_b) = haystack_span(start, end, source, target, slot);
        if !edge_visible(fanned_a, fanned_b, None, viewport) {
            return None;
        }
        return Some(haystack_edge(source, target, fanned_a, fanned_b, style));
    }
    if should_use_manhattan(options) {
        let (bend_a, bend_b) = manhattan_bends(start, end, options);
        let mut via = Vec::new();
        if let Some(bend) = bend_a {
            via.push(bend);
        }
        if let Some(bend) = bend_b {
            via.push(bend);
        }
        let line = segmented_polyline(start, &via, end);
        if !polyline_visible(&line, viewport) {
            return None;
        }
        return Some(manhattan_edge(source, target, start, end, via, style));
    }
    let ctrl = bezier_control(start, end, source, target, slot, bundle_len, options);
    if !edge_visible(start, end, ctrl, viewport) {
        return None;
    }
    Some(curved_edge(source, target, start, end, ctrl, style))
}

#[cfg(test)]
mod tests {
    use cg_geometry::OrthoDirection;
    use cg_graph::MockGraph;
    use cg_types::{Point2, Rect};

    use super::*;
    use crate::lod::DetailLevel;

    fn viewport() -> Vec2 {
        Vec2::new(1024.0, 768.0)
    }

    fn camera() -> Camera {
        Camera::new(Point2::ZERO, 1.0)
    }

    fn edge_style(_source: NodeIndex, _target: NodeIndex) -> EdgeStyle {
        EdgeStyle::default()
    }

    #[test]
    fn edges_skip_endpoints_without_positions() {
        let graph = MockGraph::chain(3);
        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(-900.0, -900.0));
        positions.insert(NodeIndex::new(1), Point2::new(-880.0, -900.0));
        let edges = paint_edges(&graph, &positions, &camera(), viewport(), edge_style);
        assert!(edges.is_empty());
    }

    #[test]
    fn lone_edge_stays_straight() {
        let graph = MockGraph::chain(2);
        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(-400.0, 0.0));
        positions.insert(NodeIndex::new(1), Point2::new(-300.0, 0.0));
        let edges = paint_edges(&graph, &positions, &camera(), viewport(), edge_style);
        assert_eq!(edges.len(), 1);
        assert!(edges[0].ctrl.is_none());
        assert!(edges[0].loop_ctrls.is_none());
    }

    #[test]
    fn opposite_edges_curve_to_opposite_sides() {
        let mut graph = MockGraph::chain(2);
        graph.push_edge(1, 0);
        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(-400.0, 0.0));
        positions.insert(NodeIndex::new(1), Point2::new(-300.0, 0.0));
        let edges = paint_edges(&graph, &positions, &camera(), viewport(), edge_style);
        assert_eq!(edges.len(), 2);
        let ctrls: Vec<Option<Point2>> = edges.iter().map(|edge| edge.ctrl).collect();
        assert!(ctrls.iter().all(|ctrl| ctrl.is_some()));
        let first = ctrls[0].unwrap_or(Point2::ZERO);
        let second = ctrls[1].unwrap_or(Point2::ZERO);
        assert!((first.y - 384.0).abs() > 0.5);
        assert!((first.y + second.y - 2.0 * 384.0).abs() < 1e-3);
    }

    #[test]
    fn self_loop_becomes_an_upward_cubic() {
        let mut graph = MockGraph::isolated(1);
        graph.push_edge(0, 0);
        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(0.0, 0.0));
        let edges = paint_edges(&graph, &positions, &camera(), viewport(), edge_style);
        assert_eq!(edges.len(), 1);
        let [ctrl_a, ctrl_b] = edges[0].loop_ctrls.unwrap_or([Point2::ZERO; 2]);
        assert_eq!(edges[0].start, edges[0].end);
        assert!(ctrl_a.y < edges[0].start.y && ctrl_b.y < edges[0].start.y);
    }

    #[test]
    fn edge_opacity_rides_on_the_resolved_style() {
        let graph = MockGraph::chain(2);
        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(-400.0, 0.0));
        positions.insert(NodeIndex::new(1), Point2::new(-300.0, 0.0));
        let edges = paint_edges(&graph, &positions, &camera(), viewport(), |_, _| {
            EdgeStyle {
                opacity: 0.25,
                ..EdgeStyle::default()
            }
        });
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].opacity, 0.25);
    }

    #[test]
    fn painted_edge_hit_covers_straight_curved_and_loop() {
        use crate::arrows::ArrowKind;

        let straight = PaintedEdge {
            source: NodeIndex::new(0),
            target: NodeIndex::new(1),
            start: Point2::new(0.0, 0.0),
            end: Point2::new(10.0, 0.0),
            ctrl: None,
            loop_ctrls: None,
            bends: Vec::new(),
            aggregated: false,
            tint: 0,
            width: 1.0,
            opacity: 1.0,
            arrow: ArrowKind::Triangle,
            arrow_scale: 1.0,
        };
        let rect = Rect::new(Point2::new(4.0, -1.0), Vec2::new(2.0, 2.0));
        assert!(painted_edge_hits(&straight, rect));
        let far = Rect::new(Point2::new(4.0, 50.0), Vec2::new(2.0, 2.0));
        assert!(!painted_edge_hits(&straight, far));
        let curved = PaintedEdge {
            ctrl: Some(Point2::new(5.0, 10.0)),
            ..straight
        };
        let bulge = Rect::new(Point2::new(3.0, 3.0), Vec2::new(4.0, 4.0));
        assert!(painted_edge_hits(&curved, bulge));
    }

    #[test]
    fn simplified_level_drops_bezier_controls() {
        use super::super::heads::paint_arrows_for_level;

        let mut graph = MockGraph::chain(2);
        graph.push_edge(1, 0);
        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(-400.0, 0.0));
        positions.insert(NodeIndex::new(1), Point2::new(-300.0, 0.0));
        let options = EdgePaintOptions {
            level: DetailLevel::Simplified,
            ..EdgePaintOptions::default()
        };
        let edges = paint_edges_with_options(
            &graph,
            &positions,
            &camera(),
            viewport(),
            options,
            edge_style,
        );
        assert_eq!(edges.len(), 2);
        assert!(edges.iter().all(|edge| edge.ctrl.is_none()));
        assert!(paint_arrows_for_level(&edges, DetailLevel::Minimal).is_empty());
        assert_eq!(paint_arrows_for_level(&edges, DetailLevel::Full).len(), 2);
    }

    #[test]
    fn dense_bundle_renders_as_haystack_without_controls() {
        let mut graph = MockGraph::isolated(2);
        for _ in 0..6 {
            graph.push_edge(0, 1);
        }
        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(-400.0, 0.0));
        positions.insert(NodeIndex::new(1), Point2::new(-300.0, 0.0));
        let options = EdgePaintOptions {
            force_haystack: true,
            ..EdgePaintOptions::default()
        };
        let edges = paint_edges_with_options(
            &graph,
            &positions,
            &camera(),
            viewport(),
            options,
            edge_style,
        );
        assert_eq!(edges.len(), 6);
        assert!(edges.iter().all(|edge| edge.ctrl.is_none()));
        assert!(edges.iter().all(|edge| edge.aggregated));
        let repeated = paint_edges_with_options(
            &graph,
            &positions,
            &camera(),
            viewport(),
            options,
            edge_style,
        );
        assert_eq!(
            edges.iter().map(|edge| edge.start).collect::<Vec<_>>(),
            repeated.iter().map(|edge| edge.start).collect::<Vec<_>>()
        );
    }

    #[test]
    fn ortho_option_routes_with_capped_bends() {
        let graph = MockGraph::chain(2);
        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(-400.0, 0.0));
        positions.insert(NodeIndex::new(1), Point2::new(-300.0, 40.0));
        let options = EdgePaintOptions {
            ortho: Some(OrthoDirection::Auto),
            ..EdgePaintOptions::default()
        };
        let edges = paint_edges_with_options(
            &graph,
            &positions,
            &camera(),
            viewport(),
            options,
            edge_style,
        );
        assert_eq!(edges.len(), 1);
        assert!(edges[0].bends().len() <= 2);
        assert!(!edges[0].aggregated);
        let rect = Rect::from_corners(edges[0].start, edges[0].end);
        assert!(painted_edge_hits(&edges[0], rect));
    }

    #[test]
    fn taxi_option_routes_through_a_single_corner() {
        let graph = MockGraph::chain(2);
        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(-400.0, 0.0));
        positions.insert(NodeIndex::new(1), Point2::new(-300.0, 40.0));
        let options = EdgePaintOptions {
            taxi: Some(OrthoDirection::HorizontalFirst),
            ..EdgePaintOptions::default()
        };
        let edges = paint_edges_with_options(
            &graph,
            &positions,
            &camera(),
            viewport(),
            options,
            edge_style,
        );
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].bends().len(), 1);
        assert!(!edges[0].aggregated);
        let corner = edges[0].bends()[0];
        assert_eq!(corner, Point2::new(edges[0].end.x, edges[0].start.y));
        assert!(painted_edge_hits(
            &edges[0],
            Rect::from_corners(edges[0].start, corner)
        ));
    }

    #[test]
    fn single_edge_rebuild_matches_the_bulk_plan() {
        let mut graph = MockGraph::chain(3);
        graph.push_edge(1, 0);
        graph.push_edge(2, 2);
        let mut pairs = graph.edges();
        pairs.sort_unstable_by_key(|(source, target)| (source.index(), target.index()));
        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(-400.0, 0.0));
        positions.insert(NodeIndex::new(1), Point2::new(-300.0, 0.0));
        positions.insert(NodeIndex::new(2), Point2::new(-200.0, 40.0));
        let options = EdgePaintOptions::default();
        let bulk = paint_edges_for(
            &pairs,
            &positions,
            &camera(),
            viewport(),
            options,
            edge_style,
        );
        assert_eq!(bulk.len(), pairs.len());
        for (ordinal, pair) in pairs.iter().enumerate() {
            let single = paint_single_edge(
                &pairs,
                ordinal,
                &positions,
                &camera(),
                viewport(),
                options,
                edge_style,
            )
            .unwrap_or_else(|| panic!("pair {pair:?} stays visible"));
            let from_bulk = bulk
                .iter()
                .find(|edge| edge.source == pair.0 && edge.target == pair.1)
                .cloned()
                .unwrap_or_else(|| panic!("bulk keeps {pair:?}"));
            assert_eq!(single.start, from_bulk.start);
            assert_eq!(single.end, from_bulk.end);
            assert_eq!(single.ctrl, from_bulk.ctrl);
            assert_eq!(single.loop_ctrls, from_bulk.loop_ctrls);
        }
        assert!(
            paint_single_edge(
                &pairs,
                99,
                &positions,
                &camera(),
                viewport(),
                options,
                edge_style
            )
            .is_none()
        );
    }

    #[test]
    fn edge_ordinals_track_parallel_edges_and_loops() {
        let mut graph = MockGraph::isolated(2);
        graph.push_edge(0, 1);
        graph.push_edge(0, 1);
        graph.push_edge(0, 0);
        let mut pairs = graph.edges();
        pairs.sort_unstable_by_key(|(source, target)| (source.index(), target.index()));
        assert_eq!(
            pairs,
            vec![
                (NodeIndex::new(0), NodeIndex::new(0)),
                (NodeIndex::new(0), NodeIndex::new(1)),
                (NodeIndex::new(0), NodeIndex::new(1)),
            ]
        );
        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(-400.0, 0.0));
        positions.insert(NodeIndex::new(1), Point2::new(-300.0, 0.0));
        let edges = paint_edges_for(
            &pairs,
            &positions,
            &camera(),
            viewport(),
            EdgePaintOptions::default(),
            edge_style,
        );
        assert_eq!(edges.len(), 3);
        assert_eq!(edge_ordinals_for(&edges, &pairs), vec![0, 1, 2]);
    }

    #[test]
    fn painted_loop_hit_matches_the_upward_geometry() {
        let mut graph = MockGraph::isolated(1);
        graph.push_edge(0, 0);
        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(0.0, 0.0));
        let loops = paint_edges(&graph, &positions, &camera(), viewport(), |_, _| {
            EdgeStyle::default()
        });
        assert_eq!(loops.len(), 1);
        let above = Rect::new(Point2::new(412.0, 284.0), Vec2::new(200.0, 80.0));
        assert!(painted_edge_hits(&loops[0], above));
        let below = Rect::new(Point2::new(412.0, 500.0), Vec2::new(200.0, 80.0));
        assert!(!painted_edge_hits(&loops[0], below));
    }

    #[test]
    fn waypoint_edge_routes_directly_and_hits_consistently() {
        use crate::waypoints::WaypointStore;

        let graph = MockGraph::chain(2);
        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(-400.0, 0.0));
        positions.insert(NodeIndex::new(1), Point2::new(-300.0, 40.0));
        let mut pairs = graph.edges();
        pairs.sort_unstable_by_key(|(source, target)| (source.index(), target.index()));
        let plain = paint_edges_for(
            &pairs,
            &positions,
            &camera(),
            viewport(),
            EdgePaintOptions::default(),
            edge_style,
        );
        assert_eq!(plain.len(), 1);
        assert!(plain[0].bends().is_empty());
        let mut store = WaypointStore::new();
        store.set_single(
            NodeIndex::new(0),
            NodeIndex::new(1),
            vec![Point2::new(-370.0, -30.0), Point2::new(-330.0, 60.0)],
        );
        let routed = paint_edges_for_with_waypoints(
            &pairs,
            &positions,
            &camera(),
            viewport(),
            EdgePaintOptions::default(),
            &store,
            edge_style,
        );
        assert_eq!(routed.len(), 1);
        assert_eq!(routed[0].bends().len(), 2);
        assert!(routed[0].ctrl.is_none());
        assert!(painted_edge_hits(
            &routed[0],
            Rect::from_corners(routed[0].start, routed[0].bends()[0])
        ));
        let single = paint_single_edge_with_waypoints(
            &pairs,
            0,
            &positions,
            &camera(),
            viewport(),
            EdgePaintOptions::default(),
            &store,
            edge_style,
        )
        .expect("waypoint edge stays visible");
        assert_eq!(single.bends(), routed[0].bends());
    }

    #[test]
    fn waypoint_cleaning_degrades_to_straight() {
        use crate::waypoints::WaypointStore;

        let graph = MockGraph::chain(2);
        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(-400.0, 0.0));
        positions.insert(NodeIndex::new(1), Point2::new(-300.0, 0.0));
        let mut pairs = graph.edges();
        pairs.sort_unstable_by_key(|(source, target)| (source.index(), target.index()));
        let mut store = WaypointStore::new();
        store.set_single(
            NodeIndex::new(0),
            NodeIndex::new(1),
            vec![Point2::new(f32::NAN, 0.0), Point2::new(-400.0, 0.0)],
        );
        let routed = paint_edges_for_with_waypoints(
            &pairs,
            &positions,
            &camera(),
            viewport(),
            EdgePaintOptions::default(),
            &store,
            edge_style,
        );
        assert_eq!(routed.len(), 1);
        assert!(routed[0].bends().is_empty());
    }

    #[test]
    fn waypoints_skip_haystack_and_manhattan_derivation() {
        use crate::waypoints::WaypointStore;

        let mut graph = MockGraph::isolated(2);
        for _ in 0..6 {
            graph.push_edge(0, 1);
        }
        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(-400.0, 0.0));
        positions.insert(NodeIndex::new(1), Point2::new(-300.0, 40.0));
        let mut pairs = graph.edges();
        pairs.sort_unstable_by_key(|(source, target)| (source.index(), target.index()));
        let mut store = WaypointStore::new();
        store.set(
            NodeIndex::new(0),
            NodeIndex::new(1),
            0,
            vec![Point2::new(-350.0, -40.0)],
        );
        let options = EdgePaintOptions {
            force_haystack: true,
            ortho: Some(OrthoDirection::Auto),
            ..EdgePaintOptions::default()
        };
        let routed = paint_edges_for_with_waypoints(
            &pairs,
            &positions,
            &camera(),
            viewport(),
            options,
            &store,
            edge_style,
        );
        assert_eq!(routed.len(), 6);
        let first = routed
            .iter()
            .find(|edge| !edge.bends().is_empty())
            .expect("first parallel edge keeps waypoints");
        assert_eq!(first.bends().len(), 1);
        assert!(!first.aggregated);
    }
}
