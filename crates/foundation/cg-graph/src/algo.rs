//! Thin wrappers over petgraph algorithms.
//!
//! Each algorithm family lives in its own file under `algo/`; this module
//! only declares the families and re-exports their public functions so
//! existing `algo::` paths keep working.

pub mod combinatorial;
pub mod connectivity;
pub mod order;
pub mod paths;
pub mod spanning;
pub mod traversal;

pub use combinatorial::{
    dsatur_groups, feedback_arc_edges, greedy_matching_pairs, maximal_clique_groups,
    maximum_flow_value, maximum_matching_pairs, simple_paths_limited,
};
pub use connectivity::{
    articulation_points, bridges, has_directed_path, is_bipartite_graph, is_cyclic_directed_graph,
    is_cyclic_undirected_graph, undirected_connected_components,
};
pub use order::{
    condensation_groups, immediate_dominators, kosaraju_components,
    strongly_connected_components, topological_order, transitive_reduction,
};
pub use paths::{
    all_pairs_shortest_paths, bellman_ford_paths, bidirectional_path_cost, heuristic_shortest_path,
    johnson_paths, kth_shortest_costs, negative_cycle_path, shortest_path, shortest_path_cost,
    shortest_paths, spfa_paths,
};
pub use spanning::{minimum_spanning_forest, minimum_spanning_tree_single};
pub use traversal::{breadth_first_order, depth_first_order, post_order, topo_order};

pub use crate::centrality::rank_nodes;
