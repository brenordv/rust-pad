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

#[cfg(test)]
mod tests {
    use super::*;

    // ── hash_str ───────────────────────────────────────────────────

    #[test]
    fn hash_str_deterministic() {
        let h1 = hash_str("hello world");
        let h2 = hash_str("hello world");
        assert_eq!(h1, h2);
    }

    #[test]
    fn hash_str_different_strings() {
        assert_ne!(hash_str("abc"), hash_str("def"));
    }

    #[test]
    fn hash_str_empty() {
        // Should not panic, returns the FNV offset basis
        let h = hash_str("");
        assert_eq!(h, 0xcbf29ce484222325);
    }

    #[test]
    fn hash_str_single_char_differs() {
        assert_ne!(hash_str("a"), hash_str("b"));
    }

    #[test]
    fn hash_str_similar_strings_differ() {
        // Near-identical strings should produce different hashes
        assert_ne!(hash_str("hello world"), hash_str("hello worle"));
    }

    #[test]
    fn hash_str_unicode() {
        let h1 = hash_str("café");
        let h2 = hash_str("café");
        assert_eq!(h1, h2);
        assert_ne!(hash_str("café"), hash_str("cafe"));
    }

    // ── RenderCache ────────────────────────────────────────────────

    #[test]
    fn cache_new_is_empty() {
        let cache = RenderCache::new();
        assert!(cache.get(0, 123).is_none());
    }

    /// Creates a test galley using egui's font system.
    fn test_galley() -> Arc<Galley> {
        let ctx = egui::Context::default();
        // Must call run() once to initialize fonts
        let _ = ctx.run(egui::RawInput::default(), |_| {});
        ctx.fonts_mut(|fonts| {
            let job = egui::text::LayoutJob::simple_singleline(
                "test".to_string(),
                egui::FontId::monospace(14.0),
                egui::Color32::WHITE,
            );
            fonts.layout_job(job)
        })
    }

    #[test]
    fn cache_validate_version_change_clears() {
        let mut cache = RenderCache::new();
        cache.validate(1, 14.0);

        cache.insert(0, 42, test_galley());
        assert!(cache.get(0, 42).is_some());

        // Same version → preserved
        cache.validate(1, 14.0);
        assert!(cache.get(0, 42).is_some());

        // Different version → cleared
        cache.validate(2, 14.0);
        assert!(cache.get(0, 42).is_none());
    }

    #[test]
    fn cache_validate_font_size_change_clears() {
        let mut cache = RenderCache::new();
        cache.validate(1, 14.0);

        cache.insert(0, 42, test_galley());
        assert!(cache.get(0, 42).is_some());

        // Different font size → cleared
        cache.validate(1, 16.0);
        assert!(cache.get(0, 42).is_none());
    }

    #[test]
    fn cache_get_mismatched_hash() {
        let mut cache = RenderCache::new();
        cache.validate(1, 14.0);

        cache.insert(0, 42, test_galley());

        // Correct line, wrong hash
        assert!(cache.get(0, 99).is_none());
        // Wrong line, correct hash
        assert!(cache.get(1, 42).is_none());
        // Correct line and hash
        assert!(cache.get(0, 42).is_some());
    }

    #[test]
    fn cache_insert_and_get() {
        let mut cache = RenderCache::new();
        cache.validate(1, 14.0);

        cache.insert(5, 100, test_galley());

        assert!(cache.get(5, 100).is_some());
        assert!(cache.get(5, 101).is_none());
    }

    #[test]
    fn cache_insert_overwrites_same_line() {
        let mut cache = RenderCache::new();
        cache.validate(1, 14.0);

        cache.insert(0, 42, test_galley());
        cache.insert(0, 99, test_galley());

        assert!(cache.get(0, 42).is_none());
        assert!(cache.get(0, 99).is_some());
    }

    // ── get_render_cache ───────────────────────────────────────────

    #[test]
    fn get_render_cache_creates_new_when_none() {
        let mut slot: Option<Box<dyn std::any::Any + Send>> = None;
        let cache = get_render_cache(&mut slot);
        assert!(cache.get(0, 0).is_none()); // fresh cache is empty
    }

    #[test]
    fn get_render_cache_reuses_existing() {
        let mut slot: Option<Box<dyn std::any::Any + Send>> = None;

        {
            let cache = get_render_cache(&mut slot);
            cache.validate(1, 14.0);
            cache.insert(5, 100, test_galley());
        }

        {
            let cache = get_render_cache(&mut slot);
            assert!(cache.get(5, 100).is_some());
        }
    }

    #[test]
    fn get_render_cache_replaces_wrong_type() {
        let mut slot: Option<Box<dyn std::any::Any + Send>> = Some(Box::new(42u32));
        let cache = get_render_cache(&mut slot);
        assert!(cache.get(0, 0).is_none());
    }
}
