//! Edge paint plans with bundling, routing and culling.
//!
//! The bulk builder and the single-edge rebuild share one geometry path so
//! the retained cache can refresh moved nodes without rebuilding every edge.
//! `pairs` doubles as the bundling context in both entries.

use std::collections::HashMap;

use cg_geometry::{
    BEZIER_HIT_SAMPLES, bezier_control_for_edge, haystack_endpoints, manhattan_route,
    parallel_offsets, polyline_intersects_rect, sample_cubic_bezier, sample_quadratic_bezier,
    segment_intersects_rect, segmented_polyline, self_loop_controls, use_haystack,
};
use cg_graph::{GraphView, NodeIndex, Positions};
use cg_types::{Point2, Rect, Vec2};

use crate::camera::Camera;
use crate::style::EdgeStyle;

use super::culling::{
    edge_visible, loop_visible, point_in_grown_rect, polyline_visible, segment_in_grown_rect,
    spread_for, unordered_key, world_margin_for, world_viewport_rect,
};
use super::plans::{EdgePaintOptions, NODE_SIDE, PARALLEL_STEP, PaintedEdge};

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
    let mut bundle_of: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    for (ordinal, (source, target)) in pairs.iter().enumerate() {
        let key = unordered_key(*source, *target);
        bundle_of.entry(key).or_default().push(ordinal);
    }
    let mut offsets_of: HashMap<(usize, usize), Vec<f32>> = HashMap::new();
    for (key, members) in &bundle_of {
        offsets_of.insert(*key, parallel_offsets(members.len(), PARALLEL_STEP));
    }
    let world_rect = world_viewport_rect(camera, viewport);
    let margin = world_margin_for(camera);
    let mut loops_seen: HashMap<usize, usize> = HashMap::new();
    let mut painted = Vec::new();
    for (ordinal, (source, target)) in pairs.iter().enumerate() {
        let style = edge_style(*source, *target);
        if source == target {
            let Some(anchor) = positions.get(source) else {
                continue;
            };
            if !point_in_grown_rect(*anchor, world_rect, margin + NODE_SIDE) {
                continue;
            }
            let screen = camera.world_to_viewport(viewport, *anchor);
            let loop_ordinal = loops_seen.get(&source.index()).copied().unwrap_or(0);
            loops_seen.insert(source.index(), loop_ordinal + 1);
            let ctrls = self_loop_controls(screen, NODE_SIDE, loop_ordinal);
            if loop_visible(screen, ctrls, viewport) {
                painted.push(PaintedEdge {
                    source: *source,
                    target: *target,
                    start: screen,
                    end: screen,
                    ctrl: None,
                    loop_ctrls: Some(ctrls),
                    bend_a: None,
                    bend_b: None,
                    aggregated: false,
                    tint: style.tint,
                    width: style.width,
                    opacity: style.opacity,
                    arrow: style.arrow,
                    arrow_scale: style.arrow_scale,
                });
            }
            continue;
        }
        let (Some(a), Some(b)) = (positions.get(source), positions.get(target)) else {
            continue;
        };
        let key = unordered_key(*source, *target);
        let bundle_len = bundle_of.get(&key).map(Vec::len).unwrap_or(1);
        if !segment_in_grown_rect(*a, *b, world_rect, margin + spread_for(bundle_len)) {
            continue;
        }
        let mut start = camera.world_to_viewport(viewport, *a);
        let mut end = camera.world_to_viewport(viewport, *b);
        let bundle = bundle_of.get(&key).map(Vec::as_slice).unwrap_or(&[]);
        let slot = bundle
            .iter()
            .position(|member| *member == ordinal)
            .unwrap_or(0);
        let haystack = options.ortho.is_none()
            && options.taxi.is_none()
            && if options.force_haystack {
                use_haystack(bundle.len(), true)
            } else {
                bundle.len() >= options.aggregate_threshold.max(1)
            };
        if haystack {
            let (fanned_a, fanned_b) = haystack_endpoints(
                start,
                end,
                source.index() as u32,
                target.index() as u32,
                slot as u32,
                NODE_SIDE,
            );
            start = fanned_a;
            end = fanned_b;
            if edge_visible(start, end, None, viewport) {
                let mut edge = PaintedEdge::straight(
                    *source,
                    *target,
                    start,
                    end,
                    style.tint,
                    style.width,
                    style.opacity,
                )
                .with_arrow(style.arrow, style.arrow_scale);
                edge.aggregated = true;
                painted.push(edge);
            }
            continue;
        }
        if options.ortho.is_some() || options.taxi.is_some() {
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
                painted.push(PaintedEdge {
                    source: *source,
                    target: *target,
                    start,
                    end,
                    ctrl: None,
                    loop_ctrls: None,
                    bend_a,
                    bend_b,
                    aggregated: false,
                    tint: style.tint,
                    width: style.width,
                    opacity: style.opacity,
                    arrow: style.arrow,
                    arrow_scale: style.arrow_scale,
                });
            }
            continue;
        }
        let offsets = offsets_of.get(&key).map(Vec::as_slice).unwrap_or(&[]);
        let mut offset = offsets.get(slot).copied().unwrap_or(0.0);
        if source.index() > target.index() {
            offset = -offset;
        }
        let ctrl = if offset == 0.0 || !options.level.draws_curves() {
            None
        } else {
            Some(bezier_control_for_edge(start, end, offset))
        };
        if edge_visible(start, end, ctrl, viewport) {
            painted.push(PaintedEdge {
                source: *source,
                target: *target,
                start,
                end,
                ctrl,
                loop_ctrls: None,
                bend_a: None,
                bend_b: None,
                aggregated: false,
                tint: style.tint,
                width: style.width,
                opacity: style.opacity,
                arrow: style.arrow,
                arrow_scale: style.arrow_scale,
            });
        }
    }
    painted
}

/// Bundle slot and size of `pairs[ordinal]` within its unordered bundle.
///
/// The retained cache uses this to rebuild one edge exactly as the bulk path
/// would; unknown ordinals report a lone edge instead of failing.
pub fn bundle_slot(pairs: &[(NodeIndex, NodeIndex)], ordinal: usize) -> (usize, usize) {
    let (source, target) = match pairs.get(ordinal) {
        Some(pair) => *pair,
        None => return (0, 1),
    };
    let key = unordered_key(source, target);
    let mut slot = 0usize;
    let mut len = 0usize;
    for (member, pair) in pairs.iter().enumerate() {
        if unordered_key(pair.0, pair.1) == key {
            if member == ordinal {
                slot = len;
            }
            len += 1;
        }
    }
    (slot, len.max(1))
}

/// Count of earlier self loops on the same node before `pairs[ordinal]`.
pub fn loop_ordinal(pairs: &[(NodeIndex, NodeIndex)], ordinal: usize) -> usize {
    let (source, target) = match pairs.get(ordinal) {
        Some(pair) => *pair,
        None => return 0,
    };
    if source != target {
        return 0;
    }
    pairs
        .iter()
        .take(ordinal)
        .filter(|(a, b)| *a == source && *b == target)
        .count()
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
        let ctrls = self_loop_controls(screen, NODE_SIDE, loop_ordinal(pairs, ordinal));
        if !loop_visible(screen, ctrls, viewport) {
            return None;
        }
        return Some(PaintedEdge {
            source,
            target,
            start: screen,
            end: screen,
            ctrl: None,
            loop_ctrls: Some(ctrls),
            bend_a: None,
            bend_b: None,
            aggregated: false,
            tint: style.tint,
            width: style.width,
            opacity: style.opacity,
            arrow: style.arrow,
            arrow_scale: style.arrow_scale,
        });
    }
    let (a, b) = match (positions.get(&source), positions.get(&target)) {
        (Some(a), Some(b)) => (*a, *b),
        _ => return None,
    };
    let (slot, bundle_len) = bundle_slot(pairs, ordinal);
    if !segment_in_grown_rect(a, b, world_rect, margin + spread_for(bundle_len)) {
        return None;
    }
    let mut start = camera.world_to_viewport(viewport, a);
    let mut end = camera.world_to_viewport(viewport, b);
    let haystack = options.ortho.is_none()
        && options.taxi.is_none()
        && if options.force_haystack {
            use_haystack(bundle_len, true)
        } else {
            bundle_len >= options.aggregate_threshold.max(1)
        };
    if haystack {
        let (fanned_a, fanned_b) = haystack_endpoints(
            start,
            end,
            source.index() as u32,
            target.index() as u32,
            slot as u32,
            NODE_SIDE,
        );
        start = fanned_a;
        end = fanned_b;
        if !edge_visible(start, end, None, viewport) {
            return None;
        }
        let mut edge = PaintedEdge::straight(
            source,
            target,
            start,
            end,
            style.tint,
            style.width,
            style.opacity,
        )
        .with_arrow(style.arrow, style.arrow_scale);
        edge.aggregated = true;
        return Some(edge);
    }
    if options.ortho.is_some() || options.taxi.is_some() {
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
        return Some(PaintedEdge {
            source,
            target,
            start,
            end,
            ctrl: None,
            loop_ctrls: None,
            bend_a,
            bend_b,
            aggregated: false,
            tint: style.tint,
            width: style.width,
            opacity: style.opacity,
            arrow: style.arrow,
            arrow_scale: style.arrow_scale,
        });
    }
    let offsets = parallel_offsets(bundle_len, PARALLEL_STEP);
    let mut offset = offsets.get(slot).copied().unwrap_or(0.0);
    if source.index() > target.index() {
        offset = -offset;
    }
    let ctrl = if offset == 0.0 || !options.level.draws_curves() {
        None
    } else {
        Some(bezier_control_for_edge(start, end, offset))
    };
    if !edge_visible(start, end, ctrl, viewport) {
        return None;
    }
    Some(PaintedEdge {
        source,
        target,
        start,
        end,
        ctrl,
        loop_ctrls: None,
        bend_a: None,
        bend_b: None,
        aggregated: false,
        tint: style.tint,
        width: style.width,
        opacity: style.opacity,
        arrow: style.arrow,
        arrow_scale: style.arrow_scale,
    })
}

/// Pair-list ordinals behind `edges`, in plan order.
///
/// The bulk builder preserves pair order, so the k-th painted edge of one
/// directed pair maps to the k-th pair entry. Used to key retained entries
/// without changing the plan functions' return shapes.
pub fn edge_ordinals_for(edges: &[PaintedEdge], pairs: &[(NodeIndex, NodeIndex)]) -> Vec<usize> {
    let mut ordinal_of: HashMap<(usize, usize, usize), usize> = HashMap::new();
    let mut occurrence: HashMap<(usize, usize), usize> = HashMap::new();
    for (ordinal, (source, target)) in pairs.iter().enumerate() {
        let key = (source.index(), target.index());
        let seen = occurrence.get(&key).copied().unwrap_or(0);
        occurrence.insert(key, seen + 1);
        ordinal_of.insert((key.0, key.1, seen), ordinal);
    }
    let mut next: HashMap<(usize, usize), usize> = HashMap::new();
    edges
        .iter()
        .map(|edge| {
            let key = (edge.source.index(), edge.target.index());
            let seen = next.get(&key).copied().unwrap_or(0);
            next.insert(key, seen + 1);
            ordinal_of
                .get(&(key.0, key.1, seen))
                .copied()
                .unwrap_or(usize::MAX)
        })
        .collect()
}

/// True when the painted edge touches the screen-space selection `rect`.
///
/// Straight edges use the segment test; curved edges and self loops are
/// flattened with the shared sampling, so box selection matches the paint.
pub fn painted_edge_hits(edge: &PaintedEdge, rect: Rect) -> bool {
    if let Some([ctrl_a, ctrl_b]) = edge.loop_ctrls {
        let samples = sample_cubic_bezier(edge.start, ctrl_a, ctrl_b, edge.end, BEZIER_HIT_SAMPLES);
        return polyline_intersects_rect(&samples, rect);
    }
    if edge.bend_a.is_some() || edge.bend_b.is_some() {
        return polyline_intersects_rect(&edge.polyline(), rect);
    }
    match edge.ctrl {
        None => segment_intersects_rect(edge.start, edge.end, rect),
        Some(mid) => {
            let samples = sample_quadratic_bezier(edge.start, mid, edge.end, BEZIER_HIT_SAMPLES);
            polyline_intersects_rect(&samples, rect)
        }
    }
}

/// Bends of the Manhattan route selected by `options`, if any.
///
/// Taxi routes carry their single corner in the first bend; two-bend
/// orthogonal routes fill both. The shared route selector also feeds hit
/// testing, so the plan and box selection flatten identical points.
/// Degenerate axes fall back to no bends, leaving the edge straight.
fn manhattan_bends(
    start: Point2,
    end: Point2,
    options: EdgePaintOptions,
) -> (Option<Point2>, Option<Point2>) {
    match manhattan_route(start, end, options.ortho, options.taxi).as_slice() {
        [_, mid_a, mid_b, _] => (Some(*mid_a), Some(*mid_b)),
        [_, mid, _] => (Some(*mid), None),
        _ => (None, None),
    }
}

#[cfg(test)]
mod tests {
    use cg_geometry::OrthoDirection;
    use cg_graph::MockGraph;

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
            bend_a: None,
            bend_b: None,
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
                .copied()
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
}
