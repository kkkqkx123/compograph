//! Software rasterizer turning paint plans into flat RGB buffers.
//!
//! Edges paint first and nodes on top; curves reuse the shared flattening
//! samplers so the export matches the canvas. This is the software fallback
//! behind both export scopes and needs no image dependency.

use cg_geometry::{
    BEZIER_HIT_SAMPLES, point_in_polygon, sample_cubic_bezier, sample_quadratic_bezier,
};
use cg_types::Point2;

use super::view::{PaintedArrow, PaintedEdge, PaintedNode};
use crate::glyph::{
    GLYPH_ADVANCE, GLYPH_HEIGHT, GLYPH_LINE_ADVANCE, GLYPH_WIDTH, glyph_rows, is_printable_ascii,
};
use crate::style::NodeFill;
use crate::text::{LabelAlign, LabelBackground, PaintedEdgeLabel, PaintedLabel, split_label_lines};

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
    rasterize_with_labels(
        width,
        height,
        nodes,
        edges,
        arrows,
        background,
        LabelOverlays {
            nodes: &[],
            edges: &[],
        },
    )
}

/// Label plates composited above the geometry in one export pass.
///
/// Node and edge labels travel together because both paint after the
/// geometry through the same glyph pass; the plain [`rasterize`] entry
/// passes empty slices.
pub struct LabelOverlays<'a> {
    pub nodes: &'a [PaintedLabel],
    pub edges: &'a [PaintedEdgeLabel],
}

/// Renders plans with label plates composited above the geometry.
///
/// Labels reuse the planned origins and wrap with the same line splitter as
/// the canvas, so export text lands where the canvas draws it. Glyphs come
/// from the built-in bitmap font; characters outside printable ASCII render
/// as solid fallback blocks.
pub fn rasterize_with_labels(
    width: u32,
    height: u32,
    nodes: &[PaintedNode],
    edges: &[PaintedEdge],
    arrows: &[PaintedArrow],
    background: [u8; 3],
    labels: LabelOverlays<'_>,
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
            if node.fill.is_solid() {
                canvas.fill_rect(
                    node.origin,
                    node.side,
                    tint_to_rgb(node.fill.start),
                    node.opacity,
                );
            } else {
                canvas.fill_rect_gradient(node.origin, node.side, &node.fill, node.opacity);
            }
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
        } else if node.fill.is_solid() {
            canvas.fill_polygon(&node.points, tint_to_rgb(node.fill.start), node.opacity);
            if node.stroke_width > 0.0 {
                canvas.stroke_polygon(
                    &node.points,
                    tint_to_rgb(node.stroke),
                    node.stroke_width,
                    node.opacity,
                );
            }
        } else {
            canvas.fill_polygon_gradient(
                &node.points,
                node.origin,
                node.side,
                &node.fill,
                node.opacity,
            );
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
    for label in labels.nodes {
        canvas.fill_label(LabelFace {
            origin: label.origin,
            text: &label.text,
            size: label.size,
            rgb: tint_to_rgb(label.color),
            align: label.align,
            background: label.background,
            background_rgb: tint_to_rgb(label.background_color),
            padding: label.padding,
        });
    }
    for label in labels.edges {
        canvas.fill_label(LabelFace {
            origin: label.origin,
            text: &label.text,
            size: label.size,
            rgb: tint_to_rgb(label.color),
            align: label.align,
            background: label.background,
            background_rgb: tint_to_rgb(label.background_color),
            padding: label.padding,
        });
    }
    canvas.pixels
}

/// Flattened screen path of one painted edge.
fn edge_path(edge: &PaintedEdge) -> Vec<Point2> {
    if let Some([ctrl_a, ctrl_b]) = edge.loop_ctrls {
        return sample_cubic_bezier(edge.start, ctrl_a, ctrl_b, edge.end, BEZIER_HIT_SAMPLES);
    }
    if !edge.bends.is_empty() {
        return edge.polyline();
    }
    match edge.ctrl {
        None => vec![edge.start, edge.end],
        Some(mid) => sample_quadratic_bezier(edge.start, mid, edge.end, BEZIER_HIT_SAMPLES),
    }
}

/// Gradient direction for an angle in degrees.
///
/// Zero runs top to bottom with values growing clockwise, matching the canvas
/// gradient convention.
fn gradient_direction(angle_deg: f32) -> (f32, f32) {
    let angle = if angle_deg.is_finite() {
        angle_deg.clamp(0.0, 360.0).to_radians()
    } else {
        0.0
    };
    (angle.sin(), angle.cos())
}

/// Normalized position of a pixel inside the node bounds along the gradient.
fn gradient_ratio(x: f32, y: f32, origin: Point2, side: f32, angle_deg: f32) -> f32 {
    let (dx, dy) = gradient_direction(angle_deg);
    let corners = [
        origin,
        Point2::new(origin.x + side, origin.y),
        Point2::new(origin.x, origin.y + side),
        Point2::new(origin.x + side, origin.y + side),
    ];
    let mut min = f32::INFINITY;
    let mut max = f32::NEG_INFINITY;
    for corner in corners {
        let projection = corner.x * dx + corner.y * dy;
        min = min.min(projection);
        max = max.max(projection);
    }
    if max <= min {
        return 0.0;
    }
    ((x * dx + y * dy - min) / (max - min)).clamp(0.0, 1.0)
}

struct Image {
    width: i32,
    height: i32,
    pixels: Vec<u8>,
}

/// One label plate with resolved colors, ready for the glyph pass.
///
/// Node and edge labels carry the same visual fields under different id
/// types, so both convert into this face and share one paint path.
/// Opacity stays fully opaque: export plates never fade, matching the
/// canvas label paint the plans were built from.
struct LabelFace<'a> {
    origin: Point2,
    text: &'a str,
    size: f32,
    rgb: [u8; 3],
    align: LabelAlign,
    background: LabelBackground,
    background_rgb: [u8; 3],
    padding: f32,
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
        self.fill_block(x0, y0, x1, y1, rgb, alpha);
    }

    fn fill_block(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, rgb: [u8; 3], alpha: f32) {
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

    /// Composites one label plate with bitmap text above the geometry.
    ///
    /// Lines wrap with the same splitter the canvas plans with, so export
    /// text lands where the canvas draws it. Characters outside printable
    /// ASCII render as solid fallback blocks. Rounded backgrounds paint
    /// square; corner carving stays a future refinement.
    fn fill_label(&mut self, face: LabelFace<'_>) {
        const ALPHA: f32 = 1.0;

        let LabelFace {
            origin,
            text,
            size,
            rgb,
            align,
            background,
            background_rgb,
            padding,
        } = face;
        if text.trim().is_empty() {
            return;
        }
        let pixel = if size.is_finite() && size > 0.0 {
            (size / 12.0).round().clamp(1.0, 8.0) as i32
        } else {
            1
        };
        let lines = split_label_lines(text);
        let max_chars = lines
            .iter()
            .map(|line| line.chars().count())
            .max()
            .unwrap_or(0) as i32;
        if max_chars == 0 {
            return;
        }
        let width = max_chars * GLYPH_ADVANCE as i32 * pixel;
        let height = lines.len() as i32 * GLYPH_LINE_ADVANCE as i32 * pixel;
        let left = match align {
            LabelAlign::Center => (origin.x - width as f32 / 2.0).round() as i32,
            LabelAlign::Left => origin.x.round() as i32,
            LabelAlign::Right => (origin.x - width as f32).round() as i32,
        };
        let top = origin.y.round() as i32;
        if !matches!(background, LabelBackground::None) {
            let pad = if padding.is_finite() && padding > 0.0 {
                padding.round() as i32
            } else {
                0
            };
            self.fill_block(
                left - pad,
                top - pad,
                left + width + pad,
                top + height + pad,
                background_rgb,
                ALPHA,
            );
        }
        for (row, line) in lines.iter().enumerate() {
            let baseline = top + row as i32 * GLYPH_LINE_ADVANCE as i32 * pixel;
            let mut cursor = left;
            for point in line.chars() {
                let mut bytes = [0u8; 4];
                let encoded = point.encode_utf8(&mut bytes);
                if encoded.len() == 1 && is_printable_ascii(encoded.as_bytes()[0]) {
                    let rows = glyph_rows(encoded.as_bytes()[0]);
                    for (glyph_row, bits) in rows.iter().enumerate() {
                        for column in 0..GLYPH_WIDTH {
                            if bits & (1 << (GLYPH_WIDTH - 1 - column)) != 0 {
                                self.fill_block(
                                    cursor + column as i32 * pixel,
                                    baseline + glyph_row as i32 * pixel,
                                    cursor + (column as i32 + 1) * pixel,
                                    baseline + (glyph_row as i32 + 1) * pixel,
                                    rgb,
                                    ALPHA,
                                );
                            }
                        }
                    }
                } else if !point.is_whitespace() {
                    self.fill_block(
                        cursor,
                        baseline,
                        cursor + GLYPH_WIDTH as i32 * pixel,
                        baseline + GLYPH_HEIGHT as i32 * pixel,
                        rgb,
                        ALPHA,
                    );
                }
                cursor += GLYPH_ADVANCE as i32 * pixel;
            }
        }
    }

    fn fill_rect_gradient(&mut self, origin: Point2, side: f32, fill: &NodeFill, opacity: f32) {
        let alpha = opacity.clamp(0.0, 1.0);
        if alpha <= 0.0 {
            return;
        }
        let x0 = origin.x.floor() as i32;
        let y0 = origin.y.floor() as i32;
        let x1 = (origin.x + side).ceil() as i32;
        let y1 = (origin.y + side).ceil() as i32;
        for y in y0..y1 {
            for x in x0..x1 {
                let ratio =
                    gradient_ratio(x as f32 + 0.5, y as f32 + 0.5, origin, side, fill.angle_deg);
                let rgb = tint_to_rgb(fill.sample(ratio));
                if alpha >= 1.0 {
                    self.plot(x, y, rgb);
                } else {
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

    fn fill_polygon_gradient(
        &mut self,
        vertices: &[Point2],
        origin: Point2,
        side: f32,
        fill: &NodeFill,
        opacity: f32,
    ) {
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
                    let ratio = gradient_ratio(point.x, point.y, origin, side, fill.angle_deg);
                    let rgb = tint_to_rgb(fill.sample(ratio));
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
            bends: Vec::new(),
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
    fn raster_places_nodes_over_edges() {
        use crate::shapes::NodeShape;
        use crate::view::{PaintedArrow, PaintedEdge, PaintedNode};
        use cg_graph::NodeIndex;

        let nodes = vec![PaintedNode {
            id: NodeIndex::new(0),
            origin: Point2::new(1.0, 1.0),
            side: 4.0,
            fill: crate::style::NodeFill::solid(0xFF0000),
            stroke: 0x000000,
            stroke_width: 0.0,
            opacity: 1.0,
            shape: NodeShape::Square,
            points: vec![],
            image: None,
        }];
        let edges = vec![PaintedEdge {
            source: NodeIndex::new(0),
            target: NodeIndex::new(1),
            start: Point2::new(0.0, 0.0),
            end: Point2::new(9.0, 9.0),
            ctrl: None,
            loop_ctrls: None,
            bends: Vec::new(),
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

    #[test]
    fn gradient_rect_runs_from_start_to_end() {
        use crate::shapes::NodeShape;
        use crate::view::PaintedNode;

        let nodes = vec![PaintedNode {
            id: NodeIndex::new(0),
            origin: Point2::new(0.0, 0.0),
            side: 10.0,
            fill: NodeFill::gradient(0x000000, 0xFFFFFF, 90.0),
            stroke: 0x000000,
            stroke_width: 0.0,
            opacity: 1.0,
            shape: NodeShape::Square,
            points: vec![],
            image: None,
        }];
        let pixels = rasterize(10, 10, &nodes, &[], &[], [0, 0, 0]);
        let left = (5 * 10) * 3;
        let right = (5 * 10 + 9) * 3;
        assert!(pixels[left] < 64);
        assert!(pixels[right] > 192);
        let middle = (5 * 10 + 5) * 3;
        assert!((pixels[middle] as i16 - 128).abs() < 24);
    }
}
