//! Split-view (dual pane) types and per-pane tab ownership.
//!
//! When the editor is in single-pane mode, [`TabManager`] uses its flat
//! `documents` list directly. When the user enables a split, the manager
//! holds an additional [`PaneTabSplit`] that records which document indices
//! belong to each pane and which tab is active in each. The flat document
//! vector remains the source of truth for the documents themselves; pane
//! state only stores indices into it.
//!
//! [`TabManager`]: super::TabManager

/// Identifies one of the (up to) two panes in split view.
///
/// In horizontal splits, [`PaneId::Left`] is the **top** pane and
/// [`PaneId::Right`] is the **bottom** pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PaneId {
    /// The left (or, in horizontal mode, top) pane.
    Left,
    /// The right (or, in horizontal mode, bottom) pane.
    Right,
}

impl PaneId {
    /// Returns the opposite pane.
    pub fn other(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }
}

/// How the editor area is divided when split view is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitOrientation {
    /// Side-by-side: Left | Right.
    Vertical,
    /// Stacked: Left (top) over Right (bottom).
    Horizontal,
}

/// Per-pane tab assignment when split view is active.
///
/// `left_order` and `right_order` together partition the document indices
/// owned by the manager: every document appears in exactly one pane.
/// `left_active` / `right_active` are document indices (not positions
/// within `*_order`) and must always satisfy `*_order.contains(*_active)`.
#[derive(Debug, Clone)]
pub struct PaneTabSplit {
    /// Document indices belonging to the left pane, in display order.
    pub left_order: Vec<usize>,
    /// Document indices belonging to the right pane, in display order.
    pub right_order: Vec<usize>,
    /// Document index of the left pane's active tab.
    pub left_active: usize,
    /// Document index of the right pane's active tab.
    pub right_active: usize,
    /// Which pane currently owns global focus (menu/shortcut routing).
    pub focused: PaneId,
}

impl PaneTabSplit {
    /// Returns the active document index for the given pane.
    pub fn active_for(&self, pane: PaneId) -> usize {
        match pane {
            PaneId::Left => self.left_active,
            PaneId::Right => self.right_active,
        }
    }

    /// Returns the tab order slice for the given pane.
    pub fn order_for(&self, pane: PaneId) -> &[usize] {
        match pane {
            PaneId::Left => &self.left_order,
            PaneId::Right => &self.right_order,
        }
    }

    /// Returns a mutable reference to the tab order vec for the given pane.
    pub fn order_for_mut(&mut self, pane: PaneId) -> &mut Vec<usize> {
        match pane {
            PaneId::Left => &mut self.left_order,
            PaneId::Right => &mut self.right_order,
        }
    }

    /// Sets the active document index for the given pane.
    pub fn set_active(&mut self, pane: PaneId, doc_idx: usize) {
        match pane {
            PaneId::Left => self.left_active = doc_idx,
            PaneId::Right => self.right_active = doc_idx,
        }
    }

    /// Returns a mutable reference to the active-doc slot for the given pane.
    pub fn active_mut(&mut self, pane: PaneId) -> &mut usize {
        match pane {
            PaneId::Left => &mut self.left_active,
            PaneId::Right => &mut self.right_active,
        }
    }

    /// Returns the pane that currently owns the given document index, if any.
    pub fn pane_of(&self, doc_idx: usize) -> Option<PaneId> {
        if self.left_order.contains(&doc_idx) {
            Some(PaneId::Left)
        } else if self.right_order.contains(&doc_idx) {
            Some(PaneId::Right)
        } else {
            None
        }
    }
}
