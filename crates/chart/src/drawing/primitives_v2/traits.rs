//! Core primitive trait - the foundation for all drawing primitives
//!
//! This trait-based architecture allows adding new primitives without
//! modifying the DrawingManager.

use serde::{Deserialize, Serialize};
use crate::{PriceScale, Viewport};
use super::types::{ControlPoint, ControlPointType, LineStyle, PrimitiveColor, PrimitiveText, TextAlign, CONTROL_POINT_RADIUS};
use crate::render::{RenderContext, crisp};
use super::config::{TimeframeVisibilityConfig, ConfigProperty, PropertyValue, PropertyCategory};

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
    /// Multiple points until double-click or Enter (e.g., polyline)
    /// The u8 is the minimum number of points needed
    MultiPoint(u8),
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
    /// Timestamps for each point (Unix seconds) - source of truth for timeframe-independent positioning
    /// Indices correspond to points() order. Empty if not yet synced.
    #[serde(default)]
    pub point_timestamps: Vec<i64>,
    /// Origin primitive ID — if Some, this is a synced clone from another window.
    /// When sync is disabled, clones (origin_id.is_some()) are purged.
    /// None means this is an original primitive drawn on this window.
    #[serde(default)]
    pub origin_id: Option<u64>,
    /// The symbol this primitive was created for.
    #[serde(default)]
    pub symbol: String,
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
        }
    }
}

impl PrimitiveData {
    /// Get base config properties (common to all primitives)
    pub fn base_properties(&self) -> Vec<ConfigProperty> {
        vec![
            ConfigProperty::color("stroke_color", "Line Color", &self.color.stroke)
                .with_category(PropertyCategory::Style)
                .with_order(0),
            ConfigProperty::number("width", "Line Width", self.width, Some(1.0), Some(10.0))
                .with_category(PropertyCategory::Style)
                .with_order(1),
            ConfigProperty::line_style("style", "Line Style", self.style.as_str())
                .with_category(PropertyCategory::Style)
                .with_order(2),
            ConfigProperty::boolean("locked", "Locked", self.locked)
                .with_category(PropertyCategory::Style)
                .with_order(100),
            ConfigProperty::boolean("visible", "Visible", self.visible)
                .with_category(PropertyCategory::Visibility)
                .with_order(0),
        ]
    }

    /// Get text properties (if primitive has text configured)
    pub fn text_properties(&self) -> Vec<ConfigProperty> {
        let mut props = Vec::new();
        if let Some(ref text) = self.text {
            props.push(
                ConfigProperty::comment(&text.content)
                    .with_order(0)
            );
            props.push(
                ConfigProperty::font_size(text.font_size)
                    .with_category(PropertyCategory::Text)
                    .with_order(1)
            );
            props.push(
                ConfigProperty::text_color(text.color.as_deref().unwrap_or(&self.color.stroke))
                    .with_category(PropertyCategory::Text)
                    .with_order(2)
            );
            props.push(
                ConfigProperty::bold(text.bold)
                    .with_category(PropertyCategory::Text)
                    .with_order(3)
            );
            props.push(
                ConfigProperty::italic(text.italic)
                    .with_category(PropertyCategory::Text)
                    .with_order(4)
            );
            props.push(
                ConfigProperty::h_align(text.h_align.as_str())
                    .with_category(PropertyCategory::Text)
                    .with_order(5)
            );
            props.push(
                ConfigProperty::v_align(text.v_align.as_str())
                    .with_category(PropertyCategory::Text)
                    .with_order(6)
            );
        }
        props
    }

    /// Apply a property value to base data
    pub fn apply_property(&mut self, id: &str, value: &PropertyValue) -> bool {
        match id {
            "stroke_color" => {
                if let Some(c) = value.as_color() {
                    self.color.stroke = c.to_string();
                    return true;
                }
            }
            "fill_color" => {
                if let Some(c) = value.as_color() {
                    self.color.fill = Some(c.to_string());
                    return true;
                }
            }
            "width" => {
                if let Some(w) = value.as_number() {
                    self.width = w.clamp(1.0, 10.0);
                    return true;
                }
            }
            "style" => {
                if let Some(s) = value.as_string() {
                    self.style = LineStyle::from_str(s);
                    return true;
                }
            }
            "locked" => {
                if let Some(b) = value.as_bool() {
                    self.locked = b;
                    return true;
                }
            }
            "visible" => {
                if let Some(b) = value.as_bool() {
                    self.visible = b;
                    return true;
                }
            }
            // Text properties
            "text_content" => {
                if let Some(s) = value.as_string() {
                    if let Some(ref mut text) = self.text {
                        text.content = s.to_string();
                    } else {
                        self.text = Some(PrimitiveText::new(s));
                    }
                    return true;
                }
            }
            "text_font_size" => {
                if let Some(size) = value.as_number() {
                    if let Some(ref mut text) = self.text {
                        text.font_size = size.clamp(8.0, 72.0);
                        return true;
                    }
                }
            }
            "text_color" => {
                if let Some(c) = value.as_color() {
                    if let Some(ref mut text) = self.text {
                        text.color = Some(c.to_string());
                        return true;
                    }
                }
            }
            "text_bold" => {
                if let Some(b) = value.as_bool() {
                    if let Some(ref mut text) = self.text {
                        text.bold = b;
                        return true;
                    }
                }
            }
            "text_italic" => {
                if let Some(b) = value.as_bool() {
                    if let Some(ref mut text) = self.text {
                        text.italic = b;
                        return true;
                    }
                }
            }
            "text_h_align" => {
                if let Some(s) = value.as_string() {
                    if let Some(ref mut text) = self.text {
                        text.h_align = match s {
                            "start" => TextAlign::Start,
                            "center" => TextAlign::Center,
                            "end" => TextAlign::End,
                            _ => TextAlign::Center,
                        };
                        return true;
                    }
                }
            }
            "text_v_align" => {
                if let Some(s) = value.as_string() {
                    if let Some(ref mut text) = self.text {
                        text.v_align = match s {
                            "start" => TextAlign::Start,
                            "center" => TextAlign::Center,
                            "end" => TextAlign::End,
                            _ => TextAlign::Start,
                        };
                        return true;
                    }
                }
            }
            _ => {}
        }
        false
    }

    /// Initialize text if primitive supports it (call in new() of primitives)
    pub fn init_text(&mut self, default_content: &str) {
        if self.text.is_none() {
            self.text = Some(PrimitiveText::new(default_content));
        }
    }

    /// Check if text is enabled
    pub fn has_text(&self) -> bool {
        self.text.is_some()
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

    /// Get all coordinate points as (bar, price) pairs
    fn points(&self) -> Vec<(f64, f64)>;

    /// Set coordinate points (for creation and editing)
    fn set_points(&mut self, points: &[(f64, f64)]);

    /// Translate the primitive by bar/price delta
    fn translate(&mut self, bar_delta: f64, price_delta: f64);

    /// Move a specific control point to new coordinates
    fn move_control_point(&mut self, point_type: ControlPointType, bar: f64, price: f64);

    /// Move a specific control point using screen coordinates
    /// Default implementation converts to data coords and calls move_control_point
    /// Override for primitives that need screen-space resize (emoji, image)
    fn move_control_point_screen(
        &mut self,
        point_type: ControlPointType,
        screen_x: f64,
        screen_y: f64,
        viewport: &Viewport,
        price_scale: &PriceScale,
    ) {
        let bar = viewport.x_to_bar_f64(screen_x);
        let price = viewport.y_to_price(screen_y, price_scale.price_min, price_scale.price_max);
        self.move_control_point(point_type, bar, price);
    }

    // =========================================================================
    // Hit Testing
    // =========================================================================

    /// Hit test at screen coordinates
    fn hit_test(
        &self,
        screen_x: f64,
        screen_y: f64,
        viewport: &Viewport,
        price_scale: &PriceScale,
    ) -> HitTestResult;

    /// Get control points in screen coordinates
    fn control_points(
        &self,
        viewport: &Viewport,
        price_scale: &PriceScale,
    ) -> Vec<ControlPoint>;

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
    fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool) {
        // Inline default implementation to avoid Sized bound issues
        let data = self.data();
        let points = self.points();
        let kind = self.kind();
        let dpr = ctx.dpr();

        if points.is_empty() {
            return;
        }

        // Convert to screen coordinates
        let screen_points: Vec<(f64, f64)> = points
            .iter()
            .map(|(bar, price)| (ctx.bar_to_x(*bar), ctx.price_to_y(*price)))
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

        if is_selected {
            draw_control_points(ctx, &screen_points);
        }
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

/// Render control points for any primitive
pub fn draw_control_points(ctx: &mut dyn RenderContext, screen_points: &[(f64, f64)]) {
    ctx.set_stroke_color(super::types::CONTROL_POINT_STROKE);
    ctx.set_fill_color(super::types::CONTROL_POINT_FILL);
    ctx.set_stroke_width(1.5);
    ctx.set_line_dash(&[]);

    for (x, y) in screen_points {
        ctx.begin_path();
        ctx.arc(*x, *y, CONTROL_POINT_RADIUS as f64, 0.0, std::f64::consts::TAU);
        ctx.fill();
        ctx.stroke();
    }
}
