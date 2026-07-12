/// Hex color type with serde support for `"#RRGGBB"` / `"#RRGGBBAA"` strings.
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HexColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl HexColor {
    pub fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b, a: 255 }
    }

    pub fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub fn from_hex(s: &str) -> Option<Self> {
        let s = s.strip_prefix('#')?;
        // Non-ASCII input would make byte-indexed slicing panic mid-character;
        // reject it before any slicing happens (hex digits are always ASCII).
        if !s.is_ascii() {
            return None;
        }
        let channel = |range| u8::from_str_radix(s.get(range)?, 16).ok();
        match s.len() {
            6 => Some(Self {
                r: channel(0..2)?,
                g: channel(2..4)?,
                b: channel(4..6)?,
                a: 255,
            }),
            8 => Some(Self {
                r: channel(0..2)?,
                g: channel(2..4)?,
                b: channel(4..6)?,
                a: channel(6..8)?,
            }),
            _ => None,
        }
    }

    /// Linearly interpolates each RGB channel toward `other`.
    ///
    /// `t = 0.0` returns `self`, `t = 1.0` returns `other`. Alpha is kept
    /// from `self`.
    pub fn mix(self, other: HexColor, t: f32) -> HexColor {
        let t = t.clamp(0.0, 1.0);
        let lerp = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t) as u8;
        HexColor {
            r: lerp(self.r, other.r),
            g: lerp(self.g, other.g),
            b: lerp(self.b, other.b),
            a: self.a,
        }
    }

    /// Scales each RGB channel by `f` (clamped to valid range); alpha unchanged.
    pub fn scale_rgb(self, f: f32) -> HexColor {
        let scale = |c: u8| (f32::from(c) * f).round().clamp(0.0, 255.0) as u8;
        HexColor {
            r: scale(self.r),
            g: scale(self.g),
            b: scale(self.b),
            a: self.a,
        }
    }

    /// Returns the same color with a different alpha.
    pub fn with_alpha(self, a: u8) -> HexColor {
        HexColor { a, ..self }
    }

    /// Perceived relative luminance in `0.0..=1.0` (Rec. 601 weights).
    pub fn luminance(self) -> f32 {
        (0.299 * f32::from(self.r) + 0.587 * f32::from(self.g) + 0.114 * f32::from(self.b)) / 255.0
    }

    pub fn to_hex(self) -> String {
        if self.a == 255 {
            format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
        } else {
            format!("#{:02X}{:02X}{:02X}{:02X}", self.r, self.g, self.b, self.a)
        }
    }
}

impl Serialize for HexColor {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for HexColor {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Self::from_hex(&s)
            .ok_or_else(|| serde::de::Error::custom(format!("invalid hex color: {s}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_rgb() {
        let c = HexColor::from_hex("#FF8800").unwrap();
        assert_eq!(c, HexColor::rgb(255, 136, 0));
    }

    #[test]
    fn test_parse_rgba() {
        let c = HexColor::from_hex("#326EC864").unwrap();
        assert_eq!(c, HexColor::rgba(50, 110, 200, 100));
    }

    #[test]
    fn test_parse_lowercase() {
        let c = HexColor::from_hex("#ff0000").unwrap();
        assert_eq!(c, HexColor::rgb(255, 0, 0));
    }

    #[test]
    fn test_round_trip_rgb() {
        let c = HexColor::rgb(30, 30, 30);
        let hex = c.to_hex();
        assert_eq!(hex, "#1E1E1E");
        assert_eq!(HexColor::from_hex(&hex).unwrap(), c);
    }

    #[test]
    fn test_round_trip_rgba() {
        let c = HexColor::rgba(50, 110, 200, 100);
        let hex = c.to_hex();
        assert_eq!(hex, "#326EC864");
        assert_eq!(HexColor::from_hex(&hex).unwrap(), c);
    }

    #[test]
    fn test_invalid_input() {
        assert!(HexColor::from_hex("").is_none());
        assert!(HexColor::from_hex("#").is_none());
        assert!(HexColor::from_hex("#GG0000").is_none());
        assert!(HexColor::from_hex("#12345").is_none());
        assert!(HexColor::from_hex("123456").is_none());
    }

    #[test]
    fn test_multibyte_input_is_rejected_not_panicking() {
        assert!(HexColor::from_hex("#aé123").is_none());
        assert!(HexColor::from_hex("#ééé").is_none());
        assert!(HexColor::from_hex("#12345é").is_none());
        assert!(HexColor::from_hex("#1234567é").is_none());
    }

    #[test]
    fn test_multibyte_input_via_serde_is_an_error_not_a_panic() {
        let result: Result<HexColor, _> = serde_json::from_str("\"#aé123\"");
        assert!(result.is_err());
    }

    #[test]
    fn test_mix_endpoints_and_midpoint() {
        let black = HexColor::rgb(0, 0, 0);
        let white = HexColor::rgb(255, 255, 255);
        assert_eq!(black.mix(white, 0.0), black);
        assert_eq!(black.mix(white, 1.0), HexColor::rgb(255, 255, 255));
        let mid = black.mix(white, 0.5);
        assert!(mid.r >= 126 && mid.r <= 128);
    }

    #[test]
    fn test_mix_preserves_own_alpha() {
        let a = HexColor::rgba(10, 20, 30, 100);
        let b = HexColor::rgb(200, 200, 200);
        assert_eq!(a.mix(b, 0.5).a, 100);
    }

    #[test]
    fn test_scale_rgb_rounds_and_clamps() {
        let c = HexColor::rgb(45, 212, 191);
        let dim = c.scale_rgb(0.7);
        assert_eq!((dim.r, dim.g, dim.b), (32, 148, 134));
        let over = c.scale_rgb(10.0);
        assert_eq!((over.r, over.g, over.b), (255, 255, 255));
    }

    #[test]
    fn test_with_alpha() {
        let c = HexColor::rgb(1, 2, 3).with_alpha(0x22);
        assert_eq!(c, HexColor::rgba(1, 2, 3, 0x22));
    }

    #[test]
    fn test_luminance_extremes() {
        assert!(HexColor::rgb(0, 0, 0).luminance() < 0.01);
        assert!(HexColor::rgb(255, 255, 255).luminance() > 0.99);
        assert!(HexColor::rgb(45, 212, 191).luminance() > 0.5);
    }

    #[test]
    fn test_serde_round_trip() {
        let c = HexColor::rgb(212, 212, 212);
        let json = serde_json::to_string(&c).unwrap();
        assert_eq!(json, "\"#D4D4D4\"");
        let parsed: HexColor = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, c);
    }
}
