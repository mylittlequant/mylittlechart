//! Pure geometry types cut from mlc-app-layout's toolbar modules, plus color presets.
//!
//! - `WidgetRect`        — from `ui/toolbar_core.rs`
//! - `ToolbarRect`       — from `ui/toolbar_render.rs`
//! - `ToolbarRenderResult` — from `ui/toolbar_render.rs`
//! - `PRESET_COLORS`     — from `ui/sync_color_grid.rs`
//!
//! The live composer (`toolbar_core::draw_toolbar_with_icons`) lives in mlc-app-layout.

// =============================================================================
// WidgetRect (from toolbar_core.rs)
// =============================================================================

/// Axis-aligned rectangle used for toolbar layout and hit-testing.
///
/// Mirrors `uzor::types::Rect` / `zengeld_core::ui::WidgetRect` without
/// pulling in those crates.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WidgetRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl WidgetRect {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self { x, y, width, height }
    }

    pub fn center_x(&self) -> f64 {
        self.x + self.width / 2.0
    }

    pub fn center_y(&self) -> f64 {
        self.y + self.height / 2.0
    }

    pub fn right(&self) -> f64 {
        self.x + self.width
    }

    pub fn bottom(&self) -> f64 {
        self.y + self.height
    }

    pub fn contains(&self, px: f64, py: f64) -> bool {
        px >= self.x && px <= self.right() && py >= self.y && py <= self.bottom()
    }

    /// Shrink the rect by `padding` on all sides.
    pub fn inset(&self, padding: f64) -> Self {
        Self {
            x: self.x + padding,
            y: self.y + padding,
            width: (self.width - padding * 2.0).max(0.0),
            height: (self.height - padding * 2.0).max(0.0),
        }
    }
}

// =============================================================================
// ToolbarRect (from toolbar_render.rs)
// =============================================================================

/// Axis-aligned rectangle used for toolbar layout and hit-testing.
#[derive(Clone, Copy, Debug, Default)]
pub struct ToolbarRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl ToolbarRect {
    /// Create a new rect from position and size.
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, width: w, height: h }
    }

    /// Right edge (exclusive).
    pub fn right(&self) -> f64 {
        self.x + self.width
    }

    /// Bottom edge (exclusive).
    pub fn bottom(&self) -> f64 {
        self.y + self.height
    }

    /// Horizontal centre.
    pub fn center_x(&self) -> f64 {
        self.x + self.width / 2.0
    }

    /// Vertical centre.
    pub fn center_y(&self) -> f64 {
        self.y + self.height / 2.0
    }

    /// Returns `true` if the point `(px, py)` is inside the rect.
    pub fn contains(&self, px: f64, py: f64) -> bool {
        px >= self.x && px < self.right() && py >= self.y && py < self.bottom()
    }
}

// =============================================================================
// ToolbarRenderResult (from toolbar_render.rs)
// =============================================================================

/// Output of a toolbar render pass.
///
/// Contains hit zones that the caller can use to map pointer events back to
/// toolbar items.
#[derive(Clone, Debug, Default)]
pub struct ToolbarRenderResult {
    /// `(item_id, rect)` pairs for every rendered interactive item.
    pub item_rects: Vec<(String, ToolbarRect)>,
}

// =============================================================================
// PRESET_COLORS (from sync_color_grid.rs)
// =============================================================================

/// 12 visually distinct, trading-platform-appropriate preset colors (RGBA, 0.0–1.0).
///
/// Order: Red, Blue, Green, Yellow, Purple, Orange, Cyan, Magenta, Lime, Pink, Teal, Amber.
pub const PRESET_COLORS: [[f32; 4]; 12] = [
    [0.918, 0.263, 0.208, 1.0], // Red        #EA4335
    [0.259, 0.522, 0.957, 1.0], // Blue       #4285F4
    [0.204, 0.659, 0.325, 1.0], // Green      #34A853
    [1.000, 0.839, 0.000, 1.0], // Yellow     #FFD700
    [0.608, 0.349, 0.714, 1.0], // Purple     #9B59B6
    [1.000, 0.596, 0.000, 1.0], // Orange     #FF9800
    [0.000, 0.749, 1.000, 1.0], // Cyan       #00BFFF
    [0.878, 0.000, 0.549, 1.0], // Magenta    #E0008C
    [0.498, 0.855, 0.000, 1.0], // Lime       #7FDB00
    [1.000, 0.427, 0.608, 1.0], // Pink       #FF6D9B
    [0.000, 0.588, 0.533, 1.0], // Teal       #009688
    [1.000, 0.702, 0.000, 1.0], // Amber      #FFB300
];
