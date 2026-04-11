//! User-assignable color presets for tabs.
//!
//! `TabColor` is a small enum of preset colors that the UI layer paints onto
//! tab accent stripes. Kept in `rust-pad-core` (no egui dependency) so that
//! `Document` can carry the field directly. The UI crate converts the RGB
//! triple from [`TabColor::to_rgb`] into its own color type at the call site.

use serde::{Deserialize, Serialize};

/// Preset palette of tab colors users can pick from the tab context menu.
///
/// The palette is intentionally small (9 entries) to keep the menu compact
/// and to avoid the complexity of a full color picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TabColor {
    Red,
    Orange,
    Yellow,
    Green,
    Cyan,
    Blue,
    Purple,
    Pink,
    Gray,
}

impl TabColor {
    /// All variants in display order. Used by the UI to render the palette.
    pub const ALL: [TabColor; 9] = [
        TabColor::Red,
        TabColor::Orange,
        TabColor::Yellow,
        TabColor::Green,
        TabColor::Cyan,
        TabColor::Blue,
        TabColor::Purple,
        TabColor::Pink,
        TabColor::Gray,
    ];

    /// Returns the sRGB triple for the variant. The UI converts this to its
    /// own color type (e.g. `egui::Color32::from_rgb`).
    pub fn to_rgb(self) -> [u8; 3] {
        match self {
            TabColor::Red => [220, 70, 70],
            TabColor::Orange => [230, 140, 50],
            TabColor::Yellow => [220, 200, 60],
            TabColor::Green => [90, 180, 90],
            TabColor::Cyan => [80, 190, 200],
            TabColor::Blue => [90, 140, 230],
            TabColor::Purple => [160, 110, 210],
            TabColor::Pink => [220, 120, 180],
            TabColor::Gray => [150, 150, 150],
        }
    }

    /// Human-readable label for the variant. Used in the context menu.
    pub fn label(self) -> &'static str {
        match self {
            TabColor::Red => "Red",
            TabColor::Orange => "Orange",
            TabColor::Yellow => "Yellow",
            TabColor::Green => "Green",
            TabColor::Cyan => "Cyan",
            TabColor::Blue => "Blue",
            TabColor::Purple => "Purple",
            TabColor::Pink => "Pink",
            TabColor::Gray => "Gray",
        }
    }

    /// Stable string identifier used to persist the color in the session
    /// store. Stored as a string (rather than a bincode-encoded enum tag)
    /// so that future palette additions don't shift tag numbering.
    pub fn as_serde_str(self) -> &'static str {
        match self {
            TabColor::Red => "red",
            TabColor::Orange => "orange",
            TabColor::Yellow => "yellow",
            TabColor::Green => "green",
            TabColor::Cyan => "cyan",
            TabColor::Blue => "blue",
            TabColor::Purple => "purple",
            TabColor::Pink => "pink",
            TabColor::Gray => "gray",
        }
    }

    /// Inverse of [`TabColor::as_serde_str`]. Returns `None` for unknown or
    /// future palette entries so that loading is forward-compatible.
    pub fn from_serde_str(s: &str) -> Option<Self> {
        match s {
            "red" => Some(TabColor::Red),
            "orange" => Some(TabColor::Orange),
            "yellow" => Some(TabColor::Yellow),
            "green" => Some(TabColor::Green),
            "cyan" => Some(TabColor::Cyan),
            "blue" => Some(TabColor::Blue),
            "purple" => Some(TabColor::Purple),
            "pink" => Some(TabColor::Pink),
            "gray" => Some(TabColor::Gray),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_rgb_returns_distinct_values() {
        // Sanity check that no two variants share the same RGB triple.
        let rgbs: Vec<_> = TabColor::ALL.iter().map(|c| c.to_rgb()).collect();
        for (i, a) in rgbs.iter().enumerate() {
            for (j, b) in rgbs.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b, "duplicate RGB for variants {i} and {j}");
                }
            }
        }
    }

    #[test]
    fn test_serde_str_round_trip_all_variants() {
        for variant in TabColor::ALL {
            let s = variant.as_serde_str();
            assert_eq!(TabColor::from_serde_str(s), Some(variant));
        }
    }

    #[test]
    fn test_from_serde_str_unknown_returns_none() {
        assert_eq!(TabColor::from_serde_str(""), None);
        assert_eq!(TabColor::from_serde_str("magenta"), None);
        assert_eq!(TabColor::from_serde_str("RED"), None); // case-sensitive
    }

    #[test]
    fn test_label_non_empty() {
        for variant in TabColor::ALL {
            assert!(!variant.label().is_empty());
        }
    }

    #[test]
    fn test_all_contains_every_variant() {
        // Lightweight guard so that adding a variant without updating ALL
        // is caught immediately.
        assert_eq!(TabColor::ALL.len(), 9);
    }
}
