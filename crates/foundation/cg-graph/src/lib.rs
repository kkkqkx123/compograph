//! Editable graph model with change notifications.

pub mod algo;
pub mod attrs;
pub mod binding;
pub mod centrality;
pub mod classes;
pub mod compound;
pub mod euler;
pub mod events;
pub mod hierarchical;
pub mod io;
pub mod kmeans;
pub mod markov;
pub mod min_cut;
pub mod positions;
pub mod store;
pub mod view;

pub use algo::{
    all_pairs_shortest_paths, articulation_points, bellman_ford_paths, breadth_first_order,
    bridges, depth_first_order, heuristic_shortest_path, immediate_dominators,
    minimum_spanning_forest, minimum_spanning_tree_single, negative_cycle_path, shortest_path,
    shortest_path_cost, shortest_paths, strongly_connected_components, topological_order,
    transitive_reduction,
};
pub use attrs::{DataValue, valid_attr_key};
pub use binding::{ChangeFilter, subscribe_graph};
pub use centrality::{
    betweenness_centrality, closeness_centrality, degree_centrality, node_order, rank_nodes,
};
pub use classes::valid_class_name;
pub use compound::{CompoundError, MAX_COMPOUND_DEPTH, visible_members};
pub use euler::{eulerian_path_directed, eulerian_path_undirected};
pub use events::GraphChangeEvent;
pub use hierarchical::hierarchical_clusters;
pub use io::{
    EdgeEntry, GraphDocument, IoError, NodeEntry, export_dot, import_dot, remap_positions,
};
#[cfg(feature = "json-io")]
pub use io::{export_json, import_json};
pub use kmeans::kmeans_clusters;
pub use markov::markov_clusters;
pub use min_cut::{MinCut, global_min_cut};
pub use positions::{FixedNodes, Positions};
pub use store::{EdgeData, GraphStore, NodeData};
pub use view::{GraphView, MockGraph};

/// Node identifier, re-exported so downstream crates can name a node without
/// depending on petgraph directly.
pub use petgraph::stable_graph::NodeIndex;

/// Edge identifier, re-exported for the same reason as [`NodeIndex`].
pub use petgraph::stable_graph::EdgeIndex;
