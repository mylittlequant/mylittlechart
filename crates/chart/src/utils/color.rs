//! Color utilities
//!
//! Provides CSS color parsing that returns raw RGBA tuples.

/// Parse CSS color string to RGBA tuple
///
/// Supports:
/// - Hex formats: `#RGB`, `#RRGGBB`, `#RRGGBBAA`
/// - RGB function: `rgb(r, g, b)`
/// - RGBA function: `rgba(r, g, b, a)`
/// - Keyword: `transparent`
///
/// Returns (r, g, b, a) where each component is 0-255.
/// Returns (255, 255, 255, 255) (white) for unrecognized formats.
///
/// # Examples
///
/// ```
/// use zengeld_chart::parse_css_color;
///
/// assert_eq!(parse_css_color("#FF0000"), (255, 0, 0, 255));
/// assert_eq!(parse_css_color("#00FF0080"), (0, 255, 0, 128));
/// assert_eq!(parse_css_color("rgb(100, 150, 200)"), (100, 150, 200, 255));
/// assert_eq!(parse_css_color("rgba(100, 150, 200, 0.5)"), (100, 150, 200, 127));
/// assert_eq!(parse_css_color("transparent"), (0, 0, 0, 0));
/// ```
pub fn parse_css_color(color: &str) -> (u8, u8, u8, u8) {
    // Handle "transparent" keyword
    if color == "transparent" {
        return (0, 0, 0, 0);
    }

    if color.starts_with('#') {
        let hex = &color[1..];
        match hex.len() {
            // #RGB -> #RRGGBB
            3 => {
                let r = u8::from_str_radix(&hex[0..1], 16).unwrap_or(15);
                let g = u8::from_str_radix(&hex[1..2], 16).unwrap_or(15);
                let b = u8::from_str_radix(&hex[2..3], 16).unwrap_or(15);
                // Expand: F -> FF (multiply by 17)
                return (r * 17, g * 17, b * 17, 255);
            }
            // #RRGGBB
            6 => {
                let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(255);
                let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(255);
                let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(255);
                return (r, g, b, 255);
            }
            // #RRGGBBAA
            8 => {
                let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(255);
                let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(255);
                let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(255);
                let a = u8::from_str_radix(&hex[6..8], 16).unwrap_or(255);
                return (r, g, b, a);
            }
            _ => {}
        }
    } else if color.starts_with("rgba(") && color.ends_with(')') {
        let inner = &color[5..color.len() - 1];
        let parts: Vec<&str> = inner.split(',').map(|s| s.trim()).collect();
        if parts.len() == 4 {
            let r = parts[0].parse::<u8>().unwrap_or(255);
            let g = parts[1].parse::<u8>().unwrap_or(255);
            let b = parts[2].parse::<u8>().unwrap_or(255);
            let a = (parts[3].parse::<f32>().unwrap_or(1.0) * 255.0) as u8;
            return (r, g, b, a);
        }
    } else if color.starts_with("rgb(") && color.ends_with(')') {
        let inner = &color[4..color.len() - 1];
        let parts: Vec<&str> = inner.split(',').map(|s| s.trim()).collect();
        if parts.len() == 3 {
            let r = parts[0].parse::<u8>().unwrap_or(255);
            let g = parts[1].parse::<u8>().unwrap_or(255);
            let b = parts[2].parse::<u8>().unwrap_or(255);
            return (r, g, b, 255);
        }
    }

    // Default: white
    (255, 255, 255, 255)
}

/// Apply opacity to a CSS color string
///
/// Takes a color in any supported format and returns `#RRGGBBAA` hex string
/// with the specified opacity (0.0 = transparent, 1.0 = opaque).
///
/// # Examples
///
/// ```
/// use zengeld_chart::apply_opacity;
///
/// assert_eq!(apply_opacity("#FF0000", 0.5), "#FF00007F"); // 0.5 * 255 = 127 = 0x7F
/// assert_eq!(apply_opacity("#F00", 1.0), "#FF0000FF");
/// assert_eq!(apply_opacity("rgb(255, 0, 0)", 0.25), "#FF00003F"); // 0.25 * 255 = 63 = 0x3F
/// ```
pub fn apply_opacity(color: &str, opacity: f64) -> String {
    let (r, g, b, _) = parse_css_color(color);
    let alpha = (opacity.clamp(0.0, 1.0) * 255.0) as u8;
    format!("#{:02X}{:02X}{:02X}{:02X}", r, g, b, alpha)
}

/// Convert RGBA tuple to hex string
///
/// Returns `#RRGGBB` if alpha is 255, otherwise `#RRGGBBAA`.
pub fn rgba_to_hex(r: u8, g: u8, b: u8, a: u8) -> String {
    if a == 255 {
        format!("#{:02X}{:02X}{:02X}", r, g, b)
    } else {
        format!("#{:02X}{:02X}{:02X}{:02X}", r, g, b, a)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hex_6() {
        assert_eq!(parse_css_color("#FF0000"), (255, 0, 0, 255));
        assert_eq!(parse_css_color("#00FF00"), (0, 255, 0, 255));
        assert_eq!(parse_css_color("#0000FF"), (0, 0, 255, 255));
        assert_eq!(parse_css_color("#FFFFFF"), (255, 255, 255, 255));
        assert_eq!(parse_css_color("#000000"), (0, 0, 0, 255));
    }

    #[test]
    fn test_hex_8() {
        assert_eq!(parse_css_color("#FF000080"), (255, 0, 0, 128));
        assert_eq!(parse_css_color("#00FF00FF"), (0, 255, 0, 255));
        assert_eq!(parse_css_color("#0000FF00"), (0, 0, 255, 0));
    }

    #[test]
    fn test_hex_3() {
        assert_eq!(parse_css_color("#F00"), (255, 0, 0, 255));
        assert_eq!(parse_css_color("#0F0"), (0, 255, 0, 255));
        assert_eq!(parse_css_color("#00F"), (0, 0, 255, 255));
        assert_eq!(parse_css_color("#FFF"), (255, 255, 255, 255));
        assert_eq!(parse_css_color("#000"), (0, 0, 0, 255));
    }

    #[test]
    fn test_rgb() {
        assert_eq!(parse_css_color("rgb(255, 0, 0)"), (255, 0, 0, 255));
        assert_eq!(parse_css_color("rgb(100, 150, 200)"), (100, 150, 200, 255));
        assert_eq!(parse_css_color("rgb(0,0,0)"), (0, 0, 0, 255));
    }

    #[test]
    fn test_rgba() {
        assert_eq!(parse_css_color("rgba(255, 0, 0, 1.0)"), (255, 0, 0, 255));
        assert_eq!(parse_css_color("rgba(255, 0, 0, 0.5)"), (255, 0, 0, 127));
        assert_eq!(parse_css_color("rgba(100, 150, 200, 0)"), (100, 150, 200, 0));
    }

    #[test]
    fn test_transparent() {
        assert_eq!(parse_css_color("transparent"), (0, 0, 0, 0));
    }

    #[test]
    fn test_invalid_returns_white() {
        assert_eq!(parse_css_color("invalid"), (255, 255, 255, 255));
        assert_eq!(parse_css_color(""), (255, 255, 255, 255));
        assert_eq!(parse_css_color("#GGG"), (255, 255, 255, 255));
    }

    #[test]
    fn test_lowercase_hex() {
        assert_eq!(parse_css_color("#ff0000"), (255, 0, 0, 255));
        assert_eq!(parse_css_color("#aabbcc"), (170, 187, 204, 255));
    }

    #[test]
    fn test_apply_opacity() {
        assert_eq!(apply_opacity("#FF0000", 1.0), "#FF0000FF");
        assert_eq!(apply_opacity("#FF0000", 0.5), "#FF00007F");
        assert_eq!(apply_opacity("#FF0000", 0.0), "#FF000000");
        assert_eq!(apply_opacity("#F00", 0.4), "#FF000066");
        assert_eq!(apply_opacity("rgb(0, 255, 0)", 0.5), "#00FF007F");
    }

    #[test]
    fn test_rgba_to_hex() {
        assert_eq!(rgba_to_hex(255, 0, 0, 255), "#FF0000");
        assert_eq!(rgba_to_hex(255, 0, 0, 128), "#FF000080");
        assert_eq!(rgba_to_hex(0, 255, 0, 0), "#00FF0000");
    }
}
