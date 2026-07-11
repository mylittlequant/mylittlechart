//! Chart-settings data bags (instrument/scales/status-line). Pure serde data,
//! consumed by templates + app; lives in state so the frontend chart-settings
//! modal can extract.

/// Settings for chart instrument (candle colors, etc.)
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct InstrumentSettings {
    /// Active chart type of the focused chart panel (e.g. `"candles"`,
    /// `"renko"`, `"footprint"`). Drives which chart-type-params block the
    /// Instrument tab renders (candle-family vs. derived-type controls vs.
    /// none). Not persisted as part of this DTO's own round-trip — it is a
    /// read-only mirror of `ChartPanelState::chart_type` for the modal to
    /// render against; the durable copy lives on `ChartWindowSnapshot`.
    pub chart_type: String,
    /// Current numeric parameter for the active derived chart type (box
    /// size / reversal / threshold / min-ticks / lines-back / ...). Mirrors
    /// `ChartPanelState::chart_type_param`. Unused for candle-family and
    /// parameterless chart types.
    pub chart_type_param: f64,
    /// Current bucket interval for the `"footprint"` chart type. Mirrors
    /// `ChartPanelState::chart_type_interval`. Unused for every other type.
    pub chart_type_interval: String,
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
    /// Footprint checkbox: draw ABS-marker (absorption tint + "ABS" label).
    /// Unused when `chart_type != "footprint"`.
    pub footprint_show_abs_markers: bool,
    /// Footprint checkbox: draw per-cell buy/sell volume numbers.
    /// Unused when `chart_type != "footprint"`.
    pub footprint_show_volume_text: bool,
    /// Footprint buy-side (ask) color — mirrors `ChartTheme::footprint_buy`.
    /// Unused when `chart_type != "footprint"`.
    pub footprint_buy_color: String,
    /// Footprint sell-side (bid) color — mirrors `ChartTheme::footprint_sell`.
    /// Unused when `chart_type != "footprint"`.
    pub footprint_sell_color: String,
    /// Renko brick up color — mirrors `ChartTheme::renko_up`. Unused when
    /// `chart_type != "renko"`.
    pub renko_up_color: String,
    /// Renko brick down color — mirrors `ChartTheme::renko_down`. Unused
    /// when `chart_type != "renko"`.
    pub renko_down_color: String,
    /// Point & Figure X-column color — mirrors `ChartTheme::pnf_x`. Unused
    /// when `chart_type != "pnf"`.
    pub pnf_x_color: String,
    /// Point & Figure O-column color — mirrors `ChartTheme::pnf_o`. Unused
    /// when `chart_type != "pnf"`.
    pub pnf_o_color: String,
    /// Three Line Break up-line color — mirrors `ChartTheme::tlb_up`.
    /// Unused when `chart_type != "three_line_break"`.
    pub tlb_up_color: String,
    /// Three Line Break down-line color — mirrors `ChartTheme::tlb_down`.
    /// Unused when `chart_type != "three_line_break"`.
    pub tlb_down_color: String,
    /// Kagi yang (thick line) color — mirrors `ChartTheme::kagi_yang`.
    /// Unused when `chart_type != "kagi"`.
    pub kagi_yang_color: String,
    /// Kagi yin (thin line) color — mirrors `ChartTheme::kagi_yin`. Unused
    /// when `chart_type != "kagi"`.
    pub kagi_yin_color: String,
    /// Hollow candles bullish (outline) color — mirrors
    /// `ChartTheme::hollow_up`. Unused when `chart_type != "hollow_candles"`.
    pub hollow_up_color: String,
    /// Hollow candles bearish (fill) color — mirrors
    /// `ChartTheme::hollow_down`. Unused when `chart_type != "hollow_candles"`.
    pub hollow_down_color: String,
    // ── Simple line-family per-type colors + widths (owner-approved
    // settings matrix wave 3, 2026-07-11) ──────────────────────────────────
    /// "Line" chart type's own line color — mirrors `ChartTheme::line`.
    /// Unused when `chart_type != "line"`.
    pub line_color: String,
    /// "Line" chart type's line width label ("1" / "2" / "3" / "4" px).
    /// Unused when `chart_type != "line"`.
    pub line_width_label: String,
    /// Step line (staircase) line color — mirrors `ChartTheme::step_line`.
    /// Unused when `chart_type != "step_line"`.
    pub step_line_color: String,
    /// Step line width label ("1" / "2" / "3" / "4" px). Unused when
    /// `chart_type != "step_line"`.
    pub step_line_width_label: String,
    /// Area chart line color — mirrors `ChartTheme::area_line`. Unused when
    /// `chart_type != "area"`.
    pub area_line_color: String,
    /// Area chart top fill color — mirrors `ChartTheme::area_top`. Unused
    /// when `chart_type != "area"`.
    pub area_top_color: String,
    /// Area chart bottom fill color — mirrors `ChartTheme::area_bottom`.
    /// Unused when `chart_type != "area"`.
    pub area_bottom_color: String,
    /// Area chart line width label ("1" / "2" / "3" / "4" px). Unused when
    /// `chart_type != "area"`.
    pub area_line_width_label: String,
    /// Baseline chart top (above-baseline) line color — mirrors
    /// `ChartTheme::baseline_top_line`. Unused when `chart_type != "baseline"`.
    pub baseline_top_line_color: String,
    /// Baseline chart top (above-baseline) fill color — mirrors
    /// `ChartTheme::baseline_top_fill`. Unused when `chart_type != "baseline"`.
    pub baseline_top_fill_color: String,
    /// Baseline chart bottom (below-baseline) line color — mirrors
    /// `ChartTheme::baseline_bottom_line`. Unused when
    /// `chart_type != "baseline"`.
    pub baseline_bottom_line_color: String,
    /// Baseline chart bottom (below-baseline) fill color — mirrors
    /// `ChartTheme::baseline_bottom_fill`. Unused when
    /// `chart_type != "baseline"`.
    pub baseline_bottom_fill_color: String,
    /// Baseline chart line width label ("1" / "2" / "3" / "4" px). Unused
    /// when `chart_type != "baseline"`.
    pub baseline_line_width_label: String,
    /// HLC/OHLC bars up color — mirrors `ChartTheme::bar_up`. Unused when
    /// `chart_type != "bars"`.
    pub bars_up_color: String,
    /// HLC/OHLC bars down color — mirrors `ChartTheme::bar_down`. Unused
    /// when `chart_type != "bars"`.
    pub bars_down_color: String,
    /// Line-with-markers marker color — mirrors `ChartTheme::line_markers`.
    /// Unused when `chart_type != "line_markers"`.
    pub line_markers_color: String,
    /// Line-with-markers marker shape label ("Circle" / "Square" /
    /// "Triangle" / "Diamond"). Unused when `chart_type != "line_markers"`.
    pub line_markers_style_label: String,
    /// Line-with-markers marker size label ("3" / "5" / "7" / "9" px).
    /// Unused when `chart_type != "line_markers"`.
    pub line_markers_size_label: String,
    /// Footprint heat-fill intensity label ("Low" / "Normal" / "High").
    /// Unused when `chart_type != "footprint"`.
    pub footprint_heat_intensity_label: String,
    /// Footprint row-grouping preset label ("Raw" / "2 ticks" / "5 ticks" /
    /// "10 ticks"). Mirrors `ChartPanelState::footprint_display.group_ticks`
    /// (canon-fix wave 2, item 5). Unused when `chart_type != "footprint"`.
    pub footprint_row_grouping_label: String,
    /// Footprint stacked-imbalance ratio label ("1.5x" / "2x" / "3x" /
    /// "4x"). Mirrors `ChartPanelState::footprint_display.imbalance_threshold`
    /// (owner-approved settings matrix, wave 1, item 8). Unused when
    /// `chart_type != "footprint"`.
    pub footprint_imbalance_threshold_label: String,
    /// Footprint cell layout mode label ("Bid×Ask" / "Delta" / "Volume" /
    /// "Volume (Delta Color)" / "Profile" — exact doctrine labels, see
    /// `FootprintCellMode::label`). Mirrors
    /// `ChartPanelState::footprint_display.cell_mode`
    /// (chart-type-render-doctrine-2026-07-09.md §8). Unused when
    /// `chart_type != "footprint"`.
    pub footprint_cell_mode_label: String,
    /// Tick-bar checkbox: modulate body opacity by trade-count intensity
    /// (`(vol/max_vol).sqrt().clamp(0.25,1.0)`, wick stays full opacity).
    /// Default `false` — plain solid bodies exactly like `draw_candles`.
    /// Unused when `chart_type != "tick_bar"`.
    pub tick_intensity: bool,
    /// Bar-family "Wicks" checkbox: re-enable wicks for range_bar /
    /// volume_bar / dollar_bar, which render WITHOUT wicks by default.
    /// Default `false`. Unused for every other chart type.
    pub bar_family_wicks: bool,
    /// TPO checkbox: draw the session date-header strip. Default `false`
    /// (owner order: duplicates the time axis). Mirrors
    /// `ChartPanelState::tpo_display.show_session_dates`. Unused when
    /// `chart_type != "tpo"`.
    pub tpo_show_session_dates: bool,
    /// TPO checkbox: draw the POC tint + line. Default `true`. Mirrors
    /// `ChartPanelState::tpo_display.show_poc`. Unused when
    /// `chart_type != "tpo"`.
    pub tpo_show_poc: bool,
    /// TPO checkbox: draw the Value Area fill band. Default `true`.
    /// Mirrors `ChartPanelState::tpo_display.show_value_area`. Unused when
    /// `chart_type != "tpo"`.
    pub tpo_show_value_area: bool,
    /// TPO checkbox: draw single-print dashed rows. Default `true`.
    /// Mirrors `ChartPanelState::tpo_display.show_single_prints`. Unused
    /// when `chart_type != "tpo"`.
    pub tpo_show_single_prints: bool,
    /// TPO checkbox: draw Poor High / Poor Low markers. Default `true`.
    /// Mirrors `ChartPanelState::tpo_display.show_poor_hl`. Unused when
    /// `chart_type != "tpo"`.
    pub tpo_show_poor_hl: bool,
    /// TPO checkbox: draw the Initial Balance bracket. Default `true`.
    /// Mirrors `ChartPanelState::tpo_display.show_initial_balance`. Unused
    /// when `chart_type != "tpo"`.
    pub tpo_show_initial_balance: bool,
    /// TPO Initial Balance period-count label ("2" / "3" / "4"). Mirrors
    /// `ChartPanelState::tpo_display.ib_periods`. Unused when
    /// `chart_type != "tpo"`.
    pub tpo_ib_periods_label: String,
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
