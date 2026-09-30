//! Node paint plans with viewport culling.
//!
//! This entry iterates caller-supplied candidates and culls off-screen ones,
//! so the spatial index narrows the set while the exact shape test rejects
//! off-screen bodies. Minimal detail degrades every shape to a square while
//! keeping positions and fills.

use cg_graph::{GraphView, NodeIndex, Positions};
use cg_types::{Point2, Rect, Vec2};

use crate::camera::Camera;
use crate::lod::DetailLevel;
use crate::shapes::{NodeShape, node_polygon, shape_for_level, shape_hits_rect};
use crate::spatial::SpatialIndex;
use crate::style::NodeStyle;

use super::plans::{NODE_SIDE, PaintedNode};

/// Nodes whose bodies touch `rect`, in index order.
///
/// The spatial index narrows candidates and each body is tested with its
/// effective shape, so plan generation iterates only visible nodes on large
/// graphs. The query grows by the body extent because the index keys on
/// centers; the exact shape test still rejects off-screen bodies, so growth
/// only adds candidates. Callers pass the painted shape: squares under the
/// minimal detail level, true shapes otherwise. Callers keep the index
/// versioned: viewport motion reuses it, position write-backs rebuild it.
pub fn visible_node_ids(
    positions: &Positions,
    index: &SpatialIndex,
    rect: Rect,
    half_extent: f32,
    shape_of: impl Fn(NodeIndex) -> NodeShape,
) -> Vec<NodeIndex> {
    let grown = Rect::new(
        Point2::new(rect.origin.x - half_extent, rect.origin.y - half_extent),
        Vec2::new(
            rect.size.x + half_extent * 2.0,
            rect.size.y + half_extent * 2.0,
        ),
    );
    let mut found: Vec<NodeIndex> = index
        .query_rect(grown)
        .into_iter()
        .filter(|node| {
            positions
                .get(node)
                .map(|center| shape_hits_rect(shape_of(*node), *center, half_extent, rect))
                .unwrap_or(false)
        })
        .collect();
    found.sort_unstable_by_key(|node| node.index());
    found
}

/// Transforms graph structure and layout output into a node paint plan.
///
/// This entry iterates every node and culls off-screen ones one by one, which
/// keeps it suitable as the scale-benchmark baseline. The product path prefers
/// [`paint_nodes_for`] over a caller-narrowed visible set. Fill colors arrive
/// resolved through `node_style`, so selection and highlights reach the canvas
/// without touching the stylesheet.
pub fn paint_nodes(
    graph: &dyn GraphView,
    positions: &Positions,
    camera: &Camera,
    viewport: Vec2,
    node_style: impl Fn(NodeIndex) -> NodeStyle,
) -> Vec<PaintedNode> {
    let mut ids = graph.node_ids();
    ids.sort_unstable_by_key(|node| node.index());
    paint_nodes_for(&ids, positions, camera, viewport, node_style)
}

/// Node plan for an explicit candidate list, such as the visible set.
///
/// Candidates outside the viewport are still skipped, so index over-queries
/// stay harmless.
pub fn paint_nodes_for(
    nodes: &[NodeIndex],
    positions: &Positions,
    camera: &Camera,
    viewport: Vec2,
    node_style: impl Fn(NodeIndex) -> NodeStyle,
) -> Vec<PaintedNode> {
    paint_nodes_for_level(
        nodes,
        positions,
        camera,
        viewport,
        DetailLevel::Full,
        node_style,
    )
}

/// Node plan honoring the detail level; minimal degrades every shape to a
/// square while keeping positions and fills.
pub fn paint_nodes_for_level(
    nodes: &[NodeIndex],
    positions: &Positions,
    camera: &Camera,
    viewport: Vec2,
    level: DetailLevel,
    node_style: impl Fn(NodeIndex) -> NodeStyle,
) -> Vec<PaintedNode> {
    let mut painted = Vec::new();
    for node in nodes {
        if let Some(entry) =
            paint_single_node_for_level(*node, positions, camera, viewport, level, &node_style)
        {
            painted.push(entry);
        }
    }
    painted
}

/// Node plan for one node, or nothing when it is missing or off-screen.
pub fn paint_single_node(
    node: NodeIndex,
    positions: &Positions,
    camera: &Camera,
    viewport: Vec2,
    node_style: impl Fn(NodeIndex) -> NodeStyle,
) -> Option<PaintedNode> {
    paint_single_node_for_level(
        node,
        positions,
        camera,
        viewport,
        DetailLevel::Full,
        node_style,
    )
}

/// Node plan for one node with explicit detail handling.
pub fn paint_single_node_for_level(
    node: NodeIndex,
    positions: &Positions,
    camera: &Camera,
    viewport: Vec2,
    level: DetailLevel,
    node_style: impl Fn(NodeIndex) -> NodeStyle,
) -> Option<PaintedNode> {
    let world = positions.get(&node)?;
    let screen = camera.world_to_viewport(viewport, *world);
    let visible = screen.x >= -NODE_SIDE
        && screen.y >= -NODE_SIDE
        && screen.x <= viewport.x + NODE_SIDE
        && screen.y <= viewport.y + NODE_SIDE;
    if !visible {
        return None;
    }
    let style = node_style(node);
    let side = (NODE_SIDE * style.scale).max(4.0);
    let minimal = level == DetailLevel::Minimal;
    let shape = shape_for_level(style.shape, minimal);
    let fill = if minimal {
        crate::style::NodeFill::solid(style.fill.solid_fallback())
    } else {
        style.fill
    };
    let image = if minimal {
        None
    } else {
        style.image.clone().filter(|spec| spec.is_supported())
    };
    let points = node_polygon(shape, side)
        .into_iter()
        .map(|vertex| Point2::new(screen.x + vertex.x, screen.y + vertex.y))
        .collect();
    Some(PaintedNode {
        id: node,
        origin: Point2::new(screen.x - side / 2.0, screen.y - side / 2.0),
        side,
        fill,
        stroke: style.stroke,
        stroke_width: style.stroke_width,
        opacity: style.opacity,
        shape,
        points,
        image,
    })
}

#[cfg(test)]
mod tests {
    use cg_graph::MockGraph;

    use super::*;
    use crate::style::{
        BypassStore, NodeFill, NodeStylePatch, SELECTED_NODE_FILL, StyleMapper, StyleSheet,
    };

    fn viewport() -> Vec2 {
        Vec2::new(1024.0, 768.0)
    }

    fn camera() -> Camera {
        Camera::new(Point2::ZERO, 1.0)
    }

    #[test]
    fn node_plan_uses_resolved_styles() {
        let graph = MockGraph::chain(2);
        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(-400.0, 0.0));
        positions.insert(NodeIndex::new(1), Point2::new(-300.0, 0.0));
        let sheet = StyleSheet::default();
        let mapper = StyleMapper::new();
        let mut bypass = BypassStore::new();
        bypass.set_node(NodeIndex::new(1), NodeStylePatch::selected());
        let plan = paint_nodes(&graph, &positions, &camera(), viewport(), |node| {
            bypass.resolve_node(&sheet, &mapper, node, None, 1)
        });
        assert_eq!(plan.len(), 2);
        assert_eq!(plan[0].fill, NodeStyle::default().fill);
        assert_eq!(plan[1].fill, NodeFill::solid(SELECTED_NODE_FILL));
    }

    #[test]
    fn plans_carry_stroke_and_opacity_from_styles() {
        let graph = MockGraph::chain(2);
        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(-400.0, 0.0));
        positions.insert(NodeIndex::new(1), Point2::new(-300.0, 0.0));
        let nodes = paint_nodes(&graph, &positions, &camera(), viewport(), |_| NodeStyle {
            stroke: 0x112233,
            stroke_width: 2.5,
            opacity: 0.5,
            ..NodeStyle::default()
        });
        assert_eq!(nodes.len(), 2);
        assert!(nodes.iter().all(|node| node.stroke == 0x112233));
        assert!(nodes.iter().all(|node| node.stroke_width == 2.5));
        assert!(nodes.iter().all(|node| node.opacity == 0.5));
    }

    #[test]
    fn visible_query_narrows_to_the_viewport() {
        use crate::spatial::SpatialIndex;

        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(0.0, 0.0));
        positions.insert(NodeIndex::new(1), Point2::new(5000.0, 5000.0));
        let mut index = SpatialIndex::new(48.0);
        index.rebuild(&positions);
        let near = Rect::new(Point2::new(-100.0, -100.0), Vec2::new(200.0, 200.0));
        assert_eq!(
            visible_node_ids(&positions, &index, near, NODE_SIDE / 2.0, |_| {
                NodeShape::Square
            }),
            vec![NodeIndex::new(0)]
        );
        let far = Rect::new(Point2::new(4900.0, 4900.0), Vec2::new(200.0, 200.0));
        assert_eq!(
            visible_node_ids(&positions, &index, far, NODE_SIDE / 2.0, |_| {
                NodeShape::Square
            }),
            vec![NodeIndex::new(1)]
        );
    }

    #[test]
    fn visible_query_honors_effective_shapes() {
        use crate::spatial::SpatialIndex;

        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::ZERO);
        let mut index = SpatialIndex::new(48.0);
        index.rebuild(&positions);
        let corner = Rect::from_corners(Point2::new(-12.0, -12.0), Point2::new(-8.0, -8.0));
        assert_eq!(
            visible_node_ids(&positions, &index, corner, NODE_SIDE / 2.0, |_| {
                NodeShape::Square
            }),
            vec![NodeIndex::new(0)]
        );
        assert!(
            visible_node_ids(&positions, &index, corner, NODE_SIDE / 2.0, |_| {
                NodeShape::Triangle
            })
            .is_empty()
        );
    }

    #[test]
    fn default_node_plan_stays_square() {
        let graph = MockGraph::isolated(1);
        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(0.0, 0.0));
        let plan = paint_nodes(&graph, &positions, &camera(), viewport(), |_| {
            NodeStyle::default()
        });
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].shape, NodeShape::Square);
        assert_eq!(plan[0].points.len(), 4);
    }

    #[test]
    fn shaped_nodes_carry_polygons_and_minimal_degrades() {
        let _graph = MockGraph::isolated(1);
        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(0.0, 0.0));
        let shaped = NodeStyle {
            shape: NodeShape::Diamond,
            ..NodeStyle::default()
        };
        let full = paint_single_node_for_level(
            NodeIndex::new(0),
            &positions,
            &camera(),
            viewport(),
            DetailLevel::Full,
            |_| shaped.clone(),
        )
        .expect("node stays visible");
        assert_eq!(full.shape, NodeShape::Diamond);
        assert_eq!(full.points.len(), 4);
        let minimal = paint_single_node_for_level(
            NodeIndex::new(0),
            &positions,
            &camera(),
            viewport(),
            DetailLevel::Minimal,
            |_| shaped.clone(),
        )
        .expect("node stays visible");
        assert_eq!(minimal.shape, NodeShape::Square);
    }

    #[test]
    fn minimal_detail_falls_back_to_a_solid_fill() {
        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(0.0, 0.0));
        let gradient = NodeStyle {
            fill: NodeFill::gradient(0x112233, 0x445566, 90.0),
            ..NodeStyle::default()
        };
        let full = paint_single_node_for_level(
            NodeIndex::new(0),
            &positions,
            &camera(),
            viewport(),
            DetailLevel::Full,
            |_| gradient.clone(),
        )
        .expect("node stays visible");
        assert!(!full.fill.is_solid());
        let minimal = paint_single_node_for_level(
            NodeIndex::new(0),
            &positions,
            &camera(),
            viewport(),
            DetailLevel::Minimal,
            |_| gradient.clone(),
        )
        .expect("node stays visible");
        assert!(minimal.fill.is_solid());
        assert_eq!(minimal.fill.start, 0x112233);
    }

    #[test]
    fn image_rides_on_the_plan_and_minimal_clears_it() {
        use crate::image::{ImageFit, NodeImage};

        let mut positions = Positions::new();
        positions.insert(NodeIndex::new(0), Point2::new(0.0, 0.0));
        let imaged = NodeStyle {
            image: Some(NodeImage::new("/tmp/a.png", ImageFit::Cover)),
            ..NodeStyle::default()
        };
        let full = paint_single_node_for_level(
            NodeIndex::new(0),
            &positions,
            &camera(),
            viewport(),
            DetailLevel::Full,
            |_| imaged.clone(),
        )
        .expect("node stays visible");
        assert!(full.image.is_some());
        let minimal = paint_single_node_for_level(
            NodeIndex::new(0),
            &positions,
            &camera(),
            viewport(),
            DetailLevel::Minimal,
            |_| imaged.clone(),
        )
        .expect("node stays visible");
        assert!(minimal.image.is_none());
        let remote = NodeStyle {
            image: Some(NodeImage::new(
                "https://example.com/a.png",
                ImageFit::Contain,
            )),
            ..NodeStyle::default()
        };
        let filtered = paint_single_node_for_level(
            NodeIndex::new(0),
            &positions,
            &camera(),
            viewport(),
            DetailLevel::Full,
            |_| remote.clone(),
        )
        .expect("node stays visible");
        assert!(filtered.image.is_none());
    }
}
