//! Batched notifications and snapshot undo for the graph store.
//!
//! Mutations normally broadcast one event each. A batch wraps any number of
//! mutations so subscribers observe a single commit instead, mirroring the
//! nested batching of the reference implementation while staying capped at
//! one event kind. Undo restores whole snapshots at batch granularity.

use std::collections::{BTreeSet, HashMap, HashSet};

use petgraph::Directed;
use petgraph::stable_graph::StableGraph;

use crate::attrs::DataValue;
use crate::events::GraphChangeEvent;
use crate::store::{EdgeData, GraphStore, NodeData};

/// Depth of stored undo states; older states fall off the front.
pub const MAX_HISTORY: usize = 64;

/// Owned copy of every table backing the store, used for undo and redo.
#[derive(Clone, Debug)]
pub(crate) struct StoreSnapshot {
    graph: StableGraph<NodeData, EdgeData, Directed>,
    node_attr_table: HashMap<petgraph::stable_graph::NodeIndex, HashMap<String, DataValue>>,
    edge_attr_table: HashMap<petgraph::stable_graph::EdgeIndex, HashMap<String, DataValue>>,
    node_class_table: HashMap<petgraph::stable_graph::NodeIndex, BTreeSet<String>>,
    edge_class_table: HashMap<petgraph::stable_graph::EdgeIndex, BTreeSet<String>>,
    parents: HashMap<petgraph::stable_graph::NodeIndex, petgraph::stable_graph::NodeIndex>,
    children: HashMap<petgraph::stable_graph::NodeIndex, BTreeSet<petgraph::stable_graph::NodeIndex>>,
    collapsed: HashSet<petgraph::stable_graph::NodeIndex>,
}

impl GraphStore {
    pub(crate) fn snapshot(&self) -> StoreSnapshot {
        StoreSnapshot {
            graph: self.graph.clone(),
            node_attr_table: self.node_attr_table.clone(),
            edge_attr_table: self.edge_attr_table.clone(),
            node_class_table: self.node_class_table.clone(),
            edge_class_table: self.edge_class_table.clone(),
            parents: self.parents.clone(),
            children: self.children.clone(),
            collapsed: self.collapsed.clone(),
        }
    }

    /// Installs `snapshot` as the current state.
    ///
    /// The restored graph is compacted once on the way in: snapshots clone the
    /// whole [`StableGraph`] including its spare capacity, and every later
    /// [`GraphStore::before_mutation`] copies that capacity again, so dropping
    /// the slack here keeps the undo chain from carrying dead allocations
    /// forward. Indices keep their meaning, so downstream caches stay valid.
    pub(crate) fn restore(&mut self, snapshot: StoreSnapshot) {
        self.graph = snapshot.graph;
        self.node_attr_table = snapshot.node_attr_table;
        self.edge_attr_table = snapshot.edge_attr_table;
        self.node_class_table = snapshot.node_class_table;
        self.edge_class_table = snapshot.edge_class_table;
        self.parents = snapshot.parents;
        self.children = snapshot.children;
        self.collapsed = snapshot.collapsed;
        self.graph.shrink_to_fit();
    }

    /// Records the pre-mutation state for undo.
    ///
    /// Outside a batch every mutation checkpoints on its own. Inside a batch
    /// only the outermost entry checkpoints, so the whole batch undoes as one
    /// step. Callers invoke this before mutating any table.
    pub(crate) fn before_mutation(&mut self) {
        if self.batch_depth > 0 {
            if self.batch_snapshot.is_none() {
                self.batch_snapshot = Some(self.snapshot());
                self.future.clear();
            }
            return;
        }
        self.history.push(self.snapshot());
        if self.history.len() > MAX_HISTORY {
            self.history.remove(0);
        }
        self.future.clear();
    }

    /// Emits `event`, or defers it into the open batch.
    ///
    /// Deferred mutations only mark the batch dirty; the outermost
    /// [`GraphStore::end_batch`] announces once on their behalf.
    pub(crate) fn announce(&mut self, cx: &mut gpui::Context<Self>, event: GraphChangeEvent) {
        if self.batch_depth > 0 {
            self.batch_dirty = true;
            return;
        }
        cx.emit(event);
        cx.notify();
    }

    /// True while a batch started by [`GraphStore::begin_batch`] is open.
    pub fn in_batch(&self) -> bool {
        self.batch_depth > 0
    }

    /// Opens a batch; batches nest and only the outermost close broadcasts.
    pub fn begin_batch(&mut self) {
        if self.batch_depth == 0 {
            self.batch_snapshot = None;
            self.batch_dirty = false;
        }
        self.batch_depth += 1;
    }

    /// Closes one batch level, broadcasting once when the outermost closes.
    ///
    /// Returns true when a commit was broadcast. Empty batches record no
    /// history and stay silent.
    pub fn end_batch(&mut self, cx: &mut gpui::Context<Self>) -> bool {
        if self.batch_depth == 0 {
            return false;
        }
        self.batch_depth -= 1;
        if self.batch_depth > 0 {
            return false;
        }
        let dirty = self.batch_dirty;
        self.batch_dirty = false;
        if let Some(snapshot) = self.batch_snapshot.take()
            && dirty
        {
            self.history.push(snapshot);
            if self.history.len() > MAX_HISTORY {
                self.history.remove(0);
            }
        }
        if dirty {
            cx.emit(GraphChangeEvent::BatchCommitted);
            cx.notify();
            true
        } else {
            false
        }
    }

    /// Runs `body` inside a batch, broadcasting at most once.
    pub fn batch(
        &mut self,
        cx: &mut gpui::Context<Self>,
        body: impl FnOnce(&mut Self, &mut gpui::Context<Self>),
    ) {
        self.begin_batch();
        body(self, cx);
        self.end_batch(cx);
    }

    /// Number of undoable steps currently stored.
    pub fn history_len(&self) -> usize {
        self.history.len()
    }

    /// True when [`GraphStore::undo`] would restore a prior state.
    pub fn can_undo(&self) -> bool {
        !self.history.is_empty()
    }

    /// True when [`GraphStore::redo`] would reapply an undone state.
    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }

    /// Restores the state before the latest mutation or batch.
    ///
    /// Positions live outside the store and are not restored; downstream
    /// stages re-derive them from the restored structure. Refused inside an
    /// open batch so a batch always undoes as one step after it closes.
    pub fn undo(&mut self, cx: &mut gpui::Context<Self>) -> bool {
        if self.batch_depth > 0 {
            return false;
        }
        let Some(previous) = self.history.pop() else {
            return false;
        };
        self.future.push(self.snapshot());
        self.restore(previous);
        cx.emit(GraphChangeEvent::BatchCommitted);
        cx.notify();
        true
    }

    /// Reapplies the latest undone state.
    pub fn redo(&mut self, cx: &mut gpui::Context<Self>) -> bool {
        if self.batch_depth > 0 {
            return false;
        }
        let Some(next) = self.future.pop() else {
            return false;
        };
        self.history.push(self.snapshot());
        if self.history.len() > MAX_HISTORY {
            self.history.remove(0);
        }
        self.restore(next);
        cx.emit(GraphChangeEvent::BatchCommitted);
        cx.notify();
        true
    }
}
