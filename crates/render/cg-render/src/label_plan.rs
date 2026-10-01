//! Node and edge label paint planning for the graph canvas.

use cg_graph::NodeIndex;
use cg_types::{Point2, Vec2};

use crate::label_envelope::{EDGE_LABEL_GAP, edge_label_anchor, node_label_origin};
use crate::label_style::{
    DEFAULT_EDGE_LABEL_COLOR, DEFAULT_EDGE_LABEL_SIZE, DEFAULT_LABEL_COLOR, DEFAULT_LABEL_SIZE,
    LabelAlign, LabelBackground, LabelStyle,
};
use crate::lod::DetailLevel;
use crate::view::{PaintedEdge, PaintedNode};

/// One label scheduled for painting, in screen pixels.
///
/// `origin` is the label's horizontal anchor and its top edge; center aligned
/// text centers on the anchor while left and right aligned text start or end
/// there. The renderer resolves the baseline from the line height. `rotation`
/// stays zero because the canvas paints horizontally; selection and bounds
/// use the same horizontal envelope so hits match the visible text.
/// `background` draws a plate behind the text in `background_color` with
/// `corner_radius` and `padding`; the default keeps the historical bare text
/// look.
#[derive(Clone, Debug, PartialEq)]
pub struct PaintedLabel {
    pub id: NodeIndex,
    pub text: String,
    pub origin: Point2,
    pub size: f32,
    pub color: u32,
    pub rotation: f32,
    pub background: LabelBackground,
    pub align: LabelAlign,
    pub background_color: u32,
    pub corner_radius: f32,
    pub padding: f32,
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
/// `origin` follows the node label convention: the label's horizontal anchor
/// and its top edge. The text shows the edge weight, the only edge payload
/// the store carries, so labels never need a schema change. `rotation` stays
/// zero to match the horizontal canvas paint; [`edge_label_angle`] remains as
/// a geometry helper for a future rotated paint path.
///
/// [`edge_label_angle`]: crate::label_envelope::edge_label_angle
#[derive(Clone, Debug, PartialEq)]
pub struct PaintedEdgeLabel {
    pub source: NodeIndex,
    pub target: NodeIndex,
    pub text: String,
    pub origin: Point2,
    pub size: f32,
    pub color: u32,
    pub rotation: f32,
    pub background: LabelBackground,
    pub align: LabelAlign,
    pub background_color: u32,
    pub corner_radius: f32,
    pub padding: f32,
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
    paint_edge_labels_for_with_style(edges, level, label_of, size_of, |_, _| {
        LabelStyle::default()
    })
}

/// Builds the edge label plan carrying per-edge label styles.
pub fn paint_edge_labels_for_with_style(
    edges: &[PaintedEdge],
    level: DetailLevel,
    label_of: impl Fn(NodeIndex, NodeIndex) -> Option<String>,
    size_of: impl Fn(NodeIndex, NodeIndex) -> f32,
    style_of: impl Fn(NodeIndex, NodeIndex) -> LabelStyle,
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
        let style = style_of(edge.source, edge.target);
        labels.push(PaintedEdgeLabel {
            source: edge.source,
            target: edge.target,
            text,
            origin,
            size,
            color: DEFAULT_EDGE_LABEL_COLOR,
            rotation: 0.0,
            background: style.background,
            align: style.align,
            background_color: style.background_color,
            corner_radius: style.radius(),
            padding: style.padding(),
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
    paint_labels_for_with_style(nodes, level, label_of, size_of, |_| {
        LabelStyle::default()
    })
}

/// Builds the label plan carrying per-node label styles.
pub fn paint_labels_for_with_style(
    nodes: &[PaintedNode],
    level: DetailLevel,
    label_of: impl Fn(NodeIndex) -> Option<String>,
    size_of: impl Fn(NodeIndex) -> f32,
    style_of: impl Fn(NodeIndex) -> LabelStyle,
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
        let center = Point2::new(node.origin.x + node.side / 2.0, node.origin.y + node.side / 2.0);
        let style = style_of(node.id);
        let origin = node_label_origin(center, node.side, &style);
        labels.push(PaintedLabel {
            id: node.id,
            text,
            origin,
            size,
            color: DEFAULT_LABEL_COLOR,
            rotation: 0.0,
            background: style.background,
            align: style.align,
            background_color: style.background_color,
            corner_radius: style.radius(),
            padding: style.padding(),
        });
    }
    labels
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::label_envelope::edge_label_angle;
    use crate::label_style::LabelBackground;

    fn node(id: usize, origin_x: f32, origin_y: f32, side: f32) -> PaintedNode {
        PaintedNode {
            id: NodeIndex::new(id),
            origin: Point2::new(origin_x, origin_y),
            side,
            fill: crate::style::NodeFill::solid(0),
            stroke: 0,
            stroke_width: 0.0,
            opacity: 1.0,
            shape: crate::shapes::NodeShape::Square,
            points: Vec::new(),
            image: None,
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
            bends: Vec::new(),
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

    #[test]
    fn node_labels_stay_horizontal_without_background() {
        let nodes = vec![node(0, 0.0, 0.0, 24.0)];
        let labels = paint_labels_for(
            &nodes,
            DetailLevel::Full,
            |_| Some("a".to_string()),
            |_| 12.0,
        );
        assert_eq!(labels.len(), 1);
        assert_eq!(labels[0].rotation, 0.0);
        assert_eq!(labels[0].background, LabelBackground::None);
    }

    #[test]
    fn edge_labels_follow_the_edge_tangent_readably() {
        use crate::arrows::ArrowKind;
        use crate::view::PaintedEdge;
        fn edge(start: Point2, end: Point2) -> PaintedEdge {
            PaintedEdge {
                source: NodeIndex::new(0),
                target: NodeIndex::new(1),
                start,
                end,
                ctrl: None,
                loop_ctrls: None,
                bends: Vec::new(),
                aggregated: false,
                tint: 0,
                width: 1.0,
                opacity: 1.0,
                arrow: ArrowKind::Triangle,
                arrow_scale: 1.0,
            }
        }
        let horizontal = edge(Point2::new(0.0, 0.0), Point2::new(100.0, 0.0));
        assert!((edge_label_angle(&horizontal)).abs() < 1e-4);
        let vertical = edge(Point2::new(0.0, 0.0), Point2::new(0.0, 100.0));
        assert!((edge_label_angle(&vertical) - std::f32::consts::FRAC_PI_2).abs() < 1e-4);
        let backwards = edge(Point2::new(100.0, 0.0), Point2::new(0.0, 0.0));
        assert!((edge_label_angle(&backwards)).abs() < 1e-4);
        let labels = paint_edge_labels_for(
            &[horizontal, vertical],
            DetailLevel::Full,
            |_, _| Some("1".to_string()),
            |_, _| 11.0,
        );
        assert_eq!(labels.len(), 2);
        for label in &labels {
            assert!((label.rotation).abs() < 1e-4);
            assert_eq!(label.background, LabelBackground::None);
        }
    }

    #[test]
    fn styled_labels_carry_background_and_padding() {
        let nodes = vec![node(0, 0.0, 0.0, 24.0)];
        let style = LabelStyle {
            background: LabelBackground::RoundRect,
            background_color: 0xABCDEF,
            corner_radius: 6.0,
            padding: 5.0,
            ..LabelStyle::default()
        };
        let labels = paint_labels_for_with_style(
            &nodes,
            DetailLevel::Full,
            |_| Some("a".to_string()),
            |_| 12.0,
            |_| style,
        );
        assert_eq!(labels.len(), 1);
        assert_eq!(labels[0].background, LabelBackground::RoundRect);
        assert_eq!(labels[0].background_color, 0xABCDEF);
        assert_eq!(labels[0].corner_radius, 6.0);
        assert_eq!(labels[0].padding, 5.0);
        let envelope =
            crate::label_envelope::label_envelope_styled(labels[0].origin, "a", 12.0, 0.0, &style);
        let bare = crate::label_envelope::label_envelope(
            labels[0].origin,
            "a",
            12.0,
            0.0,
            LabelBackground::None,
        );
        assert!((envelope.size.x - bare.size.x - 10.0).abs() < 1e-3);
    }
}
