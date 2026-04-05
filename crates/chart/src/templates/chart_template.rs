//! Template for chart settings (instrument, scales & lines, status line).
//!
//! A [`ChartTemplate`] snapshots all three groups of chart settings as
//! flexible JSON values so that the exact schema of the settings structs
//! does not have to be reproduced here.  This also means new fields added
//! to the settings structs are captured automatically on the next save.

use serde::{Deserialize, Serialize};

use crate::layout::modals::chart_settings::ChartSettingsData;
use crate::preset::preset::unix_timestamp_parts;

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
        use crate::layout::modals::chart_settings::{
            ChartSettingsData, InstrumentSettings, ScalesLinesSettings, StatusLineSettings,
        };
        let data = ChartSettingsData {
            instrument: InstrumentSettings {
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
                precision_label: "Авто".to_string(),
                timezone_label: "(UTC+0) Лондон".to_string(),
                use_24h: true,
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
                timezone_label: "(UTC+0) Лондон".to_string(),
            },
        };
        Self::new("__developer_default__", &data)
    }
}
