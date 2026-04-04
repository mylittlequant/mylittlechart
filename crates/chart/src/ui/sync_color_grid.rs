//! Sync Color Grid popup for panel color tag selection.
//!
//! A lightweight replacement for the full L1/L2 color picker when assigning
//! sync-group color tags to leaf panels. Shows 12 preset swatches in a 4×3
//! grid plus up to 6 user-defined custom colors and a "Remove" row.

use uzor::panels::LeafId;
use uzor::render::{RenderContext, TextAlign, TextBaseline};

// =============================================================================
// Preset Colors
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

// =============================================================================
// Layout constants
// =============================================================================

const PADDING: f64 = 6.0;
const SWATCH_SIZE: f64 = 20.0;
const GAP: f64 = 3.0;
const COLUMNS: usize = 4;
const SEPARATOR_H: f64 = 1.0;
const SEPARATOR_GAP: f64 = 4.0;
const REMOVE_ROW_H: f64 = 22.0;

// =============================================================================
// SyncColorGridHitResult
// =============================================================================

/// Result of a hit-test against the sync color grid popup.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SyncColorGridHitResult {
    /// A color swatch was clicked. Index into `SyncColorGridState::all_colors()`.
    Color(usize),
    /// The "+" add-custom button was clicked.
    AddCustom,
    /// The "Remove" row was clicked.
    Remove,
    /// Click was inside the popup but did not land on an actionable element.
    Inside,
    /// Click was outside the popup entirely.
    Outside,
}

// =============================================================================
// SyncColorGridState
// =============================================================================

/// State for the sync color grid popup.
#[derive(Clone, Debug)]
pub struct SyncColorGridState {
    /// Whether the popup is currently open.
    pub open: bool,
    /// Popup top-left corner in screen coordinates.
    pub origin: (f64, f64),
    /// Which leaf's color tag is being edited (set when opened).
    pub target_leaf: Option<LeafId>,
    /// User-defined custom colors (up to 6).
    pub custom_colors: Vec<[f32; 4]>,
    /// Index of the currently hovered swatch (0..all_colors().len()), or `None`.
    pub hovered_index: Option<usize>,
    /// Whether the "Remove" row is currently hovered.
    pub hovered_remove: bool,
    /// Whether the "+" button is currently hovered.
    pub hovered_add: bool,
    /// When `true`, the panel color picker is open for adding a custom color.
    /// After the picker closes, the chosen color is added to `custom_colors`.
    pub adding_custom_color: bool,
}

impl Default for SyncColorGridState {
    fn default() -> Self {
        Self::new()
    }
}

impl SyncColorGridState {
    /// Create a new, closed state.
    pub fn new() -> Self {
        Self {
            open: false,
            origin: (0.0, 0.0),
            target_leaf: None,
            custom_colors: Vec::new(),
            hovered_index: None,
            hovered_remove: false,
            hovered_add: false,
            adding_custom_color: false,
        }
    }

    /// Open the popup for `leaf_id`, anchored near `(anchor_x, anchor_y)`.
    ///
    /// The popup is positioned so it stays within the window bounds
    /// `(window_w, window_h)`.
    pub fn open(
        &mut self,
        leaf_id: LeafId,
        anchor_x: f64,
        anchor_y: f64,
        window_w: f64,
        window_h: f64,
    ) {
        self.target_leaf = Some(leaf_id);
        self.hovered_index = None;
        self.hovered_remove = false;
        self.hovered_add = false;
        self.open = true;

        let (pw, ph) = self.popup_size();
        let margin = 4.0;

        // Try to open below anchor; if it goes off screen move above.
        let mut x = anchor_x;
        let mut y = anchor_y;

        // If going off the right edge, shift left.
        if x + pw > window_w - margin {
            x = (window_w - pw - margin).max(margin);
        }
        // If going off the bottom edge, open above anchor.
        if y + ph > window_h - margin {
            y = (anchor_y - ph).max(margin);
        }
        x = x.clamp(margin, (window_w - pw - margin).max(margin));
        y = y.clamp(margin, (window_h - ph - margin).max(margin));

        self.origin = (x, y);
    }

    /// Close the popup.
    pub fn close(&mut self) {
        self.open = false;
        self.target_leaf = None;
        self.hovered_index = None;
        self.hovered_remove = false;
        self.hovered_add = false;
    }

    /// Reopen the popup at its previous origin for `leaf_id`.
    ///
    /// Used after the custom color picker flow to restore the grid
    /// at the same position it was before.
    pub fn reopen(&mut self, leaf_id: LeafId) {
        self.target_leaf = Some(leaf_id);
        self.hovered_index = None;
        self.hovered_remove = false;
        self.hovered_add = false;
        self.open = true;
        // origin stays unchanged from the previous open()
    }

    /// Returns `true` if the popup is currently open.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Calculate the popup dimensions based on content.
    ///
    /// Layout (top-to-bottom):
    /// - Padding
    /// - Preset rows (3 rows of 4 swatches)
    /// - Separator
    /// - Custom row (custom swatches + "+" if <6 custom)
    /// - Separator
    /// - Remove row
    /// - Padding
    pub fn popup_size(&self) -> (f64, f64) {
        let preset_rows = 3usize; // 12 presets / 4 columns
        let preset_h = preset_rows as f64 * (SWATCH_SIZE + GAP) - GAP;

        // Custom row is shown even when empty (just the "+" button).
        let custom_h = SWATCH_SIZE;

        let width = PADDING * 2.0 + COLUMNS as f64 * (SWATCH_SIZE + GAP) - GAP;
        let height = PADDING
            + preset_h
            + SEPARATOR_GAP + SEPARATOR_H + SEPARATOR_GAP
            + custom_h
            + SEPARATOR_GAP + SEPARATOR_H + SEPARATOR_GAP
            + REMOVE_ROW_H
            + PADDING;

        (width, height)
    }

    /// Returns all colors: 12 presets followed by any custom colors.
    ///
    /// Indices 0–11 are preset colors, indices 12–(12+n-1) are custom colors.
    pub fn all_colors(&self) -> Vec<[f32; 4]> {
        let mut colors: Vec<[f32; 4]> = PRESET_COLORS.to_vec();
        colors.extend_from_slice(&self.custom_colors);
        colors
    }

    /// Convert an `[f32; 4]` RGBA color to a CSS hex string for rendering.
    fn rgba_to_hex(rgba: [f32; 4]) -> String {
        let r = (rgba[0].clamp(0.0, 1.0) * 255.0).round() as u8;
        let g = (rgba[1].clamp(0.0, 1.0) * 255.0).round() as u8;
        let b = (rgba[2].clamp(0.0, 1.0) * 255.0).round() as u8;
        format!("#{:02x}{:02x}{:02x}", r, g, b)
    }
}

// =============================================================================
// SyncColorGridDrawResult
// =============================================================================

/// Hit-zone data produced by `draw_sync_color_grid`.
///
/// Used by `hit_test_sync_color_grid` to map mouse coordinates to actions.
#[derive(Clone, Debug, Default)]
pub struct SyncColorGridDrawResult {
    /// Total popup bounding rect `[x, y, w, h]`.
    pub popup_rect: [f64; 4],
    /// Swatch rects: `(color_index, [x, y, w, h])`.
    pub swatch_rects: Vec<(usize, [f64; 4])>,
    /// "+" add-custom button rect, if present.
    pub add_button_rect: Option<[f64; 4]>,
    /// "Remove" row rect.
    pub remove_rect: [f64; 4],
}

// =============================================================================
// draw_sync_color_grid
// =============================================================================

/// Draw the sync color grid popup and return hit-zone data.
///
/// # Parameters
/// - `ctx`           — render context
/// - `state`         — popup state (origin, custom colors, hover highlights)
/// - `current_color` — the color currently assigned to the target leaf (for highlight)
/// - `toolbar_theme` — toolbar theme for corporate/consistent colors
pub fn draw_sync_color_grid(
    ctx: &mut dyn RenderContext,
    state: &SyncColorGridState,
    current_color: Option<[f32; 4]>,
    toolbar_theme: &crate::ui::toolbar_render::ToolbarTheme,
) -> SyncColorGridDrawResult {
    if !state.open {
        return SyncColorGridDrawResult::default();
    }

    let (pw, ph) = state.popup_size();
    let ox = state.origin.0;
    let oy = state.origin.1;

    let all_colors = state.all_colors();

    // -------------------------------------------------------------------------
    // Background
    // -------------------------------------------------------------------------
    ctx.save();

    // Popup background (from toolbar theme)
    ctx.set_fill_color(&toolbar_theme.dropdown_bg);
    ctx.fill_rounded_rect(ox, oy, pw, ph, 6.0);

    // 1px border (from toolbar theme separator)
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(ox, oy, pw, ph, 6.0);

    // -------------------------------------------------------------------------
    // Helper: draw one swatch cell
    // -------------------------------------------------------------------------
    let draw_swatch = |ctx: &mut dyn RenderContext,
                       sx: f64, sy: f64,
                       color: [f32; 4],
                       is_current: bool,
                       is_hovered: bool| {
        let hex = SyncColorGridState::rgba_to_hex(color);
        ctx.set_fill_color(&hex);
        ctx.fill_rounded_rect(sx, sy, SWATCH_SIZE, SWATCH_SIZE, 3.0);

        if is_current {
            // White border for selected color
            ctx.set_stroke_color("#ffffff");
            ctx.set_stroke_width(2.0);
            ctx.stroke_rounded_rect(sx - 1.0, sy - 1.0, SWATCH_SIZE + 2.0, SWATCH_SIZE + 2.0, 4.0);
        } else if is_hovered {
            // Semi-transparent white border for hovered color
            ctx.set_stroke_color("#ffffff99"); // rgba(255,255,255,0.6)
            ctx.set_stroke_width(2.0);
            ctx.stroke_rounded_rect(sx - 1.0, sy - 1.0, SWATCH_SIZE + 2.0, SWATCH_SIZE + 2.0, 4.0);
        }
    };

    // -------------------------------------------------------------------------
    // Render preset rows (3 rows × 4 columns)
    // -------------------------------------------------------------------------
    let mut swatch_rects: Vec<(usize, [f64; 4])> = Vec::new();
    let preset_count = PRESET_COLORS.len(); // 12

    let mut cursor_y = oy + PADDING;

    for row in 0..3usize {
        for col in 0..COLUMNS {
            let idx = row * COLUMNS + col;
            if idx >= preset_count {
                break;
            }
            let sx = ox + PADDING + col as f64 * (SWATCH_SIZE + GAP);
            let sy = cursor_y;

            let color = all_colors[idx];
            let is_current = current_color.is_some_and(|c| colors_match(c, color));
            let is_hovered = state.hovered_index == Some(idx);

            draw_swatch(ctx, sx, sy, color, is_current, is_hovered);
            swatch_rects.push((idx, [sx, sy, SWATCH_SIZE, SWATCH_SIZE]));
        }
        cursor_y += SWATCH_SIZE + GAP;
    }
    // Remove the last extra GAP
    cursor_y -= GAP;

    // -------------------------------------------------------------------------
    // Separator after presets
    // -------------------------------------------------------------------------
    cursor_y += SEPARATOR_GAP;
    ctx.set_fill_color(&toolbar_theme.separator);
    ctx.fill_rect(ox + PADDING, cursor_y, pw - PADDING * 2.0, SEPARATOR_H);
    cursor_y += SEPARATOR_H + SEPARATOR_GAP;

    // -------------------------------------------------------------------------
    // Custom colors row
    // -------------------------------------------------------------------------
    let custom_start_idx = preset_count;
    let max_custom = 6usize;
    let custom_count = state.custom_colors.len().min(max_custom);

    for i in 0..custom_count {
        let idx = custom_start_idx + i;
        let sx = ox + PADDING + i as f64 * (SWATCH_SIZE + GAP);
        let sy = cursor_y;

        let color = all_colors[idx];
        let is_current = current_color.is_some_and(|c| colors_match(c, color));
        let is_hovered = state.hovered_index == Some(idx);

        draw_swatch(ctx, sx, sy, color, is_current, is_hovered);
        swatch_rects.push((idx, [sx, sy, SWATCH_SIZE, SWATCH_SIZE]));
    }

    // "+" button (if < 6 custom colors)
    let add_button_rect: Option<[f64; 4]> = if custom_count < max_custom {
        let plus_x = ox + PADDING + custom_count as f64 * (SWATCH_SIZE + GAP);
        let plus_y = cursor_y;

        // Hover effect: brighter border and text when hovered
        let (border_color, text_color_add) = if state.hovered_add {
            ("#ffffff80", "#ffffffaa") // brighter on hover
        } else {
            ("#ffffff40", "#ffffff66") // default dim
        };

        // Dashed border rectangle
        ctx.set_stroke_color(border_color);
        ctx.set_stroke_width(1.0);
        ctx.set_line_dash(&[3.0, 3.0]);
        ctx.stroke_rounded_rect(plus_x, plus_y, SWATCH_SIZE, SWATCH_SIZE, 3.0);
        ctx.set_line_dash(&[]);

        // "+" text
        ctx.set_fill_color(text_color_add);
        ctx.set_font("12px sans-serif");
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text("+", plus_x + SWATCH_SIZE / 2.0, plus_y + SWATCH_SIZE / 2.0);

        Some([plus_x, plus_y, SWATCH_SIZE, SWATCH_SIZE])
    } else {
        None
    };

    cursor_y += SWATCH_SIZE;

    // -------------------------------------------------------------------------
    // Separator after custom row
    // -------------------------------------------------------------------------
    cursor_y += SEPARATOR_GAP;
    ctx.set_fill_color(&toolbar_theme.separator);
    ctx.fill_rect(ox + PADDING, cursor_y, pw - PADDING * 2.0, SEPARATOR_H);
    cursor_y += SEPARATOR_H + SEPARATOR_GAP;

    // -------------------------------------------------------------------------
    // Remove row
    // -------------------------------------------------------------------------
    let remove_rect = [ox, cursor_y, pw, REMOVE_ROW_H];

    if state.hovered_remove {
        // Hover highlight
        ctx.set_fill_color("#ff443320"); // rgba(255,68,51,0.125)
        ctx.fill_rounded_rect(ox + 2.0, cursor_y, pw - 4.0, REMOVE_ROW_H, 3.0);
    }

    // "Remove" text
    let text_color = if state.hovered_remove { "#ff6b5b" } else { &toolbar_theme.item_text };
    ctx.set_fill_color(text_color);
    ctx.set_font("11px sans-serif");
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(
        "Remove",
        ox + pw / 2.0,
        cursor_y + REMOVE_ROW_H / 2.0,
    );

    ctx.restore();

    SyncColorGridDrawResult {
        popup_rect: [ox, oy, pw, ph],
        swatch_rects,
        add_button_rect,
        remove_rect,
    }
}

// =============================================================================
// hit_test_sync_color_grid
// =============================================================================

/// Hit-test a mouse position against the draw result of the sync color grid.
///
/// Returns the matching `SyncColorGridHitResult`.
pub fn hit_test_sync_color_grid(
    draw_result: &SyncColorGridDrawResult,
    mouse_x: f64,
    mouse_y: f64,
) -> SyncColorGridHitResult {
    let [px, py, pw, ph] = draw_result.popup_rect;
    if mouse_x < px || mouse_x >= px + pw || mouse_y < py || mouse_y >= py + ph {
        return SyncColorGridHitResult::Outside;
    }

    // Check remove row first (full-width, easy check)
    let [rx, ry, rw, rh] = draw_result.remove_rect;
    if mouse_x >= rx && mouse_x < rx + rw && mouse_y >= ry && mouse_y < ry + rh {
        return SyncColorGridHitResult::Remove;
    }

    // Check "+" add button
    if let Some([ax, ay, aw, ah]) = draw_result.add_button_rect {
        if mouse_x >= ax && mouse_x < ax + aw && mouse_y >= ay && mouse_y < ay + ah {
            return SyncColorGridHitResult::AddCustom;
        }
    }

    // Check swatch rects
    for &(idx, [sx, sy, sw, sh]) in &draw_result.swatch_rects {
        if mouse_x >= sx && mouse_x < sx + sw && mouse_y >= sy && mouse_y < sy + sh {
            return SyncColorGridHitResult::Color(idx);
        }
    }

    SyncColorGridHitResult::Inside
}

// =============================================================================
// Helper
// =============================================================================

/// Returns `true` if two `[f32; 4]` colors are approximately equal (within 1/512 per channel).
fn colors_match(a: [f32; 4], b: [f32; 4]) -> bool {
    let tol = 1.0 / 512.0;
    (a[0] - b[0]).abs() < tol
        && (a[1] - b[1]).abs() < tol
        && (a[2] - b[2]).abs() < tol
        && (a[3] - b[3]).abs() < tol
}
