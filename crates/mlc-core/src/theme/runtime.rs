//! Runtime Theme - Dynamic theme configuration with owned String values
//!
//! Unlike UITheme which uses &'static str for compile-time themes,
//! RuntimeTheme uses owned Strings allowing runtime modifications.
//!
//! # Usage
//! ```ignore
//! let mut theme = RuntimeTheme::from_preset("dark");
//! theme.colors.toolbar_bg = "#ff0000".to_string();  // Custom color
//! ```

use serde::{Deserialize, Serialize};
use super::UITheme;
use super::style::{UIStyle, StyleParams};

/// Runtime-modifiable theme with owned String values
/// This is the primary type for storing the current active theme
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeTheme {
    pub name: String,
    pub colors: RuntimeUIColors,
    pub chart: RuntimeChartColors,
    pub series: RuntimeSeriesColors,
    pub fonts: RuntimeFonts,
    pub sizing: RuntimeSizing,
    pub effects: RuntimeEffects,
    /// UI style (Solid, Glass, FrostedGlass, LiquidGlass) - orthogonal to colors
    #[serde(default)]
    pub style: UIStyle,
    /// Style parameters (opacity, blur, effects)
    #[serde(default)]
    pub style_params: StyleParams,
    /// Trading panel colors — panel-specific overrides not present in base colors
    #[serde(default)]
    pub trading: RuntimeTradingColors,
    /// Skin texture/draw-form layer (bar texture, candle style, glow,
    /// zebra) — set by skin presets, read by panel/chart renderers.
    #[serde(default)]
    pub panel_style: super::skin::PanelStyleParams,
}

/// UI element colors (toolbars, buttons, dropdowns, etc.)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeUIColors {
    // Backgrounds
    pub toolbar_bg: String,
    pub button_bg: String,
    pub button_bg_hover: String,
    pub button_bg_active: String,
    pub dropdown_bg: String,
    pub button_hover_stroke: String,
    pub button_active_stroke: String,
    pub button_rounding: f32,
    pub status_bar_bg: String,

    // Text
    pub text_primary: String,
    pub text_secondary: String,
    pub text_muted: String,
    pub text_active: String,

    // Borders
    pub border: String,
    pub border_light: String,
    pub divider: String,
    pub toolbar_divider: String,
    pub ui_border: String,

    // Accents
    pub accent: String,
    pub accent_hover: String,
    pub success: String,
    pub danger: String,
    pub warning: String,
    // Agent panel
    pub text_selection: String,
    pub bubble_user_bg: String,
}

/// Chart-specific colors (background, grid, scales, crosshair)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeChartColors {
    // Background
    pub background: String,

    // Grid
    pub grid_line: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grid_line_horz: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grid_line_vert: Option<String>,

    // Price scale (right axis)
    pub scale_bg: String,
    pub scale_border: String,
    pub scale_text: String,
    pub scale_text_muted: String,

    // Time scale (bottom axis)
    pub time_scale_bg: String,
    pub time_scale_border: String,
    pub time_scale_text: String,        // Major ticks (Year, Month)
    pub time_scale_text_medium: String, // Medium ticks (Day, Week)
    pub time_scale_text_muted: String,  // Minor ticks (Hour, etc.)

    // Crosshair
    pub crosshair_line: String,
    pub crosshair_label_bg: String,
    pub crosshair_label_text: String,

    // Legend (OHLC display)
    pub legend_text: String,
    pub legend_value_up: String,
    pub legend_value_down: String,

    // Watermark
    pub watermark_text: String,

    // Sidebar panels
    pub sidebar_bg: String,
    pub sidebar_border: String,
    pub sidebar_header_bg: String,
    pub sidebar_text: String,

    // Chart frame borders
    pub chart_border: String,  // Border around the chart area (all 4 sides)
    pub frame_border: String,  // Outer frame border (right of price scale, bottom of time scale)
}

/// Series/data visualization colors
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeSeriesColors {
    // Candlestick
    pub candle_up_body: String,
    pub candle_up_wick: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candle_up_border: Option<String>,
    pub candle_down_body: String,
    pub candle_down_wick: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candle_down_border: Option<String>,

    // Line series
    pub line_color: String,
    pub line_width: f64,

    // Step line (staircase) / Line-with-markers per-type colors (owner-
    // approved settings matrix wave 3, 2026-07-11). Additive — old theme
    // JSON without these keys falls back to `line_color`'s own default
    // (same green the line-family renderers always hardcoded via
    // `candle_up` before this field existed), so existing saved themes
    // render pixel-identically to before this field existed. Widths/
    // marker-style/marker-size are non-color per-type options, stored in
    // `LineFamilyDisplayOptions` (per-panel), not here — mirrors how wave 2
    // kept numeric params out of theme.
    #[serde(default = "default_footprint_buy")]
    pub step_line_color: String,
    #[serde(default = "default_footprint_buy")]
    pub line_markers_color: String,

    // Area series
    pub area_line: String,
    pub area_top: String,
    pub area_bottom: String,

    // Histogram
    pub histogram_positive: String,
    pub histogram_negative: String,

    // Baseline
    pub baseline_top_line: String,
    pub baseline_top_fill: String,
    pub baseline_bottom_line: String,
    pub baseline_bottom_fill: String,
    pub baseline_line: String,

    // Bar series (OHLC bars)
    pub bar_up: String,
    pub bar_down: String,

    // Moving averages
    pub ma_fast: String,
    pub ma_slow: String,
    pub ma_third: String,

    // Volume
    pub volume_up: String,
    pub volume_down: String,

    // Footprint clusters (buy/sell volume text + heat fill). Additive
    // 2026-07-05 — old theme JSON without these keys falls back to the
    // same green/red the footprint renderer always hardcoded, so existing
    // saved themes render pixel-identically to before this field existed.
    #[serde(default = "default_footprint_buy")]
    pub footprint_buy: String,
    #[serde(default = "default_footprint_sell")]
    pub footprint_sell: String,

    // Per-type color identity (owner-approved wave 2, 2026-07-11). Additive —
    // old theme JSON without these keys falls back to the same green/red
    // every one of these renderers hardcoded via `candle_up`/`candle_down`
    // before this field existed, so existing saved themes render
    // pixel-identically to before this field existed.
    #[serde(default = "default_footprint_buy")]
    pub renko_up: String,
    #[serde(default = "default_footprint_sell")]
    pub renko_down: String,
    #[serde(default = "default_footprint_buy")]
    pub pnf_x: String,
    #[serde(default = "default_footprint_sell")]
    pub pnf_o: String,
    #[serde(default = "default_footprint_buy")]
    pub tlb_up: String,
    #[serde(default = "default_footprint_sell")]
    pub tlb_down: String,
    #[serde(default = "default_footprint_buy")]
    pub kagi_yang: String,
    #[serde(default = "default_footprint_sell")]
    pub kagi_yin: String,
    #[serde(default = "default_footprint_buy")]
    pub hollow_up: String,
    #[serde(default = "default_footprint_sell")]
    pub hollow_down: String,
}

fn default_rc_input_border() -> String {
    "#30363dff".to_string()
}

fn default_footprint_buy() -> String {
    "#26a69a".to_string()
}

fn default_footprint_sell() -> String {
    "#ef5350".to_string()
}

/// Font settings
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeFonts {
    // Font families
    pub family: String,
    pub family_mono: String,
    pub family_chart: String,

    // Base sizes
    pub size_small: f64,
    pub size_normal: f64,
    pub size_large: f64,

    // Weights
    pub weight_light: u16,
    pub weight_normal: u16,
    pub weight_medium: u16,
    pub weight_bold: u16,

    // Scale-specific settings
    pub price_scale_size_min: f64,
    pub price_scale_size_max: f64,
    pub price_scale_weight: u16,

    pub time_scale_size: f64,
    pub time_scale_weight: u16,

    // Legend
    pub legend_size: f64,
    pub legend_weight: u16,

    // Crosshair labels
    pub crosshair_label_size: f64,
    pub crosshair_label_weight: u16,

    // Watermark
    pub watermark_size: f64,
    pub watermark_weight: u16,

    // Status bar
    pub status_bar_size: f64,
    pub status_bar_weight: u16,
}

/// Sizing settings
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeSizing {
    // Toolbar dimensions
    pub top_toolbar_height: f32,
    pub left_toolbar_width: f32,
    pub right_toolbar_width: f32,
    pub bottom_toolbar_height: f32,

    // Button sizing
    pub button_height: f32,
    pub button_padding_x: f32,
    pub button_padding_y: f32,

    // Icons
    pub icon_size: f32,

    // Other
    pub border_radius: f32,
    pub dropdown_min_width: f32,
}

/// Visual effects
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeEffects {
    pub transition_duration: String,
    pub shadow_dropdown: String,
    pub shadow_floating: String,
    pub hover_scale: f64,
}

/// Trading panel colors (DOM, Footprint, Order Entry, etc.)
///
/// Fields that are not present in serialized themes fall back to `Default`
/// via `#[serde(default)]` on the `RuntimeTheme.trading` field.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeTradingColors {
    // DOM
    pub dom_spread_bg: String,
    pub dom_best_bid_bg: String,
    pub dom_best_ask_bg: String,
    pub dom_user_order: String,

    // Footprint
    pub fp_cell_text: String,
    pub fp_poc_marker: String,
    pub fp_poc_border: String,
    pub fp_bullish: String,

    // Volume Profile
    pub vp_bar: String,
    pub vp_bar_poc: String,
    pub vp_poc_line: String,
    pub vp_vah_line: String,
    pub vp_val_line: String,
    pub vp_value_area: String,

    // Liquidity Heatmap
    pub heatmap_price_line: String,

    // Order Entry
    pub oe_tab_active: String,
    pub oe_tab_inactive: String,
    pub oe_input_bg: String,
    pub oe_input_border: String,
    pub oe_buy_button: String,
    pub oe_sell_button: String,
    pub oe_buy_button_text: String,
    pub oe_sell_button_text: String,

    // Position Manager
    pub pm_pnl_positive: String,
    pub pm_pnl_negative: String,
    pub pm_pnl_neutral: String,
    pub pm_long: String,
    pub pm_short: String,
    pub pm_liquidation: String,
    pub pm_summary_bg: String,

    // Trade Log
    pub tl_row_bg_alt: String,
    pub tl_profit: String,
    pub tl_loss: String,

    // Risk Calculator
    pub rc_risk: String,
    pub rc_profit: String,
    pub rc_good_rr: String,
    pub rc_input_bg: String,
    #[serde(default = "default_rc_input_border")]
    pub rc_input_border: String,

    // Trading Container
    pub tc_bg: String,
    pub tc_inner_bg: String,
    pub tc_separator: String,

    // Common panel colors
    pub panel_bg: String,
    pub row_bg_alt: String,
    pub header_bg: String,
    pub hover: String,
    pub selected: String,
    pub buy_bright: String,
    pub sell_bright: String,
}

impl Default for RuntimeTradingColors {
    fn default() -> Self {
        Self {
            // DOM
            dom_spread_bg:    "#14141eff".to_string(),
            dom_best_bid_bg:  "#0a3520ff".to_string(),
            dom_best_ask_bg:  "#3a100aff".to_string(),
            dom_user_order:   "#58a6ffff".to_string(),

            // Footprint
            fp_cell_text:  "#e0e0e0ff".to_string(),
            fp_poc_marker: "#ffde00ff".to_string(),
            fp_poc_border: "#b8860bff".to_string(),
            fp_bullish:    "#2ea043ff".to_string(),

            // Volume Profile
            vp_bar:        "#6699cc80".to_string(),
            vp_bar_poc:    "#88bbffff".to_string(),
            vp_poc_line:   "#ffde00ff".to_string(),
            vp_vah_line:   "#da363380".to_string(),
            vp_val_line:   "#2ea04380".to_string(),
            vp_value_area: "#58a6ff20".to_string(),

            // Liquidity Heatmap
            heatmap_price_line: "#ffde00ff".to_string(),

            // Order Entry
            oe_tab_active:      "#58a6ffff".to_string(),
            oe_tab_inactive:    "#21262dff".to_string(),
            oe_input_bg:        "#0d1117ff".to_string(),
            oe_input_border:    "#30363dff".to_string(),
            oe_buy_button:      "#2ea043ff".to_string(),
            oe_sell_button:     "#cc2233ff".to_string(),
            oe_buy_button_text: "#ffffffff".to_string(),
            oe_sell_button_text:"#ffffffff".to_string(),

            // Position Manager
            pm_pnl_positive: "#3fb950ff".to_string(),
            pm_pnl_negative: "#f85149ff".to_string(),
            pm_pnl_neutral:  "#8b949eff".to_string(),
            pm_long:         "#2ea043ff".to_string(),
            pm_short:        "#cc2233ff".to_string(),
            pm_liquidation:  "#f0883eff".to_string(),
            pm_summary_bg:   "#161b22ff".to_string(),

            // Trade Log
            tl_row_bg_alt: "#161b22ff".to_string(),
            tl_profit:     "#3fb950ff".to_string(),
            tl_loss:       "#f85149ff".to_string(),

            // Risk Calculator
            rc_risk:     "#cc2233ff".to_string(),
            rc_profit:   "#2ea043ff".to_string(),
            rc_good_rr:  "#ffde00ff".to_string(),
            rc_input_bg: "#0d1117ff".to_string(),
            rc_input_border: default_rc_input_border(),

            // Trading Container
            tc_bg:        "#0a0a0fff".to_string(),
            tc_inner_bg:  "#1c1c29ff".to_string(),
            tc_separator: "#33333fff".to_string(),

            // Common panel colors
            panel_bg:   "#0d1117ff".to_string(),
            row_bg_alt: "#10151bff".to_string(),
            header_bg:  "#161b22ff".to_string(),
            hover:      "#2a2f40ff".to_string(),
            selected:   "#1e2538ff".to_string(),
            buy_bright: "#00ff87ff".to_string(),
            sell_bright:"#ff4466ff".to_string(),
        }
    }
}

// =============================================================================
// Conversions from UITheme (static) to RuntimeTheme (dynamic)
// =============================================================================

impl From<&UITheme> for RuntimeTheme {
    fn from(theme: &UITheme) -> Self {
        Self {
            name: theme.name.to_string(),
            colors: RuntimeUIColors {
                toolbar_bg: theme.colors.toolbar_bg.to_string(),
                button_bg: theme.colors.button_bg.to_string(),
                button_bg_hover: theme.colors.button_bg_hover.to_string(),
                button_bg_active: theme.colors.button_bg_active.to_string(),
                button_hover_stroke: theme.colors.button_hover_stroke.to_string(),
                button_active_stroke: theme.colors.button_active_stroke.to_string(),
                button_rounding: theme.colors.button_rounding,
                dropdown_bg: theme.colors.dropdown_bg.to_string(),
                status_bar_bg: theme.colors.status_bar_bg.to_string(),
                text_primary: theme.colors.text_primary.to_string(),
                text_secondary: theme.colors.text_secondary.to_string(),
                text_muted: theme.colors.text_muted.to_string(),
                text_active: theme.colors.text_active.to_string(),
                border: theme.colors.border.to_string(),
                border_light: theme.colors.border_light.to_string(),
                divider: theme.colors.divider.to_string(),
                toolbar_divider: theme.colors.toolbar_divider.to_string(),
                ui_border: theme.colors.ui_border.to_string(),
                accent: theme.colors.accent.to_string(),
                accent_hover: theme.colors.accent_hover.to_string(),
                success: theme.colors.success.to_string(),
                danger: theme.colors.danger.to_string(),
                warning: theme.colors.warning.to_string(),
                text_selection: theme.colors.text_selection.to_string(),
                bubble_user_bg: theme.colors.bubble_user_bg.to_string(),
            },
            chart: RuntimeChartColors {
                background: theme.chart.background.to_string(),
                grid_line: theme.chart.grid_line.to_string(),
                grid_line_horz: theme.chart.grid_line_horz.map(|s| s.to_string()),
                grid_line_vert: theme.chart.grid_line_vert.map(|s| s.to_string()),
                scale_bg: theme.chart.scale_bg.to_string(),
                scale_border: theme.chart.scale_border.to_string(),
                scale_text: theme.chart.scale_text.to_string(),
                scale_text_muted: theme.chart.scale_text_muted.to_string(),
                time_scale_bg: theme.chart.time_scale_bg.to_string(),
                time_scale_border: theme.chart.time_scale_border.to_string(),
                time_scale_text: theme.chart.time_scale_text.to_string(),
                time_scale_text_medium: theme.chart.time_scale_text_medium.to_string(),
                time_scale_text_muted: theme.chart.time_scale_text_muted.to_string(),
                crosshair_line: theme.chart.crosshair_line.to_string(),
                crosshair_label_bg: theme.chart.crosshair_label_bg.to_string(),
                crosshair_label_text: theme.chart.crosshair_label_text.to_string(),
                legend_text: theme.chart.legend_text.to_string(),
                legend_value_up: theme.chart.legend_value_up.to_string(),
                legend_value_down: theme.chart.legend_value_down.to_string(),
                watermark_text: theme.chart.watermark_text.to_string(),
                sidebar_bg: theme.chart.sidebar_bg.to_string(),
                sidebar_border: theme.chart.sidebar_border.to_string(),
                sidebar_header_bg: theme.chart.sidebar_header_bg.to_string(),
                sidebar_text: theme.chart.sidebar_text.to_string(),
                chart_border: theme.chart.chart_border.to_string(),
                frame_border: theme.chart.frame_border.to_string(),
            },
            series: RuntimeSeriesColors {
                candle_up_body: theme.series.candle_up_body.to_string(),
                candle_up_wick: theme.series.candle_up_wick.to_string(),
                candle_up_border: theme.series.candle_up_border.map(|s| s.to_string()),
                candle_down_body: theme.series.candle_down_body.to_string(),
                candle_down_wick: theme.series.candle_down_wick.to_string(),
                candle_down_border: theme.series.candle_down_border.map(|s| s.to_string()),
                line_color: theme.series.line_color.to_string(),
                line_width: theme.series.line_width,
                step_line_color: theme.series.step_line_color.to_string(),
                line_markers_color: theme.series.line_markers_color.to_string(),
                area_line: theme.series.area_line.to_string(),
                area_top: theme.series.area_top.to_string(),
                area_bottom: theme.series.area_bottom.to_string(),
                histogram_positive: theme.series.histogram_positive.to_string(),
                histogram_negative: theme.series.histogram_negative.to_string(),
                baseline_top_line: theme.series.baseline_top_line.to_string(),
                baseline_top_fill: theme.series.baseline_top_fill.to_string(),
                baseline_bottom_line: theme.series.baseline_bottom_line.to_string(),
                baseline_bottom_fill: theme.series.baseline_bottom_fill.to_string(),
                baseline_line: theme.series.baseline_line.to_string(),
                bar_up: theme.series.bar_up.to_string(),
                bar_down: theme.series.bar_down.to_string(),
                ma_fast: theme.series.ma_fast.to_string(),
                ma_slow: theme.series.ma_slow.to_string(),
                ma_third: theme.series.ma_third.to_string(),
                volume_up: theme.series.volume_up.to_string(),
                volume_down: theme.series.volume_down.to_string(),
                footprint_buy: theme.series.footprint_buy.to_string(),
                footprint_sell: theme.series.footprint_sell.to_string(),
                renko_up: theme.series.renko_up.to_string(),
                renko_down: theme.series.renko_down.to_string(),
                pnf_x: theme.series.pnf_x.to_string(),
                pnf_o: theme.series.pnf_o.to_string(),
                tlb_up: theme.series.tlb_up.to_string(),
                tlb_down: theme.series.tlb_down.to_string(),
                kagi_yang: theme.series.kagi_yang.to_string(),
                kagi_yin: theme.series.kagi_yin.to_string(),
                hollow_up: theme.series.hollow_up.to_string(),
                hollow_down: theme.series.hollow_down.to_string(),
            },
            fonts: RuntimeFonts {
                family: theme.fonts.family.to_string(),
                family_mono: theme.fonts.family_mono.to_string(),
                family_chart: theme.fonts.family_chart.to_string(),
                size_small: theme.fonts.size_small,
                size_normal: theme.fonts.size_normal,
                size_large: theme.fonts.size_large,
                weight_light: theme.fonts.weight_light,
                weight_normal: theme.fonts.weight_normal,
                weight_medium: theme.fonts.weight_medium,
                weight_bold: theme.fonts.weight_bold,
                price_scale_size_min: theme.fonts.price_scale_size_min,
                price_scale_size_max: theme.fonts.price_scale_size_max,
                price_scale_weight: theme.fonts.price_scale_weight,
                time_scale_size: theme.fonts.time_scale_size,
                time_scale_weight: theme.fonts.time_scale_weight,
                legend_size: theme.fonts.legend_size,
                legend_weight: theme.fonts.legend_weight,
                crosshair_label_size: theme.fonts.crosshair_label_size,
                crosshair_label_weight: theme.fonts.crosshair_label_weight,
                watermark_size: theme.fonts.watermark_size,
                watermark_weight: theme.fonts.watermark_weight,
                status_bar_size: theme.fonts.status_bar_size,
                status_bar_weight: theme.fonts.status_bar_weight,
            },
            sizing: RuntimeSizing {
                top_toolbar_height: theme.sizing.top_toolbar_height,
                left_toolbar_width: theme.sizing.left_toolbar_width,
                right_toolbar_width: 48.0, // Default from core constants
                bottom_toolbar_height: 32.0,
                button_height: theme.sizing.button_height,
                button_padding_x: theme.sizing.button_padding_x,
                button_padding_y: theme.sizing.button_padding_y,
                icon_size: theme.sizing.icon_size,
                border_radius: theme.sizing.border_radius,
                dropdown_min_width: theme.sizing.dropdown_min_width,
            },
            effects: RuntimeEffects {
                transition_duration: theme.effects.transition_duration.to_string(),
                shadow_dropdown: theme.effects.shadow_dropdown.to_string(),
                shadow_floating: theme.effects.shadow_floating.to_string(),
                hover_scale: theme.effects.hover_scale,
            },
            // Default to Solid style
            style: UIStyle::default(),
            style_params: StyleParams::default(),
            trading: RuntimeTradingColors::default(),
            panel_style: super::skin::PanelStyleParams::default(),
        }
    }
}

impl RuntimeTheme {
    /// Available preset names. The tail entries are 8-token SKINS
    /// (`theme::SkinTokens` presets derived over the dark base).
    pub const PRESETS: &'static [&'static str] = &[
        "dark", "light", "high_contrast", "high_contrast_mono", "mascot",
        "surf", "vaporwave", "terminal",
    ];

    /// Create from a preset name
    pub fn from_preset(name: &str) -> Self {
        // Skin presets: dark base + the 8-token derivation over every
        // color surface (UI chrome, chart, series, trading panels).
        if let Some(tokens) = super::skin::SkinTokens::by_name(name) {
            let mut rt = Self::from(&UITheme::dark());
            tokens.apply(&mut rt);
            return rt;
        }
        match name {
            "dark" => Self::from(&UITheme::dark()),
            "light" => Self::from(&UITheme::light()),
            "high_contrast" => Self::from(&UITheme::high_contrast()),
            "high_contrast_mono" => Self::from(&UITheme::high_contrast_mono()),
            "mascot" => Self::from(&UITheme::mascot()),
            _ => Self::from(&UITheme::dark()),
        }
    }

    /// Create default (dark) theme
    pub fn dark() -> Self {
        Self::from_preset("dark")
    }

    /// Create light theme
    pub fn light() -> Self {
        Self::from_preset("light")
    }

    /// Create high contrast theme
    pub fn high_contrast() -> Self {
        Self::from_preset("high_contrast")
    }

    /// Create high contrast mono theme
    pub fn high_contrast_mono() -> Self {
        Self::from_preset("high_contrast_mono")
    }

    /// Create mascot theme
    pub fn mascot() -> Self {
        Self::from_preset("mascot")
    }

    // === Style Management ===

    /// Set UI style and update params to match
    pub fn set_style(&mut self, style: UIStyle) {
        self.style = style;
        self.style_params = style.default_params();
    }

    /// Set UI style keeping custom params
    pub fn set_style_keep_params(&mut self, style: UIStyle) {
        self.style = style;
    }

    /// Get toolbar background with style opacity applied
    pub fn toolbar_bg_styled(&self) -> String {
        use super::style::OpacityType;
        self.style_params.apply_opacity(&self.colors.toolbar_bg, OpacityType::Toolbar)
    }

    /// Get modal background with style opacity applied
    pub fn modal_bg_styled(&self) -> String {
        use super::style::OpacityType;
        self.style_params.apply_opacity(&self.colors.dropdown_bg, OpacityType::Modal)
    }

    /// Get sidebar background with style opacity applied
    pub fn sidebar_bg_styled(&self) -> String {
        use super::style::OpacityType;
        self.style_params.apply_opacity(&self.chart.sidebar_bg, OpacityType::Sidebar)
    }

    /// Get menu background with style opacity applied
    pub fn menu_bg_styled(&self) -> String {
        use super::style::OpacityType;
        self.style_params.apply_opacity(&self.colors.dropdown_bg, OpacityType::Menu)
    }

    /// Get scale background with style opacity applied
    pub fn scale_bg_styled(&self) -> String {
        use super::style::OpacityType;
        self.style_params.apply_opacity(&self.chart.scale_bg, OpacityType::Scale)
    }

    /// Get sub-pane background with style opacity applied
    pub fn sub_pane_bg_styled(&self) -> String {
        use super::style::OpacityType;
        self.style_params.apply_opacity(&self.chart.background, OpacityType::SubPane)
    }

    /// Get hover background with style opacity applied
    pub fn hover_bg_styled(&self) -> String {
        use super::style::OpacityType;
        self.style_params.apply_opacity(&self.colors.button_bg_hover, OpacityType::Hover)
    }

    /// Get active background with style opacity applied
    pub fn active_bg_styled(&self) -> String {
        use super::style::OpacityType;
        self.style_params.apply_opacity(&self.colors.button_bg_active, OpacityType::Active)
    }

    /// Get crosshair label background with style opacity applied
    pub fn crosshair_label_bg_styled(&self) -> String {
        use super::style::OpacityType;
        self.style_params.apply_opacity(&self.chart.crosshair_label_bg, OpacityType::CrosshairLabel)
    }

    /// Check if blur should be applied (FrostedGlass/LiquidGlass with blur_radius > 0)
    pub fn should_blur(&self) -> bool {
        self.style_params.blur_radius > 0.0
    }

    /// Check if hover shimmer effect should be applied
    pub fn should_shimmer(&self) -> bool {
        self.style_params.hover_shimmer
    }

    // === JSON Serialization ===

    /// Serialize to JSON string
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// Serialize to pretty JSON string
    pub fn to_json_pretty(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    /// Deserialize from JSON string
    pub fn from_json(json: &str) -> Option<Self> {
        serde_json::from_str(json).ok()
    }

    // === Helper methods for canvas rendering ===

    /// Get price scale font string (e.g., "12px 'Trebuchet MS', Arial")
    pub fn price_scale_font(&self, size: f64) -> String {
        format!("{}px {}", size as i32, self.fonts.family_chart)
    }

    /// Get time scale font string
    pub fn time_scale_font(&self) -> String {
        format!("{}px {}", self.fonts.time_scale_size as i32, self.fonts.family_chart)
    }

    /// Get legend font string
    pub fn legend_font(&self) -> String {
        format!("{}px {}", self.fonts.legend_size as i32, self.fonts.family)
    }

    /// Get crosshair label font string
    pub fn crosshair_font(&self) -> String {
        format!("{}px {}", self.fonts.crosshair_label_size as i32, self.fonts.family_chart)
    }

    /// Get grid color (with optional directional override)
    pub fn grid_color(&self, horizontal: bool) -> &str {
        if horizontal {
            self.chart.grid_line_horz.as_deref().unwrap_or(&self.chart.grid_line)
        } else {
            self.chart.grid_line_vert.as_deref().unwrap_or(&self.chart.grid_line)
        }
    }
}

impl Default for RuntimeTheme {
    fn default() -> Self {
        Self::dark()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_preset_creation() {
        let dark = RuntimeTheme::from_preset("dark");
        assert_eq!(dark.name, "Dark");

        let light = RuntimeTheme::from_preset("light");
        assert_eq!(light.name, "Light");

        let unknown = RuntimeTheme::from_preset("unknown");
        assert_eq!(unknown.name, "Dark"); // Falls back to dark
    }

    #[test]
    fn test_json_roundtrip() {
        let theme = RuntimeTheme::dark();
        let json = theme.to_json();
        let restored = RuntimeTheme::from_json(&json).unwrap();
        assert_eq!(theme.name, restored.name);
        assert_eq!(theme.colors.toolbar_bg, restored.colors.toolbar_bg);
    }

    #[test]
    fn test_color_modification() {
        let mut theme = RuntimeTheme::dark();
        theme.colors.toolbar_bg = "#ff0000".to_string();
        assert_eq!(theme.colors.toolbar_bg, "#ff0000");
    }

    /// Old theme JSON saved before the per-type color fields existed
    /// (owner-approved matrix wave 2, 2026-07-11) must still deserialize —
    /// missing keys fall back to the same green/red every renderer
    /// hardcoded before this field existed, matching the `footprint_buy`/
    /// `footprint_sell` precedent (2026-07-05).
    #[test]
    fn test_per_type_colors_default_on_missing_json_keys() {
        let mut value = serde_json::to_value(RuntimeTheme::dark()).unwrap();
        let series = value.get_mut("series").unwrap().as_object_mut().unwrap();
        for key in [
            "renko_up", "renko_down", "pnf_x", "pnf_o",
            "tlb_up", "tlb_down", "kagi_yang", "kagi_yin",
            "hollow_up", "hollow_down",
        ] {
            series.remove(key);
        }
        let restored: RuntimeTheme = serde_json::from_value(value).unwrap();
        assert_eq!(restored.series.renko_up, "#26a69a");
        assert_eq!(restored.series.renko_down, "#ef5350");
        assert_eq!(restored.series.pnf_x, "#26a69a");
        assert_eq!(restored.series.pnf_o, "#ef5350");
        assert_eq!(restored.series.tlb_up, "#26a69a");
        assert_eq!(restored.series.tlb_down, "#ef5350");
        assert_eq!(restored.series.kagi_yang, "#26a69a");
        assert_eq!(restored.series.kagi_yin, "#ef5350");
        assert_eq!(restored.series.hollow_up, "#26a69a");
        assert_eq!(restored.series.hollow_down, "#ef5350");
    }

    /// Old theme JSON saved before the line-family per-type color fields
    /// existed (owner-approved matrix wave 3, 2026-07-11) must still
    /// deserialize — missing keys fall back to the same green every
    /// line-family renderer hardcoded before this field existed.
    #[test]
    fn test_line_family_colors_default_on_missing_json_keys() {
        let mut value = serde_json::to_value(RuntimeTheme::dark()).unwrap();
        let series = value.get_mut("series").unwrap().as_object_mut().unwrap();
        for key in ["step_line_color", "line_markers_color"] {
            series.remove(key);
        }
        let restored: RuntimeTheme = serde_json::from_value(value).unwrap();
        assert_eq!(restored.series.step_line_color, "#26a69a");
        assert_eq!(restored.series.line_markers_color, "#26a69a");
    }
}
