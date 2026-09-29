//! Detail levels trading visual fidelity for frame rate.
//!
//! The camera zoom and visible element count select one of three levels.
//! Hysteresis keeps the level stable near thresholds, and hit testing always
//! uses full-precision geometry regardless of the painted level.

/// Painted detail of the current frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DetailLevel {
    /// Curved edges, arrowheads, full node blocks.
    #[default]
    Full,
    /// Straight edges, arrows on demand, plain self loops.
    Simplified,
    /// Square nodes, straight edgeless-arrow lines, no labels or hover layer.
    Minimal,
}

impl DetailLevel {
    pub fn draws_arrows(self) -> bool {
        matches!(self, DetailLevel::Full | DetailLevel::Simplified)
    }

    pub fn draws_curves(self) -> bool {
        matches!(self, DetailLevel::Full)
    }
}

/// Tunable thresholds selecting the detail level.
#[derive(Clone, Copy, Debug)]
pub struct LodParams {
    /// Zoom below which simplified rendering starts.
    pub simplify_zoom: f32,
    /// Zoom below which minimal rendering starts.
    pub minimal_zoom: f32,
    /// Visible elements above which simplified rendering starts.
    pub simplify_count: usize,
    /// Visible elements above which minimal rendering starts.
    pub minimal_count: usize,
    /// Hysteresis margin applied when stepping back up a level.
    pub hysteresis: f32,
}

impl Default for LodParams {
    fn default() -> Self {
        Self {
            simplify_zoom: 0.6,
            minimal_zoom: 0.3,
            simplify_count: 2_000,
            minimal_count: 6_000,
            hysteresis: 0.05,
        }
    }
}

impl LodParams {
    /// Selects the level for `zoom` and `visible` elements.
    ///
    /// `previous` applies the hysteresis margin when the candidate level
    /// would step back up, preventing flicker at threshold boundaries.
    pub fn select(&self, zoom: f32, visible: usize, previous: DetailLevel) -> DetailLevel {
        let base = self.base_level(zoom, visible);
        if level_rank(base) < level_rank(previous) {
            let margin = self.hysteresis;
            let strict = self.base_level(zoom - margin, visible.saturating_add(64));
            if level_rank(strict) < level_rank(previous) {
                base
            } else {
                previous
            }
        } else {
            base
        }
    }

    fn base_level(&self, zoom: f32, visible: usize) -> DetailLevel {
        if zoom <= self.minimal_zoom || visible >= self.minimal_count {
            DetailLevel::Minimal
        } else if zoom <= self.simplify_zoom || visible >= self.simplify_count {
            DetailLevel::Simplified
        } else {
            DetailLevel::Full
        }
    }
}

fn level_rank(level: DetailLevel) -> u8 {
    match level {
        DetailLevel::Full => 0,
        DetailLevel::Simplified => 1,
        DetailLevel::Minimal => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thresholds_step_down_without_history() {
        let params = LodParams::default();
        assert_eq!(
            params.select(1.0, 100, DetailLevel::Full),
            DetailLevel::Full
        );
        assert_eq!(
            params.select(0.5, 100, DetailLevel::Full),
            DetailLevel::Simplified
        );
        assert_eq!(
            params.select(0.2, 100, DetailLevel::Full),
            DetailLevel::Minimal
        );
        assert_eq!(
            params.select(1.0, 10_000, DetailLevel::Full),
            DetailLevel::Minimal
        );
    }

    #[test]
    fn hysteresis_holds_the_lower_level_near_the_boundary() {
        let params = LodParams::default();
        let down = params.select(0.5, 100, DetailLevel::Full);
        assert_eq!(down, DetailLevel::Simplified);
        let hovering = params.select(0.61, 100, DetailLevel::Simplified);
        assert_eq!(hovering, DetailLevel::Simplified);
        let recovered = params.select(0.8, 100, DetailLevel::Simplified);
        assert_eq!(recovered, DetailLevel::Full);
    }
}
