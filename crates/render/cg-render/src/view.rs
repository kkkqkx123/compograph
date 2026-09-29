//! Immediate-mode canvas element that draws the graph.

use std::collections::HashMap;

use cg_geometry::{
    BEZIER_HIT_SAMPLES, bezier_control_for_edge, parallel_offsets, polyline_intersects_rect,
    sample_cubic_bezier, sample_quadratic_bezier, segment_intersects_rect, self_loop_controls,
};
use cg_graph::{GraphView, NodeIndex, Positions};
use cg_types::{Point2, Rect, Vec2};
use gpui::{App, Bounds, IntoElement, Pixels, Rgba, Window, canvas, fill, rgb};

use crate::camera::Camera;
use crate::style::{EdgeStyle, NodeStyle};

/// Side length, in logical pixels, of the placeholder node rectangle.
pub const NODE_SIDE: f32 = 24.0;

/// Perpendicular spread between parallel edges sharing endpoints.
pub const PARALLEL_STEP: f32 = 12.0;

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
    pub tint: u32,
    pub width: f32,
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

/// Transforms graph structure and layout output into a node paint plan.
///
/// Nodes outside the viewport are culled so the paint closure stays cheap on
/// large graphs. Fill colors arrive resolved through `node_style`, so
/// selection and highlights reach the canvas without touching the stylesheet.
pub fn paint_nodes(
    graph: &dyn GraphView,
    positions: &Positions,
    camera: &Camera,
    viewport: Vec2,
    node_style: impl Fn(NodeIndex) -> NodeStyle,
) -> Vec<PaintedNode> {
    let mut painted = Vec::new();
    let mut ids = graph.node_ids();
    ids.sort_unstable_by_key(|node| node.index());
    for node in ids {
        if let Some(world) = positions.get(&node) {
            let screen = camera.world_to_viewport(viewport, *world);
            let visible = screen.x >= -NODE_SIDE
                && screen.y >= -NODE_SIDE
                && screen.x <= viewport.x + NODE_SIDE
                && screen.y <= viewport.y + NODE_SIDE;
            if visible {
                let style = node_style(node);
                let side = (NODE_SIDE * style.scale).max(4.0);
                painted.push(PaintedNode {
                    id: node,
                    origin: Point2::new(screen.x - side / 2.0, screen.y - side / 2.0),
                    side,
                    fill: style.fill,
                    opacity: style.opacity,
                });
            }
        }
    }
    painted
}

/// Transforms edges into a paint plan with parallel edges spread as curves.
///
/// Endpoints missing from `positions` are skipped. Self loops become upward
/// cubic loops sized by the node constant; overlapping loops on one node
/// spread outward in edge order. Opposite directed edges share one bundle so
/// they curve to opposite sides instead of overlapping.
pub fn paint_edges(
    graph: &dyn GraphView,
    positions: &Positions,
    camera: &Camera,
    viewport: Vec2,
    edge_style: impl Fn(NodeIndex, NodeIndex) -> EdgeStyle,
) -> Vec<PaintedEdge> {
    let mut edges = graph.edges();
    edges.sort_unstable_by_key(|(source, target)| (source.index(), target.index()));
    let mut bundle_of: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    for (ordinal, (source, target)) in edges.iter().enumerate() {
        let key = unordered_key(*source, *target);
        bundle_of.entry(key).or_default().push(ordinal);
    }
    let mut loops_seen: HashMap<usize, usize> = HashMap::new();
    let mut painted = Vec::new();
    for (ordinal, (source, target)) in edges.iter().enumerate() {
        let style = edge_style(*source, *target);
        if source == target {
            let Some(anchor) = positions.get(source) else {
                continue;
            };
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
                    tint: style.tint,
                    width: style.width,
                });
            }
            continue;
        }
        let (Some(a), Some(b)) = (positions.get(source), positions.get(target)) else {
            continue;
        };
        let start = camera.world_to_viewport(viewport, *a);
        let end = camera.world_to_viewport(viewport, *b);
        let key = unordered_key(*source, *target);
        let bundle = bundle_of.get(&key).map(Vec::as_slice).unwrap_or(&[]);
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
                tint: style.tint,
                width: style.width,
            });
        }
    }
    painted
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
    match edge.ctrl {
        None => segment_intersects_rect(edge.start, edge.end, rect),
        Some(mid) => {
            let samples = sample_quadratic_bezier(edge.start, mid, edge.end, BEZIER_HIT_SAMPLES);
            polyline_intersects_rect(&samples, rect)
        }
    }
}

/// Arrowheads for every painted edge, pointing along the end tangent.
///
/// Self loops point along the return tangent from the second loop control.
pub fn paint_arrows(edges: &[PaintedEdge]) -> Vec<PaintedArrow> {
    edges
        .iter()
        .map(|edge| {
            let direction = edge_direction(edge);
            let angle = direction.y.atan2(direction.x);
            let corners = arrow_triangle(edge.end, angle, ARROW_LENGTH, ARROW_HALF_WIDTH);
            PaintedArrow {
                tip: corners[0],
                left: corners[1],
                right: corners[2],
                tint: edge.tint,
            }
        })
        .collect()
}

/// Unit tangent of an edge at its endpoint.
fn edge_direction(edge: &PaintedEdge) -> Vec2 {
    let reference = edge
        .loop_ctrls
        .map(|ctrls| ctrls[1])
        .or(edge.ctrl)
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
