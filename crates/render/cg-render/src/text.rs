//! Label paint planning for the graph canvas.
//!
//! Labels are planned separately from node and edge geometry because they are
//! shaped as text rather than stroked as paths. Planning stays free of gpui
//! types so it can be exercised headlessly; [`crate::view::graph_view`] turns
//! the resulting plan into shaped lines at paint time.
//!
//! Labels anchor just below the node body and are centered horizontally by the
//! renderer. Nodes without text are skipped, and the coarsest detail level
//! drops labels entirely to keep dense views readable.

use cg_geometry::{cubic_bezier, quadratic_bezier};
use cg_graph::NodeIndex;
use cg_types::{Point2, Vec2};

use crate::lod::DetailLevel;
use crate::view::{PaintedEdge, PaintedNode};

/// Vertical gap, in pixels, between a node body and its label baseline.
pub const LABEL_GAP: f32 = 4.0;

/// Fallback label size used when a resolved node style carries none.
pub const DEFAULT_LABEL_SIZE: f32 = 12.0;

/// Text color of a node label.
pub const DEFAULT_LABEL_COLOR: u32 = 0x1f2933;

/// Vertical gap, in pixels, between an edge anchor and its label top edge.
pub const EDGE_LABEL_GAP: f32 = 3.0;

/// Fallback label size used when a resolved edge style carries none.
pub const DEFAULT_EDGE_LABEL_SIZE: f32 = 11.0;

/// Text color of an edge label.
pub const DEFAULT_EDGE_LABEL_COLOR: u32 = 0x39424e;

/// Line height multiplier applied to the font size for stacked label lines.
pub const LABEL_LINE_HEIGHT_SCALE: f32 = 1.25;

/// Maximum characters per label line before hard wrapping.
///
/// Wrapping cuts at character boundaries rather than spaces so CJK text
/// without word separators still breaks; Latin words may split mid-word,
/// which is acceptable without rich text support.
pub const MAX_LABEL_CHARS_PER_LINE: usize = 24;

/// Height of one label line for `size`, used to stack wrapped lines.
pub fn line_height(size: f32) -> f32 {
    size.max(1.0) * LABEL_LINE_HEIGHT_SCALE
}

/// Splits label text into drawable lines.
///
/// Lines break on explicit newlines first, then overlong lines hard-wrap at
/// character boundaries. Blank lines are preserved as empty entries so the
/// vertical rhythm stays aligned with the source text.
pub fn split_label_lines(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in text.split('\n') {
        wrap_label_line(line, &mut out);
    }
    if out.is_empty() {
        out.push(String::new());
    }
    out
}

/// Pushes one newline-delimited line, wrapping it when overlong.
fn wrap_label_line(line: &str, out: &mut Vec<String>) {
    if line.chars().count() <= MAX_LABEL_CHARS_PER_LINE {
        out.push(line.to_string());
        return;
    }
    let mut current = String::new();
    let mut count = 0usize;
    for glyph in line.chars() {
        current.push(glyph);
        count += 1;
        if count >= MAX_LABEL_CHARS_PER_LINE {
            out.push(std::mem::take(&mut current));
            count = 0;
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
}

/// One label scheduled for painting, in screen pixels.
///
/// `origin` is the label's horizontal center and its top edge; the renderer
/// resolves the baseline from the line height.
#[derive(Clone, Debug, PartialEq)]
pub struct PaintedLabel {
    pub id: NodeIndex,
    pub text: String,
    pub origin: Point2,
    pub size: f32,
    pub color: u32,
}

/// Whether `level` still draws labels.
///
/// The minimal level trades labels for frame rate on dense or far-out views,
/// matching the arrow and curve downgrades applied to edges.
pub fn draws_labels(level: DetailLevel) -> bool {
    !matches!(level, DetailLevel::Minimal)
}

/// One edge label scheduled for painting, in screen pixels.
///
/// `origin` follows the node label convention: the label's horizontal center
/// and its top edge. The text shows the edge weight, the only edge payload
/// the store carries, so labels never need a schema change.
#[derive(Clone, Debug, PartialEq)]
pub struct PaintedEdgeLabel {
    pub source: NodeIndex,
    pub target: NodeIndex,
    pub text: String,
    pub origin: Point2,
    pub size: f32,
    pub color: u32,
}

/// Anchor of an edge label along the painted path.
///
/// Straight edges anchor at the segment midpoint and curved edges at the
/// curve midpoint, so the label tracks the visible geometry. Polylines anchor
/// at the middle of the bend list and self loops at the cubic midpoint.
pub fn edge_label_anchor(edge: &PaintedEdge) -> Point2 {
    if let Some([ctrl_a, ctrl_b]) = edge.loop_ctrls {
        return cubic_bezier(edge.start, ctrl_a, ctrl_b, edge.end, 0.5);
    }
    let bends = edge.bends();
    if !bends.is_empty() {
        let mut line = Vec::with_capacity(bends.len() + 2);
        line.push(edge.start);
        line.extend_from_slice(&bends);
        line.push(edge.end);
        let mid = line.len() / 2;
        if line.len() % 2 == 1 {
            return line[mid];
        }
        let (a, b) = (line[mid - 1], line[mid]);
        return Point2::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
    }
    if let Some(ctrl) = edge.ctrl {
        return quadratic_bezier(edge.start, ctrl, edge.end, 0.5);
    }
    Point2::new(
        (edge.start.x + edge.end.x) / 2.0,
        (edge.start.y + edge.end.y) / 2.0,
    )
}

/// Builds the edge label plan for the edges that survived culling.
///
/// `label_of` resolves each directed pair to its weight text and `size_of`
/// resolves its font size. Empty text is dropped and the minimal detail level
/// returns an empty plan, matching the node label behavior.
pub fn paint_edge_labels_for(
    edges: &[PaintedEdge],
    level: DetailLevel,
    label_of: impl Fn(NodeIndex, NodeIndex) -> Option<String>,
    size_of: impl Fn(NodeIndex, NodeIndex) -> f32,
) -> Vec<PaintedEdgeLabel> {
    if !draws_labels(level) {
        return Vec::new();
    }
    let mut labels = Vec::new();
    for edge in edges {
        let Some(text) = label_of(edge.source, edge.target) else {
            continue;
        };
        if text.trim().is_empty() {
            continue;
        }
        let size = size_of(edge.source, edge.target);
        let size = if size.is_finite() && size > 0.0 {
            size
        } else {
            DEFAULT_EDGE_LABEL_SIZE
        };
        let anchor = edge_label_anchor(edge);
        let origin = anchor + Vec2::new(0.0, EDGE_LABEL_GAP);
        labels.push(PaintedEdgeLabel {
            source: edge.source,
            target: edge.target,
            text,
            origin,
            size,
            color: DEFAULT_EDGE_LABEL_COLOR,
        });
    }
    labels
}

/// Builds the label plan for the nodes that survived culling.
///
/// `nodes` is the visible node plan so labels never outlive their bodies;
/// `label_of` resolves each node's text, and `size_of` resolves its font size.
/// Empty or whitespace-only text is dropped, and the minimal detail level
/// returns an empty plan.
pub fn paint_labels_for(
    nodes: &[PaintedNode],
    level: DetailLevel,
    label_of: impl Fn(NodeIndex) -> Option<String>,
    size_of: impl Fn(NodeIndex) -> f32,
) -> Vec<PaintedLabel> {
    if !draws_labels(level) {
        return Vec::new();
    }
    let mut labels = Vec::new();
    for node in nodes {
        let Some(text) = label_of(node.id) else {
            continue;
        };
        if text.trim().is_empty() {
            continue;
        }
        let size = size_of(node.id);
        let size = if size.is_finite() && size > 0.0 {
            size
        } else {
            DEFAULT_LABEL_SIZE
        };
        let center_x = node.origin.x + node.side / 2.0;
        let top = node.origin.y + node.side + LABEL_GAP;
        labels.push(PaintedLabel {
            id: node.id,
            text,
            origin: Point2::new(center_x, top),
            size,
            color: DEFAULT_LABEL_COLOR,
        });
    }
    labels
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: usize, origin_x: f32, origin_y: f32, side: f32) -> PaintedNode {
        PaintedNode {
            id: NodeIndex::new(id),
            origin: Point2::new(origin_x, origin_y),
            side,
            fill: 0,
            stroke: 0,
            stroke_width: 0.0,
            opacity: 1.0,
            shape: crate::shapes::NodeShape::Square,
            points: Vec::new(),
        }
    }

    #[test]
    fn labels_follow_the_visible_node_plan() {
        let nodes = vec![node(0, 100.0, 50.0, 24.0), node(1, 200.0, 80.0, 20.0)];
        let labels = paint_labels_for(
            &nodes,
            DetailLevel::Full,
            |id| Some(format!("n{}", id.index())),
            |_| 12.0,
        );
        assert_eq!(labels.len(), 2);
        let first = &labels[0];
        assert_eq!(first.text, "n0");
        assert_eq!(first.origin, Point2::new(112.0, 78.0));
        assert_eq!(first.size, 12.0);
        assert_eq!(labels[1].origin, Point2::new(210.0, 104.0));
    }

    #[test]
    fn empty_and_missing_text_is_skipped() {
        let nodes = vec![node(0, 0.0, 0.0, 24.0), node(1, 0.0, 0.0, 24.0)];
        let labels = paint_labels_for(
            &nodes,
            DetailLevel::Full,
            |id| {
                if id.index() == 0 {
                    Some("   ".to_string())
                } else {
                    None
                }
            },
            |_| 12.0,
        );
        assert!(labels.is_empty());
    }

    #[test]
    fn minimal_detail_drops_labels() {
        let nodes = vec![node(0, 0.0, 0.0, 24.0)];
        let labels = paint_labels_for(
            &nodes,
            DetailLevel::Minimal,
            |_| Some("a".to_string()),
            |_| 12.0,
        );
        assert!(labels.is_empty());
        assert!(!draws_labels(DetailLevel::Minimal));
        assert!(draws_labels(DetailLevel::Simplified));
        assert!(draws_labels(DetailLevel::Full));
    }

    #[test]
    fn straight_and_curved_edges_anchor_at_their_midpoints() {
        use crate::arrows::ArrowKind;
        use crate::view::PaintedEdge;
        fn edge(start: Point2, end: Point2, ctrl: Option<Point2>) -> PaintedEdge {
            PaintedEdge {
                source: NodeIndex::new(0),
                target: NodeIndex::new(1),
                start,
                end,
                ctrl,
                loop_ctrls: None,
                bend_a: None,
                bend_b: None,
                aggregated: false,
                tint: 0,
                width: 1.0,
                opacity: 1.0,
                arrow: ArrowKind::Triangle,
                arrow_scale: 1.0,
            }
        }
        let straight = edge(Point2::new(0.0, 0.0), Point2::new(100.0, 0.0), None);
        assert_eq!(edge_label_anchor(&straight), Point2::new(50.0, 0.0));
        let curved = edge(
            Point2::new(0.0, 0.0),
            Point2::new(20.0, 0.0),
            Some(Point2::new(10.0, 10.0)),
        );
        assert_eq!(edge_label_anchor(&curved), Point2::new(10.0, 5.0));
        let labels = paint_edge_labels_for(
            &[straight, curved],
            DetailLevel::Full,
            |_, _| Some("1".to_string()),
            |_, _| 11.0,
        );
        assert_eq!(labels.len(), 2);
        assert_eq!(labels[0].origin, Point2::new(50.0, EDGE_LABEL_GAP));
        assert_eq!(labels[1].origin, Point2::new(10.0, 5.0 + EDGE_LABEL_GAP));
    }

    #[test]
    fn edge_labels_hide_on_minimal_and_skip_empty_text() {
        use crate::arrows::ArrowKind;
        use crate::view::PaintedEdge;
        let edge = PaintedEdge {
            source: NodeIndex::new(0),
            target: NodeIndex::new(1),
            start: Point2::ZERO,
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
        let hidden = paint_edge_labels_for(
            std::slice::from_ref(&edge),
            DetailLevel::Minimal,
            |_, _| Some("1".to_string()),
            |_, _| 11.0,
        );
        assert!(hidden.is_empty());
        let blank = paint_edge_labels_for(
            &[edge],
            DetailLevel::Full,
            |_, _| Some("  ".to_string()),
            |_, _| 11.0,
        );
        assert!(blank.is_empty());
    }

    #[test]
    fn multiline_text_splits_and_wraps_without_panicking() {
        assert_eq!(split_label_lines("hello"), vec!["hello".to_string()]);
        assert_eq!(
            split_label_lines("你好\n世界"),
            vec!["你好".to_string(), "世界".to_string()]
        );
        assert_eq!(
            split_label_lines("a\n\nb"),
            vec!["a".to_string(), String::new(), "b".to_string()]
        );
        let long: String = "中".repeat(MAX_LABEL_CHARS_PER_LINE * 2 + 2);
        let wrapped = split_label_lines(&long);
        assert_eq!(wrapped.len(), 3);
        assert!(
            wrapped
                .iter()
                .all(|line| line.chars().count() <= MAX_LABEL_CHARS_PER_LINE)
        );
        assert_eq!(wrapped.concat(), long);
        assert_eq!(line_height(12.0), 15.0);
        assert_eq!(line_height(0.0), LABEL_LINE_HEIGHT_SCALE);
    }

    #[test]
    fn invalid_sizes_fall_back_to_the_default() {
        let nodes = vec![node(0, 0.0, 0.0, 24.0)];
        let zero = paint_labels_for(
            &nodes,
            DetailLevel::Full,
            |_| Some("a".to_string()),
            |_| 0.0,
        );
        assert_eq!(zero[0].size, DEFAULT_LABEL_SIZE);
        let nan = paint_labels_for(
            &nodes,
            DetailLevel::Full,
            |_| Some("a".to_string()),
            |_| f32::NAN,
        );
        assert_eq!(nan[0].size, DEFAULT_LABEL_SIZE);
    }
}
