//! Editable graph model with change notifications.

pub mod algo;
pub mod binding;
pub mod events;
pub mod io;
pub mod positions;
pub mod store;
pub mod view;

pub use algo::{
    heuristic_shortest_path, immediate_dominators, minimum_spanning_forest,
    minimum_spanning_tree_single, rank_nodes, shortest_path, shortest_path_cost, shortest_paths,
    strongly_connected_components, topological_order, transitive_reduction,
};
pub use binding::{ChangeFilter, subscribe_graph};
pub use events::GraphChangeEvent;
pub use io::{
    EdgeEntry, GraphDocument, IoError, NodeEntry, export_dot, import_dot, remap_positions,
};
#[cfg(feature = "json-io")]
pub use io::{export_json, import_json};
pub use positions::{FixedNodes, Positions};
pub use store::{EdgeData, GraphStore, NodeData};
pub use view::{GraphView, MockGraph};

/// Node identifier, re-exported so downstream crates can name a node without
/// depending on petgraph directly.
pub use petgraph::stable_graph::NodeIndex;

/// Edge identifier, re-exported for the same reason as [`NodeIndex`].
pub use petgraph::stable_graph::EdgeIndex;
