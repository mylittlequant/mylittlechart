//! Primitive Registry - factory pattern for creating primitives
//!
//! This allows adding new primitives without modifying DrawingManager.
//! Each primitive type registers itself with metadata and a factory function.

use std::collections::HashMap;
use std::sync::{RwLock, OnceLock};
use super::traits::{Primitive, PrimitiveKind, ClickBehavior};

/// Factory function type for creating primitives
pub type PrimitiveFactory = fn(points: &[(i64, f64)], color: &str) -> Box<dyn Primitive>;

/// Metadata about a primitive type
#[derive(Clone)]
pub struct PrimitiveMetadata {
    /// Unique type ID (e.g., "trend_line")
    pub type_id: &'static str,
    /// Display name for UI
    pub display_name: &'static str,
    /// Category for toolbar organization
    pub kind: PrimitiveKind,
    /// How to create this primitive
    pub click_behavior: ClickBehavior,
    /// Tooltip text
    pub tooltip: &'static str,
    /// Icon ID for toolbar
    pub icon: &'static str,
    /// Factory function
    pub factory: PrimitiveFactory,
    /// Default color
    pub default_color: &'static str,
    /// Whether this primitive supports text labels (shows "Text" tab in settings)
    pub supports_text: bool,
    /// Whether this primitive has configurable levels (Fibonacci, Gann, Pitchfork - shows "Levels" tab)
    pub has_levels: bool,
}

/// Global primitive registry
///
/// Use `PrimitiveRegistry::global()` to access.
pub struct PrimitiveRegistry {
    primitives: HashMap<&'static str, PrimitiveMetadata>,
    by_kind: HashMap<PrimitiveKind, Vec<&'static str>>,
    /// How to rebuild each type from the JSON it wrote. Filled by
    /// [`PrimitiveRegistry::register_typed`], which is the only registration
    /// that knows the concrete type — see [`PrimitiveRegistry::from_json`].
    deserializers: HashMap<&'static str, DeserializeFn>,
}

/// Rebuild one primitive from its own `to_json` output.
type DeserializeFn = fn(&str) -> Option<Box<dyn Primitive>>;

impl PrimitiveRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            primitives: HashMap::new(),
            by_kind: HashMap::new(),
            deserializers: HashMap::new(),
        }
    }

    /// Get the global registry instance
    pub fn global() -> &'static RwLock<PrimitiveRegistry> {
        static REGISTRY: OnceLock<RwLock<PrimitiveRegistry>> = OnceLock::new();
        REGISTRY.get_or_init(|| {
            let mut registry = PrimitiveRegistry::new();
            // Register built-in primitives
            registry.register_builtins();
            RwLock::new(registry)
        })
    }

    /// Register a primitive type
    pub fn register(&mut self, metadata: PrimitiveMetadata) {
        let type_id = metadata.type_id;
        let kind = metadata.kind;

        self.primitives.insert(type_id, metadata);
        self.by_kind
            .entry(kind)
            .or_default()
            .push(type_id);
    }

    /// Register a primitive type together with the way to read it back.
    ///
    /// This is what [`register`](Self::register) cannot do: it takes an
    /// already-erased `PrimitiveMetadata`, so nothing at that point knows the
    /// concrete type and `from_json` had to carry a hand-written match arm.
    /// It carried five, out of 121 registered types.
    pub fn register_typed<T>(&mut self, metadata: PrimitiveMetadata)
    where
        T: Primitive + serde::de::DeserializeOwned + 'static,
    {
        self.deserializers.insert(metadata.type_id, |json| {
            serde_json::from_str::<T>(json)
                .ok()
                .map(|p| Box::new(p) as Box<dyn Primitive>)
        });
        self.register(metadata);
    }

    /// Get metadata for a primitive type
    pub fn get(&self, type_id: &str) -> Option<&PrimitiveMetadata> {
        self.primitives.get(type_id)
    }

    /// Create a primitive by type ID
    pub fn create(&self, type_id: &str, points: &[(i64, f64)], color: Option<&str>) -> Option<Box<dyn Primitive>> {
        let meta = self.primitives.get(type_id)?;
        let color = color.unwrap_or(meta.default_color);
        Some((meta.factory)(points, color))
    }

    /// Get all primitive type IDs in a category
    pub fn by_kind(&self, kind: PrimitiveKind) -> &[&'static str] {
        self.by_kind.get(&kind).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Get all registered primitive types
    pub fn all(&self) -> impl Iterator<Item = &PrimitiveMetadata> {
        self.primitives.values()
    }

    /// Get click behavior for a type
    pub fn click_behavior(&self, type_id: &str) -> Option<ClickBehavior> {
        self.primitives.get(type_id).map(|m| m.click_behavior)
    }

    /// Check if type requires single click
    pub fn is_single_click(&self, type_id: &str) -> bool {
        matches!(
            self.click_behavior(type_id),
            Some(ClickBehavior::SingleClick)
        )
    }

    /// Check if type requires two clicks
    pub fn is_two_point(&self, type_id: &str) -> bool {
        matches!(
            self.click_behavior(type_id),
            Some(ClickBehavior::TwoPoint) | Some(ClickBehavior::ClickDrag)
        )
    }

    /// Check if primitive type has configurable levels (Fibonacci, Gann, Pitchfork)
    pub fn has_levels(&self, type_id: &str) -> bool {
        self.primitives.get(type_id).map(|m| m.has_levels).unwrap_or(false)
    }

    /// Check if primitive type supports text
    pub fn supports_text(&self, type_id: &str) -> bool {
        self.primitives.get(type_id).map(|m| m.supports_text).unwrap_or(false)
    }

    /// Rebuild a primitive from the JSON it wrote.
    ///
    /// This is the hydration path — a type that answers `None` here is a
    /// drawing the user loses on reload, and it is also the undo/redo path.
    /// It used to be a hand-written match of five arms plus a fallback that
    /// looked for a top-level `points` array; only a dozen primitives have a
    /// field by that name, so **104 of 121 registered types did not survive a
    /// round trip**. Every type now supplies its own reader through
    /// [`register_typed`](Self::register_typed).
    ///
    /// The `points` fallback stays for a snapshot written before a type's
    /// serde shape changed: geometry back is better than the drawing gone.
    pub fn from_json(&self, type_id: &str, json: &str) -> Option<Box<dyn Primitive>> {
        if let Some(read) = self.deserializers.get(type_id) {
            if let Some(p) = read(json) {
                return Some(p);
            }
        }
        // Legacy / shape-drift fallback: recreate from a `points` array.
        let value = serde_json::from_str::<serde_json::Value>(json).ok()?;
        let points_arr = value.get("points")?.as_array()?;
        let points: Vec<(i64, f64)> = points_arr
            .iter()
            .filter_map(|p| {
                let arr = p.as_array()?;
                Some((arr.first()?.as_i64()?, arr.get(1)?.as_f64()?))
            })
            .collect();
        let color = value
            .get("data")
            .and_then(|d| d.get("color"))
            .and_then(|c| c.get("stroke"))
            .and_then(|s| s.as_str());
        self.create(type_id, &points, color)
    }

    /// Register all built-in primitives
    fn register_builtins(&mut self) {
        // Lines
        self.register_typed::<super::lines::trend_line::TrendLine>(super::lines::trend_line::metadata());
        self.register_typed::<super::lines::horizontal_line::HorizontalLine>(super::lines::horizontal_line::metadata());
        self.register_typed::<super::lines::vertical_line::VerticalLine>(super::lines::vertical_line::metadata());
        self.register_typed::<super::lines::ray::Ray>(super::lines::ray::metadata());
        self.register_typed::<super::lines::extended_line::ExtendedLine>(super::lines::extended_line::metadata());
        self.register_typed::<super::lines::info_line::InfoLine>(super::lines::info_line::metadata());
        self.register_typed::<super::lines::trend_angle::TrendAngle>(super::lines::trend_angle::metadata());
        self.register_typed::<super::lines::horizontal_ray::HorizontalRay>(super::lines::horizontal_ray::metadata());
        self.register_typed::<super::lines::cross_line::CrossLine>(super::lines::cross_line::metadata());

        // Channels
        self.register_typed::<super::channels::parallel_channel::ParallelChannel>(super::channels::parallel_channel::metadata());
        self.register_typed::<super::channels::regression_trend::RegressionTrend>(super::channels::regression_trend::metadata());
        self.register_typed::<super::channels::flat_top_bottom::FlatTopBottom>(super::channels::flat_top_bottom::metadata());
        self.register_typed::<super::channels::disjoint_channel::DisjointChannel>(super::channels::disjoint_channel::metadata());

        // Shapes
        self.register_typed::<super::shapes::rectangle::Rectangle>(super::shapes::rectangle::metadata());
        self.register_typed::<super::shapes::circle::Circle>(super::shapes::circle::metadata());
        self.register_typed::<super::shapes::ellipse::Ellipse>(super::shapes::ellipse::metadata());
        self.register_typed::<super::shapes::triangle::Triangle>(super::shapes::triangle::metadata());
        self.register_typed::<super::shapes::arc::Arc>(super::shapes::arc::metadata());
        self.register_typed::<super::shapes::polyline::Polyline>(super::shapes::polyline::metadata());
        self.register_typed::<super::shapes::path::Path>(super::shapes::path::metadata());
        self.register_typed::<super::shapes::rotated_rectangle::RotatedRectangle>(super::shapes::rotated_rectangle::metadata());
        self.register_typed::<super::shapes::curve::Curve>(super::shapes::curve::metadata());
        self.register_typed::<super::shapes::double_curve::DoubleCurve>(super::shapes::double_curve::metadata());

        // Fibonacci
        self.register_typed::<super::fibonacci::retracement::FibRetracement>(super::fibonacci::retracement::metadata());
        self.register_typed::<super::fibonacci::trend_extension::FibTrendExtension>(super::fibonacci::trend_extension::metadata());
        self.register_typed::<super::fibonacci::channel::FibChannel>(super::fibonacci::channel::metadata());
        self.register_typed::<super::fibonacci::time_zones::FibTimeZones>(super::fibonacci::time_zones::metadata());
        self.register_typed::<super::fibonacci::speed_resistance::FibSpeedResistance>(super::fibonacci::speed_resistance::metadata());
        self.register_typed::<super::fibonacci::trend_time::FibTrendTime>(super::fibonacci::trend_time::metadata());
        self.register_typed::<super::fibonacci::circles::FibCircles>(super::fibonacci::circles::metadata());
        self.register_typed::<super::fibonacci::spiral::FibSpiral>(super::fibonacci::spiral::metadata());
        self.register_typed::<super::fibonacci::arcs::FibArcs>(super::fibonacci::arcs::metadata());
        self.register_typed::<super::fibonacci::wedge::FibWedge>(super::fibonacci::wedge::metadata());
        self.register_typed::<super::fibonacci::fan::FibFan>(super::fibonacci::fan::metadata());

        // Pitchforks
        self.register_typed::<super::pitchforks::pitchfork::Pitchfork>(super::pitchforks::pitchfork::metadata());
        self.register_typed::<super::pitchforks::schiff::SchiffPitchfork>(super::pitchforks::schiff::metadata());
        self.register_typed::<super::pitchforks::modified_schiff::ModifiedSchiff>(super::pitchforks::modified_schiff::metadata());
        self.register_typed::<super::pitchforks::inside_pitchfork::InsidePitchfork>(super::pitchforks::inside_pitchfork::metadata());

        // Gann
        self.register_typed::<super::gann::gann_box::GannBox>(super::gann::gann_box::metadata());
        self.register_typed::<super::gann::gann_square_fixed::GannSquareFixed>(super::gann::gann_square_fixed::metadata());
        self.register_typed::<super::gann::gann_square::GannSquare>(super::gann::gann_square::metadata());
        self.register_typed::<super::gann::gann_fan::GannFan>(super::gann::gann_fan::metadata());

        // Arrows
        self.register_typed::<super::arrows::arrow_line::ArrowLine>(super::arrows::arrow_line::metadata());

        // Annotations
        self.register_typed::<super::annotations::text::Text>(super::annotations::text::metadata());
        self.register_typed::<super::annotations::anchored_text::AnchoredText>(super::annotations::anchored_text::metadata());
        self.register_typed::<super::annotations::note::Note>(super::annotations::note::metadata());
        self.register_typed::<super::annotations::price_note::PriceNote>(super::annotations::price_note::metadata());
        self.register_typed::<super::annotations::signpost::Signpost>(super::annotations::signpost::metadata());
        self.register_typed::<super::annotations::callout::Callout>(super::annotations::callout::metadata());
        self.register_typed::<super::annotations::comment::Comment>(super::annotations::comment::metadata());
        self.register_typed::<super::annotations::price_label::PriceLabel>(super::annotations::price_label::metadata());
        self.register_typed::<super::annotations::sign::Sign>(super::annotations::sign::metadata());
        self.register_typed::<super::annotations::flag::Flag>(super::annotations::flag::metadata());
        self.register_typed::<super::annotations::table::Table>(super::annotations::table::metadata());
        self.register_typed::<super::annotations::triangle_up::TriangleUp>(super::annotations::triangle_up::metadata());
        self.register_typed::<super::annotations::triangle_down::TriangleDown>(super::annotations::triangle_down::metadata());

        // Patterns
        self.register_typed::<super::patterns::xabcd_pattern::XabcdPattern>(super::patterns::xabcd_pattern::metadata());
        self.register_typed::<super::patterns::cypher_pattern::CypherPattern>(super::patterns::cypher_pattern::metadata());
        self.register_typed::<super::patterns::head_shoulders::HeadShoulders>(super::patterns::head_shoulders::metadata());
        self.register_typed::<super::patterns::abcd_pattern::AbcdPattern>(super::patterns::abcd_pattern::metadata());
        self.register_typed::<super::patterns::triangle_pattern::TrianglePattern>(super::patterns::triangle_pattern::metadata());
        self.register_typed::<super::patterns::three_drives::ThreeDrives>(super::patterns::three_drives::metadata());
        self.register_typed::<super::patterns::candle_pattern::CandlePattern>(super::patterns::candle_pattern::metadata());
        self.register_typed::<super::patterns::candle_pattern_scan::CandlePatternScan>(super::patterns::candle_pattern_scan::metadata());

        // Smart money (markup-engines wave, 2026-08-25)
        self.register_typed::<super::smc::order_block::OrderBlock>(super::smc::order_block::metadata());
        self.register_typed::<super::smc::fair_value_gap::FairValueGap>(super::smc::fair_value_gap::metadata());
        self.register_typed::<super::smc::liquidity_sweep::LiquiditySweep>(super::smc::liquidity_sweep::metadata());
        self.register_typed::<super::smc::structure_break::StructureBreak>(super::smc::structure_break::metadata());

        // Elliott
        self.register_typed::<super::elliott::elliott_impulse::ElliottImpulse>(super::elliott::elliott_impulse::metadata());
        self.register_typed::<super::elliott::elliott_correction::ElliottCorrection>(super::elliott::elliott_correction::metadata());
        self.register_typed::<super::elliott::elliott_triangle::ElliottTriangle>(super::elliott::elliott_triangle::metadata());
        self.register_typed::<super::elliott::elliott_double_combo::ElliottDoubleCombo>(super::elliott::elliott_double_combo::metadata());
        self.register_typed::<super::elliott::elliott_triple_combo::ElliottTripleCombo>(super::elliott::elliott_triple_combo::metadata());

        // Cycles
        self.register_typed::<super::cycles::cycle_lines::CycleLines>(super::cycles::cycle_lines::metadata());
        self.register_typed::<super::cycles::time_cycles::TimeCycles>(super::cycles::time_cycles::metadata());
        self.register_typed::<super::cycles::sine_wave::SineWave>(super::cycles::sine_wave::metadata());

        // Projection
        self.register_typed::<super::projection::long_position::LongPosition>(super::projection::long_position::metadata());
        self.register_typed::<super::projection::short_position::ShortPosition>(super::projection::short_position::metadata());
        self.register_typed::<super::projection::bars_pattern::BarsPattern>(super::projection::bars_pattern::metadata());
        self.register_typed::<super::projection::price_projection::PriceProjection>(super::projection::price_projection::metadata());
        self.register_typed::<super::projection::projection::Projection>(super::projection::projection::metadata());

        // Volume
        self.register_typed::<super::volume::fixed_volume_profile::FixedVolumeProfile>(super::volume::fixed_volume_profile::metadata());
        self.register_typed::<super::volume::anchored_volume_profile::AnchoredVolumeProfile>(super::volume::anchored_volume_profile::metadata());
        self.register_typed::<super::volume::anchored_vwap::AnchoredVwap>(super::volume::anchored_vwap::metadata());
        self.register_typed::<super::volume::fixed_vwap::FixedVwap>(super::volume::fixed_vwap::metadata());

        // Measurement
        self.register_typed::<super::measurement::price_range::PriceRange>(super::measurement::price_range::metadata());
        self.register_typed::<super::measurement::date_range::DateRange>(super::measurement::date_range::metadata());
        self.register_typed::<super::measurement::price_date_range::PriceDateRange>(super::measurement::price_date_range::metadata());

        // Brushes
        self.register_typed::<super::brushes::brush::Brush>(super::brushes::brush::metadata());
        self.register_typed::<super::brushes::highlighter::Highlighter>(super::brushes::highlighter::metadata());

        // Icons
        self.register_typed::<super::icons::image::Image>(super::icons::image::metadata());
        self.register_typed::<super::icons::emoji::Emoji>(super::icons::emoji::metadata());

        // Individual emoji primitives (36 type_ids, all one struct)
        for meta in super::icons::emoji::all_emoji_metadata() {
            self.register_typed::<super::icons::emoji::Emoji>(meta);
        }
    }
}

impl Default for PrimitiveRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper macro to define primitive metadata
#[macro_export]
macro_rules! define_primitive {
    (
        type_id: $type_id:literal,
        display_name: $display_name:literal,
        kind: $kind:expr,
        click_behavior: $click:expr,
        tooltip: $tooltip:literal,
        icon: $icon:literal,
        default_color: $color:literal,
        factory: $factory:expr $(,)?
    ) => {
        pub fn metadata() -> $crate::drawing::primitives::PrimitiveMetadata {
            $crate::drawing::primitives::PrimitiveMetadata {
                type_id: $type_id,
                display_name: $display_name,
                kind: $kind,
                click_behavior: $click,
                tooltip: $tooltip,
                icon: $icon,
                default_color: $color,
                factory: $factory,
                supports_text: true,
                has_levels: false,
            }
        }
    };
}

#[cfg(test)]
mod hydration {
    use super::*;

    /// Every registered type must survive `to_json` → `from_json`. That pair is
    /// the hydration path, so a type that fails here is a drawing the user
    /// loses on reload.
    #[test]
    fn every_registered_type_survives_a_round_trip() {
        let reg = PrimitiveRegistry::global().read().unwrap();
        let pts = [
            (1_700_000_000_000i64, 100.0f64),
            (1_700_003_600_000, 110.0),
            (1_700_007_200_000, 105.0),
            (1_700_010_800_000, 115.0),
            (1_700_014_400_000, 108.0),
            (1_700_018_000_000, 112.0),
            (1_700_021_600_000, 109.0),
        ];
        let mut lost = Vec::new();
        let mut kept = Vec::new();
        let mut ids: Vec<&str> = reg.all().map(|m| m.type_id).collect();
        ids.sort_unstable();
        for id in ids {
            let Some(p) = reg.create(id, &pts, None) else { lost.push(format!("{id} (no factory)")); continue };
            let json = p.to_json();
            match reg.from_json(id, &json) {
                Some(back) => {
                    if back.points() == p.points() { kept.push(id) } else { lost.push(format!("{id} (points differ)")) }
                }
                None => lost.push(format!("{id} (from_json None)")),
            }
        }
        assert!(lost.is_empty(), "{} of {} primitive types do not survive a round trip", lost.len(), lost.len() + kept.len());
    }
}

#[cfg(test)]
mod hit_test_safety_net {
    //! The hit-test seam's safety net (2026-08-07): `Primitive::control_points`
    //! already returns every handle with its screen coordinates, and
    //! `Primitive::geometry` (default: empty; a family opts in by overriding
    //! it) is meant to return the strokes a primitive draws. Both this
    //! module's tests exist to make two classes of defect impossible to ship
    //! silently again:
    //!
    //! 1. A handle `control_points()` declares that nothing can ever grab
    //!    (a per-primitive `hit_test` override whose own proximity math
    //!    disagrees with what it just advertised — the exact bug class a
    //!    prior arc found fourteen instances of, one hand-written radius
    //!    literal at a time).
    //! 2. A segment `geometry()` reports that its own `hit_test` cannot
    //!    find — impossible today (every primitive's `geometry()` is still
    //!    the empty default; no family has migrated), so this half of the
    //!    test is currently a no-op that activates automatically as each
    //!    family adopts `geometry()`.
    //!
    //! Every registered type is constructed through its own factory with the
    //! SAME sample points the round-trip test above uses (proven to
    //! construct every type without panicking), plus a matching `bars`/
    //! `viewport`/`price_scale` triplet. For each control point, a click at
    //! its EXACT reported coordinate must resolve to THAT SAME
    //! `ControlPointType` via the primitive's own `hit_test` (whichever body
    //! it runs — its own override, today, for every registered type, or the
    //! new trait default once a primitive relies on it). For each `geometry`
    //! segment, a click at its midpoint must resolve to `Body`.
    //!
    //! Every registered type constructs from the round-trip test's 7-point
    //! fixture (verified above), so none is excluded here for construction
    //! reasons.
    use super::*;
    use crate::{Bar, PriceScale, Viewport};
    use super::super::traits::HitTestResult;

    /// Same fixture as `hydration::every_registered_type_survives_a_round_trip`
    /// — hourly points, price drifting 100..115 — plus a matching `bars`
    /// slice so `timestamp_ms_to_bar_f64` resolves every timestamp to a real
    /// (not extrapolated) bar index.
    fn fixture() -> (Vec<(i64, f64)>, Vec<Bar>, Viewport, PriceScale) {
        let pts = vec![
            (1_700_000_000_000i64, 100.0f64),
            (1_700_003_600_000, 110.0),
            (1_700_007_200_000, 105.0),
            (1_700_010_800_000, 115.0),
            (1_700_014_400_000, 108.0),
            (1_700_018_000_000, 112.0),
            (1_700_021_600_000, 109.0),
        ];
        let bars: Vec<Bar> = pts
            .iter()
            .map(|(ts_ms, price)| Bar::new(ts_ms / 1000, *price, *price + 2.0, *price - 2.0, *price))
            .collect();
        let viewport = Viewport {
            view_start: 0.0,
            // NOT 8.0 — at 8px spacing consecutive control points land
            // closer together than CONTROL_POINT_HIT_RADIUS, so a click on
            // one legitimately resolves to its neighbour. That is a fixture
            // defect, not a hit-test defect: it inflated this test to 164
            // false-positive mismatches across 121 types (108 of them the
            // same 3 lines repeated over the 36 emoji glyph ids sharing one
            // struct). 60px spacing separates every real handle and leaves
            // 9 genuine mismatches. Do not tidy this back down.
            bar_spacing: 60.0,
            bar_width_ratio: 0.8,
            chart_width: 1200.0,
            chart_height: 800.0,
            bar_count: bars.len(),
        };
        let price_scale = PriceScale { price_min: 50.0, price_max: 150.0, ..Default::default() };
        (pts, bars, viewport, price_scale)
    }

    #[test]
    fn every_registered_type_control_points_and_geometry_are_hit_testable() {
        let reg = PrimitiveRegistry::global().read().unwrap();
        let (pts, bars, viewport, price_scale) = fixture();
        let mut ids: Vec<&str> = reg.all().map(|m| m.type_id).collect();
        ids.sort_unstable();

        let mut failures = Vec::new();
        let mut checked_types = 0usize;
        let mut checked_control_points = 0usize;
        let mut checked_geometry_segments = 0usize;

        for id in ids {
            let Some(p) = reg.create(id, &pts, None) else {
                failures.push(format!("{id}: no factory (cannot construct — see round-trip test)"));
                continue;
            };
            checked_types += 1;

            for cp in p.control_points(&bars, &viewport, &price_scale) {
                checked_control_points += 1;
                match p.hit_test(cp.x, cp.y, &bars, &viewport, &price_scale) {
                    HitTestResult::ControlPoint(found) if found == cp.point_type => {}
                    other => failures.push(format!(
                        "{id}: click at its own {:?} ({:.2}, {:.2}) resolved to {:?}, not ControlPoint({:?})",
                        cp.point_type, cp.x, cp.y, other, cp.point_type,
                    )),
                }
            }

            for seg in p.geometry(&bars, &viewport, &price_scale) {
                // A zero-length segment collapses to a point that may
                // legitimately coincide with a control point — not the
                // defect this half of the test targets.
                if (seg.x1 - seg.x2).abs() < 0.01 && (seg.y1 - seg.y2).abs() < 0.01 {
                    continue;
                }
                checked_geometry_segments += 1;
                let mx = (seg.x1 + seg.x2) / 2.0;
                let my = (seg.y1 + seg.y2) / 2.0;
                match p.hit_test(mx, my, &bars, &viewport, &price_scale) {
                    HitTestResult::Body => {}
                    other => failures.push(format!(
                        "{id}: click at its own drawn segment's midpoint ({:.2}, {:.2}) resolved to {:?}, not Body",
                        mx, my, other,
                    )),
                }
            }
        }

        assert!(
            failures.is_empty(),
            "{} hit-test mismatches across {} types ({} control points, {} geometry segments checked):\n{}",
            failures.len(), checked_types, checked_control_points, checked_geometry_segments,
            failures.join("\n"),
        );
    }
}

#[cfg(test)]
mod catalogue_dump {
    use super::*;
    use crate::state::icons::icon_svg;

    /// SVG uses double quotes for its attributes; single quotes are equally
    /// valid there, so swapping them keeps the markup usable inside a JSON
    /// string without an escaping pass.
    fn q(s: &str) -> String {
        s.replace(char::from(34), "'").replace(char::from(10), " ")
    }

    /// Dump every registered type: metadata plus its resolved SVG icon.
    ///
    /// Not a test — the machine half of the primitive audit, so the browser
    /// half only has to draw and photograph. Run with:
    /// `cargo test -p mylittlechart-core --lib dump_catalogue -- --ignored --nocapture`
    ///
    /// Resolves via `icon_svg()` — the SAME resolver the live toolbar rail
    /// calls (`toolbar_core::render_icon`) — not the narrower `Icon` enum,
    /// which never grew per-emoji-type variants and let this dumper report
    /// false negatives for every `emoji_*` primitive (2026-08-07 audit).
    #[test]
    #[ignore]
    fn dump_catalogue() {
        let reg = PrimitiveRegistry::global().read().unwrap();
        let mut ids: Vec<&str> = reg.all().map(|m| m.type_id).collect();
        ids.sort_unstable();
        println!("[");
        for (n, id) in ids.iter().enumerate() {
            let m = reg.get(id).unwrap();
            let svg = icon_svg(m.icon).unwrap_or("");
            let comma = if n + 1 < ids.len() { "," } else { "" };
            println!(
                "  {{\"type_id\":\"{}\",\"display_name\":\"{}\",\"kind\":\"{:?}\",\"click_behavior\":\"{:?}\",\"tooltip\":\"{}\",\"icon_name\":\"{}\",\"icon_resolved\":{},\"default_color\":\"{}\",\"supports_text\":{},\"has_levels\":{},\"svg\":\"{}\"}}{}",
                q(m.type_id),
                q(m.display_name),
                m.kind,
                m.click_behavior,
                q(m.tooltip),
                q(m.icon),
                !svg.is_empty(),
                q(m.default_color),
                m.supports_text,
                m.has_levels,
                q(svg),
                comma,
            );
        }
        println!("]");
    }
}
