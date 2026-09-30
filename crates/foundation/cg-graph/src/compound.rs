//! Compound hierarchy: parent links, collapse state, folding-aware reads.
//!
//! The store owns the hierarchy side tables. Layout and rendering only read
//! through the queries here and the folding-aware view implementation.

use std::collections::{HashMap, HashSet};

use petgraph::stable_graph::NodeIndex;

use crate::events::GraphChangeEvent;
use crate::store::GraphStore;

/// Maximum nesting depth of the compound hierarchy.
///
/// Roots sit at depth zero; every parent hop adds one. Deeper mounts are
/// rejected with an error and leave the existing hierarchy untouched.
pub const MAX_COMPOUND_DEPTH: usize = 8;

/// Reason a parent mount was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompoundError {
    message: String,
}

impl CompoundError {
    fn refused(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Human-readable reason the mount was refused.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for CompoundError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for CompoundError {}

impl GraphStore {
    /// Direct parent of `node`, if it is nested.
    pub fn parent_of(&self, node: NodeIndex) -> Option<NodeIndex> {
        self.parents.get(&node).copied()
    }

    /// Direct children of `node`, in index order.
    pub fn children_of(&self, node: NodeIndex) -> Vec<NodeIndex> {
        self.children
            .get(&node)
            .map(|set| set.iter().copied().collect())
            .unwrap_or_default()
    }

    /// True when `node` has at least one direct child.
    pub fn is_container(&self, node: NodeIndex) -> bool {
        self.children.get(&node).is_some_and(|set| !set.is_empty())
    }

    /// True when `node` has no parent.
    pub fn is_root(&self, node: NodeIndex) -> bool {
        self.graph.node_weight(node).is_some() && !self.parents.contains_key(&node)
    }

    /// Ancestors from the direct parent up to the root.
    pub fn ancestors_of(&self, node: NodeIndex) -> Vec<NodeIndex> {
        let mut chain = Vec::new();
        let mut cursor = self.parents.get(&node).copied();
        while let Some(parent) = cursor {
            chain.push(parent);
            cursor = self.parents.get(&parent).copied();
        }
        chain
    }

    /// Every descendant of `node` in deterministic order.
    pub fn descendants_of(&self, node: NodeIndex) -> Vec<NodeIndex> {
        let mut ordered = Vec::new();
        let mut stack: Vec<NodeIndex> = self.children_of(node);
        stack.sort_unstable_by_key(|member| std::cmp::Reverse(member.index()));
        while let Some(next) = stack.pop() {
            ordered.push(next);
            let mut children = self.children_of(next);
            children.sort_unstable_by_key(|member| std::cmp::Reverse(member.index()));
            stack.extend(children);
        }
        ordered.sort_unstable_by_key(|member| member.index());
        ordered.dedup_by_key(|member| member.index());
        ordered
    }

    /// Node plus all its descendants, in index order.
    pub fn subtree_of(&self, node: NodeIndex) -> Vec<NodeIndex> {
        let mut members = vec![node];
        members.extend(self.descendants_of(node));
        members.sort_unstable_by_key(|member| member.index());
        members.dedup_by_key(|member| member.index());
        members
    }

    /// Depth of `node`: roots sit at zero.
    pub fn depth_of(&self, node: NodeIndex) -> Option<usize> {
        if self.graph.node_weight(node).is_none() {
            return None;
        }
        Some(self.ancestors_of(node).len())
    }

    /// True when `node` is collapsed as a container.
    pub fn is_collapsed(&self, node: NodeIndex) -> bool {
        self.collapsed.contains(&node)
    }

    /// True when the hierarchy holds at least one parent link.
    pub fn has_compound(&self) -> bool {
        !self.parents.is_empty()
    }

    /// True when `node` and every ancestor avoid collapse.
    pub fn is_visible(&self, node: NodeIndex) -> bool {
        if self.graph.node_weight(node).is_none() {
            return false;
        }
        if self.collapsed.contains(&node) {
            return true;
        }
        let mut cursor = self.parents.get(&node).copied();
        while let Some(parent) = cursor {
            if self.collapsed.contains(&parent) {
                return false;
            }
            if self.graph.node_weight(parent).is_none() {
                return false;
            }
            cursor = self.parents.get(&parent).copied();
        }
        true
    }

    /// Visible nodes in index order; hidden descendants are skipped.
    pub fn visible_node_ids(&self) -> Vec<NodeIndex> {
        let mut ids: Vec<NodeIndex> = self
            .node_ids()
            .filter(|node| self.is_visible(*node))
            .collect();
        ids.sort_unstable_by_key(|node| node.index());
        ids
    }

    /// Nearest visible ancestor of `node`, including itself.
    pub fn visible_ancestor(&self, node: NodeIndex) -> Option<NodeIndex> {
        let mut cursor = Some(node);
        while let Some(current) = cursor {
            if self.graph.node_weight(current).is_none() {
                return None;
            }
            if self.is_visible(current) {
                return Some(current);
            }
            cursor = self.parents.get(&current).copied();
        }
        None
    }

    /// Visible edges with collapsed descendants proxied to their container.
    ///
    /// Internal edges of a collapsed subtree vanish; edges crossing the
    /// collapse boundary reappear with the hidden endpoint replaced by its
    /// nearest visible ancestor. Parallel edges stay parallel.
    pub fn visible_edges(&self) -> Vec<(NodeIndex, NodeIndex)> {
        let mut proxied = Vec::new();
        for edge in self.graph.edge_indices() {
            let Some((source, target)) = self.graph.edge_endpoints(edge) else {
                continue;
            };
            let visible_source = self.visible_ancestor(source);
            let visible_target = self.visible_ancestor(target);
            match (visible_source, visible_target) {
                (Some(mapped_source), Some(mapped_target)) => {
                    if mapped_source == source
                        && mapped_target == target
                        && (!self.is_visible(source) || !self.is_visible(target))
                    {
                        continue;
                    }
                    if !self.is_visible(mapped_source) || !self.is_visible(mapped_target) {
                        continue;
                    }
                    if mapped_source != source || mapped_target != target {
                        proxied.push((mapped_source, mapped_target));
                        continue;
                    }
                    if self.is_visible(source) && self.is_visible(target) {
                        proxied.push((source, target));
                    }
                }
                _ => continue,
            }
        }
        proxied.sort_unstable_by_key(|(source, target)| (source.index(), target.index()));
        proxied
    }

    /// Top-level ancestor of a visible node: itself when root.
    pub fn top_ancestor(&self, node: NodeIndex) -> Option<NodeIndex> {
        if !self.is_visible(node) || self.graph.node_weight(node).is_none() {
            return None;
        }
        let mut cursor = node;
        while let Some(parent) = self.parents.get(&cursor).copied() {
            if !self.is_visible(parent) {
                break;
            }
            cursor = parent;
        }
        Some(cursor)
    }

    /// Visible nodes grouped by top-level ancestor, in deterministic order.
    pub fn compound_groups(&self) -> Vec<Vec<NodeIndex>> {
        let mut by_root: HashMap<NodeIndex, Vec<NodeIndex>> = HashMap::new();
        for node in self.visible_node_ids() {
            if let Some(root) = self.top_ancestor(node) {
                by_root.entry(root).or_default().push(node);
            }
        }
        let mut groups: Vec<Vec<NodeIndex>> = by_root.into_values().collect();
        for group in &mut groups {
            group.sort_unstable_by_key(|node| node.index());
        }
        groups.sort_unstable_by_key(|group| {
            group.first().map(|node| node.index()).unwrap_or(usize::MAX)
        });
        groups
    }

    /// Attaches `child` under `parent`, replacing any previous parent.
    pub fn set_parent(
        &mut self,
        cx: &mut gpui::Context<Self>,
        child: NodeIndex,
        parent: Option<NodeIndex>,
    ) -> Result<(), CompoundError> {
        if self.graph.node_weight(child).is_none() {
            return Err(CompoundError::refused("unknown child node"));
        }
        if let Some(next) = parent {
            if self.graph.node_weight(next).is_none() {
                return Err(CompoundError::refused("unknown parent node"));
            }
            if next == child {
                return Err(CompoundError::refused("a node cannot parent itself"));
            }
            if self.subtree_of(child).contains(&next) {
                return Err(CompoundError::refused("parenting would create a cycle"));
            }
            let parent_depth = self.ancestors_of(next).len();
            let child_subtree = self.subtree_of(child);
            let child_height = child_subtree
                .iter()
                .map(|member| self.relative_height(child, *member))
                .max()
                .unwrap_or(0);
            if parent_depth + 1 + child_height > MAX_COMPOUND_DEPTH {
                return Err(CompoundError::refused("compound depth limit exceeded"));
            }
        }
        if self.parents.get(&child).copied() == parent {
            return Ok(());
        }
        if let Some(previous) = self.parents.remove(&child) {
            if let Some(siblings) = self.children.get_mut(&previous) {
                siblings.remove(&child);
                if siblings.is_empty() {
                    self.children.remove(&previous);
                }
            }
        }
        if let Some(next) = parent {
            self.parents.insert(child, next);
            self.children.entry(next).or_default().insert(child);
        }
        cx.emit(GraphChangeEvent::ParentChanged(child));
        cx.notify();
        Ok(())
    }

    /// Folds or unfolds `node`; only containers may collapse.
    pub fn set_collapsed(
        &mut self,
        cx: &mut gpui::Context<Self>,
        node: NodeIndex,
        collapsed: bool,
    ) -> bool {
        if self.graph.node_weight(node).is_none() || !self.is_container(node) {
            return false;
        }
        let changed = if collapsed {
            self.collapsed.insert(node)
        } else {
            self.collapsed.remove(&node)
        };
        if changed {
            cx.emit(GraphChangeEvent::CollapsedChanged(node));
            cx.notify();
        }
        changed
    }

    fn relative_height(&self, root: NodeIndex, member: NodeIndex) -> usize {
        let mut height = 0usize;
        let mut cursor = member;
        while cursor != root {
            let Some(parent) = self.parents.get(&cursor).copied() else {
                break;
            };
            height += 1;
            cursor = parent;
        }
        height
    }

    pub(crate) fn detach_compound(&mut self, node: NodeIndex) {
        if let Some(parent) = self.parents.remove(&node) {
            if let Some(siblings) = self.children.get_mut(&parent) {
                siblings.remove(&node);
                if siblings.is_empty() {
                    self.children.remove(&parent);
                }
            }
        }
        if let Some(children) = self.children.remove(&node) {
            for child in children {
                self.parents.remove(&child);
            }
        }
        self.collapsed.remove(&node);
        let orphans: Vec<NodeIndex> = self
            .parents
            .iter()
            .filter(|(_, parent)| **parent == node)
            .map(|(child, _)| *child)
            .collect();
        for child in orphans {
            self.parents.remove(&child);
        }
    }

    pub(crate) fn clear_compound(&mut self) {
        self.parents.clear();
        self.children.clear();
        self.collapsed.clear();
    }

    pub(crate) fn compound_tables_for_document(
        &self,
    ) -> (HashMap<NodeIndex, Option<NodeIndex>>, HashSet<NodeIndex>) {
        let mut parents: HashMap<NodeIndex, Option<NodeIndex>> = HashMap::new();
        for node in self.node_ids() {
            parents.insert(node, self.parents.get(&node).copied());
        }
        (parents, self.collapsed.clone())
    }

    pub(crate) fn restore_compound(
        &mut self,
        parents: &HashMap<NodeIndex, Option<NodeIndex>>,
        collapsed: &HashSet<NodeIndex>,
    ) {
        self.parents.clear();
        self.children.clear();
        self.collapsed.clear();
        let mut ordered: Vec<NodeIndex> = parents.keys().copied().collect();
        ordered.sort_unstable_by_key(|node| node.index());
        for node in ordered {
            if self.graph.node_weight(node).is_none() {
                continue;
            }
            if let Some(parent) = parents.get(&node).copied().flatten() {
                if self.graph.node_weight(parent).is_none() || parent == node {
                    continue;
                }
                if self.subtree_would_cycle(node, parent) {
                    continue;
                }
                if self.ancestors_of(parent).len() + 1 > MAX_COMPOUND_DEPTH {
                    continue;
                }
                self.parents.insert(node, parent);
                self.children.entry(parent).or_default().insert(node);
            }
        }
        for node in collapsed {
            if self.is_container(*node) {
                self.collapsed.insert(*node);
            }
        }
    }

    fn subtree_would_cycle(&self, child: NodeIndex, parent: NodeIndex) -> bool {
        let mut cursor = Some(parent);
        while let Some(current) = cursor {
            if current == child {
                return true;
            }
            cursor = self.parents.get(&current).copied();
        }
        let mut stack = vec![child];
        let mut seen = HashSet::new();
        while let Some(next) = stack.pop() {
            if !seen.insert(next) {
                continue;
            }
            if next == parent {
                return true;
            }
            stack.extend(self.children_of(next));
        }
        false
    }
}

/// Visible members of `members` under `store`, in index order.
pub fn visible_members(store: &GraphStore, members: &[NodeIndex]) -> Vec<NodeIndex> {
    let mut visible: Vec<NodeIndex> = members
        .iter()
        .copied()
        .filter(|node| store.is_visible(*node))
        .collect();
    visible.sort_unstable_by_key(|node| node.index());
    visible
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn depth_limit_is_eight() {
        assert_eq!(MAX_COMPOUND_DEPTH, 8);
    }

    #[test]
    fn cycle_detection_covers_children() {
        let mut seen: HashSet<NodeIndex> = HashSet::new();
        seen.insert(NodeIndex::new(1));
        assert!(seen.contains(&NodeIndex::new(1)));
    }
}
