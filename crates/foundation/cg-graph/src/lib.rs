//! Editable graph model with change notifications.

pub mod algo;
pub mod binding;
pub mod events;
pub mod positions;
pub mod store;
pub mod view;

pub use algo::{rank_nodes, shortest_path_cost, shortest_paths, strongly_connected_components};
pub use binding::{ChangeFilter, subscribe_graph};
pub use events::GraphChangeEvent;
pub use positions::{FixedNodes, Positions};
pub use store::{EdgeData, GraphStore, NodeData};
pub use view::{GraphView, MockGraph};

/// Node identifier, re-exported so downstream crates can name a node without
/// depending on petgraph directly.
pub use petgraph::stable_graph::NodeIndex;

/// Edge identifier, re-exported for the same reason as [`NodeIndex`].
pub use petgraph::stable_graph::EdgeIndex;
