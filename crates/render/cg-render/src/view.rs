//! Immediate-mode canvas element that draws the graph.

use std::collections::HashMap;

use cg_geometry::{
    BEZIER_HIT_SAMPLES, OrthoDirection, bezier_control_for_edge, haystack_endpoints,
    ortho_polyline, parallel_offsets, polyline_intersects_rect, sample_cubic_bezier,
    sample_quadratic_bezier, segment_intersects_rect, self_loop_controls, use_haystack,
};

use crate::lod::DetailLevel;
use crate::spatial::SpatialIndex;
use cg_graph::{GraphView, NodeIndex, Positions};
use cg_types::{Point2, Rect, Vec2};
use gpui::{App, Bounds, IntoElement, Pixels, Rgba, Window, canvas, fill, rgb};

use crate::camera::Camera;
use crate::style::{EdgeStyle, NodeStyle};

/// Side length, in logical pixels, of the placeholder node rectangle.
pub const NODE_SIDE: f32 = 24.0;

/// Perpendicular spread between parallel edges sharing endpoints.
pub const PARALLEL_STEP: f32 = 12.0;

/// Edge count above which dense bundles switch to haystack rendering.
///
/// This is the density policy default fed into [`EdgePaintOptions`]. The
/// geometry crate keeps its own lower fan-out floor for the shape itself; the
/// two constants serve different layers and must not be merged.
pub const EDGE_AGGREGATION_THRESHOLD: usize = 24;

/// Margin around the viewport still scheduled for painting.
const CULL_MARGIN: f32 = 32.0;

/// Length of the arrowhead along the edge direction, in screen pixels.
const ARROW_LENGTH: f32 = 10.0;

/// Half width of the arrowhead across the edge direction.
const ARROW_HALF_WIDTH: f32 = 4.0;

/// Fill of the rubber-band box-selection rectangle.
const RUBBER_BAND_FILL: u32 = 0x4a9eff22;

/// Outline of the rubber-band box-selection rectangle.
pub const RUBBER_BAND_STROKE: u32 = 0x4a9eff;

/// A node rectangle scheduled for painting, in screen pixels.
#[derive(Clone, Copy, Debug)]
pub struct PaintedNode {
    pub id: NodeIndex,
    pub origin: Point2,
    pub side: f32,
    pub fill: u32,
    pub opacity: f32,
}

/// An edge polyline scheduled for painting, in screen pixels.
#[derive(Clone, Copy, Debug)]
pub struct PaintedEdge {
    pub source: NodeIndex,
    pub target: NodeIndex,
    pub start: Point2,
    pub end: Point2,
    /// Control point for curved edges; straight edges carry none.
    pub ctrl: Option<Point2>,
    /// Control points of a self loop; only set when start equals end.
    pub loop_ctrls: Option<[Point2; 2]>,
    /// First orthogonal bend, if the edge routes as a polyline.
    pub bend_a: Option<Point2>,
    /// Second orthogonal bend, if the route needs two turns.
    pub bend_b: Option<Point2>,
    /// True when the edge was simplified for dense bundles.
    pub aggregated: bool,
    pub tint: u32,
    pub width: f32,
}

impl PaintedEdge {
    fn straight(
        source: NodeIndex,
        target: NodeIndex,
        start: Point2,
        end: Point2,
        tint: u32,
        width: f32,
    ) -> Self {
        Self {
            source,
            target,
            start,
            end,
            ctrl: None,
            loop_ctrls: None,
            bend_a: None,
            bend_b: None,
            aggregated: false,
            tint,
            width,
        }
    }

    /// Interior points of the painted path excluding the endpoints.
    pub fn bends(&self) -> Vec<Point2> {
        let mut bends = Vec::new();
        if let Some(bend) = self.bend_a {
            bends.push(bend);
        }
        if let Some(bend) = self.bend_b {
            bends.push(bend);
        }
        bends
    }

    /// Full painted polyline from start through bends to end.
    pub fn polyline(&self) -> Vec<Point2> {
        let mut line = Vec::with_capacity(4);
        line.push(self.start);
        if let Some(bend) = self.bend_a {
            line.push(bend);
        }
        if let Some(bend) = self.bend_b {
            line.push(bend);
        }
        line.push(self.end);
        line
    }
}

/// Options selecting simplified edge geometry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EdgePaintOptions {
    pub level: DetailLevel,
    pub aggregate_threshold: usize,
    pub force_haystack: bool,
    pub ortho: Option<OrthoDirection>,
}

impl Default for EdgePaintOptions {
    fn default() -> Self {
        Self {
            level: DetailLevel::Full,
            aggregate_threshold: EDGE_AGGREGATION_THRESHOLD,
            force_haystack: false,
            ortho: None,
        }
    }
}

/// An arrowhead triangle scheduled for painting, in screen pixels.
#[derive(Clone, Copy, Debug)]
pub struct PaintedArrow {
    pub tip: Point2,
    pub left: Point2,
    pub right: Point2,
    pub tint: u32,
}

/// Rubber-band rectangle scheduled for painting, in screen pixels.
#[derive(Clone, Copy, Debug)]
pub struct PaintedRubberBand {
    pub origin: Point2,
    pub size: Vec2,
}

/// Nodes whose bodies touch `rect`, in index order.
///
/// The spatial index narrows candidates and the node body square decides, so
/// plan generation iterates only visible nodes on large graphs. Callers keep
/// the index versioned: viewport motion reuses it, position write-backs
/// rebuild it.
pub fn visible_node_ids(
    positions: &Positions,
    index: &SpatialIndex,
    rect: Rect,
    half_extent: f32,
) -> Vec<NodeIndex> {
    let mut found: Vec<NodeIndex> = index
        .query_rect(rect)
        .into_iter()
        .filter(|node| {
            positions
                .get(node)
                .map(|center| {
                    let body = Rect::new(
                        Point2::new(center.x - half_extent, center.y - half_extent),
                        Vec2::new(half_extent * 2.0, half_extent * 2.0),
                    );
                    body.intersects(rect)
                })
                .unwrap_or(false)
        })
        .collect();
    found.sort_unstable_by_key(|node| node.index());
    found
}

/// Transforms graph structure and layout output into a node paint plan.
///
/// This entry iterates every node and culls off-screen ones one by one, which
/// keeps it suitable as the scale-benchmark baseline. The product path prefers
/// [`paint_nodes_for`] over a caller-narrowed visible set. Fill colors arrive
/// resolved through `node_style`, so selection and highlights reach the canvas
/// without touching the stylesheet.
pub fn paint_nodes(
    graph: &dyn GraphView,
    positions: &Positions,
    camera: &Camera,
    viewport: Vec2,
    node_style: impl Fn(NodeIndex) -> NodeStyle,
) -> Vec<PaintedNode> {
    let mut ids = graph.node_ids();
    ids.sort_unstable_by_key(|node| node.index());
    paint_nodes_for(&ids, positions, camera, viewport, node_style)
}

/// Node plan for an explicit candidate list, such as the visible set.
///
/// Candidates outside the viewport are still skipped, so index over-queries
/// stay harmless.
pub fn paint_nodes_for(
    nodes: &[NodeIndex],
    positions: &Positions,
    camera: &Camera,
    viewport: Vec2,
    node_style: impl Fn(NodeIndex) -> NodeStyle,
) -> Vec<PaintedNode> {
    let mut painted = Vec::new();
    for node in nodes {
        if let Some(entry) = paint_single_node(*node, positions, camera, viewport, &node_style) {
            painted.push(entry);
        }
    }
    painted
}

/// Node plan for one node, or nothing when it is missing or off-screen.
pub fn paint_single_node(
    node: NodeIndex,
    positions: &Positions,
    camera: &Camera,
    viewport: Vec2,
    node_style: impl Fn(NodeIndex) -> NodeStyle,
) -> Option<PaintedNode> {
    let world = positions.get(&node)?;
    let screen = camera.world_to_viewport(viewport, *world);
    let visible = screen.x >= -NODE_SIDE
        && screen.y >= -NODE_SIDE
        && screen.x <= viewport.x + NODE_SIDE
        && screen.y <= viewport.y + NODE_SIDE;
    if !visible {
        return None;
    }
    let style = node_style(node);
    let side = (NODE_SIDE * style.scale).max(4.0);
    Some(PaintedNode {
        id: node,
        origin: Point2::new(screen.x - side / 2.0, screen.y - side / 2.0),
        side,
        fill: style.fill,
        opacity: style.opacity,
    })
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
/// dense bundles fan out as haystack lines; an explicit orthogonal direction
/// routes edges as polylines with at most two bends. Hit testing stays on the
/// full-precision path so visual downgrades never change selection.
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
                let mut edge =
                    PaintedEdge::straight(*source, *target, start, end, style.tint, style.width);
                edge.aggregated = true;
                painted.push(edge);
            }
            continue;
        }
        if let Some(direction) = options.ortho {
            let line = ortho_polyline(start, end, direction);
            let (bend_a, bend_b) = match line.as_slice() {
                [_, mid_a, mid_b, _] => (Some(*mid_a), Some(*mid_b)),
                [_, mid, _] => (Some(*mid), None),
                _ => (None, None),
            };
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
                    aggregated: true,
                    tint: style.tint,
                    width: style.width,
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
        let mut edge = PaintedEdge::straight(source, target, start, end, style.tint, style.width);
        edge.aggregated = true;
        return Some(edge);
    }
    if let Some(direction) = options.ortho {
        let line = ortho_polyline(start, end, direction);
        let (bend_a, bend_b) = match line.as_slice() {
            [_, mid_a, mid_b, _] => (Some(*mid_a), Some(*mid_b)),
            [_, mid, _] => (Some(*mid), None),
            _ => (None, None),
        };
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
            aggregated: true,
            tint: style.tint,
            width: style.width,
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

/// Model-space rectangle currently visible through the camera.
pub fn world_viewport_rect(camera: &Camera, viewport: Vec2) -> Rect {
    let top_left = camera.viewport_to_world(viewport, Point2::ZERO);
    let bottom_right = camera.viewport_to_world(viewport, Point2::new(viewport.x, viewport.y));
    Rect::from_corners(top_left, bottom_right)
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

/// Arrowheads honoring the detail level.
///
/// Minimal detail drops arrowheads entirely; other levels point every edge
/// along its end tangent. Self loops point along the return tangent.
pub fn paint_arrows_for_level(edges: &[PaintedEdge], level: DetailLevel) -> Vec<PaintedArrow> {
    if level == DetailLevel::Minimal {
        return Vec::new();
    }
    paint_arrows(edges)
}

/// Arrowheads for every painted edge, pointing along the end tangent.
///
/// Self loops point along the return tangent from the second loop control.
pub fn paint_arrows(edges: &[PaintedEdge]) -> Vec<PaintedArrow> {
    edges.iter().map(paint_single_arrow).collect()
}

/// Arrowhead for one painted edge, pointing along its end tangent.
///
/// Self loops point along the return tangent from the second loop control.
pub fn paint_single_arrow(edge: &PaintedEdge) -> PaintedArrow {
    let direction = edge_direction(edge);
    let angle = direction.y.atan2(direction.x);
    let corners = arrow_triangle(edge.end, angle, ARROW_LENGTH, ARROW_HALF_WIDTH);
    PaintedArrow {
        tip: corners[0],
        left: corners[1],
        right: corners[2],
        tint: edge.tint,
    }
}

/// Unit tangent of an edge at its endpoint.
fn edge_direction(edge: &PaintedEdge) -> Vec2 {
    let reference = edge
        .loop_ctrls
        .map(|ctrls| ctrls[1])
        .or(edge.ctrl)
        .or(edge.bend_b)
        .or(edge.bend_a)
        .unwrap_or(edge.start);
    let delta = edge.end - reference;
    let length = delta.length();
    if length <= f32::EPSILON {
        Vec2::new(1.0, 0.0)
    } else {
        Vec2::new(delta.x / length, delta.y / length)
    }
}

/// Arrowhead corners with the tip first, pointing along `angle`.
pub fn arrow_triangle(tip: Point2, angle: f32, length: f32, half_width: f32) -> [Point2; 3] {
    let axis = Vec2::new(angle.cos(), angle.sin());
    let normal = Vec2::new(-axis.y, axis.x);
    let base = tip + axis * (-length);
    [
        tip,
        base + normal * half_width,
        base + normal * (-half_width),
    ]
}

fn unordered_key(source: NodeIndex, target: NodeIndex) -> (usize, usize) {
    let (a, b) = (source.index(), target.index());
    if a <= b { (a, b) } else { (b, a) }
}

/// Screen cull margin expressed in model units at the current zoom.
fn world_margin_for(camera: &Camera) -> f32 {
    CULL_MARGIN / camera.zoom.max(f32::EPSILON)
}

/// Extra model-space spread a bundle may fan out to around its endpoints.
fn spread_for(bundle_len: usize) -> f32 {
    NODE_SIDE + PARALLEL_STEP * bundle_len as f32
}

fn point_in_grown_rect(point: Point2, rect: Rect, grow: f32) -> bool {
    point.x >= rect.origin.x - grow
        && point.y >= rect.origin.y - grow
        && point.x <= rect.origin.x + rect.size.x + grow
        && point.y <= rect.origin.y + rect.size.y + grow
}

fn segment_in_grown_rect(a: Point2, b: Point2, rect: Rect, grow: f32) -> bool {
    let min_x = a.x.min(b.x);
    let min_y = a.y.min(b.y);
    let max_x = a.x.max(b.x);
    let max_y = a.y.max(b.y);
    max_x >= rect.origin.x - grow
        && max_y >= rect.origin.y - grow
        && min_x <= rect.origin.x + rect.size.x + grow
        && min_y <= rect.origin.y + rect.size.y + grow
}

fn edge_visible(start: Point2, end: Point2, ctrl: Option<Point2>, viewport: Vec2) -> bool {
    let mut min_x = start.x.min(end.x);
    let mut min_y = start.y.min(end.y);
    let mut max_x = start.x.max(end.x);
    let mut max_y = start.y.max(end.y);
    if let Some(mid) = ctrl {
        min_x = min_x.min(mid.x);
        min_y = min_y.min(mid.y);
        max_x = max_x.max(mid.x);
        max_y = max_y.max(mid.y);
    }
    bounds_visible(min_x, min_y, max_x, max_y, viewport)
}

fn polyline_visible(line: &[Point2], viewport: Vec2) -> bool {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for point in line {
        min_x = min_x.min(point.x);
        min_y = min_y.min(point.y);
        max_x = max_x.max(point.x);
        max_y = max_y.max(point.y);
    }
    bounds_visible(min_x, min_y, max_x, max_y, viewport)
}

fn loop_visible(anchor: Point2, ctrls: [Point2; 2], viewport: Vec2) -> bool {
    let min_x = anchor.x.min(ctrls[0].x.min(ctrls[1].x));
    let min_y = anchor.y.min(ctrls[0].y.min(ctrls[1].y));
    let max_x = anchor.x.max(ctrls[0].x.max(ctrls[1].x));
    let max_y = anchor.y.max(ctrls[0].y.max(ctrls[1].y));
    bounds_visible(min_x, min_y, max_x, max_y, viewport)
}

fn bounds_visible(min_x: f32, min_y: f32, max_x: f32, max_y: f32, viewport: Vec2) -> bool {
    max_x >= -CULL_MARGIN
        && max_y >= -CULL_MARGIN
        && min_x <= viewport.x + CULL_MARGIN
        && min_y <= viewport.y + CULL_MARGIN
}

/// Canvas element painting edges under arrows under nodes.
///
/// Each edge carries its own tint and width, so strokes are built per edge
/// instead of sharing one path. The closure receives owned plans so the
/// element stays `'static`.
pub fn graph_view(
    nodes: Vec<PaintedNode>,
    edges: Vec<PaintedEdge>,
    arrows: Vec<PaintedArrow>,
    rubber_band: Option<PaintedRubberBand>,
) -> impl IntoElement {
    canvas(
        |_bounds: Bounds<Pixels>, _window: &mut Window, _cx: &mut App| {},
        move |_bounds: Bounds<Pixels>, (), window: &mut Window, _cx: &mut App| {
            for edge in &edges {
                let mut strokes = gpui::PathBuilder::stroke(gpui::px(edge.width.max(0.5)));
                strokes.move_to(to_pixels(edge.start));
                if let Some([ctrl_a, ctrl_b]) = edge.loop_ctrls {
                    strokes.cubic_bezier_to(
                        to_pixels(edge.end),
                        to_pixels(ctrl_a),
                        to_pixels(ctrl_b),
                    );
                } else if let Some(ctrl) = edge.ctrl {
                    strokes.curve_to(to_pixels(edge.end), to_pixels(ctrl));
                } else {
                    for bend in edge.bends() {
                        strokes.line_to(to_pixels(bend));
                    }
                    strokes.line_to(to_pixels(edge.end));
                }
                if let Ok(path) = strokes.build() {
                    window.paint_path(path, rgb(edge.tint));
                }
            }
            if !arrows.is_empty() {
                let mut by_tint: HashMap<u32, Vec<[Point2; 3]>> = HashMap::new();
                for arrow in &arrows {
                    by_tint.entry(arrow.tint).or_default().push([
                        arrow.tip,
                        arrow.left,
                        arrow.right,
                    ]);
                }
                let mut tints: Vec<u32> = by_tint.keys().copied().collect();
                tints.sort_unstable();
                for tint in tints {
                    let mut heads = gpui::PathBuilder::fill();
                    for corners in by_tint.get(&tint).unwrap_or(&Vec::new()) {
                        heads.add_polygon(
                            &[
                                to_pixels(corners[0]),
                                to_pixels(corners[1]),
                                to_pixels(corners[2]),
                            ],
                            true,
                        );
                    }
                    if let Ok(path) = heads.build() {
                        window.paint_path(path, rgb(tint));
                    }
                }
            }
            for node in &nodes {
                let quad = fill(
                    Bounds {
                        origin: gpui::point(gpui::px(node.origin.x), gpui::px(node.origin.y)),
                        size: gpui::size(gpui::px(node.side), gpui::px(node.side)),
                    },
                    with_opacity(node.fill, node.opacity),
                );
                window.paint_quad(quad);
            }
            if let Some(band) = rubber_band {
                let quad = fill(
                    Bounds {
                        origin: gpui::point(gpui::px(band.origin.x), gpui::px(band.origin.y)),
                        size: gpui::size(gpui::px(band.size.x), gpui::px(band.size.y)),
                    },
                    rgba(RUBBER_BAND_FILL),
                );
                window.paint_quad(quad);
                let mut outline = gpui::PathBuilder::stroke(gpui::px(1.0));
                let far = Point2::new(band.origin.x + band.size.x, band.origin.y + band.size.y);
                outline.move_to(to_pixels(band.origin));
                outline.line_to(to_pixels(Point2::new(far.x, band.origin.y)));
                outline.line_to(to_pixels(far));
                outline.line_to(to_pixels(Point2::new(band.origin.x, far.y)));
                outline.line_to(to_pixels(band.origin));
                if let Ok(path) = outline.build() {
                    window.paint_path(path, rgb(RUBBER_BAND_STROKE));
                }
            }
        },
    )
}

fn with_opacity(tint: u32, opacity: f32) -> Rgba {
    let mut color = rgb(tint);
    color.a = opacity.clamp(0.0, 1.0);
    color
}

fn to_pixels(point: Point2) -> gpui::Point<Pixels> {
    gpui::point(gpui::px(point.x), gpui::px(point.y))
}

fn rgba(hex: u32) -> Rgba {
    gpui::rgba(hex)
}

#[cfg(test)]
mod tests {
    use cg_graph::MockGraph;

    use super::*;
    use crate::style::{BypassStore, NodeStylePatch, SELECTED_NODE_FILL, StyleMapper, StyleSheet};

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
        let arrows = paint_arrows(&edges);
        assert_eq!(arrows.len(), 1);
        assert!(arrows[0].tip == edges[0].end);
    }

    #[test]
    fn arrow_tip_leads_along_the_edge() {
        let corners = arrow_triangle(Point2::new(10.0, 0.0), 0.0, ARROW_LENGTH, ARROW_HALF_WIDTH);
        assert_eq!(corners[0], Point2::new(10.0, 0.0));
        assert!(corners[1].x < corners[0].x && corners[2].x < corners[0].x);
        assert!(corners[1].y > 0.0 && corners[2].y < 0.0);
    }

    #[test]
    fn node_plan_uses_resolved_styles() {
        let graph = MockGraph::chain(2);
        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(-400.0, 0.0));
        positions.insert(NodeIndex::new(1), Point2::new(-300.0, 0.0));
        let sheet = StyleSheet::default();
        let mapper = StyleMapper::new();
        let mut bypass = BypassStore::new();
        bypass.set_node(NodeIndex::new(1), NodeStylePatch::selected());
        let plan = paint_nodes(&graph, &positions, &camera(), viewport(), |node| {
            bypass.resolve_node(&sheet, &mapper, node, None, 1)
        });
        assert_eq!(plan.len(), 2);
        assert_eq!(plan[0].fill, NodeStyle::default().fill);
        assert_eq!(plan[1].fill, SELECTED_NODE_FILL);
    }

    #[test]
    fn painted_edge_hit_covers_straight_curved_and_loop() {
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
        assert!(edges[0].aggregated);
        let rect = Rect::from_corners(edges[0].start, edges[0].end);
        assert!(painted_edge_hits(&edges[0], rect));
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
    fn visible_query_narrows_to_the_viewport() {
        use crate::spatial::SpatialIndex;

        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(0.0, 0.0));
        positions.insert(NodeIndex::new(1), Point2::new(5000.0, 5000.0));
        let mut index = SpatialIndex::new(48.0);
        index.rebuild(&positions);
        let near = Rect::new(Point2::new(-100.0, -100.0), Vec2::new(200.0, 200.0));
        assert_eq!(
            visible_node_ids(&positions, &index, near, NODE_SIDE / 2.0),
            vec![NodeIndex::new(0)]
        );
        let far = Rect::new(Point2::new(4900.0, 4900.0), Vec2::new(200.0, 200.0));
        assert_eq!(
            visible_node_ids(&positions, &index, far, NODE_SIDE / 2.0),
            vec![NodeIndex::new(1)]
        );
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
