//! Color picker state types (pure data, no rendering dependencies)
//!
//! These types manage color picker UI state. Rendering functions live in
//! `zengeld-terminal-core`.

// =============================================================================
// Standard Color Palette
// =============================================================================

/// Standard color palette - 10 columns x 10 rows
/// Row 0: Grayscale (white to black)
/// Rows 1-9: Colors with varying saturation/brightness
pub const STANDARD_PALETTE: &[&str] = &[
    // Row 0: Grayscale
    "#ffffff", "#e0e0e0", "#c0c0c0", "#a0a0a0", "#808080",
    "#606060", "#404040", "#303030", "#202020", "#000000",
    // Row 1: Reds
    "#ffcdd2", "#ef9a9a", "#e57373", "#ef5350", "#f44336",
    "#e53935", "#d32f2f", "#c62828", "#b71c1c", "#880e0e",
    // Row 2: Pinks/Magentas
    "#f8bbd9", "#f48fb1", "#f06292", "#ec407a", "#e91e63",
    "#d81b60", "#c2185b", "#ad1457", "#880e4f", "#560027",
    // Row 3: Purples
    "#e1bee7", "#ce93d8", "#ba68c8", "#ab47bc", "#9c27b0",
    "#8e24aa", "#7b1fa2", "#6a1b9a", "#4a148c", "#2a0054",
    // Row 4: Deep Purples/Indigos
    "#d1c4e9", "#b39ddb", "#9575cd", "#7e57c2", "#673ab7",
    "#5e35b1", "#512da8", "#4527a0", "#311b92", "#1a0060",
    // Row 5: Blues
    "#bbdefb", "#90caf9", "#64b5f6", "#42a5f5", "#2196f3",
    "#1e88e5", "#1976d2", "#1565c0", "#0d47a1", "#002171",
    // Row 6: Cyans/Teals
    "#b2ebf2", "#80deea", "#4dd0e1", "#26c6da", "#00bcd4",
    "#00acc1", "#0097a7", "#00838f", "#006064", "#003d40",
    // Row 7: Greens
    "#c8e6c9", "#a5d6a7", "#81c784", "#66bb6a", "#4caf50",
    "#43a047", "#388e3c", "#2e7d32", "#1b5e20", "#0a3d0a",
    // Row 8: Light Greens/Limes
    "#dcedc8", "#c5e1a5", "#aed581", "#9ccc65", "#8bc34a",
    "#7cb342", "#689f38", "#558b2f", "#33691e", "#1a4010",
    // Row 9: Yellows/Oranges
    "#fff9c4", "#fff59d", "#fff176", "#ffee58", "#ffeb3b",
    "#fdd835", "#fbc02d", "#f9a825", "#f57f17", "#ff6f00",
];

/// Custom colors row (user-defined, starts empty)
pub const MAX_CUSTOM_COLORS: usize = 10;

// =============================================================================
// HSV Color and Conversion Utilities
// =============================================================================

/// HSV color representation
#[derive(Clone, Copy, Debug, Default)]
pub struct HsvColor {
    /// Hue (0.0 - 360.0)
    pub h: f64,
    /// Saturation (0.0 - 1.0)
    pub s: f64,
    /// Value/Brightness (0.0 - 1.0)
    pub v: f64,
}

impl HsvColor {
    pub fn new(h: f64, s: f64, v: f64) -> Self {
        Self { h, s, v }
    }

    /// Convert HSV to RGB hex string
    pub fn to_hex(&self) -> String {
        let (r, g, b) = hsv_to_rgb(self.h, self.s, self.v);
        format!("#{:02x}{:02x}{:02x}", r, g, b)
    }

    /// Convert HSV to RGBA hex string with opacity
    pub fn to_hex_with_alpha(&self, opacity: f64) -> String {
        let (r, g, b) = hsv_to_rgb(self.h, self.s, self.v);
        let a = (opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
        format!("#{:02x}{:02x}{:02x}{:02x}", r, g, b, a)
    }

    /// Create HSV from hex string
    pub fn from_hex(hex: &str) -> Option<Self> {
        let hex = hex.trim_start_matches('#');
        if hex.len() != 6 && hex.len() != 8 {
            return None;
        }
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        let (h, s, v) = rgb_to_hsv(r, g, b);
        Some(Self { h, s, v })
    }
}

/// Apply opacity to a hex color string
/// Returns rgba() format string for CSS compatibility
pub fn apply_opacity_to_hex(hex: &str, opacity: f64) -> String {
    let hex = hex.trim_start_matches('#');
    if hex.len() < 6 {
        return format!("rgba(0,0,0,{})", opacity);
    }
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
    format!("rgba({},{},{},{})", r, g, b, opacity)
}

/// Convert HSV to RGB
/// h: 0-360, s: 0-1, v: 0-1
/// Returns (r, g, b) as u8 values 0-255
pub fn hsv_to_rgb(h: f64, s: f64, v: f64) -> (u8, u8, u8) {
    let h = h % 360.0;
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;

    let (r1, g1, b1) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };

    let r = ((r1 + m) * 255.0).round() as u8;
    let g = ((g1 + m) * 255.0).round() as u8;
    let b = ((b1 + m) * 255.0).round() as u8;

    (r, g, b)
}

/// Convert RGB to HSV
/// r, g, b: 0-255
/// Returns (h, s, v) where h: 0-360, s: 0-1, v: 0-1
pub fn rgb_to_hsv(r: u8, g: u8, b: u8) -> (f64, f64, f64) {
    let r = r as f64 / 255.0;
    let g = g as f64 / 255.0;
    let b = b as f64 / 255.0;

    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;

    let v = max;

    let s = if max == 0.0 { 0.0 } else { delta / max };

    let h = if delta == 0.0 {
        0.0
    } else if max == r {
        60.0 * (((g - b) / delta) % 6.0)
    } else if max == g {
        60.0 * (((b - r) / delta) + 2.0)
    } else {
        60.0 * (((r - g) / delta) + 4.0)
    };

    let h = if h < 0.0 { h + 360.0 } else { h };

    (h, s, v)
}

// =============================================================================
// Color Picker Config Types (data/size calculation only)
// =============================================================================

/// L1 Color picker configuration (quick palette) - size/data portion
#[derive(Clone, Debug)]
pub struct ColorPickerL1Config {
    /// Swatch size
    pub swatch_size: f64,
    /// Gap between swatches
    pub gap: f64,
    /// Number of columns
    pub columns: usize,
    /// Corner radius for swatches
    pub swatch_radius: f64,
    /// Custom colors (user-defined)
    pub custom_colors: Vec<String>,
    /// Current color (for selection highlight)
    pub current_color: Option<String>,
    /// Current opacity (0.0 - 1.0)
    pub opacity: f64,
    /// Whether opacity is toggled off (has stored previous value)
    pub is_opacity_toggled_off: bool,
}

impl Default for ColorPickerL1Config {
    fn default() -> Self {
        Self {
            swatch_size: 18.0,
            gap: 2.0,
            columns: 10,
            swatch_radius: 2.0,
            custom_colors: Vec::new(),
            current_color: None,
            opacity: 1.0,
            is_opacity_toggled_off: false,
        }
    }
}

impl ColorPickerL1Config {
    /// Calculate popup size
    pub fn calculate_size(&self) -> (f64, f64) {
        let padding = 8.0;
        let rows = 10; // Standard palette rows
        let custom_row_height = self.swatch_size + self.gap + 8.0; // Custom colors + "+" button
        let opacity_row_height = 24.0; // Toggle button height (no extra padding needed)

        let width = padding * 2.0 + self.columns as f64 * (self.swatch_size + self.gap) - self.gap;
        let height = padding * 2.0
            + rows as f64 * (self.swatch_size + self.gap) - self.gap  // Palette
            + 8.0 + custom_row_height  // Custom colors section
            + 12.0 + opacity_row_height; // Opacity section (12.0 = gap before opacity row)

        (width, height)
    }
}

/// L2 Color picker configuration (full HSV picker) - data portion
#[derive(Clone, Debug)]
pub struct ColorPickerL2Config {
    /// Current HSV color
    pub hsv: HsvColor,
    /// Current opacity (0.0 - 1.0)
    pub opacity: f64,
    /// Hex input value (may differ from hsv during editing)
    pub hex_input: String,
    /// Whether hex input is being edited
    pub hex_editing: bool,
    /// SV square size
    pub sv_square_size: f64,
    /// Hue bar width
    pub hue_bar_width: f64,
    /// Gap between elements
    pub gap: f64,
    /// Whether opacity is toggled off (has stored previous value)
    pub is_opacity_toggled_off: bool,
}

impl Default for ColorPickerL2Config {
    fn default() -> Self {
        Self {
            hsv: HsvColor::new(0.0, 1.0, 1.0),
            opacity: 1.0,
            hex_input: "#ff0000".to_string(),
            hex_editing: false,
            sv_square_size: 180.0,
            hue_bar_width: 20.0,
            gap: 8.0,
            is_opacity_toggled_off: false,
        }
    }
}

impl ColorPickerL2Config {
    /// Calculate popup size
    pub fn calculate_size(&self) -> (f64, f64) {
        let padding = 12.0;
        let hex_row_height = 32.0;
        let opacity_row_height = 24.0; // Toggle button height
        let button_row_height = 32.0;

        let width = padding * 2.0 + self.sv_square_size + self.gap + self.hue_bar_width;
        let height = padding * 2.0
            + self.sv_square_size  // SV square
            + self.gap + hex_row_height  // Hex input
            + self.gap + opacity_row_height  // Opacity row (toggle + label + slider)
            + self.gap + button_row_height;  // Buttons

        (width, height)
    }

    /// Create from hex color
    pub fn from_hex(hex: &str) -> Self {
        let hsv = HsvColor::from_hex(hex).unwrap_or(HsvColor::new(0.0, 1.0, 1.0));
        Self {
            hsv,
            hex_input: hex.to_string(),
            ..Default::default()
        }
    }
}

// =============================================================================
// Color Picker State (manages L1/L2 transitions)
// =============================================================================

/// Areas in L2 color picker for hover/hit testing
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColorPickerL2Area {
    SVSquare,
    HueBar,
    HexInput,
    OpacitySlider,
    AddButton,
    BackButton,
    Inside,
    Outside,
}

/// Current level of the color picker
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum ColorPickerLevel {
    #[default]
    Closed,
    L1,
    L2,
}

/// Color picker state for managing L1/L2 transitions
#[derive(Clone, Debug, Default)]
pub struct ColorPickerState {
    /// Current level
    pub level: ColorPickerLevel,
    /// Current color (hex)
    pub current_color: String,
    /// Current opacity
    pub opacity: f64,
    /// Previous opacity (for toggle functionality - stores value before setting to 0)
    pub previous_opacity: Option<f64>,
    /// HSV for L2 (only valid when level == L2)
    pub hsv: HsvColor,
    /// Hex input text (may differ from hsv during editing)
    pub hex_input: String,
    /// Whether hex input is focused
    pub hex_editing: bool,
    /// Custom colors saved by user
    pub custom_colors: Vec<String>,
    /// Origin position for popup
    pub origin: (f64, f64),
    /// Whether popup is being dragged
    pub dragging: bool,
    /// Drag start position (mouse position when drag started)
    pub drag_start: (f64, f64),
    /// Currently hovered swatch color (for hover effect)
    pub hovered_swatch: Option<String>,
    /// Currently hovered area in L2 picker
    pub hovered_area: Option<ColorPickerL2Area>,
    /// Whether dragging opacity slider
    pub dragging_opacity: bool,
    /// Whether dragging SV square
    pub dragging_sv: bool,
    /// Whether dragging hue bar
    pub dragging_hue: bool,
}

impl ColorPickerState {
    pub fn new() -> Self {
        Self {
            level: ColorPickerLevel::Closed,
            opacity: 1.0,
            ..Default::default()
        }
    }

    /// Open L1 picker at position with initial color
    pub fn open_l1(&mut self, x: f64, y: f64, color: Option<&str>) {
        self.level = ColorPickerLevel::L1;
        self.origin = (x, y);
        self.opacity = 1.0; // Reset opacity to 100% when opening
        if let Some(c) = color {
            self.current_color = c.to_string();
        }
    }

    /// Transition to L2 (from L1's "+" button)
    pub fn open_l2(&mut self) {
        self.level = ColorPickerLevel::L2;
        // Initialize HSV from current color
        self.hsv = HsvColor::from_hex(&self.current_color)
            .unwrap_or(HsvColor::new(0.0, 1.0, 1.0));
        self.hex_input = self.current_color.clone();
    }

    /// Return to L1 (from L2's back button)
    pub fn back_to_l1(&mut self) {
        self.level = ColorPickerLevel::L1;
        // Update current color from HSV
        self.current_color = self.hsv.to_hex();
    }

    /// Close picker
    pub fn close(&mut self) {
        self.level = ColorPickerLevel::Closed;
        self.hex_editing = false;
    }

    /// Check if picker is open
    pub fn is_open(&self) -> bool {
        self.level != ColorPickerLevel::Closed
    }

    /// Set color from L1 palette selection
    pub fn select_color(&mut self, color: &str) {
        self.current_color = color.to_string();
    }

    /// Update HSV from SV square drag
    pub fn set_sv(&mut self, s: f64, v: f64) {
        self.hsv.s = s;
        self.hsv.v = v;
        self.current_color = self.hsv.to_hex();
        self.hex_input = self.current_color.clone();
    }

    /// Update hue from hue bar drag
    pub fn set_hue(&mut self, h: f64) {
        self.hsv.h = h;
        self.current_color = self.hsv.to_hex();
        self.hex_input = self.current_color.clone();
    }

    /// Get the current opacity value (0.0–1.0)
    pub fn get_opacity(&self) -> f64 {
        self.opacity
    }

    /// Update opacity (manual change via slider - clears previous_opacity)
    pub fn set_opacity(&mut self, opacity: f64) {
        self.opacity = opacity.clamp(0.0, 1.0);
        // Clear previous_opacity when user manually changes opacity
        self.previous_opacity = None;
    }

    /// Toggle opacity between 0 and previous value
    /// Returns the new opacity value
    pub fn toggle_opacity(&mut self) -> f64 {
        if let Some(prev) = self.previous_opacity {
            // Restore previous opacity
            self.opacity = prev;
            self.previous_opacity = None;
        } else {
            // Store current and set to 0
            if self.opacity > 0.001 {
                self.previous_opacity = Some(self.opacity);
                self.opacity = 0.0;
            }
        }
        self.opacity
    }

    /// Check if opacity is currently toggled off (has stored previous value)
    pub fn is_opacity_toggled_off(&self) -> bool {
        self.previous_opacity.is_some()
    }

    /// Set hex input text (during editing)
    pub fn set_hex_input(&mut self, text: &str) {
        self.hex_input = text.to_string();
        // Try to parse and update HSV
        if let Some(hsv) = HsvColor::from_hex(text) {
            self.hsv = hsv;
            self.current_color = text.to_string();
        }
    }

    /// Add current color to custom colors
    pub fn add_to_custom(&mut self) {
        let color = self.hsv.to_hex();
        if !self.custom_colors.contains(&color) && self.custom_colors.len() < MAX_CUSTOM_COLORS {
            self.custom_colors.push(color);
        }
    }

    /// Get the final color with opacity applied as rgba() string
    pub fn get_color_with_opacity(&self) -> String {
        apply_opacity_to_hex(&self.current_color, self.opacity)
    }

    /// Get the final color - returns hex if opacity is 1.0, otherwise rgba()
    pub fn get_final_color(&self) -> String {
        if (self.opacity - 1.0).abs() < 0.001 {
            // Full opacity - return hex
            self.current_color.clone()
        } else {
            // Has transparency - return rgba
            self.get_color_with_opacity()
        }
    }

    /// Get L1 config from state
    pub fn l1_config(&self) -> ColorPickerL1Config {
        ColorPickerL1Config {
            current_color: Some(self.current_color.clone()),
            opacity: self.opacity,
            custom_colors: self.custom_colors.clone(),
            is_opacity_toggled_off: self.is_opacity_toggled_off(),
            ..Default::default()
        }
    }

    /// Get L2 config from state
    pub fn l2_config(&self) -> ColorPickerL2Config {
        ColorPickerL2Config {
            hsv: self.hsv,
            opacity: self.opacity,
            hex_input: self.hex_input.clone(),
            hex_editing: self.hex_editing,
            is_opacity_toggled_off: self.is_opacity_toggled_off(),
            ..Default::default()
        }
    }

    /// Start dragging the popup
    pub fn start_drag(&mut self, mouse_x: f64, mouse_y: f64) {
        self.dragging = true;
        self.drag_start = (mouse_x, mouse_y);
    }

    /// Update position during drag
    pub fn drag(&mut self, mouse_x: f64, mouse_y: f64) {
        if self.dragging {
            let dx = mouse_x - self.drag_start.0;
            let dy = mouse_y - self.drag_start.1;
            self.origin.0 += dx;
            self.origin.1 += dy;
            self.drag_start = (mouse_x, mouse_y);
        }
    }

    /// Stop dragging
    pub fn end_drag(&mut self) {
        self.dragging = false;
    }

    /// Start dragging opacity slider
    pub fn start_opacity_drag(&mut self) {
        self.dragging_opacity = true;
    }

    /// Stop dragging opacity slider
    pub fn end_opacity_drag(&mut self) {
        self.dragging_opacity = false;
    }

    /// Check if dragging opacity
    pub fn is_dragging_opacity(&self) -> bool {
        self.dragging_opacity
    }

    /// Start dragging SV square
    pub fn start_sv_drag(&mut self) {
        self.dragging_sv = true;
    }

    /// Stop dragging SV square
    pub fn end_sv_drag(&mut self) {
        self.dragging_sv = false;
    }

    /// Check if dragging SV
    pub fn is_dragging_sv(&self) -> bool {
        self.dragging_sv
    }

    /// Start dragging hue bar
    pub fn start_hue_drag(&mut self) {
        self.dragging_hue = true;
    }

    /// Stop dragging hue bar
    pub fn end_hue_drag(&mut self) {
        self.dragging_hue = false;
    }

    /// Check if dragging hue
    pub fn is_dragging_hue(&self) -> bool {
        self.dragging_hue
    }

    /// Check if any color picker element is being dragged
    pub fn is_dragging_any(&self) -> bool {
        self.dragging || self.dragging_opacity || self.dragging_sv || self.dragging_hue
    }

    /// Stop all dragging
    pub fn end_all_drags(&mut self) {
        self.dragging = false;
        self.dragging_opacity = false;
        self.dragging_sv = false;
        self.dragging_hue = false;
    }

    /// Set hovered swatch color (for L1 hover effects)
    pub fn set_hovered_swatch(&mut self, color: Option<String>) {
        self.hovered_swatch = color;
    }

    /// Set hovered area (for L2 hover effects)
    pub fn set_hovered_area(&mut self, area: Option<ColorPickerL2Area>) {
        self.hovered_area = area;
    }

    /// Get hovered swatch as &str for rendering
    pub fn hovered_swatch_str(&self) -> Option<&str> {
        self.hovered_swatch.as_deref()
    }

    /// Clamp origin to stay within bounds
    pub fn clamp_to_bounds(&mut self, min_x: f64, min_y: f64, max_x: f64, max_y: f64, popup_width: f64, popup_height: f64) {
        let max_x_clamped = (max_x - popup_width).max(min_x);
        let max_y_clamped = (max_y - popup_height).max(min_y);
        self.origin.0 = self.origin.0.clamp(min_x, max_x_clamped);
        self.origin.1 = self.origin.1.clamp(min_y, max_y_clamped);
    }

    /// Calculate smart popup position that stays within window bounds
    ///
    /// For sidebar buttons: popup appears to the LEFT of button, with right edge touching button
    /// For bottom toolbar: popup appears ABOVE the button
    /// Always clamps to stay within window bounds
    pub fn calculate_smart_origin(
        anchor_x: f64,
        anchor_y: f64,
        _anchor_w: f64,
        anchor_h: f64,
        popup_w: f64,
        popup_h: f64,
        window_w: f64,
        window_h: f64,
        margin: f64,
    ) -> (f64, f64) {
        // Try to position LEFT of anchor (right edge of popup touches left edge of button)
        let mut x = anchor_x - popup_w;
        let mut y = anchor_y;

        // If popup goes off left edge, position it at window edge with margin
        if x < margin {
            // Can't fit left, try BELOW the button instead
            x = anchor_x;
            y = anchor_y + anchor_h + margin;
        }

        // If popup goes off bottom, try ABOVE
        if y + popup_h > window_h - margin {
            y = anchor_y - popup_h;
        }

        // Final clamp: ensure popup stays within window bounds
        x = x.clamp(margin, (window_w - popup_w - margin).max(margin));
        y = y.clamp(margin, (window_h - popup_h - margin).max(margin));

        (x, y)
    }

    /// Open L1 picker with smart positioning within window bounds
    pub fn open_l1_smart(
        &mut self,
        anchor_x: f64,
        anchor_y: f64,
        anchor_w: f64,
        anchor_h: f64,
        window_w: f64,
        window_h: f64,
        color: Option<&str>,
    ) {
        // Calculate popup size for L1
        let config = ColorPickerL1Config::default();
        let (popup_w, popup_h) = config.calculate_size();
        let margin = 4.0;

        let (x, y) = Self::calculate_smart_origin(
            anchor_x, anchor_y,
            anchor_w, anchor_h,
            popup_w, popup_h,
            window_w, window_h,
            margin,
        );

        self.level = ColorPickerLevel::L1;
        self.origin = (x, y);
        self.opacity = 1.0; // Reset opacity to 100% when opening
        if let Some(c) = color {
            self.current_color = c.to_string();
        }
    }

    /// Recalculate position when transitioning to L2 (popup size changes)
    pub fn recalculate_l2_position(&mut self, window_w: f64, window_h: f64) {
        let config = ColorPickerL2Config::default();
        let (popup_w, popup_h) = config.calculate_size();
        let margin = 4.0;

        // Keep current origin but clamp to window bounds
        self.origin.0 = self.origin.0.clamp(margin, (window_w - popup_w - margin).max(margin));
        self.origin.1 = self.origin.1.clamp(margin, (window_h - popup_h - margin).max(margin));
    }
}
