//! UI Style System - Visual effects orthogonal to themes
//!
//! Styles define HOW colors are applied (opacity, blur, effects),
//! while themes define WHAT colors are used.
//!
//! Theme (colors) x Style (effects) = Final appearance
//!
//! # Styles
//! - **Solid**: Opaque backgrounds (default, current behavior)
//! - **Glass**: Semi-transparent backgrounds with subtle transparency
//! - **FrostedGlassFlat**: Blur effect with flat buttons

use serde::{Deserialize, Serialize};

// =============================================================================
// Glass Button Style
// =============================================================================

/// Style for hover/active buttons in FrostedGlass/LiquidGlass modes
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum GlassButtonStyle {
    /// Flat buttons - blur background + semi-transparent color overlay
    #[default]
    Flat,
    /// 3D convex buttons - raised glass lens effect with specular highlights
    Convex3D,
}

impl GlassButtonStyle {
    /// Get display name
    pub fn label(&self) -> &'static str {
        match self {
            Self::Flat => "Flat",
            Self::Convex3D => "3D Convex",
        }
    }

    /// Get description
    pub fn description(&self) -> &'static str {
        match self {
            Self::Flat => "Flat buttons with blur background",
            Self::Convex3D => "Raised glass buttons with specular highlights",
        }
    }

    /// All available styles
    pub fn all() -> &'static [GlassButtonStyle] {
        &[Self::Flat, Self::Convex3D]
    }

    /// Get style from index
    pub fn from_index(idx: usize) -> Option<Self> {
        Self::all().get(idx).copied()
    }

    /// Get index of this style
    pub fn index(&self) -> usize {
        Self::all().iter().position(|s| s == self).unwrap_or(0)
    }
}

// =============================================================================
// UI Style Enum
// =============================================================================

/// UI visual style (orthogonal to theme colors)
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum UIStyle {
    /// Opaque backgrounds - current default behavior
    #[default]
    Solid,
    /// Semi-transparent backgrounds (opacity ~0.75-0.9)
    Glass,
    /// Frosted glass with flat blur buttons
    FrostedGlassFlat,
}

impl UIStyle {
    /// Get display name
    pub fn label(&self) -> &'static str {
        match self {
            Self::Solid => "Solid",
            Self::Glass => "Glass",
            Self::FrostedGlassFlat => "Frosted Glass",
        }
    }

    /// Get description
    pub fn description(&self) -> &'static str {
        match self {
            Self::Solid => "Opaque backgrounds (default)",
            Self::Glass => "Semi-transparent backgrounds",
            Self::FrostedGlassFlat => "Blur effect with flat buttons",
        }
    }

    /// All available styles
    pub fn all() -> &'static [UIStyle] {
        &[
            Self::Solid,
            Self::Glass,
            Self::FrostedGlassFlat,
        ]
    }

    /// Get style from index
    pub fn from_index(idx: usize) -> Option<Self> {
        Self::all().get(idx).copied()
    }

    /// Get index of this style
    pub fn index(&self) -> usize {
        Self::all().iter().position(|s| s == self).unwrap_or(0)
    }

    /// Get default style params for this style
    pub fn default_params(&self) -> StyleParams {
        match self {
            Self::Solid => StyleParams::solid(),
            Self::Glass => StyleParams::glass(),
            Self::FrostedGlassFlat => StyleParams::frosted_glass_flat(),
        }
    }

    /// Check if this style uses blur effects
    pub fn has_blur(&self) -> bool {
        matches!(self, Self::FrostedGlassFlat)
    }
}

// =============================================================================
// Style Parameters
// =============================================================================

/// Parameters that control how styles are rendered
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StyleParams {
    // === Opacity ===
    /// Background opacity for toolbars (1.0 = solid, 0.0 = transparent)
    pub toolbar_bg_opacity: f32,
    /// Background opacity for modals/dropdowns
    pub modal_bg_opacity: f32,
    /// Background opacity for sidebar panels
    pub sidebar_bg_opacity: f32,
    /// Background opacity for context menus
    pub menu_bg_opacity: f32,
    /// Background opacity for price/time scales
    pub scale_bg_opacity: f32,
    /// Background opacity for sub-panes (indicator panes below main chart)
    pub sub_pane_bg_opacity: f32,
    /// Background opacity for hover state overlays
    pub hover_bg_opacity: f32,
    /// Background opacity for active state overlays
    pub active_bg_opacity: f32,
    /// Background opacity for crosshair labels on scales
    pub crosshair_label_bg_opacity: f32,

    // === Blur (for Glass/LiquidGlass) ===
    /// Backdrop blur radius in pixels (0 = no blur)
    pub blur_radius: f32,

    // === Borders ===
    /// Border opacity multiplier (1.0 = full, 0.5 = subtle)
    pub border_opacity: f32,
    /// Add subtle glow to borders on hover
    pub border_glow: bool,
    /// Border glow color (usually accent color with alpha)
    pub border_glow_color: String,
    /// Border glow spread in pixels
    pub border_glow_spread: f32,

    // === Hover Effects ===
    /// Intensity of hover highlight (0.0-1.0)
    pub hover_highlight_intensity: f32,
    /// Enable gradient shimmer effect on hover (LiquidGlass)
    pub hover_shimmer: bool,
    /// Shimmer animation duration in ms
    pub shimmer_duration_ms: u32,

    // === Shadows ===
    /// Shadow opacity multiplier
    pub shadow_opacity: f32,
    /// Enable soft shadow for floating elements
    pub soft_shadow: bool,

    // === Toolbar Style ===
    /// Use sidebar-style buttons with accent indicator for vertical toolbars
    /// When true, vertical toolbar buttons have a 3px accent bar on the left (like modal sidebars)
    pub toolbar_sidebar_style: bool,

    // === Glass Button Style ===
    /// Style for hover/active buttons in FrostedGlass/LiquidGlass modes
    /// Flat = blur + color overlay, Convex3D = raised glass lens effect
    pub glass_button_style: GlassButtonStyle,

    // === Liquid Glass Effects ===
    /// Refractive index for lens distortion (1.0 = no distortion, 1.15 = subtle, 1.3 = strong)
    pub liquid_refraction: f32,
    /// Chromatic aberration strength (RGB channel separation, 0.0-0.02)
    pub liquid_chromatic: f32,
    /// Specular highlight intensity (0.0-0.5)
    pub liquid_specular: f32,
    /// Wave amplitude for cursor/ripple effects (1.0-10.0)
    pub liquid_amplitude: f32,
    /// Cursor influence radius in pixels (50-200)
    pub liquid_cursor_radius: f32,
    /// Cursor trail length - how many positions to remember for trail effect (5-30)
    pub liquid_trail_length: u32,
    /// Ripple speed in pixels per second (100-400)
    pub liquid_ripple_speed: f32,
    /// Ripple duration in seconds (1.0-4.0)
    pub liquid_ripple_duration: f32,
}

impl Default for StyleParams {
    fn default() -> Self {
        Self::solid()
    }
}

impl StyleParams {
    /// Solid style - fully opaque, no effects
    pub fn solid() -> Self {
        Self {
            toolbar_bg_opacity: 1.0,
            modal_bg_opacity: 1.0,
            sidebar_bg_opacity: 1.0,
            menu_bg_opacity: 1.0,
            scale_bg_opacity: 1.0,
            sub_pane_bg_opacity: 1.0,
            hover_bg_opacity: 1.0,
            active_bg_opacity: 1.0,
            crosshair_label_bg_opacity: 1.0,
            blur_radius: 0.0,
            border_opacity: 1.0,
            border_glow: false,
            border_glow_color: "rgba(41, 98, 255, 0.3)".to_string(),
            border_glow_spread: 0.0,
            hover_highlight_intensity: 0.1,
            hover_shimmer: false,
            shimmer_duration_ms: 0,
            shadow_opacity: 1.0,
            soft_shadow: false,
            // Toolbar style
            toolbar_sidebar_style: false, // Standard rounded buttons for Solid
            // Glass button style (not used in Solid)
            glass_button_style: GlassButtonStyle::Flat,
            // Liquid glass defaults (not used in Solid)
            liquid_refraction: 1.0,
            liquid_chromatic: 0.0,
            liquid_specular: 0.0,
            liquid_amplitude: 0.0,
            liquid_cursor_radius: 0.0,
            liquid_trail_length: 0,
            liquid_ripple_speed: 0.0,
            liquid_ripple_duration: 0.0,
        }
    }

    /// Glass style - semi-transparent backgrounds
    pub fn glass() -> Self {
        Self {
            toolbar_bg_opacity: 0.85,
            modal_bg_opacity: 0.9,
            sidebar_bg_opacity: 0.88,
            menu_bg_opacity: 0.92,
            scale_bg_opacity: 0.85,
            sub_pane_bg_opacity: 0.85,
            hover_bg_opacity: 0.7,
            active_bg_opacity: 0.8,
            crosshair_label_bg_opacity: 0.5,
            blur_radius: 0.0, // No blur in simple glass
            border_opacity: 0.6,
            border_glow: false,
            border_glow_color: "rgba(41, 98, 255, 0.3)".to_string(),
            border_glow_spread: 0.0,
            hover_highlight_intensity: 0.15,
            hover_shimmer: false,
            shimmer_duration_ms: 0,
            shadow_opacity: 0.8,
            soft_shadow: true,
            // Toolbar style
            toolbar_sidebar_style: false, // Standard rounded buttons for Glass
            // Glass button style (not used in Glass - no blur)
            glass_button_style: GlassButtonStyle::Flat,
            // Liquid glass defaults (not used in Glass)
            liquid_refraction: 1.0,
            liquid_chromatic: 0.0,
            liquid_specular: 0.0,
            liquid_amplitude: 0.0,
            liquid_cursor_radius: 0.0,
            liquid_trail_length: 0,
            liquid_ripple_speed: 0.0,
            liquid_ripple_duration: 0.0,
        }
    }

    /// Frosted Glass style with flat blur buttons
    pub fn frosted_glass_flat() -> Self {
        Self {
            toolbar_bg_opacity: 0.0,
            modal_bg_opacity: 0.0,
            sidebar_bg_opacity: 0.0,
            menu_bg_opacity: 0.0,
            scale_bg_opacity: 0.0,
            sub_pane_bg_opacity: 0.0,
            hover_bg_opacity: 0.5,
            active_bg_opacity: 0.4,
            crosshair_label_bg_opacity: 0.3,
            blur_radius: 12.0,
            border_opacity: 0.4,
            border_glow: false,
            border_glow_color: "rgba(41, 98, 255, 0.3)".to_string(),
            border_glow_spread: 0.0,
            hover_highlight_intensity: 0.15,
            hover_shimmer: false,
            shimmer_duration_ms: 0,
            shadow_opacity: 0.6,
            soft_shadow: true,
            toolbar_sidebar_style: true,
            glass_button_style: GlassButtonStyle::Flat,
            liquid_refraction: 1.0,
            liquid_chromatic: 0.0,
            liquid_specular: 0.0,
            liquid_amplitude: 0.0,
            liquid_cursor_radius: 0.0,
            liquid_trail_length: 0,
            liquid_ripple_speed: 0.0,
            liquid_ripple_duration: 0.0,
        }
    }

    /// Apply opacity to a color string (e.g., "#1e222d" -> "rgba(30, 34, 45, 0.85)")
    pub fn apply_opacity(&self, color: &str, opacity_type: OpacityType) -> String {
        let opacity = match opacity_type {
            OpacityType::Toolbar => self.toolbar_bg_opacity,
            OpacityType::Modal => self.modal_bg_opacity,
            OpacityType::Sidebar => self.sidebar_bg_opacity,
            OpacityType::Menu => self.menu_bg_opacity,
            OpacityType::Scale => self.scale_bg_opacity,
            OpacityType::SubPane => self.sub_pane_bg_opacity,
            OpacityType::Hover => self.hover_bg_opacity,
            OpacityType::Active => self.active_bg_opacity,
            OpacityType::Border => self.border_opacity,
            OpacityType::CrosshairLabel => self.crosshair_label_bg_opacity,
            OpacityType::Custom(o) => o,
        };

        // If opacity is 1.0, return original color
        if (opacity - 1.0).abs() < 0.001 {
            return color.to_string();
        }

        // Parse hex color and convert to rgba
        if let Some(rgba) = hex_to_rgba(color, opacity) {
            rgba
        } else {
            // If color is already rgba or unparseable, try to modify alpha
            modify_alpha(color, opacity)
        }
    }
}

/// Type of element for opacity selection
#[derive(Clone, Copy, Debug)]
pub enum OpacityType {
    Toolbar,
    Modal,
    Sidebar,
    Menu,
    Scale,
    SubPane,
    Hover,
    Active,
    Border,
    CrosshairLabel,
    Custom(f32),
}

// =============================================================================
// Color Utilities
// =============================================================================

/// Convert hex color to rgba with specified alpha
fn hex_to_rgba(hex: &str, alpha: f32) -> Option<String> {
    let hex = hex.trim_start_matches('#');

    if hex.len() == 6 {
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        Some(format!("rgba({}, {}, {}, {:.2})", r, g, b, alpha))
    } else if hex.len() == 3 {
        let r = u8::from_str_radix(&hex[0..1], 16).ok()? * 17;
        let g = u8::from_str_radix(&hex[1..2], 16).ok()? * 17;
        let b = u8::from_str_radix(&hex[2..3], 16).ok()? * 17;
        Some(format!("rgba({}, {}, {}, {:.2})", r, g, b, alpha))
    } else {
        None
    }
}

/// Modify alpha in an existing color string
fn modify_alpha(color: &str, alpha: f32) -> String {
    // Handle rgba(r, g, b, a) format
    if color.starts_with("rgba(") {
        if let Some(end) = color.rfind(',') {
            return format!("{}, {:.2})", &color[..end], alpha);
        }
    }
    // Handle rgb(r, g, b) format
    if color.starts_with("rgb(") {
        let inner = color.trim_start_matches("rgb(").trim_end_matches(')');
        return format!("rgba({}, {:.2})", inner, alpha);
    }
    // Return original if can't parse
    color.to_string()
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hex_to_rgba() {
        assert_eq!(hex_to_rgba("#1e222d", 0.85), Some("rgba(30, 34, 45, 0.85)".to_string()));
        assert_eq!(hex_to_rgba("#fff", 0.5), Some("rgba(255, 255, 255, 0.50)".to_string()));
        assert_eq!(hex_to_rgba("2962ff", 1.0), Some("rgba(41, 98, 255, 1.00)".to_string()));
    }

    #[test]
    fn test_modify_alpha() {
        assert_eq!(modify_alpha("rgba(30, 34, 45, 1.0)", 0.5), "rgba(30, 34, 45, 0.50)");
        assert_eq!(modify_alpha("rgb(30, 34, 45)", 0.75), "rgba(30, 34, 45, 0.75)");
    }

    #[test]
    fn test_style_params_apply_opacity() {
        let params = StyleParams::glass();
        let result = params.apply_opacity("#1e222d", OpacityType::Toolbar);
        assert!(result.starts_with("rgba("));
        assert!(result.contains("0.85"));
    }

    #[test]
    fn test_solid_no_opacity_change() {
        let params = StyleParams::solid();
        let result = params.apply_opacity("#1e222d", OpacityType::Toolbar);
        assert_eq!(result, "#1e222d");
    }
}
