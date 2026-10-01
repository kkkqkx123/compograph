//! Synthetic import merging a transfer document into the live store.
//!
//! Documents validate before anything mutates, so illegal input never leaves
//! a half-applied graph. Matching identifiers update in place; anything else
//! falls back to a full replace, which the report flags explicitly. Every
//! path runs inside one batch, so subscribers observe a single commit.

use std::collections::{BTreeSet, HashMap};

use petgraph::stable_graph::NodeIndex;

use crate::attrs::valid_attr_key;
use crate::classes::valid_class_name;
use crate::io::{GraphDocument, IoError};
use crate::positions::Positions;
use crate::store::GraphStore;

/// Outcome of merging a document into the store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SyncReport {
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
    pub replaced: bool,
}

/// Merges `document` into `store`, keeping `positions` aligned.
///
/// Identifiers that match on both sides update in place; when the identifier
/// or edge sets differ, the store clears and rebuilds instead. Both paths
/// broadcast exactly once through the store batching.
pub fn sync_document(
    store: &mut GraphStore,
    cx: &mut gpui::Context<GraphStore>,
    document: &GraphDocument,
    positions: &mut Positions,
) -> Result<SyncReport, IoError> {
    document.validate()?;
    let mut current: Vec<usize> = store.node_ids().map(|node| node.index()).collect();
    current.sort_unstable();
    let mut wanted: Vec<usize> = document.nodes.iter().map(|entry| entry.id).collect();
    wanted.sort_unstable();
    if current == wanted && edge_sets_match(store, document) {
        Ok(update_in_place(store, cx, document, positions))
    } else {
        Ok(replace_all(store, cx, document, positions))
    }
}

fn edge_sets_match(store: &GraphStore, document: &GraphDocument) -> bool {
    let mut current: Vec<(usize, usize)> = store
        .graph()
        .edge_indices()
        .filter_map(|edge| store.graph().edge_endpoints(edge))
        .map(|(source, target)| (source.index(), target.index()))
        .collect();
    current.sort_unstable();
    let mut wanted: Vec<(usize, usize)> = document
        .edges
        .iter()
        .map(|entry| (entry.source, entry.target))
        .collect();
    wanted.sort_unstable();
    current == wanted
}

fn update_in_place(
    store: &mut GraphStore,
    cx: &mut gpui::Context<GraphStore>,
    document: &GraphDocument,
    positions: &mut Positions,
) -> SyncReport {
    store.begin_batch();
    let mut updated = 0usize;
    for entry in &document.nodes {
        let node = NodeIndex::new(entry.id);
        store.set_node_label(cx, node, entry.label.clone());
        sync_node_tables(store, cx, node, entry);
        if let Some(parent) = entry.parent {
            let _ = store.set_parent(cx, node, Some(NodeIndex::new(parent)));
        } else {
            let _ = store.set_parent(cx, node, None);
        }
        store.set_collapsed(cx, node, entry.collapsed);
        if let Some(position) = entry.position {
            positions.insert(
                node,
                cg_types::Point2::new(position[0], position[1]),
            );
        }
        updated += 1;
    }
    let mut current: Vec<(usize, usize, petgraph::stable_graph::EdgeIndex)> = store
        .graph()
        .edge_indices()
        .filter_map(|edge| {
            store
                .graph()
                .edge_endpoints(edge)
                .map(|(source, target)| (source.index(), target.index(), edge))
        })
        .collect();
    current.sort_unstable();
    let mut wanted: Vec<&crate::io::EdgeEntry> = document.edges.iter().collect();
    wanted.sort_by_key(|entry| (entry.source, entry.target));
    for ((_, _, edge), entry) in current.into_iter().zip(wanted) {
        store.set_edge_weight(cx, edge, entry.weight);
        sync_edge_tables(store, cx, edge, entry);
    }
    let wanted_ids: BTreeSet<usize> = document.nodes.iter().map(|entry| entry.id).collect();
    positions.retain(|node, _| wanted_ids.contains(&node.index()));
    store.end_batch(cx);
    SyncReport {
        added: 0,
        updated,
        removed: 0,
        replaced: false,
    }
}

fn replace_all(
    store: &mut GraphStore,
    cx: &mut gpui::Context<GraphStore>,
    document: &GraphDocument,
    positions: &mut Positions,
) -> SyncReport {
    let removed = store.node_count();
    store.begin_batch();
    store.clear(cx);
    let mut order: Vec<&crate::io::NodeEntry> = document.nodes.iter().collect();
    order.sort_by_key(|entry| entry.id);
    let mut id_map: HashMap<usize, NodeIndex> = HashMap::new();
    for entry in &order {
        let node = store.add_node(cx, entry.label.clone());
        id_map.insert(entry.id, node);
        sync_node_tables(store, cx, node, entry);
    }
    for entry in &order {
        let Some(node) = id_map.get(&entry.id).copied() else {
            continue;
        };
        if let Some(parent) = entry.parent
            && let Some(parent_node) = id_map.get(&parent).copied()
        {
            let _ = store.set_parent(cx, node, Some(parent_node));
        }
    }
    for entry in &order {
        if entry.collapsed
            && let Some(node) = id_map.get(&entry.id).copied()
        {
            store.set_collapsed(cx, node, true);
        }
    }
    let mut edges = document.edges.clone();
    edges.sort_by_key(|entry| (entry.source, entry.target));
    for entry in &edges {
        let (Some(source), Some(target)) = (
            id_map.get(&entry.source).copied(),
            id_map.get(&entry.target).copied(),
        ) else {
            continue;
        };
        let edge = store.add_edge(cx, source, target, entry.weight);
        sync_edge_tables(store, cx, edge, entry);
    }
    let mut fresh = Positions::new();
    for entry in &order {
        if let (Some(position), Some(node)) =
            (entry.position, id_map.get(&entry.id).copied())
        {
            fresh.insert(node, cg_types::Point2::new(position[0], position[1]));
        }
    }
    *positions = fresh;
    store.end_batch(cx);
    SyncReport {
        added: order.len(),
        updated: 0,
        removed,
        replaced: true,
    }
}

fn sync_node_tables(
    store: &mut GraphStore,
    cx: &mut gpui::Context<GraphStore>,
    node: NodeIndex,
    entry: &crate::io::NodeEntry,
) {
    let stale: Vec<String> = store
        .node_attrs(node)
        .keys()
        .filter(|key| !entry.attrs.contains_key(*key))
        .cloned()
        .collect();
    for key in stale {
        store.remove_node_attr(cx, node, &key);
    }
    for (key, value) in &entry.attrs {
        if valid_attr_key(key) {
            store.set_node_attr(cx, node, key.clone(), value.clone());
        }
    }
    let stale_classes: Vec<String> = store
        .node_classes(node)
        .into_iter()
        .filter(|name| !entry.classes.contains(name))
        .collect();
    for name in stale_classes {
        store.remove_node_class(cx, node, &name);
    }
    for name in &entry.classes {
        if valid_class_name(name) {
            store.add_node_class(cx, node, name.clone());
        }
    }
}

fn sync_edge_tables(
    store: &mut GraphStore,
    cx: &mut gpui::Context<GraphStore>,
    edge: petgraph::stable_graph::EdgeIndex,
    entry: &crate::io::EdgeEntry,
) {
    let stale: Vec<String> = store
        .edge_attrs(edge)
        .keys()
        .filter(|key| !entry.attrs.contains_key(*key))
        .cloned()
        .collect();
    for key in stale {
        store.remove_edge_attr(cx, edge, &key);
    }
    for (key, value) in &entry.attrs {
        if valid_attr_key(key) {
            store.set_edge_attr(cx, edge, key.clone(), value.clone());
        }
    }
    let stale_classes: Vec<String> = store
        .edge_classes(edge)
        .into_iter()
        .filter(|name| !entry.classes.contains(name))
        .collect();
    for name in stale_classes {
        store.remove_edge_class(cx, edge, &name);
    }
    for name in &entry.classes {
        if valid_class_name(name) {
            store.add_edge_class(cx, edge, name.clone());
        }
    }
}
