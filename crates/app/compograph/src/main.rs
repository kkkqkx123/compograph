//! compograph desktop application entry point.

use cg_graph::{GraphStore, Positions};
use cg_layout::{LayoutDriver, RandomLayout};
use cg_render::{Camera, graph_view, paint_nodes, subscribe_repaint};
use cg_types::{Point2, Vec2};
use gpui::{
    App, AppContext, Bounds, Context, Entity, IntoElement, Render, Subscription, Window,
    WindowBounds, WindowOptions, px, size,
};
use gpui_platform::application;

/// Node count of the built-in smoke scene.
const DEMO_NODE_COUNT: usize = 12;

/// Fixed viewport used until window resize plumbing lands.
const VIEWPORT: Vec2 = Vec2::new(1024.0, 768.0);

/// Radius of the initial scatter layout.
const DEMO_LAYOUT_RADIUS: f32 = 220.0;

/// Root view that owns the graph state and draws the canvas.
struct GraphWindow {
    store: Entity<GraphStore>,
    layout: Entity<LayoutDriver>,
    /// Kept alive so structural edits keep repainting the window.
    _refresh: Subscription,
    camera: Camera,
}

impl GraphWindow {
    fn new(cx: &mut Context<Self>) -> Self {
        let store = cx.new(|_| GraphStore::new());
        // Seed the demo scene. Each mutation broadcasts through the store, and
        // the layout driver reacts on its own subscription.
        store.update(cx, |graph, cx| {
            for ordinal in 0..DEMO_NODE_COUNT {
                graph.add_node(cx, format!("n{ordinal}"));
            }
            let node_ids: Vec<_> = graph.node_ids().collect();
            for pair in node_ids.windows(2) {
                graph.add_edge(cx, pair[0], pair[1], 1.0);
            }
        });
        let layout = cx.new(|cx| {
            LayoutDriver::new(cx, &store, Box::new(RandomLayout::new(DEMO_LAYOUT_RADIUS)))
        });
        // Structural edits repaint the window directly, independently of the
        // position changes the layout driver reports.
        let refresh = subscribe_repaint(cx, &store);
        Self {
            store,
            layout,
            _refresh: refresh,
            camera: Camera::new(Point2::ZERO, 1.0),
        }
    }
}

impl Render for GraphWindow {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let store = self.store.read(cx);
        let positions: &Positions = self.layout.read(cx).positions();
        let plan = paint_nodes(store, positions, &self.camera, VIEWPORT);
        graph_view(plan)
    }
}

fn main() {
    application().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1024.0), px(768.0)), cx);
        let opened = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| cx.new(GraphWindow::new),
        );
        if let Err(error) = opened {
            eprintln!("failed to open graph window: {error}");
        }
        cx.activate(true);
    });
}
