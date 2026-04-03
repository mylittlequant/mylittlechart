//! Theme Manager - Centralized theme management for zengeld-chart
//!
//! ThemeManager is the single source of truth for all theme operations.
//! It handles:
//! - Preset switching (dark, light, high_contrast, high_contrast_mono, mascot)
//! - Individual color/font modifications
//! - CSS export for web platforms
//! - JSON serialization for persistence
//!
//! # Usage
//! ```ignore
//! let mut manager = ThemeManager::new();
//! manager.set_preset("dark");           // Switch preset
//! manager.set_toolbar_bg("#ff0000");    // Customize color
//! let css = manager.to_css_variables(); // Export for web
//! ```

use super::runtime::RuntimeTheme;

/// Centralized theme management
/// Single source of truth for all theme operations
pub struct ThemeManager {
    /// Current active theme (runtime-modifiable)
    current: RuntimeTheme,

    /// Base preset name (for "reset to preset" functionality)
    base_preset: String,

    /// Dirty flag for optimization (skip re-render if unchanged)
    dirty: bool,
}

impl ThemeManager {
    /// Create new ThemeManager with dark theme
    pub fn new() -> Self {
        Self {
            current: RuntimeTheme::from_preset("dark"),
            base_preset: "dark".to_string(),
            dirty: false,
        }
    }

    /// Create with specific preset
    pub fn with_preset(name: &str) -> Self {
        Self {
            current: RuntimeTheme::from_preset(name),
            base_preset: name.to_string(),
            dirty: false,
        }
    }

    // =========================================================================
    // Preset operations
    // =========================================================================

    /// Switch to a preset theme
    pub fn set_preset(&mut self, name: &str) {
        self.current = RuntimeTheme::from_preset(name);
        self.base_preset = name.to_string();
        self.dirty = true;
    }

    /// Reset current theme to base preset (discards customizations)
    pub fn reset_to_preset(&mut self) {
        self.current = RuntimeTheme::from_preset(&self.base_preset);
        self.dirty = true;
    }

    /// Get current preset name
    pub fn preset_name(&self) -> &str {
        &self.base_preset
    }

    /// Get available preset names
    pub fn presets() -> &'static [&'static str] {
        RuntimeTheme::PRESETS
    }

    // =========================================================================
    // Individual setters - UI Colors
    // =========================================================================

    pub fn set_toolbar_bg(&mut self, color: &str) {
        self.current.colors.toolbar_bg = color.to_string();
        self.dirty = true;
    }

    pub fn set_button_bg(&mut self, color: &str) {
        self.current.colors.button_bg = color.to_string();
        self.dirty = true;
    }

    pub fn set_button_hover_bg(&mut self, color: &str) {
        self.current.colors.button_bg_hover = color.to_string();
        self.dirty = true;
    }

    pub fn set_button_active_bg(&mut self, color: &str) {
        self.current.colors.button_bg_active = color.to_string();
        self.dirty = true;
    }
    pub fn set_button_hover_stroke(&mut self, color: &str) {
        self.current.colors.button_hover_stroke = color.to_string();
        self.dirty = true;
    }

    pub fn set_button_active_stroke(&mut self, color: &str) {
        self.current.colors.button_active_stroke = color.to_string();
        self.dirty = true;
    }

    pub fn set_button_rounding(&mut self, value: f32) {
        self.current.colors.button_rounding = value;
        self.dirty = true;
    }

    pub fn set_dropdown_bg(&mut self, color: &str) {
        self.current.colors.dropdown_bg = color.to_string();
        self.dirty = true;
    }

    pub fn set_status_bar_bg(&mut self, color: &str) {
        self.current.colors.status_bar_bg = color.to_string();
        self.dirty = true;
    }

    pub fn set_text_primary(&mut self, color: &str) {
        self.current.colors.text_primary = color.to_string();
        self.dirty = true;
    }

    pub fn set_text_secondary(&mut self, color: &str) {
        self.current.colors.text_secondary = color.to_string();
        self.dirty = true;
    }

    pub fn set_text_muted(&mut self, color: &str) {
        self.current.colors.text_muted = color.to_string();
        self.dirty = true;
    }

    pub fn set_border(&mut self, color: &str) {
        self.current.colors.border = color.to_string();
        self.dirty = true;
    }

    pub fn set_divider(&mut self, color: &str) {
        self.current.colors.divider = color.to_string();
        self.dirty = true;
    }

    pub fn set_toolbar_divider(&mut self, color: &str) {
        self.current.colors.toolbar_divider = color.to_string();
        self.dirty = true;
    }

    pub fn set_ui_border(&mut self, color: &str) {
        self.current.colors.ui_border = color.to_string();
        self.dirty = true;
    }

    pub fn set_accent(&mut self, color: &str) {
        self.current.colors.accent = color.to_string();
        self.dirty = true;
    }

    // =========================================================================
    // Individual setters - Chart Colors
    // =========================================================================

    pub fn set_chart_bg(&mut self, color: &str) {
        self.current.chart.background = color.to_string();
        self.dirty = true;
    }

    pub fn set_grid_color(&mut self, color: &str) {
        self.current.chart.grid_line = color.to_string();
        self.dirty = true;
    }

    pub fn set_scale_bg(&mut self, color: &str) {
        self.current.chart.scale_bg = color.to_string();
        self.dirty = true;
    }

    pub fn set_scale_border(&mut self, color: &str) {
        self.current.chart.scale_border = color.to_string();
        self.dirty = true;
    }

    pub fn set_scale_text_color(&mut self, color: &str) {
        self.current.chart.scale_text = color.to_string();
        self.dirty = true;
    }

    pub fn set_time_scale_bg(&mut self, color: &str) {
        self.current.chart.time_scale_bg = color.to_string();
        self.dirty = true;
    }

    pub fn set_time_scale_text_color(&mut self, color: &str) {
        self.current.chart.time_scale_text = color.to_string();
        self.dirty = true;
    }

    pub fn set_time_scale_text_medium(&mut self, color: &str) {
        self.current.chart.time_scale_text_medium = color.to_string();
        self.dirty = true;
    }

    pub fn set_time_scale_text_muted(&mut self, color: &str) {
        self.current.chart.time_scale_text_muted = color.to_string();
        self.dirty = true;
    }

    pub fn set_crosshair_color(&mut self, color: &str) {
        self.current.chart.crosshair_line = color.to_string();
        self.dirty = true;
    }

    pub fn set_crosshair_label_bg(&mut self, color: &str) {
        self.current.chart.crosshair_label_bg = color.to_string();
        self.dirty = true;
    }

    pub fn set_legend_text_color(&mut self, color: &str) {
        self.current.chart.legend_text = color.to_string();
        self.dirty = true;
    }

    pub fn set_sidebar_bg(&mut self, color: &str) {
        self.current.chart.sidebar_bg = color.to_string();
        self.dirty = true;
    }

    pub fn set_chart_border(&mut self, color: &str) {
        self.current.chart.chart_border = color.to_string();
        self.dirty = true;
    }

    pub fn set_frame_border(&mut self, color: &str) {
        self.current.chart.frame_border = color.to_string();
        self.dirty = true;
    }

    // =========================================================================
    // Individual setters - Series Colors
    // =========================================================================

    pub fn set_candle_up_color(&mut self, color: &str) {
        self.current.series.candle_up_body = color.to_string();
        self.current.series.candle_up_wick = color.to_string();
        self.dirty = true;
    }

    pub fn set_candle_down_color(&mut self, color: &str) {
        self.current.series.candle_down_body = color.to_string();
        self.current.series.candle_down_wick = color.to_string();
        self.dirty = true;
    }

    pub fn set_line_color(&mut self, color: &str) {
        self.current.series.line_color = color.to_string();
        self.dirty = true;
    }

    pub fn set_area_colors(&mut self, line: &str, top: &str, bottom: &str) {
        self.current.series.area_line = line.to_string();
        self.current.series.area_top = top.to_string();
        self.current.series.area_bottom = bottom.to_string();
        self.dirty = true;
    }

    pub fn set_histogram_colors(&mut self, positive: &str, negative: &str) {
        self.current.series.histogram_positive = positive.to_string();
        self.current.series.histogram_negative = negative.to_string();
        self.dirty = true;
    }

    pub fn set_ma_colors(&mut self, fast: &str, slow: &str, third: &str) {
        self.current.series.ma_fast = fast.to_string();
        self.current.series.ma_slow = slow.to_string();
        self.current.series.ma_third = third.to_string();
        self.dirty = true;
    }

    // =========================================================================
    // Individual setters - Fonts
    // =========================================================================

    pub fn set_font_family(&mut self, family: &str) {
        self.current.fonts.family = family.to_string();
        self.dirty = true;
    }

    pub fn set_font_family_mono(&mut self, family: &str) {
        self.current.fonts.family_mono = family.to_string();
        self.dirty = true;
    }

    pub fn set_font_family_chart(&mut self, family: &str) {
        self.current.fonts.family_chart = family.to_string();
        self.dirty = true;
    }

    pub fn set_price_scale_font_size(&mut self, min: f64, max: f64) {
        self.current.fonts.price_scale_size_min = min;
        self.current.fonts.price_scale_size_max = max;
        self.dirty = true;
    }

    pub fn set_time_scale_font_size(&mut self, size: f64) {
        self.current.fonts.time_scale_size = size;
        self.dirty = true;
    }

    pub fn set_legend_font_size(&mut self, size: f64) {
        self.current.fonts.legend_size = size;
        self.dirty = true;
    }

    // =========================================================================
    // Bulk operations
    // =========================================================================

    /// Set theme from JSON string
    pub fn set_from_json(&mut self, json: &str) -> bool {
        if let Some(theme) = RuntimeTheme::from_json(json) {
            self.current = theme;
            self.dirty = true;
            true
        } else {
            false
        }
    }

    /// Set theme from RuntimeTheme
    pub fn set_theme(&mut self, theme: RuntimeTheme) {
        self.current = theme;
        self.dirty = true;
    }

    // =========================================================================
    // Getters
    // =========================================================================

    /// Get current theme reference
    pub fn current(&self) -> &RuntimeTheme {
        &self.current
    }

    /// Get mutable current theme reference (for advanced use)
    pub fn current_mut(&mut self) -> &mut RuntimeTheme {
        self.dirty = true;
        &mut self.current
    }

    /// Check if theme has been modified since last clear_dirty()
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Clear dirty flag (call after applying theme changes)
    pub fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    // =========================================================================
    // JSON Serialization
    // =========================================================================

    /// Export current theme as JSON
    pub fn to_json(&self) -> String {
        self.current.to_json()
    }

    /// Export current theme as pretty JSON
    pub fn to_json_pretty(&self) -> String {
        self.current.to_json_pretty()
    }

    // =========================================================================
    // CSS Export (for web platforms)
    // =========================================================================

    /// Generate CSS variables for efficient web theming
    ///
    /// Usage in HTML:
    /// ```css
    /// .toolbar { background: var(--toolbar-bg); }
    /// .btn { background: var(--button-bg); color: var(--text-primary); }
    /// ```
    ///
    /// Then in JS: inject this CSS into a <style> tag
    pub fn to_css_variables(&self) -> String {
        let c = &self.current.colors;
        let ch = &self.current.chart;
        let s = &self.current.series;
        let f = &self.current.fonts;
        let sz = &self.current.sizing;
        let e = &self.current.effects;

        format!(r#":root {{
    /* UI Colors */
    --toolbar-bg: {toolbar_bg};
    --button-bg: {button_bg};
    --button-hover: {button_hover};
    --button-active: {button_active};
    --button-hover-stroke: {button_hover_stroke};
    --button-active-stroke: {button_active_stroke};
    --button-rounding: {button_rounding}px;
    --dropdown-bg: {dropdown_bg};
    --status-bar-bg: {status_bar_bg};
    --text-primary: {text_primary};
    --text-secondary: {text_secondary};
    --text-muted: {text_muted};
    --border: {border};
    --border-light: {border_light};
    --divider: {divider};
    --toolbar-divider: {toolbar_divider};
    --ui-border: {ui_border};
    --accent: {accent};
    --accent-hover: {accent_hover};
    --success: {success};
    --danger: {danger};
    --warning: {warning};

    /* Chart Colors */
    --chart-bg: {chart_bg};
    --grid-line: {grid_line};
    --scale-bg: {scale_bg};
    --scale-border: {scale_border};
    --scale-text: {scale_text};
    --scale-text-muted: {scale_text_muted};
    --time-scale-bg: {time_scale_bg};
    --time-scale-text: {time_scale_text};
    --time-scale-text-medium: {time_scale_text_medium};
    --time-scale-text-muted: {time_scale_text_muted};
    --crosshair-line: {crosshair_line};
    --crosshair-label-bg: {crosshair_label_bg};
    --crosshair-label-text: {crosshair_label_text};
    --legend-text: {legend_text};
    --legend-value-up: {legend_value_up};
    --legend-value-down: {legend_value_down};
    --sidebar-bg: {sidebar_bg};
    --sidebar-border: {sidebar_border};
    --chart-border: {chart_border};
    --frame-border: {frame_border};

    /* Series Colors */
    --candle-up: {candle_up};
    --candle-down: {candle_down};
    --line-color: {line_color};
    --histogram-positive: {histogram_positive};
    --histogram-negative: {histogram_negative};
    --ma-fast: {ma_fast};
    --ma-slow: {ma_slow};

    /* Fonts */
    --font-family: {font_family};
    --font-family-mono: {font_family_mono};
    --font-family-chart: {font_family_chart};
    --font-size-small: {font_size_small}px;
    --font-size-normal: {font_size_normal}px;
    --font-size-large: {font_size_large}px;

    /* Sizing */
    --toolbar-height: {toolbar_height}px;
    --button-height: {button_height}px;
    --border-radius: {border_radius}px;
    --icon-size: {icon_size}px;

    /* Effects */
    --transition: {transition};
    --shadow-dropdown: {shadow_dropdown};
    --shadow-floating: {shadow_floating};
}}"#,
            // UI Colors
            toolbar_bg = c.toolbar_bg,
            button_bg = c.button_bg,
            button_hover = c.button_bg_hover,
            button_active = c.button_bg_active,
            button_hover_stroke = c.button_hover_stroke,
            button_active_stroke = c.button_active_stroke,
            button_rounding = c.button_rounding,
            dropdown_bg = c.dropdown_bg,
            status_bar_bg = c.status_bar_bg,
            text_primary = c.text_primary,
            text_secondary = c.text_secondary,
            text_muted = c.text_muted,
            border = c.border,
            border_light = c.border_light,
            divider = c.divider,
            toolbar_divider = c.toolbar_divider,
            ui_border = c.ui_border,
            accent = c.accent,
            accent_hover = c.accent_hover,
            success = c.success,
            danger = c.danger,
            warning = c.warning,
            // Chart Colors
            chart_bg = ch.background,
            grid_line = ch.grid_line,
            scale_bg = ch.scale_bg,
            scale_border = ch.scale_border,
            scale_text = ch.scale_text,
            scale_text_muted = ch.scale_text_muted,
            time_scale_bg = ch.time_scale_bg,
            time_scale_text = ch.time_scale_text,
            time_scale_text_medium = ch.time_scale_text_medium,
            time_scale_text_muted = ch.time_scale_text_muted,
            crosshair_line = ch.crosshair_line,
            crosshair_label_bg = ch.crosshair_label_bg,
            crosshair_label_text = ch.crosshair_label_text,
            legend_text = ch.legend_text,
            legend_value_up = ch.legend_value_up,
            legend_value_down = ch.legend_value_down,
            sidebar_bg = ch.sidebar_bg,
            sidebar_border = ch.sidebar_border,
            chart_border = ch.chart_border,
            frame_border = ch.frame_border,
            // Series Colors
            candle_up = s.candle_up_body,
            candle_down = s.candle_down_body,
            line_color = s.line_color,
            histogram_positive = s.histogram_positive,
            histogram_negative = s.histogram_negative,
            ma_fast = s.ma_fast,
            ma_slow = s.ma_slow,
            // Fonts
            font_family = f.family,
            font_family_mono = f.family_mono,
            font_family_chart = f.family_chart,
            font_size_small = f.size_small,
            font_size_normal = f.size_normal,
            font_size_large = f.size_large,
            // Sizing
            toolbar_height = sz.top_toolbar_height,
            button_height = sz.button_height,
            border_radius = sz.border_radius,
            icon_size = sz.icon_size,
            // Effects
            transition = e.transition_duration,
            shadow_dropdown = e.shadow_dropdown,
            shadow_floating = e.shadow_floating,
        )
    }

    // =========================================================================
    // Color parsing helpers
    // =========================================================================

    /// Parse hex color to RGB tuple
    /// "transparent" returns black (0,0,0) - caller should check original string for transparency
    /// Supports both 6-char (#rrggbb) and 8-char (#rrggbbaa) hex formats
    pub fn parse_hex_color(hex: &str) -> Option<(u8, u8, u8)> {
        // Handle "transparent" keyword - return black, caller handles alpha
        if hex == "transparent" {
            return Some((0, 0, 0));
        }
        let hex = hex.trim_start_matches('#');
        // Support both 6-char and 8-char hex (ignore alpha for RGB)
        if hex.len() != 6 && hex.len() != 8 {
            return None;
        }
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        Some((r, g, b))
    }

    /// Parse hex color to RGBA tuple (handles rgba() format, "transparent" keyword)
    pub fn parse_color_rgba(color: &str) -> Option<(u8, u8, u8, u8)> {
        let color = color.trim();

        // Handle "transparent" keyword - fully transparent black
        if color == "transparent" {
            return Some((0, 0, 0, 0));
        }

        // Handle hex format
        if color.starts_with('#') {
            let (r, g, b) = Self::parse_hex_color(color)?;
            return Some((r, g, b, 255));
        }

        // Handle rgba() format
        if color.starts_with("rgba(") {
            let inner = color.trim_start_matches("rgba(").trim_end_matches(')');
            let parts: Vec<&str> = inner.split(',').map(|s| s.trim()).collect();
            if parts.len() == 4 {
                let r = parts[0].parse::<u8>().ok()?;
                let g = parts[1].parse::<u8>().ok()?;
                let b = parts[2].parse::<u8>().ok()?;
                let a = (parts[3].parse::<f32>().ok()? * 255.0) as u8;
                return Some((r, g, b, a));
            }
        }

        // Handle rgb() format
        if color.starts_with("rgb(") {
            let inner = color.trim_start_matches("rgb(").trim_end_matches(')');
            let parts: Vec<&str> = inner.split(',').map(|s| s.trim()).collect();
            if parts.len() == 3 {
                let r = parts[0].parse::<u8>().ok()?;
                let g = parts[1].parse::<u8>().ok()?;
                let b = parts[2].parse::<u8>().ok()?;
                return Some((r, g, b, 255));
            }
        }

        None
    }

    /// Check if color string represents transparent
    pub fn is_transparent(color: &str) -> bool {
        let color = color.trim();
        if color == "transparent" {
            return true;
        }
        // Check rgba with 0 alpha
        if let Some((_, _, _, a)) = Self::parse_color_rgba(color) {
            return a == 0;
        }
        false
    }
}

impl Default for ThemeManager {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// Theme Settings Panel Definition
// =============================================================================

/// A single color field in the theme settings panel
#[derive(Debug, Clone, Copy)]
pub struct ThemeColorField {
    pub label: &'static str,
    pub id: &'static str,
    pub path: ThemeColorPath,
}

/// Path to a color value in RuntimeTheme
#[derive(Debug, Clone, Copy)]
pub enum ThemeColorPath {
    // UI Colors
    ToolbarBg,
    ButtonHover,
    ButtonActive,
    ButtonHoverStroke,
    ButtonActiveStroke,
    Border,
    TextPrimary,
    TextSecondary,
    Divider,
    ToolbarDivider,
    UiBorder,
    DropdownBg,
    // Chart Colors
    ChartBg,
    Grid,
    Crosshair,
    ScaleBg,
    ScaleText,
    // Time Scale
    TimeScaleMajor,
    TimeScaleMedium,
    TimeScaleMinor,
    // Borders
    ChartBorder,
    FrameBorder,
    // Candle Colors
    CandleUpBody,
    CandleUpWick,
    CandleDownBody,
    CandleDownWick,
}

impl ThemeColorPath {
    /// Get the color value from RuntimeTheme
    pub fn get<'a>(&self, theme: &'a super::RuntimeTheme) -> &'a str {
        match self {
            Self::ToolbarBg => &theme.colors.toolbar_bg,
            Self::ButtonHover => &theme.colors.button_bg_hover,
            Self::ButtonActive => &theme.colors.button_bg_active,
            Self::ButtonHoverStroke => &theme.colors.button_hover_stroke,
            Self::ButtonActiveStroke => &theme.colors.button_active_stroke,
            Self::Border => &theme.colors.border,
            Self::TextPrimary => &theme.colors.text_primary,
            Self::TextSecondary => &theme.colors.text_secondary,
            Self::Divider => &theme.colors.divider,
            Self::ToolbarDivider => &theme.colors.toolbar_divider,
            Self::UiBorder => &theme.colors.ui_border,
            Self::DropdownBg => &theme.colors.dropdown_bg,
            Self::ChartBg => &theme.chart.background,
            Self::Grid => &theme.chart.grid_line,
            Self::Crosshair => &theme.chart.crosshair_line,
            Self::ScaleBg => &theme.chart.scale_bg,
            Self::ScaleText => &theme.chart.scale_text,
            Self::TimeScaleMajor => &theme.chart.time_scale_text,
            Self::TimeScaleMedium => &theme.chart.time_scale_text_medium,
            Self::TimeScaleMinor => &theme.chart.time_scale_text_muted,
            Self::ChartBorder => &theme.chart.chart_border,
            Self::FrameBorder => &theme.chart.frame_border,
            Self::CandleUpBody => &theme.series.candle_up_body,
            Self::CandleUpWick => &theme.series.candle_up_wick,
            Self::CandleDownBody => &theme.series.candle_down_body,
            Self::CandleDownWick => &theme.series.candle_down_wick,
        }
    }

    /// Set the color value in ThemeManager
    pub fn set(&self, manager: &mut ThemeManager, color: &str) {
        match self {
            Self::ToolbarBg => manager.set_toolbar_bg(color),
            Self::ButtonHover => manager.set_button_hover_bg(color),
            Self::ButtonActive => manager.set_button_active_bg(color),
            Self::ButtonHoverStroke => manager.set_button_hover_stroke(color),
            Self::ButtonActiveStroke => manager.set_button_active_stroke(color),
            Self::Border => manager.set_border(color),
            Self::TextPrimary => manager.set_text_primary(color),
            Self::TextSecondary => manager.set_text_secondary(color),
            Self::Divider => manager.set_divider(color),
            Self::ToolbarDivider => manager.set_toolbar_divider(color),
            Self::UiBorder => manager.set_ui_border(color),
            Self::DropdownBg => manager.set_dropdown_bg(color),
            Self::ChartBg => manager.set_chart_bg(color),
            Self::Grid => manager.set_grid_color(color),
            Self::Crosshair => manager.set_crosshair_color(color),
            Self::ScaleBg => manager.set_scale_bg(color),
            Self::ScaleText => manager.set_scale_text_color(color),
            Self::TimeScaleMajor => manager.set_time_scale_text_color(color),
            Self::TimeScaleMedium => manager.set_time_scale_text_medium(color),
            Self::TimeScaleMinor => manager.set_time_scale_text_muted(color),
            Self::ChartBorder => manager.set_chart_border(color),
            Self::FrameBorder => manager.set_frame_border(color),
            Self::CandleUpBody => { manager.current_mut().series.candle_up_body = color.to_string(); },
            Self::CandleUpWick => { manager.current_mut().series.candle_up_wick = color.to_string(); },
            Self::CandleDownBody => { manager.current_mut().series.candle_down_body = color.to_string(); },
            Self::CandleDownWick => { manager.current_mut().series.candle_down_wick = color.to_string(); },
        }
    }
}

/// A section of color fields in the theme settings panel
#[derive(Debug, Clone, Copy)]
pub struct ThemeSettingsSection {
    pub title: &'static str,
    pub fields: &'static [ThemeColorField],
}

/// Theme Settings Panel - single source of truth for all theme color fields
pub struct ThemeSettingsPanel;

impl ThemeSettingsPanel {
    /// All sections displayed in the theme settings panel
    pub const SECTIONS: &'static [ThemeSettingsSection] = &[
        ThemeSettingsSection {
            title: "UI Colors",
            fields: &[
                ThemeColorField { label: "Toolbar BG", id: "toolbar_bg", path: ThemeColorPath::ToolbarBg },
                ThemeColorField { label: "Button Hover", id: "button_hover", path: ThemeColorPath::ButtonHover },
                ThemeColorField { label: "Button Active", id: "button_active", path: ThemeColorPath::ButtonActive },
                ThemeColorField { label: "Hover Stroke", id: "button_hover_stroke", path: ThemeColorPath::ButtonHoverStroke },
                ThemeColorField { label: "Active Stroke", id: "button_active_stroke", path: ThemeColorPath::ButtonActiveStroke },
                ThemeColorField { label: "Menu Border", id: "border", path: ThemeColorPath::Border },
                ThemeColorField { label: "Text Primary", id: "text_primary", path: ThemeColorPath::TextPrimary },
                ThemeColorField { label: "Text Secondary", id: "text_secondary", path: ThemeColorPath::TextSecondary },
                ThemeColorField { label: "Menu Divider", id: "divider", path: ThemeColorPath::Divider },
                ThemeColorField { label: "Toolbar Divider", id: "toolbar_divider", path: ThemeColorPath::ToolbarDivider },
                ThemeColorField { label: "UI Border", id: "ui_border", path: ThemeColorPath::UiBorder },
                ThemeColorField { label: "Menu Dropdown BG", id: "dropdown_bg", path: ThemeColorPath::DropdownBg },
            ],
        },
        ThemeSettingsSection {
            title: "Chart Colors",
            fields: &[
                ThemeColorField { label: "Chart BG", id: "chart_bg", path: ThemeColorPath::ChartBg },
                ThemeColorField { label: "Grid", id: "grid", path: ThemeColorPath::Grid },
                ThemeColorField { label: "Crosshair", id: "crosshair", path: ThemeColorPath::Crosshair },
                ThemeColorField { label: "Scale BG", id: "scale_bg", path: ThemeColorPath::ScaleBg },
                ThemeColorField { label: "Scale Text", id: "scale_text", path: ThemeColorPath::ScaleText },
            ],
        },
        ThemeSettingsSection {
            title: "Time Scale",
            fields: &[
                ThemeColorField { label: "Major", id: "time_scale_major", path: ThemeColorPath::TimeScaleMajor },
                ThemeColorField { label: "Medium", id: "time_scale_medium", path: ThemeColorPath::TimeScaleMedium },
                ThemeColorField { label: "Minor", id: "time_scale_minor", path: ThemeColorPath::TimeScaleMinor },
            ],
        },
        ThemeSettingsSection {
            title: "Borders",
            fields: &[
                ThemeColorField { label: "Chart Border", id: "chart_border", path: ThemeColorPath::ChartBorder },
                ThemeColorField { label: "Frame Border", id: "frame_border", path: ThemeColorPath::FrameBorder },
            ],
        },
        ThemeSettingsSection {
            title: "Candle Colors",
            fields: &[
                ThemeColorField { label: "Up Body", id: "candle_up_body", path: ThemeColorPath::CandleUpBody },
                ThemeColorField { label: "Up Wick", id: "candle_up_wick", path: ThemeColorPath::CandleUpWick },
                ThemeColorField { label: "Down Body", id: "candle_down_body", path: ThemeColorPath::CandleDownBody },
                ThemeColorField { label: "Down Wick", id: "candle_down_wick", path: ThemeColorPath::CandleDownWick },
            ],
        },
    ];

    /// Get available theme presets
    pub fn presets() -> &'static [&'static str] {
        ThemeManager::presets()
    }

    /// Get color value by field ID
    pub fn get_color_by_id<'a>(theme: &'a super::RuntimeTheme, id: &str) -> Option<&'a str> {
        for section in Self::SECTIONS {
            for field in section.fields {
                if field.id == id {
                    return Some(field.path.get(theme));
                }
            }
        }
        None
    }

    /// Set color value by field ID
    pub fn set_color_by_id(manager: &mut ThemeManager, id: &str, color: &str) -> bool {
        for section in Self::SECTIONS {
            for field in section.fields {
                if field.id == id {
                    field.path.set(manager, color);
                    return true;
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preset_switching() {
        let mut manager = ThemeManager::new();
        assert_eq!(manager.current().name, "Dark");

        manager.set_preset("light");
        assert_eq!(manager.current().name, "Light");
        assert!(manager.is_dirty());

        manager.clear_dirty();
        assert!(!manager.is_dirty());
    }

    #[test]
    fn test_color_modification() {
        let mut manager = ThemeManager::new();
        manager.set_toolbar_bg("#ff0000");
        assert_eq!(manager.current().colors.toolbar_bg, "#ff0000");
        assert!(manager.is_dirty());
    }

    #[test]
    fn test_reset_to_preset() {
        let mut manager = ThemeManager::new();
        manager.set_toolbar_bg("#ff0000");
        manager.reset_to_preset();
        assert_ne!(manager.current().colors.toolbar_bg, "#ff0000");
    }

    #[test]
    fn test_css_variables() {
        let manager = ThemeManager::new();
        let css = manager.to_css_variables();
        assert!(css.contains("--toolbar-bg:"));
        assert!(css.contains("--candle-up:"));
        assert!(css.contains("--font-family:"));
    }

    #[test]
    fn test_color_parsing() {
        assert_eq!(ThemeManager::parse_hex_color("#ff0000"), Some((255, 0, 0)));
        assert_eq!(ThemeManager::parse_hex_color("#00ff00"), Some((0, 255, 0)));

        assert_eq!(
            ThemeManager::parse_color_rgba("rgba(255, 0, 0, 0.5)"),
            Some((255, 0, 0, 127))
        );
    }
}
