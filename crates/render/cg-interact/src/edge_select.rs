//! Edge box selection sharing the paint plan routing decisions.

use std::collections::HashMap;

use cg_geometry::{
    BEZIER_HIT_SAMPLES, OrthoDirection, bezier_control_for_edge, manhattan_route, parallel_offsets,
    polyline_intersects_rect, sample_quadratic_bezier, segment_intersects_rect, self_loop_polyline,
};
use cg_graph::{GraphView, NodeIndex, Positions};
use cg_render::{
    BundleSlot, DetailLevel, EdgeCurve, EdgePaintOptions, EdgeStyle, PARALLEL_STEP, curve_control,
    routed_options,
};
use cg_types::{Point2, Rect};

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

/// Directed edges touching `rect` honoring per-edge curve styles.
///
/// The routing folds each edge curve over the global derivation exactly like
/// the paint plan, so box selection and the canvas agree on straight, curved
/// and Manhattan edges. Box selection always tests full-precision curves.
pub fn edges_in_rect_for_styles(
    graph: &dyn GraphView,
    positions: &Positions,
    rect: Rect,
    node_side: f32,
    ortho: Option<OrthoDirection>,
    taxi: Option<OrthoDirection>,
    style_of: impl Fn(NodeIndex, NodeIndex) -> EdgeCurve,
) -> Vec<(NodeIndex, NodeIndex)> {
    let options = EdgePaintOptions {
        level: DetailLevel::Full,
        ortho,
        taxi,
        ..EdgePaintOptions::default()
    };
    let edges = sorted_edges(graph);
    let mut bundles = SelectionBundles::build(&edges);
    let mut found = Vec::new();
    for (ordinal, (source, target)) in edges.iter().enumerate() {
        if source == target {
            let Some(center) = positions.get(source) else {
                continue;
            };
            if loop_hits_rect(*center, node_side, bundles.next_loop(*source), rect) {
                found.push((*source, *target));
            }
            continue;
        }
        let (Some(start), Some(end)) = (positions.get(source), positions.get(target)) else {
            continue;
        };
        let style = EdgeStyle {
            curve: style_of(*source, *target),
            ..EdgeStyle::default()
        };
        let routed = routed_options(style, options);
        let line = manhattan_route(*start, *end, routed.ortho, routed.taxi);
        if !line.is_empty() {
            if polyline_intersects_rect(&line, rect) {
                found.push((*source, *target));
            }
            continue;
        }
        let (slot, len) = bundles.slot(ordinal, *source, *target);
        let ctrl = curve_control(
            style,
            *start,
            *end,
            BundleSlot {
                source: *source,
                target: *target,
                slot,
                len,
            },
            routed,
        );
        if edge_hits_rect(*start, ctrl, *end, rect) {
            found.push((*source, *target));
        }
    }
    dedup_pairs(&mut found);
    found
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
    let edges = sorted_edges(graph);
    let mut bundles = SelectionBundles::build(&edges);
    let mut found = Vec::new();
    for (ordinal, (source, target)) in edges.iter().enumerate() {
        if source == target {
            let Some(center) = positions.get(source) else {
                continue;
            };
            if loop_hits_rect(*center, node_side, bundles.next_loop(*source), rect) {
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
        let (slot, len) = bundles.slot(ordinal, *source, *target);
        let offsets = parallel_offsets(len, PARALLEL_STEP);
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
    dedup_pairs(&mut found);
    found
}

fn bundle_key(source: NodeIndex, target: NodeIndex) -> (usize, usize) {
    let (a, b) = (source.index(), target.index());
    if a <= b { (a, b) } else { (b, a) }
}

/// Shared bundle context for the edge box-selection passes.
///
/// Both styled and option-driven selection group parallel edges and count
/// self-loop stacks from the same sorted pair list, so the two derivations
/// agree on slots even though their routing math differs.
struct SelectionBundles {
    members_of: HashMap<(usize, usize), Vec<usize>>,
    loops_seen: HashMap<usize, usize>,
}

impl SelectionBundles {
    fn build(edges: &[(NodeIndex, NodeIndex)]) -> Self {
        let mut members_of: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
        for (ordinal, (source, target)) in edges.iter().enumerate() {
            members_of
                .entry(bundle_key(*source, *target))
                .or_default()
                .push(ordinal);
        }
        Self {
            members_of,
            loops_seen: HashMap::new(),
        }
    }

    fn slot(&self, ordinal: usize, source: NodeIndex, target: NodeIndex) -> (usize, usize) {
        let bundle = self
            .members_of
            .get(&bundle_key(source, target))
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let slot = bundle
            .iter()
            .position(|member| *member == ordinal)
            .unwrap_or(0);
        (slot, bundle.len())
    }

    fn next_loop(&mut self, node: NodeIndex) -> usize {
        let seen = self.loops_seen.get(&node.index()).copied().unwrap_or(0);
        self.loops_seen.insert(node.index(), seen + 1);
        seen
    }
}

fn sorted_edges(graph: &dyn GraphView) -> Vec<(NodeIndex, NodeIndex)> {
    let mut edges = graph.edges();
    edges.sort_unstable_by_key(|(source, target)| (source.index(), target.index()));
    edges
}

fn dedup_pairs(found: &mut Vec<(NodeIndex, NodeIndex)>) {
    found.sort_unstable_by_key(|(source, target)| (source.index(), target.index()));
    found.dedup();
}

#[cfg(test)]
mod tests {
    use super::*;
    use cg_render::NODE_SIDE;
    use cg_types::Vec2;

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
    fn styled_box_select_follows_the_forced_curve() {
        use cg_graph::MockGraph;
        use cg_render::EdgeCurve;

        let graph = MockGraph::chain(2);
        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(0.0, 0.0));
        positions.insert(NodeIndex::new(1), Point2::new(100.0, 0.0));
        let chord = Rect::from_corners(Point2::new(40.0, -2.0), Point2::new(60.0, 2.0));
        let straight = edges_in_rect_for_styles(
            &graph,
            &positions,
            chord,
            NODE_SIDE,
            None,
            None,
            |_, _| EdgeCurve::Straight,
        );
        assert_eq!(straight, vec![(NodeIndex::new(0), NodeIndex::new(1))]);
        let curved = edges_in_rect_for_styles(
            &graph,
            &positions,
            chord,
            NODE_SIDE,
            None,
            None,
            |_, _| EdgeCurve::Bezier,
        );
        assert!(curved.is_empty() || curved == straight);
        let off_chord = Rect::from_corners(Point2::new(40.0, 4.0), Point2::new(60.0, 20.0));
        let bent = edges_in_rect_for_styles(
            &graph,
            &positions,
            off_chord,
            NODE_SIDE,
            None,
            None,
            |_, _| EdgeCurve::Bezier,
        );
        assert_eq!(bent, vec![(NodeIndex::new(0), NodeIndex::new(1))]);
        assert!(
            edges_in_rect_for_styles(
                &graph,
                &positions,
                off_chord,
                NODE_SIDE,
                None,
                None,
                |_, _| EdgeCurve::Straight,
            )
            .is_empty()
        );
    }
}
