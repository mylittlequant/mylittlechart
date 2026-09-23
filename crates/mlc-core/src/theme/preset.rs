//! UI Theme Presets - Static compile-time theme definitions
//!
//! # Architecture
//!
//! This module defines CHART-SPECIFIC theming:
//! - **ChartColors**: Background, grid, scales, crosshair, watermark, sidebar
//! - **SeriesColors**: Candles, line, area, histogram, baseline, bars, volume
//! - **ChartFonts**: Price scale, time scale, legend, crosshair, watermark fonts
//!
//! ## Terminal Concerns (NOT in chart crate)
//!
//! The following are **terminal-specific** and should be managed by the terminal:
//! - Toolbar colors, button colors, dropdown colors
//! - Status bar styling
//! - Modal/dialog styling
//! - UI borders and dividers (except chart-specific)
//! - Button sizing, toolbar dimensions
//! - Animation effects, transitions
//!
//! Terminal should compose its own `TerminalTheme` that includes `ChartTheme`.

/// Complete chart theme definition
#[derive(Clone, Debug)]
pub struct ChartTheme {
    pub name: &'static str,

    /// Chart-specific colors (grid, scales, background)
    pub chart: ChartColors,

    /// Series colors (candles, line, area, etc.)
    pub series: SeriesColors,

    /// Chart-specific typography
    pub fonts: ChartFonts,
}

/// Chart-specific colors (background, grid, scales, crosshair)
#[derive(Clone, Debug)]
pub struct ChartColors {
    // Background
    pub background: &'static str,

    // Grid
    pub grid_line: &'static str,
    pub grid_line_horz: Option<&'static str>,  // Override for horizontal lines
    pub grid_line_vert: Option<&'static str>,  // Override for vertical lines

    // Price scale (right axis)
    pub scale_bg: &'static str,
    pub scale_border: &'static str,
    pub scale_text: &'static str,
    pub scale_text_muted: &'static str,

    // Time scale (bottom axis)
    pub time_scale_bg: &'static str,
    pub time_scale_border: &'static str,
    pub time_scale_text: &'static str,        // Major ticks (Year, Month)
    pub time_scale_text_medium: &'static str, // Medium ticks (Day, Week)
    pub time_scale_text_muted: &'static str,  // Minor ticks (Hour, etc.)

    // Crosshair
    pub crosshair_line: &'static str,
    pub crosshair_label_bg: &'static str,
    pub crosshair_label_text: &'static str,

    // Legend (OHLC display)
    pub legend_text: &'static str,
    pub legend_value_up: &'static str,
    pub legend_value_down: &'static str,

    // Watermark
    pub watermark_text: &'static str,

    // Sidebar panels (indicator pane headers)
    pub sidebar_bg: &'static str,
    pub sidebar_border: &'static str,
    pub sidebar_header_bg: &'static str,
    pub sidebar_text: &'static str,

    // Chart frame borders
    pub chart_border: &'static str,  // Border around the chart area (all 4 sides)
    pub frame_border: &'static str,  // Outer frame border (right of price scale, bottom of time scale)
}

/// Series/data visualization colors
#[derive(Clone, Debug)]
pub struct SeriesColors {
    // Candlestick
    pub candle_up_body: &'static str,
    pub candle_up_wick: &'static str,
    pub candle_up_border: Option<&'static str>,  // None = same as body
    pub candle_down_body: &'static str,
    pub candle_down_wick: &'static str,
    pub candle_down_border: Option<&'static str>,

    // Line series
    pub line_color: &'static str,
    pub line_width: f64,

    // Step line (staircase) — own color identity, wave 3 (2026-07-11):
    // previously rendered via the shared `line_color`; now independently
    // colorable in the settings modal per-type block. Width lives in
    // `LineFamilyDisplayOptions` (non-color per-type option, mirrors how
    // wave 2 kept numeric params out of `ChartTheme`).
    pub step_line_color: &'static str,

    // Line with markers — line color reuses `line_color` (shared "Line"
    // family identity); marker color is its own field (wave 3, 2026-07-11).
    // Marker style/size live in `LineFamilyDisplayOptions`.
    pub line_markers_color: &'static str,

    // Area series
    pub area_line: &'static str,
    pub area_top: &'static str,
    pub area_bottom: &'static str,

    // Histogram
    pub histogram_positive: &'static str,
    pub histogram_negative: &'static str,

    // Baseline
    pub baseline_top_line: &'static str,
    pub baseline_top_fill: &'static str,
    pub baseline_bottom_line: &'static str,
    pub baseline_bottom_fill: &'static str,
    pub baseline_line: &'static str,

    // Bar series (OHLC bars)
    pub bar_up: &'static str,
    pub bar_down: &'static str,

    // Moving averages
    pub ma_fast: &'static str,
    pub ma_slow: &'static str,
    pub ma_third: &'static str,  // For 3rd MA if needed

    // Volume — hue is NOT stored here. Volume bars always derive their hue
    // from `candle_up_body`/`candle_down_body` (owner ruling 2026-09-24,
    // R21: "volume bars and candles must share ONE hue in every theme").
    // Only the alpha — the one thing a preset is allowed to vary — lives
    // here; see `mlc_core::utils::color::apply_opacity`.
    pub volume_up_alpha: f64,
    pub volume_down_alpha: f64,

    // Footprint clusters (buy/sell volume text + heat fill)
    pub footprint_buy: &'static str,
    pub footprint_sell: &'static str,

    // Renko bricks (fill wick lines keep global wick colors)
    pub renko_up: &'static str,
    pub renko_down: &'static str,

    // Point & Figure columns (X / O glyphs)
    pub pnf_x: &'static str,
    pub pnf_o: &'static str,

    // Three Line Break (san-sen-ashi) line fill
    pub tlb_up: &'static str,
    pub tlb_down: &'static str,

    // Kagi (yang = thick line, yin = thin line)
    pub kagi_yang: &'static str,
    pub kagi_yin: &'static str,

    // Hollow candles (bullish outline / bearish fill)
    pub hollow_up: &'static str,
    pub hollow_down: &'static str,
}

/// Chart-specific font settings
#[derive(Clone, Debug)]
pub struct ChartFonts {
    /// Font family for chart elements (scales, legend, watermark)
    pub family: &'static str,

    // Price scale font
    pub price_scale_size_min: f64,
    pub price_scale_size_max: f64,
    pub price_scale_weight: u16,

    // Time scale font
    pub time_scale_size: f64,
    pub time_scale_weight: u16,

    // Legend font
    pub legend_size: f64,
    pub legend_weight: u16,

    // Crosshair label font
    pub crosshair_label_size: f64,
    pub crosshair_label_weight: u16,

    // Watermark font
    pub watermark_size: f64,
    pub watermark_weight: u16,
}

impl Default for ChartTheme {
    fn default() -> Self {
        Self::dark()
    }
}

impl ChartTheme {
    /// Dark theme (TradingView-style)
    pub fn dark() -> Self {
        Self {
            name: "Dark",
            chart: ChartColors {
                background: "#131722",

                grid_line: "#2a2e3999",
                grid_line_horz: None,
                grid_line_vert: None,

                scale_bg: "#1e222d",
                scale_border: "#2a2e39",
                scale_text: "#b2b5be",
                scale_text_muted: "#787b86",

                time_scale_bg: "#1e222d",
                time_scale_border: "#2a2e39",
                time_scale_text: "#b2b5be",
                time_scale_text_medium: "#9598a1",
                time_scale_text_muted: "#787b86",

                crosshair_line: "#758696",
                crosshair_label_bg: "#363a45",
                crosshair_label_text: "#d1d4dc",

                legend_text: "#b2b5be",
                legend_value_up: "#26a69a",
                legend_value_down: "#ef5350",

                watermark_text: "rgba(120, 123, 134, 0.3)",

                sidebar_bg: "#1e222d",
                sidebar_border: "#363a45",
                sidebar_header_bg: "#131722",
                sidebar_text: "#b2b5be",

                chart_border: "#363a45",
                frame_border: "#2a2e39",
            },
            series: SeriesColors {
                candle_up_body: "#26a69a",
                candle_up_wick: "#26a69a",
                candle_up_border: None,
                candle_down_body: "#ef5350",
                candle_down_wick: "#ef5350",
                candle_down_border: None,

                line_color: "#2962ff",
                line_width: 2.0,

                // Per-type color identity (owner-approved settings matrix
                // wave 3, 2026-07-11): defaults mirror this preset's own
                // candle up/down colors (matches wave 2's renko/pnf/tlb/
                // kagi/hollow convention) — the renderer read `candle_up`/
                // `candle_down` before these fields existed, so this keeps
                // every existing chart pixel-identical until a user edits
                // a swatch.
                step_line_color: "#26a69a",
                line_markers_color: "#26a69a",

                area_line: "#26a69a",
                area_top: "rgba(38, 166, 154, 0.28)",
                area_bottom: "rgba(38, 166, 154, 0.05)",

                histogram_positive: "#26a69a",
                histogram_negative: "#ef5350",

                baseline_top_line: "#26a69a",
                baseline_top_fill: "rgba(38, 166, 154, 0.28)",
                baseline_bottom_line: "#ef5350",
                baseline_bottom_fill: "rgba(239, 83, 80, 0.28)",
                baseline_line: "#758696",

                bar_up: "#26a69a",
                bar_down: "#ef5350",

                ma_fast: "#2962ff",
                ma_slow: "#ff6d00",
                ma_third: "#e040fb",

                volume_up_alpha: 0.5,
                volume_down_alpha: 0.5,

                footprint_buy: "#26a69a",
                footprint_sell: "#ef5350",

                renko_up: "#26a69a",
                renko_down: "#ef5350",

                pnf_x: "#26a69a",
                pnf_o: "#ef5350",

                tlb_up: "#26a69a",
                tlb_down: "#ef5350",

                kagi_yang: "#26a69a",
                kagi_yin: "#ef5350",

                hollow_up: "#26a69a",
                hollow_down: "#ef5350",
            },
            fonts: ChartFonts {
                family: "'Trebuchet MS', Arial, sans-serif",

                price_scale_size_min: 9.0,
                price_scale_size_max: 13.0,
                price_scale_weight: 400,

                time_scale_size: 12.0,
                time_scale_weight: 400,

                legend_size: 12.0,
                legend_weight: 500,

                crosshair_label_size: 11.0,
                crosshair_label_weight: 400,

                watermark_size: 52.0,
                watermark_weight: 700,
            },
        }
    }

    /// Light theme
    pub fn light() -> Self {
        Self {
            name: "Light",
            chart: ChartColors {
                background: "#ffffff",

                grid_line: "rgba(0,0,0,0.09)",
                grid_line_horz: None,
                grid_line_vert: None,

                scale_bg: "#f8f9fa",
                scale_border: "#dee2e6",
                scale_text: "#434651",
                scale_text_muted: "#787b86",

                time_scale_bg: "#f8f9fa",
                time_scale_border: "#dee2e6",
                time_scale_text: "#434651",
                time_scale_text_medium: "#5d606b",
                time_scale_text_muted: "#787b86",

                crosshair_line: "#9598a1",
                crosshair_label_bg: "#131722",
                crosshair_label_text: "#ffffff",

                legend_text: "#434651",
                legend_value_up: "#26a69a",
                legend_value_down: "#ef5350",

                watermark_text: "rgba(0, 0, 0, 0.06)",

                sidebar_bg: "#f8f9fa",
                sidebar_border: "#dee2e6",
                sidebar_header_bg: "#e9ecef",
                sidebar_text: "#434651",

                chart_border: "#dee2e6",
                frame_border: "#ced4da",
            },
            series: SeriesColors {
                candle_up_body: "#26a69a",
                candle_up_wick: "#26a69a",
                candle_up_border: None,
                candle_down_body: "#ef5350",
                candle_down_wick: "#ef5350",
                candle_down_border: None,

                line_color: "#2962ff",
                line_width: 2.0,

                step_line_color: "#26a69a",
                line_markers_color: "#26a69a",

                area_line: "#26a69a",
                area_top: "rgba(38, 166, 154, 0.28)",
                area_bottom: "rgba(38, 166, 154, 0.05)",

                histogram_positive: "#26a69a",
                histogram_negative: "#ef5350",

                baseline_top_line: "#26a69a",
                baseline_top_fill: "rgba(38, 166, 154, 0.28)",
                baseline_bottom_line: "#ef5350",
                baseline_bottom_fill: "rgba(239, 83, 80, 0.28)",
                baseline_line: "#9598a1",

                bar_up: "#26a69a",
                bar_down: "#ef5350",

                ma_fast: "#2962ff",
                ma_slow: "#ff6d00",
                ma_third: "#e040fb",

                volume_up_alpha: 0.5,
                volume_down_alpha: 0.5,

                footprint_buy: "#26a69a",
                footprint_sell: "#ef5350",

                renko_up: "#26a69a",
                renko_down: "#ef5350",

                pnf_x: "#26a69a",
                pnf_o: "#ef5350",

                tlb_up: "#26a69a",
                tlb_down: "#ef5350",

                kagi_yang: "#26a69a",
                kagi_yin: "#ef5350",

                hollow_up: "#26a69a",
                hollow_down: "#ef5350",
            },
            fonts: ChartFonts {
                family: "'Trebuchet MS', Arial, sans-serif",

                price_scale_size_min: 9.0,
                price_scale_size_max: 13.0,
                price_scale_weight: 400,

                time_scale_size: 12.0,
                time_scale_weight: 400,

                legend_size: 12.0,
                legend_weight: 500,

                crosshair_label_size: 11.0,
                crosshair_label_weight: 400,

                watermark_size: 52.0,
                watermark_weight: 700,
            },
        }
    }

    /// High contrast theme (accessibility)
    pub fn high_contrast() -> Self {
        Self {
            name: "High Contrast",
            chart: ChartColors {
                background: "#000000",

                grid_line: "#333333",
                grid_line_horz: None,
                grid_line_vert: None,

                scale_bg: "#000000",
                scale_border: "#ffffff",
                scale_text: "#ffffff",
                scale_text_muted: "#cccccc",

                time_scale_bg: "#000000",
                time_scale_border: "#ffffff",
                time_scale_text: "#ffffff",
                time_scale_text_medium: "#dddddd",
                time_scale_text_muted: "#cccccc",

                crosshair_line: "#ffffff",
                crosshair_label_bg: "#0066ff",
                crosshair_label_text: "#ffffff",

                legend_text: "#ffffff",
                legend_value_up: "#00ff00",
                legend_value_down: "#ff0000",

                watermark_text: "rgba(255, 255, 255, 0.1)",

                sidebar_bg: "#000000",
                sidebar_border: "#ffffff",
                sidebar_header_bg: "#1a1a1a",
                sidebar_text: "#ffffff",

                chart_border: "#ffffff",
                frame_border: "#808080",
            },
            series: SeriesColors {
                candle_up_body: "#00ff00",
                candle_up_wick: "#00ff00",
                candle_up_border: None,
                candle_down_body: "#ff0000",
                candle_down_wick: "#ff0000",
                candle_down_border: None,

                line_color: "#0066ff",
                line_width: 2.0,

                step_line_color: "#00ff00",
                line_markers_color: "#00ff00",

                area_line: "#00ff00",
                area_top: "rgba(0, 255, 0, 0.4)",
                area_bottom: "rgba(0, 255, 0, 0.1)",

                histogram_positive: "#00ff00",
                histogram_negative: "#ff0000",

                baseline_top_line: "#00ff00",
                baseline_top_fill: "rgba(0, 255, 0, 0.3)",
                baseline_bottom_line: "#ff0000",
                baseline_bottom_fill: "rgba(255, 0, 0, 0.3)",
                baseline_line: "#ffffff",

                bar_up: "#00ff00",
                bar_down: "#ff0000",

                ma_fast: "#0066ff",
                ma_slow: "#ffff00",
                ma_third: "#ff00ff",

                volume_up_alpha: 0.5,
                volume_down_alpha: 0.5,

                footprint_buy: "#26a69a",
                footprint_sell: "#ef5350",

                renko_up: "#00ff00",
                renko_down: "#ff0000",

                pnf_x: "#00ff00",
                pnf_o: "#ff0000",

                tlb_up: "#00ff00",
                tlb_down: "#ff0000",

                kagi_yang: "#00ff00",
                kagi_yin: "#ff0000",

                hollow_up: "#00ff00",
                hollow_down: "#ff0000",
            },
            fonts: ChartFonts {
                family: "Arial, sans-serif",

                price_scale_size_min: 11.0,
                price_scale_size_max: 14.0,
                price_scale_weight: 600,

                time_scale_size: 13.0,
                time_scale_weight: 600,

                legend_size: 14.0,
                legend_weight: 600,

                crosshair_label_size: 12.0,
                crosshair_label_weight: 600,

                watermark_size: 60.0,
                watermark_weight: 700,
            },
        }
    }

    /// High Contrast Monochrome theme
    /// Black background, white elements, hollow bearish candles
    pub fn high_contrast_mono() -> Self {
        Self {
            name: "High Contrast Mono",
            chart: ChartColors {
                background: "#000000",

                grid_line: "#222222",
                grid_line_horz: None,
                grid_line_vert: None,

                scale_bg: "#000000",
                scale_border: "#333333",
                scale_text: "#ffffff",
                scale_text_muted: "#888888",

                time_scale_bg: "#000000",
                time_scale_border: "#333333",
                time_scale_text: "#ffffff",
                time_scale_text_medium: "#cccccc",
                time_scale_text_muted: "#888888",

                crosshair_line: "#ffffff",
                crosshair_label_bg: "#ffffff",
                crosshair_label_text: "#000000",

                legend_text: "#ffffff",
                legend_value_up: "#ffffff",
                legend_value_down: "#999999",

                watermark_text: "rgba(255, 255, 255, 0.06)",

                sidebar_bg: "#000000",
                sidebar_border: "#333333",
                sidebar_header_bg: "#111111",
                sidebar_text: "#ffffff",

                chart_border: "#333333",
                frame_border: "#222222",
            },
            series: SeriesColors {
                // Bullish: filled white, Bearish: hollow (background color = visually empty)
                candle_up_body: "#ffffff",
                candle_up_wick: "#ffffff",
                candle_up_border: None,
                candle_down_body: "#000000",  // Same as background = hollow look
                candle_down_wick: "#ffffff",
                candle_down_border: Some("#ffffff"),  // Border for future use

                line_color: "#ffffff",
                line_width: 2.0,

                step_line_color: "#ffffff",
                line_markers_color: "#ffffff",

                area_line: "#ffffff",
                area_top: "rgba(255, 255, 255, 0.2)",
                area_bottom: "rgba(255, 255, 255, 0.02)",

                histogram_positive: "#ffffff",
                histogram_negative: "#777777",

                baseline_top_line: "#ffffff",
                baseline_top_fill: "rgba(255, 255, 255, 0.15)",
                baseline_bottom_line: "#888888",
                baseline_bottom_fill: "rgba(136, 136, 136, 0.15)",
                baseline_line: "#666666",

                bar_up: "#ffffff",
                bar_down: "#888888",

                ma_fast: "#ffffff",
                ma_slow: "#aaaaaa",
                ma_third: "#666666",

                volume_up_alpha: 0.4,
                volume_down_alpha: 0.15,

                footprint_buy: "#26a69a",
                footprint_sell: "#ef5350",

                renko_up: "#ffffff",
                renko_down: "#000000",

                pnf_x: "#ffffff",
                pnf_o: "#000000",

                tlb_up: "#ffffff",
                tlb_down: "#000000",

                kagi_yang: "#ffffff",
                kagi_yin: "#000000",

                hollow_up: "#ffffff",
                hollow_down: "#000000",
            },
            fonts: ChartFonts {
                family: "Arial, sans-serif",

                price_scale_size_min: 11.0,
                price_scale_size_max: 14.0,
                price_scale_weight: 600,

                time_scale_size: 12.0,
                time_scale_weight: 500,

                legend_size: 12.0,
                legend_weight: 500,

                crosshair_label_size: 11.0,
                crosshair_label_weight: 600,

                watermark_size: 52.0,
                watermark_weight: 600,
            },
        }
    }

    /// Mascot theme (landing page aesthetic: deep navy + blue accents + cream text)
    pub fn mascot() -> Self {
        Self {
            name: "Wizard Hat",
            chart: ChartColors {
                background: "#0a0f1a",

                grid_line: "#3a2e1433",
                grid_line_horz: None,
                grid_line_vert: None,

                scale_bg: "#0e1525",
                scale_border: "#3a2e14",
                scale_text: "#FEFFEE",
                scale_text_muted: "#6b7a8d",

                time_scale_bg: "#0e1525",
                time_scale_border: "#3a2e14",
                time_scale_text: "#FEFFEE",
                time_scale_text_medium: "#b8c4d0",
                time_scale_text_muted: "#6b7a8d",

                crosshair_line: "#F4CD63",
                crosshair_label_bg: "#F4CD63",
                crosshair_label_text: "#0a0f1a",

                legend_text: "#FEFFEE",
                legend_value_up: "#26a69a",
                legend_value_down: "#ef5350",

                watermark_text: "rgba(33,88,164,0.12)",

                sidebar_bg: "#0e1525",
                sidebar_border: "#3a2e14",
                sidebar_header_bg: "#0a0f1a",
                sidebar_text: "#FEFFEE",

                chart_border: "#3a2e14",
                frame_border: "#3a2e14",
            },
            series: SeriesColors {
                candle_up_body: "#26a69a",
                candle_up_wick: "#26a69a",
                candle_up_border: None,
                candle_down_body: "#ef5350",
                candle_down_wick: "#ef5350",
                candle_down_border: None,

                line_color: "#2962ff",
                line_width: 2.0,

                step_line_color: "#26a69a",
                line_markers_color: "#26a69a",

                area_line: "#26a69a",
                area_top: "rgba(38,166,154,0.28)",
                area_bottom: "rgba(38,166,154,0.05)",

                histogram_positive: "#26a69a",
                histogram_negative: "#ef5350",

                baseline_top_line: "#26a69a",
                baseline_top_fill: "rgba(38,166,154,0.28)",
                baseline_bottom_line: "#ef5350",
                baseline_bottom_fill: "rgba(239,83,80,0.28)",
                baseline_line: "#2158A4",

                bar_up: "#26a69a",
                bar_down: "#ef5350",

                ma_fast: "#2962ff",
                ma_slow: "#F4CD63",   // Gold from mascot stars
                ma_third: "#EBA25D", // Orange from mascot nose

                volume_up_alpha: 0.5,
                volume_down_alpha: 0.5,

                footprint_buy: "#26a69a",
                footprint_sell: "#ef5350",

                renko_up: "#26a69a",
                renko_down: "#ef5350",

                pnf_x: "#26a69a",
                pnf_o: "#ef5350",

                tlb_up: "#26a69a",
                tlb_down: "#ef5350",

                kagi_yang: "#26a69a",
                kagi_yin: "#ef5350",

                hollow_up: "#26a69a",
                hollow_down: "#ef5350",
            },
            fonts: ChartFonts {
                family: "'Trebuchet MS', Arial, sans-serif",

                price_scale_size_min: 9.0,
                price_scale_size_max: 13.0,
                price_scale_weight: 400,

                time_scale_size: 12.0,
                time_scale_weight: 400,

                legend_size: 12.0,
                legend_weight: 500,

                crosshair_label_size: 11.0,
                crosshair_label_weight: 400,

                watermark_size: 52.0,
                watermark_weight: 700,
            },
        }
    }

    // =========================================================================
    // Helper methods
    // =========================================================================

    /// Get the price scale font string
    pub fn price_scale_font(&self, size: f64) -> String {
        format!("{}px {}", size as i32, self.fonts.family)
    }

    /// Get the time scale font string
    pub fn time_scale_font(&self) -> String {
        format!("{}px {}", self.fonts.time_scale_size as i32, self.fonts.family)
    }

    /// Get the legend font string
    pub fn legend_font(&self) -> String {
        format!("{}px {}", self.fonts.legend_size as i32, self.fonts.family)
    }

    /// Get crosshair label font string
    pub fn crosshair_font(&self) -> String {
        format!("{}px {}", self.fonts.crosshair_label_size as i32, self.fonts.family)
    }

    /// Get candle up color (body)
    pub fn candle_up(&self) -> &str {
        self.series.candle_up_body
    }

    /// Get candle down color (body)
    pub fn candle_down(&self) -> &str {
        self.series.candle_down_body
    }

    /// Get grid line color (with optional directional override)
    pub fn grid_color(&self, horizontal: bool) -> &str {
        if horizontal {
            self.chart.grid_line_horz.unwrap_or(self.chart.grid_line)
        } else {
            self.chart.grid_line_vert.unwrap_or(self.chart.grid_line)
        }
    }
}

impl Default for ChartColors {
    fn default() -> Self {
        ChartTheme::dark().chart
    }
}

impl Default for SeriesColors {
    fn default() -> Self {
        ChartTheme::dark().series
    }
}

impl Default for ChartFonts {
    fn default() -> Self {
        ChartTheme::dark().fonts
    }
}

// =============================================================================
// LEGACY TYPES - Terminal should use these, then migrate to its own crate
// =============================================================================
// These types are kept for backwards compatibility.
// Terminal should eventually move them to its own ui_theme crate.

/// LEGACY: UI Colors for terminal elements (toolbars, buttons, etc.)
///
/// **Note**: This is terminal-specific. Chart crate should not use these directly.
/// Terminal should eventually move this to its own ui_theme crate.
#[derive(Clone, Debug)]
pub struct UIColors {
    // Backgrounds
    pub toolbar_bg: &'static str,
    pub button_bg: &'static str,
    pub button_bg_hover: &'static str,
    pub button_bg_active: &'static str,
    pub dropdown_bg: &'static str,
    pub button_hover_stroke: &'static str,
    pub button_active_stroke: &'static str,
    pub button_rounding: f32,
    pub status_bar_bg: &'static str,

    // Text
    pub text_primary: &'static str,
    pub text_secondary: &'static str,
    pub text_muted: &'static str,
    /// Text/icon colour inside active items (defaults to text_primary; Wizard Hat uses crosshair blue).
    pub text_active: &'static str,

    // Borders
    pub border: &'static str,
    pub border_light: &'static str,
    pub divider: &'static str,
    pub toolbar_divider: &'static str,
    pub ui_border: &'static str,

    // Accents
    pub accent: &'static str,
    pub accent_hover: &'static str,
    pub success: &'static str,
    pub danger: &'static str,
    pub warning: &'static str,
    // Agent panel
    pub text_selection: &'static str,
    pub bubble_user_bg: &'static str,
}

/// LEGACY: UI Font settings for terminal
///
/// **Note**: This is terminal-specific. Chart uses ChartFonts instead.
#[derive(Clone, Debug)]
pub struct UIFonts {
    pub family: &'static str,
    pub family_mono: &'static str,
    pub family_chart: &'static str,
    pub size_small: f64,
    pub size_normal: f64,
    pub size_large: f64,
    pub weight_light: u16,
    pub weight_normal: u16,
    pub weight_medium: u16,
    pub weight_bold: u16,
    pub price_scale_size_min: f64,
    pub price_scale_size_max: f64,
    pub price_scale_weight: u16,
    pub time_scale_size: f64,
    pub time_scale_weight: u16,
    pub legend_size: f64,
    pub legend_weight: u16,
    pub crosshair_label_size: f64,
    pub crosshair_label_weight: u16,
    pub watermark_size: f64,
    pub watermark_weight: u16,
    pub status_bar_size: f64,
    pub status_bar_weight: u16,
}

/// LEGACY: UI Sizing for terminal elements
///
/// **Note**: This is terminal-specific. Chart doesn't need toolbar dimensions.
#[derive(Clone, Debug)]
pub struct UISizing {
    pub top_toolbar_height: f32,
    pub left_toolbar_width: f32,
    pub button_height: f32,
    pub button_padding_x: f32,
    pub button_padding_y: f32,
    pub icon_size: f32,
    pub border_radius: f32,
    pub dropdown_min_width: f32,
    pub kbd_padding: f32,
}

/// LEGACY: Visual effects for terminal
///
/// **Note**: This is terminal-specific (transitions, shadows, hover scale).
#[derive(Clone, Debug)]
pub struct UIEffects {
    pub transition_duration: &'static str,
    pub shadow_dropdown: &'static str,
    pub shadow_floating: &'static str,
    pub hover_scale: f64,
}

/// LEGACY: Complete UI theme (for terminal backwards compatibility)
///
/// **Note**: Terminal should eventually create its own TerminalTheme that
/// composes ChartTheme + terminal-specific styles.
#[derive(Clone, Debug)]
pub struct UITheme {
    pub name: &'static str,
    pub colors: UIColors,
    pub chart: ChartColors,
    pub series: SeriesColors,
    pub fonts: UIFonts,
    pub sizing: UISizing,
    pub effects: UIEffects,
}

impl Default for UITheme {
    fn default() -> Self {
        Self::dark()
    }
}

impl UITheme {
    /// Dark theme (full UI + chart)
    pub fn dark() -> Self {
        Self {
            name: "Dark",
            colors: UIColors {
                toolbar_bg: "#131722",
                button_bg: "#1e222d",
                button_bg_hover: "#2a2e39",
                button_bg_active: "#2962ff",
                button_hover_stroke: "transparent",
                button_active_stroke: "transparent",
                button_rounding: 4.0,
                dropdown_bg: "#1e222d",
                status_bar_bg: "#131722",
                text_primary: "#d1d4dc",
                text_secondary: "#b2b5be",
                text_muted: "#787b86",
                text_active: "#d1d4dc",
                border: "#131722",
                border_light: "#2a2e39",
                divider: "#363a45",
                toolbar_divider: "#363a45",
                ui_border: "#363a45",
                accent: "#2962ff",
                accent_hover: "#1e53e4",
                success: "#26a69a",
                danger: "#f23645",
                warning: "#ff9800",
                text_selection: "#2962ff55",
                bubble_user_bg: "#1e1e3f",
            },
            chart: ChartTheme::dark().chart,
            series: ChartTheme::dark().series,
            fonts: UIFonts {
                family: "-apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif",
                family_mono: "'Share Tech Mono', 'Consolas', monospace",
                family_chart: "'Trebuchet MS', Arial, sans-serif",
                size_small: 11.0,
                size_normal: 13.0,
                size_large: 14.0,
                weight_light: 300,
                weight_normal: 400,
                weight_medium: 500,
                weight_bold: 600,
                price_scale_size_min: 9.0,
                price_scale_size_max: 13.0,
                price_scale_weight: 400,
                time_scale_size: 12.0,
                time_scale_weight: 400,
                legend_size: 12.0,
                legend_weight: 500,
                crosshair_label_size: 11.0,
                crosshair_label_weight: 400,
                watermark_size: 52.0,
                watermark_weight: 700,
                status_bar_size: 11.0,
                status_bar_weight: 400,
            },
            sizing: UISizing {
                top_toolbar_height: 40.0,
                left_toolbar_width: 50.0,
                button_height: 28.0,
                button_padding_x: 12.0,
                button_padding_y: 6.0,
                icon_size: 16.0,
                border_radius: 4.0,
                dropdown_min_width: 160.0,
                kbd_padding: 4.0,
            },
            effects: UIEffects {
                transition_duration: "0.15s",
                shadow_dropdown: "0 8px 24px rgba(0,0,0,0.4)",
                shadow_floating: "0 4px 12px rgba(0,0,0,0.3)",
                hover_scale: 0.97,
            },
        }
    }

    /// Light theme (full UI + chart)
    pub fn light() -> Self {
        Self {
            name: "Light",
            colors: UIColors {
                toolbar_bg: "#f8f9fa",
                button_bg: "#e9ecef",
                button_bg_hover: "#dee2e6",
                button_bg_active: "#4a90d9",
                button_hover_stroke: "transparent",
                button_active_stroke: "transparent",
                button_rounding: 4.0,
                dropdown_bg: "#ffffff",
                status_bar_bg: "#f8f9fa",
                text_primary: "#131722",
                text_secondary: "#434651",
                text_muted: "#787b86",
                text_active: "#131722",
                border: "#f8f9fa",
                border_light: "#e9ecef",
                divider: "#dee2e6",
                toolbar_divider: "#dee2e6",
                ui_border: "#dee2e6",
                accent: "#4a90d9",
                accent_hover: "#3a7bc8",
                success: "#26a69a",
                danger: "#f23645",
                warning: "#ff9800",
                text_selection: "#4a90d940",
                bubble_user_bg: "#e8eaff",
            },
            chart: ChartTheme::light().chart,
            series: ChartTheme::light().series,
            fonts: UIFonts {
                family: "-apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif",
                family_mono: "'Share Tech Mono', 'Consolas', monospace",
                family_chart: "'Trebuchet MS', Arial, sans-serif",
                size_small: 11.0,
                size_normal: 12.0,
                size_large: 14.0,
                weight_light: 300,
                weight_normal: 400,
                weight_medium: 500,
                weight_bold: 600,
                price_scale_size_min: 9.0,
                price_scale_size_max: 13.0,
                price_scale_weight: 400,
                time_scale_size: 12.0,
                time_scale_weight: 400,
                legend_size: 12.0,
                legend_weight: 500,
                crosshair_label_size: 11.0,
                crosshair_label_weight: 400,
                watermark_size: 52.0,
                watermark_weight: 700,
                status_bar_size: 11.0,
                status_bar_weight: 400,
            },
            sizing: UISizing {
                top_toolbar_height: 44.0,
                left_toolbar_width: 48.0,
                button_height: 28.0,
                button_padding_x: 12.0,
                button_padding_y: 6.0,
                icon_size: 16.0,
                border_radius: 4.0,
                dropdown_min_width: 160.0,
                kbd_padding: 4.0,
            },
            effects: UIEffects {
                transition_duration: "0.15s",
                shadow_dropdown: "0 8px 24px rgba(0,0,0,0.15)",
                shadow_floating: "0 4px 12px rgba(0,0,0,0.1)",
                hover_scale: 0.97,
            },
        }
    }

    /// High contrast theme (full UI + chart)
    pub fn high_contrast() -> Self {
        Self {
            name: "High Contrast",
            colors: UIColors {
                toolbar_bg: "#000000",
                button_bg: "#1a1a1a",
                button_bg_hover: "#333333",
                button_bg_active: "#0066ff",
                button_hover_stroke: "transparent",
                button_active_stroke: "transparent",
                button_rounding: 4.0,
                dropdown_bg: "#000000",
                status_bar_bg: "#000000",
                text_primary: "#ffffff",
                text_secondary: "#cccccc",
                text_muted: "#999999",
                text_active: "#ffffff",
                border: "#000000",
                border_light: "#666666",
                divider: "#ffffff",
                toolbar_divider: "#ffffff",
                ui_border: "#ffffff",
                accent: "#0066ff",
                accent_hover: "#0055dd",
                success: "#00ff00",
                danger: "#ff0000",
                warning: "#ffff00",
                text_selection: "#0066ffaa",
                bubble_user_bg: "#001a4d",
            },
            chart: ChartTheme::high_contrast().chart,
            series: ChartTheme::high_contrast().series,
            fonts: UIFonts {
                family: "-apple-system, sans-serif",
                family_mono: "'Consolas', monospace",
                family_chart: "Arial, sans-serif",
                size_small: 12.0,
                size_normal: 14.0,
                size_large: 16.0,
                weight_light: 400,
                weight_normal: 400,
                weight_medium: 600,
                weight_bold: 700,
                price_scale_size_min: 11.0,
                price_scale_size_max: 14.0,
                price_scale_weight: 600,
                time_scale_size: 13.0,
                time_scale_weight: 600,
                legend_size: 14.0,
                legend_weight: 600,
                crosshair_label_size: 12.0,
                crosshair_label_weight: 600,
                watermark_size: 60.0,
                watermark_weight: 700,
                status_bar_size: 12.0,
                status_bar_weight: 600,
            },
            sizing: UISizing {
                top_toolbar_height: 48.0,
                left_toolbar_width: 56.0,
                button_height: 32.0,
                button_padding_x: 14.0,
                button_padding_y: 8.0,
                icon_size: 18.0,
                border_radius: 2.0,
                dropdown_min_width: 180.0,
                kbd_padding: 5.0,
            },
            effects: UIEffects {
                transition_duration: "0s",
                shadow_dropdown: "none",
                shadow_floating: "none",
                hover_scale: 1.0,
            },
        }
    }

    /// High Contrast Mono theme (full UI + chart)
    pub fn high_contrast_mono() -> Self {
        Self {
            name: "High Contrast Mono",
            colors: UIColors {
                toolbar_bg: "#000000",
                button_bg: "#111111",
                button_bg_hover: "#222222",
                button_bg_active: "#999999",
                button_hover_stroke: "#444444",
                button_active_stroke: "#ffffff",
                button_rounding: 0.0,
                dropdown_bg: "#111111",
                status_bar_bg: "#000000",
                text_primary: "#ffffff",
                text_secondary: "#cccccc",
                text_muted: "#888888",
                text_active: "#ffffff",
                border: "#000000",
                border_light: "#333333",
                divider: "#444444",
                toolbar_divider: "#333333",
                ui_border: "#333333",
                accent: "#999999",
                accent_hover: "#bbbbbb",
                success: "#cccccc",
                danger: "#ffffff",
                warning: "#aaaaaa",
                text_selection: "#99999955",
                bubble_user_bg: "#222222",
            },
            chart: ChartTheme::high_contrast_mono().chart,
            series: ChartTheme::high_contrast_mono().series,
            fonts: UIFonts {
                family: "Arial, sans-serif",
                family_mono: "'Consolas', monospace",
                family_chart: "Arial, sans-serif",
                size_small: 11.0,
                size_normal: 13.0,
                size_large: 15.0,
                weight_light: 400,
                weight_normal: 500,
                weight_medium: 600,
                weight_bold: 700,
                price_scale_size_min: 11.0,
                price_scale_size_max: 14.0,
                price_scale_weight: 600,
                time_scale_size: 12.0,
                time_scale_weight: 500,
                legend_size: 12.0,
                legend_weight: 500,
                crosshair_label_size: 11.0,
                crosshair_label_weight: 600,
                watermark_size: 52.0,
                watermark_weight: 600,
                status_bar_size: 11.0,
                status_bar_weight: 500,
            },
            sizing: UISizing {
                top_toolbar_height: 44.0,
                left_toolbar_width: 52.0,
                button_height: 30.0,
                button_padding_x: 12.0,
                button_padding_y: 6.0,
                icon_size: 16.0,
                border_radius: 0.0,
                dropdown_min_width: 160.0,
                kbd_padding: 4.0,
            },
            effects: UIEffects {
                transition_duration: "0s",
                shadow_dropdown: "none",
                shadow_floating: "none",
                hover_scale: 1.0,
            },
        }
    }

    /// Mascot theme (full UI + chart)
    pub fn mascot() -> Self {
        Self {
            name: "Wizard Hat",
            colors: UIColors {
                toolbar_bg: "#0a0f1a",
                button_bg: "#131d2e",
                button_bg_hover: "#2158A440",
                button_bg_active: "#2158A4",
                button_hover_stroke: "transparent",
                button_active_stroke: "transparent",
                button_rounding: 6.0,
                dropdown_bg: "#131d2e",
                status_bar_bg: "#0a0f1a",
                text_primary: "#FEFFEE",
                text_secondary: "#FEFFEE99",
                text_muted: "#6b7a8d",
                text_active: "#FEFFEE",
                border: "#0a0f1a",
                border_light: "#3a2e14",
                divider: "#3a2e14",
                toolbar_divider: "#3a2e14",
                ui_border: "#3a2e14",
                accent: "#2158A4",
                accent_hover: "#1A4A8A",
                danger: "#f23645",
                success: "#26a69a",
                warning: "#F4CD63",
                text_selection: "#2158A455",
                bubble_user_bg: "#0f1a33",
            },
            chart: ChartTheme::mascot().chart,
            series: ChartTheme::mascot().series,
            fonts: UIFonts {
                family: "-apple-system, BlinkMacSystemFont, 'Segoe UI', sans-serif",
                family_mono: "'Share Tech Mono', 'Consolas', monospace",
                family_chart: "'Trebuchet MS', Arial, sans-serif",
                size_small: 11.0,
                size_normal: 13.0,
                size_large: 14.0,
                weight_light: 300,
                weight_normal: 400,
                weight_medium: 500,
                weight_bold: 600,
                price_scale_size_min: 9.0,
                price_scale_size_max: 13.0,
                price_scale_weight: 400,
                time_scale_size: 12.0,
                time_scale_weight: 400,
                legend_size: 12.0,
                legend_weight: 500,
                crosshair_label_size: 11.0,
                crosshair_label_weight: 400,
                watermark_size: 52.0,
                watermark_weight: 700,
                status_bar_size: 11.0,
                status_bar_weight: 400,
            },
            sizing: UISizing {
                top_toolbar_height: 40.0,
                left_toolbar_width: 48.0,
                button_height: 28.0,
                button_padding_x: 12.0,
                button_padding_y: 6.0,
                icon_size: 16.0,
                border_radius: 6.0,
                dropdown_min_width: 160.0,
                kbd_padding: 4.0,
            },
            effects: UIEffects {
                transition_duration: "0.15s",
                shadow_dropdown: "0 8px 24px rgba(10,15,26,0.7)",
                shadow_floating: "0 4px 12px rgba(10,15,26,0.5)",
                hover_scale: 0.98,
            },
        }
    }

    /// Generate CSS for this theme (terminal helper)
    pub fn to_css(&self, class_prefix: &str) -> String {
        format!(
            r#"
.{prefix}-toolbar {{
    background: {toolbar_bg};
    border-color: {border};
    font-family: {font_family};
    font-size: {font_size}px;
    padding: {toolbar_padding}px;
}}

.{prefix}-btn {{
    background: {button_bg};
    color: {text_primary};
    border-radius: {border_radius}px;
    padding: {btn_pad_y}px {btn_pad_x}px;
    font-weight: {font_weight};
    transition: all {transition};
}}

.{prefix}-btn:hover {{
    background: {button_bg_hover};
    transform: scale({hover_scale});
}}

.{prefix}-btn.active {{
    background: {button_bg_active};
}}
"#,
            prefix = class_prefix,
            toolbar_bg = self.colors.toolbar_bg,
            border = self.colors.border,
            font_family = self.fonts.family,
            font_size = self.fonts.size_normal,
            toolbar_padding = self.sizing.button_padding_y,
            button_bg = self.colors.button_bg,
            text_primary = self.colors.text_primary,
            border_radius = self.sizing.border_radius,
            btn_pad_y = self.sizing.button_padding_y,
            btn_pad_x = self.sizing.button_padding_x,
            font_weight = self.fonts.weight_medium,
            transition = self.effects.transition_duration,
            button_bg_hover = self.colors.button_bg_hover,
            hover_scale = self.effects.hover_scale,
            button_bg_active = self.colors.button_bg_active,
        )
    }

    /// Get the price scale font string
    pub fn price_scale_font(&self, size: f64) -> String {
        format!("{}px {}", size as i32, self.fonts.family_chart)
    }

    /// Get the time scale font string
    pub fn time_scale_font(&self) -> String {
        format!("{}px {}", self.fonts.time_scale_size as i32, self.fonts.family_chart)
    }

    /// Get the legend font string
    pub fn legend_font(&self) -> String {
        format!("{}px {}", self.fonts.legend_size as i32, self.fonts.family)
    }

    /// Get crosshair label font string
    pub fn crosshair_font(&self) -> String {
        format!("{}px {}", self.fonts.crosshair_label_size as i32, self.fonts.family_chart)
    }

    /// Get candle up color (body)
    pub fn candle_up(&self) -> &str {
        self.series.candle_up_body
    }

    /// Get candle down color (body)
    pub fn candle_down(&self) -> &str {
        self.series.candle_down_body
    }

    /// Get candle up wick color
    pub fn candle_up_wick(&self) -> &str {
        self.series.candle_up_wick
    }

    /// Get candle down wick color
    pub fn candle_down_wick(&self) -> &str {
        self.series.candle_down_wick
    }

    /// Get grid line color (with optional directional override)
    pub fn grid_color(&self, horizontal: bool) -> &str {
        if horizontal {
            self.chart.grid_line_horz.unwrap_or(self.chart.grid_line)
        } else {
            self.chart.grid_line_vert.unwrap_or(self.chart.grid_line)
        }
    }
}

#[cfg(test)]
mod per_type_color_tests {
    //! Completeness check for the per-type color identity fields (owner-
    //! approved matrix wave 2, 2026-07-11): every `ChartTheme` preset must
    //! define non-empty renko/pnf/tlb/kagi/hollow colors, and each pair
    //! must default to that SAME preset's own candle up/down colors so the
    //! new fields don't change any existing chart's appearance until a
    //! user edits a swatch.
    use super::ChartTheme;

    fn all_presets() -> Vec<ChartTheme> {
        vec![
            ChartTheme::dark(),
            ChartTheme::light(),
            ChartTheme::high_contrast(),
            ChartTheme::high_contrast_mono(),
            ChartTheme::mascot(),
        ]
    }

    #[test]
    fn every_preset_defines_non_empty_per_type_colors() {
        for theme in all_presets() {
            let s = &theme.series;
            for (label, up, down) in [
                ("renko", s.renko_up, s.renko_down),
                ("pnf", s.pnf_x, s.pnf_o),
                ("tlb", s.tlb_up, s.tlb_down),
                ("kagi", s.kagi_yang, s.kagi_yin),
                ("hollow", s.hollow_up, s.hollow_down),
            ] {
                assert!(!up.is_empty(), "{}: '{}' up/x/yang color is empty", theme.name, label);
                assert!(!down.is_empty(), "{}: '{}' down/o/yin color is empty", theme.name, label);
            }
        }
    }

    #[test]
    fn defaults_derive_from_this_presets_own_candle_colors() {
        for theme in all_presets() {
            let s = &theme.series;
            let (up, down) = (s.candle_up_body, s.candle_down_body);
            assert_eq!(s.renko_up, up, "{}: renko_up should default to candle_up_body", theme.name);
            assert_eq!(s.renko_down, down, "{}: renko_down should default to candle_down_body", theme.name);
            assert_eq!(s.pnf_x, up, "{}: pnf_x should default to candle_up_body", theme.name);
            assert_eq!(s.pnf_o, down, "{}: pnf_o should default to candle_down_body", theme.name);
            assert_eq!(s.tlb_up, up, "{}: tlb_up should default to candle_up_body", theme.name);
            assert_eq!(s.tlb_down, down, "{}: tlb_down should default to candle_down_body", theme.name);
            assert_eq!(s.kagi_yang, up, "{}: kagi_yang should default to candle_up_body", theme.name);
            assert_eq!(s.kagi_yin, down, "{}: kagi_yin should default to candle_down_body", theme.name);
            assert_eq!(s.hollow_up, up, "{}: hollow_up should default to candle_up_body", theme.name);
            assert_eq!(s.hollow_down, down, "{}: hollow_down should default to candle_down_body", theme.name);
        }
    }
}

#[cfg(test)]
mod volume_hue_tests {
    //! Owner ruling 2026-09-24 (R21): "volume bars and candles must share
    //! ONE hue in every theme" — for every preset, the derived volume
    //! colour's RGB must equal that same preset's own candle body RGB
    //! (alpha may differ; hue may not). This exercises the exact function
    //! the renderer calls (`ChartTheme::derive_volume_colors`), not a
    //! hand-rolled re-implementation of it.
    use super::ChartTheme;
    use crate::chart::render_data::ChartTheme as RenderChartTheme;
    use crate::utils::color::parse_css_color;

    fn all_presets() -> Vec<ChartTheme> {
        vec![
            ChartTheme::dark(),
            ChartTheme::light(),
            ChartTheme::high_contrast(),
            ChartTheme::high_contrast_mono(),
            ChartTheme::mascot(),
        ]
    }

    #[test]
    fn volume_hue_matches_candle_hue_per_preset() {
        for theme in all_presets() {
            let s = &theme.series;
            let (volume_up, volume_down) = RenderChartTheme::derive_volume_colors(
                s.candle_up_body,
                s.candle_down_body,
                s.volume_up_alpha,
                s.volume_down_alpha,
            );

            let (cr, cg, cb, _) = parse_css_color(s.candle_up_body);
            let (vr, vg, vb, _) = parse_css_color(&volume_up);
            assert_eq!(
                (cr, cg, cb), (vr, vg, vb),
                "{}: volume_up hue must match candle_up_body hue", theme.name,
            );

            let (cr, cg, cb, _) = parse_css_color(s.candle_down_body);
            let (vr, vg, vb, _) = parse_css_color(&volume_down);
            assert_eq!(
                (cr, cg, cb), (vr, vg, vb),
                "{}: volume_down hue must match candle_down_body hue", theme.name,
            );
        }
    }

    #[test]
    fn volume_alpha_is_in_valid_range_per_preset() {
        for theme in all_presets() {
            let s = &theme.series;
            assert!(
                (0.0..=1.0).contains(&s.volume_up_alpha),
                "{}: volume_up_alpha {} out of [0, 1]", theme.name, s.volume_up_alpha,
            );
            assert!(
                (0.0..=1.0).contains(&s.volume_down_alpha),
                "{}: volume_down_alpha {} out of [0, 1]", theme.name, s.volume_down_alpha,
            );
        }
    }
}

#[cfg(test)]
mod line_family_color_tests {
    //! Completeness check for the simple line-family per-type color fields
    //! (owner-approved settings matrix wave 3, 2026-07-11): `step_line`/
    //! `line_markers` get their own color field, `area`/`baseline`/`bars`
    //! reuse the pre-existing `SeriesColors` fields (`area_line`/`area_top`/
    //! `area_bottom`/`baseline_top_line`/`baseline_top_fill`/
    //! `baseline_bottom_line`/`baseline_bottom_fill`/`bar_up`/`bar_down`)
    //! that were declared but never read by any renderer before this wave —
    //! every preset's default for all of these now equals that SAME
    //! preset's own candle up/down color so switching the renderers from
    //! `candle_up`/`candle_down` to these fields is pixel-identical until a
    //! user edits a swatch.
    use super::ChartTheme;

    fn all_presets() -> Vec<ChartTheme> {
        vec![
            ChartTheme::dark(),
            ChartTheme::light(),
            ChartTheme::high_contrast(),
            ChartTheme::high_contrast_mono(),
            ChartTheme::mascot(),
        ]
    }

    #[test]
    fn every_preset_defines_non_empty_line_family_colors() {
        for theme in all_presets() {
            let s = &theme.series;
            for (label, color) in [
                ("step_line", s.step_line_color),
                ("line_markers", s.line_markers_color),
                ("area_line", s.area_line),
                ("area_top", s.area_top),
                ("area_bottom", s.area_bottom),
                ("baseline_top_line", s.baseline_top_line),
                ("baseline_top_fill", s.baseline_top_fill),
                ("baseline_bottom_line", s.baseline_bottom_line),
                ("baseline_bottom_fill", s.baseline_bottom_fill),
                ("bar_up", s.bar_up),
                ("bar_down", s.bar_down),
            ] {
                assert!(!color.is_empty(), "{}: '{}' color is empty", theme.name, label);
            }
        }
    }

    #[test]
    fn up_side_defaults_derive_from_this_presets_own_candle_up_color() {
        for theme in all_presets() {
            let s = &theme.series;
            let up = s.candle_up_body;
            assert_eq!(s.step_line_color, up, "{}: step_line_color should default to candle_up_body", theme.name);
            assert_eq!(s.line_markers_color, up, "{}: line_markers_color should default to candle_up_body", theme.name);
            assert_eq!(s.area_line, up, "{}: area_line should default to candle_up_body", theme.name);
            assert_eq!(s.baseline_top_line, up, "{}: baseline_top_line should default to candle_up_body", theme.name);
            assert_eq!(s.bar_up, up, "{}: bar_up should default to candle_up_body", theme.name);
        }
    }

    #[test]
    fn baseline_bottom_and_bar_down_default_to_candle_down_except_mono_hollow_convention() {
        // `high_contrast_mono`'s `candle_down_body` is literally the
        // background color (`#000000`) — a hollow-candle-family visual
        // trick, not a usable line/fill color. `bar_down`/
        // `baseline_bottom_line` for that ONE preset predate this wave and
        // already use `#888888` (the same convention `histogram_negative`/
        // `ma_slow` use), so this check excludes it rather than asserting a
        // literal-black line/bar would be invisible.
        for theme in all_presets() {
            if theme.name == "High Contrast Mono" {
                continue;
            }
            let s = &theme.series;
            let down = s.candle_down_body;
            assert_eq!(s.baseline_bottom_line, down, "{}: baseline_bottom_line should default to candle_down_body", theme.name);
            assert_eq!(s.bar_down, down, "{}: bar_down should default to candle_down_body", theme.name);
        }
    }
}
