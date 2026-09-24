//! Harvest-ready "count badge" widget — the messenger concept's small
//! pills: unread counts (`.ub`), muted/grey counts (`.ub.m`), the "@"
//! mention marker (`.ub.at`), and the folder rail's own unread counts
//! (`.bd`/`.bd.m`). One shape, two colour states (accent vs muted — the
//! caller's own choice of which `BadgeColors` to pass), two content kinds.
//!
//! Harvest: uzor count-badge
//!
//! Pure draw, no `ChartApp`/`BackendCore`/modal-state access and no MLC
//! domain types in its signatures — colours arrive already resolved from
//! the caller's own theme (`ToolbarTheme`/`UIColors` presets).

use mlc_core::engine::render::{RenderContext, TextAlign, TextBaseline};

/// What the pill shows.
pub enum BadgeContent<'a> {
    /// A plain count string — `"99+"` capping is the caller's own job
    /// (this widget draws whatever it is given, verbatim).
    Count(&'a str),
    /// The mention marker (`@`) — the concept's `.ub.at`.
    Mention,
}

impl BadgeContent<'_> {
    fn label(&self) -> &str {
        match self {
            BadgeContent::Count(s) => s,
            BadgeContent::Mention => "@",
        }
    }
}

pub struct BadgeColors<'a> {
    pub background: &'a str,
    pub text: &'a str,
}

/// Natural width for `content` at pill height `height` — the caller
/// measures before laying out a row (same "measure before you place"
/// split every other widget in this crate follows). Never narrower than
/// `height` itself, so a one-character badge is still round, not a
/// sliver.
pub fn badge_width(ctx: &dyn RenderContext, content: &BadgeContent, height: f64) -> f64 {
    let label_w = ctx.measure_text(content.label());
    (label_w + height * 0.7).max(height)
}

/// Paint the pill at `(x, y)` with size `(w, height)`.
pub fn draw_count_badge(
    ctx: &mut dyn RenderContext,
    x: f64,
    y: f64,
    w: f64,
    height: f64,
    content: &BadgeContent,
    colors: &BadgeColors,
) {
    ctx.set_fill_color(colors.background);
    ctx.fill_rounded_rect(x, y, w, height, height / 2.0);
    ctx.set_fill_color(colors.text);
    ctx.set_font(&format!("600 {:.0}px sans-serif", (height * 0.58).max(9.0)));
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(content.label(), x + w / 2.0, y + height / 2.0 + 0.5);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_picks_the_right_string_per_content_kind() {
        assert_eq!(BadgeContent::Count("42").label(), "42");
        assert_eq!(BadgeContent::Mention.label(), "@");
    }
}
