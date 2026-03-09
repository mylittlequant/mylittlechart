//! Shared types for primitives
//!
//! These types are used by all primitives and the drawing system.

use serde::{Deserialize, Serialize};

// Re-export LineStyle from annotations
pub use crate::chart::annotations::price_line::LineStyle;

// =============================================================================
// Color Configuration
// =============================================================================

/// Color configuration for primitives
#[derive(Clone, Debug, Serialize, Deserialize)]
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
/// use zengeld_chart::drawing::normalize_text_rotation;
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

    let was_flipped = raw_angle > FRAC_PI_2 || raw_angle < -FRAC_PI_2;
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
    /// Level handle (for Fibonacci) - index is level number
    Level(u8),
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
pub const CONTROL_POINT_STROKE: &str = "#2196F3";

/// Control point fill color
pub const CONTROL_POINT_FILL: &str = "#FFFFFF";

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

/// Check if point is inside rectangle
pub fn point_in_rect(px: f64, py: f64, x1: f64, y1: f64, x2: f64, y2: f64, tolerance: f64) -> bool {
    let (min_x, max_x) = if x1 < x2 { (x1, x2) } else { (x2, x1) };
    let (min_y, max_y) = if y1 < y2 { (y1, y2) } else { (y2, y1) };
    px >= min_x - tolerance && px <= max_x + tolerance &&
    py >= min_y - tolerance && py <= max_y + tolerance
}

/// Calculate distance from point to point
pub fn point_distance(x1: f64, y1: f64, x2: f64, y2: f64) -> f64 {
    let dx = x2 - x1;
    let dy = y2 - y1;
    (dx * dx + dy * dy).sqrt()
}
