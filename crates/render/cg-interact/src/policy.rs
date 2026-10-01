//! Gesture gating and selection policy for pointer input.

use cg_graph::NodeIndex;

use super::input::{InteractLocks, SelectMode, SelectionState};

/// Zoom factor for a scroll wheel line delta.
///
/// Positive deltas zoom in one notch per line; the factor compounds, so small
/// trackpad deltas stay smooth while notched wheels move visibly.
pub fn wheel_zoom_factor(line_delta: f32) -> f32 {
    if !line_delta.is_finite() {
        return 1.0;
    }
    1.15f32.powf(-line_delta)
}

/// Applies one point tap to `selection` under `mode` and the modifier key.
///
/// A held modifier always toggles the tapped node, preserving the existing
/// rubber-band path. Without a modifier, single mode replaces the set while
/// additive mode accumulates.
pub fn apply_point_select(
    selection: &mut SelectionState,
    mode: SelectMode,
    node: NodeIndex,
    additive_modifier: bool,
) {
    if additive_modifier {
        selection.toggle(node);
        return;
    }
    match mode {
        SelectMode::Single => selection.select(node),
        SelectMode::Additive => selection.add(node),
    }
}

/// True when a blank press clears the selection.
///
/// Locking deselect or holding the additive modifier both keep the set.
pub fn should_clear_on_blank(locks: &InteractLocks, additive_modifier: bool) -> bool {
    locks.can_deselect() && !additive_modifier
}

/// True when a node drag gesture may start under `locks`.
pub fn can_begin_drag(locks: &InteractLocks) -> bool {
    locks.can_drag()
}

/// True when a press may run node hit testing under `locks`.
pub fn can_grab_node(locks: &InteractLocks) -> bool {
    locks.can_grab()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wheel_factor_zooms_in_on_negative_delta() {
        assert!(wheel_zoom_factor(-1.0) > 1.0);
        assert!(wheel_zoom_factor(1.0) < 1.0);
        assert_eq!(wheel_zoom_factor(f32::NAN), 1.0);
    }

    #[test]
    fn point_select_modes_and_modifier_combine() {
        use super::super::input::SelectionState;

        let mut selection = SelectionState::default();
        apply_point_select(&mut selection, SelectMode::Single, NodeIndex::new(1), false);
        apply_point_select(&mut selection, SelectMode::Single, NodeIndex::new(2), false);
        assert_eq!(
            selection.iter().collect::<Vec<_>>(),
            vec![NodeIndex::new(2)]
        );
        let mut additive = SelectionState::default();
        apply_point_select(
            &mut additive,
            SelectMode::Additive,
            NodeIndex::new(1),
            false,
        );
        apply_point_select(
            &mut additive,
            SelectMode::Additive,
            NodeIndex::new(2),
            false,
        );
        assert_eq!(additive.len(), 2);
        apply_point_select(&mut additive, SelectMode::Single, NodeIndex::new(1), true);
        assert!(!additive.contains(NodeIndex::new(1)));
        assert!(additive.contains(NodeIndex::new(2)));
    }

    #[test]
    fn locks_gate_drag_grab_and_deselect() {
        let open = InteractLocks::default();
        assert!(can_begin_drag(&open));
        assert!(can_grab_node(&open));
        assert!(should_clear_on_blank(&open, false));
        assert!(!should_clear_on_blank(&open, true));
        let locked = InteractLocks {
            lock_drag: true,
            no_grab: true,
            no_deselect: true,
        };
        assert!(!can_begin_drag(&locked));
        assert!(!can_grab_node(&locked));
        assert!(!should_clear_on_blank(&locked, false));
    }
}
