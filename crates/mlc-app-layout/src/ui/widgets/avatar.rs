//! Shared circular avatar placeholder — a filled accent disc with the
//! label's first letter, centered. Lifted here 2026-09-10 (owner ruling: DM
//! bubbles/conversation rows need the exact same shape the chrome's account
//! button already draws) so both draw sites share one body instead of
//! drifting apart as two private copies — `mlc-app-input-shell::chrome::
//! render` cannot be the shared home since `mlc-app-layout` sits BELOW it in
//! the crate graph (shells depend on layout, never the reverse).

use mlc_core::engine::render::{RenderContext, TextAlign, TextBaseline};

/// A round avatar placeholder: a filled circle in `bg` with the label's
/// first letter (uppercased), centered.
pub fn draw_avatar_placeholder(ctx: &mut dyn RenderContext, cx: f64, cy: f64, r: f64, label: &str, bg: &str) {
    ctx.set_fill_color(bg);
    ctx.begin_path();
    ctx.arc(cx, cy, r, 0.0, std::f64::consts::TAU);
    ctx.fill();
    if let Some(letter) = label.chars().next() {
        ctx.set_fill_color(contrasting_text_color(bg));
        ctx.set_font("600 11px sans-serif");
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        let upper: String = letter.to_uppercase().collect();
        ctx.fill_text(&upper, cx, cy + 0.5);
    }
}

/// Black or white text, whichever reads better on `bg_hex` — via relative
/// luminance (ITU-R BT.601). Duplicated from `mlc_core::drawing::
/// primitives_v2::contrasting_text_color` (crate-private there) — same
/// precedent `mlc-app-input-shell::chrome::render`'s own former copy
/// documented before it switched to calling this shared function.
///
/// `pub(crate)` (not private) so other `mlc-app-layout` render sites that
/// paint text over a THEME-DERIVED (never hardcoded) background — an accent
/// pill, an accent-tinted bubble — can pick a legible ink without guessing
/// per-preset (owner ruling 2026-09-10, DM pane theme-tokens pass: no
/// literal light/dark text color may assume which theme preset is active).
pub(crate) fn contrasting_text_color(bg_hex: &str) -> &'static str {
    let hex = bg_hex.trim_start_matches('#');
    if hex.len() < 6 {
        return "#ffffff";
    }
    let parse = |s: &str| u8::from_str_radix(s, 16).unwrap_or(128) as f64;
    let r = parse(&hex[0..2]);
    let g = parse(&hex[2..4]);
    let b = parse(&hex[4..6]);
    let luminance = 0.299 * r + 0.587 * g + 0.114 * b;
    if luminance > 150.0 { "#000000" } else { "#ffffff" }
}
