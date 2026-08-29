//! Shared types for primitives
//!
//! These types are used by all primitives and the drawing system.

use serde::{Deserialize, Serialize};
use std::sync::{OnceLock, RwLock};

// Re-export LineStyle from annotations
pub use crate::chart::annotations::price_line::LineStyle;

// =============================================================================
// Color Configuration
// =============================================================================

/// Color configuration for primitives
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PrimitiveColor {
    /// Stroke/border color (hex)
    pub stroke: String,
    /// Fill color (hex with alpha, optional)
    pub fill: Option<String>,
}

impl Default for PrimitiveColor {
    fn default() -> Self {
        Self {
            stroke: "#2196F3".to_string(),
            fill: None,
        }
    }
}

impl PrimitiveColor {
    /// Create with stroke color only
    pub fn new(stroke: &str) -> Self {
        Self {
            stroke: stroke.to_string(),
            fill: None,
        }
    }

    /// Create with stroke and fill colors
    pub fn with_fill(stroke: &str, fill: &str) -> Self {
        Self {
            stroke: stroke.to_string(),
            fill: Some(fill.to_string()),
        }
    }

    /// Create semi-transparent fill from stroke color
    pub fn with_alpha_fill(stroke: &str, alpha: u8) -> Self {
        let fill = format!("{}{:02x}", stroke, alpha);
        Self {
            stroke: stroke.to_string(),
            fill: Some(fill),
        }
    }
}

// =============================================================================
// Text Configuration
// =============================================================================

/// Text alignment options
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextAlign {
    #[default]
    Start,  // Left / Top
    Center,
    End,    // Right / Bottom
}

impl TextAlign {
    pub fn as_str(&self) -> &'static str {
        match self {
            TextAlign::Start => "start",
            TextAlign::Center => "center",
            TextAlign::End => "end",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "start" => TextAlign::Start,
            "center" => TextAlign::Center,
            "end" => TextAlign::End,
            _ => TextAlign::Start,
        }
    }
}

/// Text configuration for primitives
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PrimitiveText {
    /// Text content
    pub content: String,
    /// Font size in pixels
    pub font_size: f64,
    /// Text color (defaults to stroke color if None)
    pub color: Option<String>,
    /// Bold text
    pub bold: bool,
    /// Italic text
    pub italic: bool,
    /// Vertical alignment
    pub v_align: TextAlign,
    /// Horizontal alignment
    pub h_align: TextAlign,
}

impl Default for PrimitiveText {
    fn default() -> Self {
        Self {
            content: String::new(),
            font_size: 14.0,
            color: None,
            bold: false,
            italic: false,
            v_align: TextAlign::Start,
            h_align: TextAlign::Center,
        }
    }
}

impl PrimitiveText {
    pub fn new(content: &str) -> Self {
        Self {
            content: content.to_string(),
            font_size: 14.0,
            color: None,
            bold: false,
            italic: false,
            v_align: TextAlign::Start,
            h_align: TextAlign::Center,
        }
    }

    pub fn with_size(content: &str, font_size: f64) -> Self {
        Self {
            content: content.to_string(),
            font_size,
            ..Default::default()
        }
    }
}

// =============================================================================
// Text Anchor (for centralized text rendering)
// =============================================================================

/// Text anchor point for centralized rendering
///
/// Primitives return this to indicate where their text should be drawn.
/// Coordinates are calculated centrally from primitive's points() + these relative params.
#[derive(Clone, Debug)]
pub struct TextAnchor {
    /// Position along primitive: 0.0 = first point, 0.5 = middle, 1.0 = last point
    pub position: f64,
    /// Perpendicular offset from primitive line (pixels, positive = above/left of line direction)
    pub offset: f64,
    /// Rotation angle in radians (for text along angled lines)
    pub rotation: f64,
    /// Fallback color if text.color is None
    pub fallback_color: String,
    /// Optional background color for text
    pub background: Option<String>,
    /// Padding around text (used with background)
    pub padding: f64,
}

impl TextAnchor {
    /// Create a simple text anchor at middle of primitive
    pub fn new(fallback_color: &str) -> Self {
        Self {
            position: 0.5,
            offset: 0.0,
            rotation: 0.0,
            fallback_color: fallback_color.to_string(),
            background: None,
            padding: 0.0,
        }
    }

    /// Create text anchor with position and offset
    pub fn with_position(position: f64, offset: f64, fallback_color: &str) -> Self {
        Self {
            position,
            offset,
            rotation: 0.0,
            fallback_color: fallback_color.to_string(),
            background: None,
            padding: 0.0,
        }
    }

    /// Create text anchor with rotation (for angled lines)
    pub fn with_rotation(position: f64, offset: f64, fallback_color: &str, rotation: f64) -> Self {
        Self {
            position,
            offset,
            rotation,
            fallback_color: fallback_color.to_string(),
            background: None,
            padding: 0.0,
        }
    }

    /// Create text anchor with background
    pub fn with_background(position: f64, offset: f64, fallback_color: &str, bg_color: &str, padding: f64) -> Self {
        Self {
            position,
            offset,
            rotation: 0.0,
            fallback_color: fallback_color.to_string(),
            background: Some(bg_color.to_string()),
            padding,
        }
    }

    /// Create text anchor from PrimitiveText settings
    ///
    /// Uses h_align for position along primitive (Start=0.0, Center=0.5, End=1.0)
    /// Uses v_align for perpendicular offset (Start=above, Center=on, End=below)
    pub fn from_text(text: &super::PrimitiveText, fallback_color: &str) -> Self {
        use super::TextAlign;

        let position = match text.h_align {
            TextAlign::Start => 0.0,
            TextAlign::Center => 0.5,
            TextAlign::End => 1.0,
        };

        let text_offset = 8.0 + text.font_size / 2.0;
        let offset = match text.v_align {
            TextAlign::Start => text_offset,   // above
            TextAlign::Center => 0.0,          // on line
            TextAlign::End => -text_offset,    // below
        };

        Self {
            position,
            offset,
            rotation: 0.0,
            fallback_color: fallback_color.to_string(),
            background: None,
            padding: 0.0,
        }
    }
}

/// Normalize rotation angle for readable text.
///
/// When text is rotated along a line, angles beyond ±90° would make text upside-down.
/// This function flips such angles by ±180° to keep text readable.
///
/// # Arguments
/// * `raw_angle` - The raw angle in radians (typically from `dy.atan2(dx)`)
///
/// # Returns
/// * `(normalized_angle, was_flipped)` - The normalized angle and whether it was flipped
///
/// # Example
/// ```ignore
/// use mlc_core::drawing::normalize_text_rotation;
///
/// // Angle pointing right (0°) - no change
/// let (angle, flipped) = normalize_text_rotation(0.0);
/// assert!(!flipped);
///
/// // Angle pointing left (180°) - flipped to 0°
/// let (angle, flipped) = normalize_text_rotation(std::f64::consts::PI);
/// assert!(flipped);
/// ```
pub fn normalize_text_rotation(raw_angle: f64) -> (f64, bool) {
    use std::f64::consts::{FRAC_PI_2, PI};

    let was_flipped = !(-FRAC_PI_2..=FRAC_PI_2).contains(&raw_angle);
    let normalized = if raw_angle > FRAC_PI_2 {
        raw_angle - PI
    } else if raw_angle < -FRAC_PI_2 {
        raw_angle + PI
    } else {
        raw_angle
    };
    (normalized, was_flipped)
}

/// Parameters for rendering text along a line with rotation
pub struct LineTextParams {
    /// Text position X (screen coords)
    pub x: f64,
    /// Text position Y (screen coords)
    pub y: f64,
    /// Rotation angle in radians (normalized for readability)
    pub rotation: f64,
    /// Bounding box of text in screen coords (for line gap calculation)
    /// Format: (min_x, min_y, max_x, max_y) with padding
    pub text_bbox: Option<(f64, f64, f64, f64)>,
}

/// A line segment defined by two points
#[derive(Clone, Copy, Debug)]
pub struct LineSegment {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
}

/// Calculate line segments that avoid a rotated text bounding box.
/// Returns segments of the line that don't intersect with the text area.
///
/// # Arguments
/// * `x1, y1` - Line start point
/// * `x2, y2` - Line end point
/// * `text_params` - Text parameters including bbox
/// * `padding` - Extra padding around text bbox
///
/// # Returns
/// Vector of line segments to draw (usually 0, 1, or 2 segments)
pub fn line_segments_avoiding_text(
    x1: f64, y1: f64,
    x2: f64, y2: f64,
    text_params: &LineTextParams,
    padding: f64,
) -> Vec<LineSegment> {
    // If no bbox, return full line
    let bbox = match text_params.text_bbox {
        Some(b) => b,
        None => return vec![LineSegment { x1, y1, x2, y2 }],
    };

    let (box_min_x, box_min_y, box_max_x, box_max_y) = bbox;

    // Add padding
    let box_min_x = box_min_x - padding;
    let box_min_y = box_min_y - padding;
    let box_max_x = box_max_x + padding;
    let box_max_y = box_max_y + padding;

    // Line direction
    let dx = x2 - x1;
    let dy = y2 - y1;
    let len = (dx * dx + dy * dy).sqrt();

    if len < 0.001 {
        return vec![LineSegment { x1, y1, x2, y2 }];
    }

    // Find intersections with bbox edges using parametric line equation
    // Point on line: P(t) = (x1 + t*dx, y1 + t*dy), t in [0, 1]
    let mut t_enter = 0.0_f64;
    let mut t_exit = 1.0_f64;

    // Check intersection with vertical edges (x = box_min_x and x = box_max_x)
    if dx.abs() > 0.0001 {
        let t1 = (box_min_x - x1) / dx;
        let t2 = (box_max_x - x1) / dx;
        let (t_min, t_max) = if t1 < t2 { (t1, t2) } else { (t2, t1) };
        t_enter = t_enter.max(t_min);
        t_exit = t_exit.min(t_max);
    } else {
        // Line is vertical - check if it's inside x bounds
        if x1 < box_min_x || x1 > box_max_x {
            // Line doesn't intersect bbox
            return vec![LineSegment { x1, y1, x2, y2 }];
        }
    }

    // Check intersection with horizontal edges (y = box_min_y and y = box_max_y)
    if dy.abs() > 0.0001 {
        let t1 = (box_min_y - y1) / dy;
        let t2 = (box_max_y - y1) / dy;
        let (t_min, t_max) = if t1 < t2 { (t1, t2) } else { (t2, t1) };
        t_enter = t_enter.max(t_min);
        t_exit = t_exit.min(t_max);
    } else {
        // Line is horizontal - check if it's inside y bounds
        if y1 < box_min_y || y1 > box_max_y {
            // Line doesn't intersect bbox
            return vec![LineSegment { x1, y1, x2, y2 }];
        }
    }

    // No intersection if entry point is after exit point
    if t_enter >= t_exit {
        return vec![LineSegment { x1, y1, x2, y2 }];
    }

    // Clamp to line bounds [0, 1]
    t_enter = t_enter.max(0.0);
    t_exit = t_exit.min(1.0);

    // If still no valid intersection
    if t_enter >= t_exit {
        return vec![LineSegment { x1, y1, x2, y2 }];
    }

    // Build segments avoiding the intersection region
    let mut segments = Vec::new();

    // Segment before text
    if t_enter > 0.001 {
        segments.push(LineSegment {
            x1,
            y1,
            x2: x1 + t_enter * dx,
            y2: y1 + t_enter * dy,
        });
    }

    // Segment after text
    if t_exit < 0.999 {
        segments.push(LineSegment {
            x1: x1 + t_exit * dx,
            y1: y1 + t_exit * dy,
            x2,
            y2,
        });
    }

    // If no segments (text covers entire line), return empty
    if segments.is_empty() {
        return segments;
    }

    segments
}

/// Calculate text position and rotation for a two-point line.
///
/// This function:
/// 1. Calculates position along the line based on h_align (Start=0%, Center=50%, End=100%)
/// 2. Calculates perpendicular offset based on v_align (Start=above, Center=on, End=below)
/// 3. Calculates rotation angle to align text with the line
/// 4. Normalizes rotation so text is always readable (not upside-down)
/// 5. Flips the perpendicular offset when rotation is flipped (so "above" stays visually above)
///
/// # Arguments
/// * `x1, y1` - First point screen coordinates
/// * `x2, y2` - Second point screen coordinates
/// * `text` - Text configuration with alignment settings
///
/// # Returns
/// LineTextParams with final position and rotation
pub fn calculate_line_text_params(
    x1: f64, y1: f64,
    x2: f64, y2: f64,
    text: &PrimitiveText,
) -> LineTextParams {
    // Position along line based on h_align
    let t = match text.h_align {
        TextAlign::Start => 0.0,
        TextAlign::Center => 0.5,
        TextAlign::End => 1.0,
    };
    let base_x = x1 + (x2 - x1) * t;
    let base_y = y1 + (y2 - y1) * t;

    let dx = x2 - x1;
    let dy = y2 - y1;
    let len = (dx * dx + dy * dy).sqrt();

    if len < 0.001 {
        return LineTextParams { x: base_x, y: base_y, rotation: 0.0, text_bbox: None };
    }

    // Calculate and normalize rotation for text readability
    let raw_angle = dy.atan2(dx);
    let (rotation, flipped) = normalize_text_rotation(raw_angle);

    // Perpendicular offset from line
    // The perpendicular vector to (dx, dy) is (-dy, dx) normalized
    // This points "to the left" of the line direction
    // But we want Start=above (visually up on screen), End=below (visually down)
    // For a line going right (dx>0), "up" is negative Y, so we need negative perp
    let perp_x = -dy / len;
    let perp_y = dx / len;

    // Offset amount based on v_align
    // Start = above (visually up on screen) = negative perpendicular direction
    // End = below (visually down on screen) = positive perpendicular direction
    let text_offset = 8.0 + text.font_size / 2.0;
    let base_offset = match text.v_align {
        TextAlign::Start => -text_offset,  // above line (visually up)
        TextAlign::Center => 0.0,          // on line
        TextAlign::End => text_offset,     // below line (visually down)
    };

    // When text is flipped (reading direction reversed), flip the perpendicular
    // so "above" stays visually above the line
    let offset = if flipped { -base_offset } else { base_offset };

    let final_x = base_x + perp_x * offset;
    let final_y = base_y + perp_y * offset;

    // Calculate text bounding box for line gap
    // Estimate text width based on character count and font size
    let char_count = text.content.len() as f64;
    let avg_char_width = text.font_size * 0.6;  // Approximate
    let text_width = char_count * avg_char_width;
    let text_height = text.font_size * 1.2;  // Line height

    // The bbox needs to be axis-aligned but the text is rotated
    // We compute the rotated rectangle corners and find the axis-aligned bbox
    let half_w = text_width / 2.0;
    let half_h = text_height / 2.0;

    // Text center is at (final_x, final_y) with rotation
    let cos_r = rotation.cos();
    let sin_r = rotation.sin();

    // Four corners of the text box in rotated space, then transformed
    let corners = [
        (-half_w, -half_h),
        (half_w, -half_h),
        (half_w, half_h),
        (-half_w, half_h),
    ];

    let transformed: Vec<(f64, f64)> = corners.iter().map(|(lx, ly)| {
        let rx = final_x + lx * cos_r - ly * sin_r;
        let ry = final_y + lx * sin_r + ly * cos_r;
        (rx, ry)
    }).collect();

    let min_x = transformed.iter().map(|(x, _)| *x).fold(f64::INFINITY, f64::min);
    let max_x = transformed.iter().map(|(x, _)| *x).fold(f64::NEG_INFINITY, f64::max);
    let min_y = transformed.iter().map(|(_, y)| *y).fold(f64::INFINITY, f64::min);
    let max_y = transformed.iter().map(|(_, y)| *y).fold(f64::NEG_INFINITY, f64::max);

    let text_bbox = if text.content.is_empty() {
        None
    } else {
        Some((min_x, min_y, max_x, max_y))
    };

    LineTextParams {
        x: final_x,
        y: final_y,
        rotation,
        text_bbox,
    }
}

// =============================================================================
// Line Extension Mode
// =============================================================================

/// Line extension mode for trend lines
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExtendMode {
    /// No extension (default for TrendLine)
    #[default]
    None,
    /// Extend to the right only (Ray)
    Right,
    /// Extend to the left only
    Left,
    /// Extend both directions (ExtendedLine)
    Both,
}

// =============================================================================
// Control Points (Handles)
// =============================================================================

/// Type of control point for editing primitives
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ControlPointType {
    /// Move the entire primitive
    Move,
    /// Endpoint 1 (start point)
    Point1,
    /// Endpoint 2 (end point)
    Point2,
    /// Endpoint 3 (for 3-point primitives)
    Point3,
    /// Endpoint 4 (for 4-point primitives like disjoint channel)
    Point4,
    /// Corner handle (for rectangles) - index 0=TL, 1=TR, 2=BR, 3=BL
    Corner(u8),
    /// Edge midpoint handle (for rectangles) - index 0=Top, 1=Right, 2=Bottom, 3=Left
    Edge(u8),
    /// Generic indexed point (for polylines, patterns)
    Index(u8),
}

/// Cursor style for control points
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ControlPointCursor {
    #[default]
    Move,
    ResizeNS,    // North-South (vertical)
    ResizeEW,    // East-West (horizontal)
    ResizeNESW,  // Diagonal NE-SW
    ResizeNWSE,  // Diagonal NW-SE
    Crosshair,   // For adding points
}

impl ControlPointCursor {
    pub fn as_css(&self) -> &'static str {
        match self {
            ControlPointCursor::Move => "move",
            ControlPointCursor::ResizeNS => "ns-resize",
            ControlPointCursor::ResizeEW => "ew-resize",
            ControlPointCursor::ResizeNESW => "nesw-resize",
            ControlPointCursor::ResizeNWSE => "nwse-resize",
            ControlPointCursor::Crosshair => "crosshair",
        }
    }
}

/// A control point (handle) for editing a primitive
#[derive(Clone, Debug)]
pub struct ControlPoint {
    /// Type of control point
    pub point_type: ControlPointType,
    /// X position in screen coordinates
    pub x: f64,
    /// Y position in screen coordinates
    pub y: f64,
    /// Cursor style when hovering
    pub cursor: ControlPointCursor,
}

impl ControlPoint {
    pub fn new(point_type: ControlPointType, x: f64, y: f64, cursor: ControlPointCursor) -> Self {
        Self { point_type, x, y, cursor }
    }

    /// Create a control point with default Move cursor
    pub fn with_type(point_type: ControlPointType, x: f64, y: f64) -> Self {
        Self::new(point_type, x, y, ControlPointCursor::Move)
    }

    pub fn move_point(x: f64, y: f64) -> Self {
        Self::new(ControlPointType::Move, x, y, ControlPointCursor::Move)
    }

    pub fn point1(x: f64, y: f64) -> Self {
        Self::new(ControlPointType::Point1, x, y, ControlPointCursor::Move)
    }

    pub fn point2(x: f64, y: f64) -> Self {
        Self::new(ControlPointType::Point2, x, y, ControlPointCursor::Move)
    }

    pub fn point3(x: f64, y: f64) -> Self {
        Self::new(ControlPointType::Point3, x, y, ControlPointCursor::Move)
    }

    pub fn point4(x: f64, y: f64) -> Self {
        Self::new(ControlPointType::Point4, x, y, ControlPointCursor::Move)
    }

    pub fn index(i: u8, x: f64, y: f64) -> Self {
        Self::new(ControlPointType::Index(i), x, y, ControlPointCursor::Move)
    }
}

// =============================================================================
// Constants
// =============================================================================

/// Hit-test tolerance in pixels
pub const HIT_TOLERANCE: f64 = 10.0;

/// Control point visual radius
pub const CONTROL_POINT_RADIUS: f64 = 5.0;

/// Control point hit-test radius (larger than visual)
pub const CONTROL_POINT_HIT_RADIUS: f64 = 12.0;

/// Control point stroke color
///
/// Literal fallback only — [`draw_control_points`](super::draw_control_points)
/// no longer reads this directly. Live callers resolve stroke/fill from the
/// active theme (`ChartTheme::crosshair` / `ChartTheme::background`, the same
/// fields the crosshair's own magnet dot already uses) and thread them in as
/// parameters. Kept as the documented pre-theming default and for any
/// caller that still wants a hardcoded literal.
pub const CONTROL_POINT_STROKE: &str = "#2196F3";

/// Control point fill color — see [`CONTROL_POINT_STROKE`] doc.
pub const CONTROL_POINT_FILL: &str = "#FFFFFF";

// =============================================================================
// Control Point Style (user-level, configurable) — 2026-08-07
//
// One process-wide setting for every drawing primitive's control-point
// (handle) markers: shape and size. Read fresh, every frame, by
// `draw_control_points` and by the default `Primitive::control_point_hit_test`
// — never copied onto a primitive, never baked into a saved drawing.
// Flipping the active style changes every existing drawing (including ones
// loaded from disk months ago) on the very next frame, because both halves
// call `control_point_style()` instead of caching a value.
//
// Colour is deliberately NOT part of this struct: colour must follow the
// active THEME (a `RuntimeTheme` concept that lives above this crate's
// theme-resolution boundary is not needed here — `mlc-core` already owns
// `ChartTheme`/`RuntimeTheme` — see `draw_control_points`'s `stroke`/`fill`
// parameters), while shape/size are pure user preference independent of
// theme. Mixing the two into one persisted blob would make a theme switch
// require re-deriving stroke/fill inside a settings struct instead of at
// render time, which is exactly the "baked in" trap this seam avoids.
//
// Global, not per-`ChartApp`-instance, because the owner's ask is a
// SYSTEM-level setting ("системная настройка (пользовательский уровень)")
// — one look for every primitive, on every window, mirroring the existing
// `uzor::i18n::{current_lang_index, set_lang_index}` global-language idiom
// this codebase already uses for the same class of setting
// (`mlc_core::i18n::{current_language, set_language}`).
// =============================================================================

/// Visual shape of a control-point (handle) marker.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ControlPointShape {
    /// Filled circle with a stroked ring.
    Circle,
    /// Filled square with rounded corners — the default look (owner's call:
    /// soft corners over a plain circle).
    #[default]
    RoundedSquare,
    /// Filled square, sharp corners.
    Square,
}

/// Visual/hit-test size of a control-point (handle) marker. A small closed
/// set (radio-button UI) rather than a free-typed pixel value, so a saved
/// profile can never carry an out-of-range or degenerate radius.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ControlPointSize {
    Small,
    /// Matches [`CONTROL_POINT_RADIUS`] — today's (and the default) size.
    #[default]
    Medium,
    Large,
}

impl ControlPointSize {
    /// Visual radius in pixels.
    pub fn radius_px(self) -> f64 {
        match self {
            Self::Small => 4.0,
            Self::Medium => CONTROL_POINT_RADIUS,
            Self::Large => 7.0,
        }
    }
}

/// User-level control-point (handle) style — geometry only (see the module
/// doc above for why colour lives elsewhere). This is the ONE struct
/// `draw_control_points` and the default `Primitive::control_point_hit_test`
/// both read, via [`control_point_style`] — the single source of truth that
/// closes the class of bug where a drawn handle's radius and its hitbox
/// radius could independently drift (declared handles nothing could ever
/// grab, or a grab zone bigger than what was drawn).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlPointStyle {
    pub shape: ControlPointShape,
    pub size: ControlPointSize,
}

impl ControlPointStyle {
    /// Visual radius in pixels — what `draw_control_points` paints.
    pub fn radius(&self) -> f64 {
        self.size.radius_px()
    }

    /// Hit-test radius, derived from the visual radius. Preserves the
    /// original 5px-visual / 12px-hit relationship (a fixed +7px grab
    /// margin beyond what's drawn) at every size, so a configurable radius
    /// can never drift from its own hitbox — the whole point of this seam.
    pub fn hit_radius(&self) -> f64 {
        self.radius() + (CONTROL_POINT_HIT_RADIUS - CONTROL_POINT_RADIUS)
    }

    /// Corner radius for [`ControlPointShape::RoundedSquare`] (ignored by
    /// the other shapes) — a fixed proportion of the visual radius rather
    /// than its own persisted/configurable knob, so "rounded square" stays
    /// a single decision (shape) instead of growing a third setting.
    pub fn corner_radius(&self) -> f64 {
        self.radius() * 0.4
    }
}

static CONTROL_POINT_STYLE: OnceLock<RwLock<ControlPointStyle>> = OnceLock::new();

/// Read the active control-point style. Never fails — falls back to
/// [`ControlPointStyle::default`] on a poisoned lock (a styling setting
/// must never crash a render frame).
pub fn control_point_style() -> ControlPointStyle {
    CONTROL_POINT_STYLE
        .get_or_init(|| RwLock::new(ControlPointStyle::default()))
        .read()
        .map(|guard| *guard)
        .unwrap_or_default()
}

/// Set the active control-point style. Called once at startup (profile
/// load, `ChartApp::apply_profile_state`) and again whenever the user
/// changes the General-tab "Control Points" setting — every primitive's
/// handles pick it up on the very next frame because both
/// `draw_control_points` and `control_point_hit_test` read it fresh and
/// never cache it.
pub fn set_control_point_style(style: ControlPointStyle) {
    let lock = CONTROL_POINT_STYLE.get_or_init(|| RwLock::new(ControlPointStyle::default()));
    if let Ok(mut guard) = lock.write() {
        *guard = style;
    }
}

// =============================================================================
// Geometry Utilities
// =============================================================================

/// Calculate distance from point to line segment
pub fn point_to_line_distance(px: f64, py: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    let dx = x2 - x1;
    let dy = y2 - y1;
    let len_sq = dx * dx + dy * dy;

    if len_sq < 0.0001 {
        // Line is a point
        let ddx = px - x1;
        let ddy = py - y1;
        return (ddx * ddx + ddy * ddy).sqrt();
    }

    // Project point onto line, clamping to segment
    let t = ((px - x1) * dx + (py - y1) * dy) / len_sq;
    let t = t.clamp(0.0, 1.0);

    let proj_x = x1 + t * dx;
    let proj_y = y1 + t * dy;

    let ddx = px - proj_x;
    let ddy = py - proj_y;
    (ddx * ddx + ddy * ddy).sqrt()
}

/// Calculate distance from point to an infinite ray starting at `(x1, y1)`
/// and pointing through `(x2, y2)` — like [`point_to_line_distance`] but
/// only clamps the projection at the ray's origin (`t >= 0`), never at the
/// far end. Promoted here from five byte-identical local copies (the
/// `pitchforks` family — `pitchfork.rs`, `schiff.rs`, `modified_schiff.rs`,
/// `inside_pitchfork.rs` — and `gann::gann_fan`), the primitives that draw
/// an "extend" ray past one of their anchors.
pub fn point_to_ray_distance(px: f64, py: f64, x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    let dx = x2 - x1;
    let dy = y2 - y1;
    let len_sq = dx * dx + dy * dy;

    if len_sq < 0.0001 {
        // Ray is a point
        let ddx = px - x1;
        let ddy = py - y1;
        return (ddx * ddx + ddy * ddy).sqrt();
    }

    // Project point onto the ray, clamping only at the origin (t >= 0)
    let t = ((px - x1) * dx + (py - y1) * dy) / len_sq;
    let t = t.max(0.0);

    let proj_x = x1 + t * dx;
    let proj_y = y1 + t * dy;

    let ddx = px - proj_x;
    let ddy = py - proj_y;
    (ddx * ddx + ddy * ddy).sqrt()
}

/// Extend a screen-space segment `(x1,y1)-(x2,y2)` horizontally to the panel
/// edges, independently per side: `extend_left` walks the `(x1,y1)` end back
/// along the segment's own slope to `x = 0`; `extend_right` walks the
/// `(x2,y2)` end forward to `x = chart_width`. A side left `false` keeps its
/// original anchor untouched — the return value is `(x1,y1,x2,y2)` unchanged
/// when both flags are `false`.
///
/// Same per-side slope-projection math as `TrendLine`'s own `ExtendMode`
/// render arms (`lines/trend_line.rs`), the working reference this
/// generalizes — parameterized by two independent bools instead of one
/// shared enum because the channel family (`channels/*.rs`,
/// `regression_trend.rs`) needs each side toggleable across MULTIPLE
/// boundary lines with independent slopes (`disjoint_channel`'s two lines
/// are not parallel), not one mode shared by a single line. Callers use the
/// SAME returned coordinates for both the drawn stroke and the hit-test
/// segment, so a click reaches exactly as far as the line is actually drawn.
///
/// Near-vertical segments (`dx` ~ 0) fall back to a large parametric `t`
/// rather than a defined intersection — a segment with (about) zero
/// horizontal run has no meaningful "reach the edge" height, so this keeps
/// `TrendLine`'s own guard-band intent (`-1000.0`/`1000.0`) without
/// hardcoding the literal at every call site.

/// Snap `(x1,y1)` toward `(x0,y0)` onto the nearest of 8 compass
/// directions (0/45/90/…). Drawing-app Shift while placing a line/ray.
pub fn snap_compass8(x0: f64, y0: f64, x1: f64, y1: f64) -> (f64, f64) {
    let dx = x1 - x0;
    let dy = y1 - y0;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1e-9 {
        return (x1, y1);
    }
    let ang = dy.atan2(dx);
    let step = std::f64::consts::PI / 4.0;
    let snapped = (ang / step).round() * step;
    (x0 + len * snapped.cos(), y0 + len * snapped.sin())
}

/// Shift-constrain a box tool to a square in screen pixels (equal |dx|, |dy|).
pub fn snap_square(x0: f64, y0: f64, x1: f64, y1: f64) -> (f64, f64) {
    let dx = x1 - x0;
    let dy = y1 - y0;
    let m = dx.abs().max(dy.abs());
    if m < 1e-9 {
        return (x1, y1);
    }
    (x0 + m * dx.signum(), y0 + m * dy.signum())
}

/// True for tools whose Shift-constrain is "square/circle", not 8-compass.
pub fn shift_snaps_square(tool_id: &str) -> bool {
    matches!(
        tool_id,
        "rectangle" | "rotated_rectangle" | "ellipse" | "circle"
            | "gann_box" | "price_date_range" | "price_range" | "date_range" | "triangle"
    )
}

pub fn snap_drawing_screen(tool_id: &str, x0: f64, y0: f64, x1: f64, y1: f64) -> (f64, f64) {
    if shift_snaps_square(tool_id) {
        snap_square(x0, y0, x1, y1)
    } else {
        snap_compass8(x0, y0, x1, y1)
    }
}

/// Shift-snap a data-space cursor against the first placed point.
pub fn snap_drawing_data(
    tool_id: &str,
    bars: &[crate::Bar],
    vp: &crate::Viewport,
    price_min: f64,
    price_max: f64,
    ts0: i64,
    p0: f64,
    ts1: i64,
    p1: f64,
) -> (i64, f64) {
    let x0 = vp.bar_to_x_f64(crate::timestamp_ms_to_bar_f64(bars, ts0));
    let y0 = vp.price_to_y(p0, price_min, price_max);
    let x1 = vp.bar_to_x_f64(crate::timestamp_ms_to_bar_f64(bars, ts1));
    let y1 = vp.price_to_y(p1, price_min, price_max);
    let (sx, sy) = snap_drawing_screen(tool_id, x0, y0, x1, y1);
    let bar = vp.x_to_bar_f64(sx);
    let price = vp.y_to_price(sy, price_min, price_max);
    (crate::bar_f64_to_timestamp_ms(bars, bar), price)
}

pub fn extend_segment_to_edges(
    x1: f64, y1: f64, x2: f64, y2: f64,
    extend_left: bool, extend_right: bool,
    chart_width: f64,
) -> (f64, f64, f64, f64) {
    let dx = x2 - x1;
    let dy = y2 - y1;
    let (ex1, ey1) = if extend_left {
        let t = if dx.abs() > 0.001 { -x1 / dx } else { -1000.0 };
        (0.0, y1 + dy * t)
    } else {
        (x1, y1)
    };
    let (ex2, ey2) = if extend_right {
        let t = if dx.abs() > 0.001 { (chart_width - x1) / dx } else { 1000.0 };
        (chart_width, y1 + dy * t)
    } else {
        (x2, y2)
    };
    (ex1, ey1, ex2, ey2)
}

#[cfg(test)]
mod extend_segment_to_edges_tests {
    use super::extend_segment_to_edges;

    /// Neither side extended: the segment is returned byte-for-byte, the
    /// exact "today's rendering" guarantee every new `extend_left`/
    /// `extend_right` field on the channel family relies on.
    #[test]
    fn no_extension_returns_original_segment() {
        let (x1, y1, x2, y2) = extend_segment_to_edges(100.0, 50.0, 300.0, 150.0, false, false, 1200.0);
        assert_eq!((x1, y1, x2, y2), (100.0, 50.0, 300.0, 150.0));
    }

    /// Extending reaches the panel edges and moves the reached endpoint
    /// FURTHER from the original segment's own midpoint than the
    /// unextended endpoint was — the "reaches further" behaviour every
    /// extend-geometry test in the channel family asserts.
    #[test]
    fn extension_reaches_further_than_unextended_segment() {
        let (ux1, _uy1, ux2, _uy2) = extend_segment_to_edges(100.0, 50.0, 300.0, 150.0, false, false, 1200.0);
        let (ex1, _ey1, ex2, _ey2) = extend_segment_to_edges(100.0, 50.0, 300.0, 150.0, true, true, 1200.0);
        assert_eq!(ex1, 0.0, "extend_left reaches the left panel edge exactly");
        assert_eq!(ex2, 1200.0, "extend_right reaches the right panel edge exactly");
        assert!(ex1 < ux1, "extended left endpoint is further left than the unextended one");
        assert!(ex2 > ux2, "extended right endpoint is further right than the unextended one");
    }
}

// =============================================================================
// Stroke hit-testing — shared by every family whose `hit_test` must cover
// every segment its `render` actually draws (doctrine: every drawn edge is
// clickable over its full length; the interior fill never is).
// =============================================================================

/// A single screen-space line segment — the drawn-stroke unit hit-testing
/// operates on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment {
    pub x1: f64,
    pub y1: f64,
    pub x2: f64,
    pub y2: f64,
}

impl Segment {
    pub fn new(x1: f64, y1: f64, x2: f64, y2: f64) -> Self {
        Self { x1, y1, x2, y2 }
    }
}

/// Shortest distance from `(px, py)` to any segment in `segments`.
/// `f64::INFINITY` if `segments` is empty (never hits).
pub fn nearest_segment_distance(px: f64, py: f64, segments: &[Segment]) -> f64 {
    segments
        .iter()
        .map(|s| point_to_line_distance(px, py, s.x1, s.y1, s.x2, s.y2))
        .fold(f64::INFINITY, f64::min)
}

/// True if `(px, py)` lies within `tol` pixels of any segment's drawn
/// length. The one-call shape most `hit_test` bodies want: build the same
/// segment list `render` draws, then test it.
pub fn hit_test_strokes(px: f64, py: f64, segments: &[Segment], tol: f64) -> bool {
    nearest_segment_distance(px, py, segments) < tol
}
