//! Selection, neighborhood, bypass, and spatial index maintenance.
//!
//! This module owns the pointer-selection state: clearing, box selection,
//! neighborhood derivation, bypass rebuilds, and the spatial index the hit
//! testing shares with the canvas. It never paints or talks to dialogs.

use std::collections::BTreeSet;

use cg_graph::{DataValue, GraphStore, GraphView, NodeIndex, Positions};
use cg_interact::{
    NODE_HALF_EXTENT, SelectionState, edges_in_rect, expand_neighborhood, neighborhood_edges,
    nodes_in_rect,
};
use cg_render::{DetailLevel, EdgeStylePatch, NODE_SIDE, NodeStylePatch, label_envelope};
use cg_types::{Point2, Rect};
use gpui::{App, Context};

use crate::app_state::{GraphWindow, TAP_THRESHOLD};

impl GraphWindow {
    /// Drops the node and edge selection and aborts any box select.
    ///
    /// Algorithm highlights survive: they live in their own maps and are
    /// merged back by [`GraphWindow::rebuild_bypass`]. Derived neighborhood
    /// entries clear with the seeds, so no separate cleanup path exists.
    pub(crate) fn clear_selection(&mut self) {
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
    pub(crate) fn derive_neighborhood(
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
    pub(crate) fn rebuild_bypass(&mut self) {
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
    pub(crate) fn clear_algo_highlights(&mut self) {
        self.algo_nodes.clear();
        self.algo_edges.clear();
        self.rebuild_bypass();
    }

    /// Rebuilds the spatial index only after position write-backs.
    ///
    /// Viewport motion reuses the cached cells; structural edits and layout
    /// write-backs move the positions version and trigger a rebuild. The
    /// adaptive cell follows the node extent.
    pub(crate) fn refresh_spatial(&mut self, positions: &Positions, version: u64) {
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
    pub(crate) fn finish_rubber(&mut self, viewport_rect: Rect, additive: bool, cx: &mut Context<Self>) {
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
        let store = self.store.read(cx);
        let visible_nodes = nodes_in_rect(
            &positions,
            &self.spatial,
            model_rect,
            NODE_HALF_EXTENT,
            |node| self.node_shape(cx, node),
        )
        .into_iter()
        .filter(|node| store.is_visible(*node))
        .collect::<Vec<_>>();
        let mut nodes = visible_nodes;
        let view: &dyn GraphView = store;
        let mut edges = edges_in_rect(view, &positions, model_rect, NODE_SIDE);
        if self.lod != DetailLevel::Minimal {
            let camera = self.camera;
            let viewport = self.viewport;
            for node in view.node_ids() {
                let Some(center) = positions.get(&node) else {
                    continue;
                };
                let label = store
                    .node_data(node)
                    .map(|data| data.label.clone())
                    .unwrap_or_default();
                if label.trim().is_empty() {
                    continue;
                }
                let attrs = store.node_attrs(node);
                let classes = store.node_classes(node);
                let style = self.bypass.resolve_node_with_data(
                    &self.sheet,
                    &self.mapper,
                    node,
                    Some(label.as_str()),
                    view.degree(node),
                    cg_render::NodeDataTables {
                        attrs: &attrs,
                        classes: &classes,
                    },
                );
                let screen = camera.world_to_viewport(viewport, *center);
                let side = (cg_render::NODE_SIDE * style.scale).max(4.0);
                let origin = Point2::new(screen.x, screen.y + side / 2.0 + cg_render::LABEL_GAP);
                let envelope = label_envelope(
                    origin,
                    &label,
                    style.label_size,
                    0.0,
                    cg_render::LabelBackground::None,
                );
                if envelope.intersects(viewport_rect) && !nodes.contains(&node) {
                    nodes.push(node);
                }
            }
            for (source, target) in view.edges() {
                let (Some(a), Some(b)) = (positions.get(&source), positions.get(&target)) else {
                    continue;
                };
                let text = store
                    .edge_weight(source, target)
                    .map(|weight| weight.to_string())
                    .unwrap_or_default();
                if text.trim().is_empty() {
                    continue;
                }
                let (attrs, classes) = Self::edge_attrs_classes(store, source, target);
                let size = self
                    .bypass
                    .resolve_edge_with_data(
                        &self.sheet,
                        &self.edge_mapper,
                        source,
                        target,
                        &attrs,
                        &classes,
                    )
                    .label_size;
                let start = camera.world_to_viewport(viewport, *a);
                let end = camera.world_to_viewport(viewport, *b);
                let anchor = Point2::new(
                    (start.x + end.x) / 2.0,
                    (start.y + end.y) / 2.0 + cg_render::EDGE_LABEL_GAP,
                );
                let envelope =
                    label_envelope(anchor, &text, size, 0.0, cg_render::LabelBackground::None);
                if envelope.intersects(viewport_rect) && !edges.contains(&(source, target)) {
                    edges.push((source, target));
                }
            }
            nodes.sort_unstable_by_key(|node| node.index());
            edges.sort_unstable_by_key(|(source, target)| (source.index(), target.index()));
        }
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

    pub(crate) fn hover_label(&self, cx: &App) -> Option<String> {
        let node = self.hovered?;
        self.store
            .read(cx)
            .node_data(node)
            .map(|data| data.label.clone())
    }

    pub(crate) fn node_shape(&self, cx: &App, node: NodeIndex) -> cg_render::NodeShape {
        let store = self.store.read(cx);
        let label = store
            .node_data(node)
            .map(|data| data.label.clone())
            .unwrap_or_default();
        let view: &dyn GraphView = store;
        let attrs = store.node_attrs(node);
        let classes = store.node_classes(node);
        self.bypass
            .resolve_node_with_data(
                &self.sheet,
                &self.mapper,
                node,
                Some(label.as_str()),
                view.degree(node),
                cg_render::NodeDataTables {
                    attrs: &attrs,
                    classes: &classes,
                },
            )
            .shape
    }

    pub(crate) fn edge_attrs_classes(
        store: &GraphStore,
        source: NodeIndex,
        target: NodeIndex,
    ) -> (
        std::collections::HashMap<String, DataValue>,
        std::collections::BTreeSet<String>,
    ) {
        match store.find_edge(source, target) {
            Some(edge) => (store.edge_attrs(edge), store.edge_classes(edge)),
            None => (
                std::collections::HashMap::new(),
                std::collections::BTreeSet::new(),
            ),
        }
    }
}
