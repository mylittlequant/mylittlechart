//! Template for chart settings (instrument, scales & lines, status line).
//!
//! A [`ChartTemplate`] snapshots all three groups of chart settings as
//! flexible JSON values so that the exact schema of the settings structs
//! does not have to be reproduced here.  This also means new fields added
//! to the settings structs are captured automatically on the next save.

use serde::{Deserialize, Serialize};

use crate::state::chart_settings_data::ChartSettingsData;
use crate::utils::unix_timestamp_parts;
use crate::scale_settings::TimeFormatSettings;

// =============================================================================
// ChartTemplate
// =============================================================================

/// Saved snapshot of all chart settings.
///
/// The three settings groups are stored as [`serde_json::Value`] to keep this
/// type forward-compatible: fields can be added to the underlying structs
/// without breaking existing template files on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChartTemplate {
    /// Unique identifier generated at creation time.
    /// Format: `"chtmpl_{unix_secs}_{nanos_suffix}"`.
    pub id: String,
    /// User-visible display name for this template.
    pub name: String,
    /// Snapshot of [`InstrumentSettings`].
    pub instrument: serde_json::Value,
    /// Snapshot of [`ScalesLinesSettings`].
    pub scales: serde_json::Value,
    /// Snapshot of [`StatusLineSettings`].
    pub status_line: serde_json::Value,
}

impl ChartTemplate {
    /// Create a template by snapshotting all three settings groups from
    /// `data`.
    ///
    /// Returns `None` if any of the settings groups cannot be serialized
    /// (which should never happen for well-formed settings structs).
    pub fn new(name: &str, data: &ChartSettingsData) -> Self {
        let (secs, nanos) = unix_timestamp_parts();
        Self {
            id: format!("chtmpl_{}_{}", secs, nanos),
            name: name.to_string(),
            instrument: serde_json::to_value(&data.instrument)
                .unwrap_or(serde_json::Value::Null),
            scales: serde_json::to_value(&data.scales)
                .unwrap_or(serde_json::Value::Null),
            status_line: serde_json::to_value(&data.status_line)
                .unwrap_or(serde_json::Value::Null),
        }
    }

    /// Factory defaults from the developer — never depends on user state.
    ///
    /// These values are hardcoded in source and used to restore "По умолчанию"
    /// without relying on any runtime snapshot.
    pub fn developer_defaults() -> Self {
        use crate::state::chart_settings_data::{
            ChartSettingsData, InstrumentSettings, ScalesLinesSettings, StatusLineSettings,
        };
        let data = ChartSettingsData {
            instrument: InstrumentSettings {
                // Not applied by `apply_chart_template` (a template reset
                // only touches candle colors/checkboxes, never the active
                // chart type) — placeholder defaults, never read back.
                chart_type: "candles".to_string(),
                chart_type_param: 0.0,
                chart_type_interval: "1m".to_string(),
                use_prev_close_color: false,
                body_enabled: true,
                body_up_color: "#26a69a".to_string(),
                body_down_color: "#ef5350".to_string(),
                border_enabled: true,
                border_up_color: "#26a69a".to_string(),
                border_down_color: "#ef5350".to_string(),
                wick_enabled: true,
                wick_up_color: "#26a69a".to_string(),
                wick_down_color: "#ef5350".to_string(),
                candle_style_label: "Solid".to_string(),
                footprint_show_abs_markers: true,
                footprint_show_volume_text: true,
                footprint_buy_color: "#26a69a".to_string(),
                footprint_sell_color: "#ef5350".to_string(),
                renko_up_color: "#26a69a".to_string(),
                renko_down_color: "#ef5350".to_string(),
                pnf_x_color: "#26a69a".to_string(),
                pnf_o_color: "#ef5350".to_string(),
                tlb_up_color: "#26a69a".to_string(),
                tlb_down_color: "#ef5350".to_string(),
                kagi_yang_color: "#26a69a".to_string(),
                kagi_yin_color: "#ef5350".to_string(),
                hollow_up_color: "#26a69a".to_string(),
                hollow_down_color: "#ef5350".to_string(),
                line_color: "#26a69a".to_string(),
                line_width_label: "2".to_string(),
                step_line_color: "#26a69a".to_string(),
                step_line_width_label: "2".to_string(),
                area_line_color: "#26a69a".to_string(),
                area_top_color: "rgba(38, 166, 154, 0.28)".to_string(),
                area_bottom_color: "rgba(38, 166, 154, 0.05)".to_string(),
                area_line_width_label: "2".to_string(),
                baseline_top_line_color: "#26a69a".to_string(),
                baseline_top_fill_color: "rgba(38, 166, 154, 0.28)".to_string(),
                baseline_bottom_line_color: "#ef5350".to_string(),
                baseline_bottom_fill_color: "rgba(239, 83, 80, 0.28)".to_string(),
                baseline_line_width_label: "2".to_string(),
                bars_up_color: "#26a69a".to_string(),
                bars_down_color: "#ef5350".to_string(),
                line_markers_color: "#26a69a".to_string(),
                line_markers_style_label: "Circle".to_string(),
                line_markers_size_label: "3".to_string(),
                footprint_heat_intensity_label: "Normal".to_string(),
                footprint_row_grouping_label: "Raw".to_string(),
                footprint_imbalance_threshold_label: "1.5x".to_string(),
                footprint_cell_mode_label: "Bid×Ask".to_string(),
                tick_intensity: false,
                bar_family_wicks: false,
                tpo_show_session_dates: false,
                tpo_show_poc: true,
                tpo_show_value_area: true,
                tpo_show_single_prints: true,
                tpo_show_poor_hl: true,
                tpo_show_initial_balance: true,
                tpo_ib_periods_label: "2".to_string(),
                precision_label: "Авто".to_string(),
                timezone_label: TimeFormatSettings { timezone_offset_hours: 0, ..TimeFormatSettings::default() }.timezone_label(),
                use_24h: true,
                show_utc_prefix: true,
                date_format_label: "21.01.2026".to_string(),
                show_day_of_week: false,
                show_bar_countdown: true,
                price_tick_style: "dotted".to_string(),
                price_tick_extend_right: true,
                price_tick_extend_left: true,
            },
            status_line: StatusLineSettings {
                legend_position: "top_left".to_string(),
                legend_show_ohlc: true,
                legend_show_change: true,
                legend_show_percent: true,
                tooltip_visible: false,
                tooltip_follow_cursor: false,
                watermark_visible: false,
                watermark_position: "center".to_string(),
                watermark_color: "#787B86".to_string(),
                watermark_text: String::new(),
                show_indicator_overlay: true,
            },
            scales: ScalesLinesSettings {
                show_grid: true,
                vert_lines: true,
                horz_lines: true,
                price_scale_right: true,
                auto_scale: true,
                time_scale_bottom: true,
                crosshair_mode: "Normal".to_string(),
                crosshair_line_style: "Dashed".to_string(),
                crosshair_line_width: 1.0,
                crosshair_line_color: "#787B86".to_string(),
                price_scale_position: "right".to_string(),
                time_scale_position: "bottom".to_string(),
                corner_visibility: "on_hover".to_string(),
                price_scale_width: 70.0,
                time_scale_height: 30.0,
                date_format: "day_month_year".to_string(),
                use_24h: true,
                show_day_of_week: false,
                show_bar_countdown: false,
                show_prev_close: false,
                timezone_label: TimeFormatSettings { timezone_offset_hours: 0, ..TimeFormatSettings::default() }.timezone_label(),
            },
        };
        Self::new("__developer_default__", &data)
    }
}
