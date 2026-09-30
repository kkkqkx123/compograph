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
