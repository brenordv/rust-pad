//! Synchronized scrolling between the two panes of a split view.
//!
//! When [`App::sync_scroll_enabled`] is on and the editor is in split-pane
//! mode, vertical (and optionally horizontal) scroll deltas applied to one
//! pane during a frame are mirrored on the other pane the next frame.
//!
//! The propagation runs *after* both panes have rendered, so it observes
//! whatever the editor widgets wrote to `Document::scroll_y` /
//! `Document::scroll_x` (mouse wheel, scrollbar drag, etc.). Programmatic
//! scrolls — Go to Line, Find/Replace navigation, bookmark jumps — are
//! filtered out via [`rust_pad_core::document::ScrollOrigin`]: only writes
//! tagged `UserInput` are propagated.
//!
//! Drift mitigation: each pane's `scroll_y`/`scroll_x` is clamped by its own
//! widget on the next frame, so deltas that would push one pane past its
//! content boundary are silently capped. This is the documented "delta mode"
//! behaviour from `12-synchronized-scrolling.md`.

use rust_pad_core::document::ScrollOrigin;

use crate::tabs::PaneId;

use super::App;

/// Snapshot of both panes' scroll offsets at the end of the previous frame.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SyncScrollSnapshot {
    pub left: (f32, f32),
    pub right: (f32, f32),
}

impl App {
    /// Captures the current `(scroll_y, scroll_x)` of each pane's active
    /// document. Called from the propagation step before/after mirroring.
    fn snapshot_panes(&self) -> SyncScrollSnapshot {
        let left = {
            let d = self.tabs.pane_active_doc_ref(PaneId::Left);
            (d.scroll_y, d.scroll_x)
        };
        let right = {
            let d = self.tabs.pane_active_doc_ref(PaneId::Right);
            (d.scroll_y, d.scroll_x)
        };
        SyncScrollSnapshot { left, right }
    }

    /// Mirrors user-initiated scroll deltas from the focused pane to the
    /// other pane. Called from the per-frame UI loop after the central
    /// panel has rendered.
    pub(crate) fn propagate_sync_scroll(&mut self) {
        // Inactive cases: clear the snapshot so a future enable starts
        // measuring deltas from a fresh baseline rather than from a stale
        // pre-disable position.
        if !self.sync_scroll_enabled || !self.is_split() {
            self.sync_scroll_last = None;
            return;
        }

        let now = self.snapshot_panes();
        let prev = self.sync_scroll_last;

        if let Some(prev) = prev {
            let focused = self.tabs.focused_pane();
            // Only propagate if the focused pane's most recent write was
            // tagged as user input. Programmatic jumps (Goto / Find /
            // Bookmark) keep the other pane in place.
            let src_origin = self.tabs.pane_active_doc_ref(focused).scroll_origin;
            if src_origin == ScrollOrigin::UserInput {
                let (src_prev, src_now, dst_pane) = match focused {
                    PaneId::Left => (prev.left, now.left, PaneId::Right),
                    PaneId::Right => (prev.right, now.right, PaneId::Left),
                };
                let dy = src_now.0 - src_prev.0;
                let dx = if self.sync_scroll_horizontal {
                    src_now.1 - src_prev.1
                } else {
                    0.0
                };
                if dy != 0.0 || dx != 0.0 {
                    let dst = self.tabs.pane_active_doc_mut(dst_pane);
                    if dy != 0.0 {
                        dst.scroll_y = (dst.scroll_y + dy).max(0.0);
                    }
                    if dx != 0.0 {
                        dst.scroll_x = (dst.scroll_x + dx).max(0.0);
                    }
                    // The destination pane's `clamp_scroll_values` (next
                    // frame) caps any overshoot — we don't need to know
                    // its content height here.
                }
            }
        }

        // Re-snapshot AFTER propagation so next frame's delta is measured
        // from the post-mirror state (otherwise mirrored writes would
        // double-count themselves).
        self.sync_scroll_last = Some(self.snapshot_panes());
    }

    /// Toggles synchronized scrolling on or off. Used by the View menu
    /// entry and the Ctrl+Alt+S shortcut.
    pub(crate) fn toggle_sync_scroll(&mut self) {
        self.sync_scroll_enabled = !self.sync_scroll_enabled;
        if !self.sync_scroll_enabled {
            self.sync_scroll_last = None;
        }
    }
}
