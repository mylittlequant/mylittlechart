//! Primitive Registry - factory pattern for creating primitives
//!
//! This allows adding new primitives without modifying DrawingManager.
//! Each primitive type registers itself with metadata and a factory function.

use std::collections::HashMap;
use std::sync::{RwLock, OnceLock};
use super::traits::{Primitive, PrimitiveKind, ClickBehavior};

/// Factory function type for creating primitives
pub type PrimitiveFactory = fn(points: &[(f64, f64)], color: &str) -> Box<dyn Primitive>;

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
    /// Whether this primitive has configurable control points (Elliott, Patterns - shows "Points" tab)
    pub has_points_config: bool,
}

/// Global primitive registry
///
/// Use `PrimitiveRegistry::global()` to access.
pub struct PrimitiveRegistry {
    primitives: HashMap<&'static str, PrimitiveMetadata>,
    by_kind: HashMap<PrimitiveKind, Vec<&'static str>>,
}

impl PrimitiveRegistry {
    /// Create a new empty registry
    pub fn new() -> Self {
        Self {
            primitives: HashMap::new(),
            by_kind: HashMap::new(),
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
            .or_insert_with(Vec::new)
            .push(type_id);
    }

    /// Get metadata for a primitive type
    pub fn get(&self, type_id: &str) -> Option<&PrimitiveMetadata> {
        self.primitives.get(type_id)
    }

    /// Create a primitive by type ID
    pub fn create(&self, type_id: &str, points: &[(f64, f64)], color: Option<&str>) -> Option<Box<dyn Primitive>> {
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

    /// Check if primitive type has configurable control points (Elliott, Patterns)
    pub fn has_points_config(&self, type_id: &str) -> bool {
        self.primitives.get(type_id).map(|m| m.has_points_config).unwrap_or(false)
    }

    /// Create a primitive from JSON (for undo/redo)
    /// Note: This only supports primitives that implement serde
    pub fn from_json(&self, type_id: &str, json: &str) -> Option<Box<dyn Primitive>> {
        match type_id {
            "fib_retracement" => {
                use super::fibonacci::retracement::FibRetracement;
                serde_json::from_str::<FibRetracement>(json)
                    .ok()
                    .map(|p| Box::new(p) as Box<dyn Primitive>)
            }
            "trend_line" => {
                use super::lines::trend_line::TrendLine;
                serde_json::from_str::<TrendLine>(json)
                    .ok()
                    .map(|p| Box::new(p) as Box<dyn Primitive>)
            }
            "horizontal_line" => {
                use super::lines::horizontal_line::HorizontalLine;
                serde_json::from_str::<HorizontalLine>(json)
                    .ok()
                    .map(|p| Box::new(p) as Box<dyn Primitive>)
            }
            "vertical_line" => {
                use super::lines::vertical_line::VerticalLine;
                serde_json::from_str::<VerticalLine>(json)
                    .ok()
                    .map(|p| Box::new(p) as Box<dyn Primitive>)
            }
            "rectangle" => {
                use super::shapes::rectangle::Rectangle;
                serde_json::from_str::<Rectangle>(json)
                    .ok()
                    .map(|p| Box::new(p) as Box<dyn Primitive>)
            }
            // For other types, fall back to re-creating from points
            _ => {
                // Try to extract points from JSON and recreate
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(json) {
                    if let Some(points_arr) = value.get("points").and_then(|p| p.as_array()) {
                        let points: Vec<(f64, f64)> = points_arr.iter()
                            .filter_map(|p| {
                                let arr = p.as_array()?;
                                Some((arr.first()?.as_f64()?, arr.get(1)?.as_f64()?))
                            })
                            .collect();
                        let color = value.get("data")
                            .and_then(|d| d.get("color"))
                            .and_then(|c| c.get("stroke"))
                            .and_then(|s| s.as_str());
                        return self.create(type_id, &points, color);
                    }
                }
                None
            }
        }
    }

    /// Register all built-in primitives
    fn register_builtins(&mut self) {
        // Lines
        self.register(super::lines::trend_line::metadata());
        self.register(super::lines::horizontal_line::metadata());
        self.register(super::lines::vertical_line::metadata());
        self.register(super::lines::ray::metadata());
        self.register(super::lines::extended_line::metadata());
        self.register(super::lines::info_line::metadata());
        self.register(super::lines::trend_angle::metadata());
        self.register(super::lines::horizontal_ray::metadata());
        self.register(super::lines::cross_line::metadata());

        // Channels
        self.register(super::channels::parallel_channel::metadata());
        self.register(super::channels::regression_trend::metadata());
        self.register(super::channels::flat_top_bottom::metadata());
        self.register(super::channels::disjoint_channel::metadata());

        // Shapes
        self.register(super::shapes::rectangle::metadata());
        self.register(super::shapes::circle::metadata());
        self.register(super::shapes::ellipse::metadata());
        self.register(super::shapes::triangle::metadata());
        self.register(super::shapes::arc::metadata());
        self.register(super::shapes::polyline::metadata());
        self.register(super::shapes::path::metadata());
        self.register(super::shapes::rotated_rectangle::metadata());
        self.register(super::shapes::curve::metadata());
        self.register(super::shapes::double_curve::metadata());

        // Fibonacci
        self.register(super::fibonacci::retracement::metadata());
        self.register(super::fibonacci::trend_extension::metadata());
        self.register(super::fibonacci::channel::metadata());
        self.register(super::fibonacci::time_zones::metadata());
        self.register(super::fibonacci::speed_resistance::metadata());
        self.register(super::fibonacci::trend_time::metadata());
        self.register(super::fibonacci::circles::metadata());
        self.register(super::fibonacci::spiral::metadata());
        self.register(super::fibonacci::arcs::metadata());
        self.register(super::fibonacci::wedge::metadata());
        self.register(super::fibonacci::fan::metadata());

        // Pitchforks
        self.register(super::pitchforks::pitchfork::metadata());
        self.register(super::pitchforks::schiff::metadata());
        self.register(super::pitchforks::modified_schiff::metadata());
        self.register(super::pitchforks::inside_pitchfork::metadata());

        // Gann
        self.register(super::gann::gann_box::metadata());
        self.register(super::gann::gann_square_fixed::metadata());
        self.register(super::gann::gann_square::metadata());
        self.register(super::gann::gann_fan::metadata());

        // Arrows
        self.register(super::arrows::arrow_line::metadata());

        // Annotations
        self.register(super::annotations::text::metadata());
        self.register(super::annotations::anchored_text::metadata());
        self.register(super::annotations::note::metadata());
        self.register(super::annotations::price_note::metadata());
        self.register(super::annotations::signpost::metadata());
        self.register(super::annotations::callout::metadata());
        self.register(super::annotations::comment::metadata());
        self.register(super::annotations::price_label::metadata());
        self.register(super::annotations::sign::metadata());
        self.register(super::annotations::flag::metadata());
        self.register(super::annotations::table::metadata());
        self.register(super::annotations::triangle_up::metadata());
        self.register(super::annotations::triangle_down::metadata());

        // Patterns
        self.register(super::patterns::xabcd_pattern::metadata());
        self.register(super::patterns::cypher_pattern::metadata());
        self.register(super::patterns::head_shoulders::metadata());
        self.register(super::patterns::abcd_pattern::metadata());
        self.register(super::patterns::triangle_pattern::metadata());
        self.register(super::patterns::three_drives::metadata());

        // Elliott
        self.register(super::elliott::elliott_impulse::metadata());
        self.register(super::elliott::elliott_correction::metadata());
        self.register(super::elliott::elliott_triangle::metadata());
        self.register(super::elliott::elliott_double_combo::metadata());
        self.register(super::elliott::elliott_triple_combo::metadata());

        // Cycles
        self.register(super::cycles::cycle_lines::metadata());
        self.register(super::cycles::time_cycles::metadata());
        self.register(super::cycles::sine_wave::metadata());

        // Projection
        self.register(super::projection::long_position::metadata());
        self.register(super::projection::short_position::metadata());
        self.register(super::projection::bars_pattern::metadata());
        self.register(super::projection::price_projection::metadata());
        self.register(super::projection::projection::metadata());

        // Volume
        self.register(super::volume::fixed_volume_profile::metadata());
        self.register(super::volume::anchored_volume_profile::metadata());

        // Measurement
        self.register(super::measurement::price_range::metadata());
        self.register(super::measurement::date_range::metadata());
        self.register(super::measurement::price_date_range::metadata());

        // Brushes
        self.register(super::brushes::brush::metadata());
        self.register(super::brushes::highlighter::metadata());

        // Icons
        self.register(super::icons::image::metadata());
        self.register(super::icons::emoji::metadata());

        // Individual emoji primitives (36 types)
        for meta in super::icons::emoji::all_emoji_metadata() {
            self.register(meta);
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
                has_points_config: false,
            }
        }
    };
}
