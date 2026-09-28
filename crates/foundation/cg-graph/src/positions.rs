//! Model-space coordinates for nodes, kept outside the graph structure.

use std::collections::HashMap;

use cg_types::Point2;
use petgraph::stable_graph::NodeIndex;

/// Layout output: one position per node currently present in the graph.
///
/// Positions live outside the graph store because the structure and its
/// geometric embedding have independent life cycles.
pub type Positions = HashMap<NodeIndex, Point2>;
