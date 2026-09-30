//! Software image export reusing cached paint plans.
//!
//! Two scopes are supported: the current viewport and the full graph bounds.
//! Plans are scaled by the magnification factor without rerunning layout.
//! Pixels encode as PNG so export needs no image dependency. Export covers
//! node and edge geometry only; labels stay canvas-only.

use std::collections::HashMap;

use cg_geometry::{
    BEZIER_HIT_SAMPLES, point_in_polygon, sample_cubic_bezier, sample_quadratic_bezier,
};
use cg_graph::{NodeIndex, Positions};
use cg_types::{Point2, Rect, Vec2};

use crate::camera::Camera;
use crate::lod::DetailLevel;
use crate::style::{EdgeStyle, NodeStyle};
use crate::view::{
    EDGE_AGGREGATION_THRESHOLD, EdgePaintOptions, NODE_SIDE, PaintedArrow, PaintedEdge,
    PaintedNode, paint_arrows_for_level, paint_edges_for, paint_nodes_for,
};

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
            bend_a: edge
                .bend_a
                .map(|point| Point2::new(point.x * scale, point.y * scale)),
            bend_b: edge
                .bend_b
                .map(|point| Point2::new(point.x * scale, point.y * scale)),
            width: edge.width * scale,
            ..*edge
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
        |node| snapshot.node_styles.get(&node).copied().unwrap_or_default(),
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

/// Encodes a flat RGB buffer as PNG with uncompressed deflate blocks.
///
/// The encoder is self-contained so image export needs no image dependency:
/// rows carry a zero filter byte and travel as stored deflate blocks inside a
/// zlib stream, with CRC32 per chunk and Adler32 over the stream. Short pixel
/// buffers are zero-padded so truncated plans still produce a valid image.
pub fn encode_png(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
    const SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];
    let mut out = Vec::new();
    out.extend_from_slice(&SIGNATURE);
    let mut head = Vec::with_capacity(13);
    head.extend_from_slice(&width.to_be_bytes());
    head.extend_from_slice(&height.to_be_bytes());
    head.extend_from_slice(&[8, 2, 0, 0, 0]);
    push_chunk(&mut out, b"IHDR", &head);
    let row_bytes = width as usize * 3;
    let mut raw = Vec::with_capacity(height as usize * (row_bytes + 1));
    let mut rows = pixels.chunks(row_bytes.max(1));
    for _ in 0..height {
        raw.extend_from_slice(&[0]);
        match rows.next() {
            Some(bytes) => {
                raw.extend_from_slice(bytes);
                raw.resize(raw.len() + row_bytes.saturating_sub(bytes.len()), 0);
            }
            None => raw.resize(raw.len() + row_bytes, 0),
        }
    }
    push_chunk(&mut out, b"IDAT", &zlib_store(&raw));
    push_chunk(&mut out, b"IEND", &[]);
    out
}

/// Appends one PNG chunk with its length, type, data, and CRC.
fn push_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut keyed = Vec::with_capacity(kind.len() + data.len());
    keyed.extend_from_slice(kind);
    keyed.extend_from_slice(data);
    out.extend_from_slice(&crc32(&keyed).to_be_bytes());
}

/// Wraps raw filtered rows in a zlib stream of stored deflate blocks.
///
/// Stored blocks skip compression, which keeps the encoder dependency-free;
/// each block carries at most 65535 bytes with its length complement.
fn zlib_store(raw: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    if raw.is_empty() {
        out.extend_from_slice(&[0x01, 0x00, 0x00, 0xFF, 0xFF]);
    } else {
        let blocks: Vec<&[u8]> = raw.chunks(65535).collect();
        for (index, block) in blocks.iter().enumerate() {
            out.push(u8::from(index + 1 == blocks.len()));
            let len = block.len() as u16;
            out.extend_from_slice(&len.to_le_bytes());
            out.extend_from_slice(&(!len).to_le_bytes());
            out.extend_from_slice(block);
        }
    }
    out.extend_from_slice(&adler32(raw).to_be_bytes());
    out
}

/// IEEE CRC32 over `data`, used for PNG chunk checksums.
fn crc32(data: &[u8]) -> u32 {
    static TABLE: [u32; 256] = crc_table();
    let mut crc = 0xFFFF_FFFFu32;
    for byte in data {
        let slot = (crc ^ u32::from(*byte)) & 0xFF;
        crc = TABLE[slot as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

/// Compile-time CRC table for the IEEE polynomial.
const fn crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut index = 0;
    while index < 256 {
        let mut crc = index as u32;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 1 == 1 {
                0xEDB8_8320 ^ (crc >> 1)
            } else {
                crc >> 1
            };
            bit += 1;
        }
        table[index] = crc;
        index += 1;
    }
    table
}

/// Adler32 over `data`, closing the zlib stream.
fn adler32(data: &[u8]) -> u32 {
    let mut low = 1u32;
    let mut high = 0u32;
    for byte in data {
        low = (low + u32::from(*byte)) % 65521;
        high = (high + low) % 65521;
    }
    (high << 16) | low
}

/// Fills a solid background RGB buffer.
pub fn solid_background(width: u32, height: u32, rgb: [u8; 3]) -> Vec<u8> {
    let mut pixels = Vec::with_capacity((width * height * 3) as usize);
    for _ in 0..width * height {
        pixels.extend_from_slice(&rgb);
    }
    pixels
}

/// Splits a packed `0xRRGGBB` tint into bytes.
fn tint_to_rgb(tint: u32) -> [u8; 3] {
    [
        ((tint >> 16) & 0xFF) as u8,
        ((tint >> 8) & 0xFF) as u8,
        (tint & 0xFF) as u8,
    ]
}

/// Renders screen-space plans into a flat top-down RGB buffer.
///
/// Edges paint first and nodes on top; curves reuse the shared flattening
/// samplers so the export matches the canvas. Squares keep the fast rectangle
/// path while other shapes and every arrow kind fill their shared polygons.
/// This is the software fallback behind both export scopes and needs no image
/// dependency.
pub fn rasterize(
    width: u32,
    height: u32,
    nodes: &[PaintedNode],
    edges: &[PaintedEdge],
    arrows: &[PaintedArrow],
    background: [u8; 3],
) -> Vec<u8> {
    let mut canvas = Image::new(width, height, background);
    for edge in edges {
        canvas.stroke_polyline(
            &edge_path(edge),
            tint_to_rgb(edge.tint),
            edge.width,
            edge.opacity,
        );
    }
    for arrow in arrows {
        canvas.fill_polygon(&arrow.points, tint_to_rgb(arrow.tint), 1.0);
    }
    for node in nodes {
        if node.shape == crate::shapes::NodeShape::Square {
            canvas.fill_rect(node.origin, node.side, tint_to_rgb(node.fill), node.opacity);
            if node.stroke_width > 0.0 {
                let origin = node.origin;
                let side = node.side;
                canvas.stroke_polygon(
                    &[
                        origin,
                        Point2::new(origin.x + side, origin.y),
                        Point2::new(origin.x + side, origin.y + side),
                        Point2::new(origin.x, origin.y + side),
                    ],
                    tint_to_rgb(node.stroke),
                    node.stroke_width,
                    node.opacity,
                );
            }
        } else {
            canvas.fill_polygon(&node.points, tint_to_rgb(node.fill), node.opacity);
            if node.stroke_width > 0.0 {
                canvas.stroke_polygon(
                    &node.points,
                    tint_to_rgb(node.stroke),
                    node.stroke_width,
                    node.opacity,
                );
            }
        }
    }
    canvas.pixels
}

/// Flattened screen path of one painted edge.
fn edge_path(edge: &PaintedEdge) -> Vec<Point2> {
    if let Some([ctrl_a, ctrl_b]) = edge.loop_ctrls {
        return sample_cubic_bezier(edge.start, ctrl_a, ctrl_b, edge.end, BEZIER_HIT_SAMPLES);
    }
    if edge.bend_a.is_some() || edge.bend_b.is_some() {
        return edge.polyline();
    }
    match edge.ctrl {
        None => vec![edge.start, edge.end],
        Some(mid) => sample_quadratic_bezier(edge.start, mid, edge.end, BEZIER_HIT_SAMPLES),
    }
}

struct Image {
    width: i32,
    height: i32,
    pixels: Vec<u8>,
}

impl Image {
    fn new(width: u32, height: u32, background: [u8; 3]) -> Self {
        let mut pixels = Vec::with_capacity((width * height * 3) as usize);
        for _ in 0..width * height {
            pixels.extend_from_slice(&background);
        }
        Self {
            width: width.max(1) as i32,
            height: height.max(1) as i32,
            pixels,
        }
    }

    fn plot(&mut self, x: i32, y: i32, rgb: [u8; 3]) {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return;
        }
        let at = ((y * self.width + x) * 3) as usize;
        if at + 2 < self.pixels.len() {
            self.pixels[at] = rgb[0];
            self.pixels[at + 1] = rgb[1];
            self.pixels[at + 2] = rgb[2];
        }
    }

    fn fill_rect(&mut self, origin: Point2, side: f32, rgb: [u8; 3], opacity: f32) {
        let alpha = opacity.clamp(0.0, 1.0);
        let x0 = origin.x.floor() as i32;
        let y0 = origin.y.floor() as i32;
        let x1 = (origin.x + side).ceil() as i32;
        let y1 = (origin.y + side).ceil() as i32;
        for y in y0..y1 {
            for x in x0..x1 {
                if alpha >= 1.0 {
                    self.plot(x, y, rgb);
                } else if alpha > 0.0 {
                    self.blend(x, y, rgb, alpha);
                }
            }
        }
    }

    fn blend(&mut self, x: i32, y: i32, rgb: [u8; 3], alpha: f32) {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return;
        }
        let at = ((y * self.width + x) * 3) as usize;
        if at + 2 < self.pixels.len() {
            for (channel, cell) in self.pixels[at..at + 3].iter_mut().enumerate() {
                let back = *cell as f32;
                let front = rgb[channel] as f32;
                *cell = (front * alpha + back * (1.0 - alpha)).round() as u8;
            }
        }
    }

    fn stroke_polyline(&mut self, line: &[Point2], rgb: [u8; 3], width: f32, alpha: f32) {
        for pair in line.windows(2) {
            self.stroke_segment(pair[0], pair[1], rgb, width, alpha);
        }
    }

    fn stroke_polygon(&mut self, vertices: &[Point2], rgb: [u8; 3], width: f32, alpha: f32) {
        if vertices.len() < 2 {
            return;
        }
        for (index, point) in vertices.iter().enumerate() {
            let next = vertices[(index + 1) % vertices.len()];
            self.stroke_segment(*point, next, rgb, width, alpha);
        }
    }

    fn stroke_segment(&mut self, a: Point2, b: Point2, rgb: [u8; 3], width: f32, alpha: f32) {
        let clamped = alpha.clamp(0.0, 1.0);
        if clamped <= 0.0 {
            return;
        }
        let radius = (width.max(1.0).round() as i32 - 1).max(0) / 2;
        let steps = ((b.x - a.x).abs().max((b.y - a.y).abs()).ceil() as i32).max(1);
        for step in 0..=steps {
            let t = step as f32 / steps as f32;
            let x = (a.x + (b.x - a.x) * t).round() as i32;
            let y = (a.y + (b.y - a.y) * t).round() as i32;
            for dy in -radius..=radius {
                for dx in -radius..=radius {
                    if clamped >= 1.0 {
                        self.plot(x + dx, y + dy, rgb);
                    } else {
                        self.blend(x + dx, y + dy, rgb, clamped);
                    }
                }
            }
        }
    }

    fn fill_polygon(&mut self, vertices: &[Point2], rgb: [u8; 3], opacity: f32) {
        if vertices.len() < 3 {
            return;
        }
        let mut x0 = f32::INFINITY;
        let mut y0 = f32::INFINITY;
        let mut x1 = f32::NEG_INFINITY;
        let mut y1 = f32::NEG_INFINITY;
        for point in vertices {
            x0 = x0.min(point.x);
            y0 = y0.min(point.y);
            x1 = x1.max(point.x);
            y1 = y1.max(point.y);
        }
        let alpha = opacity.clamp(0.0, 1.0);
        if alpha <= 0.0 {
            return;
        }
        for y in (y0.floor() as i32)..=(y1.ceil() as i32) {
            for x in (x0.floor() as i32)..=(x1.ceil() as i32) {
                let point = Point2::new(x as f32 + 0.5, y as f32 + 0.5);
                if point_in_polygon(point, vertices) {
                    if alpha >= 1.0 {
                        self.plot(x, y, rgb);
                    } else {
                        self.blend(x, y, rgb, alpha);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cg_graph::NodeIndex;

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

    #[test]
    fn translucent_edges_blend_with_the_background() {
        use crate::arrows::ArrowKind;
        use crate::view::{PaintedArrow, PaintedEdge};

        let edges = vec![PaintedEdge {
            source: NodeIndex::new(0),
            target: NodeIndex::new(1),
            start: Point2::new(0.0, 0.0),
            end: Point2::new(1.0, 0.0),
            ctrl: None,
            loop_ctrls: None,
            bend_a: None,
            bend_b: None,
            aggregated: false,
            tint: 0xFFFFFF,
            width: 1.0,
            opacity: 0.5,
            arrow: ArrowKind::Triangle,
            arrow_scale: 1.0,
        }];
        let arrows: Vec<PaintedArrow> = Vec::new();
        let pixels = rasterize(2, 1, &[], &edges, &arrows, [0, 0, 0]);
        assert_eq!(&pixels[0..3], &[128, 128, 128]);
    }

    #[test]
    fn png_round_trips_pixels_through_stored_blocks() {
        let pixels = vec![255, 0, 0, 0, 255, 0];
        let encoded = encode_png(2, 1, &pixels);
        assert_eq!(&encoded[0..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
        let chunks = split_chunks(&encoded);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].0, *b"IHDR");
        assert_eq!(&chunks[0].1[0..8], &[0, 0, 0, 2, 0, 0, 0, 1]);
        assert_eq!(chunks[1].0, *b"IDAT");
        assert_eq!(chunks[2].0, *b"IEND");
        assert!(chunks[2].1.is_empty());
        let raw = inflate_store(&chunks[1].1);
        assert_eq!(raw, vec![0, 255, 0, 0, 0, 255, 0]);
    }

    #[test]
    fn png_header_matches_dimensions() {
        let encoded = encode_png(3, 2, &[9; 18]);
        let chunks = split_chunks(&encoded);
        assert_eq!(&chunks[0].1[0..8], &[0, 0, 0, 3, 0, 0, 0, 2]);
        let raw = inflate_store(&chunks[1].1);
        assert_eq!(raw.len(), 2 * (3 * 3 + 1));
    }

    /// Splits an encoded image into (kind, data) pairs, checking CRCs.
    fn split_chunks(encoded: &[u8]) -> Vec<([u8; 4], Vec<u8>)> {
        let mut chunks = Vec::new();
        let mut cursor = 8;
        while cursor + 8 <= encoded.len() {
            let len = u32::from_be_bytes(encoded[cursor..cursor + 4].try_into().unwrap_or([0; 4]))
                as usize;
            let kind: [u8; 4] = encoded[cursor + 4..cursor + 8].try_into().unwrap_or([0; 4]);
            let data = encoded[cursor + 8..cursor + 8 + len].to_vec();
            let stored = u32::from_be_bytes(
                encoded[cursor + 8 + len..cursor + 12 + len]
                    .try_into()
                    .unwrap_or([0; 4]),
            );
            let mut keyed = Vec::new();
            keyed.extend_from_slice(&kind);
            keyed.extend_from_slice(&data);
            assert_eq!(stored, crc32(&keyed));
            chunks.push((kind, data));
            cursor += 12 + len;
            if kind == *b"IEND" {
                break;
            }
        }
        chunks
    }

    /// Decodes the stored-block zlib stream the encoder writes.
    fn inflate_store(stream: &[u8]) -> Vec<u8> {
        assert_eq!(&stream[0..2], &[0x78, 0x01]);
        let mut raw = Vec::new();
        let mut cursor = 2;
        loop {
            let final_block = stream[cursor] & 0x01 == 0x01;
            assert_eq!(stream[cursor] >> 1, 0);
            let len = u16::from_le_bytes([stream[cursor + 1], stream[cursor + 2]]) as usize;
            let complement = u16::from_le_bytes([stream[cursor + 3], stream[cursor + 4]]);
            assert_eq!(complement, !len as u16);
            raw.extend_from_slice(&stream[cursor + 5..cursor + 5 + len]);
            cursor += 5 + len;
            if final_block {
                break;
            }
        }
        let stored = u32::from_be_bytes(stream[cursor..cursor + 4].try_into().unwrap_or([0; 4]));
        assert_eq!(stored, adler32(&raw));
        raw
    }

    #[test]
    fn raster_places_nodes_over_edges() {
        use crate::shapes::NodeShape;
        use crate::view::{PaintedArrow, PaintedEdge, PaintedNode};
        use cg_graph::NodeIndex;

        let nodes = vec![PaintedNode {
            id: NodeIndex::new(0),
            origin: Point2::new(1.0, 1.0),
            side: 4.0,
            fill: 0xFF0000,
            stroke: 0x000000,
            stroke_width: 0.0,
            opacity: 1.0,
            shape: NodeShape::Square,
            points: vec![],
        }];
        let edges = vec![PaintedEdge {
            source: NodeIndex::new(0),
            target: NodeIndex::new(1),
            start: Point2::new(0.0, 0.0),
            end: Point2::new(9.0, 9.0),
            ctrl: None,
            loop_ctrls: None,
            bend_a: None,
            bend_b: None,
            aggregated: false,
            tint: 0x00FF00,
            width: 1.0,
            opacity: 1.0,
            arrow: crate::arrows::ArrowKind::Triangle,
            arrow_scale: 1.0,
        }];
        let arrows: Vec<PaintedArrow> = Vec::new();
        let pixels = rasterize(10, 10, &nodes, &edges, &arrows, [0, 0, 0]);
        assert_eq!(pixels.len(), 10 * 10 * 3);
        let node_at = (2 * 10 + 2) * 3;
        assert_eq!(&pixels[node_at..node_at + 3], &[255, 0, 0]);
        let edge_at = (9 * 10 + 9) * 3;
        assert_eq!(&pixels[edge_at..edge_at + 3], &[0, 255, 0]);
    }
}
