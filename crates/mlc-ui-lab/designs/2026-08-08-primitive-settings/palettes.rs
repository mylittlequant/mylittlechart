//! The four palette presets variant C offers, straight out of the approved
//! mockup (`mlc-app-shell-web/pkg/levels_panel.html`, the `ramps` array).
//!
//! These hex strings are content, not chrome — the owner brief is explicit
//! that "the palette swatches in variant C are content, not chrome, and may
//! be literal", unlike every other colour in this crate which reads off the
//! active theme.

/// One named preset: a display label and five colour stops spanning the
/// level ladder from lowest to highest.
pub struct Palette {
    pub name: &'static str,
    pub colors: [&'static str; 5],
}

pub const PALETTES: [Palette; 4] = [
    Palette { name: "Классика", colors: ["#f23645", "#f5a623", "#e3c800", "#4caf50", "#26a69a"] },
    Palette { name: "Тепло", colors: ["#7f1d1d", "#c2410c", "#f59e0b", "#fbbf24", "#fde68a"] },
    Palette { name: "Холод", colors: ["#1e3a8a", "#2563eb", "#0ea5e9", "#22d3ee", "#a5f3fc"] },
    Palette { name: "Моно", colors: ["#3a3f4b", "#565c69", "#787b86", "#9aa0ab", "#c9ced8"] },
];

fn hex_to_rgb(hex: &str) -> (u8, u8, u8) {
    let h = hex.trim_start_matches('#');
    if h.len() < 6 {
        return (0, 0, 0);
    }
    let r = u8::from_str_radix(&h[0..2], 16).unwrap_or(0);
    let g = u8::from_str_radix(&h[2..4], 16).unwrap_or(0);
    let b = u8::from_str_radix(&h[4..6], 16).unwrap_or(0);
    (r, g, b)
}

fn lerp_hex(a: &str, b: &str, t: f64) -> String {
    let (ar, ag, ab) = hex_to_rgb(a);
    let (br, bg, bb) = hex_to_rgb(b);
    let lerp = |x: u8, y: u8| -> u8 { (x as f64 + (y as f64 - x as f64) * t).round().clamp(0.0, 255.0) as u8 };
    format!("#{:02x}{:02x}{:02x}", lerp(ar, br), lerp(ag, bg), lerp(ab, bb))
}

/// Interpolate a colour at position `t` (`0.0..=1.0`) across a palette's
/// five stops.
pub fn palette_color_at(colors: &[&str; 5], t: f64) -> String {
    let t = t.clamp(0.0, 1.0);
    let n = colors.len().saturating_sub(1).max(1);
    let scaled = t * n as f64;
    let i = (scaled.floor() as usize).min(n.saturating_sub(1));
    let local_t = scaled - i as f64;
    let j = (i + 1).min(n);
    lerp_hex(colors[i], colors[j], local_t)
}
