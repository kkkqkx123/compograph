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

use cg_graph::NodeIndex;
use cg_types::Point2;

use crate::lod::DetailLevel;
use crate::view::PaintedNode;

/// Vertical gap, in pixels, between a node body and its label baseline.
pub const LABEL_GAP: f32 = 4.0;

/// Fallback label size used when a resolved node style carries none.
pub const DEFAULT_LABEL_SIZE: f32 = 12.0;

/// Text color of a node label.
pub const DEFAULT_LABEL_COLOR: u32 = 0x1f2933;

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
