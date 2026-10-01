//! Bottom status bar: counts, timings, and messages.
//!
//! This module builds the status line only. It reads counters and messages
//! and formats them; it never mutates state.

use cg_render::DetailLevel;
use gpui::{Context, IntoElement, ParentElement, Styled, div};

use crate::app_state::GraphWindow;

impl GraphWindow {
    pub(crate) fn status_view(&self, cx: &Context<Self>) -> impl IntoElement {
        let store = self.store.read(cx);
        let node_count = store.node_count();
        let edge_count = store.edge_count();
        let zoom = self.camera.zoom;
        let selected_count = self.selection.len();
        let engine_name = self.layout.read(cx).engine_name().to_string();
        let last_ms = self.layout.read(cx).last_refine_ms();
        let progress = self.layout.read(cx).progress();
        let frame_ms = self.metrics.average_plan_ms();
        let index_ms = self.metrics.average_index_ms();
        let visible = self.metrics.latest_visible();
        let lod_label = match self.lod {
            DetailLevel::Full => "full",
            DetailLevel::Simplified => "simplified",
            DetailLevel::Minimal => "minimal",
        };
        let io_message = self.io_message.clone();
        let export_message = self.export_message.clone();
        div().px_2().py_1().child(format!(
            "nodes: {node_count} edges: {edge_count} visible: {visible} zoom: {zoom:.2} lod:{lod_label} frame:{frame_ms:.1}ms index:{index_ms:.1}ms layout: {engine_name} {last_ms:.1}ms chunks:{} gen:{} selected: {selected_count} {io_message} {export_message}",
            progress.chunks_written,
            progress.generation,
        ))
    }
}
