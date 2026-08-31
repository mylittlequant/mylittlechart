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
