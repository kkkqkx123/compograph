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
    BundleSlot, clean_waypoints, curved_edge, curve_control, haystack_edge, haystack_span,
    loop_edge, manhattan_bends, manhattan_edge, routed_options, should_use_haystack,
    should_use_manhattan, waypoint_edge,
};
use crate::waypoints::WaypointStore;
use cg_types::Point2;

/// Self-loop plan for one anchor, or nothing when it is culled.
fn loop_plan(
    source: NodeIndex,
    target: NodeIndex,
    screen: Point2,
    stack: usize,
    style: EdgeStyle,
    viewport: Vec2,
) -> Option<PaintedEdge> {
    let entry = loop_edge(source, target, screen, stack, style);
    if loop_visible(screen, entry.loop_ctrls.unwrap_or([screen; 2]), viewport) {
        Some(entry)
    } else {
        None
    }
}

/// Waypoint plan through cleaned screen points, or nothing when culled.
///
/// User waypoints skip parallel offsets, haystack simplification and
/// Manhattan derivation; self loops never reach this path and keep loop
/// geometry in their caller.
fn waypoint_plan(
    source: NodeIndex,
    target: NodeIndex,
    start: Point2,
    end: Point2,
    screen_raw: &[Point2],
    style: EdgeStyle,
    viewport: Vec2,
) -> Option<PaintedEdge> {
    let bends = clean_waypoints(start, screen_raw, end);
    let mut line = Vec::with_capacity(bends.len() + 2);
    line.push(start);
    line.extend_from_slice(&bends);
    line.push(end);
    if !polyline_visible(&line, viewport) {
        return None;
    }
    Some(waypoint_edge(source, target, start, end, bends, style))
}

/// Routed plan for a non-loop edge without waypoints, or nothing when culled.
///
/// Haystack fan-out, Manhattan polylines and parallel Bezier curves share
/// this decision so bulk planning and single-edge rebuilds cannot diverge.
struct RoutedPlan {
    source: NodeIndex,
    target: NodeIndex,
    start: Point2,
    end: Point2,
    style: EdgeStyle,
    routed: EdgePaintOptions,
    slot: usize,
    bundle_len: usize,
    viewport: Vec2,
}

fn routed_plan(plan: RoutedPlan) -> Option<PaintedEdge> {
    let RoutedPlan {
        source,
        target,
        start,
        end,
        style,
        routed,
        slot,
        bundle_len,
        viewport,
    } = plan;
    if should_use_haystack(routed, bundle_len) {
        let (fanned_a, fanned_b) = haystack_span(start, end, source, target, slot);
        if !edge_visible(fanned_a, fanned_b, None, viewport) {
            return None;
        }
        return Some(haystack_edge(source, target, fanned_a, fanned_b, style));
    }
    if should_use_manhattan(routed) {
        let (bend_a, bend_b) = manhattan_bends(start, end, routed);
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
    let ctrl = curve_control(
        style,
        start,
        end,
        BundleSlot {
            source,
            target,
            slot,
            len: bundle_len,
        },
        routed,
    );
    if !edge_visible(start, end, ctrl, viewport) {
        return None;
    }
    Some(curved_edge(source, target, start, end, ctrl, style))
}

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
            if let Some(entry) = loop_plan(*source, *target, screen, stack, style, viewport) {
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
            if let Some(entry) =
                waypoint_plan(*source, *target, start, end, &screen_raw, style, viewport)
            {
                painted.push(entry);
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
        let routed = routed_options(style, options);
        if let Some(entry) = routed_plan(RoutedPlan {
            source: *source,
            target: *target,
            start,
            end,
            style,
            routed,
            slot,
            bundle_len,
            viewport,
        }) {
            painted.push(entry);
        }
    }
    painted
}

/// One entry of a pair list with its bundling context attached.
///
/// The ordinal is meaningless without the list: loop stacking, parallel
/// slots and waypoint occurrences all count earlier entries. Grouping both
/// keeps single-edge rebuilds agreeing with the bulk plan for the same
/// ordinal.
#[derive(Clone, Copy, Debug)]
pub struct EdgeOrdinal<'a> {
    pub pairs: &'a [(NodeIndex, NodeIndex)],
    pub ordinal: usize,
}

impl EdgeOrdinal<'_> {
    /// Endpoints of the entry, or nothing when the ordinal is out of range.
    pub fn endpoints(self) -> Option<(NodeIndex, NodeIndex)> {
        self.pairs.get(self.ordinal).copied()
    }

    /// Occurrence of these endpoints before the entry, for waypoint lookup.
    pub fn occurrence(self) -> usize {
        let (source, target) = match self.endpoints() {
            Some(pair) => pair,
            None => return 0,
        };
        self.pairs
            .iter()
            .take(self.ordinal)
            .filter(|(a, b)| *a == source && *b == target)
            .count()
    }

    /// Stacking ordinal among earlier self loops on the same node.
    pub fn loop_ordinal(self) -> usize {
        loop_ordinal(self.pairs, self.ordinal)
    }

    /// Parallel slot and bundle size of the entry.
    pub fn bundle(self) -> (usize, usize) {
        bundle_slot(self.pairs, self.ordinal)
    }
}

/// Edge plan for one pair-list entry, or nothing when it is missing or culled.
///
/// The result agrees with [`paint_edges_for`] for the same ordinal, so the
/// retained cache can refresh moved nodes without rebuilding every edge.
pub fn paint_single_edge(
    edge: EdgeOrdinal<'_>,
    positions: &Positions,
    camera: &Camera,
    viewport: Vec2,
    options: EdgePaintOptions,
    edge_style: impl Fn(NodeIndex, NodeIndex) -> EdgeStyle,
) -> Option<PaintedEdge> {
    paint_single_edge_with_waypoints(
        edge,
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
    edge: EdgeOrdinal<'_>,
    positions: &Positions,
    camera: &Camera,
    viewport: Vec2,
    options: EdgePaintOptions,
    waypoints: &WaypointStore,
    edge_style: impl Fn(NodeIndex, NodeIndex) -> EdgeStyle,
) -> Option<PaintedEdge> {
    let (source, target) = edge.endpoints()?;
    let style = edge_style(source, target);
    let world_rect = world_viewport_rect(camera, viewport);
    let margin = world_margin_for(camera);
    if source == target {
        let anchor = positions.get(&source)?;
        if !point_in_grown_rect(*anchor, world_rect, margin + NODE_SIDE) {
            return None;
        }
        let screen = camera.world_to_viewport(viewport, *anchor);
        return loop_plan(source, target, screen, edge.loop_ordinal(), style, viewport);
    }
    let (a, b) = match (positions.get(&source), positions.get(&target)) {
        (Some(a), Some(b)) => (*a, *b),
        _ => return None,
    };
    let occurrence = edge.occurrence();
    let raw = waypoints.get(source, target, occurrence);
    if !raw.is_empty() {
        let start = camera.world_to_viewport(viewport, a);
        let end = camera.world_to_viewport(viewport, b);
        let screen_raw: Vec<cg_types::Point2> = raw
            .iter()
            .map(|point| camera.world_to_viewport(viewport, *point))
            .collect();
        return waypoint_plan(source, target, start, end, &screen_raw, style, viewport);
    }
    let (slot, bundle_len) = edge.bundle();
    if !segment_in_grown_rect(a, b, world_rect, margin + spread_for(bundle_len)) {
        return None;
    }
    let start = camera.world_to_viewport(viewport, a);
    let end = camera.world_to_viewport(viewport, b);
    let routed = routed_options(style, options);
    routed_plan(RoutedPlan {
        source,
        target,
        start,
        end,
        style,
        routed,
        slot,
        bundle_len,
        viewport,
    })
}
