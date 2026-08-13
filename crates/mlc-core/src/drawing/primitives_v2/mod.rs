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

mod traits;
mod types;
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

/// The vertical push applied to wave label `i`'s own anchor point before
/// drawing it — UP when this pivot sits above (smaller screen `y`) the one
/// before it, DOWN otherwise, so a label never sits on top of the wave line
/// leading into it. Every Elliott type's own label loop computed this same
/// branch inline before this existed; factored out so `render`'s own label
/// placement and [`hit_test_label_plates`]'s own hit area can never drift
/// apart.
pub(crate) fn wave_label_offset(screen: &[(f64, f64)], i: usize, offset_y: f64) -> f64 {
    if i > 0 && screen[i].1 < screen[i - 1].1 { -offset_y } else { offset_y }
}

/// Hit-test every REALIZED, non-empty wave label's own plate rectangle —
/// the label-side counterpart to `elliott::ratio::hit_test_ratio_ribs`'s own
/// rib hit test. Shares [`wave_label_offset`] (the SAME anchor placement
/// `render`'s own label loop uses) and [`label_plate_rect`] (the SAME
/// padding math [`draw_label_plate`]'s own background draws), fed an
/// ESTIMATED width ([`estimate_text_width`]) instead of a real
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
) -> bool {
    for (i, &(x, y)) in screen.iter().enumerate() {
        if i >= realized || i >= labels.len() || labels[i].is_empty() {
            continue;
        }
        let label_y = y + wave_label_offset(screen, i, label_style.offset_y);
        let width = estimate_text_width(labels[i], label_style.font_size);
        let (bg_x, bg_y, bg_w, bg_h) =
            label_plate_rect(x, label_y, width, label_style.font_size, label_style.background_padding);
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
    point_to_line_distance, point_to_ray_distance,
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
    FibLevelConfig, TimeframeVisibilityConfig,
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
pub use elliott::*;
pub use cycles::*;
pub use projection::*;
pub use volume::*;
pub use measurement::*;
pub use brushes::*;
pub use icons::*;
pub use signals::{SystemSignal, SignalType, SignalPrimitive, StrategySignalConfig};
