//! Embedded monospace font used by the PDF print/export pipeline.
//!
//! The TTF is compiled into the binary via `include_bytes!` so the feature
//! works without touching any system font configuration and produces
//! deterministic output across platforms.
//!
//! Font: **DejaVu Sans Mono** (regular weight).
//! License: Bitstream Vera / DejaVu license — permissive, reproduced in
//! `THIRD_PARTY_LICENSES.md`.

/// Raw TTF bytes of the bundled monospace font.
pub const FONT_BYTES: &[u8] = include_bytes!("../../../assets/DejaVuSansMono.ttf");

/// Horizontal advance width of a single glyph in this font, expressed as a
/// fraction of the font size.
///
/// DejaVu Sans Mono's advance width is 1233 units on a 2048-unit em square,
/// which is `1233 / 2048 ≈ 0.602`. At a font size of N points, each
/// character therefore occupies `N * 0.602` points horizontally.
///
/// Layout code uses this to translate "available body width in points" into
/// "characters per line" without having to parse the font at runtime.
pub const CHAR_ADVANCE_RATIO: f32 = 0.602;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_bytes_are_ttf() {
        // TTF magic: 0x00010000
        assert!(FONT_BYTES.len() > 1024, "font file looks truncated");
        assert_eq!(
            &FONT_BYTES[..4],
            &[0x00, 0x01, 0x00, 0x00],
            "bundled font is not a valid TTF (wrong signature)"
        );
    }

    #[test]
    fn char_advance_ratio_is_sensible() {
        // Any reasonable monospace font sits in this range. Compile-time
        // assertion keeps clippy happy and catches typos at build time.
        const _: () = {
            assert!(CHAR_ADVANCE_RATIO > 0.4);
            assert!(CHAR_ADVANCE_RATIO < 0.8);
        };
    }
}
