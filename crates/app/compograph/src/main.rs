//! compograph desktop application entry point.

use cg_graph::{GraphStore, GraphView, Positions};
use cg_interact::{
    DragState, PanState, SelectionState, drag_position, press_hit, wheel_zoom_factor,
};
use cg_layout::{ForceLayout, LayoutDriver, LayoutEngine, PresetLayout, RandomLayout};
use cg_render::{
    Camera, SpatialIndex, graph_view, paint_arrows, paint_edges, paint_nodes, subscribe_repaint,
};
use cg_types::{Point2, Vec2};
use gpui::{
    App, AppContext, Bounds, ClickEvent, Context, Entity, InteractiveElement, IntoElement,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Render, ScrollDelta,
    ScrollWheelEvent, StatefulInteractiveElement, Styled, Subscription, Window, WindowBounds,
    WindowOptions, div, px, size,
};
use gpui_platform::application;

/// Node count of the built-in smoke scene.
const DEMO_NODE_COUNT: usize = 12;

/// Engine names cycled by the toolbar, in order.
const LAYOUT_CYCLE: [&str; 3] = ["force", "random", "preset"];

/// Radius of the scatter layouts.
const DEMO_LAYOUT_RADIUS: f32 = 220.0;

/// Builds the engine behind a toolbar name.
fn engine_for(name: &str) -> Box<dyn LayoutEngine> {
    match name {
        "random" => Box::new(RandomLayout::new(DEMO_LAYOUT_RADIUS)),
        "preset" => Box::new(PresetLayout::new(DEMO_LAYOUT_RADIUS)),
        _ => Box::new(ForceLayout::new()),
    }
}

/// Root view that owns the graph state and draws the canvas.
struct GraphWindow {
    store: Entity<GraphStore>,
    layout: Entity<LayoutDriver>,
    /// Kept alive so structural edits keep repainting the window.
    _refresh: Subscription,
    /// Kept alive so background layout passes repaint the window.
    _layout_observer: Subscription,
    camera: Camera,
    viewport: Vec2,
    drag: DragState,
    pan: PanState,
    selection: SelectionState,
    spatial: SpatialIndex,
    layout_index: usize,
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
        let layout = cx.new(|cx| LayoutDriver::new(cx, &store, engine_for(LAYOUT_CYCLE[0])));
        // Structural edits repaint the window directly, independently of the
        // position changes the layout driver reports.
        let refresh = subscribe_repaint(cx, &store);
        let layout_observer = cx.observe(&layout, |_this, _entity, cx| {
            cx.notify();
        });
        let view = Self {
            store,
            layout,
            _refresh: refresh,
            _layout_observer: layout_observer,
            camera: Camera::new(Point2::ZERO, 1.0),
            viewport: Vec2::new(1024.0, 768.0),
            drag: DragState::default(),
            pan: PanState::default(),
            selection: SelectionState::default(),
            spatial: SpatialIndex::new(48.0),
            layout_index: 0,
        };
        view.layout.update(cx, |driver, cx| {
            driver.request_refine(&view.store, cx);
        });
        view
    }

    fn viewport_point(position: gpui::Point<gpui::Pixels>) -> Point2 {
        Point2::new(f32::from(position.x), f32::from(position.y))
    }

    fn hit_radius(&self) -> f32 {
        (24.0 / self.camera.zoom).max(4.0)
    }

    fn cycle_layout(&mut self, cx: &mut Context<Self>) {
        self.layout_index = (self.layout_index + 1) % LAYOUT_CYCLE.len();
        let engine = engine_for(LAYOUT_CYCLE[self.layout_index]);
        let store = self.store.clone();
        self.layout.update(cx, |driver, cx| {
            driver.set_engine(&store, engine, cx);
            driver.request_refine(&store, cx);
        });
        cx.notify();
    }
}

impl Render for GraphWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let size = window.viewport_size();
        self.viewport = Vec2::new(f32::from(size.width), f32::from(size.height));
        let store = self.store.read(cx);
        let view: &dyn GraphView = store;
        let positions: &Positions = self.layout.read(cx).positions();
        self.spatial.rebuild(positions);
        let selected = self.selection.selected();
        let nodes = paint_nodes(view, positions, &self.camera, self.viewport, selected);
        let edges = paint_edges(view, positions, &self.camera, self.viewport);
        let arrows = paint_arrows(&edges);
        let engine_name = self.layout.read(cx).engine_name();
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .id("layout-switch")
                    .px_2()
                    .py_1()
                    .child(format!("layout: {engine_name} (click to switch)"))
                    .on_click(cx.listener(|this, _event: &ClickEvent, _window, cx| {
                        this.cycle_layout(cx);
                    })),
            )
            .child(
                div()
                    .flex_1()
                    .child(graph_view(nodes, edges, arrows))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &MouseDownEvent, _window, cx| {
                            let viewport_point = Self::viewport_point(event.position);
                            let world =
                                this.camera.viewport_to_world(this.viewport, viewport_point);
                            let positions = this.layout.read(cx).positions().clone();
                            this.spatial.rebuild(&positions);
                            match press_hit(world, &positions, &this.spatial, this.hit_radius()) {
                                Some((node, offset)) => {
                                    this.drag.begin(node, offset);
                                    this.selection.select(node);
                                    this.pan.end();
                                }
                                None => {
                                    this.selection.clear();
                                    this.pan.begin(viewport_point);
                                }
                            }
                            cx.notify();
                        }),
                    )
                    .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _window, cx| {
                        let viewport_point = Self::viewport_point(event.position);
                        if let Some(node) = this.drag.active_node() {
                            let offset = this
                                .drag
                                .active
                                .as_ref()
                                .map(|gesture| gesture.grab_offset)
                                .unwrap_or_default();
                            let world =
                                this.camera.viewport_to_world(this.viewport, viewport_point);
                            let target = drag_position(world, offset);
                            this.layout.update(cx, |driver, cx| {
                                driver.move_pinned(node, target, cx);
                            });
                            cx.notify();
                        } else if let Some(delta) = this.pan.advance(viewport_point) {
                            let zoom = this.camera.zoom;
                            this.camera
                                .pan_by(Vec2::new(-delta.x / zoom, -delta.y / zoom));
                            cx.notify();
                        }
                    }))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _event: &MouseUpEvent, _window, cx| {
                            let dragged = this.drag.end().is_some();
                            this.pan.end();
                            if dragged {
                                let store = this.store.clone();
                                this.layout.update(cx, |driver, cx| {
                                    driver.request_refine(&store, cx);
                                });
                            }
                            cx.notify();
                        }),
                    )
                    .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _window, cx| {
                        let anchor = Self::viewport_point(event.position);
                        let lines = match &event.delta {
                            ScrollDelta::Pixels(pixels) => f32::from(pixels.y) / 16.0,
                            ScrollDelta::Lines(lines) => lines.y,
                        };
                        this.camera
                            .zoom_at(this.viewport, anchor, wheel_zoom_factor(lines));
                        cx.notify();
                    })),
            )
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
