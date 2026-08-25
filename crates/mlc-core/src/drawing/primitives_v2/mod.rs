//! Primitives module - organized by category
//!
//! # Architecture
//!
//! Each primitive category is a submodule containing:
//! - Individual primitive types implementing the `Primitive` trait
//! - Category-specific shared utilities
//!
//! # Categories
//!
//! ```text
//! primitives/
//! ├── mod.rs              # This file - exports and registry
//! ├── traits.rs           # Core Primitive trait definition
//! ├── types.rs            # Shared types (PrimitiveColor, PrimitiveText, etc.)
//! ├── registry.rs         # PrimitiveRegistry for factory pattern
//! │
//! ├── lines/              # Line-based primitives
//! │   ├── mod.rs
//! │   ├── trend_line.rs
//! │   ├── horizontal_line.rs
//! │   ├── vertical_line.rs
//! │   ├── ray.rs
//! │   ├── extended_line.rs
//! │   ├── parallel_channel.rs
//! │   └── info_line.rs
//! │
//! ├── shapes/             # Geometric shapes
//! │   ├── mod.rs
//! │   ├── rectangle.rs
//! │   ├── ellipse.rs
//! │   ├── triangle.rs
//! │   ├── arc.rs
//! │   └── polyline.rs
//! │
//! ├── fibonacci/          # Fibonacci tools
//! │   ├── mod.rs
//! │   ├── retracement.rs
//! │   ├── extension.rs
//! │   ├── channel.rs
//! │   ├── circles.rs
//! │   ├── spiral.rs
//! │   ├── time_zones.rs
//! │   └── wedge.rs
//! │
//! ├── gann/               # Gann tools
//! │   ├── mod.rs
//! │   ├── fan.rs
//! │   ├── square.rs
//! │   └── box.rs
//! │
//! ├── patterns/           # Chart patterns
//! │   ├── mod.rs
//! │   ├── head_shoulders.rs
//! │   ├── elliott_wave.rs
//! │   ├── abcd.rs
//! │   ├── xabcd.rs
//! │   └── cypher.rs
//! │
//! ├── annotations/        # Text and markers
//! │   ├── mod.rs
//! │   ├── text.rs
//! │   ├── note.rs
//! │   ├── callout.rs
//! │   ├── price_label.rs
//! │   ├── arrow.rs
//! │   └── icon.rs
//! │
//! ├── measurement/        # Measuring tools
//! │   ├── mod.rs
//! │   ├── price_range.rs
//! │   ├── date_range.rs
//! │   ├── bars_pattern.rs
//! │   └── ruler.rs
//! │
//! └── trading/            # Trading-specific tools
//!     ├── mod.rs
//!     ├── position.rs
//!     ├── long_position.rs
//!     ├── short_position.rs
//!     └── risk_reward.rs
//! ```

use serde::{Deserialize, Serialize};

mod traits;
mod types;
mod bar_window;
pub mod registry;
pub mod config;
pub mod fill_defaults;

/// Format a price value with smart precision for display in drawing primitives.
/// Handles everything from BTC ($70000) to PEPE ($0.000000003).
/// For prices below 1e-8, uses scientific notation (e.g. "1.23e-10").
pub fn fmt_price(price: f64) -> String {
    if price != 0.0 && price.abs() < 1e-8 {
        let exp = price.abs().log10().floor() as i32;
        let mantissa = price / 10f64.powi(exp);
        return format!("{:.2}e{}", mantissa, exp);
    }

    let precision = if price.abs() >= 10000.0 {
        2
    } else if price.abs() >= 1000.0 {
        2
    } else if price.abs() >= 100.0 {
        3
    } else if price.abs() >= 1.0 {
        4
    } else if price.abs() >= 0.01 {
        6
    } else {
        8
    };

    let formatted = format!("{:.prec$}", price, prec = precision);

    if formatted.contains('.') {
        let trimmed = formatted.trim_end_matches('0');
        let dot_pos = trimmed.find('.').unwrap();
        let decimals_len = trimmed.len() - dot_pos - 1;
        if decimals_len < 2 {
            format!("{:.2}", price)
        } else {
            trimmed.to_string()
        }
    } else {
        format!("{:.2}", price)
    }
}

/// Draw a single line of label text with an optional background "plate" (fill +
/// border, driven by `LabelStyle`), centered at `(x, y)` — `y` is the vertical
/// CENTER the text will be drawn at (matches `TextBaseline::Middle`, which every
/// caller sets before invoking this).
///
/// `font_size` sizes the plate height; it is passed separately from
/// `label_style.font_size` because some callers (e.g. harmonic-pattern ratio
/// numbers) intentionally render at a smaller size than the point labels while
/// still sharing the same `LabelStyle` toggle.
///
/// Caller must already have set font, fill color (the label's own color) and
/// text alignment on `ctx`. Shared by every pattern/Elliott primitive so one
/// `LabelStyle` (and its "Show Background" toggle) governs every label a
/// primitive draws, not just the point labels — precedent:
/// `measurement::draw_stat_plate`.
pub(crate) fn draw_label_plate(
    ctx: &mut dyn RenderContext,
    x: f64,
    y: f64,
    label: &str,
    font_size: f64,
    label_style: &LabelStyle,
    label_color: &str,
) {
    if let Some(ref bg_color) = label_style.background_color {
        let text_width = ctx.measure_text(label);
        let padding = label_style.background_padding;
        let radius = label_style.background_radius;
        let (bg_x, bg_y, bg_w, bg_h) = label_plate_rect(x, y, text_width, font_size, padding);

        ctx.set_fill_color(bg_color);
        ctx.begin_path();
        ctx.rounded_rect(bg_x, bg_y, bg_w, bg_h, radius);
        ctx.fill();

        // Gated on the explicit flag, not on the colour being set — see
        // `LabelStyle::border_enabled`. `None` here means "no colour of its
        // own", which falls back to the label's colour, not "no border".
        if label_style.border_enabled {
            ctx.set_stroke_color(label_style.border_color.as_deref().unwrap_or(label_color));
            ctx.set_stroke_width(label_style.border_width);
            ctx.stroke();
        }

        ctx.set_fill_color(label_color);
    }

    ctx.fill_text(label, x, y);
}

/// The label plate's own bounding rectangle around a piece of text centered
/// at `(x, y)` (vertical center — matches `TextBaseline::Middle`, which
/// [`draw_label_plate`]'s own caller always sets first) — returns
/// `(bg_x, bg_y, bg_w, bg_h)`. The SAME padding math [`draw_label_plate`]'s
/// own background-fill path uses, factored out here so a caller with no
/// live `RenderContext` (every Elliott type's own `hit_test` — see
/// [`estimate_text_width`]'s own doc for why) can test the SAME rectangle a
/// label's own background is drawn into, never a second copy of it (owner
/// ask 2026-08-13: "a label plate is clickable" — a drawn-but-not-clickable
/// plate must not be able to come back once this exists).
pub(crate) fn label_plate_rect(x: f64, y: f64, text_width: f64, font_size: f64, padding: f64) -> (f64, f64, f64, f64) {
    let bg_x = x - text_width / 2.0 - padding;
    let bg_y = y - font_size / 2.0 - padding;
    let bg_w = text_width + padding * 2.0;
    let bg_h = font_size + padding * 2.0;
    (bg_x, bg_y, bg_w, bg_h)
}

/// A font-metrics-free estimate of a label's own rendered width, for a
/// caller with no live `RenderContext` to call `ctx.measure_text` on. Every
/// Elliott type's own `hit_test` is exactly that caller —
/// `Primitive::hit_test`'s own signature hands it `(bars, viewport,
/// price_scale)`, never `ctx` — so [`hit_test_label_plates`] cannot measure
/// the real string the way [`draw_label_plate`] does; extending the trait's
/// own `hit_test` signature to carry a `RenderContext` would touch every
/// registered primitive's own override, far past this pass's scope.
///
/// `0.56` is a plain average glyph-width-to-font-size ratio for a typical
/// sans-serif font at the short tokens wave labels actually carry (numbers,
/// roman numerals, single letters, parenthesised custom overrides) — wide
/// enough that the estimated plate tends to run slightly WIDER than the one
/// `ctx.measure_text` would report, the safer direction for a HIT AREA: a
/// plate estimated a little too generous still reads as "clicked the
/// label", where one estimated too small would silently miss real clicks
/// near the label's own edge. Never used for anything that paints a pixel —
/// `render`'s own [`draw_label_plate`] always measures the real string.
pub(crate) fn estimate_text_width(text: &str, font_size: f64) -> f64 {
    text.chars().count() as f64 * font_size * 0.56
}

/// Stroke every contiguous run of `segments` (each `(i, i + 1)` — a single
/// leg, in point-index order) as one polyline — `elliott::count::
/// visible_leg_segments`'s own render-side counterpart (owner report
/// 2026-08-13: a spawned subdivision must REPLACE the leg of its parent it
/// nests inside, not draw on top of it, so that leg's own render segment is
/// missing from `segments` and this must not bridge the gap with a
/// straight line of its own). `segments` need not cover every index — this
/// starts a fresh subpath whenever the next segment does not continue
/// directly from the last point drawn, so a subdivided primitive's own
/// leftover legs render as separate strokes either side of the gap. For an
/// UN-subdivided primitive `segments` is one unbroken run end to end, so
/// this draws EXACTLY the single polyline every Elliott type's own `render`
/// drew before subdivision skipping existed — byte-for-byte the same path.
///
/// `ctx` must already have its stroke colour/width/dash set by the caller
/// (matches every other draw helper in this module). An index in
/// `segments` past `screen`'s own bounds is skipped rather than panicking —
/// defensive only, every real caller's `segments` indices are already
/// bounds-checked by construction (`elliott::count::slot_span`'s own
/// lookup never returns an index past a type's fixed point count).
pub(crate) fn stroke_leg_segments(
    ctx: &mut dyn RenderContext,
    screen: &[(f64, f64)],
    segments: &[(usize, usize)],
    dpr: f64,
) {
    let mut iter = segments.iter().peekable();
    while let Some(&(a, b)) = iter.next() {
        let (Some(&(ax, ay)), Some(&(bx, by))) = (screen.get(a), screen.get(b)) else { continue };
        ctx.begin_path();
        ctx.move_to(crisp(ax, dpr), crisp(ay, dpr));
        ctx.line_to(crisp(bx, dpr), crisp(by, dpr));
        let mut last = b;
        while let Some(&&(next_a, next_b)) = iter.peek() {
            if next_a != last {
                break;
            }
            iter.next();
            let Some(&(nx, ny)) = screen.get(next_b) else { break };
            ctx.line_to(crisp(nx, dpr), crisp(ny, dpr));
            last = next_b;
        }
        ctx.stroke();
    }
}

/// How a collision-separated label is pushed off its shared pivot — the
/// EWA-engine setting the owner asked for (owner ask 2026-08-14: "отдалять
/// можно разными способами: один над другим, или параллельно, или
/// треугольничком — это должно быть настраиваемым через модалку ЕВА",
/// implemented in the EWA Engine Settings modal,
/// `EwaEngineConfig::label_separation`). `pub`, not `pub(crate)`, since that
/// config field must round-trip through `mlc-app-layout`'s own modal and
/// `mlc-app`'s own click dispatch, neither of which lives in this crate.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LabelSeparationStyle {
    /// Push straight along the same axis the collision-direction sign
    /// already picked (`wave_label_offset`'s own UP/DOWN branch) — one
    /// added rung of DISTANCE from the pivot, same vertical line the base
    /// offset already sits on. The only style that existed before the other
    /// two were added — the default, so an untouched EWA engine config
    /// renders every structure byte-for-byte as before.
    #[default]
    Stacked,
    /// Push sideways instead of further away: the colliding label lands at
    /// the SAME height as the base (non-colliding) offset, shifted
    /// horizontally by one rung — two labels that read side by side at the
    /// same vertical level ("параллельно").
    Parallel,
    /// Push diagonally — one rung out both horizontally and vertically from
    /// the base offset, so the pivot and the two labels (the child's own
    /// label at the base offset, this one stepped out diagonally) trace a
    /// small triangle ("треугольничком").
    Triangle,
}

/// Push `base_dy` one further rung out from the pivot, in the direction
/// [`LabelSeparationStyle`] names — see that enum's own doc for what each
/// style looks like. `rung` reuses the SAME per-label metrics
/// `wave_label_offset`'s own base offset already scales with (font size +
/// background padding), so the added distance grows with the label's own
/// rendered size instead of a fixed pixel constant. Returns `(dx, dy)`
/// relative to the label's own anchor point — `Stacked` alone stays
/// horizontal-free (`dx == 0.0`), matching the single axis this function
/// moved along before the other two styles existed.
fn step_out_for_collision(base_dy: f64, sign: f64, label_style: &LabelStyle, style: LabelSeparationStyle) -> (f64, f64) {
    let rung = label_style.font_size * 0.9 + label_style.background_padding;
    match style {
        LabelSeparationStyle::Stacked => (0.0, base_dy + sign * rung),
        LabelSeparationStyle::Parallel => (sign * rung, base_dy),
        LabelSeparationStyle::Triangle => (sign * rung * 0.7, base_dy + sign * rung * 0.7),
    }
}

/// The push applied to wave label `i`'s own anchor point before drawing it,
/// as `(dx, dy)` — `dy` is UP when this pivot sits above (smaller screen
/// `y`) the one before it, DOWN otherwise, so a label never sits on top of
/// the wave line leading into it; `dx` is `0.0` unless a collision AND a
/// sideways-pushing [`LabelSeparationStyle`] both apply (see that enum's own
/// doc). Every Elliott type's own label loop computed this same branch
/// inline before this existed; factored out so `render`'s own label
/// placement and [`hit_test_label_plates`]'s own hit area can never drift
/// apart.
///
/// `collides` is `true` only for a label whose OWN pivot is a point index
/// present in this structure's own `Primitive::label_collisions` cache —
/// every caller of this function reads that cache
/// (`self.label_collisions().contains(&i)`) and passes the membership test
/// in; this function performs no lookup of its own, it only reacts to the
/// bool. When `true`, the label steps out by ONE rung beyond the base
/// offset via [`step_out_for_collision`], in whichever direction the ACTIVE
/// `EwaEngineConfig::label_separation` names — read fresh here via
/// [`crate::state::ewa_engine::ewa_engine_config`], the same "read fresh,
/// never cached" contract `control_point_style` already uses, so flipping
/// the setting changes every open chart's own colliding labels on the very
/// next frame. So it clears the child's own label drawn closer to the
/// shared pivot at the child's own base (non-colliding) offset — Elliott
/// notation convention: the finer degree sits closest to a shared pivot, the
/// coarser one pushed further out (owner's own framing, "младший цикл... и
/// старший"). Every other label — every label of a structure with no
/// subdivisions, and every label of a structure that IS subdivided but does
/// not sit on a pivot the child actually labels too — passes `false` and
/// draws at EXACTLY the offset this function produced before the collision
/// case existed: `(0.0, sign * label_style.offset_y)`.
///
/// Regression fix (owner report 2026-08-14, first pass): a prior revision
/// (`cad6b834`) grew this DISTANCE by an unconditional per-degree ring
/// applied to EVERY label of EVERY structure, so even a lone impulse with
/// no subdivisions at all had its five labels pushed away from their
/// pivots for no reason. `collides` replaces that ring entirely — a
/// structure with no subdivisions now renders byte-for-byte what it
/// rendered before `cad6b834`.
///
/// Second pass (same day): the FIRST fix derived `collides` from the slot
/// alone (whether point `i` bounded a subdivided `elliott::count::slot_span`,
/// regardless of what the child actually drew there) — also wrong, for the
/// same reason: it could still step out a label with nothing to collide
/// with (a correction's own EMPTY first label, see each family's own
/// `wave_labels`). `elliott::count::label_collision_indices` now derives
/// the cache from the CHILD's own `Primitive::point_labels`, so only a
/// pivot the child genuinely labels too ever collides.
///
/// Third pass (EWA-engine-settings arc, 2026-08-14): the return type widened
/// from a single `f64` (a pure vertical offset) to `(f64, f64)` so
/// `Parallel`/`Triangle` can genuinely move a label sideways — every caller
/// applies `dx` to its own `x` and `dy` to its own `y`, in place of the old
/// `y + offset`.
pub(crate) fn wave_label_offset(screen: &[(f64, f64)], i: usize, label_style: &LabelStyle, collides: bool) -> (f64, f64) {
    let sign = if i > 0 && screen[i].1 < screen[i - 1].1 { -1.0 } else { 1.0 };
    let base_dy = sign * label_style.offset_y;
    if collides {
        let style = crate::state::ewa_engine::ewa_engine_config().label_separation;
        step_out_for_collision(base_dy, sign, label_style, style)
    } else {
        (0.0, base_dy)
    }
}

/// Hit-test every REALIZED, non-empty wave label's own plate rectangle —
/// the label-side counterpart to `elliott::ratio::hit_test_ratio_ribs`'s own
/// rib hit test. Shares [`wave_label_offset`] (the SAME anchor placement
/// `render`'s own label loop uses, so `label_collisions` must be the SAME
/// structure's own `Primitive::label_collisions` cache `render` reads —
/// see that function's own doc for what it does with the bool this derives
/// via plain index membership, and `elliott::count::label_collision_indices`'s
/// own doc for the rule that fills the cache) and [`label_plate_rect`]
/// (the SAME padding math [`draw_label_plate`]'s own background draws),
/// fed an ESTIMATED width ([`estimate_text_width`]) instead of a real
/// `ctx.measure_text` one — see that function's own doc for why.
///
/// Gated on `show_labels` by the CALLER, not here — mirrors
/// `hit_test_ratio_ribs`'s own "caller gates on show_ratios" contract, so a
/// hidden label is never a hit target.
pub(crate) fn hit_test_label_plates(
    sx: f64,
    sy: f64,
    screen: &[(f64, f64)],
    labels: &[&str],
    realized: usize,
    label_style: &LabelStyle,
    label_collisions: &[usize],
) -> bool {
    for (i, &(x, y)) in screen.iter().enumerate() {
        if i >= realized || i >= labels.len() || labels[i].is_empty() {
            continue;
        }
        let collides = label_collisions.contains(&i);
        let (dx, dy) = wave_label_offset(screen, i, label_style, collides);
        let (label_x, label_y) = (x + dx, y + dy);
        let width = estimate_text_width(labels[i], label_style.font_size);
        let (bg_x, bg_y, bg_w, bg_h) =
            label_plate_rect(label_x, label_y, width, label_style.font_size, label_style.background_padding);
        if sx >= bg_x && sx <= bg_x + bg_w && sy >= bg_y && sy <= bg_y + bg_h {
            return true;
        }
    }
    false
}

/// Pick black or white text for readability over a `#rrggbb`/`#rrggbbaa`
/// background, via relative luminance (ITU-R BT.601). Annotation shapes
/// (comment/callout/note/signpost) each carry a user-configurable fill
/// colour, so a single hardcoded text colour reads fine on some fills and
/// disappears on others — this picks the readable one instead of guessing.
/// Falls back to white on a malformed hex (shouldn't happen; `PrimitiveColor`
/// always stores `#rrggbb`).
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

/// Deterministic per-level colour so a multi-level tool (Fibonacci ratios,
/// Gann angles/rings, time zones) reads as a ramp instead of every level
/// falling back to the same flat stroke colour. `rank` is the level's
/// 0-indexed position among `total` levels sorted ascending by value, not
/// the raw level value — this way it works identically whether levels are
/// 0.0..1.0 ratios, Gann angle slopes, or Fibonacci bar counts. Derives a
/// hex string via HSL so levels stay visually distinct at any `total`
/// without hand-picking a palette per tool.
pub(crate) fn level_ramp_color(rank: usize, total: usize) -> String {
    let hue = if total <= 1 {
        200.0
    } else {
        (rank as f64 / (total - 1) as f64) * 300.0
    };
    hsl_to_hex(hue, 0.68, 0.56)
}

fn hsl_to_hex(h: f64, s: f64, l: f64) -> String {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let h_prime = h / 60.0;
    let x = c * (1.0 - (h_prime.rem_euclid(2.0) - 1.0).abs());
    let m = l - c / 2.0;
    let (r1, g1, b1) = if h_prime < 1.0 {
        (c, x, 0.0)
    } else if h_prime < 2.0 {
        (x, c, 0.0)
    } else if h_prime < 3.0 {
        (0.0, c, x)
    } else if h_prime < 4.0 {
        (0.0, x, c)
    } else if h_prime < 5.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    let to_byte = |v: f64| (((v + m) * 255.0).round().clamp(0.0, 255.0)) as u8;
    format!("#{:02x}{:02x}{:02x}", to_byte(r1), to_byte(g1), to_byte(b1))
}

/// Position along a shared-origin ray/line (0.0 = origin, 1.0 = far end) for
/// the `rank`-th of `total` labels anchored near the same `anchor` fraction,
/// so labels on closely angled rays that share one origin point (Fibonacci
/// fan/speed-resistance, Gann fan) spread out along their own ray instead of
/// landing on top of each other at the origin. `spread` is the total width
/// of the band the ranks are distributed across, centered on `anchor`.
pub(crate) fn stagger_t(anchor: f64, rank: usize, total: usize, spread: f64) -> f64 {
    if total <= 1 {
        return anchor.clamp(0.02, 0.98);
    }
    let centered = rank as f64 / (total - 1) as f64 - 0.5;
    (anchor + centered * spread).clamp(0.02, 0.98)
}

/// Rank of `level` (0-indexed, ascending) among `sorted_levels` — the
/// position used by [`stagger_t`] and label-plate colour lookups so a
/// level's screen placement/colour is stable regardless of the primitive's
/// own (unsorted) `level_configs` storage order.
pub(crate) fn level_rank(level: f64, sorted_levels: &[f64]) -> usize {
    sorted_levels
        .iter()
        .position(|&l| (l - level).abs() < 1e-9)
        .unwrap_or(0)
}

#[cfg(test)]
mod wave_label_offset_tests {
    use super::*;

    // A single point at the origin — `wave_label_offset`'s own `i == 0`
    // branch always picks the positive (DOWN) sign, so every case below
    // isolates the `collides` term without also depending on local
    // geometry (see the function's own doc for that separate branch).
    fn single_point() -> Vec<(f64, f64)> {
        vec![(0.0, 0.0)]
    }

    fn style() -> LabelStyle {
        LabelStyle::default()
    }

    /// Regression fix (owner report 2026-08-14): a structure with no
    /// subdivisions must offset NOTHING beyond the plain per-primitive
    /// `offset_y` — this is the exact expression `wave_label_offset` used
    /// before `cad6b834` added the (now-deleted) blanket per-degree ring,
    /// restored here as the `collides == false` path. No horizontal push
    /// either — `dx` is `0.0` unless a collision AND a sideways-pushing
    /// `LabelSeparationStyle` both apply (see that enum's own doc).
    #[test]
    fn no_collision_offsets_nothing_beyond_the_base() {
        let screen = single_point();
        let s = style();
        let (dx, dy) = wave_label_offset(&screen, 0, &s, false);
        assert_eq!(dx, 0.0, "no collision must never push sideways");
        assert_eq!(dy, s.offset_y, "no collision must reproduce the pre-cad6b834 expression exactly: sign * offset_y");
    }

    /// A structure with no subdivisions never collides at any of its own
    /// labels — every label of it must therefore land at the SAME base
    /// offset, matching `no_collision_offsets_nothing_beyond_the_base`'s
    /// own single-point case for a whole array of points.
    #[test]
    fn a_structure_with_no_subdivisions_offsets_no_label_beyond_the_base() {
        let screen = vec![(0.0, 0.0), (10.0, -20.0), (20.0, 10.0), (30.0, -5.0), (40.0, 30.0), (50.0, 0.0)];
        let s = style();
        for i in 0..screen.len() {
            let sign = if i > 0 && screen[i].1 < screen[i - 1].1 { -1.0 } else { 1.0 };
            assert_eq!(wave_label_offset(&screen, i, &s, false), (0.0, sign * s.offset_y), "label {i}: no collision, base offset only");
        }
    }

    /// Owner report 2026-08-14: a parent's own label and its directly
    /// nested subdivision's own label share one pivot. The subdivided
    /// (colliding) label must step out by exactly one rung beyond the base
    /// offset — the child's own label at that SAME point stays at the base
    /// offset, closest to the pivot, matching Elliott notation convention.
    /// Under the default `LabelSeparationStyle::Stacked` (the process-wide
    /// config's own default, untouched by this test) that rung is entirely
    /// vertical, exactly as it was before `LabelSeparationStyle` grew its
    /// other two variants.
    #[test]
    fn collision_steps_out_by_exactly_one_rung_beyond_the_base() {
        let screen = single_point();
        let s = style();
        let (base_dx, base_dy) = wave_label_offset(&screen, 0, &s, false);
        let (collided_dx, collided_dy) = wave_label_offset(&screen, 0, &s, true);
        let rung = s.font_size * 0.9 + s.background_padding;
        assert_eq!(collided_dx, base_dx, "the default Stacked style never pushes sideways");
        assert_eq!(collided_dy, base_dy + rung, "a colliding label steps out by exactly one rung beyond the base offset");
        assert_ne!(collided_dy, base_dy, "a colliding label must land somewhere different from the base offset");
    }

    /// Same inputs, same output: the offset is a pure function of its
    /// arguments PLUS the active `EwaEngineConfig::label_separation` — the
    /// same "read fresh, never cached" contract `control_point_style`
    /// already uses (see `wave_label_offset`'s own doc). Calling it twice in
    /// a row, with nothing in between that could change the active config,
    /// must still agree.
    #[test]
    fn same_inputs_produce_same_offset() {
        let screen = single_point();
        let s = style();
        let a = wave_label_offset(&screen, 0, &s, true);
        let b = wave_label_offset(&screen, 0, &s, true);
        assert_eq!(a, b);
    }

    /// Direction stays exactly what local geometry picks — a pivot ABOVE
    /// the previous one still pushes the label UP (negative `dy`) regardless
    /// of whether it collides, only the distance grows when it does.
    #[test]
    fn direction_still_follows_local_geometry_not_collision() {
        let screen = vec![(0.0, 100.0), (10.0, 50.0)]; // point 1 sits above point 0
        let s = style();
        assert!(wave_label_offset(&screen, 1, &s, false).1 < 0.0, "a pivot above the previous one must still push its label UP");
        assert!(wave_label_offset(&screen, 1, &s, true).1 < 0.0, "...even while colliding — collision only grows the distance");
    }
}

// Category modules
pub mod lines;
pub mod channels;
pub mod shapes;
pub mod fibonacci;
pub mod pitchforks;
pub mod gann;
pub mod arrows;
pub mod annotations;
pub mod patterns;
pub mod smc;
pub mod elliott;
pub mod cycles;
pub mod projection;
pub mod volume;
pub mod measurement;
pub mod brushes;
pub mod icons;
pub mod signals;

// Re-export core types
pub use traits::{
    Primitive, PrimitiveData, PrimitiveKind, Archetype, ClickBehavior,
    HitTestResult, ControlPointInfo, SyncMode, StyleReference,
    draw_control_points,
    PrimitiveExt,  // Extension trait with is_locked, set_locked, is_visible, set_visible
};
pub use types::{
    PrimitiveColor, PrimitiveText, TextAlign, ExtendMode, LineStyle,
    ControlPoint, ControlPointType, ControlPointCursor,
    HIT_TOLERANCE, CONTROL_POINT_RADIUS, CONTROL_POINT_HIT_RADIUS,
    CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    // User-level control-point style seam (2026-08-07): shape/size read at
    // render time by `draw_control_points` and the default
    // `Primitive::control_point_hit_test` — see `types.rs`'s "Control Point
    // Style" section for why this is a single re-export line, unavoidable
    // because both `mlc-app` (profile load/UI dispatch) and `mlc-app-layout`
    // (render/settings-modal) need these names and neither owns `types.rs`.
    ControlPointShape, ControlPointSize, ControlPointStyle,
    control_point_style, set_control_point_style,
    point_to_line_distance, point_to_ray_distance, extend_segment_to_edges,
    Segment, nearest_segment_distance, hit_test_strokes,
    TextAnchor, normalize_text_rotation,
    LineTextParams, calculate_line_text_params,
    LineSegment, line_segments_avoiding_text,
};
pub use registry::{PrimitiveRegistry, PrimitiveFactory, PrimitiveMetadata};
// Re-export render types from engine::render (unified RenderContext)
pub use crate::render::{
    RenderContext, RenderOp, RenderOps, TextAlign as RenderTextAlign, TextBaseline,
    crisp, crisp_rect, execute_ops,
    render_primitive_text, render_primitive_text_rotated, measure_primitive_text, render_text_with_background,
};
pub use config::{
    ConfigProperty, PropertyType, PropertyValue, PropertyKind, PropertyCategory, PropertyGroup,
    FibLevelConfig, LevelSetPreset, TimeframeVisibilityConfig,
    SelectOption, WaveDegree, LabelStyle,
};
pub use fill_defaults::{
    AREA_FILL_OPACITY, BAND_FILL_OPACITY, PLATE_FILL_OPACITY, BOX_ANNOTATION_FILL_OPACITY,
    default_area_fill_opacity, default_band_fill_opacity, default_plate_fill_opacity,
    default_box_annotation_fill_opacity,
};

// Re-export all primitives
pub use lines::*;
pub use channels::*;
pub use shapes::*;
pub use fibonacci::*;
pub use pitchforks::*;
pub use gann::*;
pub use arrows::*;
pub use annotations::*;
pub use patterns::*;
pub use smc::*;
pub use elliott::*;
pub use cycles::*;
pub use projection::*;
pub use volume::*;
pub use measurement::*;
pub use brushes::*;
pub use icons::*;
pub use signals::{SystemSignal, SignalType, SignalPrimitive, StrategySignalConfig};
