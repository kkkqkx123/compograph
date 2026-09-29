//! Immediate-mode canvas element that draws the graph.

use std::collections::HashMap;

use cg_geometry::{bezier_control_for_edge, parallel_offsets};
use cg_graph::{GraphView, NodeIndex, Positions};
use cg_types::{Point2, Vec2};
use gpui::{App, Bounds, IntoElement, Pixels, Window, canvas, fill, rgb};

use crate::camera::Camera;

/// Side length, in logical pixels, of the placeholder node rectangle.
const NODE_SIDE: f32 = 24.0;

/// Perpendicular spread between parallel edges sharing endpoints.
const PARALLEL_STEP: f32 = 12.0;

/// Margin around the viewport still scheduled for painting.
const CULL_MARGIN: f32 = 32.0;

/// Length of the arrowhead along the edge direction, in screen pixels.
const ARROW_LENGTH: f32 = 10.0;

/// Half width of the arrowhead across the edge direction.
const ARROW_HALF_WIDTH: f32 = 4.0;

/// Fill of unselected nodes.
const NODE_FILL: u32 = 0x4a9eff;

/// Fill of the selected node.
const SELECTED_FILL: u32 = 0xff9f2e;

/// Stroke of edges and fill of arrowheads.
const EDGE_TINT: u32 = 0x8a93a6;

/// A node rectangle scheduled for painting, in screen pixels.
#[derive(Clone, Copy, Debug)]
pub struct PaintedNode {
    pub id: NodeIndex,
    pub origin: Point2,
    pub side: f32,
    pub selected: bool,
}

/// An edge polyline scheduled for painting, in screen pixels.
#[derive(Clone, Copy, Debug)]
pub struct PaintedEdge {
    pub start: Point2,
    pub end: Point2,
    /// Control point for curved edges; straight edges carry none.
    pub ctrl: Option<Point2>,
}

/// An arrowhead triangle scheduled for painting, in screen pixels.
#[derive(Clone, Copy, Debug)]
pub struct PaintedArrow {
    pub tip: Point2,
    pub left: Point2,
    pub right: Point2,
}

/// Transforms graph structure and layout output into a node paint plan.
///
/// Nodes outside the viewport are culled so the paint closure stays cheap on
/// large graphs.
pub fn paint_nodes(
    graph: &dyn GraphView,
    positions: &Positions,
    camera: &Camera,
    viewport: Vec2,
    selected: Option<NodeIndex>,
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
                painted.push(PaintedNode {
                    id: node,
                    origin: Point2::new(screen.x - NODE_SIDE / 2.0, screen.y - NODE_SIDE / 2.0),
                    side: NODE_SIDE,
                    selected: Some(node) == selected,
                });
            }
        }
    }
    painted
}

/// Transforms edges into a paint plan with parallel edges spread as curves.
///
/// Endpoints missing from `positions` are skipped; self loops are skipped
/// until loop geometry lands. Opposite directed edges share one bundle so
/// they curve to opposite sides instead of overlapping.
pub fn paint_edges(
    graph: &dyn GraphView,
    positions: &Positions,
    camera: &Camera,
    viewport: Vec2,
) -> Vec<PaintedEdge> {
    let mut edges = graph.edges();
    edges.sort_unstable_by_key(|(source, target)| (source.index(), target.index()));
    let mut bundle_of: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    for (ordinal, (source, target)) in edges.iter().enumerate() {
        let key = unordered_key(*source, *target);
        bundle_of.entry(key).or_default().push(ordinal);
    }
    let mut painted = Vec::new();
    for (ordinal, (source, target)) in edges.iter().enumerate() {
        if source == target {
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
            painted.push(PaintedEdge { start, end, ctrl });
        }
    }
    painted
}

/// Arrowheads for every painted edge, pointing along the end tangent.
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
            }
        })
        .collect()
}

/// Unit tangent of an edge at its endpoint.
fn edge_direction(edge: &PaintedEdge) -> Vec2 {
    let reference = edge.ctrl.unwrap_or(edge.start);
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
    max_x >= -CULL_MARGIN
        && max_y >= -CULL_MARGIN
        && min_x <= viewport.x + CULL_MARGIN
        && min_y <= viewport.y + CULL_MARGIN
}

/// Canvas element painting edges under arrows under nodes.
///
/// The closure receives owned plans so the element stays `'static`.
pub fn graph_view(
    nodes: Vec<PaintedNode>,
    edges: Vec<PaintedEdge>,
    arrows: Vec<PaintedArrow>,
) -> impl IntoElement {
    canvas(
        |_bounds: Bounds<Pixels>, _window: &mut Window, _cx: &mut App| {},
        move |_bounds: Bounds<Pixels>, (), window: &mut Window, _cx: &mut App| {
            if !edges.is_empty() {
                let mut strokes = gpui::PathBuilder::stroke(gpui::px(1.5));
                for edge in &edges {
                    strokes.move_to(to_pixels(edge.start));
                    if let Some(ctrl) = edge.ctrl {
                        strokes.curve_to(to_pixels(edge.end), to_pixels(ctrl));
                    } else {
                        strokes.line_to(to_pixels(edge.end));
                    }
                }
                if let Ok(path) = strokes.build() {
                    window.paint_path(path, rgb(EDGE_TINT));
                }
            }
            if !arrows.is_empty() {
                let mut heads = gpui::PathBuilder::fill();
                for arrow in &arrows {
                    heads.add_polygon(
                        &[
                            to_pixels(arrow.tip),
                            to_pixels(arrow.left),
                            to_pixels(arrow.right),
                        ],
                        true,
                    );
                }
                if let Ok(path) = heads.build() {
                    window.paint_path(path, rgb(EDGE_TINT));
                }
            }
            for node in &nodes {
                let tint = if node.selected {
                    SELECTED_FILL
                } else {
                    NODE_FILL
                };
                let quad = fill(
                    Bounds {
                        origin: gpui::point(gpui::px(node.origin.x), gpui::px(node.origin.y)),
                        size: gpui::size(gpui::px(node.side), gpui::px(node.side)),
                    },
                    rgb(tint),
                );
                window.paint_quad(quad);
            }
        },
    )
}

fn to_pixels(point: Point2) -> gpui::Point<Pixels> {
    gpui::point(gpui::px(point.x), gpui::px(point.y))
}

#[cfg(test)]
mod tests {
    use cg_graph::MockGraph;
    use cg_types::Vec2;

    use super::*;

    fn viewport() -> Vec2 {
        Vec2::new(1024.0, 768.0)
    }

    fn camera() -> Camera {
        Camera::new(Point2::ZERO, 1.0)
    }

    #[test]
    fn edges_skip_endpoints_without_positions() {
        let graph = MockGraph::chain(3);
        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(-900.0, -900.0));
        positions.insert(NodeIndex::new(1), Point2::new(-880.0, -900.0));
        let edges = paint_edges(&graph, &positions, &camera(), viewport());
        assert!(edges.is_empty());
    }

    #[test]
    fn lone_edge_stays_straight() {
        let graph = MockGraph::chain(2);
        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(-400.0, 0.0));
        positions.insert(NodeIndex::new(1), Point2::new(-300.0, 0.0));
        let edges = paint_edges(&graph, &positions, &camera(), viewport());
        assert_eq!(edges.len(), 1);
        assert!(edges[0].ctrl.is_none());
    }

    #[test]
    fn opposite_edges_curve_to_opposite_sides() {
        let mut graph = MockGraph::chain(2);
        graph.push_edge(1, 0);
        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(-400.0, 0.0));
        positions.insert(NodeIndex::new(1), Point2::new(-300.0, 0.0));
        let edges = paint_edges(&graph, &positions, &camera(), viewport());
        assert_eq!(edges.len(), 2);
        let ctrls: Vec<Option<Point2>> = edges.iter().map(|edge| edge.ctrl).collect();
        assert!(ctrls.iter().all(|ctrl| ctrl.is_some()));
        let first = ctrls[0].unwrap_or(Point2::ZERO);
        let second = ctrls[1].unwrap_or(Point2::ZERO);
        assert!((first.y - 384.0).abs() > 0.5);
        assert!((first.y + second.y - 2.0 * 384.0).abs() < 1e-3);
    }

    #[test]
    fn arrow_tip_leads_along_the_edge() {
        let corners = arrow_triangle(Point2::new(10.0, 0.0), 0.0, ARROW_LENGTH, ARROW_HALF_WIDTH);
        assert_eq!(corners[0], Point2::new(10.0, 0.0));
        assert!(corners[1].x < corners[0].x && corners[2].x < corners[0].x);
        assert!(corners[1].y > 0.0 && corners[2].y < 0.0);
    }

    #[test]
    fn node_plan_marks_the_selection() {
        let graph = MockGraph::chain(2);
        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(-400.0, 0.0));
        positions.insert(NodeIndex::new(1), Point2::new(-300.0, 0.0));
        let plan = paint_nodes(
            &graph,
            &positions,
            &camera(),
            viewport(),
            Some(NodeIndex::new(1)),
        );
        assert_eq!(plan.len(), 2);
        assert!(!plan[0].selected);
        assert!(plan[1].selected);
    }
}
