//! File and image actions behind the toolbar buttons.
//!
//! This module owns the dialog wiring: it resolves a path through the
//! platform picker with a scratch fallback, delegates the byte movement to
//! `file_io`, and applies loaded documents to the store. It never parses
//! formats itself.

use std::collections::HashMap;

use cg_graph::{GraphDocument, GraphView, NodeEntry, NodeIndex, export_dot, remap_positions};
use cg_render::{ExportRequest, ExportScope, ExportSnapshot, encode_png, export_pixels};
use gpui::{App, Context, PathPromptOptions};

use crate::app_state::{EXPORT_SCALE, GraphWindow, working_directory};
use crate::file_io;

impl GraphWindow {
    pub(crate) fn export_json(&mut self, cx: &mut Context<Self>) {
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

    pub(crate) fn import_json(&mut self, cx: &mut Context<Self>) {
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
                                let skipped = this.skipped_relations_note();
                                this.io_message = format!(
                                    "imported {count} nodes from {}{skipped} (picker unavailable)",
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
                        let skipped = this.skipped_relations_note();
                        this.io_message =
                            format!("imported {count} nodes from {}{skipped}", path.display());
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

    pub(crate) fn import_dot(&mut self, cx: &mut Context<Self>) {
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
                                let skipped = this.skipped_relations_note();
                                this.io_message = format!(
                                    "imported {count} nodes from {}{skipped} (picker unavailable)",
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
                        let skipped = this.skipped_relations_note();
                        this.io_message =
                            format!("imported {count} nodes from {}{skipped}", path.display());
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

    pub(crate) fn export_dot(&mut self, cx: &mut Context<Self>) {
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
    pub(crate) fn apply_document(&mut self, document: &GraphDocument, cx: &mut Context<Self>) {
        let mut sorted: Vec<&NodeEntry> = document.nodes.iter().collect();
        sorted.sort_by_key(|entry| entry.id);
        let mut order: Vec<NodeIndex> = Vec::new();
        let mut skipped_relations = 0usize;
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
                    match by_id.get(&parent_id).copied() {
                        Some(parent) => {
                            if graph.set_parent(cx, node, Some(parent)).is_err() {
                                skipped_relations += 1;
                            }
                        }
                        None => skipped_relations += 1,
                    }
                }
            }
            for (entry, node) in sorted.iter().zip(order.iter().copied()) {
                if entry.collapsed && !graph.set_collapsed(cx, node, true) {
                    skipped_relations += 1;
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
        if skipped_relations > 0 {
            self.io_message = format!("import skipped {skipped_relations} invalid relations");
        }
    }

    pub(crate) fn skipped_relations_note(&mut self) -> String {
        if self.io_message.starts_with("import skipped") {
            let note = format!("; {}", self.io_message);
            self.io_message.clear();
            note
        } else {
            String::new()
        }
    }

    pub(crate) fn export_image(&mut self, scope: ExportScope, cx: &mut Context<Self>) {
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
            let attrs = store.node_attrs(*node);
            let classes = store.node_classes(*node);
            node_styles.insert(
                *node,
                self.bypass.resolve_node_with_data(
                    &self.sheet,
                    &self.mapper,
                    *node,
                    Some(label.as_str()),
                    view.degree(*node),
                    &attrs,
                    &classes,
                ),
            );
        }
        for (source, target) in &pairs {
            let (attrs, classes) = Self::edge_attrs_classes(store, *source, *target);
            edge_styles.insert(
                (*source, *target),
                self.bypass.resolve_edge_with_data(
                    &self.sheet,
                    &self.edge_mapper,
                    *source,
                    *target,
                    &attrs,
                    &classes,
                ),
            );
        }
        let positions = self.layout.read(cx).positions().clone();
        let waypoints = self.waypoints.clone();
        let snapshot = ExportSnapshot {
            node_ids,
            pairs,
            positions,
            node_styles,
            edge_styles,
            camera: self.camera,
            viewport: self.viewport,
            aggregate: self.aggregate,
            waypoints,
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
}
