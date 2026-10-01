//! Result-group palette and score-driven node scaling.

/// Distinct fills for result groups such as strongly connected components.
///
/// Groups past the palette length share the overflow fill instead of cycling,
/// so the capped set stays visually distinct from the merged remainder.
pub const SCC_GROUP_FILLS: [u32; 8] = [
    0xff9f2e, 0x4a9eff, 0x3ecf6e, 0xb47bff, 0xff5d6c, 0x2ec4d6, 0xffd23e, 0x7a9e7e,
];

/// Fill of every group past the palette length.
pub const SCC_OVERFLOW_FILL: u32 = 0x8a93a6;

/// Fill for `group`, with overflow groups sharing one muted tone.
pub fn scc_fill(group: usize) -> u32 {
    SCC_GROUP_FILLS
        .get(group)
        .copied()
        .unwrap_or(SCC_OVERFLOW_FILL)
}

/// Smallest node scale produced by score mapping.
pub const MIN_RANK_SCALE: f32 = 0.75;

/// Largest node scale produced by score mapping.
pub const MAX_RANK_SCALE: f32 = 1.75;

/// Node scale for `score` linearly mapped from the observed score range.
///
/// A flat range maps every node to the midpoint scale. Non-positive or
/// non-finite scales fall back to the neutral scale of one.
pub fn scale_for_rank(score: f32, min: f32, max: f32) -> f32 {
    if !score.is_finite() || !min.is_finite() || !max.is_finite() || max <= min {
        return 1.0;
    }
    let ratio = ((score - min) / (max - min)).clamp(0.0, 1.0);
    let scale = MIN_RANK_SCALE + ratio * (MAX_RANK_SCALE - MIN_RANK_SCALE);
    if scale > 0.0 && scale.is_finite() {
        scale
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::appearance::{NodeStyle, NodeStylePatch};

    #[test]
    fn rank_scales_span_the_configured_range() {
        assert_eq!(scale_for_rank(0.0, 0.0, 0.0), 1.0);
        assert_eq!(scale_for_rank(0.0, 0.0, 1.0), MIN_RANK_SCALE);
        assert_eq!(scale_for_rank(1.0, 0.0, 1.0), MAX_RANK_SCALE);
        assert_eq!(scale_for_rank(f32::NAN, 0.0, 1.0), 1.0);
        assert_eq!(scc_fill(0), SCC_GROUP_FILLS[0]);
        assert_eq!(scc_fill(SCC_GROUP_FILLS.len()), SCC_OVERFLOW_FILL);
        let patch = NodeStylePatch::rescaled(1.5);
        assert!(!patch.is_empty());
        let applied = patch.apply_to(&NodeStyle::default());
        assert_eq!(applied.scale, 1.5);
    }
}
