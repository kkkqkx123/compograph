//! compograph desktop application entry point.

mod algo_panel;
mod file_io;

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::time::Instant;

use algo_panel::{AlgoOutcome, EdgePair};
use cg_graph::{
    ChangeFilter, ClusterMetric, GraphChangeEvent, GraphDocument, GraphStore, GraphView, NodeEntry,
    NodeIndex, Positions, affinity_clusters, all_pairs_shortest_paths, articulation_points,
    bellman_ford_paths, betweenness_centrality, breadth_first_order, bridges, closeness_centrality,
    degree_centrality, depth_first_order, eulerian_path_directed, eulerian_path_undirected,
    export_dot, global_min_cut, heuristic_shortest_path, hierarchical_clusters,
    immediate_dominators, kmeans_clusters, markov_clusters, metric_clusters,
    minimum_spanning_forest, minimum_spanning_tree_single, negative_cycle_path, node_order,
    rank_nodes, remap_positions, shortest_path, strongly_connected_components, subscribe_graph,
    topological_order, transitive_reduction,
};
use cg_interact::{
    BoxSelectState, DragState, InteractLocks, NODE_HALF_EXTENT, SelectMode, SelectionState,
    apply_point_select, can_begin_drag, can_grab_node, drag_position, edges_in_rect,
    expand_neighborhood, hover_node_shaped, neighborhood_edges, nodes_in_rect, press_hit_shaped,
    should_clear_on_blank, wheel_zoom_factor,
};
use cg_layout::{LayoutDriver, LayoutRegistry};
use cg_render::{
    BypassStore, CacheVersions, Camera, DetailLevel, EdgeMapper, EdgePaintOptions, EdgeStylePatch,
    ExportRequest, ExportScope, ExportSnapshot, FrameMetrics, FrameSample, LodParams, NODE_SIDE,
    NodeShape, NodeStylePatch, PaintedArrow, PaintedEdge, PaintedNode, PaintedRubberBand,
    PlanCounts, RefreshInput, RetainedCache, SpatialIndex, StoredPlans, StyleMapper, StyleSheet,
    edge_ordinals_for, encode_png, export_pixels, graph_view, paint_arrows_for_level,
    paint_edge_labels_for, paint_edges_for, paint_labels_for, paint_nodes_for_level,
    subscribe_repaint, visible_node_ids, world_viewport_rect,
};
use cg_types::{Point2, Rect, Vec2};
use gpui::{
    App, AppContext, Bounds, ClickEvent, Context, Entity, FocusHandle, InteractiveElement,
    IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    ParentElement, PathPromptOptions, Render, ScrollDelta, ScrollWheelEvent,
    StatefulInteractiveElement, Styled, Subscription, Task, Window, WindowBounds, WindowOptions,
    div, prelude::FluentBuilder, px, size,
};
use gpui_platform::application;

/// Node count of the built-in smoke scene.
const DEMO_NODE_COUNT: usize = 12;

/// Clicks shorter than this viewport distance count as taps, not box selects.
const TAP_THRESHOLD: f32 = 4.0;

/// Key dismissing the current selection and any in-progress box select.
const DISMISS_KEY: &str = "escape";

/// PageRank refinement rounds per panel run.
const PAGERANK_ITERATIONS: usize = 20;

/// Markov inflation per panel run; larger values yield finer groups.
const MARKOV_INFLATION: f32 = 2.0;

/// Markov iteration budget per panel run.
const MARKOV_ITERATIONS: usize = 20;

/// K-means iteration budget per panel run.
const KMEANS_ITERATIONS: usize = 20;

/// Single-linkage distance threshold per panel run, in model units.
const HIERARCHICAL_THRESHOLD: f32 = 120.0;

/// Affinity propagation damping per panel run.
const AFFINITY_DAMPING: f32 = 0.5;

/// Affinity propagation iteration budget per panel run.
const AFFINITY_ITERATIONS: usize = 100;

/// Metric clustering threshold per panel run, in model units.
const METRIC_THRESHOLD: f32 = 120.0;

/// Damping step of the panel controls, clamped to the unit interval.
const DAMPING_STEP: f32 = 0.05;

/// Magnification applied to exported images.
const EXPORT_SCALE: f32 = 2.0;

/// True when the pressed key dismisses the selection.
fn is_dismiss_key(key: &str) -> bool {
    key == DISMISS_KEY
}

/// Ordinal of `node` within the sorted store order, for endpoint slots.
fn endpoint_slot(ids: &[NodeIndex], node: NodeIndex) -> Option<usize> {
    ids.iter().position(|candidate| *candidate == node)
}

/// Directory the file dialogs open in, falling back to scratch space.
fn working_directory() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/tmp"))
}

/// Root view that owns the graph state and draws the canvas.
struct GraphWindow {
    store: Entity<GraphStore>,
    layout: Entity<LayoutDriver>,
    /// Kept alive so structural edits keep repainting the window.
    _refresh: Subscription,
    /// Kept alive so background layout passes repaint the window.
    _layout_observer: Subscription,
    /// Kept alive so structural edits invalidate the retained paint cache.
    _structure_mark: Subscription,
    camera: Camera,
    viewport: Vec2,
    drag: DragState,
    rubber: BoxSelectState,
    selection: SelectionState,
    selected_edges: Vec<(NodeIndex, NodeIndex)>,
    select_mode: SelectMode,
    locks: InteractLocks,
    neighbor_hops: usize,
    neighbor_nodes: BTreeSet<NodeIndex>,
    neighbor_edges: Vec<(NodeIndex, NodeIndex)>,
    spatial: SpatialIndex,
    focus: FocusHandle,
    layouts: Vec<&'static str>,
    menu_open: bool,
    sheet: StyleSheet,
    mapper: StyleMapper,
    edge_mapper: EdgeMapper,
    bypass: BypassStore,
    hovered: Option<NodeIndex>,
    hover_anchor: Point2,
    algo_nodes: HashMap<NodeIndex, NodeStylePatch>,
    algo_edges: HashMap<EdgePair, EdgeStylePatch>,
    algo_summary: String,
    algo_busy: bool,
    algo_generation: u64,
    algo_task: Option<Task<()>>,
    io_task: Option<Task<()>>,
    algo_start: usize,
    algo_goal: usize,
    damping: f32,
    io_message: String,
    lod: DetailLevel,
    lod_params: LodParams,
    aggregate: bool,
    retained: RetainedCache,
    structure_version: u64,
    style_version: u64,
    spatial_version: Option<u64>,
    metrics: FrameMetrics,
    export_message: String,
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
        let layouts = LayoutRegistry::with_defaults().names();
        let first = layouts.first().copied().unwrap_or("force");
        let engine = LayoutRegistry::engine_for(first).expect("default layout is registered");
        let layout = cx.new(|cx| LayoutDriver::new(cx, &store, engine));
        // Structural edits repaint the window directly, independently of the
        // position changes the layout driver reports.
        let refresh = subscribe_repaint(cx, &store);
        // Structural edits also invalidate the retained paint cache, so a
        // later topology change can never reuse plans from before the edit.
        // The counter is the structure generation: every accepted mutation
        // moves it exactly once through this subscription.
        let structure_mark =
            subscribe_graph(
                cx,
                &store,
                ChangeFilter::ALL,
                |this, event, _cx| match event {
                    GraphChangeEvent::NodeAttrChanged(_)
                    | GraphChangeEvent::EdgeAttrChanged(_)
                    | GraphChangeEvent::NodeClassChanged(_)
                    | GraphChangeEvent::EdgeClassChanged(_) => {
                        this.style_version += 1;
                        this.retained.mark_structure();
                    }
                    _ => {
                        this.structure_version += 1;
                        this.retained.mark_structure();
                    }
                },
            );
        let layout_observer = cx.observe(&layout, |_this, _entity, cx| {
            cx.notify();
        });
        let view = Self {
            store,
            layout,
            _refresh: refresh,
            _structure_mark: structure_mark,
            _layout_observer: layout_observer,
            camera: Camera::new(Point2::ZERO, 1.0),
            viewport: Vec2::new(1024.0, 768.0),
            drag: DragState::default(),
            rubber: BoxSelectState::default(),
            selection: SelectionState::default(),
            selected_edges: Vec::new(),
            select_mode: SelectMode::Single,
            locks: InteractLocks::default(),
            neighbor_hops: 1,
            neighbor_nodes: BTreeSet::new(),
            neighbor_edges: Vec::new(),
            spatial: SpatialIndex::new(48.0),
            focus: cx.focus_handle(),
            layouts,
            menu_open: false,
            sheet: StyleSheet::default(),
            mapper: StyleMapper::default(),
            edge_mapper: EdgeMapper::new(),
            bypass: BypassStore::new(),
            hovered: None,
            hover_anchor: Point2::ZERO,
            algo_nodes: HashMap::new(),
            algo_edges: HashMap::new(),
            algo_summary: "no algorithm run yet".to_string(),
            algo_busy: false,
            algo_generation: 0,
            algo_task: None,
            io_task: None,
            algo_start: 0,
            algo_goal: 1,
            damping: 0.85,
            io_message: String::new(),
            lod: DetailLevel::Full,
            lod_params: LodParams::default(),
            aggregate: false,
            retained: RetainedCache::new(false),
            structure_version: 0,
            style_version: 0,
            spatial_version: None,
            metrics: FrameMetrics::new(30),
            export_message: String::new(),
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

    fn switch_layout(&mut self, name: &'static str, cx: &mut Context<Self>) {
        let Some(engine) = LayoutRegistry::engine_for(name) else {
            return;
        };
        let store = self.store.clone();
        self.layout.update(cx, |driver, cx| {
            driver.set_engine(&store, engine, cx);
            driver.request_refine(&store, cx);
        });
        self.menu_open = false;
        cx.notify();
    }

    /// Drops the node and edge selection and aborts any box select.
    ///
    /// Algorithm highlights survive: they live in their own maps and are
    /// merged back by [`GraphWindow::rebuild_bypass`]. Derived neighborhood
    /// entries clear with the seeds, so no separate cleanup path exists.
    fn clear_selection(&mut self) {
        self.selection.clear();
        self.selected_edges.clear();
        self.neighbor_nodes.clear();
        self.neighbor_edges.clear();
        self.rubber.cancel();
        self.rebuild_bypass();
    }

    /// Derives fringe nodes and internal edges for a selection snapshot.
    ///
    /// The fringe holds expanded nodes minus the seeds, and the edge list
    /// holds internal edges of the expanded set. Both are merged by
    /// [`GraphWindow::rebuild_bypass`] without touching the main styles.
    fn derive_neighborhood(
        graph: &dyn GraphView,
        selection: &SelectionState,
        selected_edges: &[(NodeIndex, NodeIndex)],
        hops: usize,
    ) -> (BTreeSet<NodeIndex>, Vec<(NodeIndex, NodeIndex)>) {
        let seeds: Vec<NodeIndex> = selection.iter().collect();
        if seeds.is_empty() || hops == 0 {
            return (BTreeSet::new(), Vec::new());
        }
        let expanded = expand_neighborhood(graph, seeds, hops);
        let mut fringe = BTreeSet::new();
        for node in &expanded {
            if !selection.contains(*node) {
                fringe.insert(*node);
            }
        }
        let edges = neighborhood_edges(graph, &expanded)
            .into_iter()
            .filter(|pair| !selected_edges.contains(pair))
            .collect();
        (fringe, edges)
    }

    /// Rebuilds the bypass from hover, selection, neighborhood plus highlights.
    ///
    /// Hover sits below selection, neighborhood sits above selection, and
    /// algorithm patches overlay all three, so a highlighted path stays
    /// visible even where it crosses the current selection or the hovered
    /// node. Neighborhood entries only fill vacant slots, never overriding
    /// selection or algorithm results.
    fn rebuild_bypass(&mut self) {
        self.bypass.clear_all();
        self.style_version += 1;
        if let Some(node) = self.hovered {
            self.bypass.set_node(node, NodeStylePatch::hovered());
        }
        for node in self.selection.iter() {
            self.bypass.set_node(node, NodeStylePatch::selected());
        }
        for (source, target) in &self.selected_edges {
            self.bypass
                .set_edge(*source, *target, EdgeStylePatch::highlighted());
        }
        for node in &self.neighbor_nodes {
            if self.bypass.node_bypass(*node).is_none() {
                self.bypass.set_node(*node, NodeStylePatch::hovered());
            }
        }
        for (source, target) in &self.neighbor_edges {
            if self.bypass.edge_bypass(*source, *target).is_none() {
                self.bypass
                    .set_edge(*source, *target, EdgeStylePatch::highlighted());
            }
        }
        for (node, patch) in &self.algo_nodes {
            self.bypass.set_node(*node, patch.clone());
        }
        for ((source, target), patch) in &self.algo_edges {
            self.bypass.set_edge(*source, *target, patch.clone());
        }
    }

    /// Drops algorithm highlights while keeping the selection intact.
    fn clear_algo_highlights(&mut self) {
        self.algo_nodes.clear();
        self.algo_edges.clear();
        self.rebuild_bypass();
    }

    /// Rebuilds the spatial index only after position write-backs.
    ///
    /// Viewport motion reuses the cached cells; structural edits and layout
    /// write-backs move the positions version and trigger a rebuild. The
    /// adaptive cell follows the node extent.
    fn refresh_spatial(&mut self, positions: &Positions, version: u64) {
        if self.spatial.ensure_cell(NODE_SIDE, positions) || self.spatial_version != Some(version) {
            self.spatial.rebuild(positions);
            self.spatial_version = Some(version);
        }
    }

    /// Applies a finished rubber-band rectangle to the selection.
    ///
    /// The viewport rectangle is converted to model space once, then node and
    /// edge membership come from the shared model-space selection helpers, so
    /// box selection agrees with pointer hit testing.
    fn finish_rubber(&mut self, viewport_rect: Rect, additive: bool, cx: &mut Context<Self>) {
        if viewport_rect.size.x < TAP_THRESHOLD && viewport_rect.size.y < TAP_THRESHOLD {
            return;
        }
        let far = Point2::new(
            viewport_rect.origin.x + viewport_rect.size.x,
            viewport_rect.origin.y + viewport_rect.size.y,
        );
        let model_rect = Rect::from_corners(
            self.camera
                .viewport_to_world(self.viewport, viewport_rect.origin),
            self.camera.viewport_to_world(self.viewport, far),
        );
        let positions = self.layout.read(cx).positions().clone();
        let version = self.layout.read(cx).positions_version();
        self.refresh_spatial(&positions, version);
        let nodes = nodes_in_rect(
            &positions,
            &self.spatial,
            model_rect,
            NODE_HALF_EXTENT,
            |node| self.node_shape(cx, node),
        );
        let store = self.store.read(cx);
        let view: &dyn GraphView = store;
        let edges = edges_in_rect(view, &positions, model_rect, NODE_SIDE);
        if additive {
            self.selection.add_many(nodes);
            for pair in edges {
                if !self.selected_edges.contains(&pair) {
                    self.selected_edges.push(pair);
                }
            }
        } else {
            self.selection.select_many(nodes);
            self.selected_edges = edges;
        }
        let (fringe, neighbor_edges) = Self::derive_neighborhood(
            view,
            &self.selection,
            &self.selected_edges,
            self.neighbor_hops,
        );
        self.neighbor_nodes = fringe;
        self.neighbor_edges = neighbor_edges;
        self.rebuild_bypass();
    }

    fn hover_label(&self, cx: &App) -> Option<String> {
        let node = self.hovered?;
        self.store
            .read(cx)
            .node_data(node)
            .map(|data| data.label.clone())
    }

    fn node_shape(&self, cx: &App, node: NodeIndex) -> cg_render::NodeShape {
        let store = self.store.read(cx);
        let label = store
            .node_data(node)
            .map(|data| data.label.clone())
            .unwrap_or_default();
        let view: &dyn GraphView = store;
        self.bypass
            .resolve_node(
                &self.sheet,
                &self.mapper,
                node,
                Some(label.as_str()),
                view.degree(node),
            )
            .shape
    }

    /// Sorted node identifiers currently in the store.
    fn ordered_nodes(&self, cx: &App) -> Vec<NodeIndex> {
        let mut ids: Vec<NodeIndex> = self.store.read(cx).node_ids().collect();
        ids.sort_unstable_by_key(|node| node.index());
        ids
    }

    /// Start and goal nodes selected by the panel parameters.
    fn algo_endpoints(&self, cx: &App) -> Option<(NodeIndex, NodeIndex)> {
        let ids = self.ordered_nodes(cx);
        if ids.is_empty() {
            return None;
        }
        Some((
            ids[self.algo_start % ids.len()],
            ids[self.algo_goal % ids.len()],
        ))
    }

    /// Label shown for a panel endpoint slot.
    fn endpoint_label(&self, slot: usize, cx: &App) -> String {
        let ids = self.ordered_nodes(cx);
        if ids.is_empty() {
            return "-".to_string();
        }
        let node = ids[slot % ids.len()];
        self.store
            .read(cx)
            .node_data(node)
            .map(|data| data.label.clone())
            .unwrap_or_default()
    }

    /// Marks a new algorithm run and returns its generation.
    ///
    /// Later runs supersede earlier ones: a background task whose generation
    /// no longer matches commits nothing.
    fn begin_algo_run(&mut self) -> u64 {
        self.algo_generation += 1;
        self.algo_busy = true;
        self.algo_generation
    }

    /// Commits a background outcome unless a newer run superseded it.
    ///
    /// Returns false for stale generations without touching any state, so
    /// overlapping runs cannot overwrite each other out of order.
    fn commit_outcome(&mut self, generation: u64, outcome: AlgoOutcome) -> bool {
        if algo_panel::is_stale(self.algo_generation, generation) {
            return false;
        }
        self.algo_nodes.clear();
        self.algo_edges.clear();
        for (node, patch) in outcome.nodes {
            self.algo_nodes.insert(node, patch);
        }
        for (pair, patch) in outcome.edges {
            self.algo_edges.insert(pair, patch);
        }
        self.algo_summary = outcome.summary;
        self.algo_busy = false;
        self.rebuild_bypass();
        true
    }

    /// Hands an outcome computed off-thread back to this view.
    ///
    /// Dropping the previous handle asks the framework to cancel the
    /// superseded run; cancellation is best-effort, so the generation guard
    /// in [`GraphWindow::commit_outcome`] stays the correctness barrier.
    fn spawn_algo_task(
        &mut self,
        cx: &mut Context<Self>,
        generation: u64,
        compute: impl FnOnce() -> AlgoOutcome + Send + 'static,
    ) {
        self.algo_task = None;
        let task = cx.spawn(async move |weak, async_cx| {
            let outcome = async_cx
                .background_executor()
                .spawn(async move { compute() })
                .await;
            weak.update(&mut *async_cx, |this, cx| {
                if this.commit_outcome(generation, outcome) {
                    cx.notify();
                }
            })
            .ok();
        });
        self.algo_task = Some(task);
        cx.notify();
    }

    fn run_shortest_path(&mut self, cx: &mut Context<Self>) {
        let Some((start, goal)) = self.algo_endpoints(cx) else {
            self.algo_summary = "shortest path needs at least one node".to_string();
            cx.notify();
            return;
        };
        let snapshot = self.store.read(cx).graph().clone();
        let generation = self.begin_algo_run();
        self.spawn_algo_task(cx, generation, move || {
            let started = Instant::now();
            let found = shortest_path(&snapshot, start, goal);
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            match found {
                Some((cost, path)) => {
                    algo_panel::path_outcome(&path, cost, elapsed_ms, "shortest path")
                }
                None => algo_panel::path_outcome(&[], 0.0, elapsed_ms, "shortest path"),
            }
        });
    }

    fn run_heuristic_path(&mut self, cx: &mut Context<Self>) {
        let Some((start, goal)) = self.algo_endpoints(cx) else {
            self.algo_summary = "guided search needs at least one node".to_string();
            cx.notify();
            return;
        };
        let snapshot = self.store.read(cx).graph().clone();
        let positions: Positions = self.layout.read(cx).positions().clone();
        let generation = self.begin_algo_run();
        self.spawn_algo_task(cx, generation, move || {
            let started = Instant::now();
            let found = heuristic_shortest_path(&snapshot, &positions, start, goal);
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            match found {
                Some((cost, path)) => {
                    algo_panel::path_outcome(&path, cost, elapsed_ms, "guided search")
                }
                None => algo_panel::path_outcome(&[], 0.0, elapsed_ms, "guided search"),
            }
        });
    }

    fn run_components(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.store.read(cx).graph().clone();
        let generation = self.begin_algo_run();
        self.spawn_algo_task(cx, generation, move || {
            let started = Instant::now();
            let groups = strongly_connected_components(&snapshot);
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            algo_panel::scc_outcome(&groups, elapsed_ms)
        });
    }

    fn run_pagerank(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.store.read(cx).graph().clone();
        let damping = self.damping;
        let generation = self.begin_algo_run();
        self.spawn_algo_task(cx, generation, move || {
            let started = Instant::now();
            let order: Vec<NodeIndex> = snapshot.node_indices().collect();
            let scores = rank_nodes(&snapshot, damping, PAGERANK_ITERATIONS);
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            algo_panel::pagerank_outcome(&order, &scores, elapsed_ms)
        });
    }

    fn run_spanning_forest(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.store.read(cx).graph().clone();
        let generation = self.begin_algo_run();
        self.spawn_algo_task(cx, generation, move || {
            let started = Instant::now();
            let edges = minimum_spanning_forest(&snapshot);
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            algo_panel::forest_outcome(&edges, elapsed_ms)
        });
    }

    fn run_degree(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.store.read(cx).graph().clone();
        let generation = self.begin_algo_run();
        self.spawn_algo_task(cx, generation, move || {
            let started = Instant::now();
            let order = node_order(&snapshot);
            let scores = degree_centrality(&snapshot);
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            algo_panel::centrality_outcome(&order, &scores, "degree", elapsed_ms)
        });
    }

    fn run_cuts(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.store.read(cx).graph().clone();
        let generation = self.begin_algo_run();
        self.spawn_algo_task(cx, generation, move || {
            let started = Instant::now();
            let points = articulation_points(&snapshot);
            let cuts = bridges(&snapshot);
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            algo_panel::cut_outcome(&points, &cuts, elapsed_ms)
        });
    }

    fn run_all_pairs(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.store.read(cx).graph().clone();
        let generation = self.begin_algo_run();
        self.spawn_algo_task(cx, generation, move || {
            let started = Instant::now();
            let count = snapshot.node_count();
            match all_pairs_shortest_paths(&snapshot) {
                Ok(matrix) => algo_panel::pairs_outcome(
                    matrix.len(),
                    count * count,
                    started.elapsed().as_secs_f64() * 1000.0,
                ),
                Err(member) => AlgoOutcome {
                    summary: format!(
                        "all pairs: negative cycle at node {} ({:.1}ms)",
                        member.index(),
                        started.elapsed().as_secs_f64() * 1000.0
                    ),
                    ..AlgoOutcome::default()
                },
            }
        });
    }

    fn run_bellman_ford(&mut self, cx: &mut Context<Self>) {
        let Some((start, goal)) = self.algo_endpoints(cx) else {
            self.algo_summary = "bellman-ford needs at least one node".to_string();
            cx.notify();
            return;
        };
        let snapshot = self.store.read(cx).graph().clone();
        let generation = self.begin_algo_run();
        self.spawn_algo_task(cx, generation, move || {
            let started = Instant::now();
            let elapsed = || started.elapsed().as_secs_f64() * 1000.0;
            match bellman_ford_paths(&snapshot, start) {
                Ok((distances, predecessors)) => {
                    let cost = distances.get(&goal).copied().unwrap_or(f32::INFINITY);
                    if !cost.is_finite() {
                        return algo_panel::path_outcome(&[], 0.0, elapsed(), "bellman-ford");
                    }
                    let mut path = vec![goal];
                    while let Some(parent) = predecessors.get(&path.last().copied().unwrap_or(goal)).copied().flatten() {
                        path.push(parent);
                        if parent == start || path.len() > snapshot.node_count() + 1 {
                            break;
                        }
                    }
                    path.reverse();
                    if path.first() != Some(&start) {
                        return algo_panel::path_outcome(&[], 0.0, elapsed(), "bellman-ford");
                    }
                    algo_panel::path_outcome(&path, cost, elapsed(), "bellman-ford")
                }
                Err(member) => {
                    let cycle_len = negative_cycle_path(&snapshot, start).map(|cycle| cycle.len()).unwrap_or(0);
                    AlgoOutcome {
                        summary: format!(
                            "bellman-ford: negative cycle at node {0} ({cycle_len} nodes, {1:.1}ms)",
                            member.index(),
                            elapsed()
                        ),
                        ..AlgoOutcome::default()
                    }
                }
            }
        });
    }

    fn run_traversals(&mut self, cx: &mut Context<Self>) {
        let Some((start, _)) = self.algo_endpoints(cx) else {
            self.algo_summary = "traversals need at least one node".to_string();
            cx.notify();
            return;
        };
        let snapshot = self.store.read(cx).graph().clone();
        let generation = self.begin_algo_run();
        self.spawn_algo_task(cx, generation, move || {
            let started = Instant::now();
            let breadth = breadth_first_order(&snapshot, start);
            let depth = depth_first_order(&snapshot, start);
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            let mut outcome = algo_panel::traversal_outcome(&breadth, "bfs", elapsed_ms);
            outcome.summary = format!(
                "traversals from {}: bfs {}, dfs {} ({elapsed_ms:.1}ms)",
                start.index(),
                breadth.len(),
                depth.len()
            );
            outcome
        });
    }

    fn run_topo_reduction(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.store.read(cx).graph().clone();
        let generation = self.begin_algo_run();
        self.spawn_algo_task(cx, generation, move || {
            let started = Instant::now();
            let order = topological_order(&snapshot);
            let kept = transitive_reduction(&snapshot);
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            let topo = algo_panel::topo_outcome(&order, elapsed_ms);
            let reduced = algo_panel::reduction_outcome(&kept, elapsed_ms);
            let summary = match (&order, &kept) {
                (Ok(sequence), Ok(edges)) => format!(
                    "topo+reduction: order {}, kept {} ({elapsed_ms:.1}ms)",
                    sequence.len(),
                    edges.len()
                ),
                (Err(member), _) | (_, Err(member)) => format!(
                    "topo+reduction: cycle at node {} ({elapsed_ms:.1}ms)",
                    member.index()
                ),
            };
            AlgoOutcome {
                nodes: topo.nodes,
                edges: reduced.edges,
                summary,
            }
        });
    }

    fn run_closeness(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.store.read(cx).graph().clone();
        let generation = self.begin_algo_run();
        self.spawn_algo_task(cx, generation, move || {
            let started = Instant::now();
            let order = node_order(&snapshot);
            let scores = closeness_centrality(&snapshot);
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            algo_panel::centrality_outcome(&order, &scores, "closeness", elapsed_ms)
        });
    }

    fn run_betweenness(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.store.read(cx).graph().clone();
        let generation = self.begin_algo_run();
        self.spawn_algo_task(cx, generation, move || {
            let started = Instant::now();
            let order = node_order(&snapshot);
            let scores = betweenness_centrality(&snapshot);
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            algo_panel::centrality_outcome(&order, &scores, "betweenness", elapsed_ms)
        });
    }

    fn run_mst_single(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.store.read(cx).graph().clone();
        let generation = self.begin_algo_run();
        self.spawn_algo_task(cx, generation, move || {
            let started = Instant::now();
            let edges = minimum_spanning_tree_single(&snapshot);
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            let mut outcome = algo_panel::forest_outcome(&edges, elapsed_ms);
            outcome.summary = outcome.summary.replace("spanning forest", "spanning tree");
            outcome
        });
    }

    fn run_dominators(&mut self, cx: &mut Context<Self>) {
        let Some((start, _)) = self.algo_endpoints(cx) else {
            self.algo_summary = "dominators need at least one node".to_string();
            cx.notify();
            return;
        };
        let snapshot = self.store.read(cx).graph().clone();
        let generation = self.begin_algo_run();
        self.spawn_algo_task(cx, generation, move || {
            let started = Instant::now();
            let parents = immediate_dominators(&snapshot, start);
            let mut dominated: Vec<NodeIndex> = parents.keys().copied().collect();
            dominated.sort_unstable_by_key(|node| node.index());
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            algo_panel::dominators_outcome(&dominated, start, elapsed_ms)
        });
    }

    fn run_euler_directed(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.store.read(cx).graph().clone();
        let generation = self.begin_algo_run();
        self.spawn_algo_task(cx, generation, move || {
            let started = Instant::now();
            let trail = eulerian_path_directed(&snapshot);
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            algo_panel::euler_outcome(&trail, "euler directed", elapsed_ms)
        });
    }

    fn run_euler_undirected(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.store.read(cx).graph().clone();
        let generation = self.begin_algo_run();
        self.spawn_algo_task(cx, generation, move || {
            let started = Instant::now();
            let trail = eulerian_path_undirected(&snapshot);
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            algo_panel::euler_outcome(&trail, "euler undirected", elapsed_ms)
        });
    }

    fn run_min_cut(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.store.read(cx).graph().clone();
        let generation = self.begin_algo_run();
        self.spawn_algo_task(cx, generation, move || {
            let started = Instant::now();
            let cut = global_min_cut(&snapshot);
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            algo_panel::mincut_outcome(&cut, elapsed_ms)
        });
    }

    fn run_hierarchical(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.store.read(cx).graph().clone();
        let positions: Positions = self.layout.read(cx).positions().clone();
        let generation = self.begin_algo_run();
        self.spawn_algo_task(cx, generation, move || {
            let started = Instant::now();
            let elapsed_ms = || started.elapsed().as_secs_f64() * 1000.0;
            match hierarchical_clusters(&snapshot, &positions, HIERARCHICAL_THRESHOLD) {
                Ok(groups) => algo_panel::groups_outcome(&groups, "hierarchical", elapsed_ms()),
                Err(member) => AlgoOutcome {
                    summary: format!(
                        "hierarchical: invalid input at node {} ({:.1}ms)",
                        member.index(),
                        elapsed_ms()
                    ),
                    ..AlgoOutcome::default()
                },
            }
        });
    }

    fn run_markov(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.store.read(cx).graph().clone();
        let generation = self.begin_algo_run();
        self.spawn_algo_task(cx, generation, move || {
            let started = Instant::now();
            let elapsed_ms = || started.elapsed().as_secs_f64() * 1000.0;
            match markov_clusters(&snapshot, MARKOV_INFLATION, MARKOV_ITERATIONS) {
                Ok(groups) => algo_panel::groups_outcome(&groups, "markov", elapsed_ms()),
                Err(member) => AlgoOutcome {
                    summary: format!(
                        "markov: invalid input at node {} ({:.1}ms)",
                        member.index(),
                        elapsed_ms()
                    ),
                    ..AlgoOutcome::default()
                },
            }
        });
    }

    fn run_kmeans(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.store.read(cx).graph().clone();
        let positions: Positions = self.layout.read(cx).positions().clone();
        let classes = snapshot.node_count().clamp(1, 2);
        let generation = self.begin_algo_run();
        self.spawn_algo_task(cx, generation, move || {
            let started = Instant::now();
            let elapsed_ms = || started.elapsed().as_secs_f64() * 1000.0;
            match kmeans_clusters(&snapshot, &positions, classes, KMEANS_ITERATIONS) {
                Ok(groups) => algo_panel::groups_outcome(&groups, "k-means", elapsed_ms()),
                Err(member) => AlgoOutcome {
                    summary: format!(
                        "k-means: invalid input at node {} ({:.1}ms)",
                        member.index(),
                        elapsed_ms()
                    ),
                    ..AlgoOutcome::default()
                },
            }
        });
    }

    fn run_affinity(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.store.read(cx).graph().clone();
        let positions: Positions = self.layout.read(cx).positions().clone();
        let generation = self.begin_algo_run();
        self.spawn_algo_task(cx, generation, move || {
            let started = Instant::now();
            let elapsed_ms = || started.elapsed().as_secs_f64() * 1000.0;
            match affinity_clusters(&snapshot, &positions, AFFINITY_DAMPING, AFFINITY_ITERATIONS) {
                Ok(groups) => algo_panel::groups_outcome(&groups, "affinity", elapsed_ms()),
                Err(member) => AlgoOutcome {
                    summary: format!(
                        "affinity: invalid input at node {} ({:.1}ms)",
                        member.index(),
                        elapsed_ms()
                    ),
                    ..AlgoOutcome::default()
                },
            }
        });
    }

    fn run_metric_clusters(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.store.read(cx).graph().clone();
        let positions: Positions = self.layout.read(cx).positions().clone();
        let generation = self.begin_algo_run();
        self.spawn_algo_task(cx, generation, move || {
            let started = Instant::now();
            let elapsed_ms = || started.elapsed().as_secs_f64() * 1000.0;
            match metric_clusters(
                &snapshot,
                &positions,
                ClusterMetric::Euclidean,
                METRIC_THRESHOLD,
            ) {
                Ok(groups) => algo_panel::groups_outcome(&groups, "metric", elapsed_ms()),
                Err(member) => AlgoOutcome {
                    summary: format!(
                        "metric: invalid input at node {} ({:.1}ms)",
                        member.index(),
                        elapsed_ms()
                    ),
                    ..AlgoOutcome::default()
                },
            }
        });
    }

    fn export_json(&mut self, cx: &mut Context<Self>) {
        let store = self.store.read(cx);
        let positions = self.layout.read(cx).positions().clone();
        let document = GraphDocument::collect_from_store(store, &positions);
        let directory = working_directory();
        let receiver = cx.prompt_for_new_path(&directory, Some("compograph-graph.json"));
        let task = cx.spawn(async move |weak, async_cx| match receiver.await {
            Ok(Ok(Some(path))) => {
                let note =
                    file_io::export_json_to_path(&document, &path).unwrap_or_else(|error| error);
                weak.update(&mut *async_cx, |this, cx| {
                    this.io_message = note;
                    cx.notify();
                })
                .ok();
            }
            Ok(Ok(None)) => {
                weak.update(&mut *async_cx, |this, cx| {
                    this.io_message = "export cancelled".to_string();
                    cx.notify();
                })
                .ok();
            }
            _ => {
                let note = match file_io::export_json_file(&document, file_io::JSON_PATH) {
                    Ok(note) => format!("{note} (picker unavailable)"),
                    Err(note) => note,
                };
                weak.update(&mut *async_cx, |this, cx| {
                    this.io_message = note;
                    cx.notify();
                })
                .ok();
            }
        });
        self.io_task = Some(task);
        cx.notify();
    }

    fn import_json(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        let task = cx.spawn(async move |weak, async_cx| {
            let picked = match receiver.await {
                Ok(Ok(paths)) => paths.and_then(|mut paths| paths.pop()),
                _ => {
                    weak.update(&mut *async_cx, |this, cx| {
                        match file_io::import_json_file(file_io::JSON_PATH) {
                            Ok(document) => {
                                let count = document.nodes.len();
                                this.apply_document(&document, cx);
                                this.io_message = format!(
                                    "imported {count} nodes from {} (picker unavailable)",
                                    file_io::JSON_PATH
                                );
                            }
                            Err(note) => this.io_message = note,
                        }
                        cx.notify();
                    })
                    .ok();
                    return;
                }
            };
            // Distinguish cancellation (dialog answered with no path) from a
            // platform failure (handled above as the scratch fallback).
            let Some(path) = picked else {
                weak.update(&mut *async_cx, |this, cx| {
                    this.io_message = "import cancelled".to_string();
                    cx.notify();
                })
                .ok();
                return;
            };
            let loaded = async_cx
                .background_executor()
                .spawn(async move {
                    file_io::import_json_from_path(&path).map(|document| (document, path))
                })
                .await;
            weak.update(&mut *async_cx, |this, cx| {
                match loaded {
                    Ok((document, path)) => {
                        let count = document.nodes.len();
                        this.apply_document(&document, cx);
                        this.io_message = format!("imported {count} nodes from {}", path.display());
                    }
                    Err(note) => this.io_message = note,
                }
                cx.notify();
            })
            .ok();
        });
        self.io_task = Some(task);
        cx.notify();
    }

    fn import_dot(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
        });
        let task = cx.spawn(async move |weak, async_cx| {
            let picked = match receiver.await {
                Ok(Ok(paths)) => paths.and_then(|mut paths| paths.pop()),
                _ => {
                    weak.update(&mut *async_cx, |this, cx| {
                        match file_io::import_dot_file(file_io::DOT_PATH) {
                            Ok(document) => {
                                let count = document.nodes.len();
                                this.apply_document(&document, cx);
                                this.io_message = format!(
                                    "imported {count} nodes from {} (picker unavailable)",
                                    file_io::DOT_PATH
                                );
                            }
                            Err(note) => this.io_message = note,
                        }
                        cx.notify();
                    })
                    .ok();
                    return;
                }
            };
            // Distinguish cancellation (dialog answered with no path) from a
            // platform failure (handled above as the scratch fallback).
            let Some(path) = picked else {
                weak.update(&mut *async_cx, |this, cx| {
                    this.io_message = "import cancelled".to_string();
                    cx.notify();
                })
                .ok();
                return;
            };
            let loaded = async_cx
                .background_executor()
                .spawn(async move {
                    file_io::import_dot_from_path(&path).map(|document| (document, path))
                })
                .await;
            weak.update(&mut *async_cx, |this, cx| {
                match loaded {
                    Ok((document, path)) => {
                        let count = document.nodes.len();
                        this.apply_document(&document, cx);
                        this.io_message = format!("imported {count} nodes from {}", path.display());
                    }
                    Err(note) => this.io_message = note,
                }
                cx.notify();
            })
            .ok();
        });
        self.io_task = Some(task);
        cx.notify();
    }

    fn export_dot(&mut self, cx: &mut Context<Self>) {
        let store = self.store.read(cx);
        let encoded = export_dot(store.graph());
        let count = store.node_count();
        let directory = working_directory();
        let receiver = cx.prompt_for_new_path(&directory, Some("compograph-graph.dot"));
        let task = cx.spawn(async move |weak, async_cx| match receiver.await {
            Ok(Ok(Some(path))) => {
                let note = match file_io::write_text_to_path(&encoded, &path) {
                    Ok(()) => format!("exported dot with {count} nodes to {}", path.display()),
                    Err(note) => note,
                };
                weak.update(&mut *async_cx, |this, cx| {
                    this.io_message = note;
                    cx.notify();
                })
                .ok();
            }
            Ok(Ok(None)) => {
                weak.update(&mut *async_cx, |this, cx| {
                    this.io_message = "export cancelled".to_string();
                    cx.notify();
                })
                .ok();
            }
            _ => {
                weak.update(&mut *async_cx, |this, cx| {
                    match file_io::write_text_file(&encoded, file_io::DOT_PATH) {
                        Ok(()) => {
                            this.io_message = format!(
                                "exported dot with {count} nodes to {} (picker unavailable)",
                                file_io::DOT_PATH
                            );
                        }
                        Err(note) => this.io_message = note,
                    }
                    cx.notify();
                })
                .ok();
            }
        });
        self.io_task = Some(task);
        cx.notify();
    }

    /// Rebuilds the store from a document and restores its positions.
    ///
    /// Position restore is deferred past the effect flush, so the layout
    /// reactions queued by the structural edits run first and cannot
    /// overwrite the imported coordinates.
    fn apply_document(&mut self, document: &GraphDocument, cx: &mut Context<Self>) {
        let mut sorted: Vec<&NodeEntry> = document.nodes.iter().collect();
        sorted.sort_by_key(|entry| entry.id);
        let mut order: Vec<NodeIndex> = Vec::new();
        self.store.update(cx, |graph, cx| {
            graph.clear(cx);
            for entry in &sorted {
                order.push(graph.add_node(cx, entry.label.clone()));
            }
            let mut by_id: HashMap<usize, NodeIndex> = HashMap::new();
            for (entry, node) in sorted.iter().zip(order.iter().copied()) {
                by_id.insert(entry.id, node);
            }
            for (entry, node) in sorted.iter().zip(order.iter().copied()) {
                for (key, value) in &entry.attrs {
                    graph.set_node_attr(cx, node, key.clone(), value.clone());
                }
                for class in &entry.classes {
                    graph.add_node_class(cx, node, class.clone());
                }
            }
            for edge in &document.edges {
                if let (Some(source), Some(target)) =
                    (by_id.get(&edge.source), by_id.get(&edge.target))
                {
                    let id = graph.add_edge(cx, *source, *target, edge.weight);
                    for (key, value) in &edge.attrs {
                        graph.set_edge_attr(cx, id, key.clone(), value.clone());
                    }
                    for class in &edge.classes {
                        graph.add_edge_class(cx, id, class.clone());
                    }
                }
            }
            for (entry, node) in sorted.iter().zip(order.iter().copied()) {
                if let Some(parent_id) = entry.parent {
                    if let Some(parent) = by_id.get(&parent_id).copied() {
                        graph.set_parent(cx, node, Some(parent)).ok();
                    }
                }
            }
            for (entry, node) in sorted.iter().zip(order.iter().copied()) {
                if entry.collapsed {
                    graph.set_collapsed(cx, node, true);
                }
            }
        });
        let positions = remap_positions(document, &order);
        let layout = self.layout.clone();
        cx.defer(move |cx: &mut App| {
            layout.update(cx, |driver, cx| {
                driver.replace_positions(positions, cx);
            });
        });
        self.clear_selection();
        self.clear_algo_highlights();
    }

    fn export_image(&mut self, scope: ExportScope, cx: &mut Context<Self>) {
        let store = self.store.read(cx);
        let view: &dyn GraphView = store;
        let mut node_ids: Vec<NodeIndex> = view.node_ids();
        node_ids.sort_unstable_by_key(|node| node.index());
        let mut pairs = view.edges();
        pairs.sort_unstable_by_key(|(source, target)| (source.index(), target.index()));
        let mut node_styles = HashMap::new();
        let mut edge_styles = HashMap::new();
        for node in &node_ids {
            let label = store
                .node_data(*node)
                .map(|data| data.label.clone())
                .unwrap_or_default();
            node_styles.insert(
                *node,
                self.bypass.resolve_node(
                    &self.sheet,
                    &self.mapper,
                    *node,
                    Some(label.as_str()),
                    view.degree(*node),
                ),
            );
        }
        for (source, target) in &pairs {
            edge_styles.insert(
                (*source, *target),
                self.bypass
                    .resolve_edge(&self.sheet, &self.edge_mapper, *source, *target),
            );
        }
        let positions = self.layout.read(cx).positions().clone();
        let snapshot = ExportSnapshot {
            node_ids,
            pairs,
            positions,
            node_styles,
            edge_styles,
            camera: self.camera,
            viewport: self.viewport,
            aggregate: self.aggregate,
        };
        let request = ExportRequest {
            scope,
            scale: EXPORT_SCALE,
            viewport: self.viewport,
        };
        let default_name = match scope {
            ExportScope::Viewport => "compograph-viewport.png",
            ExportScope::FullGraph => "compograph-full.png",
        };
        let receiver = cx.prompt_for_new_path(&working_directory(), Some(default_name));
        let task = cx.spawn(async move |weak, async_cx| {
            let picked = match receiver.await {
                Ok(Ok(Some(path))) => Some(path),
                Ok(Ok(None)) => {
                    weak.update(&mut *async_cx, |this, cx| {
                        this.export_message = "export cancelled".to_string();
                        cx.notify();
                    })
                    .ok();
                    return;
                }
                _ => None,
            };
            let note = async_cx
                .background_executor()
                .spawn(async move {
                    let Some((pixels, width, height)) = export_pixels(scope, request, &snapshot)
                    else {
                        return "nothing to export".to_string();
                    };
                    let encoded = encode_png(width, height, &pixels);
                    match picked {
                        Some(path) => match file_io::write_bytes_to_path(&encoded, &path) {
                            Ok(()) => format!("exported {width}x{height} to {}", path.display()),
                            Err(note) => note,
                        },
                        None => {
                            let fallback = match scope {
                                ExportScope::Viewport => "/tmp/compograph-viewport.png",
                                ExportScope::FullGraph => "/tmp/compograph-full.png",
                            };
                            match file_io::write_bytes_file(&encoded, fallback) {
                                Ok(()) => format!(
                                    "exported {width}x{height} to {fallback} (picker unavailable)"
                                ),
                                Err(note) => note,
                            }
                        }
                    }
                })
                .await;
            weak.update(&mut *async_cx, |this, cx| {
                this.export_message = note;
                cx.notify();
            })
            .ok();
        });
        self.io_task = Some(task);
        cx.notify();
    }

    /// Plans currently held by the retained cache.
    fn cached_plans(&self) -> (Vec<PaintedNode>, Vec<PaintedEdge>, Vec<PaintedArrow>) {
        (
            self.retained.nodes().to_vec(),
            self.retained.edges().to_vec(),
            self.retained.arrows().to_vec(),
        )
    }

    fn cycle_start(&mut self, cx: &mut Context<Self>) {
        let count = self.ordered_nodes(cx).len().max(1);
        self.algo_start = (self.algo_start + 1) % count;
        cx.notify();
    }

    fn cycle_goal(&mut self, cx: &mut Context<Self>) {
        let count = self.ordered_nodes(cx).len().max(1);
        self.algo_goal = (self.algo_goal + 1) % count;
        cx.notify();
    }

    /// Points one endpoint slot at the single selected node.
    ///
    /// Algorithms run against the live graph, so the slot stores the node's
    /// ordinal in the current order rather than the identifier itself.
    fn set_endpoint_from_selection(&mut self, start: bool, cx: &mut Context<Self>) {
        let ids = self.ordered_nodes(cx);
        let mut selected = self.selection.iter();
        let note = match (selected.next(), selected.next()) {
            (Some(node), None) => match endpoint_slot(&ids, node) {
                Some(slot) => {
                    if start {
                        self.algo_start = slot;
                    } else {
                        self.algo_goal = slot;
                    }
                    format!(
                        "{} set to {}",
                        if start { "start" } else { "goal" },
                        self.endpoint_label(slot, cx)
                    )
                }
                None => "selected node left the graph".to_string(),
            },
            _ => "select exactly one node first".to_string(),
        };
        self.algo_summary = note;
        cx.notify();
    }

    fn shift_damping(&mut self, delta: f32, cx: &mut Context<Self>) {
        self.damping = (self.damping + delta).clamp(0.0, 1.0);
        cx.notify();
    }

    fn clear_highlights(&mut self, cx: &mut Context<Self>) {
        self.clear_algo_highlights();
        self.algo_summary = "highlights cleared".to_string();
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
        let positions_version = self.layout.read(cx).positions_version();
        let index_started = Instant::now();
        self.refresh_spatial(positions, positions_version);
        let index_ms = index_started.elapsed().as_secs_f64() * 1000.0;
        let mut labels: Vec<(NodeIndex, String, usize)> = Vec::new();
        for node in view.node_ids() {
            let label = store
                .node_data(node)
                .map(|data| data.label.clone())
                .unwrap_or_default();
            labels.push((node, label, view.degree(node)));
        }
        let sheet = &self.sheet;
        let mapper = &self.mapper;
        let edge_mapper = &self.edge_mapper;
        let bypass = &self.bypass;
        let style_of = |node: NodeIndex| {
            let entry = labels
                .iter()
                .find(|(id, _, _)| *id == node)
                .map(|(_, label, degree)| (label.as_str(), *degree));
            let (label, degree) = entry.unwrap_or(("", 0));
            bypass.resolve_node(sheet, mapper, node, Some(label), degree)
        };
        let world_rect = world_viewport_rect(&self.camera, self.viewport);
        // First pass with squares for a provisional level; the minimal level
        // paints squares, so its visible set is the square set. Other levels
        // paint true shapes and refine the set with per-node shapes.
        let square_ids = visible_node_ids(
            positions,
            &self.spatial,
            world_rect,
            NODE_HALF_EXTENT,
            |_| NodeShape::Square,
        );
        let provisional = self
            .lod_params
            .select(self.camera.zoom, square_ids.len(), self.lod);
        let visible_ids = if provisional == DetailLevel::Minimal {
            square_ids
        } else {
            let shapes: HashMap<NodeIndex, NodeShape> = labels
                .iter()
                .map(|(node, label, degree)| {
                    (
                        *node,
                        bypass
                            .resolve_node(sheet, mapper, *node, Some(label.as_str()), *degree)
                            .shape,
                    )
                })
                .collect();
            visible_node_ids(
                positions,
                &self.spatial,
                world_rect,
                NODE_HALF_EXTENT,
                |node| shapes.get(&node).copied().unwrap_or_default(),
            )
        };
        self.lod = self
            .lod_params
            .select(self.camera.zoom, visible_ids.len(), provisional);
        let lod = self.lod;
        let edge_options = EdgePaintOptions {
            level: lod,
            aggregate_threshold: cg_render::EDGE_AGGREGATION_THRESHOLD,
            force_haystack: self.aggregate,
            ortho: None,
            taxi: None,
        };
        let versions = CacheVersions {
            structure: self.structure_version,
            positions: positions_version,
            style: self.style_version,
        };
        let mut pairs = view.edges();
        pairs.sort_unstable_by_key(|(source, target)| (source.index(), target.index()));
        let camera = self.camera;
        let viewport = self.viewport;
        let plan_started = Instant::now();
        let cached_hit = self
            .retained
            .is_reusable(versions, edge_options, &camera, viewport);
        let partial_hit = !cached_hit
            && self.retained.refresh_moved(
                RefreshInput {
                    pairs: &pairs,
                    positions,
                    camera: &camera,
                    viewport,
                    options: edge_options,
                    versions,
                },
                style_of,
                |source, target| bypass.resolve_edge(sheet, edge_mapper, source, target),
            );
        let (nodes, edges, arrows) = if cached_hit || partial_hit {
            self.cached_plans()
        } else {
            let nodes =
                paint_nodes_for_level(&visible_ids, positions, &camera, viewport, lod, style_of);
            let edges = paint_edges_for(
                &pairs,
                positions,
                &camera,
                viewport,
                edge_options,
                |source, target| bypass.resolve_edge(sheet, edge_mapper, source, target),
            );
            let arrows = paint_arrows_for_level(&edges, lod);
            if self.retained.enabled() {
                let ordinals = edge_ordinals_for(&edges, &pairs);
                self.retained.store(
                    StoredPlans {
                        nodes: nodes.clone(),
                        node_order: visible_ids.clone(),
                        edges: edges.clone(),
                        edge_ordinals: ordinals,
                        arrows: arrows.clone(),
                    },
                    versions,
                    edge_options,
                    &camera,
                    viewport,
                );
            }
            (nodes, edges, arrows)
        };
        // Labels are planned from the visible node plan every frame rather than
        // cached: their cache key would need a text-shaping dimension the
        // retained geometry cache does not track, and shaping is cheap relative
        // to the geometry rebuilds it would key against.
        let label_of = |node: NodeIndex| {
            labels
                .iter()
                .find(|(id, _, _)| *id == node)
                .map(|(_, text, _)| text.clone())
        };
        let label_size_of = |node: NodeIndex| style_of(node).label_size;
        let painted_labels = paint_labels_for(&nodes, lod, label_of, label_size_of);
        let painted_edge_labels = paint_edge_labels_for(
            &edges,
            lod,
            |source, target| {
                store
                    .edge_weight(source, target)
                    .map(|weight| weight.to_string())
            },
            |source, target| {
                bypass
                    .resolve_edge(sheet, edge_mapper, source, target)
                    .label_size
            },
        );
        let plan_ms = plan_started.elapsed().as_secs_f64() * 1000.0;
        self.metrics.push(FrameSample {
            counts: PlanCounts {
                nodes: nodes.len(),
                edges: edges.len(),
                arrows: arrows.len(),
            },
            plan_ms,
            index_ms,
            visible_nodes: visible_ids.len(),
        });
        let rubber_band: Option<PaintedRubberBand> =
            self.rubber.rect().map(|rect| PaintedRubberBand {
                origin: rect.origin,
                size: rect.size,
            });
        let engine_name = self.layout.read(cx).engine_name().to_string();
        let last_ms = self.layout.read(cx).last_refine_ms();
        let node_count = view.node_count();
        let edge_count = view.edge_count();
        let zoom = self.camera.zoom;
        let selected_count = self.selection.len();
        let hover_text = self.hover_label(cx);
        let hover_anchor = self.hover_anchor;
        let hovering =
            self.hovered.is_some() && !self.rubber.is_active() && lod != DetailLevel::Minimal;
        let layouts = self.layouts.clone();
        let menu_open = self.menu_open;
        let current_layout = engine_name.clone();
        let start_label = self.endpoint_label(self.algo_start, cx);
        let goal_label = self.endpoint_label(self.algo_goal, cx);
        let damping = self.damping;
        let algo_busy = self.algo_busy;
        let algo_summary = self.algo_summary.clone();
        let io_message = self.io_message.clone();
        let progress = self.layout.read(cx).progress();
        let frame_ms = self.metrics.average_plan_ms();
        let index_ms = self.metrics.average_index_ms();
        let visible = self.metrics.latest_visible();
        let lod_label = match lod {
            DetailLevel::Full => "full",
            DetailLevel::Simplified => "simplified",
            DetailLevel::Minimal => "minimal",
        };
        let aggregate_label = if self.aggregate {
            "aggregate:on"
        } else {
            "aggregate:off"
        };
        let retained_label = if self.retained.enabled() {
            "retained:on"
        } else {
            "retained:off"
        };
        let mode_label = match self.select_mode {
            SelectMode::Single => "select:single",
            SelectMode::Additive => "select:additive",
        };
        let lock_drag_label = if self.locks.lock_drag {
            "lock-drag:on"
        } else {
            "lock-drag:off"
        };
        let no_grab_label = if self.locks.no_grab {
            "no-grab:on"
        } else {
            "no-grab:off"
        };
        let no_deselect_label = if self.locks.no_deselect {
            "no-deselect:on"
        } else {
            "no-deselect:off"
        };
        let neighbor_label = format!("neighbors:{}", self.neighbor_hops);
        let export_message = self.export_message.clone();
        let view = cx.entity();
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .child(
                        div()
                            .id("layout-menu-toggle")
                            .child(format!("layout: {engine_name}"))
                            .on_click(cx.listener(|this, _event: &ClickEvent, _window, cx| {
                                this.menu_open = !this.menu_open;
                                cx.notify();
                            })),
                    )
                    .when(menu_open, |bar| {
                        let mut bar = bar;
                        for (ordinal, name) in layouts.iter().enumerate() {
                            let picked: &'static str = name;
                            let label = if picked == current_layout.as_str() {
                                format!("*{picked}")
                            } else {
                                picked.to_string()
                            };
                            bar = bar.child(
                                div()
                                    .id(("layout-pick", ordinal))
                                    .px_2()
                                    .py_1()
                                    .child(label)
                                    .on_click(cx.listener(move |this, _event: &ClickEvent, _window, cx| {
                                        this.switch_layout(picked, cx);
                                    })),
                            );
                        }
                        bar
                    })
                    .child(
                        div()
                            .id("io-export-json")
                            .px_2()
                            .child("export json")
                            .on_click(cx.listener(|this, _event: &ClickEvent, _window, cx| {
                                this.export_json(cx);
                            })),
                    )
                    .child(
                        div()
                            .id("io-import-json")
                            .px_2()
                            .child("import json")
                            .on_click(cx.listener(|this, _event: &ClickEvent, _window, cx| {
                                this.import_json(cx);
                            })),
                    )
                    .child(
                        div()
                            .id("io-export-dot")
                            .px_2()
                            .child("export dot")
                            .on_click(cx.listener(|this, _event: &ClickEvent, _window, cx| {
                                this.export_dot(cx);
                            })),
                    )
                    .child(
                        div()
                            .id("io-import-dot")
                            .px_2()
                            .child("import dot")
                            .on_click(cx.listener(|this, _event: &ClickEvent, _window, cx| {
                                this.import_dot(cx);
                            })),
                    )
                    .child(
                        div()
                            .id("view-aggregate-toggle")
                            .px_2()
                            .child(aggregate_label)
                            .on_click(cx.listener(|this, _event: &ClickEvent, _window, cx| {
                                this.aggregate = !this.aggregate;
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .id("view-retained-toggle")
                            .px_2()
                            .child(retained_label)
                            .on_click(cx.listener(|this, _event: &ClickEvent, _window, cx| {
                                this.retained.set_enabled(!this.retained.enabled());
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .id("select-mode-toggle")
                            .px_2()
                            .child(mode_label)
                            .on_click(cx.listener(|this, _event: &ClickEvent, _window, cx| {
                                this.select_mode = match this.select_mode {
                                    SelectMode::Single => SelectMode::Additive,
                                    SelectMode::Additive => SelectMode::Single,
                                };
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .id("lock-drag-toggle")
                            .px_2()
                            .child(lock_drag_label)
                            .on_click(cx.listener(|this, _event: &ClickEvent, _window, cx| {
                                this.locks.lock_drag = !this.locks.lock_drag;
                                this.drag.end();
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .id("no-grab-toggle")
                            .px_2()
                            .child(no_grab_label)
                            .on_click(cx.listener(|this, _event: &ClickEvent, _window, cx| {
                                this.locks.no_grab = !this.locks.no_grab;
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .id("no-deselect-toggle")
                            .px_2()
                            .child(no_deselect_label)
                            .on_click(cx.listener(|this, _event: &ClickEvent, _window, cx| {
                                this.locks.no_deselect = !this.locks.no_deselect;
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .id("neighbor-hops-cycle")
                            .px_2()
                            .child(neighbor_label)
                            .on_click(cx.listener(|this, _event: &ClickEvent, _window, cx| {
                                this.neighbor_hops = (this.neighbor_hops + 1) % 4;
                                let store = this.store.read(cx);
                                let view: &dyn GraphView = store;
                                let (fringe, neighbor_edges) =
                                    GraphWindow::derive_neighborhood(
                                        view,
                                        &this.selection,
                                        &this.selected_edges,
                                        this.neighbor_hops,
                                    );
                                this.neighbor_nodes = fringe;
                                this.neighbor_edges = neighbor_edges;
                                this.rebuild_bypass();
                                cx.notify();
                            })),
                    )
                    .child(div().px_2().child(format!("lod:{lod_label}")))
                    .child(
                        div()
                            .id("io-export-viewport")
                            .px_2()
                            .child("export view")
                            .on_click(cx.listener(|this, _event: &ClickEvent, _window, cx| {
                                this.export_image(ExportScope::Viewport, cx);
                            })),
                    )
                    .child(
                        div()
                            .id("io-export-full")
                            .px_2()
                            .child("export full")
                            .on_click(cx.listener(|this, _event: &ClickEvent, _window, cx| {
                                this.export_image(ExportScope::FullGraph, cx);
                            })),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .flex()
                    .flex_row()
                    .child(
                        div()
                            .flex_1()
                            .relative()
                            .track_focus(&self.focus)
                            .child(graph_view(
                                nodes,
                                edges,
                                arrows,
                                painted_labels,
                                painted_edge_labels,
                                rubber_band,
                            ))
                            .when(hovering && hover_text.is_some(), |canvas| {
                                canvas.child(
                                    div()
                                        .absolute()
                                        .left(px(hover_anchor.x + 12.0))
                                        .top(px(hover_anchor.y + 12.0))
                                        .px_2()
                                        .py_1()
                                        .child(hover_text.unwrap_or_default()),
                                )
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, event: &MouseDownEvent, _window, cx| {
                                    let viewport_point = Self::viewport_point(event.position);
                                    let additive = event.modifiers.shift;
                                    let world = this
                                        .camera
                                        .viewport_to_world(this.viewport, viewport_point);
                                    let positions =
                                        this.layout.read(cx).positions().clone();
                                    let version = this.layout.read(cx).positions_version();
                                    this.refresh_spatial(&positions, version);
                                    let locks = this.locks;
                                    let mode = this.select_mode;
                                    let hit = if can_grab_node(&locks) {
                                        press_hit_shaped(
                                            world,
                                            &positions,
                                            &this.spatial,
                                            this.hit_radius(),
                                            |node| this.node_shape(cx, node),
                                        )
                                    } else {
                                        None
                                    };
                                    match hit {
                                        Some((node, offset)) => {
                                            if can_begin_drag(&locks) {
                                                this.drag.begin(node, offset);
                                            }
                                            apply_point_select(
                                                &mut this.selection,
                                                mode,
                                                node,
                                                additive,
                                            );
                                            this.rubber.cancel();
                                            let store = this.store.read(cx);
                                            let view: &dyn GraphView = store;
                                            let (fringe, neighbor_edges) =
                                                GraphWindow::derive_neighborhood(
                                                    view,
                                                    &this.selection,
                                                    &this.selected_edges,
                                                    this.neighbor_hops,
                                                );
                                            this.neighbor_nodes = fringe;
                                            this.neighbor_edges = neighbor_edges;
                                            this.rebuild_bypass();
                                        }
                                        None => {
                                            if should_clear_on_blank(&locks, additive) {
                                                this.clear_selection();
                                            }
                                            this.rubber.begin(viewport_point);
                                        }
                                    }
                                    cx.notify();
                                }),
                            )
                            .on_mouse_move(cx.listener(
                                |this, event: &MouseMoveEvent, _window, cx| {
                                    let viewport_point = Self::viewport_point(event.position);
                                    if let Some(node) = this.drag.active_node() {
                                        if !can_begin_drag(&this.locks) {
                                            this.drag.end();
                                            cx.notify();
                                            return;
                                        }
                                        let offset = this
                                            .drag
                                            .active
                                            .as_ref()
                                            .map(|gesture| gesture.grab_offset)
                                            .unwrap_or_default();
                                        let world = this
                                            .camera
                                            .viewport_to_world(this.viewport, viewport_point);
                                        let target = drag_position(world, offset);
                                        let moved = this.layout.update(cx, |driver, cx| {
                                            driver.move_pinned(node, target, cx)
                                        });
                                        if moved {
                                            this.retained.mark_moved([node]);
                                        }
                                        cx.notify();
                                    } else if this.rubber.is_active() {
                                        this.rubber.update(viewport_point);
                                        cx.notify();
                                    } else {
                                        let world = this
                                            .camera
                                            .viewport_to_world(this.viewport, viewport_point);
                                        let positions =
                                            this.layout.read(cx).positions().clone();
                                        let version = this.layout.read(cx).positions_version();
                                        this.refresh_spatial(&positions, version);
                                        let hovered = hover_node_shaped(
                                            world,
                                            &positions,
                                            &this.spatial,
                                            this.hit_radius(),
                                            |node| this.node_shape(cx, node),
                                        );
                                        if hovered != this.hovered {
                                            this.hovered = hovered;
                                            this.hover_anchor = viewport_point;
                                            this.rebuild_bypass();
                                            cx.notify();
                                        } else if hovered.is_some() {
                                            this.hover_anchor = viewport_point;
                                            cx.notify();
                                        }
                                    }
                                },
                            ))
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|this, event: &MouseUpEvent, _window, cx| {
                                    let dragged = this.drag.end().is_some();
                                    if dragged {
                                        this.rubber.cancel();
                                        let store = this.store.clone();
                                        this.layout.update(cx, |driver, cx| {
                                            driver.request_refine(&store, cx);
                                        });
                                    } else if let Some(rect) = this.rubber.rect() {
                                        this.rubber.cancel();
                                        this.finish_rubber(rect, event.modifiers.shift, cx);
                                    }
                                    cx.notify();
                                }),
                            )
                            .on_scroll_wheel(cx.listener(
                                |this, event: &ScrollWheelEvent, _window, cx| {
                                    let anchor = Self::viewport_point(event.position);
                                    let lines = match &event.delta {
                                        ScrollDelta::Pixels(pixels) => {
                                            f32::from(pixels.y) / 16.0
                                        }
                                        ScrollDelta::Lines(lines) => lines.y,
                                    };
                                    this.camera.zoom_at(
                                        this.viewport,
                                        anchor,
                                        wheel_zoom_factor(lines),
                                    );
                                    cx.notify();
                                },
                            ))
                            .on_key_down(
                                move |event: &KeyDownEvent, _window: &mut Window, cx: &mut App| {
                                    if is_dismiss_key(event.keystroke.key.as_str()) {
                                        view.update(cx, |this, cx| {
                                            this.clear_selection();
                                            cx.notify();
                                        });
                                    }
                                },
                            ),
                    )
                    .child(
                        div()
                            .w(px(240.0))
                            .flex()
                            .flex_col()
                            .gap_1()
                            .px_2()
                            .py_1()
                            .child("algorithms")
                            .child(
                                div()
                                    .id(("algo-run", 0usize))
                                    .child("shortest path")
                                    .on_click(cx.listener(
                                        |this, _event: &ClickEvent, _window, cx| {
                                            this.run_shortest_path(cx);
                                        },
                                    )),
                            )
                            .child(
                                div()
                                    .id(("algo-run", 1usize))
                                    .child("guided search")
                                    .on_click(cx.listener(
                                        |this, _event: &ClickEvent, _window, cx| {
                                            this.run_heuristic_path(cx);
                                        },
                                    )),
                            )
                            .child(
                                div()
                                    .id(("algo-run", 2usize))
                                    .child("components")
                                    .on_click(cx.listener(
                                        |this, _event: &ClickEvent, _window, cx| {
                                            this.run_components(cx);
                                        },
                                    )),
                            )
                            .child(
                                div()
                                    .id(("algo-run", 3usize))
                                    .child("pagerank")
                                    .on_click(cx.listener(
                                        |this, _event: &ClickEvent, _window, cx| {
                                            this.run_pagerank(cx);
                                        },
                                    )),
                            )
                            .child(
                                div()
                                    .id(("algo-run", 4usize))
                                    .child("spanning tree")
                                    .on_click(cx.listener(
                                        |this, _event: &ClickEvent, _window, cx| {
                                            this.run_spanning_forest(cx);
                                        },
                                    )),
                            )
                            .child(
                                div()
                                    .id(("algo-run", 5usize))
                                    .child("degree")
                                    .on_click(cx.listener(
                                        |this, _event: &ClickEvent, _window, cx| {
                                            this.run_degree(cx);
                                        },
                                    )),
                            )
                            .child(
                                div()
                                    .id(("algo-run", 6usize))
                                    .child("cuts")
                                    .on_click(cx.listener(
                                        |this, _event: &ClickEvent, _window, cx| {
                                            this.run_cuts(cx);
                                        },
                                    )),
                            )
                            .child(
                                div()
                                    .id(("algo-run", 7usize))
                                    .child("all pairs")
                                    .on_click(cx.listener(
                                        |this, _event: &ClickEvent, _window, cx| {
                                            this.run_all_pairs(cx);
                                        },
                                    )),
                            )
                            .child(
                                div()
                                    .id(("algo-run", 8usize))
                                    .child("bellman-ford")
                                    .on_click(cx.listener(
                                        |this, _event: &ClickEvent, _window, cx| {
                                            this.run_bellman_ford(cx);
                                        },
                                    )),
                            )
                            .child(
                                div()
                                    .id(("algo-run", 9usize))
                                    .child("traversals")
                                    .on_click(cx.listener(
                                        |this, _event: &ClickEvent, _window, cx| {
                                            this.run_traversals(cx);
                                        },
                                    )),
                            )
                            .child(
                                div()
                                    .id(("algo-run", 10usize))
                                    .child("topo+reduction")
                                    .on_click(cx.listener(
                                        |this, _event: &ClickEvent, _window, cx| {
                                            this.run_topo_reduction(cx);
                                        },
                                    )),
                            )
                            .child(
                                div()
                                    .id(("algo-run", 11usize))
                                    .child("closeness")
                                    .on_click(cx.listener(
                                        |this, _event: &ClickEvent, _window, cx| {
                                            this.run_closeness(cx);
                                        },
                                    )),
                            )
                            .child(
                                div()
                                    .id(("algo-run", 12usize))
                                    .child("betweenness")
                                    .on_click(cx.listener(
                                        |this, _event: &ClickEvent, _window, cx| {
                                            this.run_betweenness(cx);
                                        },
                                    )),
                            )
                            .child(
                                div()
                                    .id(("algo-run", 13usize))
                                    .child("mst single")
                                    .on_click(cx.listener(
                                        |this, _event: &ClickEvent, _window, cx| {
                                            this.run_mst_single(cx);
                                        },
                                    )),
                            )
                            .child(
                                div()
                                    .id(("algo-run", 14usize))
                                    .child("dominators")
                                    .on_click(cx.listener(
                                        |this, _event: &ClickEvent, _window, cx| {
                                            this.run_dominators(cx);
                                        },
                                    )),
                            )
                            .child(
                                div()
                                    .id(("algo-run", 15usize))
                                    .child("euler directed")
                                    .on_click(cx.listener(
                                        |this, _event: &ClickEvent, _window, cx| {
                                            this.run_euler_directed(cx);
                                        },
                                    )),
                            )
                            .child(
                                div()
                                    .id(("algo-run", 16usize))
                                    .child("euler undirected")
                                    .on_click(cx.listener(
                                        |this, _event: &ClickEvent, _window, cx| {
                                            this.run_euler_undirected(cx);
                                        },
                                    )),
                            )
                            .child(
                                div()
                                    .id(("algo-run", 17usize))
                                    .child("min cut")
                                    .on_click(cx.listener(
                                        |this, _event: &ClickEvent, _window, cx| {
                                            this.run_min_cut(cx);
                                        },
                                    )),
                            )
                            .child(
                                div()
                                    .id(("algo-run", 18usize))
                                    .child("hierarchical")
                                    .on_click(cx.listener(
                                        |this, _event: &ClickEvent, _window, cx| {
                                            this.run_hierarchical(cx);
                                        },
                                    )),
                            )
                            .child(
                                div()
                                    .id(("algo-run", 19usize))
                                    .child("markov")
                                    .on_click(cx.listener(
                                        |this, _event: &ClickEvent, _window, cx| {
                                            this.run_markov(cx);
                                        },
                                    )),
                            )
                            .child(
                                div()
                                    .id(("algo-run", 20usize))
                                    .child("k-means")
                                    .on_click(cx.listener(
                                        |this, _event: &ClickEvent, _window, cx| {
                                            this.run_kmeans(cx);
                                        },
                                    )),
                            )
                            .child(
                                div()
                                    .id(("algo-run", 21usize))
                                    .child("affinity")
                                    .on_click(cx.listener(
                                        |this, _event: &ClickEvent, _window, cx| {
                                            this.run_affinity(cx);
                                        },
                                    )),
                            )
                            .child(
                                div()
                                    .id(("algo-run", 22usize))
                                    .child("metric")
                                    .on_click(cx.listener(
                                        |this, _event: &ClickEvent, _window, cx| {
                                            this.run_metric_clusters(cx);
                                        },
                                    )),
                            )
                            .child(format!("from: {start_label}"))
                            .child(
                                div()
                                    .id("algo-start-next")
                                    .child("next start")
                                    .on_click(cx.listener(
                                        |this, _event: &ClickEvent, _window, cx| {
                                            this.cycle_start(cx);
                                        },
                                    )),
                            )
                            .child(format!("to: {goal_label}"))
                            .child(
                                div()
                                    .id("algo-goal-next")
                                    .child("next goal")
                                    .on_click(cx.listener(
                                        |this, _event: &ClickEvent, _window, cx| {
                                            this.cycle_goal(cx);
                                        },
                                    )),
                            )
                            .child(
                                div()
                                    .id("algo-start-from-selection")
                                    .child("selection as start")
                                    .on_click(cx.listener(
                                        |this, _event: &ClickEvent, _window, cx| {
                                            this.set_endpoint_from_selection(true, cx);
                                        },
                                    )),
                            )
                            .child(
                                div()
                                    .id("algo-goal-from-selection")
                                    .child("selection as goal")
                                    .on_click(cx.listener(
                                        |this, _event: &ClickEvent, _window, cx| {
                                            this.set_endpoint_from_selection(false, cx);
                                        },
                                    )),
                            )
                            .child(format!("damping: {damping:.2}"))
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .gap_2()
                                    .child(
                                        div()
                                            .id("algo-damp-down")
                                            .child("-")
                                            .on_click(cx.listener(
                                                |this, _event: &ClickEvent, _window, cx| {
                                                    this.shift_damping(-DAMPING_STEP, cx);
                                                },
                                            )),
                                    )
                                    .child(
                                        div()
                                            .id("algo-damp-up")
                                            .child("+")
                                            .on_click(cx.listener(
                                                |this, _event: &ClickEvent, _window, cx| {
                                                    this.shift_damping(DAMPING_STEP, cx);
                                                },
                                            )),
                                    ),
                            )
                            .child(
                                div()
                                    .id("algo-clear")
                                    .child("clear highlights")
                                    .on_click(cx.listener(
                                        |this, _event: &ClickEvent, _window, cx| {
                                            this.clear_highlights(cx);
                                        },
                                    )),
                            )
                            .child(if algo_busy {
                                "working...".to_string()
                            } else {
                                algo_summary
                            }),
                    ),
            )
            .child(
                div().px_2().py_1().child(format!(
                    "nodes: {node_count} edges: {edge_count} visible: {visible} zoom: {zoom:.2} lod:{lod_label} frame:{frame_ms:.1}ms index:{index_ms:.1}ms layout: {engine_name} {last_ms:.1}ms chunks:{} gen:{} selected: {selected_count} {io_message} {export_message}",
                    progress.chunks_written,
                    progress.generation,
                )),
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
        match opened {
            Ok(window) => {
                if window
                    .update(cx, |view, window, cx| {
                        window.focus(&view.focus, cx);
                    })
                    .is_err()
                {
                    eprintln!("failed to focus graph window");
                }
            }
            Err(error) => {
                eprintln!("failed to open graph window: {error}");
            }
        }
        cx.activate(true);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::TestAppContext;

    #[test]
    fn only_escape_dismisses_the_selection() {
        assert!(is_dismiss_key("escape"));
        assert!(!is_dismiss_key("Enter"));
        assert!(!is_dismiss_key(""));
    }

    #[test]
    fn endpoint_slot_resolves_ordinals_against_the_live_order() {
        let ids = vec![NodeIndex::new(0), NodeIndex::new(1), NodeIndex::new(2)];
        assert_eq!(endpoint_slot(&ids, NodeIndex::new(1)), Some(1));
        assert_eq!(endpoint_slot(&ids, NodeIndex::new(9)), None);
    }

    #[gpui::test]
    fn stale_algo_write_back_is_dropped(cx: &mut TestAppContext) {
        let view = cx.update(|cx: &mut App| cx.new(GraphWindow::new));
        cx.update(|cx| {
            view.update(cx, |this, cx| {
                this.run_shortest_path(cx);
                this.run_components(cx);
            })
        });
        cx.update(|cx| {
            view.update(cx, |this, cx| {
                assert_eq!(this.algo_generation, 2);
                assert!(this.algo_busy);
                let stale = AlgoOutcome {
                    summary: "stale".to_string(),
                    ..AlgoOutcome::default()
                };
                assert!(!this.commit_outcome(1, stale));
                assert_ne!(this.algo_summary, "stale");
                let current = AlgoOutcome {
                    nodes: vec![(NodeIndex::new(0), NodeStylePatch::selected())],
                    summary: "current".to_string(),
                    ..AlgoOutcome::default()
                };
                assert!(this.commit_outcome(2, current));
                assert_eq!(this.algo_summary, "current");
                assert!(!this.algo_busy);
                assert!(this.bypass.node_bypass(NodeIndex::new(0)).is_some());
                cx.notify();
            })
        });
    }

    #[gpui::test]
    fn selection_and_algo_highlights_share_the_bypass(cx: &mut TestAppContext) {
        let view = cx.update(|cx: &mut App| cx.new(GraphWindow::new));
        cx.update(|cx| {
            view.update(cx, |this, cx| {
                this.selection.select(NodeIndex::new(0));
                let outcome = AlgoOutcome {
                    nodes: vec![(NodeIndex::new(1), NodeStylePatch::selected())],
                    ..AlgoOutcome::default()
                };
                this.algo_generation = 1;
                assert!(this.commit_outcome(1, outcome));
                assert!(this.bypass.node_bypass(NodeIndex::new(0)).is_some());
                assert!(this.bypass.node_bypass(NodeIndex::new(1)).is_some());
                this.clear_selection();
                assert!(this.bypass.node_bypass(NodeIndex::new(0)).is_none());
                assert!(this.bypass.node_bypass(NodeIndex::new(1)).is_some());
                cx.notify();
            })
        });
    }

    #[gpui::test]
    fn json_import_restores_positions_after_driver_reactions(cx: &mut TestAppContext) {
        use cg_types::Point2;
        let view = cx.update(|cx: &mut App| cx.new(GraphWindow::new));
        let document = GraphDocument {
            nodes: vec![
                NodeEntry {
                    id: 0,
                    label: "a".to_string(),
                    position: Some([11.0, 22.0]),
                    ..NodeEntry::default()
                },
                NodeEntry {
                    id: 1,
                    label: "b".to_string(),
                    position: Some([33.0, 44.0]),
                    ..NodeEntry::default()
                },
            ],
            edges: vec![cg_graph::EdgeEntry {
                source: 0,
                target: 1,
                weight: 1.0,
                ..cg_graph::EdgeEntry::default()
            }],
        };
        cx.update(|cx| {
            view.update(cx, |this, cx| {
                this.apply_document(&document, cx);
            })
        });
        cx.update(|cx| {
            view.update(cx, |this, cx| {
                assert_eq!(this.store.read(cx).node_count(), 2);
                let positions = this.layout.read(cx).positions().clone();
                assert_eq!(positions.len(), 2);
                let mut points: Vec<Point2> = positions.values().copied().collect();
                points.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));
                assert_eq!(points[0], Point2::new(11.0, 22.0));
                assert_eq!(points[1], Point2::new(33.0, 44.0));
            })
        });
    }
}
