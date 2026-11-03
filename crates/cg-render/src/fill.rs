//! Node body fills and per-channel color interpolation.

/// Fill of nodes carrying no mapping or bypass.
pub const DEFAULT_NODE_FILL: u32 = 0x4a9eff;

/// Fill applied through the bypass to selected or highlighted nodes.
pub const SELECTED_NODE_FILL: u32 = 0xff9f2e;

/// Fill applied through the bypass to the hovered node.
pub const HOVER_NODE_FILL: u32 = 0x8fc2ff;

/// Linear fill of a node body.
///
/// The fill runs from `start` to `end` along `angle_deg`. A solid look is the
/// degenerate form with both ends equal. Angles follow the canvas gradient
/// convention: zero runs top to bottom and values grow clockwise.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NodeFill {
    pub start: u32,
    pub end: u32,
    pub angle_deg: f32,
}

impl NodeFill {
    /// Solid fill with one color for both ends.
    pub fn solid(color: u32) -> Self {
        Self {
            start: color,
            end: color,
            angle_deg: 0.0,
        }
    }

    /// Linear fill from `start` to `end` along `angle_deg`.
    pub fn gradient(start: u32, end: u32, angle_deg: f32) -> Self {
        Self {
            start,
            end,
            angle_deg: clamp_fill_angle(angle_deg),
        }
    }

    /// True when both ends share one color.
    pub fn is_solid(&self) -> bool {
        self.start == self.end
    }

    /// Gradient angle clamped to the valid range.
    pub fn angle(&self) -> f32 {
        clamp_fill_angle(self.angle_deg)
    }

    /// Solid color used when detail levels cannot afford gradients.
    pub fn solid_fallback(&self) -> u32 {
        self.start
    }

    /// Interpolated color at `t` in the closed unit interval.
    pub fn sample(&self, t: f32) -> u32 {
        let ratio = t.clamp(0.0, 1.0);
        lerp_rgb(self.start, self.end, ratio)
    }

    /// Interpolated color halfway between both ends.
    pub fn midpoint(&self) -> u32 {
        self.sample(0.5)
    }
}

impl Default for NodeFill {
    fn default() -> Self {
        Self::solid(DEFAULT_NODE_FILL)
    }
}

/// Clamps a gradient angle to degrees in the closed range.
fn clamp_fill_angle(angle: f32) -> f32 {
    if !angle.is_finite() {
        return 0.0;
    }
    angle.clamp(0.0, 360.0)
}

/// Linear interpolation of two packed colors per channel.
pub(crate) fn lerp_rgb(start: u32, end: u32, t: f32) -> u32 {
    let sr = ((start >> 16) & 0xFF) as f32;
    let sg = ((start >> 8) & 0xFF) as f32;
    let sb = (start & 0xFF) as f32;
    let er = ((end >> 16) & 0xFF) as f32;
    let eg = ((end >> 8) & 0xFF) as f32;
    let eb = (end & 0xFF) as f32;
    let r = (sr + (er - sr) * t).round().clamp(0.0, 255.0) as u32;
    let g = (sg + (eg - sg) * t).round().clamp(0.0, 255.0) as u32;
    let b = (sb + (eb - sb) * t).round().clamp(0.0, 255.0) as u32;
    (r << 16) | (g << 8) | b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gradient_samples_endpoints_midpoint_and_clamps_angle() {
        let fill = NodeFill::gradient(0x000000, 0xFFFFFF, 90.0);
        assert!(!fill.is_solid());
        assert_eq!(fill.sample(0.0), 0x000000);
        assert_eq!(fill.sample(1.0), 0xFFFFFF);
        assert_eq!(fill.midpoint(), 0x808080);
        assert_eq!(fill.sample(-1.0), 0x000000);
        assert_eq!(fill.sample(2.0), 0xFFFFFF);
        assert_eq!(
            NodeFill::gradient(0x111111, 0x222222, f32::NAN).angle(),
            0.0
        );
        assert_eq!(NodeFill::gradient(0x111111, 0x222222, 500.0).angle(), 360.0);
        assert_eq!(NodeFill::gradient(0x111111, 0x222222, -20.0).angle(), 0.0);
    }
}
