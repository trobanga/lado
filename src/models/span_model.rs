use crate::TextSpan as SlintTextSpan;
use slint::Color;

/// Model for a syntax-highlighted text span
#[derive(Debug, Clone)]
pub struct TextSpanModel {
    pub text: String,
    pub color: Color,
}

impl TextSpanModel {
    pub fn new(text: String, color: Color) -> Self {
        Self { text, color }
    }

    /// Create a span from text and a hex color string (e.g., "#RRGGBB")
    pub fn from_hex(text: String, hex_color: &str) -> Self {
        Self {
            text,
            color: parse_hex_color(hex_color),
        }
    }
}

impl From<TextSpanModel> for SlintTextSpan {
    fn from(model: TextSpanModel) -> Self {
        Self {
            text: model.text.into(),
            color: model.color,
        }
    }
}

/// Parse a hex color string (e.g., "#RRGGBB" or "RRGGBB") to slint::Color
pub fn parse_hex_color(hex: &str) -> Color {
    let hex = hex.trim_start_matches('#');
    if hex.len() >= 6 {
        let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(255);
        let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(255);
        let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(255);
        Color::from_rgb_u8(r, g, b)
    } else {
        // Default to white if parsing fails
        Color::from_rgb_u8(255, 255, 255)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hex_color() {
        let color = parse_hex_color("#ff0000");
        assert_eq!(color.red(), 255);
        assert_eq!(color.green(), 0);
        assert_eq!(color.blue(), 0);
    }

    #[test]
    fn test_parse_hex_color_without_hash() {
        let color = parse_hex_color("00ff00");
        assert_eq!(color.red(), 0);
        assert_eq!(color.green(), 255);
        assert_eq!(color.blue(), 0);
    }

    #[test]
    fn test_text_span_from_hex() {
        let span = TextSpanModel::from_hex("fn".to_string(), "#0000ff");
        assert_eq!(span.text, "fn");
        assert_eq!(span.color.blue(), 255);
    }
}
