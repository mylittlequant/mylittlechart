//! Core primitive trait - the foundation for all drawing primitives
//!
//! This trait-based architecture allows adding new primitives without
//! modifying the DrawingManager.

use serde::{Deserialize, Serialize};
use crate::{Bar, PriceScale, Viewport};
use super::types::{
    ControlPoint, ControlPointType, LineStyle, PrimitiveColor, PrimitiveText,
    Segment, control_point_style, hit_test_strokes, HIT_TOLERANCE,
};
use crate::render::{RenderContext, crisp};
use super::config::{TimeframeVisibilityConfig, WaveDegree};
use super::elliott::count::{WaveLink, WaveSlot};
use super::elliott::guard::Violation;

/// Category of primitive for toolbar organization
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PrimitiveKind {
    /// Lines: trend line, horizontal, vertical, ray, extended
    Line,
    /// Channels: parallel channel, regression trend, flat top/bottom
    Channel,
    /// Shapes: rectangle, ellipse, triangle, arc, polyline
    Shape,
    /// Fibonacci: retracement, extension, channel, circles, spiral
    Fibonacci,
    /// Gann: fan, square, box
    Gann,
    /// Patterns: head & shoulders, elliott wave, harmonic
    Pattern,
    /// Annotations: text, note, callout, user notes
    Annotation,
    /// Measurement: price range, date range, bars pattern
    Measurement,
    /// Trading: position, long/short, risk/reward
    Trading,
    /// Signal markers: buy/sell arrows, strategy indicators
    /// Programmatically placed by strategies, minimal configuration (color, size only)
    Signal,
}

/// The shared TAB SHAPE a primitive's settings-modal Subject tab uses — one
/// of the 12 archetypes the approved design settled on
/// (`mlc-ui-lab::concept::spec`, signed off 2026-08-11 — see
/// `docs/mlc/plans/archetype-modals-into-the-app-2026-08-11.md`), plus
/// `Generic` for a primitive the design does not (yet) name an archetype
/// for. An archetype names a shared SHAPE, never a replacement for a
/// type's own content — that plan's own "rule that outranks every wave":
/// Gann keeps its own angle labels, Elliott its own degrees, a position
/// its own computed R:R, all under the same archetype word.
///
/// Drives [`Primitive::subject_tab_label`]'s default and (indirectly, via
/// [`Primitive::archetype`]'s own default) which primitives merge their
/// former Levels/Appearance tabs into a ladder-shaped Subject tab versus a
/// plain one — `mlc-app-layout`'s `primitive_settings` module is the one
/// place that reads `archetype() == Archetype::Ladder` to pick the render
/// path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Archetype {
    /// A level ladder: every Fibonacci tool, every Gann tool, every
    /// pitchfork. Equivalent to `level_configs().is_some()` — see
    /// [`Primitive::archetype`]'s default.
    Ladder,
    /// The Elliott wave family.
    Wave,
    /// Harmonic/geometric chart patterns (XABCD, ABCD, Cypher, Head &
    /// Shoulders, Triangle, Three Drives).
    Pattern,
    /// A trade position (long/short) and the other `Trading`-kind
    /// projection tools.
    Position,
    /// A measuring tool — price/date range, cycles, volume profile/VWAP.
    Measurement,
    /// A text annotation whose own content IS the subject (Text, Note,
    /// Callout, Comment, Price Label/Note, Signpost, Anchored Text).
    Text,
    /// The Table primitive's own editable cell grid.
    Table,
    /// A plain geometric shape (rectangle, ellipse, triangle, arc,
    /// polyline, curve, path, ...).
    Shape,
    /// A channel (parallel channel, disjoint channel, flat top/bottom) —
    /// distinct from a pitchfork, which is `Ladder`-shaped despite sharing
    /// `PrimitiveKind::Channel`.
    Channel,
    /// The regression trend channel's own sigma-band tool.
    Regression,
    /// Freehand brush/highlighter strokes.
    Brush,
    /// A user-picked icon/glyph/image annotation (flag, emoji, image).
    Icon,
    /// A bare line: trend line, ray, extended line, horizontal/vertical
    /// line and ray, cross line, info line, trend angle, and the arrow
    /// (a line with heads). Its subject IS the line — owner sign-off
    /// 2026-08-11, which also retired the catch-all "Вид" tab: a tab named
    /// after a property rather than after its subject was the one thing
    /// left over from the pre-archetype modal.
    Line,
}

/// How many clicks required to create this primitive
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClickBehavior {
    /// Single click creates primitive (e.g., horizontal line at price)
    SingleClick,
    /// Two clicks define start and end (e.g., trend line)
    TwoPoint,
    /// Three points needed (e.g., triangle, channel)
    ThreePoint,
    /// Four points needed (e.g., disjoint channel)
    FourPoint,
    /// Open-ended: keeps accepting clicks past the u8 until the user
    /// double-clicks (`finish_multipoint`) or cancels (e.g., polyline,
    /// path). The u8 is a FLOOR — the minimum number of points needed
    /// before a double-click is allowed to finish the shape — never a
    /// terminator. There is no maximum.
    MultiPoint(u8),
    /// Fixed-arity multi-point: finalizes the instant it reaches exactly
    /// u8 points, same as `ThreePoint`/`FourPoint` but for counts >4 (e.g.,
    /// harmonic patterns, Elliott waves). The u8 IS the exact point count,
    /// never a floor — there is no double-click step.
    MultiPointFixed(u8),
    /// Click and drag (e.g., rectangle while holding mouse)
    ClickDrag,
    /// Freehand drag - continuous point collection during drag (e.g., brush, highlighter)
    /// Unlike ClickDrag, this collects many points and ignores clicks
    FreehandDrag,
}

/// Result of hit testing a primitive
#[derive(Clone, Debug)]
pub enum HitTestResult {
    /// No hit
    Miss,
    /// Hit the primitive body (for move)
    Body,
    /// Hit a control point (for resize/reshape)
    ControlPoint(ControlPointType),
}

/// Information about a control point for UI feedback
#[derive(Clone, Debug)]
pub struct ControlPointInfo {
    pub point_type: ControlPointType,
    pub label: &'static str,
    pub can_constrain: bool, // Hold Shift for constraint
}

/// Sync mode for primitives across charts
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SyncMode {
    /// Don't sync to other charts
    #[default]
    None,
    /// Sync to all charts of the same symbol
    SameSymbol,
    /// Sync everywhere
    Everywhere,
}

/// What a primitive currently claims to be built from — the reference its
/// own "Save as…" visibility is diffed against (see
/// [`crate::templates::PrimitiveTemplate::matches_primitive`]). Persisted on
/// [`PrimitiveData`] so it survives a reload exactly like the settings
/// themselves do — unlike a session-only baseline snapshot, which a reload
/// re-captures from the (possibly already-drifted) loaded state and so can
/// never detect drift that happened before the reload.
///
/// Deliberately holds an ID, never a value snapshot: a snapshot would be a
/// second copy of the settings that can itself go stale (e.g. if the
/// template it was copied from is later edited), where an ID always
/// resolves against the live source of truth — the template store or the
/// system preset's own fixed definition — at the moment it is read.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StyleReference {
    /// A user-saved [`crate::templates::PrimitiveTemplate::id`].
    Template(String),
    /// A built-in system template id — names one row of
    /// [`crate::templates::PrimitiveTemplate::system_templates`] for this
    /// primitive's type: one of `"standard"`/`"extended"`/`"key_only"`/
    /// `"all"` for a type whose [`Primitive::level_set_presets`] is
    /// non-empty (each family names its OWN subset of these generic ids —
    /// e.g. `fibonacci::retracement::LEVEL_SET_PRESETS` ships all four,
    /// `gann::gann_fan::LEVEL_SET_PRESETS` ships three, no family invents a
    /// fifth), or just `"standard"` for a primitive with no level ladder at
    /// all (`Primitive::level_configs` returns `None`) — the only case left
    /// where `level_set_presets()` is still the trait's empty default.
    /// Never `"custom"`, which is a derived display state with no fixed
    /// definition of its own, not a real system row.
    ///
    /// Serialized as `"LevelSet"` (`#[serde(rename)]` below) — the wire
    /// name is historical, from when this variant could only ever name one
    /// of the four Fibonacci level-set presets. Every drawing already on
    /// disk keeps resolving under the old name; the field this lives on
    /// (`PrimitiveData::style_reference`) is `#[serde(default)]`, so a file
    /// that predates this variant entirely also still loads.
    #[serde(rename = "LevelSet")]
    System(String),
}

/// Core primitive data that all primitives share
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PrimitiveData {
    /// Unique identifier
    pub id: u64,
    /// Primitive type ID (e.g., "trend_line", "fib_retracement")
    pub type_id: String,
    /// Display name for UI
    pub display_name: String,
    /// Color configuration
    pub color: PrimitiveColor,
    /// Line width in pixels
    pub width: f64,
    /// Line style
    pub style: LineStyle,
    /// Optional text label
    pub text: Option<PrimitiveText>,
    /// Is primitive locked (can't be edited)
    pub locked: bool,
    /// Is primitive visible
    pub visible: bool,
    /// Z-order layer
    pub z_order: i32,
    /// Timeframe visibility settings
    #[serde(default)]
    pub timeframe_visibility: Option<TimeframeVisibilityConfig>,
    /// Sync mode across charts
    #[serde(default)]
    pub sync_mode: SyncMode,
    /// Pane ID where primitive was created (None = main chart, Some(id) = sub-pane indicator instance id)
    #[serde(default)]
    pub pane_id: Option<u64>,
    /// Window ID where primitive was created (for multi-window support)
    #[serde(default)]
    pub window_id: Option<u64>,
    /// Legacy field — kept as a read-only serde stub so old JSON that still has
    /// `point_timestamps` doesn't fail to deserialize.  Not written on save.
    /// Coordinates are now stored directly as `(ts_ms, price)` in each primitive.
    #[serde(default, skip_serializing)]
    pub point_timestamps: Vec<i64>,
    /// Origin primitive ID — if Some, this is a synced clone from another window.
    /// When sync is disabled, clones (origin_id.is_some()) are purged.
    /// None means this is an original primitive drawn on this window.
    #[serde(default)]
    pub origin_id: Option<u64>,
    /// The symbol this primitive was created for.
    #[serde(default)]
    pub symbol: String,
    /// Exchange this primitive was created for (e.g. `"binance"`).
    #[serde(default)]
    pub exchange: String,
    /// Account type label this primitive was created for (e.g. `"S"`, `"F"`).
    #[serde(default)]
    pub account_type: String,
    /// What this primitive currently claims to be built from — see
    /// [`StyleReference`]. `None` means nothing NAMEABLE has been
    /// established: either no template/preset was ever applied and the
    /// primitive's seeded starting composition matches none of the system
    /// level-set presets either (a "custom" starting point, or a
    /// primitive with no level ladder at all — see
    /// `mlc-backend`'s `DrawingManager::initial_style_reference`, which
    /// stamps this on creation), or a reference that was established has
    /// since become unresolvable. "Save as…" reads `None` as SHOWN, not
    /// hidden — a state that matches nothing on record is exactly a state
    /// worth offering to save (2026-08-09; see
    /// `mlc-app-layout`'s `primitive_settings::show_save_as` for the full
    /// read).
    #[serde(default)]
    pub style_reference: Option<StyleReference>,
}

impl Default for PrimitiveData {
    fn default() -> Self {
        Self {
            id: 0,
            type_id: String::new(),
            display_name: String::new(),
            color: PrimitiveColor::default(),
            width: 2.0,
            style: LineStyle::Solid,
            text: None,
            locked: false,
            visible: true,
            z_order: 0,
            timeframe_visibility: None,
            sync_mode: SyncMode::None,
            pane_id: None,
            window_id: None,
            point_timestamps: Vec::new(),
            origin_id: None,
            symbol: String::new(),
            exchange: String::new(),
            account_type: String::new(),
            style_reference: None,
        }
    }
}

/// The core primitive trait
///
/// All drawing primitives must implement this trait. The DrawingManager
/// works with `Box<dyn Primitive>` to support any primitive type.
pub trait Primitive: Send + Sync {
    // =========================================================================
    // Identity & Metadata
    // =========================================================================

    /// Get the primitive type ID (e.g., "trend_line", "fib_retracement")
    fn type_id(&self) -> &'static str;

    /// Get display name for UI (can be localized)
    fn display_name(&self) -> &str;

    /// Get the category for toolbar organization
    fn kind(&self) -> PrimitiveKind;

    /// The archetype this primitive's settings-modal Subject tab belongs
    /// to — see [`Archetype`]'s own doc.
    ///
    /// Default: a level ladder (`level_configs().is_some()`) is always
    /// `Archetype::Ladder` regardless of `kind()` — the pitchfork family
    /// declares `PrimitiveKind::Channel` via `kind()` but IS ladder-shaped,
    /// exactly like `available_tabs`'s own pre-existing Levels-tab gate
    /// (`mlc-app-layout`) already treats it. Every other primitive falls
    /// back to its `kind()`'s own honest majority mapping — `Line`/`Signal`
    /// have no obvious archetype among the 12 named ones and default to
    /// `Generic`. Overridden per type where `kind()` alone cannot tell two
    /// different subjects apart (e.g. Elliott wave types declare
    /// `PrimitiveKind::Pattern`, shared with harmonic patterns, but are
    /// `Wave`-shaped, not `Pattern`-shaped).
    fn archetype(&self) -> Archetype {
        if self.level_configs().is_some() {
            return Archetype::Ladder;
        }
        match self.kind() {
            PrimitiveKind::Channel => Archetype::Channel,
            PrimitiveKind::Shape => Archetype::Shape,
            PrimitiveKind::Fibonacci | PrimitiveKind::Gann => Archetype::Ladder,
            PrimitiveKind::Pattern => Archetype::Pattern,
            PrimitiveKind::Annotation => Archetype::Text,
            PrimitiveKind::Measurement => Archetype::Measurement,
            PrimitiveKind::Trading => Archetype::Position,
            PrimitiveKind::Line => Archetype::Line,
            // A signal marker (triangle up/down, sign) is a glyph dropped on
            // the chart with a fill and a stroke — the same subject a flag or
            // an emoji has, so it reads "Значок", not a fourteenth archetype
            // of its own.
            PrimitiveKind::Signal => Archetype::Icon,
        }
    }

    /// The Subject tab's own localized label — the FIRST tab in the
    /// settings-modal strip once `Levels`/`Appearance` collapse into one
    /// (`mlc-app-layout`'s `primitive_settings` module, W1 of the
    /// archetype-modals rollout,
    /// `docs/mlc/plans/archetype-modals-into-the-app-2026-08-11.md`).
    ///
    /// Default: [`Archetype`]'s own canonical word. Overridden per type
    /// where the SAME archetype still needs a different subject word —
    /// every Gann tool and the two ring/circle Fibonacci tools are
    /// `Ladder`-archetype (they have a level ladder) but are not called
    /// "Levels" in the approved design.
    fn subject_tab_label(&self) -> &'static str {
        use crate::i18n::{t_concept, t_config, t_settings, ConceptKey, ConfigKey, SettingsKey};
        match self.archetype() {
            Archetype::Ladder => t_config(ConfigKey::Levels),
            Archetype::Wave => t_concept(ConceptKey::WavesTabLabel),
            Archetype::Pattern => t_concept(ConceptKey::RatiosTabLabel),
            Archetype::Position => t_concept(ConceptKey::TradeTabLabel),
            Archetype::Measurement => t_concept(ConceptKey::WhatToShowTabLabel),
            Archetype::Text => t_settings(SettingsKey::Text),
            Archetype::Table => t_config(ConfigKey::TabCells),
            Archetype::Shape => t_concept(ConceptKey::ShapeSubject),
            Archetype::Channel => t_concept(ConceptKey::ChannelSubject),
            Archetype::Regression => t_concept(ConceptKey::DeviationsTabLabel),
            Archetype::Brush => t_concept(ConceptKey::StrokeGroupTitle),
            Archetype::Icon => t_concept(ConceptKey::IconTabLabel),
            Archetype::Line => t_concept(ConceptKey::LineSubject),
        }
    }

    /// How many clicks to create this primitive
    fn click_behavior(&self) -> ClickBehavior;

    // =========================================================================
    // Common Data Access
    // =========================================================================

    /// Get shared primitive data
    fn data(&self) -> &PrimitiveData;

    /// Get mutable shared primitive data
    fn data_mut(&mut self) -> &mut PrimitiveData;

    // =========================================================================
    // Geometry
    // =========================================================================

    /// Get all coordinate points as (timestamp_ms, price) pairs.
    ///
    /// `timestamp_ms` is Unix time in **milliseconds**.
    fn points(&self) -> Vec<(i64, f64)>;

    /// Set coordinate points.  `points[n].0` is Unix timestamp in **milliseconds**.
    fn set_points(&mut self, points: &[(i64, f64)]);

    /// Per-instance point labels for the Placement tab's points table, when
    /// they depend on primitive STATE rather than just the type id (e.g. an
    /// Elliott wave's notation depends on its own `degree` field — the
    /// static `drawing::get_point_labels(type_id, count)` catalogue used for
    /// every other named-point type (harmonics, head & shoulders, ...) can
    /// only key off `type_id`, so it cannot express that). `None` means
    /// "consult the static catalogue instead" — the default for every
    /// primitive whose labels are constant for its type id. Length need not
    /// match `points().len()`; the caller takes/pads as needed.
    fn point_labels(&self) -> Option<Vec<String>> {
        None
    }

    /// Translate the primitive by a timestamp delta (ms) and a price delta.
    fn translate(&mut self, ts_delta_ms: i64, price_delta: f64);

    /// Move a specific control point to (timestamp_ms, price).
    fn move_control_point(&mut self, point_type: ControlPointType, ts_ms: i64, price: f64);

    /// Move a specific control point, given BOTH the screen-space cursor
    /// position and the already-resolved data coordinates for that same
    /// position.
    ///
    /// `ts_ms`/`price` are exactly what the drag caller (`DrawingManager::
    /// update_drag_screen`) would also hand to `move_control_point` — magnet
    /// snap and sub-pane price-range offsets are resolved upstream, before
    /// this call. The default implementation delegates straight to
    /// `move_control_point` with them, so every primitive that does not
    /// override this method behaves identically to calling
    /// `move_control_point` directly; `screen_x`/`screen_y` (plus `bars`/
    /// `viewport`/`price_scale`) are unused by the default and exist only
    /// for primitives that must reshape in PIXEL space to keep the dragged
    /// handle under the cursor — a fixed-rotation corner resize is one such
    /// case: ms-deltas and price-deltas aren't comparable, so the geometry
    /// only exists once both are in pixels. See `RotatedRectangle`.
    fn move_control_point_screen(
        &mut self,
        point_type: ControlPointType,
        _screen_x: f64,
        _screen_y: f64,
        ts_ms: i64,
        price: f64,
        _bars: &[Bar],
        _viewport: &Viewport,
        _price_scale: &PriceScale,
    ) {
        self.move_control_point(point_type, ts_ms, price);
    }

    // =========================================================================
    // Hit Testing
    // =========================================================================

    /// Hit test at screen coordinates.
    ///
    /// `bars` is required to convert the primitive's stored timestamps to screen X.
    ///
    /// Default implementation (2026-08-07 hit-test seam): control points
    /// first, via [`control_point_hit_test`](Self::control_point_hit_test)
    /// — itself built directly on [`control_points`](Self::control_points),
    /// so a handle is always grabbable exactly where it is drawn — then the
    /// primitive's own [`geometry`](Self::geometry) segments for the body.
    /// Interior fill is never a grab surface. Every primitive registered
    /// today still provides its own `hit_test` override (this default is an
    /// additive seam for primitives that opt in by implementing `geometry`,
    /// not a cutover); a primitive that relies on this default without also
    /// implementing `geometry` is selectable by its handles but not by its
    /// body, because `geometry`'s own default is empty.
    fn hit_test(
        &self,
        screen_x: f64,
        screen_y: f64,
        bars: &[Bar],
        viewport: &Viewport,
        price_scale: &PriceScale,
    ) -> HitTestResult {
        if let Some(point_type) =
            self.control_point_hit_test(screen_x, screen_y, bars, viewport, price_scale)
        {
            return HitTestResult::ControlPoint(point_type);
        }
        let segments = self.geometry(bars, viewport, price_scale);
        if !segments.is_empty() && hit_test_strokes(screen_x, screen_y, &segments, HIT_TOLERANCE) {
            return HitTestResult::Body;
        }
        HitTestResult::Miss
    }

    /// Get control points in screen coordinates.
    ///
    /// `bars` is required to convert stored timestamps to screen X.
    fn control_points(
        &self,
        bars: &[Bar],
        viewport: &Viewport,
        price_scale: &PriceScale,
    ) -> Vec<ControlPoint>;

    /// Default control-point hit test, built directly on
    /// [`control_points`](Self::control_points) — the single list both the
    /// draw side (`draw_control_points`) and the hit-test side now walk, so
    /// a handle drawn at a coordinate is always the same handle grabbable
    /// at that coordinate. Radius comes from the active
    /// [`control_point_style`], the same style `draw_control_points` reads,
    /// so a configurable size can never drift from its own hitbox.
    ///
    /// Provided as its own trait method (not inlined into `hit_test`'s
    /// default) so a primitive that wants control-point hit-testing without
    /// the body fallback — or a caller that wants ONLY control points,
    /// bypassing body geometry entirely — can call it directly.
    fn control_point_hit_test(
        &self,
        screen_x: f64,
        screen_y: f64,
        bars: &[Bar],
        viewport: &Viewport,
        price_scale: &PriceScale,
    ) -> Option<ControlPointType> {
        let hit_r_sq = control_point_style().hit_radius().powi(2);
        self.control_points(bars, viewport, price_scale)
            .into_iter()
            .find(|cp| {
                let dx = cp.x - screen_x;
                let dy = cp.y - screen_y;
                dx * dx + dy * dy <= hit_r_sq
            })
            .map(|cp| cp.point_type)
    }

    /// The strokes this primitive draws, in screen space — the shared
    /// source both a migrated `hit_test`'s body check AND (a future pass
    /// could wire it) `render` would read, so "drawn but not
    /// hit-testable" and "hit-testable but not drawn" both stop being
    /// possible for whatever this method reports.
    ///
    /// Empty by default, matching every existing primitive today (each
    /// still hand-rolls its own `hit_test` instead of overriding this).
    /// Signature deliberately mirrors `hit_test`/`control_points`
    /// — `(bars, viewport, price_scale)`, NOT `&mut dyn RenderContext` —
    /// so it is the one number-producing path `hit_test`'s default and
    /// (once a family migrates) `control_points`-adjacent code can share.
    /// `render`'s OWN coordinate path still goes through `RenderContext`
    /// (`ctx.bar_to_x`/`ctx.price_to_y`), which does not expose the
    /// underlying `Viewport`/`PriceScale` it was seeded with — unifying
    /// that side too needs `RenderContext` to grow a `viewport()`/
    /// `price_scale()` accessor (or `render` itself to take
    /// `(bars, viewport, price_scale)` instead of `ctx`), a wider refactor
    /// than this pass. Migrating a family to `geometry` is explicitly out
    /// of scope for this seam — see `registry.rs`'s
    /// `every_registered_type_control_points_and_geometry_are_hit_testable`.
    fn geometry(&self, _bars: &[Bar], _viewport: &Viewport, _price_scale: &PriceScale) -> Vec<Segment> {
        Vec::new()
    }

    // =========================================================================
    // Rendering
    // =========================================================================

    /// Render the primitive using the provided render context
    ///
    /// The primitive should use ctx.bar_to_x() and ctx.price_to_y() to convert
    /// its data coordinates to screen coordinates, then draw using the
    /// path operations (begin_path, move_to, line_to, etc.) and fill/stroke.
    ///
    /// Default implementation draws lines connecting all points - override for
    /// custom rendering (shapes, fills, text, etc.)
    fn render(&self, ctx: &mut dyn RenderContext, _is_selected: bool) {
        // Inline default implementation to avoid Sized bound issues
        let data = self.data();
        let points = self.points();
        let kind = self.kind();
        let dpr = ctx.dpr();

        if points.is_empty() {
            return;
        }

        // Convert to screen coordinates (timestamps → X via ts_to_x_ms)
        let screen_points: Vec<(f64, f64)> = points
            .iter()
            .map(|(ts_ms, price)| (ctx.ts_to_x_ms(*ts_ms), ctx.price_to_y(*price)))
            .collect();

        // Set stroke style
        ctx.set_stroke_color(&data.color.stroke);
        ctx.set_stroke_width(data.width);

        // Set line dash based on style
        match data.style {
            LineStyle::Solid => ctx.set_line_dash(&[]),
            LineStyle::Dashed => ctx.set_line_dash(&[8.0, 4.0]),
            LineStyle::Dotted => ctx.set_line_dash(&[2.0, 2.0]),
            LineStyle::LargeDashed => ctx.set_line_dash(&[12.0, 6.0]),
            LineStyle::SparseDotted => ctx.set_line_dash(&[2.0, 8.0]),
        }

        // Render based on kind
        match kind {
            PrimitiveKind::Line | PrimitiveKind::Channel => {
                if screen_points.len() >= 2 {
                    ctx.begin_path();
                    let (x0, y0) = screen_points[0];
                    ctx.move_to(crisp(x0, dpr), crisp(y0, dpr));
                    for (x, y) in screen_points.iter().skip(1) {
                        ctx.line_to(crisp(*x, dpr), crisp(*y, dpr));
                    }
                    ctx.stroke();
                }
            }
            PrimitiveKind::Shape => {
                if screen_points.len() >= 2 {
                    let (x1, y1) = screen_points[0];
                    let (x2, y2) = screen_points[1];
                    let rx = x1.min(x2);
                    let ry = y1.min(y2);
                    let rw = (x2 - x1).abs();
                    let rh = (y2 - y1).abs();

                    if let Some(ref fill) = data.color.fill {
                        ctx.set_fill_color(fill);
                        ctx.fill_rect(rx, ry, rw, rh);
                    }
                    ctx.stroke_rect(rx, ry, rw, rh);
                }
            }
            PrimitiveKind::Annotation => {
                if let Some((x, y)) = screen_points.first() {
                    ctx.set_fill_color(&data.color.stroke);
                    ctx.begin_path();
                    ctx.arc(*x, *y, 6.0, 0.0, std::f64::consts::TAU);
                    ctx.fill();
                }
            }
            _ => {
                // Default: draw lines connecting all points
                if screen_points.len() >= 2 {
                    ctx.begin_path();
                    let (x0, y0) = screen_points[0];
                    ctx.move_to(crisp(x0, dpr), crisp(y0, dpr));
                    for (x, y) in screen_points.iter().skip(1) {
                        ctx.line_to(crisp(*x, dpr), crisp(*y, dpr));
                    }
                    ctx.stroke();
                }

                ctx.set_fill_color(&data.color.stroke);
                for (x, y) in &screen_points {
                    ctx.begin_path();
                    ctx.arc(*x, *y, 3.0, 0.0, std::f64::consts::TAU);
                    ctx.fill();
                }
            }
        }

        ctx.set_line_dash(&[]);
    }


    // =========================================================================
    // Style Properties (for primitives with custom settings)
    // =========================================================================

    /// Get additional style properties specific to this primitive type
    /// (e.g., show_labels, show_lines, wave degree for Elliott patterns)
    /// These are added to base properties in the settings modal
    fn style_properties(&self) -> Vec<super::config::ConfigProperty> {
        Vec::new()
    }

    /// Apply a style property value specific to this primitive type
    /// Returns true if property was handled
    fn apply_style_property(&mut self, _id: &str, _value: &super::config::PropertyValue) -> bool {
        false
    }

    /// The primitive's own single band-fill on/off + opacity, if it has
    /// one — the value the Levels tab's global fill row edits
    /// (`primitive_settings_levels::draw_global_opacity_row`, `mlc-app-layout`).
    /// `None` means "this type has no primitive-wide fill master" (e.g.
    /// Pitchfork), which the caller reads as "draw nothing for that row" —
    /// not "false", which would render a checkbox that does nothing.
    ///
    /// Default reads the same `"fill"`/`"fill_opacity"` ids every
    /// `style_properties()` override already exposes for `style_prop:*`
    /// dispatch — this is the pre-existing behaviour for every primitive
    /// that declares those two ids (e.g. `FibWedge`). Override only where,
    /// like `FibRetracement`, the master field is no longer part of the
    /// declared list — its own doc explains why the declaration was
    /// removed (it duplicated this exact control).
    fn fill_master(&self) -> Option<(bool, f64)> {
        let props = self.style_properties();
        let on = props.iter().find(|p| p.id == "fill").and_then(|p| p.value.as_bool())?;
        let opacity = props
            .iter()
            .find(|p| p.id == "fill_opacity")
            .and_then(|p| p.value.as_number())
            .map(|pct| (pct / 100.0).clamp(0.0, 1.0))
            .unwrap_or(0.0);
        Some((on, opacity))
    }

    // =========================================================================
    // Level Properties (for level mode/preset selection - displayed on Levels tab)
    // =========================================================================

    /// Get level properties for the Levels tab in settings modal
    /// (e.g., level_mode: Base/Fibonacci/Both for Pitchfork, preset selection)
    /// These are separate from style_properties and are shown on the Levels tab
    fn level_properties(&self) -> Vec<super::config::ConfigProperty> {
        Vec::new()
    }

    /// Apply a level property value
    /// Returns true if property was handled
    fn apply_level_property(&mut self, _id: &str, _value: &super::config::PropertyValue) -> bool {
        false
    }

    // =========================================================================
    // Text Properties (for text-centric primitives like Text, Note, Callout)
    // =========================================================================

    /// Get text properties for the Text tab in settings modal
    /// For text-centric primitives (Text, Note, Callout, etc.) this defines
    /// which text fields and settings are available.
    /// Returns None to use default text editing (content + style + position grid)
    /// Returns Some(vec) to use custom text properties (hides position grid)
    fn text_properties(&self) -> Option<Vec<super::config::ConfigProperty>> {
        None
    }

    /// Apply a text property value
    /// Returns true if property was handled
    fn apply_text_property(&mut self, _id: &str, _value: &super::config::PropertyValue) -> bool {
        false
    }

    // =========================================================================
    // Level Configuration (for Fibonacci, Gann, Pitchfork)
    // =========================================================================

    /// Get level configurations (for primitives with levels like Fibonacci, Gann, Pitchfork)
    /// Returns None for primitives that don't support level configuration
    fn level_configs(&self) -> Option<Vec<super::config::FibLevelConfig>> {
        None
    }

    /// Set level configurations
    /// Returns true if the primitive supports levels and they were set
    fn set_level_configs(&mut self, _configs: Vec<super::config::FibLevelConfig>) -> bool {
        false
    }

    /// The level-set presets this primitive TYPE offers — see
    /// [`super::config::LevelSetPreset`]'s own doc. Default: none, which is
    /// the honest answer for a primitive with no level ladder at all
    /// (`level_configs()` returns `None`) — and it is what makes
    /// [`crate::templates::PrimitiveTemplate::system_templates`] give such
    /// a type exactly ONE `"standard"` row, built from its own factory
    /// seed.
    ///
    /// Override wherever the type genuinely owns a fixed catalogue of its
    /// OWN — every ladder-having primitive in this crate does today: the
    /// Fibonacci family (`fibonacci::retracement::LEVEL_SET_PRESETS`,
    /// shared by every type that seeds its ladder from `retracement::
    /// default_level_configs`/`default_level_configs_filled`), the Gann
    /// family (`gann::gann_fan`/`gann_box`/`gann_square`, each with its own
    /// angle- or fraction-based catalogue — `gann_square_fixed` shares
    /// `gann_square`'s), the pitchfork family (`pitchforks::pitchfork::
    /// LEVEL_SET_PRESETS`, shared by `schiff`/`modified_schiff`/
    /// `inside_pitchfork`), and `fibonacci::time_zones::LEVEL_SET_PRESETS`
    /// (built from the Fibonacci SEQUENCE used as bar-count offsets, its
    /// own catalogue, not `retracement`'s price ratios). Do NOT invent a
    /// value list, and do NOT borrow another family's: build a preset
    /// exclusively from the OVERRIDING type's own declared catalogue — the
    /// defect this method exists to close was narrowing a type's ladder
    /// toward ratios it never carried.
    fn level_set_presets(&self) -> &'static [super::config::LevelSetPreset] {
        &[]
    }

    // =========================================================================
    // Table Cell Grid (for the Table annotation's own bespoke Cells tab)
    // =========================================================================

    /// The Table primitive's own cell grid — `rows[r][c]` is the raw stored
    /// text for that cell. `None` for every primitive without one (every
    /// type but `Table` today). A bespoke, non-`ConfigProperty` collection —
    /// the exact same shape [`level_configs`](Self::level_configs) is for
    /// the level ladder (see `PropertyCategory`'s own doc on why a bespoke
    /// collection is never a declared property): read by the settings
    /// modal's own Cells tab (`mlc-app-layout`'s
    /// `primitive_settings_cells`), never by `style_properties()`/
    /// `text_properties()`.
    ///
    /// Cell WRITES do not get a matching `set_table_rows` — `Table` already
    /// has a real mutation path for them (`apply_text_property`'s own
    /// `"cell_{row}_{col}"`/`"rows_count"`/`"cols_count"` ids, dispatched
    /// generically through `PropertyKind::Text` exactly like every other
    /// text-family property) — adding a second one would be the "second
    /// source of truth" the Cells tab is explicitly built to avoid.
    fn table_rows(&self) -> Option<Vec<Vec<String>>> {
        None
    }

    /// Format a level ladder row's raw stored value (`FibLevelConfig::level`,
    /// a plain `f64`) for display in the settings modal's own value field —
    /// the seam a level-having primitive uses to show what its number
    /// actually MEANS, without touching the shared ladder UI
    /// (`mlc-app-layout`'s `primitive_settings_levels`) or the row's own
    /// identity key (`primitive_settings_levels::vkey`, always the raw
    /// value, 3-decimal — an identity, never a display string, so it never
    /// routes through this).
    ///
    /// Default: the same 3-decimal formatting the ladder always used
    /// (`format!("{level:.3}")`) — every primitive that does not override
    /// this renders exactly as before. Override alongside
    /// [`parse_level_value`](Self::parse_level_value) for a primitive whose
    /// raw storage does not read naturally as a bare number — e.g. a Gann
    /// Fan angle (`2.0` stored, `"2x1"` shown, see
    /// `gann::gann_fan::gann_angle_label`) or a Gann Box/Square ring
    /// fraction (`0.5` stored, `"50%"` shown, matching that type's own
    /// on-chart label).
    fn level_value_label(&self, level: f64) -> String {
        format!("{level:.3}")
    }

    /// Parse a user-typed level ladder value field back into the raw stored
    /// value — the inverse of
    /// [`level_value_label`](Self::level_value_label). `None` rejects the
    /// edit (the field's own commit path leaves the row unchanged).
    ///
    /// Default: a plain float parse (`"1.618"` -> `Some(1.618)`). An
    /// override MUST still accept a bare number typed directly (a user
    /// retyping the field from scratch, ignoring the notation shown) in
    /// addition to whatever notation [`level_value_label`](Self::level_value_label)
    /// itself produces.
    fn parse_level_value(&self, text: &str) -> Option<f64> {
        text.trim().parse().ok()
    }

    // =========================================================================
    // Elliott Wave Markup Tree (E1 — read-only surface, see `elliott::count`)
    // =========================================================================

    /// This primitive's own link upward to the parent Elliott wave it is
    /// nested inside, if any — lets a host walk the markup tree without
    /// downcasting to each concrete Elliott type. `None` for every
    /// primitive outside the Elliott family, and for an Elliott wave that
    /// is not nested under another one.
    fn wave_link(&self) -> Option<WaveLink> {
        None
    }

    /// Set (or clear) [`wave_link`](Self::wave_link) without downcasting to
    /// one of the five concrete Elliott structs — the write path chain
    /// promotion needs from OUTSIDE the Elliott family itself
    /// (`elliott::count::promote_chain_shape` + `DrawingManager::
    /// promote_wave_chain`): turning a `follows`-linked same-degree chain
    /// into subdivisions of a newly built coarser parent stamps this on
    /// every chain member, and undoing that promotion clears it back to
    /// `None` again. No-op for every primitive outside the Elliott family,
    /// mirroring [`set_wave_follows`](Self::set_wave_follows)'s own default.
    fn set_wave_link(&mut self, _link: Option<WaveLink>) {}

    /// The [`WaveDegree`] this primitive currently carries, if it is an
    /// Elliott wave — its own stored `degree` field, exactly as today. A
    /// linked child's degree is set by the CALLER at construction time (via
    /// `elliott::count::child_seed`), never resolved here — a primitive
    /// cannot see its own parent. `None` for every primitive outside the
    /// Elliott family.
    fn wave_degree(&self) -> Option<WaveDegree> {
        None
    }

    /// Set [`wave_degree`](Self::wave_degree) without downcasting to one of
    /// the five concrete Elliott structs — the write path higher-degree
    /// auto-adoption needs from OUTSIDE the Elliott family itself
    /// (`elliott::count::adoptable_children` + the backend's own detector
    /// that runs it at creation time): a newly drawn structure that adopts
    /// existing finer-degree structures as its own subdivisions must carry a
    /// degree one step COARSER than what it adopted, and that degree is only
    /// known AFTER adoption resolves — unlike every other Elliott
    /// primitive's degree, which is fixed at construction
    /// ([`wave_degree`](Self::wave_degree)'s own doc). No-op for every
    /// primitive outside the Elliott family, mirroring
    /// [`set_wave_link`](Self::set_wave_link)'s own default.
    fn set_wave_degree(&mut self, _degree: WaveDegree) {}

    /// Which slots this primitive's own points can seed a child into — the
    /// five impulse waves, the three corrective waves, the five triangle
    /// waves, or a combination's own W/X/Y(/XX/Z) slots. Empty for every
    /// primitive outside the Elliott family.
    fn wave_slots(&self) -> &'static [WaveSlot] {
        &[]
    }

    /// The id of the structure THIS primitive continues — succession, not
    /// subdivision (see `elliott::count`'s own module doc for why the two
    /// are different relations, each with its own field: this one is
    /// `follows`, [`wave_link`](Self::wave_link) is `link`). `None` for
    /// every primitive outside the Elliott family, and for an Elliott wave
    /// that opens a count rather than continuing one.
    fn wave_follows(&self) -> Option<u64> {
        None
    }

    /// Clear (or set) [`wave_follows`](Self::wave_follows) without
    /// downcasting to one of the five concrete Elliott structs — the one
    /// write path this relation needs from OUTSIDE the Elliott family
    /// itself: `DrawingManager::delete`'s choke point must drop a dangling
    /// successor reference on every SURVIVING primitive when its
    /// predecessor is deleted (see that method's own doc for why deleting a
    /// predecessor does not cascade into its successor the way deleting a
    /// parent cascades into its children). No-op for every primitive
    /// outside the Elliott family, mirroring `wave_link`'s own `None`
    /// default.
    fn set_wave_follows(&mut self, _follows: Option<u64>) {}

    /// How many of [`points`](Self::points) are REAL right now, for a
    /// primitive that supports partial/in-progress construction — see each
    /// Elliott type's own `realized_points` field doc (`elliott::
    /// elliott_impulse`) for the full rationale. Read by `elliott::guard::
    /// violations` (and each Elliott type's own `render`, to know which
    /// prefix of `points()` is real rather than padding) so a rule needing a
    /// not-yet-drawn point is skipped, not reported.
    ///
    /// Default: every point is real (`points().len()`) — exactly how every
    /// non-Elliott primitive behaves, and how every Elliott primitive
    /// behaved before this concept existed. Overridden by the five Elliott
    /// types to return their own `realized_len()`.
    fn realized_point_count(&self) -> usize {
        self.points().len()
    }

    /// Declare how many of [`points`](Self::points) are real — the write
    /// side of [`realized_point_count`](Self::realized_point_count), and the
    /// ONLY way to LOWER it (each Elliott type's own `grow_realized` raises
    /// it and deliberately never lowers it).
    ///
    /// Exists for the in-progress preview: while a multi-point structure is
    /// being clicked out, `DrawingManager::create_preview` pads the point
    /// array up to the type's full pivot count by repeating the cursor, so
    /// without
    /// this the primitive claims every pivot is placed and draws the LAST
    /// wave's label on the point the user is still placing — the owner's
    /// report of 2026-08-14: a fresh impulse showed "(5)" while wave 1 was
    /// being drawn.
    ///
    /// No-op by default: a primitive with no partial-construction concept
    /// has nothing to declare.
    fn set_realized_point_count(&mut self, _n: usize) {}

    /// This primitive's own drawn Elliott CORRECTION subtype, as the same
    /// fixed token `ElliottCorrection::apply_style_property`'s own
    /// `"correction_type"` id already reads and writes (`"zigzag"` /
    /// `"flat"` / `"expanded"` / `"running"`) — the shape `elliott::guard`
    /// needs to pick which of `mylittleindicators::ewa::primitives`'s own
    /// catalogue rule set applies (`RegularFlat`/`ExpandedFlat`/
    /// `RunningFlat` each carry a different hard-rule list; `zigzag` is also
    /// this method's own default label when unresolved, matching
    /// `CorrectionType`'s own `#[default]`). `None` for every primitive
    /// that is not an `ElliottCorrection`, Elliott or not.
    fn correction_subtype(&self) -> Option<&'static str> {
        None
    }

    /// Whether THIS impulse has DECLARED its own fifth wave truncated — the
    /// analyst's own per-structure opt-in that a fifth failing to exceed
    /// wave 3 is the real Elliott shape (`EwaPatternKind::TruncatedImpulse`)
    /// rather than a drawing mistake (owner ruling 2026-08-15: see
    /// `elliott::guard::impulse_violations`'s own `"wave5_beyond_wave3"`
    /// check, and `ElliottImpulse::truncated_fifth`'s own field doc for the
    /// full "why per-structure, not engine-wide" rationale). `false` for
    /// every primitive that is not an `ElliottImpulse` — Elliott or not —
    /// and for an impulse drawn before this field existed
    /// (`#[serde(default)]`), so the rule stays ACTIVE unless the analyst
    /// has actually ticked the box.
    fn truncated_fifth(&self) -> bool {
        false
    }

    // =========================================================================
    // Elliott Guard Context Cache (E2 — render-side cache, see
    // `elliott::guard::violations_in`)
    // =========================================================================

    /// The extra hard-rule violations this primitive currently exhibits
    /// that depend on its own SIBLINGS — its subdivisions or its own
    /// predecessor (see `elliott::guard::violations_in`'s own doc for what
    /// those checks are). A primitive cannot see its siblings on its own
    /// (`elliott::count`'s own module doc), so this is not COMPUTED here —
    /// it is a CACHE the collection's own owner fills by calling
    /// `elliott::guard::violations_in(self, others)` and writing the result
    /// back via [`set_context_violations`](Self::set_context_violations),
    /// typically once per meaningful edit to the collection. Pure render
    /// cache — never persisted, never read from or written to JSON; every
    /// drawing's own serialized form is unaffected by this existing.
    ///
    /// Default: empty — exactly how every primitive outside the Elliott
    /// family behaves (they never call `set_context_violations`), and how
    /// an Elliott primitive behaves before its owner has ever filled this.
    fn context_violations(&self) -> &[Violation] {
        &[]
    }

    /// Replace [`context_violations`](Self::context_violations) — see that
    /// method's own doc for who calls this and when. No-op default,
    /// matching every primitive outside the Elliott family.
    fn set_context_violations(&mut self, _v: Vec<Violation>) {}

    // =========================================================================
    // Elliott Subdivision Overlay (owner report 2026-08-13: "я делаю клик
    // спавню подволну внутри волны, и она вместо того чтобы ЗАМЕНЯТЬ собой
    // отрезок, просто накладывается сверху, и я могу его выдрать; если
    // делать наследование, то он именно прилипает, а тут накладывается —
    // он должен именно что становиться частью" — a spawned child must
    // REPLACE the leg of its parent it nests inside, not draw on top of it.
    // This cache is the DRAWING half of the fix; welding the child's own
    // endpoints onto the parent's own pivots (so it stays attached once
    // moved) is the backend half and lands separately in `mlc-backend` —
    // see `elliott::count::slot_span`'s own doc for the write path that
    // backend half uses.
    // =========================================================================

    /// Which of this primitive's own [`WaveSlot`]s currently carry a child
    /// subdivision nested into them — the slots whose own leg `render` must
    /// no longer draw (the subdivision draws that stretch of the count now)
    /// and whose own leg `hit_test` must no longer answer for (an invisible
    /// leg that still swallows clicks is the same defect turned inside
    /// out). The wave's own pivot points, its label, and its ratio ribs are
    /// UNAFFECTED — they belong to the parent's own count, not to the leg a
    /// subdivision replaces, so they keep drawing (and stay clickable, and
    /// still carry a violation mark) exactly as before.
    ///
    /// A primitive cannot see its own children on its own (`elliott::
    /// count`'s own module doc), so this is not COMPUTED here — it is a
    /// CACHE the collection's own owner fills, exactly like
    /// [`context_violations`](Self::context_violations) is filled. Pure
    /// render cache — never persisted, never read from or written to JSON.
    ///
    /// Default: empty — exactly how every primitive outside the Elliott
    /// family behaves (they never call
    /// [`set_subdivided_slots`](Self::set_subdivided_slots)), and how an
    /// Elliott primitive behaves before its owner has ever filled this.
    fn subdivided_slots(&self) -> &[WaveSlot] {
        &[]
    }

    /// Replace [`subdivided_slots`](Self::subdivided_slots) — see that
    /// method's own doc for who calls this and when. No-op default,
    /// matching every primitive outside the Elliott family.
    fn set_subdivided_slots(&mut self, _slots: Vec<WaveSlot>) {}

    /// This structure's OWN point indices where ANOTHER degree's label
    /// lands on the SAME pivot — the render cache
    /// `wave_label_offset`/`hit_test_label_plates`/`wave_at` read to decide
    /// which labels step out by one rung (regression fix, owner report
    /// 2026-08-14: a per-degree ring that stepped out EVERY label of EVERY
    /// structure was wrong, and so is a slot-only rule that ignores which
    /// LABEL the child actually draws at the shared pivot — see
    /// `elliott::count::label_collision_indices`'s own doc for the actual
    /// rule and why it depends on the child's own type, not just the slot).
    ///
    /// A primitive cannot see its own children on its own (`elliott::
    /// count`'s own module doc), so this is not COMPUTED here — it is a
    /// CACHE the collection's own owner fills (via
    /// `elliott::count::label_collision_indices`), exactly like
    /// [`context_violations`](Self::context_violations) and
    /// [`subdivided_slots`](Self::subdivided_slots) are filled. Pure render
    /// cache — never persisted, never read from or written to JSON.
    ///
    /// Default: empty — exactly how every primitive outside the Elliott
    /// family behaves (they never call
    /// [`set_label_collisions`](Self::set_label_collisions)), and how an
    /// Elliott primitive behaves before its owner has ever filled this (and
    /// always how a CHILD behaves at its own shared pivot — a child reads
    /// only its own cache, never its parent's, so it is never stepped out).
    fn label_collisions(&self) -> &[usize] {
        &[]
    }

    /// Replace [`label_collisions`](Self::label_collisions) — see that
    /// method's own doc for who calls this and when. No-op default,
    /// matching every primitive outside the Elliott family.
    fn set_label_collisions(&mut self, _indices: Vec<usize>) {}

    // =========================================================================
    // Elliott Wave-At-Point Resolution (E3 — owner ask 2026-08-13: a
    // double-click on a wave opens "subdivide this wave into…", a
    // double-click on the structure's own last point opens "what continues
    // this" — the app layer needs the SAME two questions
    // `elliott::guard`/`elliott::count` already answer for rules and
    // subdivision, just asked in screen space instead of data space).
    // =========================================================================

    /// Which [`WaveSlot`] the screen point `(screen_x, screen_y)` lands on,
    /// if any — the single answer combining all three things a user
    /// perceives as "this wave": its own body span, its own label plate,
    /// and (while the primitive's own ratio ribs are showing) a ratio-rib
    /// segment leading into it. See each Elliott type's own `wave_at`
    /// override (built on the shared `elliott::wave_hit::wave_slot_at`) for
    /// the exact three-way search and its nearest-wins tie-break.
    ///
    /// Default: `None` — exactly how every primitive outside the Elliott
    /// family behaves (it has no [`WaveSlot`] of its own to report), and
    /// how the Elliott family behaves before the caller wires this gesture
    /// up.
    fn wave_at(
        &self,
        _screen_x: f64,
        _screen_y: f64,
        _bars: &[Bar],
        _viewport: &Viewport,
        _price_scale: &PriceScale,
    ) -> Option<WaveSlot> {
        None
    }

    /// The [`ControlPointType`] identifying this primitive's own LAST
    /// point — the pivot a successor structure would continue from. NOT a
    /// generic default over [`control_points`](Self::control_points)'s own
    /// last entry: that ordering is NOT a trait-wide contract (e.g.
    /// `Rectangle::control_points` ends with its own `Move` handle, not any
    /// point at all — see that type's own override) — so every Elliott
    /// type overrides this directly against its own known point layout
    /// instead of trusting an assumption `control_points` never promises.
    ///
    /// Default: `None` — exactly how every primitive outside the Elliott
    /// family behaves (it has no notion of a chain "continuing" from it).
    fn terminal_control_point(
        &self,
        _bars: &[Bar],
        _viewport: &Viewport,
        _price_scale: &PriceScale,
    ) -> Option<ControlPointType> {
        None
    }

    /// The [`ControlPointType`] identifying this primitive's own FIRST
    /// point — the pivot a NEW predecessor structure would continue INTO
    /// (owner ask 2026-08-14: a count is read backwards as often as
    /// forwards, so a chain must be buildable from either end). The
    /// mirror of [`terminal_control_point`](Self::terminal_control_point)
    /// — same "not a generic default over `control_points`" reasoning
    /// applies (see that method's own doc): every Elliott type overrides
    /// this directly against its own known point layout instead of
    /// trusting an assumption `control_points` never promises.
    ///
    /// A two-point-realized structure (`terminal_control_point` and this
    /// method both resolving to index 0) reports its FIRST point here and
    /// its LAST point there regardless — the two methods never disagree
    /// with each other about which index they name, only the CALLER
    /// (`ChartApp::try_wave_gesture`) decides which one wins a hit that
    /// lands on the single realized point both describe, and picks the
    /// origin (see that method's own doc for why).
    ///
    /// Default: `None` — exactly how every primitive outside the Elliott
    /// family behaves (it has no notion of a chain "continuing" from it),
    /// same as [`terminal_control_point`](Self::terminal_control_point).
    fn origin_control_point(
        &self,
        _bars: &[Bar],
        _viewport: &Viewport,
        _price_scale: &PriceScale,
    ) -> Option<ControlPointType> {
        None
    }

    // =========================================================================
    // Bar-Derived Cache (2026-08-23 — the gap `channels::regression_trend`'s
    // own `style_properties` doc names: a primitive that must COMPUTE
    // something from price data has no method that hands it both `&mut
    // self` and `bars` at once. Every method already handed `bars`
    // (`render`, `hit_test`, `control_points`, `wave_at`, the two
    // `*_control_point` finders) takes `&self`, and the one place a
    // primitive DOES get `&mut self` at a meaningful edit boundary
    // (`elliott::guard`'s own context-cache setters) is never handed
    // `bars` — Elliott's own sweep filters to `wave_degree().is_some()`
    // and never touches price data at all, by design (its violations come
    // from sibling PRIMITIVES, not bars). This is the missing third
    // combination, generic across every primitive family — not an
    // Elliott-only concept, and not folded into the Elliott sweep despite
    // the two ending up called from the same convergence points in
    // `mlc-backend`.
    // =========================================================================

    /// Recompute whatever this primitive derives from price data into its
    /// own `#[serde(skip)]` cache field(s) — nothing else.
    ///
    /// Contract:
    /// - the caller may invoke this OFTEN (once per bar-array write, once
    ///   per restore) — this method must stay cheap when nothing about the
    ///   bars or the primitive's own fit inputs actually changed; a
    ///   primitive whose derivation is expensive should short-circuit on
    ///   its own (e.g. compare against the last bar count/timestamp it saw)
    ///   rather than assume the caller throttles for it;
    /// - must NEVER mutate anything an analyst set — points, style, level
    ///   configs, text, any field that is NOT `#[serde(skip)]` — only this
    ///   primitive's own derived cache field(s) may change;
    /// - is NEVER the place a value is first computed for DRAWING —
    ///   `render`/`hit_test`/`control_points` keep computing what they draw
    ///   straight from `bars`, exactly as before this method existed, and
    ///   MUST still draw correctly if this method is never called at all
    ///   (a primitive freshly deserialized, or of a type nothing has wired
    ///   a caller for yet, has every `#[serde(skip)]` cache at its own
    ///   `Default`). This cache exists ONLY for a reader that has `&self`
    ///   but no `bars` of its own — a settings-modal stat block today (see
    ///   `channels::regression_trend::RegressionTrend::result_stat_rows`),
    ///   a future volume-profile summary tomorrow.
    ///
    /// Default: no-op — the correct behaviour for every primitive that
    /// derives nothing from price data (the overwhelming majority; only a
    /// type with its own price-derived cache overrides this).
    fn refresh_bar_derived_cache(&mut self, _bars: &[Bar]) {}

    // =========================================================================
    // Serialization
    // =========================================================================

    /// Serialize to JSON for storage
    fn to_json(&self) -> String;

    /// Clone into a boxed trait object
    fn clone_box(&self) -> Box<dyn Primitive>;

    // =========================================================================
    // Alert Integration
    // =========================================================================

    /// Return the line extension mode as a raw `u8` discriminant for alert
    /// boundary detection.  The alerts crate converts this via
    /// `DrawingExtendMode::from_u8`.
    ///
    /// Encoding:
    /// - 0 = None (segment only, default)
    /// - 1 = Right
    /// - 2 = Left
    /// - 3 = Both
    ///
    /// The default implementation returns `0` (no extension).
    /// Override in primitives that have an `ExtendMode` field (e.g., `TrendLine`).
    fn extend_mode_raw(&self) -> u8 {
        0
    }
}

/// Helper trait for cloning boxed primitives
impl Clone for Box<dyn Primitive> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

// =========================================================================
// Convenience methods via extension trait
// =========================================================================

/// Extension trait for common operations
pub trait PrimitiveExt: Primitive {
    /// Get stroke color
    fn stroke_color(&self) -> &str {
        &self.data().color.stroke
    }

    /// Set stroke color
    fn set_stroke_color(&mut self, color: &str) {
        self.data_mut().color.stroke = color.to_string();
    }

    /// Get fill color
    fn fill_color(&self) -> Option<&str> {
        self.data().color.fill.as_deref()
    }

    /// Set fill color
    fn set_fill_color(&mut self, fill: Option<&str>) {
        self.data_mut().color.fill = fill.map(String::from);
    }

    /// Get line width
    fn line_width(&self) -> f64 {
        self.data().width
    }

    /// Set line width
    fn set_line_width(&mut self, width: f64) {
        self.data_mut().width = width.clamp(1.0, 20.0);
    }

    /// Get line style
    fn line_style(&self) -> LineStyle {
        self.data().style
    }

    /// Set line style
    fn set_line_style(&mut self, style: LineStyle) {
        self.data_mut().style = style;
    }

    /// Is visible
    fn is_visible(&self) -> bool {
        self.data().visible
    }

    /// Set visibility
    fn set_visible(&mut self, visible: bool) {
        self.data_mut().visible = visible;
    }

    /// Is locked
    fn is_locked(&self) -> bool {
        self.data().locked
    }

    /// Set locked
    fn set_locked(&mut self, locked: bool) {
        self.data_mut().locked = locked;
    }
}

// Auto-implement PrimitiveExt for all Primitive types
impl<T: Primitive + ?Sized> PrimitiveExt for T {}

// =============================================================================
// Default Rendering
// =============================================================================

/// Render control points for any primitive — the single choke point every
/// selected primitive's handles paint through.
///
/// Shape and radius come from the active [`control_point_style`] (user-level
/// setting, read fresh here every call — never cached, never per-primitive).
/// `stroke`/`fill` are resolved by the CALLER from the active theme (the
/// same `ChartTheme::crosshair`/`ChartTheme::background` fields the
/// crosshair's own magnet dot already uses — see
/// `mlc-app-layout/src/layout/render_chart.rs`) rather than the old literal
/// [`super::types::CONTROL_POINT_STROKE`]/[`super::types::CONTROL_POINT_FILL`]
/// hex constants, so a handle always matches the active theme instead of a
/// hardcoded colour.
pub fn draw_control_points(
    ctx: &mut dyn RenderContext,
    screen_points: &[(f64, f64)],
    stroke: &str,
    fill: &str,
) {
    let style = control_point_style();
    let r = style.radius();

    ctx.set_stroke_color(stroke);
    ctx.set_fill_color(fill);
    ctx.set_stroke_width(1.5);
    ctx.set_line_dash(&[]);

    for (x, y) in screen_points {
        ctx.begin_path();
        match style.shape {
            super::types::ControlPointShape::Circle => {
                ctx.arc(*x, *y, r, 0.0, std::f64::consts::TAU);
            }
            super::types::ControlPointShape::Square => {
                ctx.rect(*x - r, *y - r, r * 2.0, r * 2.0);
            }
            super::types::ControlPointShape::RoundedSquare => {
                ctx.rounded_rect(*x - r, *y - r, r * 2.0, r * 2.0, style.corner_radius());
            }
        }
        ctx.fill();
        ctx.stroke();
    }
}
