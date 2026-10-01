//! Immediate-mode canvas element that draws the graph.
//!
//! The element owns no layout or style state: callers pass fully resolved
//! screen pixel plans, and the closure paints edges under arrows under nodes
//! under labels. Each edge carries its own tint and width, so strokes are
//! built per edge instead of sharing one path.

use std::collections::HashMap;

use cg_types::Point2;
use gpui::{
    App, Background, Bounds, Font, Hsla, IntoElement, Pixels, Rgba, SharedString, TextAlign,
    TextRun, Window, canvas, fill, linear_color_stop, linear_gradient, rgb,
};

use crate::shapes::NodeShape;
use crate::style::NodeFill;
use crate::text::{LabelBackground, PaintedEdgeLabel, PaintedLabel};

use super::plans::{
    PaintedArrow, PaintedContainer, PaintedEdge, PaintedNode, PaintedRubberBand, RUBBER_BAND_STROKE,
};

/// Fill of the rubber-band box-selection rectangle.
const RUBBER_BAND_FILL: u32 = 0x4a9eff22;

/// Fill of a compound container background.
const COMPOUND_FILL: u32 = 0xe8eef7;

/// Border of a compound container.
const COMPOUND_BORDER: u32 = 0x8a93a6;

/// Title text of a compound container.
const COMPOUND_TITLE: u32 = 0x39424e;

/// Canvas element painting edges under arrows under nodes under labels.
///
/// Each edge carries its own tint and width, so strokes are built per edge
/// instead of sharing one path. Labels are shaped through the window text
/// system and centered on their anchor; edge labels paint above node labels
/// so weight text stays readable on dense bundles. The closure receives owned
/// plans so the element stays `'static`.
pub fn graph_view(
    containers: Vec<PaintedContainer>,
    nodes: Vec<PaintedNode>,
    edges: Vec<PaintedEdge>,
    arrows: Vec<PaintedArrow>,
    labels: Vec<PaintedLabel>,
    edge_labels: Vec<PaintedEdgeLabel>,
    rubber_band: Option<PaintedRubberBand>,
) -> impl IntoElement {
    canvas(
        |_bounds: Bounds<Pixels>, _window: &mut Window, _cx: &mut App| {},
        move |_bounds: Bounds<Pixels>, (), window: &mut Window, cx: &mut App| {
            for container in &containers {
                let quad = fill(
                    Bounds {
                        origin: gpui::point(
                            gpui::px(container.rect.origin.x),
                            gpui::px(container.rect.origin.y),
                        ),
                        size: gpui::size(
                            gpui::px(container.rect.size.x),
                            gpui::px(container.rect.size.y),
                        ),
                    },
                    rgb(COMPOUND_FILL),
                );
                window.paint_quad(quad);
                let mut outline = gpui::PathBuilder::stroke(gpui::px(1.5));
                let far = Point2::new(
                    container.rect.origin.x + container.rect.size.x,
                    container.rect.origin.y + container.rect.size.y,
                );
                outline.move_to(to_pixels(container.rect.origin));
                outline.line_to(to_pixels(Point2::new(far.x, container.rect.origin.y)));
                outline.line_to(to_pixels(far));
                outline.line_to(to_pixels(Point2::new(container.rect.origin.x, far.y)));
                outline.line_to(to_pixels(container.rect.origin));
                if let Ok(path) = outline.build() {
                    window.paint_path(path, rgb(COMPOUND_BORDER));
                }
                if !container.title.is_empty() {
                    let at = Point2::new(
                        container.rect.origin.x + container.rect.size.x / 2.0,
                        container.rect.origin.y + 4.0,
                    );
                    paint_text_at(&container.title, 12.0, COMPOUND_TITLE, at, window, cx);
                }
            }
            for edge in &edges {
                let mut strokes = gpui::PathBuilder::stroke(gpui::px(edge.width.max(0.5)));
                strokes.move_to(to_pixels(edge.start));
                if let Some([ctrl_a, ctrl_b]) = edge.loop_ctrls {
                    strokes.cubic_bezier_to(
                        to_pixels(edge.end),
                        to_pixels(ctrl_a),
                        to_pixels(ctrl_b),
                    );
                } else if let Some(ctrl) = edge.ctrl {
                    strokes.curve_to(to_pixels(edge.end), to_pixels(ctrl));
                } else {
                    for bend in edge.bends() {
                        strokes.line_to(to_pixels(bend));
                    }
                    strokes.line_to(to_pixels(edge.end));
                }
                if let Ok(path) = strokes.build() {
                    window.paint_path(path, with_opacity(edge.tint, edge.opacity));
                }
            }
            if !arrows.is_empty() {
                let mut by_tint: HashMap<u32, Vec<Vec<Point2>>> = HashMap::new();
                for arrow in &arrows {
                    by_tint
                        .entry(arrow.tint)
                        .or_default()
                        .push(arrow.points.clone());
                }
                let mut tints: Vec<u32> = by_tint.keys().copied().collect();
                tints.sort_unstable();
                for tint in tints {
                    let mut heads = gpui::PathBuilder::fill();
                    for corners in by_tint.get(&tint).unwrap_or(&Vec::new()) {
                        let pixels: Vec<gpui::Point<Pixels>> =
                            corners.iter().map(|point| to_pixels(*point)).collect();
                        heads.add_polygon(&pixels, true);
                    }
                    if let Ok(path) = heads.build() {
                        window.paint_path(path, rgb(tint));
                    }
                }
            }
            for node in &nodes {
                if node.shape == NodeShape::Square {
                    let quad = fill(
                        Bounds {
                            origin: gpui::point(gpui::px(node.origin.x), gpui::px(node.origin.y)),
                            size: gpui::size(gpui::px(node.side), gpui::px(node.side)),
                        },
                        node_background(&node.fill, node.opacity),
                    );
                    window.paint_quad(quad);
                    paint_node_stroke(&node_stroke_loop(node), node, window);
                    continue;
                }
                let mut body = gpui::PathBuilder::fill();
                let pixels: Vec<gpui::Point<Pixels>> =
                    node.points.iter().map(|point| to_pixels(*point)).collect();
                body.add_polygon(&pixels, true);
                if let Ok(path) = body.build() {
                    window.paint_path(path, node_background(&node.fill, node.opacity));
                }
                paint_node_stroke(&node.points, node, window);
            }
            for label in &labels {
                paint_label(label, window, cx);
            }
            for label in &edge_labels {
                paint_edge_label(label, window, cx);
            }
            if let Some(band) = rubber_band {
                let quad = fill(
                    Bounds {
                        origin: gpui::point(gpui::px(band.origin.x), gpui::px(band.origin.y)),
                        size: gpui::size(gpui::px(band.size.x), gpui::px(band.size.y)),
                    },
                    rgba(RUBBER_BAND_FILL),
                );
                window.paint_quad(quad);
                let mut outline = gpui::PathBuilder::stroke(gpui::px(1.0));
                let far = Point2::new(band.origin.x + band.size.x, band.origin.y + band.size.y);
                outline.move_to(to_pixels(band.origin));
                outline.line_to(to_pixels(Point2::new(far.x, band.origin.y)));
                outline.line_to(to_pixels(far));
                outline.line_to(to_pixels(Point2::new(band.origin.x, far.y)));
                outline.line_to(to_pixels(band.origin));
                if let Ok(path) = outline.build() {
                    window.paint_path(path, rgb(RUBBER_BAND_STROKE));
                }
            }
        },
    )
}

fn with_opacity(tint: u32, opacity: f32) -> Rgba {
    let mut color = rgb(tint);
    color.a = opacity.clamp(0.0, 1.0);
    color
}

/// Canvas background for a node fill.
///
/// Solid fills map to a single translucent color; gradients map to the
/// framework linear gradient with the stored angle passed through. Opacity
/// applies to both stops so translucent gradients fade uniformly.
fn node_background(fill: &NodeFill, opacity: f32) -> Background {
    let from = with_opacity(fill.start, opacity);
    if fill.is_solid() {
        return Background::from(from);
    }
    let to = with_opacity(fill.end, opacity);
    linear_gradient(
        fill.angle(),
        linear_color_stop(from, 0.0),
        linear_color_stop(to, 1.0),
    )
}

/// Corners of a square node body in paint order for stroking.
///
/// The fill keeps the fast quad path; only the border goes through the stroke
/// path builder, sharing the resolved stroke with the export raster.
fn node_stroke_loop(node: &PaintedNode) -> [Point2; 4] {
    [
        node.origin,
        Point2::new(node.origin.x + node.side, node.origin.y),
        Point2::new(node.origin.x + node.side, node.origin.y + node.side),
        Point2::new(node.origin.x, node.origin.y + node.side),
    ]
}

/// Strokes the closed `outline` of one node body.
///
/// Zero or negative widths skip painting so borderless styles cost nothing.
/// The stroke shares the node opacity with the fill, keeping translucent
/// nodes uniformly faded.
fn paint_node_stroke(outline: &[Point2], node: &PaintedNode, window: &mut Window) {
    if node.stroke_width <= 0.0 || outline.len() < 3 {
        return;
    }
    let mut border = gpui::PathBuilder::stroke(gpui::px(node.stroke_width.max(0.5)));
    border.move_to(to_pixels(outline[0]));
    for corner in &outline[1..] {
        border.line_to(to_pixels(*corner));
    }
    border.line_to(to_pixels(outline[0]));
    if let Ok(path) = border.build() {
        window.paint_path(path, with_opacity(node.stroke, node.opacity));
    }
}

/// Shapes and paints one node label centered on its anchor.
///
/// Shaping and painting both report errors rather than panicking; a failed
/// label is skipped so one bad glyph never blanks the frame. The line height
/// tracks the font size, and the label is centered over the node width so the
/// text stays under its body regardless of length. The plan keeps zero
/// rotation and the canvas paints horizontally, so bounds and selection use
/// the same envelope as the visible text.
fn paint_label(label: &PaintedLabel, window: &mut Window, cx: &mut App) {
    if label.background != LabelBackground::None {
        paint_label_background(&label.text, label.size, label.origin, window, cx);
    }
    paint_text_at(
        &label.text,
        label.size,
        label.color,
        label.origin,
        window,
        cx,
    );
}

/// Shapes and paints one edge label centered on its anchor.
///
/// Edge labels reuse the node label shaping path so weight text and node text
/// share one rendering behavior; only the plan source differs. The plan keeps
/// zero rotation to match the horizontal paint.
fn paint_edge_label(label: &PaintedEdgeLabel, window: &mut Window, cx: &mut App) {
    if label.background != LabelBackground::None {
        paint_label_background(&label.text, label.size, label.origin, window, cx);
    }
    paint_text_at(
        &label.text,
        label.size,
        label.color,
        label.origin,
        window,
        cx,
    );
}

/// Paints the light plate behind a label.
///
/// Widths come from shaped lines so the plate hugs the real glyphs; the total
/// height stacks every wrapped line. Rounded backgrounds currently fall back
/// to a rectangle until a rounded primitive lands.
fn paint_label_background(
    text: &str,
    size: f32,
    origin: Point2,
    window: &mut Window,
    _cx: &mut App,
) {
    use crate::text::{
        LABEL_BACKGROUND_FILL, LABEL_BACKGROUND_PAD, line_height, split_label_lines,
    };
    let size_px = gpui::px(size.max(1.0));
    let step = line_height(size);
    let lines = split_label_lines(text);
    let mut max_width = 0.0f32;
    for line in &lines {
        if line.is_empty() {
            continue;
        }
        let run = TextRun {
            len: line.len(),
            font: Font::default(),
            color: rgb(LABEL_BACKGROUND_FILL).into(),
            ..TextRun::default()
        };
        let shaped = window.text_system().shape_line(
            SharedString::from(line.clone()),
            size_px,
            &[run],
            None,
        );
        max_width = max_width.max(f32::from(shaped.width()));
    }
    if max_width <= 0.0 {
        max_width = size.max(1.0) * 0.6;
    }
    let total = lines.len().max(1) as f32 * step;
    let pad = LABEL_BACKGROUND_PAD;
    let quad = fill(
        Bounds {
            origin: gpui::point(
                gpui::px(origin.x - max_width / 2.0 - pad),
                gpui::px(origin.y - pad),
            ),
            size: gpui::size(gpui::px(max_width + pad * 2.0), gpui::px(total + pad * 2.0)),
        },
        rgb(LABEL_BACKGROUND_FILL),
    );
    window.paint_quad(quad);
}

/// Shapes label text centered on `origin` and paints it line by line.
///
/// Text is split on newlines with overlong lines hard-wrapped, and each line
/// is shaped independently with its byte length as the run length, so
/// multi-byte glyphs never slice inside a code point. Lines stack downward by
/// the line height. Failures are skipped silently so one bad label never
/// blanks the frame.
fn paint_text_at(
    text: &str,
    size: f32,
    color: u32,
    origin: Point2,
    window: &mut Window,
    cx: &mut App,
) {
    use crate::text::{line_height, split_label_lines};
    let size_px = gpui::px(size.max(1.0));
    let tint: Hsla = rgb(color).into();
    let step = line_height(size);
    for (row, line) in split_label_lines(text).iter().enumerate() {
        if line.is_empty() {
            continue;
        }
        let run = TextRun {
            len: line.len(),
            font: Font::default(),
            color: tint,
            ..TextRun::default()
        };
        let shaped = window.text_system().shape_line(
            SharedString::from(line.clone()),
            size_px,
            &[run],
            None,
        );
        let width = shaped.width();
        let at = gpui::point(
            gpui::px(origin.x - f32::from(width) / 2.0),
            gpui::px(origin.y + row as f32 * step),
        );
        let _ = shaped.paint(at, size_px, TextAlign::Left, None, window, cx);
    }
}

fn to_pixels(point: Point2) -> gpui::Point<Pixels> {
    gpui::point(gpui::px(point.x), gpui::px(point.y))
}

fn rgba(hex: u32) -> Rgba {
    gpui::rgba(hex)
}
