/// Encoding detection and conversion for file I/O.
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Supported text encodings.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextEncoding {
    #[default]
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
    Ascii,
    /// A named encoding from `encoding_rs` (e.g., "windows-1252").
    Legacy(&'static str),
}

impl std::fmt::Display for TextEncoding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Utf8 => write!(f, "UTF-8"),
            Self::Utf8Bom => write!(f, "UTF-8 BOM"),
            Self::Utf16Le => write!(f, "UTF-16 LE"),
            Self::Utf16Be => write!(f, "UTF-16 BE"),
            Self::Ascii => write!(f, "ASCII"),
            Self::Legacy(name) => write!(f, "{name}"),
        }
    }
}

/// Line ending format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LineEnding {
    /// `\n` (Unix/macOS)
    Lf,
    /// `\r\n` (Windows)
    CrLf,
    /// `\r` (Classic Mac)
    Cr,
}

impl Default for LineEnding {
    fn default() -> Self {
        if cfg!(windows) {
            Self::CrLf
        } else {
            Self::Lf
        }
    }
}

impl std::fmt::Display for LineEnding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Lf => write!(f, "LF"),
            Self::CrLf => write!(f, "CRLF"),
            Self::Cr => write!(f, "CR"),
        }
    }
}

impl LineEnding {
    /// Returns the string representation of this line ending.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::CrLf => "\r\n",
            Self::Cr => "\r",
        }
    }
}

/// Detects the encoding of raw bytes.
pub fn detect_encoding(bytes: &[u8]) -> TextEncoding {
    // Check BOM first
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return TextEncoding::Utf8Bom;
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        return TextEncoding::Utf16Le;
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        return TextEncoding::Utf16Be;
    }

    // Try UTF-8
    if std::str::from_utf8(bytes).is_ok() {
        // Check if pure ASCII
        if bytes.iter().all(|&b| b < 128) {
            return TextEncoding::Ascii;
        }
        return TextEncoding::Utf8;
    }

    // Use chardetng for other encodings
    let mut detector = chardetng::EncodingDetector::new();
    detector.feed(bytes, true);
    let encoding = detector.guess(None, true);
    TextEncoding::Legacy(encoding.name())
}

/// Detects the line ending style from text content.
pub fn detect_line_ending(text: &str) -> LineEnding {
    if text.contains("\r\n") {
        LineEnding::CrLf
    } else if text.contains('\r') {
        LineEnding::Cr
    } else {
        LineEnding::Lf
    }
}

/// Decodes raw bytes into a String using the specified encoding.
///
/// # Errors
///
/// Returns an error if decoding fails.
pub fn decode_bytes(bytes: &[u8], encoding: TextEncoding) -> Result<String> {
    match encoding {
        TextEncoding::Utf8 => String::from_utf8(bytes.to_vec()).context("invalid UTF-8 content"),
        TextEncoding::Utf8Bom => {
            let content = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
                &bytes[3..]
            } else {
                bytes
            };
            String::from_utf8(content.to_vec()).context("invalid UTF-8 BOM content")
        }
        TextEncoding::Ascii => String::from_utf8(bytes.to_vec()).context("invalid ASCII content"),
        TextEncoding::Utf16Le => {
            let content = if bytes.starts_with(&[0xFF, 0xFE]) {
                &bytes[2..]
            } else {
                bytes
            };
            let u16s: Vec<u16> = content
                .chunks_exact(2)
                .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
                .collect();
            String::from_utf16(&u16s).context("invalid UTF-16 LE content")
        }
        TextEncoding::Utf16Be => {
            let content = if bytes.starts_with(&[0xFE, 0xFF]) {
                &bytes[2..]
            } else {
                bytes
            };
            let u16s: Vec<u16> = content
                .chunks_exact(2)
                .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]]))
                .collect();
            String::from_utf16(&u16s).context("invalid UTF-16 BE content")
        }
        TextEncoding::Legacy(name) => {
            let encoding = encoding_rs::Encoding::for_label(name.as_bytes())
                .context(format!("unknown encoding: {name}"))?;
            let (decoded, _, had_errors) = encoding.decode(bytes);
            if had_errors {
                anyhow::bail!("encoding errors while decoding as {name}");
            }
            Ok(decoded.into_owned())
        }
    }
}

/// Encodes a string into bytes using the specified encoding.
///
/// # Errors
///
/// Returns an error if encoding fails.
pub fn encode_string(text: &str, encoding: TextEncoding) -> Result<Vec<u8>> {
    match encoding {
        TextEncoding::Utf8 | TextEncoding::Ascii => Ok(text.as_bytes().to_vec()),
        TextEncoding::Utf8Bom => {
            let mut bytes = vec![0xEF, 0xBB, 0xBF];
            bytes.extend_from_slice(text.as_bytes());
            Ok(bytes)
        }
        TextEncoding::Utf16Le => {
            let mut bytes = vec![0xFF, 0xFE]; // BOM
            for code_unit in text.encode_utf16() {
                bytes.extend_from_slice(&code_unit.to_le_bytes());
            }
            Ok(bytes)
        }
        TextEncoding::Utf16Be => {
            let mut bytes = vec![0xFE, 0xFF]; // BOM
            for code_unit in text.encode_utf16() {
                bytes.extend_from_slice(&code_unit.to_be_bytes());
            }
            Ok(bytes)
        }
        TextEncoding::Legacy(name) => {
            let encoding = encoding_rs::Encoding::for_label(name.as_bytes())
                .context(format!("unknown encoding: {name}"))?;
            let (encoded, _, had_errors) = encoding.encode(text);
            if had_errors {
                anyhow::bail!("encoding errors while encoding as {name}");
            }
            Ok(encoded.into_owned())
        }
    }
}

/// Normalizes line endings to `\n` (LF).
pub fn normalize_line_endings(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

/// Converts all `\n` in the text to the specified line ending.
pub fn apply_line_ending(text: &str, ending: LineEnding) -> String {
    match ending {
        LineEnding::Lf => text.to_string(),
        LineEnding::CrLf => text.replace('\n', "\r\n"),
        LineEnding::Cr => text.replace('\n', "\r"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_utf8() {
        let text = "hello world".as_bytes();
        assert!(matches!(detect_encoding(text), TextEncoding::Ascii));
    }

    #[test]
    fn test_detect_utf8_bom() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice("hello".as_bytes());
        assert_eq!(detect_encoding(&bytes), TextEncoding::Utf8Bom);
    }

    #[test]
    fn test_detect_line_ending() {
        assert_eq!(detect_line_ending("hello\nworld"), LineEnding::Lf);
        assert_eq!(detect_line_ending("hello\r\nworld"), LineEnding::CrLf);
        assert_eq!(detect_line_ending("hello\rworld"), LineEnding::Cr);
    }

    #[test]
    fn test_normalize_line_endings() {
        assert_eq!(normalize_line_endings("a\r\nb\rc"), "a\nb\nc");
    }

    #[test]
    fn test_apply_line_ending() {
        assert_eq!(
            apply_line_ending("a\nb\nc", LineEnding::CrLf),
            "a\r\nb\r\nc"
        );
        assert_eq!(apply_line_ending("a\nb\nc", LineEnding::Cr), "a\rb\rc");
    }

    #[test]
    fn test_roundtrip_utf8() {
        let text = "héllo wörld";
        let bytes = encode_string(text, TextEncoding::Utf8).unwrap();
        let decoded = decode_bytes(&bytes, TextEncoding::Utf8).unwrap();
        assert_eq!(decoded, text);
    }

    #[test]
    fn test_roundtrip_utf16le() {
        let text = "hello";
        let bytes = encode_string(text, TextEncoding::Utf16Le).unwrap();
        let decoded = decode_bytes(&bytes, TextEncoding::Utf16Le).unwrap();
        assert_eq!(decoded, text);
    }

    // ── Roundtrip: UTF-16 BE ─────────────────────────────────────────

    #[test]
    fn test_roundtrip_utf16be() {
        let text = "hello world";
        let bytes = encode_string(text, TextEncoding::Utf16Be).unwrap();
        let decoded = decode_bytes(&bytes, TextEncoding::Utf16Be).unwrap();
        assert_eq!(decoded, text);
    }

    // ── Roundtrip: UTF-8 BOM ─────────────────────────────────────────

    #[test]
    fn test_roundtrip_utf8_bom() {
        let text = "héllo wörld";
        let bytes = encode_string(text, TextEncoding::Utf8Bom).unwrap();
        // Encoded bytes should start with BOM
        assert_eq!(&bytes[..3], &[0xEF, 0xBB, 0xBF]);
        let decoded = decode_bytes(&bytes, TextEncoding::Utf8Bom).unwrap();
        assert_eq!(decoded, text);
    }

    // ── Roundtrip: ASCII ─────────────────────────────────────────────

    #[test]
    fn test_roundtrip_ascii() {
        let text = "hello world 123";
        let bytes = encode_string(text, TextEncoding::Ascii).unwrap();
        let decoded = decode_bytes(&bytes, TextEncoding::Ascii).unwrap();
        assert_eq!(decoded, text);
    }

    // ── Roundtrip: Unicode / emoji ───────────────────────────────────

    #[test]
    fn test_roundtrip_utf8_emoji() {
        let text = "Hello 🌍🎉 日本語";
        let bytes = encode_string(text, TextEncoding::Utf8).unwrap();
        let decoded = decode_bytes(&bytes, TextEncoding::Utf8).unwrap();
        assert_eq!(decoded, text);
    }

    #[test]
    fn test_roundtrip_utf16le_emoji() {
        let text = "Hello 🌍🎉";
        let bytes = encode_string(text, TextEncoding::Utf16Le).unwrap();
        let decoded = decode_bytes(&bytes, TextEncoding::Utf16Le).unwrap();
        assert_eq!(decoded, text);
    }

    #[test]
    fn test_roundtrip_utf16be_emoji() {
        let text = "日本語 中文 한국어";
        let bytes = encode_string(text, TextEncoding::Utf16Be).unwrap();
        let decoded = decode_bytes(&bytes, TextEncoding::Utf16Be).unwrap();
        assert_eq!(decoded, text);
    }

    // ── Display traits ───────────────────────────────────────────────

    #[test]
    fn test_text_encoding_display() {
        assert_eq!(TextEncoding::Utf8.to_string(), "UTF-8");
        assert_eq!(TextEncoding::Utf8Bom.to_string(), "UTF-8 BOM");
        assert_eq!(TextEncoding::Utf16Le.to_string(), "UTF-16 LE");
        assert_eq!(TextEncoding::Utf16Be.to_string(), "UTF-16 BE");
        assert_eq!(TextEncoding::Ascii.to_string(), "ASCII");
        assert_eq!(
            TextEncoding::Legacy("windows-1252").to_string(),
            "windows-1252"
        );
    }

    #[test]
    fn test_line_ending_display() {
        assert_eq!(LineEnding::Lf.to_string(), "LF");
        assert_eq!(LineEnding::CrLf.to_string(), "CRLF");
        assert_eq!(LineEnding::Cr.to_string(), "CR");
    }

    // ── as_str() ─────────────────────────────────────────────────────

    #[test]
    fn test_line_ending_as_str() {
        assert_eq!(LineEnding::Lf.as_str(), "\n");
        assert_eq!(LineEnding::CrLf.as_str(), "\r\n");
        assert_eq!(LineEnding::Cr.as_str(), "\r");
    }

    // ── Default traits ───────────────────────────────────────────────

    #[test]
    fn test_text_encoding_default() {
        assert_eq!(TextEncoding::default(), TextEncoding::Utf8);
    }

    #[test]
    fn test_line_ending_default() {
        let default = LineEnding::default();
        if cfg!(windows) {
            assert_eq!(default, LineEnding::CrLf);
        } else {
            assert_eq!(default, LineEnding::Lf);
        }
    }

    // ── detect_encoding edge cases ───────────────────────────────────

    #[test]
    fn test_detect_encoding_utf16le() {
        let mut bytes = vec![0xFF, 0xFE]; // LE BOM
        bytes.extend_from_slice(&[0x48, 0x00]); // 'H' in UTF-16 LE
        assert_eq!(detect_encoding(&bytes), TextEncoding::Utf16Le);
    }

    #[test]
    fn test_detect_encoding_utf16be() {
        let mut bytes = vec![0xFE, 0xFF]; // BE BOM
        bytes.extend_from_slice(&[0x00, 0x48]); // 'H' in UTF-16 BE
        assert_eq!(detect_encoding(&bytes), TextEncoding::Utf16Be);
    }

    #[test]
    fn test_detect_encoding_pure_ascii() {
        let bytes = b"hello world 123";
        assert_eq!(detect_encoding(bytes), TextEncoding::Ascii);
    }

    #[test]
    fn test_detect_encoding_utf8_non_ascii() {
        let bytes = "héllo".as_bytes();
        assert_eq!(detect_encoding(bytes), TextEncoding::Utf8);
    }

    #[test]
    fn test_detect_encoding_empty() {
        assert_eq!(detect_encoding(&[]), TextEncoding::Ascii);
    }

    // ── detect_line_ending edge cases ────────────────────────────────

    #[test]
    fn test_detect_line_ending_no_newlines() {
        assert_eq!(detect_line_ending("hello"), LineEnding::Lf);
    }

    #[test]
    fn test_detect_line_ending_mixed_prefers_crlf() {
        // Contains both \r\n and bare \n — \r\n check comes first
        assert_eq!(detect_line_ending("a\r\nb\nc"), LineEnding::CrLf);
    }

    // ── normalize and apply round-trip ───────────────────────────────

    #[test]
    fn test_normalize_crlf_only() {
        assert_eq!(normalize_line_endings("a\r\nb\r\nc"), "a\nb\nc");
    }

    #[test]
    fn test_normalize_cr_only() {
        assert_eq!(normalize_line_endings("a\rb\rc"), "a\nb\nc");
    }

    #[test]
    fn test_normalize_lf_no_change() {
        assert_eq!(normalize_line_endings("a\nb\nc"), "a\nb\nc");
    }

    #[test]
    fn test_apply_line_ending_lf_noop() {
        assert_eq!(apply_line_ending("a\nb\nc", LineEnding::Lf), "a\nb\nc");
    }

    #[test]
    fn test_normalize_then_apply_roundtrip() {
        let original = "a\r\nb\r\nc";
        let normalized = normalize_line_endings(original);
        let restored = apply_line_ending(&normalized, LineEnding::CrLf);
        assert_eq!(restored, original);
    }

    // ── decode_bytes edge cases ──────────────────────────────────────

    #[test]
    fn test_decode_utf8_bom_without_bom() {
        // decode_bytes with Utf8Bom should work even without actual BOM
        let bytes = "hello".as_bytes();
        let decoded = decode_bytes(bytes, TextEncoding::Utf8Bom).unwrap();
        assert_eq!(decoded, "hello");
    }

    #[test]
    fn test_decode_empty_bytes() {
        assert_eq!(decode_bytes(&[], TextEncoding::Utf8).unwrap(), "");
        assert_eq!(decode_bytes(&[], TextEncoding::Ascii).unwrap(), "");
    }

    // ── encode_string BOM inclusion ──────────────────────────────────

    #[test]
    fn test_encode_utf16le_includes_bom() {
        let bytes = encode_string("A", TextEncoding::Utf16Le).unwrap();
        assert_eq!(&bytes[..2], &[0xFF, 0xFE]); // LE BOM
    }

    #[test]
    fn test_encode_utf16be_includes_bom() {
        let bytes = encode_string("A", TextEncoding::Utf16Be).unwrap();
        assert_eq!(&bytes[..2], &[0xFE, 0xFF]); // BE BOM
    }
}
