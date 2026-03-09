//! Widget types and state structures
//!
//! Common types used across widget rendering.

/// Widget interaction state
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WidgetState {
    #[default]
    Normal,
    Hovered,
    Pressed,
    Disabled,
}

impl WidgetState {
    pub fn is_hovered(&self) -> bool {
        matches!(self, Self::Hovered | Self::Pressed)
    }

    pub fn is_pressed(&self) -> bool {
        matches!(self, Self::Pressed)
    }

    pub fn is_disabled(&self) -> bool {
        matches!(self, Self::Disabled)
    }
}

/// Common widget theme colors
#[derive(Clone, Debug)]
pub struct WidgetTheme {
    // Background colors
    pub bg_normal: String,
    pub bg_hover: String,
    pub bg_pressed: String,
    pub bg_disabled: String,

    // Text colors
    pub text_normal: String,
    pub text_hover: String,
    pub text_disabled: String,

    // Border colors
    pub border_normal: String,
    pub border_hover: String,
    pub border_focused: String,

    // Accent colors
    pub accent: String,
    pub accent_hover: String,

    // State colors
    pub success: String,
    pub warning: String,
    pub danger: String,
}

impl Default for WidgetTheme {
    fn default() -> Self {
        Self {
            // Dark theme defaults
            bg_normal: "#2a2e39".to_string(),
            bg_hover: "#363a45".to_string(),
            bg_pressed: "#434651".to_string(),
            bg_disabled: "#1e222d".to_string(),

            text_normal: "#d1d4dc".to_string(),
            text_hover: "#ffffff".to_string(),
            text_disabled: "#6a6d78".to_string(),

            border_normal: "#363a45".to_string(),
            border_hover: "#4a4e59".to_string(),
            border_focused: "#2196F3".to_string(),

            accent: "#2196F3".to_string(),
            accent_hover: "#42a5f5".to_string(),

            success: "#26a69a".to_string(),
            warning: "#ff9800".to_string(),
            danger: "#ef5350".to_string(),
        }
    }
}

impl WidgetTheme {
    /// Create a light theme
    pub fn light() -> Self {
        Self {
            bg_normal: "#f0f3fa".to_string(),
            bg_hover: "#e0e3eb".to_string(),
            bg_pressed: "#d0d4dc".to_string(),
            bg_disabled: "#f8f9fb".to_string(),

            text_normal: "#131722".to_string(),
            text_hover: "#000000".to_string(),
            text_disabled: "#9598a1".to_string(),

            border_normal: "#e0e3eb".to_string(),
            border_hover: "#c8ccd4".to_string(),
            border_focused: "#2196F3".to_string(),

            accent: "#2196F3".to_string(),
            accent_hover: "#1976d2".to_string(),

            success: "#26a69a".to_string(),
            warning: "#ff9800".to_string(),
            danger: "#ef5350".to_string(),
        }
    }
}
