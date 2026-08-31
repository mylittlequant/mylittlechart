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

/// Draw the 3-token sample as three small squares side by side, filling a
/// `size` x `size` box at `(x, y)` — background / accent / text, left to
/// right. Callers reserve the same footprint a single-color swatch used to
/// occupy, so row layout/text position does not need to change to fit it.
pub fn draw_theme_sample(ctx: &mut dyn RenderContext, x: f64, y: f64, size: f64, tokens: &[String; 3]) {
    let band_w = size / tokens.len() as f64;
    for (i, color) in tokens.iter().enumerate() {
        ctx.set_fill_color(color);
        ctx.fill_rect(x + band_w * i as f64, y, band_w, size);
    }
}
