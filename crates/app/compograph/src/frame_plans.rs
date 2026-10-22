//! Per-frame paint planning without any widgets.
//!
//! This module turns the current store, positions, and cache versions into
//! owned paint plans. It holds no callbacks; the view modules consume the
//! returned parts.

use std::collections::HashMap;
use std::time::Instant;

use compograph_graph::{GraphStore, GraphView, NodeIndex, Positions};
use compograph_interact::NODE_HALF_EXTENT;
use compograph_render::{
    CacheVersions, DetailLevel, EdgePaintOptions, FrameSample, NodeDataTables, NodeShape,
    PaintedArrow, PaintedContainer, PaintedEdge, PaintedEdgeLabel, PaintedLabel, PaintedNode,
    PaintedRubberBand, PlanCounts, RefreshInput, StoredPlans, all_compound_bounds,
    clip_painted_edge, edge_ordinals_for, graph_bounds, paint_arrows_for_level,
    paint_edge_labels_for, paint_edges_for_with_waypoints, paint_labels_for, paint_nodes_for_level,
    visible_node_ids, world_viewport_rect,
};
use compograph_types::{Point2, Rect, Vec2};
use gpui::{Context, Window};

use crate::app_state::GraphWindow;

/// Owned paint plans one frame hands to the canvas.
pub(crate) struct FrameParts {
    pub(crate) containers: Vec<PaintedContainer>,
    pub(crate) nodes: Vec<PaintedNode>,
    pub(crate) edges: Vec<PaintedEdge>,
    pub(crate) arrows: Vec<PaintedArrow>,
    pub(crate) labels: Vec<PaintedLabel>,
    pub(crate) edge_labels: Vec<PaintedEdgeLabel>,
    pub(crate) rubber_band: Option<PaintedRubberBand>,
}

/// Structure-derived data reused across frames until the structure changes.
///
/// Node labels, node degrees and the visible edge list only depend on the
/// structure, so the frame path reads them from here instead of rebuilding
/// them per frame. The degrees come from the edge list in one pass, which
/// keeps the cost linear even when the graph carries containers.
pub(crate) struct StructureCache {
    /// Structure generation the data was captured at.
    version: u64,
    /// Visible edges, ordered by their endpoints.
    pub(crate) pairs: Vec<(NodeIndex, NodeIndex)>,
    /// Node label and degree, ordered by node index.
    pub(crate) labels: Vec<(NodeIndex, String, usize)>,
}

impl StructureCache {
    /// Captures the labels, degrees and edge list of the current structure.
    fn capture(store: &GraphStore, version: u64) -> Self {
        let view: &dyn GraphView = store;
        let mut pairs = view.edges();
        pairs.sort_unstable_by_key(|(source, target)| (source.index(), target.index()));

        let mut degrees: HashMap<NodeIndex, usize> = HashMap::new();
        for (source, target) in &pairs {
            *degrees.entry(*source).or_insert(0) += 1;
            *degrees.entry(*target).or_insert(0) += 1;
            if source == target {
                // A self loop touches its node once, not twice.
                *degrees.entry(*source).or_insert(0) -= 1;
            }
        }

        let mut labels: Vec<(NodeIndex, String, usize)> = view
            .node_ids()
            .into_iter()
            .map(|node| {
                let label = store
                    .node_data(node)
                    .map(|data| data.label.clone())
                    .unwrap_or_default();
                let degree = degrees.get(&node).copied().unwrap_or(0);
                (node, label, degree)
            })
            .collect();
        labels.sort_unstable_by_key(|(node, _, _)| node.index());

        Self {
            version,
            pairs,
            labels,
        }
    }
}

/// Looks up the label and degree of one node.
///
/// The labels are ordered by node index, so the lookup is a binary search
/// instead of a scan over every node.
fn label_entry(
    labels: &[(NodeIndex, String, usize)],
    node: NodeIndex,
) -> Option<&(NodeIndex, String, usize)> {
    labels
        .binary_search_by_key(&node.index(), |(id, _, _)| id.index())
        .ok()
        .map(|index| &labels[index])
}

/// Margin kept clear around the fitted graph, in logical pixels.
const FIT_VIEWPORT_PADDING: f32 = 32.0;

impl GraphWindow {
    /// Plans currently held by the retained cache.
    pub(crate) fn cached_plans(&self) -> (Vec<PaintedNode>, Vec<PaintedEdge>, Vec<PaintedArrow>) {
        (
            self.retained.nodes().to_vec(),
            self.retained.edges().to_vec(),
            self.retained.arrows().to_vec(),
        )
    }

    /// Frames `positions` inside the viewport with a fixed margin.
    ///
    /// Empty position sets leave the camera untouched, so clearing the graph
    /// never collapses the view.
    pub(crate) fn fit_camera_to_positions(&mut self, positions: &Positions) {
        let Some(bounds) = graph_bounds(positions, 0.0) else {
            return;
        };
        self.camera
            .fit_to_bounds(bounds, self.viewport, FIT_VIEWPORT_PADDING);
    }

    /// Steps the layout transition and plans the visible geometry.
    ///
    /// The retained cache is consulted first: a full hit reuses its plans,
    /// a moved-only refresh reuses the rest, otherwise the visible set is
    /// repainted from scratch. Metrics are pushed here so every frame,
    /// cached or not, reports one sample.
    pub(crate) fn prepare_frame(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> FrameParts {
        let size = window.viewport_size();
        self.viewport = Vec2::new(f32::from(size.width), f32::from(size.height));
        let store_entity = self.store.clone();
        let still_running = self.layout.update(cx, |driver, cx| {
            if driver.has_transition() {
                driver.step_transition(&store_entity, cx)
            } else {
                false
            }
        });
        if still_running {
            cx.notify();
        }
        // Consumes a one-shot fit armed by an explicit action such as a
        // layout switch: only the transition edge fires, and an active drag
        // or box select keeps the user's viewport. Routine refinements and
        // background write-backs never reach the camera through this path.
        let just_settled = self.was_settling && !still_running;
        self.was_settling = still_running;
        if self.fit_on_settle && just_settled {
            self.fit_on_settle = false;
            let fit = self
                .layout
                .update(cx, |driver, _cx| driver.take_fit_request());
            if fit && self.drag.active_node().is_none() && !self.rubber.is_active() {
                let current = self.layout.read(cx).positions().clone();
                self.fit_camera_to_positions(&current);
            }
        }
        let store = self.store.read(cx);
        let positions: &Positions = self.layout.read(cx).positions();
        let positions_version = self.layout.read(cx).positions_version();
        let index_started = Instant::now();
        self.refresh_spatial(positions, positions_version);
        let index_ms = index_started.elapsed().as_secs_f64() * 1000.0;
        // Structure-derived data only changes with the structure, so the
        // labels, the degrees and the edge list are captured once per
        // structure instead of once per frame.
        let structure_version = self.structure_version;
        if self.structure_cache.as_ref().map(|cache| cache.version) != Some(structure_version) {
            self.structure_cache = Some(StructureCache::capture(store, structure_version));
        }
        let structure = self
            .structure_cache
            .as_ref()
            .expect("the structure cache is filled above");
        let labels = &structure.labels;
        let sheet = &self.sheet;
        let mapper = &self.mapper;
        let edge_mapper = &self.edge_mapper;
        let bypass = &self.bypass;
        let style_of = |node: NodeIndex| {
            let entry =
                label_entry(labels, node).map(|(_, label, degree)| (label.as_str(), *degree));
            let (label, degree) = entry.unwrap_or(("", 0));
            let attrs = store.node_attrs(node);
            let classes = store.node_classes(node);
            bypass.resolve_node_with_data(
                sheet,
                mapper,
                node,
                Some(label),
                degree,
                NodeDataTables {
                    attrs: &attrs,
                    classes: &classes,
                },
            )
        };
        let edge_style_of = |source: NodeIndex, target: NodeIndex| {
            let (attrs, classes) = Self::edge_attrs_classes(store, source, target);
            bypass.resolve_edge_with_data(sheet, edge_mapper, source, target, &attrs, &classes)
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
                    let attrs = store.node_attrs(*node);
                    let classes = store.node_classes(*node);
                    (
                        *node,
                        bypass
                            .resolve_node_with_data(
                                sheet,
                                mapper,
                                *node,
                                Some(label.as_str()),
                                *degree,
                                NodeDataTables {
                                    attrs: &attrs,
                                    classes: &classes,
                                },
                            )
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
            aggregate_threshold: compograph_render::EDGE_AGGREGATION_THRESHOLD,
            force_haystack: self.aggregate,
            ortho: None,
            taxi: None,
        };
        let versions = CacheVersions {
            structure: self.structure_version,
            positions: positions_version,
            style: self.style_version,
        };
        let pairs = &structure.pairs;
        let camera = self.camera;
        let viewport = self.viewport;
        let containers: Vec<PaintedContainer> = if store.has_compound() {
            all_compound_bounds(store, positions, NODE_HALF_EXTENT)
                .into_iter()
                .map(|(id, world_rect)| {
                    let far = Point2::new(
                        world_rect.origin.x + world_rect.size.x,
                        world_rect.origin.y + world_rect.size.y,
                    );
                    let screen_rect = Rect::from_corners(
                        camera.world_to_viewport(viewport, world_rect.origin),
                        camera.world_to_viewport(viewport, far),
                    );
                    let title = store
                        .node_data(id)
                        .map(|data| data.label.clone())
                        .unwrap_or_default();
                    PaintedContainer {
                        id,
                        rect: screen_rect,
                        title,
                    }
                })
                .collect()
        } else {
            Vec::new()
        };
        let plan_started = Instant::now();
        let cached_hit = self
            .retained
            .is_reusable(versions, edge_options, &camera, viewport);
        let partial_hit = !cached_hit
            && self.retained.refresh_moved(
                RefreshInput {
                    pairs,
                    positions,
                    camera: &camera,
                    viewport,
                    options: edge_options,
                    versions,
                    waypoints: &self.waypoints,
                },
                style_of,
                edge_style_of,
            );
        let (nodes, edges, arrows) = if cached_hit || partial_hit {
            self.cached_plans()
        } else {
            let nodes =
                paint_nodes_for_level(&visible_ids, positions, &camera, viewport, lod, style_of);
            let raw_edges = paint_edges_for_with_waypoints(
                pairs,
                positions,
                &camera,
                viewport,
                edge_options,
                &self.waypoints,
                edge_style_of,
            );
            let edges: Vec<PaintedEdge> = raw_edges
                .into_iter()
                .map(|edge| clip_painted_edge(&edge, &containers))
                .collect();
            let arrows = paint_arrows_for_level(&edges, lod);
            if self.retained.enabled() {
                let ordinals = edge_ordinals_for(&edges, pairs);
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
        for node in &nodes {
            if let Some(spec) = node.image.clone()
                && self.images.request(&spec)
                && !std::path::Path::new(&spec.path).exists()
            {
                self.images.fail(&spec.path, "missing file");
            }
        }
        // Labels are planned from the visible node plan every frame rather than
        // cached: their cache key would need a text-shaping dimension the
        // retained geometry cache does not track, and shaping is cheap relative
        // to the geometry rebuilds it would key against.
        let label_of = |node: NodeIndex| label_entry(labels, node).map(|(_, text, _)| text.clone());
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
            |source, target| edge_style_of(source, target).label_size,
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
        FrameParts {
            containers,
            nodes,
            edges,
            arrows,
            labels: painted_labels,
            edge_labels: painted_edge_labels,
            rubber_band,
        }
    }
}
