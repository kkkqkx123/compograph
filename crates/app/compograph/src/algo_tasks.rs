//! Background algorithm runs over store snapshots.
//!
//! This module owns the task base (endpoint queries, generation guard,
//! background handoff) and every panel dispatch. Each run clones the graph
//! off the UI thread, computes an outcome there, and commits only when its
//! generation is still current.

use std::time::Instant;

use cg_graph::{
    NodeIndex, Positions, affinity_clusters, all_pairs_shortest_paths, articulation_points,
    bellman_ford_paths, betweenness_centrality, breadth_first_order, bridges, closeness_centrality,
    degree_centrality, depth_first_order, eulerian_path_directed, eulerian_path_undirected,
    global_min_cut, heuristic_shortest_path, hierarchical_clusters, immediate_dominators,
    kmeans_clusters, markov_clusters, metric_clusters, minimum_spanning_forest,
    minimum_spanning_tree_single, negative_cycle_path, node_order, rank_nodes, shortest_path,
    strongly_connected_components, topological_order, transitive_reduction,
};
use gpui::Context;

use crate::algo_panel::{self, AlgoOutcome};
use crate::app_state::{
    AFFINITY_DAMPING, AFFINITY_ITERATIONS, GraphWindow, KMEANS_ITERATIONS, MARKOV_INFLATION,
    MARKOV_ITERATIONS, PAGERANK_ITERATIONS,
};

impl GraphWindow {
    /// Sorted node identifiers currently in the store.
    pub(crate) fn ordered_nodes(&self, cx: &gpui::App) -> Vec<NodeIndex> {
        let mut ids: Vec<NodeIndex> = self.store.read(cx).node_ids().collect();
        ids.sort_unstable_by_key(|node| node.index());
        ids
    }

    /// Start and goal nodes selected by the panel parameters.
    pub(crate) fn algo_endpoints(&self, cx: &gpui::App) -> Option<(NodeIndex, NodeIndex)> {
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
    pub(crate) fn endpoint_label(&self, slot: usize, cx: &gpui::App) -> String {
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
    pub(crate) fn begin_algo_run(&mut self) -> u64 {
        self.algo_generation += 1;
        self.algo_busy = true;
        self.algo_generation
    }

    /// Commits a background outcome unless a newer run superseded it.
    ///
    /// Returns false for stale generations without touching any state, so
    /// overlapping runs cannot overwrite each other out of order.
    pub(crate) fn commit_outcome(&mut self, generation: u64, outcome: AlgoOutcome) -> bool {
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
    pub(crate) fn spawn_algo_task(
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

    // Path searches between the panel endpoints.

    pub(crate) fn run_shortest_path(&mut self, cx: &mut Context<Self>) {
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

    pub(crate) fn run_heuristic_path(&mut self, cx: &mut Context<Self>) {
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

    pub(crate) fn run_bellman_ford(&mut self, cx: &mut Context<Self>) {
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

    pub(crate) fn run_traversals(&mut self, cx: &mut Context<Self>) {
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

    pub(crate) fn run_dominators(&mut self, cx: &mut Context<Self>) {
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

    // Rankings over the whole graph.

    pub(crate) fn run_components(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.store.read(cx).graph().clone();
        let generation = self.begin_algo_run();
        self.spawn_algo_task(cx, generation, move || {
            let started = Instant::now();
            let groups = strongly_connected_components(&snapshot);
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            algo_panel::scc_outcome(&groups, elapsed_ms)
        });
    }

    pub(crate) fn run_pagerank(&mut self, cx: &mut Context<Self>) {
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

    pub(crate) fn run_degree(&mut self, cx: &mut Context<Self>) {
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

    pub(crate) fn run_closeness(&mut self, cx: &mut Context<Self>) {
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

    pub(crate) fn run_betweenness(&mut self, cx: &mut Context<Self>) {
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

    // Structure: cuts, reachability, order, trees, trails.

    pub(crate) fn run_cuts(&mut self, cx: &mut Context<Self>) {
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

    pub(crate) fn run_all_pairs(&mut self, cx: &mut Context<Self>) {
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

    pub(crate) fn run_topo_reduction(&mut self, cx: &mut Context<Self>) {
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

    pub(crate) fn run_spanning_forest(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.store.read(cx).graph().clone();
        let generation = self.begin_algo_run();
        self.spawn_algo_task(cx, generation, move || {
            let started = Instant::now();
            let edges = minimum_spanning_forest(&snapshot);
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            algo_panel::forest_outcome(&edges, elapsed_ms)
        });
    }

    pub(crate) fn run_mst_single(&mut self, cx: &mut Context<Self>) {
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

    pub(crate) fn run_euler_directed(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.store.read(cx).graph().clone();
        let generation = self.begin_algo_run();
        self.spawn_algo_task(cx, generation, move || {
            let started = Instant::now();
            let trail = eulerian_path_directed(&snapshot);
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            algo_panel::euler_outcome(&trail, "euler directed", elapsed_ms)
        });
    }

    pub(crate) fn run_euler_undirected(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.store.read(cx).graph().clone();
        let generation = self.begin_algo_run();
        self.spawn_algo_task(cx, generation, move || {
            let started = Instant::now();
            let trail = eulerian_path_undirected(&snapshot);
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            algo_panel::euler_outcome(&trail, "euler undirected", elapsed_ms)
        });
    }

    pub(crate) fn run_min_cut(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.store.read(cx).graph().clone();
        let generation = self.begin_algo_run();
        self.spawn_algo_task(cx, generation, move || {
            let started = Instant::now();
            let cut = global_min_cut(&snapshot);
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            algo_panel::mincut_outcome(&cut, elapsed_ms)
        });
    }

    // Clusterings over positions or topology.

    pub(crate) fn run_hierarchical(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.store.read(cx).graph().clone();
        let positions: Positions = self.layout.read(cx).positions().clone();
        let threshold = self.cluster_threshold;
        let generation = self.begin_algo_run();
        self.spawn_algo_task(cx, generation, move || {
            let started = Instant::now();
            let elapsed_ms = || started.elapsed().as_secs_f64() * 1000.0;
            match hierarchical_clusters(&snapshot, &positions, threshold) {
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

    pub(crate) fn run_markov(&mut self, cx: &mut Context<Self>) {
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

    pub(crate) fn run_kmeans(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.store.read(cx).graph().clone();
        let positions: Positions = self.layout.read(cx).positions().clone();
        let classes = self.cluster_k;
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

    pub(crate) fn run_affinity(&mut self, cx: &mut Context<Self>) {
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

    pub(crate) fn run_metric_clusters(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.store.read(cx).graph().clone();
        let positions: Positions = self.layout.read(cx).positions().clone();
        let metric = self.cluster_metric;
        let threshold = self.cluster_threshold;
        let generation = self.begin_algo_run();
        self.spawn_algo_task(cx, generation, move || {
            let started = Instant::now();
            let elapsed_ms = || started.elapsed().as_secs_f64() * 1000.0;
            match metric_clusters(&snapshot, &positions, metric, threshold) {
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
}
