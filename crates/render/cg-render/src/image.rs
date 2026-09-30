//! Node background images from local files.
//!
//! The style layer owns the image specification and the plan layer carries it
//! to the canvas. This module owns the specification, the fit modes, the
//! cache state machine and the fit geometry, so loading policy and math stay
//! in one place while painting keeps its solid fallback.

use std::collections::HashMap;

use cg_types::{Point2, Rect, Vec2};

/// How a decoded image fills the node body.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImageFit {
    /// Whole image fits inside the node body, preserving aspect ratio.
    #[default]
    Contain,
    /// Image covers the node body, preserving aspect ratio.
    Cover,
}

/// Background image of one node, resolved from the style.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeImage {
    pub path: String,
    pub fit: ImageFit,
}

impl NodeImage {
    /// Image specification for a local file path.
    pub fn new(path: impl Into<String>, fit: ImageFit) -> Self {
        Self {
            path: path.into(),
            fit,
        }
    }

    /// True when the path is a supported local file reference.
    ///
    /// Empty paths and network scheme prefixes are rejected so offline
    /// deployments never attempt remote loads.
    pub fn is_supported(&self) -> bool {
        if self.path.trim().is_empty() {
            return false;
        }
        let lowered = self.path.trim_start().to_lowercase();
        !lowered.starts_with("http://")
            && !lowered.starts_with("https://")
            && !lowered.starts_with("data:")
    }
}

/// Loading state of one cached path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImageStatus {
    Pending,
    Ready,
    Failed,
}

/// One cache entry keyed by file path.
#[derive(Clone, Debug)]
pub struct ImageEntry {
    pub width: u32,
    pub height: u32,
    pub fit: ImageFit,
    pub status: ImageStatus,
    pub diagnostic: Option<String>,
}

impl ImageEntry {
    fn pending(fit: ImageFit) -> Self {
        Self {
            width: 0,
            height: 0,
            fit,
            status: ImageStatus::Pending,
            diagnostic: None,
        }
    }
}

/// Local-file image cache keyed by path.
///
/// The cache tracks loading state only; pixel decoding arrives through
/// [`ImageCache::resolve`] from the async loader and failures through
/// [`ImageCache::fail`]. Repeated requests for a known path hit without
/// re-decoding. Notifications stay with the caller: resolve and fail report
/// whether the caller should repaint once.
#[derive(Clone, Debug, Default)]
pub struct ImageCache {
    entries: HashMap<String, ImageEntry>,
}

impl ImageCache {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Registers interest in `path`, returning true on first registration.
    ///
    /// Unsupported paths are refused without an entry. Known paths hit and
    /// report false so callers skip duplicate decodes.
    pub fn request(&mut self, image: &NodeImage) -> bool {
        if !image.is_supported() {
            return false;
        }
        if self.entries.contains_key(&image.path) {
            return false;
        }
        self.entries
            .insert(image.path.clone(), ImageEntry::pending(image.fit));
        true
    }

    /// Marks `path` decoded with its pixel size, reporting true when the
    /// state moved and the caller should repaint once.
    pub fn resolve(&mut self, path: &str, width: u32, height: u32) -> bool {
        let Some(entry) = self.entries.get_mut(path) else {
            return false;
        };
        if width == 0 || height == 0 {
            entry.status = ImageStatus::Failed;
            entry.diagnostic = Some("decoded image has no pixels".to_string());
            return true;
        }
        let moved =
            entry.status != ImageStatus::Ready || entry.width != width || entry.height != height;
        entry.width = width;
        entry.height = height;
        entry.status = ImageStatus::Ready;
        entry.diagnostic = None;
        moved
    }

    /// Marks `path` failed with a diagnostic, reporting true when the state
    /// moved and the caller should repaint once.
    pub fn fail(&mut self, path: &str, reason: impl Into<String>) -> bool {
        let Some(entry) = self.entries.get_mut(path) else {
            return false;
        };
        let already_failed = entry.status == ImageStatus::Failed;
        entry.status = ImageStatus::Failed;
        entry.diagnostic = Some(reason.into());
        !already_failed
    }

    pub fn get(&self, path: &str) -> Option<&ImageEntry> {
        self.entries.get(path)
    }

    /// True when `path` holds decoded pixels ready to paint.
    pub fn is_ready(&self, path: &str) -> bool {
        self.entries
            .get(path)
            .is_some_and(|entry| entry.status == ImageStatus::Ready)
    }

    /// Diagnostic of a failed path, if any.
    pub fn diagnostic(&self, path: &str) -> Option<&str> {
        self.entries
            .get(path)
            .and_then(|entry| entry.diagnostic.as_deref())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Draw rectangle of an image inside the node bounds.
///
/// Contain fits the whole image inside, cover fills the bounds; both keep the
/// aspect ratio and center the result. Degenerate inputs fall back to the
/// node bounds so callers always receive a paintable rect.
pub fn fit_rect(bounds: Rect, img_w: u32, img_h: u32, fit: ImageFit) -> Rect {
    if img_w == 0 || img_h == 0 || bounds.size.x <= 0.0 || bounds.size.y <= 0.0 {
        return bounds;
    }
    let scale = match fit {
        ImageFit::Contain => (bounds.size.x / img_w as f32).min(bounds.size.y / img_h as f32),
        ImageFit::Cover => (bounds.size.x / img_w as f32).max(bounds.size.y / img_h as f32),
    };
    if !scale.is_finite() || scale <= 0.0 {
        return bounds;
    }
    let width = img_w as f32 * scale;
    let height = img_h as f32 * scale;
    let origin = Point2::new(
        bounds.origin.x + (bounds.size.x - width) / 2.0,
        bounds.origin.y + (bounds.size.y - height) / 2.0,
    );
    Rect::new(origin, Vec2::new(width, height))
}

/// Node body rectangle from its plan origin and side.
pub fn node_bounds(origin: Point2, side: f32) -> Rect {
    Rect::new(origin, Vec2::new(side.max(1.0), side.max(1.0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(path: &str, fit: ImageFit) -> NodeImage {
        NodeImage::new(path.to_string(), fit)
    }

    #[test]
    fn unsupported_paths_never_enter_the_cache() {
        let mut cache = ImageCache::new();
        assert!(!image("", ImageFit::Contain).is_supported());
        assert!(!image("https://example.com/a.png", ImageFit::Cover).is_supported());
        assert!(!image("http://example.com/a.png", ImageFit::Contain).is_supported());
        assert!(image("/tmp/a.png", ImageFit::Contain).is_supported());
        assert!(!cache.request(&image("", ImageFit::Contain)));
        assert!(!cache.request(&image("https://example.com/a.png", ImageFit::Contain)));
        assert!(cache.is_empty());
    }

    #[test]
    fn repeated_requests_hit_without_redecoding() {
        let mut cache = ImageCache::new();
        let spec = image("/tmp/a.png", ImageFit::Cover);
        assert!(cache.request(&spec));
        assert!(!cache.request(&spec));
        assert_eq!(cache.len(), 1);
        assert!(!cache.is_ready("/tmp/a.png"));
    }

    #[test]
    fn resolve_and_fail_each_notify_once() {
        let mut cache = ImageCache::new();
        assert!(cache.request(&image("/tmp/a.png", ImageFit::Contain)));
        assert!(cache.resolve("/tmp/a.png", 40, 20));
        assert!(cache.is_ready("/tmp/a.png"));
        assert!(!cache.resolve("/tmp/a.png", 40, 20));
        assert!(cache.request(&image("/tmp/b.png", ImageFit::Contain)));
        assert!(cache.fail("/tmp/b.png", "missing file"));
        assert_eq!(cache.diagnostic("/tmp/b.png"), Some("missing file"));
        assert!(!cache.fail("/tmp/b.png", "missing file"));
        assert!(!cache.is_ready("/tmp/b.png"));
    }

    #[test]
    fn zero_sized_decodes_fall_back() {
        let mut cache = ImageCache::new();
        assert!(cache.request(&image("/tmp/z.png", ImageFit::Contain)));
        assert!(cache.resolve("/tmp/z.png", 0, 0));
        assert!(!cache.is_ready("/tmp/z.png"));
        assert!(cache.diagnostic("/tmp/z.png").is_some());
    }

    #[test]
    fn contain_fits_inside_and_cover_fills_bounds() {
        let bounds = Rect::new(Point2::new(0.0, 0.0), Vec2::new(24.0, 24.0));
        let contain = fit_rect(bounds, 40, 20, ImageFit::Contain);
        assert!((contain.size.x - 24.0).abs() < 1e-3);
        assert!((contain.size.y - 12.0).abs() < 1e-3);
        assert!((contain.origin.y - 6.0).abs() < 1e-3);
        let cover = fit_rect(bounds, 40, 20, ImageFit::Cover);
        assert!((cover.size.x - 48.0).abs() < 1e-3);
        assert!((cover.size.y - 24.0).abs() < 1e-3);
        assert!(cover.origin.x < 0.0);
        let tall_contain = fit_rect(bounds, 20, 40, ImageFit::Contain);
        assert!((tall_contain.size.x - 12.0).abs() < 1e-3);
        assert!((tall_contain.size.y - 24.0).abs() < 1e-3);
    }

    #[test]
    fn degenerate_inputs_fall_back_to_bounds() {
        let bounds = Rect::new(Point2::new(2.0, 3.0), Vec2::new(24.0, 24.0));
        assert_eq!(fit_rect(bounds, 0, 10, ImageFit::Contain), bounds);
        assert_eq!(fit_rect(bounds, 10, 0, ImageFit::Cover), bounds);
    }
}
