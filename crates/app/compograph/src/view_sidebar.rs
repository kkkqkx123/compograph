//! Algorithm sidebar: run buttons and panel parameters.
//!
//! This module builds the right-hand column only. Each entry delegates to an
//! algorithm dispatch or a parameter adjustment; the column itself holds no
//! graph logic.

use gpui::{
    ClickEvent, Context, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, div, px,
};

use crate::app_state::{
    CLUSTER_THRESHOLD_STEP, DAMPING_STEP, WEIGHTED_ALPHA_STEP, GraphWindow,
};

impl GraphWindow {
    pub(crate) fn sidebar_view(&self, cx: &Context<Self>) -> impl IntoElement {
        let start_label = self.endpoint_label(self.algo_start, cx);
        let goal_label = self.endpoint_label(self.algo_goal, cx);
        let damping = self.damping;
        let cluster_k = self.cluster_k;
        let cluster_metric = self.cluster_metric;
        let cluster_threshold = self.cluster_threshold;
        let cluster_metric_label = match cluster_metric {
            cg_graph::ClusterMetric::Euclidean => "euclidean",
            cg_graph::ClusterMetric::Manhattan => "manhattan",
            cg_graph::ClusterMetric::Chebyshev => "chebyshev",
        };
        let linkage = self.linkage;
        let linkage_label = match linkage {
            cg_graph::Linkage::Min => "min",
            cg_graph::Linkage::Max => "max",
            cg_graph::Linkage::Mean => "mean",
        };
        let weighted_alpha = self.weighted_alpha;
        let weighted_directed = self.weighted_directed;
        let path_k = self.path_k;
        let simple_limit = self.simple_limit;
        let algo_busy = self.algo_busy;
        let algo_summary = self.algo_summary.clone();
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
            .child(
                div()
                    .id(("algo-run", 23usize))
                    .child("bidirectional")
                    .on_click(cx.listener(
                        |this, _event: &ClickEvent, _window, cx| {
                            this.run_bidirectional(cx);
                        },
                    )),
            )
            .child(
                div()
                    .id(("algo-run", 24usize))
                    .child("spfa")
                    .on_click(cx.listener(
                        |this, _event: &ClickEvent, _window, cx| {
                            this.run_spfa(cx);
                        },
                    )),
            )
            .child(
                div()
                    .id(("algo-run", 25usize))
                    .child("johnson")
                    .on_click(cx.listener(
                        |this, _event: &ClickEvent, _window, cx| {
                            this.run_johnson(cx);
                        },
                    )),
            )
            .child(
                div()
                    .id(("algo-run", 26usize))
                    .child("kth shortest")
                    .on_click(cx.listener(
                        |this, _event: &ClickEvent, _window, cx| {
                            this.run_kth_shortest(cx);
                        },
                    )),
            )
            .child(
                div()
                    .id(("algo-run", 27usize))
                    .child("kosaraju")
                    .on_click(cx.listener(
                        |this, _event: &ClickEvent, _window, cx| {
                            this.run_kosaraju(cx);
                        },
                    )),
            )
            .child(
                div()
                    .id(("algo-run", 28usize))
                    .child("condensation")
                    .on_click(cx.listener(
                        |this, _event: &ClickEvent, _window, cx| {
                            this.run_condensation(cx);
                        },
                    )),
            )
            .child(
                div()
                    .id(("algo-run", 29usize))
                    .child("connectivity")
                    .on_click(cx.listener(
                        |this, _event: &ClickEvent, _window, cx| {
                            this.run_connectivity(cx);
                        },
                    )),
            )
            .child(
                div()
                    .id(("algo-run", 30usize))
                    .child("order walk")
                    .on_click(cx.listener(
                        |this, _event: &ClickEvent, _window, cx| {
                            this.run_order_walk(cx);
                        },
                    )),
            )
            .child(
                div()
                    .id(("algo-run", 31usize))
                    .child("k-medoids")
                    .on_click(cx.listener(
                        |this, _event: &ClickEvent, _window, cx| {
                            this.run_kmedoids(cx);
                        },
                    )),
            )
            .child(
                div()
                    .id(("algo-run", 32usize))
                    .child("fuzzy c-means")
                    .on_click(cx.listener(
                        |this, _event: &ClickEvent, _window, cx| {
                            this.run_fuzzy(cx);
                        },
                    )),
            )
            .child(
                div()
                    .id(("algo-run", 33usize))
                    .child("linkage")
                    .on_click(cx.listener(
                        |this, _event: &ClickEvent, _window, cx| {
                            this.run_linkage(cx);
                        },
                    )),
            )
            .child(
                div()
                    .id(("algo-run", 34usize))
                    .child("weighted degree")
                    .on_click(cx.listener(
                        |this, _event: &ClickEvent, _window, cx| {
                            this.run_weighted_degree(cx);
                        },
                    )),
            )
            .child(
                div()
                    .id(("algo-run", 35usize))
                    .child("max flow")
                    .on_click(cx.listener(
                        |this, _event: &ClickEvent, _window, cx| {
                            this.run_max_flow(cx);
                        },
                    )),
            )
            .child(
                div()
                    .id(("algo-run", 36usize))
                    .child("greedy matching")
                    .on_click(cx.listener(
                        |this, _event: &ClickEvent, _window, cx| {
                            this.run_greedy_matching(cx);
                        },
                    )),
            )
            .child(
                div()
                    .id(("algo-run", 37usize))
                    .child("maximum matching")
                    .on_click(cx.listener(
                        |this, _event: &ClickEvent, _window, cx| {
                            this.run_max_matching(cx);
                        },
                    )),
            )
            .child(
                div()
                    .id(("algo-run", 38usize))
                    .child("dsatur")
                    .on_click(cx.listener(
                        |this, _event: &ClickEvent, _window, cx| {
                            this.run_dsatur(cx);
                        },
                    )),
            )
            .child(
                div()
                    .id(("algo-run", 39usize))
                    .child("maximal cliques")
                    .on_click(cx.listener(
                        |this, _event: &ClickEvent, _window, cx| {
                            this.run_cliques(cx);
                        },
                    )),
            )
            .child(
                div()
                    .id(("algo-run", 40usize))
                    .child("feedback arcs")
                    .on_click(cx.listener(
                        |this, _event: &ClickEvent, _window, cx| {
                            this.run_feedback(cx);
                        },
                    )),
            )
            .child(
                div()
                    .id(("algo-run", 41usize))
                    .child("simple paths")
                    .on_click(cx.listener(
                        |this, _event: &ClickEvent, _window, cx| {
                            this.run_simple_paths(cx);
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
            .child(format!("clusters: k={cluster_k}"))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .child(
                        div()
                            .id("algo-k-down")
                            .child("-")
                            .on_click(cx.listener(
                                |this, _event: &ClickEvent, _window, cx| {
                                    this.shift_cluster_k(-1, cx);
                                },
                            )),
                    )
                    .child(
                        div()
                            .id("algo-k-up")
                            .child("+")
                            .on_click(cx.listener(
                                |this, _event: &ClickEvent, _window, cx| {
                                    this.shift_cluster_k(1, cx);
                                },
                            )),
                    ),
            )
            .child(format!(
                "metric: {cluster_metric_label} threshold: {cluster_threshold:.0}"
            ))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .child(
                        div()
                            .id("algo-metric-cycle")
                            .child("cycle metric")
                            .on_click(cx.listener(
                                |this, _event: &ClickEvent, _window, cx| {
                                    this.cycle_cluster_metric(cx);
                                },
                            )),
                    )
                    .child(
                        div()
                            .id("algo-threshold-down")
                            .child("-")
                            .on_click(cx.listener(
                                |this, _event: &ClickEvent, _window, cx| {
                                    this.shift_cluster_threshold(
                                        -CLUSTER_THRESHOLD_STEP,
                                        cx,
                                    );
                                },
                            )),
                    )
                    .child(
                        div()
                            .id("algo-threshold-up")
                            .child("+")
                            .on_click(cx.listener(
                                |this, _event: &ClickEvent, _window, cx| {
                                    this.shift_cluster_threshold(
                                        CLUSTER_THRESHOLD_STEP,
                                        cx,
                                    );
                                },
                            )),
                    ),
            )
            .child(format!("linkage: {linkage_label}"))
            .child(
                div()
                    .id("algo-linkage-cycle")
                    .child("cycle linkage")
                    .on_click(cx.listener(
                        |this, _event: &ClickEvent, _window, cx| {
                            this.cycle_linkage(cx);
                        },
                    )),
            )
            .child(format!(
                "weighted: a={weighted_alpha:.2} {}",
                if weighted_directed {
                    "directed"
                } else {
                    "undirected"
                }
            ))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .child(
                        div()
                            .id("algo-alpha-down")
                            .child("-")
                            .on_click(cx.listener(
                                |this, _event: &ClickEvent, _window, cx| {
                                    this.shift_weighted_alpha(-WEIGHTED_ALPHA_STEP, cx);
                                },
                            )),
                    )
                    .child(
                        div()
                            .id("algo-alpha-up")
                            .child("+")
                            .on_click(cx.listener(
                                |this, _event: &ClickEvent, _window, cx| {
                                    this.shift_weighted_alpha(WEIGHTED_ALPHA_STEP, cx);
                                },
                            )),
                    )
                    .child(
                        div()
                            .id("algo-directed-toggle")
                            .child("toggle directed")
                            .on_click(cx.listener(
                                |this, _event: &ClickEvent, _window, cx| {
                                    this.toggle_weighted_directed(cx);
                                },
                            )),
                    ),
            )
            .child(format!("path k: {path_k} limit: {simple_limit}"))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap_2()
                    .child(
                        div()
                            .id("algo-path-k-down")
                            .child("k-")
                            .on_click(cx.listener(
                                |this, _event: &ClickEvent, _window, cx| {
                                    this.shift_path_k(-1, cx);
                                },
                            )),
                    )
                    .child(
                        div()
                            .id("algo-path-k-up")
                            .child("k+")
                            .on_click(cx.listener(
                                |this, _event: &ClickEvent, _window, cx| {
                                    this.shift_path_k(1, cx);
                                },
                            )),
                    )
                    .child(
                        div()
                            .id("algo-limit-down")
                            .child("limit-")
                            .on_click(cx.listener(
                                |this, _event: &ClickEvent, _window, cx| {
                                    this.shift_simple_limit(-1, cx);
                                },
                            )),
                    )
                    .child(
                        div()
                            .id("algo-limit-up")
                            .child("limit+")
                            .on_click(cx.listener(
                                |this, _event: &ClickEvent, _window, cx| {
                                    this.shift_simple_limit(1, cx);
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
            })
    }
}
