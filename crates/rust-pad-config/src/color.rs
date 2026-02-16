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
        match s.len() {
            6 => {
                let r = u8::from_str_radix(&s[0..2], 16).ok()?;
                let g = u8::from_str_radix(&s[2..4], 16).ok()?;
                let b = u8::from_str_radix(&s[4..6], 16).ok()?;
                Some(Self { r, g, b, a: 255 })
            }
            8 => {
                let r = u8::from_str_radix(&s[0..2], 16).ok()?;
                let g = u8::from_str_radix(&s[2..4], 16).ok()?;
                let b = u8::from_str_radix(&s[4..6], 16).ok()?;
                let a = u8::from_str_radix(&s[6..8], 16).ok()?;
                Some(Self { r, g, b, a })
            }
            _ => None,
        }
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
    fn test_serde_round_trip() {
        let c = HexColor::rgb(212, 212, 212);
        let json = serde_json::to_string(&c).unwrap();
        assert_eq!(json, "\"#D4D4D4\"");
        let parsed: HexColor = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, c);
    }
}
