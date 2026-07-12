//! Sanitization for user- or filesystem-sourced strings rendered in chrome
//! surfaces (breadcrumb segments, the workspace header label).

/// Replaces control characters and Unicode bidi overrides with U+FFFD.
///
/// A name carrying an RLO/LRO override could visually reorder a strip and
/// spoof its extension; a newline would break a single-line layout. Callers
/// that pair the text with a byte-derived hint (e.g. the breadcrumb's
/// extension hint) keep that hint unsanitized so it stays the honest
/// counter-signal.
pub(crate) fn sanitize_display_text(raw: &str) -> String {
    raw.chars()
        .map(|c| {
            let bidi = matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}');
            if c.is_control() || bidi {
                '\u{FFFD}'
            } else {
                c
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_characters_are_replaced() {
        assert_eq!(sanitize_display_text("a\nb"), "a\u{FFFD}b");
        assert_eq!(sanitize_display_text("tab\there"), "tab\u{FFFD}here");
    }

    #[test]
    fn bidi_overrides_are_replaced() {
        assert_eq!(
            sanitize_display_text("evil\u{202E}txt.exe"),
            "evil\u{FFFD}txt.exe"
        );
        assert_eq!(sanitize_display_text("iso\u{2066}late"), "iso\u{FFFD}late");
    }

    #[test]
    fn plain_unicode_passes_through() {
        assert_eq!(sanitize_display_text("café-ノート"), "café-ノート");
    }

    #[test]
    fn empty_string_stays_empty() {
        assert_eq!(sanitize_display_text(""), "");
    }
}
