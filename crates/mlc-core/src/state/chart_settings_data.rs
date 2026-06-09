//! Chart-settings data bags (instrument/scales/status-line). Pure serde data,
//! consumed by templates + app; lives in state so the frontend chart-settings
//! modal can extract.

/// Settings for chart instrument (candle colors, etc.)
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct InstrumentSettings {
    /// Use previous close for bar color
    pub use_prev_close_color: bool,
    /// Body colors enabled
    pub body_enabled: bool,
    /// Body up color
    pub body_up_color: String,
    /// Body down color
    pub body_down_color: String,
    /// Border colors enabled
    pub border_enabled: bool,
    /// Border up color
    pub border_up_color: String,
    /// Border down color
    pub border_down_color: String,
    /// Wick colors enabled
    pub wick_enabled: bool,
    /// Wick up color
    pub wick_up_color: String,
    /// Wick down color
    pub wick_down_color: String,
    /// Precision display label (e.g. "Авто", "2 (0.00)")
    pub precision_label: String,
    /// Timezone display label (e.g., "(UTC+3) Москва")
    pub timezone_label: String,
    /// 24-hour format enabled
    pub use_24h: bool,
    /// Show UTC prefix in clock display
    pub show_utc_prefix: bool,
    /// Date format label
    pub date_format_label: String,
    /// Show day of week
    pub show_day_of_week: bool,
    /// Show countdown to bar close inside the last price label
    pub show_bar_countdown: bool,
    /// Price tick line style: "dotted", "dashed", "solid"
    pub price_tick_style: String,
    /// Extend price tick line to the right
    pub price_tick_extend_right: bool,
    /// Extend price tick line to the left
    pub price_tick_extend_left: bool,
}

/// Settings for scales and lines
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct ScalesLinesSettings {
    /// Show grid
    pub show_grid: bool,
    /// Show vertical grid lines
    pub vert_lines: bool,
    /// Show horizontal grid lines
    pub horz_lines: bool,
    /// Show price scale on right
    pub price_scale_right: bool,
    /// Auto scale price
    pub auto_scale: bool,
    /// Show time scale on bottom
    pub time_scale_bottom: bool,
    /// Crosshair mode: "Normal", "Magnet", "Hidden"
    pub crosshair_mode: String,
    /// Crosshair line style: "Solid", "Dashed", "Dotted", "LargeDashed", "SparseDotted"
    pub crosshair_line_style: String,
    /// Crosshair line width (1.0 - 4.0)
    pub crosshair_line_width: f64,
    /// Crosshair line color (hex color)
    pub crosshair_line_color: String,
    /// Price scale position: "left", "right", "hidden"
    pub price_scale_position: String,
    /// Time scale position: "top", "bottom", "hidden"
    pub time_scale_position: String,
    /// Corner visibility: "always", "on_hover", "never"
    pub corner_visibility: String,
    /// Price scale width in pixels (50-150, default 70)
    pub price_scale_width: f64,
    /// Time scale height in pixels (20-60, default 30)
    pub time_scale_height: f64,
    /// Date format: "day_month_year", "month_day_year", "year_month_day", "day_month_short"
    pub date_format: String,
    /// Use 24-hour format (vs 12-hour AM/PM)
    pub use_24h: bool,
    /// Show day of week on time labels
    pub show_day_of_week: bool,
    /// Show countdown to bar close on price scale
    pub show_bar_countdown: bool,
    /// Show previous close price line
    pub show_prev_close: bool,
    /// Timezone display label (e.g., "(UTC+3) Москва")
    pub timezone_label: String,
}

/// Settings for status line elements
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct StatusLineSettings {
    // Legend
    /// Legend position: "top_left", "top_right", "bottom_left", "bottom_right"
    pub legend_position: String,
    /// Show OHLC values in legend
    pub legend_show_ohlc: bool,
    /// Show absolute change in legend
    pub legend_show_change: bool,
    /// Show percentage change in legend
    pub legend_show_percent: bool,

    // Tooltip
    /// Tooltip visibility
    pub tooltip_visible: bool,
    /// Tooltip follows cursor (vs fixed position)
    pub tooltip_follow_cursor: bool,

    // Watermark
    /// Watermark visibility
    pub watermark_visible: bool,
    /// Watermark position: "top_left", "top_right", "bottom_left", "bottom_right", "center"
    pub watermark_position: String,
    /// Watermark color (hex color)
    pub watermark_color: String,
    /// Watermark text
    pub watermark_text: String,

    // Indicator Overlay
    /// Show indicator overlay panel
    pub show_indicator_overlay: bool,
}

/// Settings passed to chart settings modal
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct ChartSettingsData {
    /// Instrument (candle) settings
    pub instrument: InstrumentSettings,
    /// Status line settings (Legend, Tooltip, Watermark, Indicator Overlay)
    pub status_line: StatusLineSettings,
    /// Scales and lines settings
    pub scales: ScalesLinesSettings,
}
