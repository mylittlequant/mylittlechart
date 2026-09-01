//! Small colour-sample swatch drawn beside a theme preset row.
//!
//! Every surface that lists theme presets — the settings modal's Appearance
//! tab, the welcome wizard's theme page, and the toolbar's Theme dropdown —
//! draws the SAME token triple so a preset can be told apart from another
//! before clicking. The tokens are the preset's own real colours, pulled
//! through `RuntimeTheme::from_preset` (the same accessor every theme
//! application path already uses) — not a hand-picked approximation. The
//! old `chart_settings.rs` preset table carried one hardcoded hex per
//! preset for its swatch that did not even match that preset's own
//! background (e.g. dark's table entry was `#1e222d`, its real
//! `colors.toolbar_bg` is `#131722`), which is why it read the same for
//! every visually-close preset.

// Base trait, not `mlc_core::engine::render::RenderContext` — this widget
// is called from `crate::ui::dropdown` (deliberately core-independent, see
// that file's header comment) as well as from the settings modal and the
// wizard, so it has to type against whichever trait BOTH callers already
// hold a `&mut dyn` of. `mlc_core::engine::render::RenderContext: uzor::
// render::RenderContext` is a supertrait, so a caller holding the extended
// trait object coerces down to this one for free at the call site.
use uzor::render::RenderContext;
use mlc_core::theme::RuntimeTheme;

/// Background / accent / text tokens for `preset_id`. Works uniformly for
/// the 5 base UI themes and the 3 skin presets — skins set these same
/// three `colors.*` fields via `SkinTokens::apply`, so one code path covers
/// every entry in `RuntimeTheme::PRESETS`.
pub fn theme_sample_tokens(preset_id: &str) -> [String; 3] {
    let rt = RuntimeTheme::from_preset(preset_id);
    [rt.colors.toolbar_bg, rt.colors.accent, rt.colors.text_primary]
}

/// Draw the 3-token sample as one rounded-square block at `(x, y)`, filling
/// a `size` x `size` footprint — the same footprint the old three-vertical-
/// bar version occupied, so callers (`chart_settings`'s Appearance tab,
/// `dropdown`'s Theme list) do not need to change their layout to fit it.
///
/// Owner review 2026-08-31: the old version split the box into three equal
/// vertical strips (background / accent / text left to right), each
/// `size/3` wide by `size` tall — a 1:3 aspect per strip that read as "narrow
/// vertical bars" rather than a color sample. This version reads as
/// "circles or squares" instead: the background token fills the whole
/// rounded square (the square), the accent token sits as an inset circle at
/// the bottom-right corner (the circle — also the strongest differentiator
/// between presets, more so than background alone), and the text token
/// becomes a thin outline ring around the square (costs no extra footprint,
/// reads as contrast).
pub fn draw_theme_sample(ctx: &mut dyn RenderContext, x: f64, y: f64, size: f64, tokens: &[String; 3]) {
    let radius = size * 0.22;
    ctx.set_fill_color(&tokens[0]);
    ctx.fill_rounded_rect(x, y, size, size, radius);

    let dot_r = size * 0.28;
    let dot_inset = size * 0.06;
    let dot_cx = x + size - dot_r - dot_inset;
    let dot_cy = y + size - dot_r - dot_inset;
    ctx.set_fill_color(&tokens[1]);
    ctx.begin_path();
    ctx.arc(dot_cx, dot_cy, dot_r, 0.0, std::f64::consts::TAU);
    ctx.close_path();
    ctx.fill();

    ctx.set_stroke_color(&tokens[2]);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(x, y, size, size, radius);
}

/// Number of chips in the palette strip. Five reads as a palette without
/// crowding the longest preset label ("High Contrast Mono") out of a grid
/// cell; see `THEME_PALETTE_W` for the footprint that follows from it.
pub const THEME_PALETTE_LEN: usize = 5;

/// The five tokens the palette strip shows, in draw order.
///
/// Owner review 2026-09-02: the single square-with-a-dot sample read as "a
/// checkbox with a ball inside" — it says a preset is *different* but not
/// *how*. A strip of the preset's own load-bearing colours says it directly.
///
/// The five are picked to be the ones a trader actually judges a theme by,
/// not the five that happen to be first in the struct: the canvas the chart
/// is drawn on, both candle bodies (the only colours on screen in every
/// single frame), the accent that marks selection everywhere, and the
/// primary text. Two presets that differ only in, say, `sidebar_bg` are not
/// worth telling apart here.
///
/// Same accessor as [`theme_sample_tokens`] — `RuntimeTheme::from_preset`
/// — so skins (`surf`) resolve through their `SkinTokens::apply` path and
/// need no special case.
pub fn theme_palette_tokens(preset_id: &str) -> [String; THEME_PALETTE_LEN] {
    let rt = RuntimeTheme::from_preset(preset_id);
    [
        rt.chart.background,
        rt.series.candle_up_body,
        rt.series.candle_down_body,
        rt.colors.accent,
        rt.colors.text_primary,
    ]
}

/// Footprint of a palette strip drawn with `chip`-sized chips and `gap`
/// between them. Callers lay out against this instead of recomputing the
/// same arithmetic and drifting from it.
pub fn theme_palette_width(chip: f64, gap: f64) -> f64 {
    chip * THEME_PALETTE_LEN as f64 + gap * (THEME_PALETTE_LEN - 1) as f64
}

/// Draw the palette strip left-to-right from `(x, y)`.
///
/// Every chip carries a hairline border in `border`: a preset's background
/// token is frequently within a hair of the row background it is drawn on
/// (dark's `#131722` on a dark row), and without the outline that chip
/// simply disappears — the strip would then read as four chips for some
/// presets and five for others.
pub fn draw_theme_palette(
    ctx: &mut dyn RenderContext,
    x: f64,
    y: f64,
    chip: f64,
    gap: f64,
    border: &str,
    tokens: &[String; THEME_PALETTE_LEN],
) {
    let radius = chip * 0.25;
    for (i, token) in tokens.iter().enumerate() {
        let cx = x + i as f64 * (chip + gap);
        ctx.set_fill_color(token);
        ctx.fill_rounded_rect(cx, y, chip, chip, radius);
        ctx.set_stroke_color(border);
        ctx.set_stroke_width(1.0);
        ctx.stroke_rounded_rect(cx, y, chip, chip, radius);
    }
}
