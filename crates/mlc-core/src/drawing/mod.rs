//! Drawing system - interactive primitives with drag-and-drop support (v2)
//!
//! This module provides a complete drawing system using trait-based primitives.
//!
//! # Architecture
//!
//! - **PrimitiveRegistry**: Factory for creating primitives by type_id
//! - **Primitive trait**: Core trait all primitives implement
//! - **DrawingManager**: High-level manager for tool selection, creation, editing
//!
//! # Usage
//!
//! ```ignore
//! let mut drawing = DrawingManager::new();
//!
//! // Select a tool by type_id
//! drawing.set_tool(Some("trend_line"));
//!
//! // Handle clicks (coordinates in bar/price data space)
//! drawing.on_click(bar_idx as f64, price);
//!
//! // Get primitives to render
//! for prim in drawing.primitives() {
//!     // render using prim.points(), prim.data().color, etc.
//! }
//!
//! // Handle drag
//! if let Some(idx) = drawing.hit_test(x, y, viewport, price_scale) {
//!     drawing.start_drag(idx, bar, price);
//! }
//! drawing.update_drag(bar, price);
//! drawing.end_drag();
//! ```

use std::sync::atomic::{AtomicU64, Ordering};

/// Process-global primitive ID counter.
/// Initialized to 1. Seeded at startup from persisted state.
/// Use [`alloc_primitive_id`] to obtain a fresh ID.
static NEXT_PRIMITIVE_ID: AtomicU64 = AtomicU64::new(1);

/// Allocate the next globally-unique primitive ID.
///
/// This is the only place new IDs should originate. The counter is
/// monotonically increasing across the entire process lifetime, so IDs
/// allocated after a group join will never collide with IDs held in the
/// stash or any other store.
pub fn alloc_primitive_id() -> u64 {
    NEXT_PRIMITIVE_ID.fetch_add(1, Ordering::Relaxed)
}

/// Seed the global counter so it starts above all existing IDs.
///
/// Call once during app startup after loading persisted primitives.
/// Safe to call multiple times — it only advances the counter, never
/// decreases it.
pub fn seed_primitive_id_counter(max_existing: u64) {
    let target = max_existing.saturating_add(1);
    let mut current = NEXT_PRIMITIVE_ID.load(Ordering::Relaxed);
    while current < target {
        match NEXT_PRIMITIVE_ID.compare_exchange_weak(
            current,
            target,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => break,
            Err(actual) => current = actual,
        }
    }
}

/// Re-assign a fresh globally-unique ID to a primitive.
///
/// Use after cloning a primitive into a new owning store (e.g. seeding
/// `group.primitives` from stash) so that the clone and the original have
/// distinct IDs across all stores.
pub fn reassign_primitive_id(prim: &mut Box<dyn primitives_v2::Primitive>) {
    prim.data_mut().id = alloc_primitive_id();
}

mod manager;
mod signal_manager;
mod trades;

// Trait-based primitive system
pub mod primitives_v2;

// Re-export manager types
pub use manager::{DrawingManager, DrawingState, DragType, PrimitiveListItem};

// Signal and trade managers
pub use signal_manager::SignalManager;
pub use trades::{Trade, TradeDirection, TradeManager};

// Re-export v2 primitive types
pub use primitives_v2::{
    // Core trait
    Primitive as PrimitiveTrait,
    PrimitiveData, PrimitiveKind, ClickBehavior,
    // Registry
    PrimitiveRegistry, PrimitiveFactory, PrimitiveMetadata,
    // Hit testing
    HitTestResult,
    // Control points
    ControlPoint, ControlPointType, ControlPointCursor,
    // Styling
    PrimitiveColor, LineStyle, PrimitiveText, TextAlign, ExtendMode,
    // Sync mode
    SyncMode,
    // Geometry helpers
    point_to_line_distance, HIT_TOLERANCE,
    CONTROL_POINT_RADIUS, CONTROL_POINT_HIT_RADIUS,
    CONTROL_POINT_STROKE, CONTROL_POINT_FILL,
    // Text rotation helper
    normalize_text_rotation,
};

// Re-export concrete primitive types for direct construction if needed
pub use primitives_v2::lines::{
    TrendLine, HorizontalLine, VerticalLine, Ray, ExtendedLine,
    InfoLine, TrendAngle, HorizontalRay, CrossLine,
};
pub use primitives_v2::shapes::{
    Rectangle, Circle, Ellipse, Triangle, Arc, Polyline, Path,
    RotatedRectangle, Curve, DoubleCurve,
};
pub use primitives_v2::fibonacci::{
    FibRetracement, FibTrendExtension, FibChannel, FibTimeZones,
    FibSpeedResistance, FibTrendTime, FibCircles, FibSpiral, FibArcs,
    FibWedge, FibFan,
};
pub use primitives_v2::channels::{
    ParallelChannel, RegressionTrend, FlatTopBottom, DisjointChannel,
};
pub use primitives_v2::gann::{
    GannBox, GannSquareFixed, GannSquare, GannFan,
};
pub use primitives_v2::pitchforks::{
    Pitchfork, SchiffPitchfork, ModifiedSchiff, InsidePitchfork,
};
pub use primitives_v2::arrows::{
    ArrowLine,
};
pub use primitives_v2::annotations::{
    Text, AnchoredText, Note, PriceNote, Signpost, Callout,
    Comment, PriceLabel, Sign, Flag, Table, TriangleUp, TriangleDown,
};
pub use primitives_v2::patterns::{
    XabcdPattern, CypherPattern, HeadShoulders, AbcdPattern,
    TrianglePattern, ThreeDrives,
};
pub use primitives_v2::elliott::{
    ElliottImpulse, ElliottCorrection, ElliottTriangle,
    ElliottDoubleCombo, ElliottTripleCombo,
};
pub use primitives_v2::cycles::{
    CycleLines, TimeCycles, SineWave,
};
pub use primitives_v2::projection::{
    LongPosition, ShortPosition, BarsPattern,
    PriceProjection, Projection,
};
pub use primitives_v2::volume::{
    FixedVolumeProfile, AnchoredVolumeProfile,
};
pub use primitives_v2::measurement::{
    PriceRange, DateRange, PriceDateRange,
};
pub use primitives_v2::brushes::{
    Brush, Highlighter,
};
pub use primitives_v2::icons::{
    Image, Emoji, EmojiType,
};
// Re-export render types from unified RenderContext (engine::render)
pub use crate::render::{
    RenderContext, RenderOp, RenderOps, TextBaseline,
    crisp as render_crisp, crisp_rect as render_crisp_rect, execute_ops,
    render_primitive_text, render_primitive_text_rotated, render_text_with_background,
};
pub use primitives_v2::draw_control_points;
pub use primitives_v2::TextAnchor;

// Configuration system
pub use primitives_v2::config::{
    Configurable, ConfigProperty, PropertyType, PropertyValue, PropertyCategory,
    ContextMenuAction, PrimitiveFullConfig, FibLevelConfig, TimeframeVisibilityConfig,
    SelectOption, SettingsTemplate, TemplateStyle,
};

// System signals (strategy-generated markers) - types and manager
pub use primitives_v2::signals::{
    SystemSignal, SignalType, SignalPrimitive, StrategySignalConfig,
};

/// Get point labels for multi-point drawing primitives.
///
/// Returns appropriate labels based on the primitive type:
/// - XABCD patterns: `["X", "A", "B", "C", "D"]`
/// - ABCD patterns: `["A", "B", "C", "D"]`
/// - Head and shoulders: named shoulder/head labels
/// - Three drives: `["1", "2", "3", "4", "5", "6"]`
/// - Elliott waves: numeric `["1", "2", "3", ...]`
/// - Default: `["Точка", ...]`
pub fn get_point_labels(primitive_type: &str, count: usize) -> Vec<String> {
    match primitive_type {
        "xabcd_pattern" | "cypher_pattern" => {
            vec!["X", "A", "B", "C", "D"]
                .into_iter()
                .take(count)
                .map(String::from)
                .collect()
        }
        "abcd_pattern" => {
            vec!["A", "B", "C", "D"]
                .into_iter()
                .take(count)
                .map(String::from)
                .collect()
        }
        "head_shoulders" => {
            vec!["L плечо", "Голова", "R плечо", "Низ 1", "Низ 2"]
                .into_iter()
                .take(count)
                .map(String::from)
                .collect()
        }
        "three_drives" => {
            vec!["1", "2", "3", "4", "5", "6"]
                .into_iter()
                .take(count)
                .map(String::from)
                .collect()
        }
        "triangle_pattern" => {
            vec!["A", "B", "C"]
                .into_iter()
                .take(count)
                .map(String::from)
                .collect()
        }
        s if s.starts_with("elliott") => {
            (1..=count).map(|i| i.to_string()).collect()
        }
        _ => {
            (1..=count).map(|_| "Точка".to_string()).collect()
        }
    }
}
