//! Label appearance types shared by node and edge labels.

/// Fallback label size used when a resolved node style carries none.
pub const DEFAULT_LABEL_SIZE: f32 = 12.0;

/// Text color of a node label.
pub const DEFAULT_LABEL_COLOR: u32 = 0x1f2933;

/// Fallback label size used when a resolved edge style carries none.
pub const DEFAULT_EDGE_LABEL_SIZE: f32 = 11.0;

/// Text color of an edge label.
pub const DEFAULT_EDGE_LABEL_COLOR: u32 = 0x39424e;

/// Fill of a label background, kept in a light tone independent of mappings.
pub const LABEL_BACKGROUND_FILL: u32 = 0xf1f5f9;

/// Padding around label text when a background is drawn, in pixels.
pub const LABEL_BACKGROUND_PAD: f32 = 3.0;

/// Corner radius of a rounded label background, in pixels.
pub const LABEL_CORNER_RADIUS: f32 = 4.0;

/// Horizontal alignment of a label relative to its anchor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LabelAlign {
    Left,
    /// Historical behavior: text centers on the anchor.
    #[default]
    Center,
    Right,
}

/// Resolved label appearance shared by node and edge labels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LabelStyle {
    pub align: LabelAlign,
    pub background: LabelBackground,
    pub background_color: u32,
    pub corner_radius: f32,
    pub padding: f32,
}

impl Default for LabelStyle {
    fn default() -> Self {
        Self {
            align: LabelAlign::Center,
            background: LabelBackground::None,
            background_color: LABEL_BACKGROUND_FILL,
            corner_radius: LABEL_CORNER_RADIUS,
            padding: LABEL_BACKGROUND_PAD,
        }
    }
}

impl LabelStyle {
    /// Padding guarded to non-negative finite values.
    pub fn padding(&self) -> f32 {
        if self.padding.is_finite() && self.padding > 0.0 {
            self.padding
        } else {
            0.0
        }
    }

    /// Corner radius guarded to non-negative finite values.
    pub fn radius(&self) -> f32 {
        if self.corner_radius.is_finite() && self.corner_radius > 0.0 {
            self.corner_radius
        } else {
            0.0
        }
    }
}

/// Background shape drawn behind a label.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LabelBackground {
    /// No background, the historical default.
    #[default]
    None,
    /// Axis aligned rectangle.
    Rect,
    /// Rounded rectangle, currently painted as a rectangle on canvas.
    RoundRect,
}
