//! Background algorithm runs over store snapshots.
//!
//! This module owns the task base (endpoint queries, generation guard,
//! background handoff) and every panel dispatch. Each run clones the graph
//! off the UI thread, computes an outcome there, and commits only when its
//! generation is still current.

use std::time::Instant;

use cg_graph::{
    GraphView, NodeIndex, Positions, affinity_clusters, all_pairs_shortest_paths,
    articulation_points, bellman_ford_paths, betweenness_centrality, bidirectional_path_cost,
    breadth_first_order, bridges, closeness_centrality, condensation_groups, degree_centrality,
    depth_first_order, dsatur_groups, eulerian_path_directed, eulerian_path_undirected,
    feedback_arc_edges, fuzzy_cmeans_groups, global_min_cut, greedy_matching_pairs,
    has_directed_path, heuristic_shortest_path, hierarchical_clusters,
    hierarchical_clusters_with_linkage, immediate_dominators, is_bipartite_graph,
    is_cyclic_directed_graph, is_cyclic_undirected_graph, johnson_paths, kmeans_clusters,
    kmedoids_clusters, kosaraju_components, kth_shortest_costs, markov_clusters,
    maximal_clique_groups, maximum_flow_value, maximum_matching_pairs, metric_clusters,
    minimum_spanning_forest, minimum_spanning_tree_single, negative_cycle_path, node_order,
    post_order, rank_nodes, shortest_path, simple_paths_limited, spfa_paths,
    strongly_connected_components, topo_order, topological_order, transitive_reduction,
    undirected_connected_components, weighted_degree_centrality,
};
use gpui::Context;

use crate::algo_panel::{self, AlgoOutcome};
use crate::app_state::{
    AFFINITY_DAMPING, AFFINITY_ITERATIONS, FUZZY_M, GraphWindow, KMEANS_ITERATIONS,
    MARKOV_INFLATION, MARKOV_ITERATIONS, PAGERANK_ITERATIONS,
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

    // Gap-plan bridges: every cg-graph algorithm reachable from the panel.

    pub(crate) fn run_bidirectional(&mut self, cx: &mut Context<Self>) {
        let Some((start, goal)) = self.algo_endpoints(cx) else {
            self.algo_summary = "bidirectional needs at least one node".to_string();
            cx.notify();
            return;
        };
        let snapshot = self.store.read(cx).graph().clone();
        let generation = self.begin_algo_run();
        self.spawn_algo_task(cx, generation, move || {
            let started = Instant::now();
            let cost = bidirectional_path_cost(&snapshot, start, goal);
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            match cost {
                Some(value) => algo_panel::path_outcome(&[start, goal], value, elapsed_ms, "bidirectional"),
                None => algo_panel::path_outcome(&[], 0.0, elapsed_ms, "bidirectional"),
            }
        });
    }

    pub(crate) fn run_spfa(&mut self, cx: &mut Context<Self>) {
        let Some((start, goal)) = self.algo_endpoints(cx) else {
            self.algo_summary = "spfa needs at least one node".to_string();
            cx.notify();
            return;
        };
        let snapshot = self.store.read(cx).graph().clone();
        let generation = self.begin_algo_run();
        self.spawn_algo_task(cx, generation, move || {
            let started = Instant::now();
            let elapsed = || started.elapsed().as_secs_f64() * 1000.0;
            match spfa_paths(&snapshot, start) {
                Ok((distances, predecessors)) => {
                    let cost = distances.get(&goal).copied().unwrap_or(f32::INFINITY);
                    if !cost.is_finite() {
                        return algo_panel::path_outcome(&[], 0.0, elapsed(), "spfa");
                    }
                    let mut path = vec![goal];
                    while let Some(parent) =
                        predecessors.get(&path.last().copied().unwrap_or(goal)).copied().flatten()
                    {
                        path.push(parent);
                        if parent == start || path.len() > snapshot.node_count() + 1 {
                            break;
                        }
                    }
                    path.reverse();
                    if path.first() != Some(&start) {
                        return algo_panel::path_outcome(&[], 0.0, elapsed(), "spfa");
                    }
                    algo_panel::path_outcome(&path, cost, elapsed(), "spfa")
                }
                Err(member) => AlgoOutcome {
                    summary: format!(
                        "spfa: negative cycle at node {} ({:.1}ms)",
                        member.index(),
                        elapsed()
                    ),
                    ..AlgoOutcome::default()
                },
            }
        });
    }

    pub(crate) fn run_johnson(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.store.read(cx).graph().clone();
        let generation = self.begin_algo_run();
        self.spawn_algo_task(cx, generation, move || {
            let started = Instant::now();
            let count = snapshot.node_count();
            match johnson_paths(&snapshot) {
                Ok(matrix) => algo_panel::pairs_outcome(
                    matrix.len(),
                    count * count,
                    started.elapsed().as_secs_f64() * 1000.0,
                ),
                Err(member) => AlgoOutcome {
                    summary: format!(
                        "johnson: negative cycle at node {} ({:.1}ms)",
                        member.index(),
                        started.elapsed().as_secs_f64() * 1000.0
                    ),
                    ..AlgoOutcome::default()
                },
            }
        });
    }

    pub(crate) fn run_kth_shortest(&mut self, cx: &mut Context<Self>) {
        let Some((start, _)) = self.algo_endpoints(cx) else {
            self.algo_summary = "kth shortest needs at least one node".to_string();
            cx.notify();
            return;
        };
        let snapshot = self.store.read(cx).graph().clone();
        let k = self.path_k;
        let generation = self.begin_algo_run();
        self.spawn_algo_task(cx, generation, move || {
            let started = Instant::now();
            let costs = kth_shortest_costs(&snapshot, start, k);
            let mut nodes: Vec<NodeIndex> = costs.keys().copied().collect();
            nodes.sort_unstable_by_key(|node| node.index());
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            let mut outcome = algo_panel::traversal_outcome(&nodes, "kth shortest", elapsed_ms);
            outcome.summary = format!(
                "kth shortest k={k} from {}: {} nodes ({elapsed_ms:.1}ms)",
                start.index(),
                nodes.len()
            );
            outcome
        });
    }

    pub(crate) fn run_kosaraju(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.store.read(cx).graph().clone();
        let generation = self.begin_algo_run();
        self.spawn_algo_task(cx, generation, move || {
            let started = Instant::now();
            let groups = kosaraju_components(&snapshot);
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            algo_panel::groups_outcome(&groups, "kosaraju", elapsed_ms)
        });
    }

    pub(crate) fn run_condensation(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.store.read(cx).graph().clone();
        let generation = self.begin_algo_run();
        self.spawn_algo_task(cx, generation, move || {
            let started = Instant::now();
            let (groups, _) = condensation_groups(&snapshot);
            let mut owner = std::collections::HashMap::new();
            for (ordinal, group) in groups.iter().enumerate() {
                for node in group {
                    owner.insert(*node, ordinal);
                }
            }
            let mut crossing = Vec::new();
            for (source, target) in GraphView::edges(&snapshot) {
                let from = owner.get(&source).copied();
                let to = owner.get(&target).copied();
                if let (Some(from), Some(to)) = (from, to)
                    && from != to
                {
                    crossing.push((source, target));
                }
            }
            crossing.sort_unstable_by_key(|(source, target)| (source.index(), target.index()));
            crossing.dedup();
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            algo_panel::condensation_outcome(&groups, &crossing, elapsed_ms)
        });
    }

    pub(crate) fn run_connectivity(&mut self, cx: &mut Context<Self>) {
        let endpoints = self.algo_endpoints(cx);
        let snapshot = self.store.read(cx).graph().clone();
        let generation = self.begin_algo_run();
        self.spawn_algo_task(cx, generation, move || {
            let started = Instant::now();
            let groups = undirected_connected_components(&snapshot);
            let reachable = endpoints
                .map(|(start, goal)| has_directed_path(&snapshot, start, goal))
                .unwrap_or(false);
            let directed_cyclic = is_cyclic_directed_graph(&snapshot);
            let undirected_cyclic = is_cyclic_undirected_graph(&snapshot);
            let bipartite = is_bipartite_graph(&snapshot);
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            algo_panel::connectivity_outcome(
                &groups,
                reachable,
                directed_cyclic,
                undirected_cyclic,
                bipartite,
                elapsed_ms,
            )
        });
    }

    pub(crate) fn run_order_walk(&mut self, cx: &mut Context<Self>) {
        let Some((start, _)) = self.algo_endpoints(cx) else {
            self.algo_summary = "order walk needs at least one node".to_string();
            cx.notify();
            return;
        };
        let snapshot = self.store.read(cx).graph().clone();
        let generation = self.begin_algo_run();
        self.spawn_algo_task(cx, generation, move || {
            let started = Instant::now();
            let post = post_order(&snapshot, start);
            let topo = topo_order(&snapshot);
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            let mut outcome = algo_panel::traversal_outcome(&post, "post order", elapsed_ms);
            outcome.summary = format!(
                "order walk from {}: post {}, topo {} ({elapsed_ms:.1}ms)",
                start.index(),
                post.len(),
                topo.len()
            );
            outcome
        });
    }

    pub(crate) fn run_kmedoids(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.store.read(cx).graph().clone();
        let positions: Positions = self.layout.read(cx).positions().clone();
        let classes = self.cluster_k;
        let generation = self.begin_algo_run();
        self.spawn_algo_task(cx, generation, move || {
            let started = Instant::now();
            let elapsed_ms = || started.elapsed().as_secs_f64() * 1000.0;
            match kmedoids_clusters(&snapshot, &positions, classes, KMEANS_ITERATIONS) {
                Ok(groups) => algo_panel::groups_outcome(&groups, "k-medoids", elapsed_ms()),
                Err(member) => AlgoOutcome {
                    summary: format!(
                        "k-medoids: invalid input at node {} ({:.1}ms)",
                        member.index(),
                        elapsed_ms()
                    ),
                    ..AlgoOutcome::default()
                },
            }
        });
    }

    pub(crate) fn run_fuzzy(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.store.read(cx).graph().clone();
        let positions: Positions = self.layout.read(cx).positions().clone();
        let classes = self.cluster_k;
        let generation = self.begin_algo_run();
        self.spawn_algo_task(cx, generation, move || {
            let started = Instant::now();
            let elapsed_ms = || started.elapsed().as_secs_f64() * 1000.0;
            match fuzzy_cmeans_groups(&snapshot, &positions, classes, KMEANS_ITERATIONS, FUZZY_M) {
                Ok(groups) => algo_panel::groups_outcome(&groups, "fuzzy c-means", elapsed_ms()),
                Err(member) => AlgoOutcome {
                    summary: format!(
                        "fuzzy c-means: invalid input at node {} ({:.1}ms)",
                        member.index(),
                        elapsed_ms()
                    ),
                    ..AlgoOutcome::default()
                },
            }
        });
    }

    pub(crate) fn run_linkage(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.store.read(cx).graph().clone();
        let positions: Positions = self.layout.read(cx).positions().clone();
        let threshold = self.cluster_threshold;
        let linkage = self.linkage;
        let generation = self.begin_algo_run();
        self.spawn_algo_task(cx, generation, move || {
            let started = Instant::now();
            let elapsed_ms = || started.elapsed().as_secs_f64() * 1000.0;
            let label = match linkage {
                cg_graph::Linkage::Min => "hierarchical min",
                cg_graph::Linkage::Max => "hierarchical max",
                cg_graph::Linkage::Mean => "hierarchical mean",
            };
            match hierarchical_clusters_with_linkage(&snapshot, &positions, threshold, linkage) {
                Ok(groups) => algo_panel::groups_outcome(&groups, label, elapsed_ms()),
                Err(member) => AlgoOutcome {
                    summary: format!(
                        "{label}: invalid input at node {} ({:.1}ms)",
                        member.index(),
                        elapsed_ms()
                    ),
                    ..AlgoOutcome::default()
                },
            }
        });
    }

    pub(crate) fn run_weighted_degree(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.store.read(cx).graph().clone();
        let alpha = self.weighted_alpha;
        let directed = self.weighted_directed;
        let generation = self.begin_algo_run();
        self.spawn_algo_task(cx, generation, move || {
            let started = Instant::now();
            let order = node_order(&snapshot);
            let scores = weighted_degree_centrality(&snapshot, alpha, directed);
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            let label = if directed {
                format!("weighted degree a={alpha:.2} directed")
            } else {
                format!("weighted degree a={alpha:.2} undirected")
            };
            algo_panel::centrality_outcome(&order, &scores, &label, elapsed_ms)
        });
    }

    pub(crate) fn run_max_flow(&mut self, cx: &mut Context<Self>) {
        let Some((start, goal)) = self.algo_endpoints(cx) else {
            self.algo_summary = "max flow needs at least one node".to_string();
            cx.notify();
            return;
        };
        let snapshot = self.store.read(cx).graph().clone();
        let generation = self.begin_algo_run();
        self.spawn_algo_task(cx, generation, move || {
            let started = Instant::now();
            let (value, detailed) = maximum_flow_value(&snapshot, start, goal);
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            algo_panel::flow_outcome(&detailed, value, elapsed_ms)
        });
    }

    pub(crate) fn run_greedy_matching(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.store.read(cx).graph().clone();
        let generation = self.begin_algo_run();
        self.spawn_algo_task(cx, generation, move || {
            let started = Instant::now();
            let pairs = greedy_matching_pairs(&snapshot);
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            algo_panel::matching_outcome(&pairs, "greedy matching", elapsed_ms)
        });
    }

    pub(crate) fn run_max_matching(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.store.read(cx).graph().clone();
        let generation = self.begin_algo_run();
        self.spawn_algo_task(cx, generation, move || {
            let started = Instant::now();
            let pairs = maximum_matching_pairs(&snapshot);
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            algo_panel::matching_outcome(&pairs, "maximum matching", elapsed_ms)
        });
    }

    pub(crate) fn run_dsatur(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.store.read(cx).graph().clone();
        let generation = self.begin_algo_run();
        self.spawn_algo_task(cx, generation, move || {
            let started = Instant::now();
            let (groups, count) = dsatur_groups(&snapshot);
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            let mut outcome = algo_panel::groups_outcome(&groups, "dsatur", elapsed_ms);
            outcome.summary = format!(
                "dsatur: {count} colors, {} groups ({elapsed_ms:.1}ms)",
                groups.len()
            );
            outcome
        });
    }

    pub(crate) fn run_cliques(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.store.read(cx).graph().clone();
        let generation = self.begin_algo_run();
        self.spawn_algo_task(cx, generation, move || {
            let started = Instant::now();
            let groups = maximal_clique_groups(&snapshot);
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            algo_panel::groups_outcome(&groups, "maximal cliques", elapsed_ms)
        });
    }

    pub(crate) fn run_feedback(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.store.read(cx).graph().clone();
        let generation = self.begin_algo_run();
        self.spawn_algo_task(cx, generation, move || {
            let started = Instant::now();
            let edges = feedback_arc_edges(&snapshot);
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            algo_panel::feedback_outcome(&edges, elapsed_ms)
        });
    }

    pub(crate) fn run_simple_paths(&mut self, cx: &mut Context<Self>) {
        let Some((start, goal)) = self.algo_endpoints(cx) else {
            self.algo_summary = "simple paths needs at least one node".to_string();
            cx.notify();
            return;
        };
        let snapshot = self.store.read(cx).graph().clone();
        let limit = self.simple_limit;
        let generation = self.begin_algo_run();
        self.spawn_algo_task(cx, generation, move || {
            let started = Instant::now();
            let paths = simple_paths_limited(&snapshot, start, goal, limit);
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            algo_panel::simple_paths_outcome(&paths, elapsed_ms)
        });
    }
}
