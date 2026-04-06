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

    /// Validates the cache against the current font size.
    ///
    /// Font size changes require a full clear because every galley is
    /// font-dependent. Content version changes do **not** clear the cache —
    /// per-line content hashes in [`get`] already handle correctness by
    /// returning `None` when a line's content has changed.
    pub fn validate(&mut self, version: u64, font_size: f32) {
        // Font size change: must clear everything (galleys are font-dependent).
        if (self.last_font_size - font_size).abs() > f32::EPSILON {
            self.galleys.clear();
            self.last_font_size = font_size;
        }
        // Version change: do NOT clear. Per-line content hashes handle correctness.
        self.last_version = version;
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

    /// Removes cached entries whose line index falls outside the visible
    /// range extended by `margin` lines on each side.
    ///
    /// Call once per frame after rendering to bound memory usage when the
    /// cache is no longer cleared on every content-version change.
    pub fn prune(&mut self, first_visible: usize, last_visible: usize, margin: usize) {
        let lo = first_visible.saturating_sub(margin);
        let hi = last_visible.saturating_add(margin);
        self.galleys
            .retain(|&line_idx, _| line_idx >= lo && line_idx <= hi);
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
        let _ = ctx.run_ui(egui::RawInput::default(), |_| {});
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
    fn cache_validate_version_change_preserves_entries() {
        let mut cache = RenderCache::new();
        cache.validate(1, 14.0);

        cache.insert(0, 42, test_galley());
        assert!(cache.get(0, 42).is_some());

        // Same version → preserved
        cache.validate(1, 14.0);
        assert!(cache.get(0, 42).is_some());

        // Different version → still preserved (per-line hash guards correctness)
        cache.validate(2, 14.0);
        assert!(cache.get(0, 42).is_some());
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

    // ── Selective invalidation ────────────────────────────────────

    #[test]
    fn version_change_with_same_hash_is_cache_hit() {
        let mut cache = RenderCache::new();
        cache.validate(1, 14.0);
        cache.insert(10, 42, test_galley());

        // Bump version — line 10 content unchanged (same hash)
        cache.validate(2, 14.0);
        assert!(
            cache.get(10, 42).is_some(),
            "unchanged line should remain a cache hit after version bump"
        );
    }

    #[test]
    fn version_change_with_different_hash_is_cache_miss() {
        let mut cache = RenderCache::new();
        cache.validate(1, 14.0);
        cache.insert(10, 42, test_galley());

        // Bump version — line 10 content changed (different hash)
        cache.validate(2, 14.0);
        assert!(
            cache.get(10, 99).is_none(),
            "changed line should be a cache miss"
        );
    }

    #[test]
    fn line_shift_causes_miss_for_shifted_lines() {
        let mut cache = RenderCache::new();
        cache.validate(1, 14.0);

        // Lines 0-3 cached with distinct hashes.
        for i in 0..4 {
            cache.insert(i, 100 + i as u64, test_galley());
        }

        // Simulate inserting a line at index 1: old line 1 (hash 101) is now
        // at index 2. Querying index 2 with hash 101 should miss because the
        // cache still holds (index 2, hash 102).
        cache.validate(2, 14.0);
        assert!(
            cache.get(2, 101).is_none(),
            "shifted line should miss (wrong hash at old index)"
        );
        // The original index 0 hasn't shifted.
        assert!(cache.get(0, 100).is_some());
    }

    // ── prune ─────────────────────────────────────────────────────

    #[test]
    fn prune_removes_entries_outside_range() {
        let mut cache = RenderCache::new();
        cache.validate(1, 14.0);

        for i in 0..200 {
            cache.insert(i, i as u64, test_galley());
        }

        // Visible range 50..60, margin 10 → keep lines 40..70
        cache.prune(50, 60, 10);

        assert!(cache.get(39, 39).is_none(), "line below margin removed");
        assert!(cache.get(40, 40).is_some(), "line at lower margin kept");
        assert!(cache.get(55, 55).is_some(), "visible line kept");
        assert!(cache.get(70, 70).is_some(), "line at upper margin kept");
        assert!(cache.get(71, 71).is_none(), "line above margin removed");
    }

    #[test]
    fn prune_handles_start_of_file() {
        let mut cache = RenderCache::new();
        cache.validate(1, 14.0);

        for i in 0..100 {
            cache.insert(i, i as u64, test_galley());
        }

        // Visible range 0..5, margin 50 → keep lines 0..55
        cache.prune(0, 5, 50);

        assert!(cache.get(0, 0).is_some(), "first line kept");
        assert!(cache.get(55, 55).is_some(), "upper margin line kept");
        assert!(cache.get(56, 56).is_none(), "line past margin removed");
    }

    #[test]
    fn prune_with_zero_margin() {
        let mut cache = RenderCache::new();
        cache.validate(1, 14.0);

        for i in 0..10 {
            cache.insert(i, i as u64, test_galley());
        }

        cache.prune(3, 5, 0);

        assert!(cache.get(2, 2).is_none());
        assert!(cache.get(3, 3).is_some());
        assert!(cache.get(5, 5).is_some());
        assert!(cache.get(6, 6).is_none());
    }

    #[test]
    fn prune_on_empty_cache_is_noop() {
        let mut cache = RenderCache::new();
        cache.validate(1, 14.0);
        cache.prune(0, 10, 50); // should not panic
    }
}
