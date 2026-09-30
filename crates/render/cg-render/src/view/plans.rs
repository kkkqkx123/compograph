//! Paint plan types shared by node, edge, arrow and canvas passes.
//!
//! These structs are screen pixel plans: layout and style resolution produce
//! them headlessly, then the canvas element and the software rasterizer
//! consume them. Keeping the vocabulary in one leaf module lets plan
//! producers and painters depend only on types without pulling in gpui.

use cg_geometry::OrthoDirection;
use cg_graph::NodeIndex;
use cg_types::{Point2, Vec2};

use crate::arrows::ArrowKind;
use crate::image::NodeImage;
use crate::lod::DetailLevel;
use crate::shapes::NodeShape;
use crate::style::NodeFill;

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

/// Length of the arrowhead along the edge direction, in screen pixels.
pub const ARROW_LENGTH: f32 = 10.0;

/// Half width of the arrowhead across the edge direction.
pub const ARROW_HALF_WIDTH: f32 = 4.0;

/// Outline of the rubber-band box-selection rectangle.
pub const RUBBER_BAND_STROKE: u32 = 0x4a9eff;

/// A node body scheduled for painting, in screen pixels.
///
/// Squares keep the fast rectangle path through `origin` and `side`; every
/// shape also carries its screen pixel polygon in `points` so drawing, export
/// and tests share one vertex table. The stroke outline reuses the resolved
/// style border so the canvas and the export raster agree. The fill carries
/// the full gradient description; minimal detail falls back to its solid end.
/// The image carries the background picture specification; minimal detail
/// clears it so dense views keep the fast solid path.
#[derive(Clone, Debug)]
pub struct PaintedNode {
    pub id: NodeIndex,
    pub origin: Point2,
    pub side: f32,
    pub fill: NodeFill,
    pub stroke: u32,
    pub stroke_width: f32,
    pub opacity: f32,
    pub shape: NodeShape,
    pub points: Vec<Point2>,
    pub image: Option<NodeImage>,
}

/// An edge polyline scheduled for painting, in screen pixels.
#[derive(Clone, Debug)]
pub struct PaintedEdge {
    pub source: NodeIndex,
    pub target: NodeIndex,
    pub start: Point2,
    pub end: Point2,
    /// Control point for curved edges; straight edges carry none.
    pub ctrl: Option<Point2>,
    /// Control points of a self loop; only set when start equals end.
    pub loop_ctrls: Option<[Point2; 2]>,
    /// Ordered interior waypoints from start to end, in screen pixels.
    ///
    /// Empty means straight or curved, one entry means a single fold, more
    /// entries mean a multi-segment polyline. Orthogonal routes store their
    /// one or two bends as the first entries; user waypoints store the full
    /// cleaned sequence.
    pub bends: Vec<Point2>,
    /// True when the edge was simplified as a haystack fan-out for dense bundles.
    /// Explicit orthogonal and taxi routes keep this false; they carry bends
    /// instead of a bundle simplification.
    pub aggregated: bool,
    pub tint: u32,
    pub width: f32,
    pub opacity: f32,
    pub arrow: ArrowKind,
    pub arrow_scale: f32,
}

impl PaintedEdge {
    pub(crate) fn straight(
        source: NodeIndex,
        target: NodeIndex,
        start: Point2,
        end: Point2,
        tint: u32,
        width: f32,
        opacity: f32,
    ) -> Self {
        Self {
            source,
            target,
            start,
            end,
            ctrl: None,
            loop_ctrls: None,
            bends: Vec::new(),
            aggregated: false,
            tint,
            width,
            opacity,
            arrow: ArrowKind::Triangle,
            arrow_scale: 1.0,
        }
    }

    pub(crate) fn with_arrow(mut self, arrow: ArrowKind, arrow_scale: f32) -> Self {
        self.arrow = arrow;
        self.arrow_scale = arrow_scale;
        self
    }

    /// Interior points of the painted path excluding the endpoints.
    pub fn bends(&self) -> Vec<Point2> {
        self.bends.clone()
    }

    /// Full painted polyline from start through bends to end.
    pub fn polyline(&self) -> Vec<Point2> {
        let mut line = Vec::with_capacity(self.bends.len() + 2);
        line.push(self.start);
        line.extend_from_slice(&self.bends);
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
    /// Single-corner taxi route; wins over `ortho` when both are set.
    pub taxi: Option<OrthoDirection>,
}

impl Default for EdgePaintOptions {
    fn default() -> Self {
        Self {
            level: DetailLevel::Full,
            aggregate_threshold: EDGE_AGGREGATION_THRESHOLD,
            force_haystack: false,
            ortho: None,
            taxi: None,
        }
    }
}

/// An arrowhead polygon scheduled for painting, in screen pixels.
///
/// Triangles keep three points with the tip first; the remaining kinds carry
/// their full vertex tables in `points` with the tip first for oriented
/// shapes. Dots center on `tip` and need no orientation.
#[derive(Clone, Debug)]
pub struct PaintedArrow {
    pub tip: Point2,
    pub points: Vec<Point2>,
    pub tint: u32,
    pub kind: ArrowKind,
}

/// Rubber-band rectangle scheduled for painting, in screen pixels.
#[derive(Clone, Copy, Debug)]
pub struct PaintedRubberBand {
    pub origin: Point2,
    pub size: Vec2,
}
