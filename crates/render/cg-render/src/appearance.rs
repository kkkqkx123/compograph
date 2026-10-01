//! Resolved element appearance, partial patches and shared defaults.

use crate::arrows::ArrowKind;
use crate::fill::{HOVER_NODE_FILL, NodeFill, SELECTED_NODE_FILL};
use crate::image::NodeImage;
use crate::shapes::NodeShape;
use crate::text::LabelStyle;

/// Outline of nodes carrying no mapping or bypass.
pub const DEFAULT_NODE_STROKE: u32 = 0x2c5f9e;

/// Stroke of edges carrying no mapping or bypass.
pub const DEFAULT_EDGE_TINT: u32 = 0x8a93a6;

/// Tint applied through the bypass to highlighted edges.
pub const HIGHLIGHT_EDGE_TINT: u32 = 0xff9f2e;

/// Resolved appearance of one node.
#[derive(Clone, Debug, PartialEq)]
pub struct NodeStyle {
    pub fill: NodeFill,
    pub stroke: u32,
    pub stroke_width: f32,
    pub opacity: f32,
    pub label_size: f32,
    pub scale: f32,
    pub shape: NodeShape,
    pub image: Option<NodeImage>,
    pub label: LabelStyle,
}

impl Default for NodeStyle {
    fn default() -> Self {
        Self {
            fill: NodeFill::default(),
            stroke: DEFAULT_NODE_STROKE,
            stroke_width: 1.5,
            opacity: 1.0,
            label_size: 12.0,
            scale: 1.0,
            shape: NodeShape::Square,
            image: None,
            label: LabelStyle::default(),
        }
    }
}

/// Routing of one edge, resolved per edge instead of per canvas.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EdgeCurve {
    /// Historical behavior: lone edges run straight, bundles curve.
    #[default]
    Auto,
    /// Always a straight segment.
    Straight,
    /// Always a curved segment, even when unbundled.
    Bezier,
    /// Single-corner taxi route.
    Taxi,
    /// Orthogonal route with up to two bends.
    Orthogonal,
}

/// Resolved appearance of one edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EdgeStyle {
    pub tint: u32,
    pub width: f32,
    pub opacity: f32,
    pub arrow_scale: f32,
    pub label_size: f32,
    pub arrow: ArrowKind,
    pub curve: EdgeCurve,
    pub label: LabelStyle,
}

impl Default for EdgeStyle {
    fn default() -> Self {
        Self {
            tint: DEFAULT_EDGE_TINT,
            width: 1.5,
            opacity: 1.0,
            arrow_scale: 1.0,
            label_size: 11.0,
            arrow: ArrowKind::Triangle,
            curve: EdgeCurve::Auto,
            label: LabelStyle::default(),
        }
    }
}

/// Partial node appearance; set fields override the base on resolution.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NodeStylePatch {
    pub fill: Option<NodeFill>,
    pub stroke: Option<u32>,
    pub stroke_width: Option<f32>,
    pub opacity: Option<f32>,
    pub label_size: Option<f32>,
    pub scale: Option<f32>,
    pub shape: Option<NodeShape>,
    pub image: Option<Option<NodeImage>>,
    pub label: Option<LabelStyle>,
}

impl NodeStylePatch {
    /// True when the patch carries no override.
    pub fn is_empty(&self) -> bool {
        self.fill.is_none()
            && self.stroke.is_none()
            && self.stroke_width.is_none()
            && self.opacity.is_none()
            && self.label_size.is_none()
            && self.scale.is_none()
            && self.shape.is_none()
            && self.image.is_none()
            && self.label.is_none()
    }

    /// Patch selecting a node through the bypass channel.
    pub fn selected() -> Self {
        Self {
            fill: Some(NodeFill::solid(SELECTED_NODE_FILL)),
            ..Self::default()
        }
    }

    /// Patch marking the hovered node through the bypass channel.
    pub fn hovered() -> Self {
        Self {
            fill: Some(NodeFill::solid(HOVER_NODE_FILL)),
            ..Self::default()
        }
    }

    /// Patch tinting a node, used for result group coloring.
    pub fn tinted(fill: u32) -> Self {
        Self {
            fill: Some(NodeFill::solid(fill)),
            ..Self::default()
        }
    }

    /// Patch filling a node with a linear gradient.
    pub fn gradient_fill(fill: NodeFill) -> Self {
        Self {
            fill: Some(fill),
            ..Self::default()
        }
    }

    /// Patch resizing a node, used for score-driven size mapping.
    pub fn rescaled(scale: f32) -> Self {
        Self {
            scale: Some(scale),
            ..Self::default()
        }
    }

    /// Patch reshaping a node.
    pub fn reshaped(shape: NodeShape) -> Self {
        Self {
            shape: Some(shape),
            ..Self::default()
        }
    }

    /// Patch showing a local background image on a node.
    pub fn with_image(image: NodeImage) -> Self {
        Self {
            image: Some(Some(image)),
            ..Self::default()
        }
    }

    /// Patch clearing any background image from a node.
    pub fn without_image() -> Self {
        Self {
            image: Some(None),
            ..Self::default()
        }
    }

    /// Patch replacing the label appearance.
    pub fn with_label(label: LabelStyle) -> Self {
        Self {
            label: Some(label),
            ..Self::default()
        }
    }

    pub fn apply_to(&self, base: &NodeStyle) -> NodeStyle {
        NodeStyle {
            fill: self.fill.unwrap_or(base.fill),
            stroke: self.stroke.unwrap_or(base.stroke),
            stroke_width: self.stroke_width.unwrap_or(base.stroke_width),
            opacity: self.opacity.unwrap_or(base.opacity),
            label_size: self.label_size.unwrap_or(base.label_size),
            scale: self.scale.unwrap_or(base.scale),
            shape: self.shape.unwrap_or(base.shape),
            image: self.image.clone().unwrap_or_else(|| base.image.clone()),
            label: self.label.unwrap_or(base.label),
        }
    }

    /// Applies the patch, exposed for selector sheets in the same crate family.
    pub fn apply_to_style(&self, base: &NodeStyle) -> NodeStyle {
        self.apply_to(base)
    }
}

/// Partial edge appearance; set fields override the base on resolution.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EdgeStylePatch {
    pub tint: Option<u32>,
    pub width: Option<f32>,
    pub opacity: Option<f32>,
    pub arrow_scale: Option<f32>,
    pub label_size: Option<f32>,
    pub arrow: Option<ArrowKind>,
    pub curve: Option<EdgeCurve>,
    pub label: Option<LabelStyle>,
}

impl EdgeStylePatch {
    /// True when the patch carries no override.
    pub fn is_empty(&self) -> bool {
        self.tint.is_none()
            && self.width.is_none()
            && self.opacity.is_none()
            && self.arrow_scale.is_none()
            && self.label_size.is_none()
            && self.arrow.is_none()
            && self.curve.is_none()
            && self.label.is_none()
    }

    /// Patch highlighting an edge through the bypass channel.
    pub fn highlighted() -> Self {
        Self {
            tint: Some(HIGHLIGHT_EDGE_TINT),
            width: Some(3.0),
            ..Self::default()
        }
    }

    /// Patch thickening an edge, used for spanning-tree emphasis.
    pub fn widened() -> Self {
        Self {
            width: Some(3.5),
            ..Self::default()
        }
    }

    /// Patch reheading an edge.
    pub fn reheaded(arrow: ArrowKind) -> Self {
        Self {
            arrow: Some(arrow),
            ..Self::default()
        }
    }

    /// Patch rerouting an edge.
    pub fn recurved(curve: EdgeCurve) -> Self {
        Self {
            curve: Some(curve),
            ..Self::default()
        }
    }

    /// Patch replacing the label appearance.
    pub fn with_label(label: LabelStyle) -> Self {
        Self {
            label: Some(label),
            ..Self::default()
        }
    }

    pub fn apply_to(&self, base: &EdgeStyle) -> EdgeStyle {
        EdgeStyle {
            tint: self.tint.unwrap_or(base.tint),
            width: self.width.unwrap_or(base.width),
            opacity: self.opacity.unwrap_or(base.opacity),
            arrow_scale: self.arrow_scale.unwrap_or(base.arrow_scale),
            label_size: self.label_size.unwrap_or(base.label_size),
            arrow: self.arrow.unwrap_or(base.arrow),
            curve: self.curve.unwrap_or(base.curve),
            label: self.label.unwrap_or(base.label),
        }
    }

    /// Applies the patch, exposed for selector sheets in the same crate family.
    pub fn apply_to_edge(&self, base: &EdgeStyle) -> EdgeStyle {
        self.apply_to(base)
    }
}

/// Default styles shared by every element.
#[derive(Clone, Debug, Default)]
pub struct StyleSheet {
    pub node: NodeStyle,
    pub edge: EdgeStyle,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fill::NodeFill;

    #[test]
    fn solid_fill_stays_the_default_appearance() {
        assert_eq!(
            NodeStyle::default().fill,
            NodeFill::solid(crate::fill::DEFAULT_NODE_FILL)
        );
        assert!(NodeStyle::default().fill.is_solid());
        assert_eq!(NodeFill::solid(0x112233).solid_fallback(), 0x112233);
    }

    #[test]
    fn shape_and_arrow_defaults_keep_legacy_look() {
        use crate::arrows::ArrowKind;
        use crate::shapes::NodeShape;

        assert_eq!(NodeStyle::default().shape, NodeShape::Square);
        assert_eq!(EdgeStyle::default().arrow, ArrowKind::Triangle);
        let reshaped = NodeStylePatch::reshaped(NodeShape::Circle).apply_to(&NodeStyle::default());
        assert_eq!(reshaped.shape, NodeShape::Circle);
        let reheaded = EdgeStylePatch::reheaded(ArrowKind::Diamond).apply_to(&EdgeStyle::default());
        assert_eq!(reheaded.arrow, ArrowKind::Diamond);
    }

    #[test]
    fn gradient_patch_replaces_the_whole_fill() {
        let base = NodeStyle::default();
        let patch = NodeStylePatch::gradient_fill(NodeFill::gradient(0x111111, 0x222222, 90.0));
        assert!(!patch.is_empty());
        let resolved = patch.apply_to(&base);
        assert_eq!(resolved.fill.start, 0x111111);
        assert_eq!(resolved.fill.end, 0x222222);
    }

    #[test]
    fn image_defaults_to_none_and_patch_sets_and_clears() {
        use crate::image::{ImageFit, NodeImage};

        assert!(NodeStyle::default().image.is_none());
        assert!(NodeStylePatch::default().is_empty());
        let base = NodeStyle::default();
        let spec = NodeImage::new("/tmp/a.png", ImageFit::Cover);
        let set = NodeStylePatch::with_image(spec.clone()).apply_to(&base);
        assert_eq!(set.image, Some(spec));
        assert!(
            !NodeStylePatch::with_image(NodeImage::new("/tmp/a.png", ImageFit::Contain)).is_empty()
        );
        let mut with_image = base.clone();
        with_image.image = set.image;
        let cleared = NodeStylePatch::without_image().apply_to(&with_image);
        assert!(cleared.image.is_none());
        let untouched = NodeStylePatch::default().apply_to(&base);
        assert!(untouched.image.is_none());
    }

    #[test]
    fn label_and_curve_patches_apply_and_clear() {
        use crate::text::{LabelAlign, LabelBackground, LabelStyle};

        let styled = LabelStyle {
            align: LabelAlign::Right,
            background: LabelBackground::RoundRect,
            background_color: 0xABCDEF,
            corner_radius: 6.0,
            padding: 5.0,
        };
        let node_patch = NodeStylePatch::with_label(styled);
        assert!(!node_patch.is_empty());
        let applied = node_patch.apply_to(&NodeStyle::default());
        assert_eq!(applied.label, styled);
        let untouched = NodeStylePatch::default().apply_to(&NodeStyle::default());
        assert_eq!(untouched.label, LabelStyle::default());
        let edge_patch = EdgeStylePatch::recurved(EdgeCurve::Taxi);
        assert!(!edge_patch.is_empty());
        let rerouted = edge_patch.apply_to(&EdgeStyle::default());
        assert_eq!(rerouted.curve, EdgeCurve::Taxi);
        assert_eq!(EdgeStyle::default().curve, EdgeCurve::Auto);
        let labeled = EdgeStylePatch::with_label(styled).apply_to(&EdgeStyle::default());
        assert_eq!(labeled.label.background_color, 0xABCDEF);
    }
}
