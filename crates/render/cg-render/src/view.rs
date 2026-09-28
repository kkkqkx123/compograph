//! Immediate-mode canvas element that draws the graph.

use cg_graph::{GraphStore, Positions};
use cg_types::{Point2, Vec2};
use gpui::{App, Bounds, IntoElement, Pixels, Window, canvas, fill, rgb};
use petgraph::visit::IntoNodeIdentifiers;

use crate::camera::Camera;

/// Side length, in logical pixels, of the placeholder node rectangle.
const NODE_SIDE: f32 = 24.0;

/// A node rectangle scheduled for painting, in screen pixels.
#[derive(Clone, Copy, Debug)]
pub struct PaintedNode {
    pub origin: Point2,
    pub side: f32,
}

/// Transforms graph structure and layout output into a paint plan.
///
/// Nodes outside the viewport are culled so the paint closure stays cheap on
/// large graphs.
pub fn paint_nodes(
    store: &GraphStore,
    positions: &Positions,
    camera: &Camera,
    viewport: Vec2,
) -> Vec<PaintedNode> {
    let mut painted = Vec::new();
    for node in store.graph().node_identifiers() {
        if let Some(world) = positions.get(&node) {
            let screen = camera.world_to_viewport(viewport, *world);
            let visible = screen.x >= -NODE_SIDE
                && screen.y >= -NODE_SIDE
                && screen.x <= viewport.x + NODE_SIDE
                && screen.y <= viewport.y + NODE_SIDE;
            if visible {
                painted.push(PaintedNode {
                    origin: Point2::new(screen.x - NODE_SIDE / 2.0, screen.y - NODE_SIDE / 2.0),
                    side: NODE_SIDE,
                });
            }
        }
    }
    painted
}

/// Canvas element that paints the prepared node plan.
///
/// Edge and label painting hook into the same closure once edge geometry
/// lands; the closure receives owned data so the element stays `'static`.
pub fn graph_view(nodes: Vec<PaintedNode>) -> impl IntoElement {
    canvas(
        |_bounds: Bounds<Pixels>, _window: &mut Window, _cx: &mut App| {},
        move |_bounds: Bounds<Pixels>, (), window: &mut Window, _cx: &mut App| {
            for node in &nodes {
                let quad = fill(
                    Bounds {
                        origin: gpui::point(gpui::px(node.origin.x), gpui::px(node.origin.y)),
                        size: gpui::size(gpui::px(node.side), gpui::px(node.side)),
                    },
                    rgb(0x4a9eff),
                );
                window.paint_quad(quad);
            }
        },
    )
}
