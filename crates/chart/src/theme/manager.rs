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

    pub fn set_accent_hover(&mut self, color: &str) {
        self.current.colors.accent_hover = color.to_string();
        self.dirty = true;
    }

    pub fn set_text_active(&mut self, color: &str) {
        self.current.colors.text_active = color.to_string();
        self.dirty = true;
    }

    pub fn set_border_light(&mut self, color: &str) {
        self.current.colors.border_light = color.to_string();
        self.dirty = true;
    }

    pub fn set_success(&mut self, color: &str) {
        self.current.colors.success = color.to_string();
        self.dirty = true;
    }

    pub fn set_danger(&mut self, color: &str) {
        self.current.colors.danger = color.to_string();
        self.dirty = true;
    }

    pub fn set_warning(&mut self, color: &str) {
        self.current.colors.warning = color.to_string();
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

    pub fn set_scale_text_muted(&mut self, color: &str) {
        self.current.chart.scale_text_muted = color.to_string();
        self.dirty = true;
    }

    pub fn set_time_scale_border(&mut self, color: &str) {
        self.current.chart.time_scale_border = color.to_string();
        self.dirty = true;
    }

    pub fn set_crosshair_label_text(&mut self, color: &str) {
        self.current.chart.crosshair_label_text = color.to_string();
        self.dirty = true;
    }

    pub fn set_legend_value_up(&mut self, color: &str) {
        self.current.chart.legend_value_up = color.to_string();
        self.dirty = true;
    }

    pub fn set_legend_value_down(&mut self, color: &str) {
        self.current.chart.legend_value_down = color.to_string();
        self.dirty = true;
    }

    pub fn set_watermark_text(&mut self, color: &str) {
        self.current.chart.watermark_text = color.to_string();
        self.dirty = true;
    }

    pub fn set_sidebar_border(&mut self, color: &str) {
        self.current.chart.sidebar_border = color.to_string();
        self.dirty = true;
    }

    pub fn set_sidebar_header_bg(&mut self, color: &str) {
        self.current.chart.sidebar_header_bg = color.to_string();
        self.dirty = true;
    }

    pub fn set_sidebar_text(&mut self, color: &str) {
        self.current.chart.sidebar_text = color.to_string();
        self.dirty = true;
    }

    pub fn set_grid_line_horz(&mut self, color: &str) {
        self.current.chart.grid_line_horz = Some(color.to_string());
        self.dirty = true;
    }

    pub fn set_grid_line_vert(&mut self, color: &str) {
        self.current.chart.grid_line_vert = Some(color.to_string());
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

    pub fn set_candle_up_border(&mut self, color: &str) {
        self.current.series.candle_up_border = Some(color.to_string());
        self.dirty = true;
    }

    pub fn set_candle_down_border(&mut self, color: &str) {
        self.current.series.candle_down_border = Some(color.to_string());
        self.dirty = true;
    }

    pub fn set_area_line(&mut self, color: &str) {
        self.current.series.area_line = color.to_string();
        self.dirty = true;
    }

    pub fn set_area_top(&mut self, color: &str) {
        self.current.series.area_top = color.to_string();
        self.dirty = true;
    }

    pub fn set_area_bottom(&mut self, color: &str) {
        self.current.series.area_bottom = color.to_string();
        self.dirty = true;
    }

    pub fn set_histogram_positive(&mut self, color: &str) {
        self.current.series.histogram_positive = color.to_string();
        self.dirty = true;
    }

    pub fn set_histogram_negative(&mut self, color: &str) {
        self.current.series.histogram_negative = color.to_string();
        self.dirty = true;
    }

    pub fn set_baseline_top_line(&mut self, color: &str) {
        self.current.series.baseline_top_line = color.to_string();
        self.dirty = true;
    }

    pub fn set_baseline_top_fill(&mut self, color: &str) {
        self.current.series.baseline_top_fill = color.to_string();
        self.dirty = true;
    }

    pub fn set_baseline_bottom_line(&mut self, color: &str) {
        self.current.series.baseline_bottom_line = color.to_string();
        self.dirty = true;
    }

    pub fn set_baseline_bottom_fill(&mut self, color: &str) {
        self.current.series.baseline_bottom_fill = color.to_string();
        self.dirty = true;
    }

    pub fn set_baseline_line(&mut self, color: &str) {
        self.current.series.baseline_line = color.to_string();
        self.dirty = true;
    }

    pub fn set_bar_up(&mut self, color: &str) {
        self.current.series.bar_up = color.to_string();
        self.dirty = true;
    }

    pub fn set_bar_down(&mut self, color: &str) {
        self.current.series.bar_down = color.to_string();
        self.dirty = true;
    }

    pub fn set_ma_fast(&mut self, color: &str) {
        self.current.series.ma_fast = color.to_string();
        self.dirty = true;
    }

    pub fn set_ma_slow(&mut self, color: &str) {
        self.current.series.ma_slow = color.to_string();
        self.dirty = true;
    }

    pub fn set_ma_third(&mut self, color: &str) {
        self.current.series.ma_third = color.to_string();
        self.dirty = true;
    }

    pub fn set_volume_up(&mut self, color: &str) {
        self.current.series.volume_up = color.to_string();
        self.dirty = true;
    }

    pub fn set_volume_down(&mut self, color: &str) {
        self.current.series.volume_down = color.to_string();
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
    ButtonBg,
    StatusBarBg,
    ButtonHover,
    ButtonActive,
    ButtonHoverStroke,
    ButtonActiveStroke,
    Border,
    BorderLight,
    TextPrimary,
    TextSecondary,
    TextMuted,
    TextActive,
    Divider,
    ToolbarDivider,
    UiBorder,
    DropdownBg,
    Accent,
    AccentHover,
    Success,
    Danger,
    Warning,
    // Chart Colors
    ChartBg,
    Grid,
    GridLineHorz,
    GridLineVert,
    Crosshair,
    ScaleBg,
    ScaleBorder,
    ScaleText,
    ScaleTextMuted,
    // Time Scale
    TimeScaleBg,
    TimeScaleBorder,
    TimeScaleMajor,
    TimeScaleMedium,
    TimeScaleMinor,
    // Crosshair
    CrosshairLabelBg,
    CrosshairLabelText,
    // Legend
    LegendText,
    LegendValueUp,
    LegendValueDown,
    // Watermark
    WatermarkText,
    // Sidebar
    SidebarBg,
    SidebarBorder,
    SidebarHeaderBg,
    SidebarText,
    // Borders
    ChartBorder,
    FrameBorder,
    // Candle Colors
    CandleUpBody,
    CandleUpWick,
    CandleDownBody,
    CandleDownWick,
    // Series Colors
    CandleUpBorder,
    CandleDownBorder,
    LineColor,
    AreaLine,
    AreaTop,
    AreaBottom,
    HistogramPositive,
    HistogramNegative,
    BaselineTopLine,
    BaselineTopFill,
    BaselineBottomLine,
    BaselineBottomFill,
    BaselineLine,
    BarUp,
    BarDown,
    MaFast,
    MaSlow,
    MaThird,
    VolumeUp,
    VolumeDown,
}

impl ThemeColorPath {
    /// Get the color value from RuntimeTheme
    pub fn get<'a>(&self, theme: &'a super::RuntimeTheme) -> &'a str {
        match self {
            Self::ToolbarBg => &theme.colors.toolbar_bg,
            Self::ButtonBg => &theme.colors.button_bg,
            Self::StatusBarBg => &theme.colors.status_bar_bg,
            Self::ButtonHover => &theme.colors.button_bg_hover,
            Self::ButtonActive => &theme.colors.button_bg_active,
            Self::ButtonHoverStroke => &theme.colors.button_hover_stroke,
            Self::ButtonActiveStroke => &theme.colors.button_active_stroke,
            Self::Border => &theme.colors.border,
            Self::BorderLight => &theme.colors.border_light,
            Self::TextPrimary => &theme.colors.text_primary,
            Self::TextSecondary => &theme.colors.text_secondary,
            Self::TextMuted => &theme.colors.text_muted,
            Self::TextActive => &theme.colors.text_active,
            Self::Divider => &theme.colors.divider,
            Self::ToolbarDivider => &theme.colors.toolbar_divider,
            Self::UiBorder => &theme.colors.ui_border,
            Self::DropdownBg => &theme.colors.dropdown_bg,
            Self::Accent => &theme.colors.accent,
            Self::AccentHover => &theme.colors.accent_hover,
            Self::Success => &theme.colors.success,
            Self::Danger => &theme.colors.danger,
            Self::Warning => &theme.colors.warning,
            Self::ChartBg => &theme.chart.background,
            Self::Grid => &theme.chart.grid_line,
            Self::GridLineHorz => theme.chart.grid_line_horz.as_deref().unwrap_or(&theme.chart.grid_line),
            Self::GridLineVert => theme.chart.grid_line_vert.as_deref().unwrap_or(&theme.chart.grid_line),
            Self::Crosshair => &theme.chart.crosshair_line,
            Self::ScaleBg => &theme.chart.scale_bg,
            Self::ScaleBorder => &theme.chart.scale_border,
            Self::ScaleText => &theme.chart.scale_text,
            Self::ScaleTextMuted => &theme.chart.scale_text_muted,
            Self::TimeScaleBg => &theme.chart.time_scale_bg,
            Self::TimeScaleBorder => &theme.chart.time_scale_border,
            Self::TimeScaleMajor => &theme.chart.time_scale_text,
            Self::TimeScaleMedium => &theme.chart.time_scale_text_medium,
            Self::TimeScaleMinor => &theme.chart.time_scale_text_muted,
            Self::CrosshairLabelBg => &theme.chart.crosshair_label_bg,
            Self::CrosshairLabelText => &theme.chart.crosshair_label_text,
            Self::LegendText => &theme.chart.legend_text,
            Self::LegendValueUp => &theme.chart.legend_value_up,
            Self::LegendValueDown => &theme.chart.legend_value_down,
            Self::WatermarkText => &theme.chart.watermark_text,
            Self::SidebarBg => &theme.chart.sidebar_bg,
            Self::SidebarBorder => &theme.chart.sidebar_border,
            Self::SidebarHeaderBg => &theme.chart.sidebar_header_bg,
            Self::SidebarText => &theme.chart.sidebar_text,
            Self::ChartBorder => &theme.chart.chart_border,
            Self::FrameBorder => &theme.chart.frame_border,
            Self::CandleUpBody => &theme.series.candle_up_body,
            Self::CandleUpWick => &theme.series.candle_up_wick,
            Self::CandleDownBody => &theme.series.candle_down_body,
            Self::CandleDownWick => &theme.series.candle_down_wick,
            Self::CandleUpBorder => theme.series.candle_up_border.as_deref().unwrap_or(""),
            Self::CandleDownBorder => theme.series.candle_down_border.as_deref().unwrap_or(""),
            Self::LineColor => &theme.series.line_color,
            Self::AreaLine => &theme.series.area_line,
            Self::AreaTop => &theme.series.area_top,
            Self::AreaBottom => &theme.series.area_bottom,
            Self::HistogramPositive => &theme.series.histogram_positive,
            Self::HistogramNegative => &theme.series.histogram_negative,
            Self::BaselineTopLine => &theme.series.baseline_top_line,
            Self::BaselineTopFill => &theme.series.baseline_top_fill,
            Self::BaselineBottomLine => &theme.series.baseline_bottom_line,
            Self::BaselineBottomFill => &theme.series.baseline_bottom_fill,
            Self::BaselineLine => &theme.series.baseline_line,
            Self::BarUp => &theme.series.bar_up,
            Self::BarDown => &theme.series.bar_down,
            Self::MaFast => &theme.series.ma_fast,
            Self::MaSlow => &theme.series.ma_slow,
            Self::MaThird => &theme.series.ma_third,
            Self::VolumeUp => &theme.series.volume_up,
            Self::VolumeDown => &theme.series.volume_down,
        }
    }

    /// Set the color value in ThemeManager
    pub fn set(&self, manager: &mut ThemeManager, color: &str) {
        match self {
            Self::ToolbarBg => manager.set_toolbar_bg(color),
            Self::ButtonBg => manager.set_button_bg(color),
            Self::StatusBarBg => manager.set_status_bar_bg(color),
            Self::ButtonHover => manager.set_button_hover_bg(color),
            Self::ButtonActive => manager.set_button_active_bg(color),
            Self::ButtonHoverStroke => manager.set_button_hover_stroke(color),
            Self::ButtonActiveStroke => manager.set_button_active_stroke(color),
            Self::Border => manager.set_border(color),
            Self::BorderLight => manager.set_border_light(color),
            Self::TextPrimary => manager.set_text_primary(color),
            Self::TextSecondary => manager.set_text_secondary(color),
            Self::TextMuted => manager.set_text_muted(color),
            Self::TextActive => manager.set_text_active(color),
            Self::Divider => manager.set_divider(color),
            Self::ToolbarDivider => manager.set_toolbar_divider(color),
            Self::UiBorder => manager.set_ui_border(color),
            Self::DropdownBg => manager.set_dropdown_bg(color),
            Self::Accent => manager.set_accent(color),
            Self::AccentHover => manager.set_accent_hover(color),
            Self::Success => manager.set_success(color),
            Self::Danger => manager.set_danger(color),
            Self::Warning => manager.set_warning(color),
            Self::ChartBg => manager.set_chart_bg(color),
            Self::Grid => manager.set_grid_color(color),
            Self::GridLineHorz => manager.set_grid_line_horz(color),
            Self::GridLineVert => manager.set_grid_line_vert(color),
            Self::Crosshair => manager.set_crosshair_color(color),
            Self::ScaleBg => manager.set_scale_bg(color),
            Self::ScaleBorder => manager.set_scale_border(color),
            Self::ScaleText => manager.set_scale_text_color(color),
            Self::ScaleTextMuted => manager.set_scale_text_muted(color),
            Self::TimeScaleBg => manager.set_time_scale_bg(color),
            Self::TimeScaleBorder => manager.set_time_scale_border(color),
            Self::TimeScaleMajor => manager.set_time_scale_text_color(color),
            Self::TimeScaleMedium => manager.set_time_scale_text_medium(color),
            Self::TimeScaleMinor => manager.set_time_scale_text_muted(color),
            Self::CrosshairLabelBg => manager.set_crosshair_label_bg(color),
            Self::CrosshairLabelText => manager.set_crosshair_label_text(color),
            Self::LegendText => manager.set_legend_text_color(color),
            Self::LegendValueUp => manager.set_legend_value_up(color),
            Self::LegendValueDown => manager.set_legend_value_down(color),
            Self::WatermarkText => manager.set_watermark_text(color),
            Self::SidebarBg => manager.set_sidebar_bg(color),
            Self::SidebarBorder => manager.set_sidebar_border(color),
            Self::SidebarHeaderBg => manager.set_sidebar_header_bg(color),
            Self::SidebarText => manager.set_sidebar_text(color),
            Self::ChartBorder => manager.set_chart_border(color),
            Self::FrameBorder => manager.set_frame_border(color),
            Self::CandleUpBody => { manager.current_mut().series.candle_up_body = color.to_string(); },
            Self::CandleUpWick => { manager.current_mut().series.candle_up_wick = color.to_string(); },
            Self::CandleDownBody => { manager.current_mut().series.candle_down_body = color.to_string(); },
            Self::CandleDownWick => { manager.current_mut().series.candle_down_wick = color.to_string(); },
            Self::CandleUpBorder => manager.set_candle_up_border(color),
            Self::CandleDownBorder => manager.set_candle_down_border(color),
            Self::LineColor => manager.set_line_color(color),
            Self::AreaLine => manager.set_area_line(color),
            Self::AreaTop => manager.set_area_top(color),
            Self::AreaBottom => manager.set_area_bottom(color),
            Self::HistogramPositive => manager.set_histogram_positive(color),
            Self::HistogramNegative => manager.set_histogram_negative(color),
            Self::BaselineTopLine => manager.set_baseline_top_line(color),
            Self::BaselineTopFill => manager.set_baseline_top_fill(color),
            Self::BaselineBottomLine => manager.set_baseline_bottom_line(color),
            Self::BaselineBottomFill => manager.set_baseline_bottom_fill(color),
            Self::BaselineLine => manager.set_baseline_line(color),
            Self::BarUp => manager.set_bar_up(color),
            Self::BarDown => manager.set_bar_down(color),
            Self::MaFast => manager.set_ma_fast(color),
            Self::MaSlow => manager.set_ma_slow(color),
            Self::MaThird => manager.set_ma_third(color),
            Self::VolumeUp => manager.set_volume_up(color),
            Self::VolumeDown => manager.set_volume_down(color),
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
                ThemeColorField { label: "Button BG", id: "button_bg", path: ThemeColorPath::ButtonBg },
                ThemeColorField { label: "Status Bar BG", id: "status_bar_bg", path: ThemeColorPath::StatusBarBg },
                ThemeColorField { label: "Button Hover", id: "button_hover", path: ThemeColorPath::ButtonHover },
                ThemeColorField { label: "Button Active", id: "button_active", path: ThemeColorPath::ButtonActive },
                ThemeColorField { label: "Hover Stroke", id: "button_hover_stroke", path: ThemeColorPath::ButtonHoverStroke },
                ThemeColorField { label: "Active Stroke", id: "button_active_stroke", path: ThemeColorPath::ButtonActiveStroke },
                ThemeColorField { label: "Menu Border", id: "border", path: ThemeColorPath::Border },
                ThemeColorField { label: "Border Light", id: "border_light", path: ThemeColorPath::BorderLight },
                ThemeColorField { label: "Text Primary", id: "text_primary", path: ThemeColorPath::TextPrimary },
                ThemeColorField { label: "Text Secondary", id: "text_secondary", path: ThemeColorPath::TextSecondary },
                ThemeColorField { label: "Text Muted", id: "text_muted", path: ThemeColorPath::TextMuted },
                ThemeColorField { label: "Text Active", id: "text_active", path: ThemeColorPath::TextActive },
                ThemeColorField { label: "Menu Divider", id: "divider", path: ThemeColorPath::Divider },
                ThemeColorField { label: "Toolbar Divider", id: "toolbar_divider", path: ThemeColorPath::ToolbarDivider },
                ThemeColorField { label: "UI Border", id: "ui_border", path: ThemeColorPath::UiBorder },
                ThemeColorField { label: "Menu Dropdown BG", id: "dropdown_bg", path: ThemeColorPath::DropdownBg },
                ThemeColorField { label: "Accent", id: "accent", path: ThemeColorPath::Accent },
                ThemeColorField { label: "Accent Hover", id: "accent_hover", path: ThemeColorPath::AccentHover },
                ThemeColorField { label: "Success", id: "success", path: ThemeColorPath::Success },
                ThemeColorField { label: "Danger", id: "danger", path: ThemeColorPath::Danger },
                ThemeColorField { label: "Warning", id: "warning", path: ThemeColorPath::Warning },
            ],
        },
        ThemeSettingsSection {
            title: "Chart Colors",
            fields: &[
                ThemeColorField { label: "Chart BG", id: "chart_bg", path: ThemeColorPath::ChartBg },
                ThemeColorField { label: "Grid", id: "grid", path: ThemeColorPath::Grid },
                ThemeColorField { label: "Grid Line Horz", id: "grid_line_horz", path: ThemeColorPath::GridLineHorz },
                ThemeColorField { label: "Grid Line Vert", id: "grid_line_vert", path: ThemeColorPath::GridLineVert },
                ThemeColorField { label: "Crosshair", id: "crosshair", path: ThemeColorPath::Crosshair },
                ThemeColorField { label: "Scale BG", id: "scale_bg", path: ThemeColorPath::ScaleBg },
                ThemeColorField { label: "Scale Border", id: "scale_border", path: ThemeColorPath::ScaleBorder },
                ThemeColorField { label: "Scale Text", id: "scale_text", path: ThemeColorPath::ScaleText },
                ThemeColorField { label: "Scale Text Muted", id: "scale_text_muted", path: ThemeColorPath::ScaleTextMuted },
                ThemeColorField { label: "Time Scale BG", id: "time_scale_bg", path: ThemeColorPath::TimeScaleBg },
                ThemeColorField { label: "Time Scale Border", id: "time_scale_border", path: ThemeColorPath::TimeScaleBorder },
                ThemeColorField { label: "Crosshair Label BG", id: "crosshair_label_bg", path: ThemeColorPath::CrosshairLabelBg },
                ThemeColorField { label: "Crosshair Label Text", id: "crosshair_label_text", path: ThemeColorPath::CrosshairLabelText },
                ThemeColorField { label: "Legend Text", id: "legend_text", path: ThemeColorPath::LegendText },
                ThemeColorField { label: "Legend Value Up", id: "legend_value_up", path: ThemeColorPath::LegendValueUp },
                ThemeColorField { label: "Legend Value Down", id: "legend_value_down", path: ThemeColorPath::LegendValueDown },
                ThemeColorField { label: "Watermark Text", id: "watermark_text", path: ThemeColorPath::WatermarkText },
                ThemeColorField { label: "Sidebar BG", id: "sidebar_bg", path: ThemeColorPath::SidebarBg },
                ThemeColorField { label: "Sidebar Border", id: "sidebar_border", path: ThemeColorPath::SidebarBorder },
                ThemeColorField { label: "Sidebar Header BG", id: "sidebar_header_bg", path: ThemeColorPath::SidebarHeaderBg },
                ThemeColorField { label: "Sidebar Text", id: "sidebar_text", path: ThemeColorPath::SidebarText },
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
        ThemeSettingsSection {
            title: "Series",
            fields: &[
                ThemeColorField { label: "Candle Up Border", id: "candle_up_border", path: ThemeColorPath::CandleUpBorder },
                ThemeColorField { label: "Candle Down Border", id: "candle_down_border", path: ThemeColorPath::CandleDownBorder },
                ThemeColorField { label: "Line Color", id: "line_color", path: ThemeColorPath::LineColor },
                ThemeColorField { label: "Area Line", id: "area_line", path: ThemeColorPath::AreaLine },
                ThemeColorField { label: "Area Top", id: "area_top", path: ThemeColorPath::AreaTop },
                ThemeColorField { label: "Area Bottom", id: "area_bottom", path: ThemeColorPath::AreaBottom },
                ThemeColorField { label: "Histogram +", id: "histogram_positive", path: ThemeColorPath::HistogramPositive },
                ThemeColorField { label: "Histogram -", id: "histogram_negative", path: ThemeColorPath::HistogramNegative },
                ThemeColorField { label: "Baseline Top Line", id: "baseline_top_line", path: ThemeColorPath::BaselineTopLine },
                ThemeColorField { label: "Baseline Top Fill", id: "baseline_top_fill", path: ThemeColorPath::BaselineTopFill },
                ThemeColorField { label: "Baseline Bottom Line", id: "baseline_bottom_line", path: ThemeColorPath::BaselineBottomLine },
                ThemeColorField { label: "Baseline Bottom Fill", id: "baseline_bottom_fill", path: ThemeColorPath::BaselineBottomFill },
                ThemeColorField { label: "Baseline", id: "baseline_line", path: ThemeColorPath::BaselineLine },
                ThemeColorField { label: "Bar Up", id: "bar_up", path: ThemeColorPath::BarUp },
                ThemeColorField { label: "Bar Down", id: "bar_down", path: ThemeColorPath::BarDown },
                ThemeColorField { label: "MA Fast", id: "ma_fast", path: ThemeColorPath::MaFast },
                ThemeColorField { label: "MA Slow", id: "ma_slow", path: ThemeColorPath::MaSlow },
                ThemeColorField { label: "MA Third", id: "ma_third", path: ThemeColorPath::MaThird },
                ThemeColorField { label: "Volume Up", id: "volume_up", path: ThemeColorPath::VolumeUp },
                ThemeColorField { label: "Volume Down", id: "volume_down", path: ThemeColorPath::VolumeDown },
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
