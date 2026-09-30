//! Thin wrappers over petgraph algorithms.
//!
//! Each algorithm family lives in its own file under `algo/`; this module
//! only declares the families and re-exports their public functions so
//! existing `algo::` paths keep working.

pub mod connectivity;
pub mod order;
pub mod paths;
pub mod spanning;
pub mod traversal;

pub use connectivity::{articulation_points, bridges};
pub use order::{
    immediate_dominators, strongly_connected_components, topological_order, transitive_reduction,
};
pub use paths::{
    all_pairs_shortest_paths, bellman_ford_paths, heuristic_shortest_path, negative_cycle_path,
    shortest_path, shortest_path_cost, shortest_paths,
};
pub use spanning::{minimum_spanning_forest, minimum_spanning_tree_single};
pub use traversal::{breadth_first_order, depth_first_order};

pub use crate::centrality::rank_nodes;
