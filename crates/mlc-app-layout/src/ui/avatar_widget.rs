//! Harvest-ready avatar widget for the messenger concept's chat rows/
//! bubbles (`messenger_concept_v1.html`'s `.av`/`.av.sq`/`.av.ch`): a
//! filled circle (people) or rounded square (groups/channels) body, up to
//! two initials, and an optional bottom-right corner marker — either a
//! plain presence dot (online) or a backdrop circle for a kind glyph
//! (group/channel/verified), the caller's own icon registry paints the
//! glyph itself on top.
//!
//! Harvest: uzor avatar
//!
//! Pure draw, no `ChartApp`/`BackendCore`/modal-state access and no MLC
//! domain types in its signatures — colours arrive already resolved from
//! the caller's own theme (`ToolbarTheme`/`UIColors` presets), never a
//! literal. This widget reuses `crate::ui::widgets::avatar`'s existing
//! single-letter circular placeholder's contrast-colour helper rather than
//! duplicating it, and is additive: that simpler placeholder stays for its
//! existing callers (plain DM rows/bubbles), this one is for anywhere the
//! concept needs a shape choice, two-letter initials, or a corner marker.

use mlc_core::engine::render::{RenderContext, TextAlign, TextBaseline};

/// Circle (1:1 people) or rounded square (groups/channels) — the concept's
/// `.av` vs `.av.sq`/`.av.ch`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AvatarShape {
    Circle,
    RoundedSquare,
}

/// Colours the caller already resolved from its own theme.
pub struct AvatarColors<'a> {
    pub background: &'a str,
    pub text: &'a str,
    /// The ring painted around a corner marker so it reads against the
    /// avatar's own fill — the colour the avatar visually sits ON (e.g.
    /// `ToolbarTheme::background`/`dropdown_bg`), never the avatar's own
    /// background colour.
    pub ring: &'a str,
}

/// What (if anything) sits in the avatar's bottom-right corner. A given
/// avatar carries at most one of these — the concept never combines a
/// presence dot with a kind badge on the same row (a person has presence,
/// a group/channel has a kind glyph, never both).
pub enum CornerMarker<'a> {
    None,
    /// A small filled presence dot.
    Presence { color: &'a str },
    /// A backdrop circle for a kind glyph (group/channel/verified) — this
    /// widget owns no icon registry, so it only fills the circle; the
    /// caller paints the actual glyph centred at the returned geometry.
    Kind { background: &'a str },
}

/// Paint one avatar centred at `(cx, cy)` with body radius/half-extent `r`.
/// Returns the corner marker's own `(x, y, radius)` when [`CornerMarker::
/// Kind`] was drawn, so the caller can centre its icon glyph on top without
/// re-deriving the corner offset formula — `None` for [`CornerMarker::
/// None`]/[`CornerMarker::Presence`] (nothing further for the caller to
/// paint there).
pub fn draw_avatar(
    ctx: &mut dyn RenderContext,
    cx: f64,
    cy: f64,
    r: f64,
    shape: AvatarShape,
    initials: &str,
    colors: &AvatarColors,
    corner: CornerMarker,
) -> Option<(f64, f64, f64)> {
    ctx.set_fill_color(colors.background);
    match shape {
        AvatarShape::Circle => {
            ctx.begin_path();
            ctx.arc(cx, cy, r, 0.0, std::f64::consts::TAU);
            ctx.fill();
        }
        AvatarShape::RoundedSquare => {
            ctx.fill_rounded_rect(cx - r, cy - r, r * 2.0, r * 2.0, r * 0.4);
        }
    }
    if !initials.is_empty() {
        ctx.set_fill_color(colors.text);
        ctx.set_font(&format!("600 {:.0}px sans-serif", (r * 0.78).max(9.0)));
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(initials, cx, cy + 0.5);
    }
    let (bx, by) = corner_anchor(cx, cy, r);
    match corner {
        CornerMarker::None => None,
        CornerMarker::Presence { color } => {
            let dot_r = presence_radius(r);
            ctx.set_fill_color(colors.ring);
            ctx.begin_path();
            ctx.arc(bx, by, dot_r + 1.5, 0.0, std::f64::consts::TAU);
            ctx.fill();
            ctx.set_fill_color(color);
            ctx.begin_path();
            ctx.arc(bx, by, dot_r, 0.0, std::f64::consts::TAU);
            ctx.fill();
            None
        }
        CornerMarker::Kind { background } => {
            let badge_r = kind_badge_radius(r);
            ctx.set_fill_color(colors.ring);
            ctx.begin_path();
            ctx.arc(bx, by, badge_r + 1.5, 0.0, std::f64::consts::TAU);
            ctx.fill();
            ctx.set_fill_color(background);
            ctx.begin_path();
            ctx.arc(bx, by, badge_r, 0.0, std::f64::consts::TAU);
            ctx.fill();
            Some((bx, by, badge_r))
        }
    }
}

/// Pure geometry: where a corner marker centres, given the avatar's own
/// centre and radius — shared by both marker kinds so the dot and the kind
/// badge always sit in the exact same spot.
fn corner_anchor(cx: f64, cy: f64, r: f64) -> (f64, f64) {
    (cx + r * 0.8, cy + r * 0.8)
}

fn presence_radius(r: f64) -> f64 {
    (r * 0.28).max(3.0)
}

fn kind_badge_radius(r: f64) -> f64 {
    (r * 0.36).max(6.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corner_anchor_moves_outward_with_a_bigger_avatar() {
        let small = corner_anchor(0.0, 0.0, 10.0);
        let big = corner_anchor(0.0, 0.0, 20.0);
        assert!(big.0 > small.0 && big.1 > small.1);
    }

    #[test]
    fn badge_radii_never_collapse_to_zero_on_a_tiny_avatar() {
        assert!(presence_radius(1.0) >= 3.0);
        assert!(kind_badge_radius(1.0) >= 6.0);
    }
}
