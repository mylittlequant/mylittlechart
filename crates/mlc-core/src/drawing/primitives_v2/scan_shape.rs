//! The shared shape every SURVEY primitive (`SmcScan`, `CandlePatternScan`,
//! `OrderFlowScan`) is bracketed by — an autodetector is a range primitive
//! the user stretches over a stretch of chart, and that stretch is either
//! two vertical lines (today's only shape, a bar-index RANGE with no price
//! bound) or a rectangle (a bar-index range AND a price band, both bounding
//! what counts as a hit).
//!
//! One shared type and one shared render/filter helper for all three
//! scanners — per the arc's explicit "no per-family copies" instruction
//! (`docs/mlc/plans/autodetectors-a0-implementation-2026-09-24.md`, piece 7).
//! Every scan struct carries its own `shape`/`p1`/`p2` fields (`#[serde(default)]`,
//! so a scan saved before this existed loads as `TwoLines` with `p1 == p2 ==
//! 0.0`, byte-for-byte the shape it always drew); this module owns the ONE
//! place that shape is drawn and the ONE place a hit is tested against it.

use serde::{Deserialize, Serialize};

use super::{crisp, RenderContext};

/// Which shape a survey's own two control points bracket.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ScanShape {
    /// Two full-height vertical brackets at the survey's own `ts1`/`ts2` —
    /// today's only shape, unchanged pixels. Every hit inside the time
    /// range counts; there is no price bound.
    #[default]
    TwoLines,
    /// A bounded rectangle over `[ts1, ts2] x [p1, p2]` — a hit whose own
    /// price falls outside `[p1, p2]` is excluded from the survey's own
    /// results (see [`price_band_filter`]), not merely drawn past the edge.
    Rectangle,
}

impl ScanShape {
    /// The wire id this shape serializes as on the Coverage tab's own Shape
    /// row (`markup_engine_settings`'s segmented toggle) — a plain string
    /// round-tripped through `PropertyValue::String`, the same convention
    /// `coverage`'s own `"range"`/`"all_bars"` pair already uses.
    pub fn id(self) -> &'static str {
        match self {
            Self::TwoLines => "two_lines",
            Self::Rectangle => "rectangle",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "two_lines" => Some(Self::TwoLines),
            "rectangle" => Some(Self::Rectangle),
            _ => None,
        }
    }
}

/// Draw either two full-height dashed vertical brackets at `x1`/`x2`
/// (`ScanShape::TwoLines` — today's only shape, unchanged pixels) or a
/// bounded, low-alpha-filled rectangle over `[x1, x2] x` the screen Y of
/// `[p1, p2]` (`ScanShape::Rectangle`), stroked in `color` at `width`. The
/// ONE place any of the three scan primitives draws its own bracket —
/// `render()`'s caller resolves `x1`/`x2` first (bar-index screen X in
/// automatic coverage, timestamp screen X otherwise — a resolution that
/// differs per scanner and stays in each file, see `SmcScan::render`'s own
/// doc), then hands this function already-resolved screen coordinates.
///
/// Rectangle mode's own fill alpha (`"29"` hex suffix) matches
/// `smc::render_shapes::draw_zone`'s own zone fill — a survey's bracket and
/// a survey's own hit zones read as the same family of shape.
pub fn render_scan_bracket(
    ctx: &mut dyn RenderContext,
    shape: ScanShape,
    x1: f64,
    x2: f64,
    p1: f64,
    p2: f64,
    color: &str,
    width: f64,
) {
    let dpr = ctx.dpr();
    let (min_x, max_x) = (x1.min(x2), x1.max(x2));

    match shape {
        ScanShape::TwoLines => {
            let chart_height = ctx.chart_height();
            ctx.set_stroke_color(color);
            ctx.set_stroke_width(width);
            ctx.set_line_dash(&[4.0 * dpr, 4.0 * dpr]);
            for x in [min_x, max_x] {
                ctx.begin_path();
                ctx.move_to(crisp(x, dpr), 0.0);
                ctx.line_to(crisp(x, dpr), chart_height);
                ctx.stroke();
            }
            ctx.set_line_dash(&[]);
        }
        ScanShape::Rectangle => {
            let y1 = ctx.price_to_y(p1);
            let y2 = ctx.price_to_y(p2);
            let (min_y, max_y) = (y1.min(y2), y1.max(y2));
            let fill = format!("{color}29");
            ctx.set_fill_color(&fill);
            ctx.fill_rect(min_x, min_y, max_x - min_x, max_y - min_y);
            ctx.set_stroke_color(color);
            ctx.set_stroke_width(width);
            ctx.set_line_dash(&[4.0 * dpr, 4.0 * dpr]);
            ctx.stroke_rect(min_x, min_y, max_x - min_x, max_y - min_y);
            ctx.set_line_dash(&[]);
        }
    }
}

/// `Rectangle` mode only: `true` when `hit_price` falls inside
/// `[min(p1, p2), max(p1, p2)]` — a hit outside the band is excluded from
/// the survey's own results BEFORE it reaches `cached_hits`, not merely
/// drawn past the rectangle's own edge (so the hit table / Stat block never
/// disagree with what the rectangle visually bounds). `TwoLines` mode is a
/// no-op passthrough: every hit in the time range counts, exactly as it did
/// before this shape existed.
pub fn price_band_filter(shape: ScanShape, p1: f64, p2: f64, hit_price: f64) -> bool {
    match shape {
        ScanShape::TwoLines => true,
        ScanShape::Rectangle => {
            let (lo, hi) = (p1.min(p2), p1.max(p2));
            hit_price >= lo && hit_price <= hi
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_shape_is_two_lines() {
        assert_eq!(ScanShape::default(), ScanShape::TwoLines);
    }

    #[test]
    fn ids_round_trip() {
        for shape in [ScanShape::TwoLines, ScanShape::Rectangle] {
            assert_eq!(ScanShape::from_id(shape.id()), Some(shape));
        }
        assert_eq!(ScanShape::from_id("bogus"), None);
    }

    #[test]
    fn two_lines_never_filters_regardless_of_price() {
        assert!(price_band_filter(ScanShape::TwoLines, 10.0, 20.0, 5.0));
        assert!(price_band_filter(ScanShape::TwoLines, 10.0, 20.0, 25.0));
        assert!(price_band_filter(ScanShape::TwoLines, 10.0, 20.0, f64::NAN));
    }

    #[test]
    fn rectangle_keeps_only_prices_inside_the_band() {
        assert!(price_band_filter(ScanShape::Rectangle, 10.0, 20.0, 15.0));
        assert!(price_band_filter(ScanShape::Rectangle, 10.0, 20.0, 10.0), "band is inclusive at the low edge");
        assert!(price_band_filter(ScanShape::Rectangle, 10.0, 20.0, 20.0), "band is inclusive at the high edge");
        assert!(!price_band_filter(ScanShape::Rectangle, 10.0, 20.0, 5.0));
        assert!(!price_band_filter(ScanShape::Rectangle, 10.0, 20.0, 25.0));
    }

    #[test]
    fn rectangle_band_works_with_reversed_endpoints() {
        // p1/p2 come from two raw clicks — nothing orders them low-to-high.
        assert!(price_band_filter(ScanShape::Rectangle, 20.0, 10.0, 15.0));
        assert!(!price_band_filter(ScanShape::Rectangle, 20.0, 10.0, 25.0));
    }
}
