//! Model-space coordinates for nodes, kept outside the graph structure.

use std::collections::{HashMap, HashSet};

use cg_types::Point2;
use petgraph::stable_graph::NodeIndex;

/// Layout output: one position per node currently present in the graph.
///
/// Positions live outside the graph store because the structure and its
/// geometric embedding have independent life cycles.
pub type Positions = HashMap<NodeIndex, Point2>;

/// Nodes the user holds in place, for example while dragging.
///
/// Pinned nodes still exert forces on their neighbours during layout, but the
/// layout itself never moves them.
pub type FixedNodes = HashSet<NodeIndex>;
