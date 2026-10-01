//! Editable graph model with change notifications.

pub mod affinity;
pub mod algo;
pub mod attrs;
pub mod batch;
pub mod binding;
pub mod centrality;
pub mod classes;
pub mod collection;
pub mod compound;
pub mod distances;
pub mod euler;
pub mod events;
pub mod hierarchical;
pub mod io;
pub mod kmeans;
pub mod markov;
pub mod min_cut;
pub mod positions;
pub mod selector;
pub mod store;
pub mod sync;
pub mod view;

pub use affinity::affinity_clusters;
pub use algo::{
    all_pairs_shortest_paths, articulation_points, bellman_ford_paths, biconnected_components,
    bidirectional_path_cost, breadth_first_order, bridges, condensation_groups, depth_first_order,
    dsatur_groups, feedback_arc_edges, greedy_matching_pairs, has_directed_path,
    heuristic_shortest_path, immediate_dominators, is_bipartite_graph, is_cyclic_directed_graph,
    is_cyclic_undirected_graph, johnson_paths, kosaraju_components, kth_shortest_costs,
    maximal_clique_groups, maximum_flow_value, maximum_matching_pairs, minimum_spanning_forest,
    minimum_spanning_tree_single, negative_cycle_path, post_order, shortest_path,
    shortest_path_cost, shortest_paths, simple_paths_limited, spfa_paths,
    strongly_connected_components, topo_order, topological_order, transitive_reduction,
    undirected_connected_components,
};
pub use attrs::{DataValue, valid_attr_key};
pub use batch::MAX_HISTORY;
pub use binding::{ChangeFilter, subscribe_graph};
pub use centrality::{
    betweenness_centrality, closeness_centrality, degree_centrality, node_order, rank_nodes,
    weighted_degree_centrality,
};
pub use classes::valid_class_name;
pub use collection::{EdgeSet, NodeSet, all_components, component_of, neighborhood};
pub use collection::{predecessors_closure, successors_closure};
pub use compound::{CompoundError, MAX_COMPOUND_DEPTH};
pub use distances::{ClusterMetric, metric_clusters, point_distance};
pub use euler::{eulerian_path_directed, eulerian_path_undirected};
pub use events::GraphChangeEvent;
pub use hierarchical::{Linkage, hierarchical_clusters, hierarchical_clusters_with_linkage};
pub use io::{
    EdgeEntry, GraphDocument, IoError, NodeEntry, export_dot, export_dot_document, import_dot,
};
#[cfg(feature = "json-io")]
pub use io::{export_json, import_json};
pub use kmeans::{fuzzy_cmeans_groups, kmeans_clusters, kmedoids_clusters};
pub use markov::markov_clusters;
pub use min_cut::{MinCut, global_min_cut};
pub use positions::{FixedNodes, Positions};
pub use selector::{
    AttrCondition, AttrOp, ParseError, SelectorCache, SelectorGroup, SelectorQuery, SelectorTarget,
    SelectorValue, StateFilter, parse_selector,
};
pub use store::{EdgeData, GraphStore, NodeData};
pub use sync::{SyncReport, sync_document};
pub use view::{GraphView, MockGraph};

/// Node identifier, re-exported so downstream crates can name a node without
/// depending on petgraph directly.
pub use petgraph::stable_graph::NodeIndex;

/// Edge identifier, re-exported for the same reason as [`NodeIndex`].
pub use petgraph::stable_graph::EdgeIndex;
