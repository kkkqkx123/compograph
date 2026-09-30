//! Software image export reusing cached paint plans.
//!
//! Two scopes are supported: the current viewport and the full graph bounds.
//! Plans are scaled by the magnification factor without rerunning layout.
//! Pixels encode as PNG so export needs no image dependency. Export covers
//! node and edge geometry only; labels stay canvas-only.

use std::collections::HashMap;

use cg_graph::{NodeIndex, Positions};
use cg_types::{Point2, Rect, Vec2};

use crate::camera::Camera;
use crate::lod::DetailLevel;
use crate::raster::rasterize;
use crate::style::{EdgeStyle, NodeStyle};
use crate::view::{
    EDGE_AGGREGATION_THRESHOLD, EdgePaintOptions, NODE_SIDE, PaintedArrow, PaintedEdge,
    PaintedNode, paint_arrows_for_level, paint_edges_for, paint_nodes_for,
};

pub use super::png::encode_png;
pub use super::raster::solid_background;

/// Export scope for image output.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExportScope {
    /// Exactly what the canvas shows.
    #[default]
    Viewport,
    /// Tight bounds around all node positions.
    FullGraph,
}

/// Sized export request.
#[derive(Clone, Copy, Debug)]
pub struct ExportRequest {
    pub scope: ExportScope,
    pub scale: f32,
    pub viewport: Vec2,
}

impl ExportRequest {
    pub fn pixel_size(&self, bounds: Rect) -> (u32, u32) {
        let width = (bounds.size.x * self.scale).round().clamp(1.0, 8192.0) as u32;
        let height = (bounds.size.y * self.scale).round().clamp(1.0, 8192.0) as u32;
        (width.max(1), height.max(1))
    }
}

/// Tight model-space bounds around all positions.
pub fn graph_bounds(positions: &Positions, pad: f32) -> Option<Rect> {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for point in positions.values() {
        min_x = min_x.min(point.x);
        min_y = min_y.min(point.y);
        max_x = max_x.max(point.x);
        max_y = max_y.max(point.y);
    }
    if !min_x.is_finite() {
        return None;
    }
    let origin = Point2::new(min_x - pad, min_y - pad);
    let size = Vec2::new(
        (max_x - min_x + pad * 2.0).max(1.0),
        (max_y - min_y + pad * 2.0).max(1.0),
    );
    Some(Rect::new(origin, size))
}

/// Scales screen plans by the export magnification.
pub fn scale_nodes(nodes: &[PaintedNode], scale: f32) -> Vec<PaintedNode> {
    nodes
        .iter()
        .map(|node| PaintedNode {
            id: node.id,
            origin: Point2::new(node.origin.x * scale, node.origin.y * scale),
            side: node.side * scale,
            fill: node.fill,
            stroke: node.stroke,
            stroke_width: node.stroke_width * scale,
            opacity: node.opacity,
            shape: node.shape,
            points: node
                .points
                .iter()
                .map(|point| Point2::new(point.x * scale, point.y * scale))
                .collect(),
            image: node.image.clone(),
        })
        .collect()
}

/// Scales edge plans by the export magnification.
pub fn scale_edges(edges: &[PaintedEdge], scale: f32) -> Vec<PaintedEdge> {
    edges
        .iter()
        .map(|edge| PaintedEdge {
            start: Point2::new(edge.start.x * scale, edge.start.y * scale),
            end: Point2::new(edge.end.x * scale, edge.end.y * scale),
            ctrl: edge
                .ctrl
                .map(|point| Point2::new(point.x * scale, point.y * scale)),
            loop_ctrls: edge.loop_ctrls.map(|[a, b]| {
                [
                    Point2::new(a.x * scale, a.y * scale),
                    Point2::new(b.x * scale, b.y * scale),
                ]
            }),
            bends: edge
                .bends
                .iter()
                .map(|point| Point2::new(point.x * scale, point.y * scale))
                .collect(),
            width: edge.width * scale,
            ..edge.clone()
        })
        .collect()
}

/// Scales arrow plans by the export magnification.
pub fn scale_arrows(arrows: &[PaintedArrow], scale: f32) -> Vec<PaintedArrow> {
    arrows
        .iter()
        .map(|arrow| PaintedArrow {
            tip: Point2::new(arrow.tip.x * scale, arrow.tip.y * scale),
            points: arrow
                .points
                .iter()
                .map(|point| Point2::new(point.x * scale, point.y * scale))
                .collect(),
            tint: arrow.tint,
            kind: arrow.kind,
        })
        .collect()
}

/// Owned inputs for one background image export.
///
/// Styles arrive resolved on the interface thread, so the background task
/// only runs plan math, rasterization and file writes with plain data.
#[derive(Clone, Debug)]
pub struct ExportSnapshot {
    pub node_ids: Vec<NodeIndex>,
    pub pairs: Vec<(NodeIndex, NodeIndex)>,
    pub positions: Positions,
    pub node_styles: HashMap<NodeIndex, NodeStyle>,
    pub edge_styles: HashMap<(NodeIndex, NodeIndex), EdgeStyle>,
    pub camera: Camera,
    pub viewport: Vec2,
    pub aggregate: bool,
}

/// Builds scaled plans for `scope` and renders them to a flat RGB buffer.
///
/// The full-graph scope fits a unit-zoom camera over the graph bounds so no
/// layout reruns; the viewport scope reuses the live camera. Returns the
/// pixels with their dimensions, or nothing when the graph is empty.
pub fn export_pixels(
    scope: ExportScope,
    request: ExportRequest,
    snapshot: &ExportSnapshot,
) -> Option<(Vec<u8>, u32, u32)> {
    let bounds = match scope {
        ExportScope::Viewport => Rect::new(Point2::ZERO, snapshot.viewport),
        ExportScope::FullGraph => graph_bounds(&snapshot.positions, NODE_SIDE)?,
    };
    let (camera, viewport) = match scope {
        ExportScope::Viewport => (snapshot.camera, snapshot.viewport),
        ExportScope::FullGraph => {
            let center = Point2::new(
                bounds.origin.x + bounds.size.x / 2.0,
                bounds.origin.y + bounds.size.y / 2.0,
            );
            (Camera::new(center, 1.0), bounds.size)
        }
    };
    let options = EdgePaintOptions {
        level: DetailLevel::Full,
        aggregate_threshold: EDGE_AGGREGATION_THRESHOLD,
        force_haystack: snapshot.aggregate,
        ortho: None,
        taxi: None,
    };
    let nodes = paint_nodes_for(
        &snapshot.node_ids,
        &snapshot.positions,
        &camera,
        viewport,
        |node| snapshot.node_styles.get(&node).cloned().unwrap_or_default(),
    );
    let edges = paint_edges_for(
        &snapshot.pairs,
        &snapshot.positions,
        &camera,
        viewport,
        options,
        |source, target| {
            snapshot
                .edge_styles
                .get(&(source, target))
                .copied()
                .unwrap_or_default()
        },
    );
    let arrows = paint_arrows_for_level(&edges, DetailLevel::Full);
    let (width, height) = request.pixel_size(bounds);
    let pixels = rasterize(
        width,
        height,
        &scale_nodes(&nodes, request.scale),
        &scale_edges(&edges, request.scale),
        &scale_arrows(&arrows, request.scale),
        [24, 28, 36],
    );
    Some((pixels, width, height))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_cover_all_positions_with_padding() {
        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(0.0, 0.0));
        positions.insert(NodeIndex::new(1), Point2::new(100.0, 50.0));
        let bounds = graph_bounds(&positions, 10.0).expect("bounds exist");
        assert_eq!(bounds.origin, Point2::new(-10.0, -10.0));
        assert_eq!(bounds.size, Vec2::new(120.0, 70.0));
        assert!(graph_bounds(&Positions::new(), 10.0).is_none());
    }

    #[test]
    fn scaled_export_doubles_image_dimensions() {
        let request = ExportRequest {
            scope: ExportScope::FullGraph,
            scale: 2.0,
            viewport: Vec2::new(100.0, 50.0),
        };
        let bounds = Rect::new(Point2::ZERO, Vec2::new(100.0, 50.0));
        assert_eq!(request.pixel_size(bounds), (200, 100));
    }

    #[test]
    fn export_viewport_scales_with_the_request() {
        use crate::style::{EdgeStyle, NodeStyle};

        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(0.0, 0.0));
        positions.insert(NodeIndex::new(1), Point2::new(40.0, 0.0));
        let mut node_styles = HashMap::new();
        node_styles.insert(NodeIndex::new(0), NodeStyle::default());
        node_styles.insert(NodeIndex::new(1), NodeStyle::default());
        let mut edge_styles = HashMap::new();
        edge_styles.insert((NodeIndex::new(0), NodeIndex::new(1)), EdgeStyle::default());
        let snapshot = ExportSnapshot {
            node_ids: vec![NodeIndex::new(0), NodeIndex::new(1)],
            pairs: vec![(NodeIndex::new(0), NodeIndex::new(1))],
            positions,
            node_styles,
            edge_styles,
            camera: Camera::new(Point2::ZERO, 1.0),
            viewport: Vec2::new(100.0, 50.0),
            aggregate: false,
        };
        let request = ExportRequest {
            scope: ExportScope::Viewport,
            scale: 2.0,
            viewport: Vec2::new(100.0, 50.0),
        };
        let (pixels, width, height) =
            export_pixels(ExportScope::Viewport, request, &snapshot).expect("viewport exports");
        assert_eq!((width, height), (200, 100));
        assert_eq!(pixels.len(), 200 * 100 * 3);
        assert!(
            export_pixels(
                ExportScope::FullGraph,
                ExportRequest {
                    scope: ExportScope::FullGraph,
                    scale: 1.0,
                    viewport: Vec2::new(100.0, 50.0),
                },
                &ExportSnapshot {
                    node_ids: Vec::new(),
                    pairs: Vec::new(),
                    positions: Positions::new(),
                    node_styles: HashMap::new(),
                    edge_styles: HashMap::new(),
                    camera: Camera::new(Point2::ZERO, 1.0),
                    viewport: Vec2::new(100.0, 50.0),
                    aggregate: false,
                },
            )
            .is_none()
        );
    }
}
