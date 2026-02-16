//! Galley render cache for the editor widget.
//!
//! Caches egui `Galley` layouts per logical line so that unchanged lines
//! skip syntax highlighting and text layout on subsequent frames.

use std::collections::HashMap;
use std::sync::Arc;

use egui::Galley;

/// Cached galley for a single line.
struct CachedGalley {
    /// Hash of the line content (used to detect changes).
    content_hash: u64,
    /// The cached galley.
    galley: Arc<Galley>,
}

/// Per-document render cache stored as `Box<dyn Any + Send>` on `Document::render_cache`.
pub(crate) struct RenderCache {
    /// Cached galleys keyed by logical line index.
    galleys: HashMap<usize, CachedGalley>,
    /// Content version when the cache was last validated.
    last_version: u64,
    /// Font size used when galleys were cached (invalidate on zoom change).
    last_font_size: f32,
}

impl RenderCache {
    pub fn new() -> Self {
        Self {
            galleys: HashMap::new(),
            last_version: u64::MAX, // force miss on first use
            last_font_size: 0.0,
        }
    }

    /// Invalidates the entire cache if the version or font size changed.
    pub fn validate(&mut self, version: u64, font_size: f32) {
        if self.last_version != version || (self.last_font_size - font_size).abs() > f32::EPSILON {
            self.galleys.clear();
            self.last_version = version;
            self.last_font_size = font_size;
        }
    }

    /// Looks up a cached galley for a line.
    pub fn get(&self, line_idx: usize, content_hash: u64) -> Option<Arc<Galley>> {
        self.galleys.get(&line_idx).and_then(|entry| {
            if entry.content_hash == content_hash {
                Some(Arc::clone(&entry.galley))
            } else {
                None
            }
        })
    }

    /// Stores a galley in the cache.
    pub fn insert(&mut self, line_idx: usize, content_hash: u64, galley: Arc<Galley>) {
        self.galleys.insert(
            line_idx,
            CachedGalley {
                content_hash,
                galley,
            },
        );
    }
}

/// Retrieves or creates the `RenderCache` from a document's opaque `render_cache` slot.
pub(crate) fn get_render_cache(
    render_cache: &mut Option<Box<dyn std::any::Any + Send>>,
) -> &mut RenderCache {
    if render_cache.is_none()
        || render_cache
            .as_ref()
            .unwrap()
            .downcast_ref::<RenderCache>()
            .is_none()
    {
        *render_cache = Some(Box::new(RenderCache::new()));
    }
    render_cache
        .as_mut()
        .unwrap()
        .downcast_mut::<RenderCache>()
        .unwrap()
}

/// Simple FNV-1a hash for line content strings.
pub(crate) fn hash_str(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in s.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}
