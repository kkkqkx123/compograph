//! Panel parameter adjustments for algorithm runs.
//!
//! This module owns the endpoint slots and the numeric panel parameters.
//! Every adjustment clamps to its legal range and notifies, so the sidebar
//! can never push the runs out of bounds.

use cg_graph::ClusterMetric;
use gpui::Context;

use crate::app_state::{
    CLUSTER_K_MAX, CLUSTER_THRESHOLD_MAX, GraphWindow, endpoint_slot,
};

impl GraphWindow {
    pub(crate) fn cycle_start(&mut self, cx: &mut Context<Self>) {
        let count = self.ordered_nodes(cx).len().max(1);
        self.algo_start = (self.algo_start + 1) % count;
        cx.notify();
    }

    pub(crate) fn cycle_goal(&mut self, cx: &mut Context<Self>) {
        let count = self.ordered_nodes(cx).len().max(1);
        self.algo_goal = (self.algo_goal + 1) % count;
        cx.notify();
    }

    /// Points one endpoint slot at the single selected node.
    ///
    /// Algorithms run against the live graph, so the slot stores the node's
    /// ordinal in the current order rather than the identifier itself.
    pub(crate) fn set_endpoint_from_selection(&mut self, start: bool, cx: &mut Context<Self>) {
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

    pub(crate) fn shift_damping(&mut self, delta: f32, cx: &mut Context<Self>) {
        self.damping = (self.damping + delta).clamp(0.0, 1.0);
        cx.notify();
    }

    pub(crate) fn shift_cluster_k(&mut self, delta: i32, cx: &mut Context<Self>) {
        let next = (self.cluster_k as i32 + delta).clamp(1, CLUSTER_K_MAX as i32);
        self.cluster_k = next as usize;
        cx.notify();
    }

    pub(crate) fn shift_cluster_threshold(&mut self, delta: f32, cx: &mut Context<Self>) {
        self.cluster_threshold = (self.cluster_threshold + delta).clamp(0.0, CLUSTER_THRESHOLD_MAX);
        cx.notify();
    }

    pub(crate) fn cycle_cluster_metric(&mut self, cx: &mut Context<Self>) {
        self.cluster_metric = match self.cluster_metric {
            ClusterMetric::Euclidean => ClusterMetric::Manhattan,
            ClusterMetric::Manhattan => ClusterMetric::Chebyshev,
            ClusterMetric::Chebyshev => ClusterMetric::Euclidean,
        };
        cx.notify();
    }

    pub(crate) fn clear_highlights(&mut self, cx: &mut Context<Self>) {
        self.clear_algo_highlights();
        self.algo_summary = "highlights cleared".to_string();
        cx.notify();
    }
}
