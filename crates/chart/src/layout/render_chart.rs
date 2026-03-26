//! Chart-only rendering functions
//!
//! This module contains pure chart rendering without UI elements.
//! It is designed to be used standalone (zengeld-chart) or as a foundation
//! for full UI rendering (zengeld-terminal).
//!
//! ## What's included:
//! - Chart area rendering (candles, bars, line, area, etc.)
//! - Price and time scales
//! - Crosshair
//! - Grid
//! - Drawing primitives
//! - Legends and tooltips
//! - Sub-pane layout helpers (grid, price scale)
//!
//! ## What's NOT included (terminal responsibility):
//! - Indicators (overlay and sub-pane) - terminal renders using layout geometry
//! - Alerts - terminal renders using chart coordinate helpers
//! - Signals - terminal renders using chart coordinate helpers
//! - Toolbars, Sidebars, Modals, Dropdowns
//! - Multi-chart window management
//!
//! ## Naming Convention
//! - `draw_*` - Low-level functions that draw a single element
//! - `render_*` - High-level functions that compose multiple draw_ calls

use crate::chart::types::price_scale::ScaleMode;
use crate::render::RenderContext;
use crate::chart::render::{
    ChartRenderState, ChartTheme, ChartRect,
    draw_grid, draw_grid_extended, draw_candles, draw_bars, draw_hollow_candles, draw_heikin_ashi,
    draw_line_series, draw_area_series, draw_baseline_series,
    draw_step_line, draw_line_with_markers, draw_hlc_area, draw_histogram, draw_columns,
    draw_price_scale, draw_time_scale,
    draw_crosshair, draw_pane_crosshair,
    draw_legend, draw_tooltip, LegendData, TooltipLines,
    draw_watermark, draw_last_price_line, draw_compare_overlay,
    ScaleConfig, ScaleTheme, CrosshairConfig,
};
use crate::chart::Tooltip;
use crate::chart::types::compare::CompareOverlay;
use crate::drawing::{DrawingManager, draw_control_points};
use crate::apply_opacity;
use crate::scale_settings::ScaleSettings;
use crate::state::SubPane;
use super::rects::{ChartAreaLayout, FrameLayout, LayoutRect, SubPaneLayout};
use uzor::panels::SeparatorOrientation;

// =============================================================================
// Render Pass Control
// =============================================================================

/// Controls which parts of the frame are rendered.
/// Used for multi-pass rendering to support effects like backdrop blur.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RenderPass {
    /// Render everything (default behavior)
    #[default]
    All,
    /// Render only the chart/background layer (no UI elements)
    /// This includes: chart background, candles, grid, scales, primitives, indicators
    ChartOnly,
    /// Render only the UI layer (no chart)
    /// This includes: toolbars, sidebars, modals, dropdowns, context menus
    UiOnly,
}

impl RenderPass {
    /// Should render chart layer?
    pub fn render_chart(&self) -> bool {
        matches!(self, Self::All | Self::ChartOnly)
    }

    /// Should render UI layer?
    pub fn render_ui(&self) -> bool {
        matches!(self, Self::All | Self::UiOnly)
    }
}

// =============================================================================
// Chart Render Config
// =============================================================================

/// Render configuration for chart area
#[derive(Clone, Debug)]
pub struct ChartRenderConfig {
    /// Scale configuration (font sizes, dimensions)
    pub scale_config: ScaleConfig,
    /// Scale theme (colors)
    pub scale_theme: ScaleTheme,
    /// Crosshair configuration
    pub crosshair_config: CrosshairConfig,
    /// Whether a chart drag is in progress (affects crosshair clamping)
    pub is_dragging: bool,
    /// Chart type to render ("candles", "bars", "hollow_candles", "heikin_ashi", "line", "area", "baseline")
    pub chart_type: &'static str,
}

impl Default for ChartRenderConfig {
    fn default() -> Self {
        Self {
            scale_config: ScaleConfig::default(),
            scale_theme: ScaleTheme::default(),
            crosshair_config: CrosshairConfig::default(),
            is_dragging: false,
            chart_type: "candles",
        }
    }
}

// =============================================================================
// Scale Corner
// =============================================================================

/// State needed for rendering scale corner with buttons
#[derive(Clone, Debug)]
pub struct ScaleCornerState {
    /// Current scale corner mode (Manual / Auto / Focus)
    pub scale_mode: ScaleMode,
    /// Mode label (e.g., "lin", "log", "%")
    pub mode_label: String,
    /// Whether mouse is hovering over A/M button
    pub am_hovered: bool,
    /// Whether mouse is hovering over mode button
    pub mode_hovered: bool,
}

impl Default for ScaleCornerState {
    fn default() -> Self {
        Self {
            scale_mode: ScaleMode::Auto,
            mode_label: "lin".to_string(),
            am_hovered: false,
            mode_hovered: false,
        }
    }
}

/// Hit zones for scale corner buttons
#[derive(Clone, Debug, Default)]
pub struct ScaleCornerHitZones {
    /// A/M button rect
    pub am_button: LayoutRect,
    /// Mode button rect
    pub mode_button: LayoutRect,
}

impl ScaleCornerHitZones {
    /// Check if a point hits the A/M button
    pub fn hits_am(&self, x: f64, y: f64) -> bool {
        self.am_button.contains(x, y)
    }

    /// Check if a point hits the mode button
    pub fn hits_mode(&self, x: f64, y: f64) -> bool {
        self.mode_button.contains(x, y)
    }

    /// Check what button (if any) is hit
    pub fn hit_test(&self, x: f64, y: f64) -> ScaleCornerButton {
        if self.hits_am(x, y) {
            ScaleCornerButton::AutoManual
        } else if self.hits_mode(x, y) {
            ScaleCornerButton::Mode
        } else {
            ScaleCornerButton::None
        }
    }
}

/// Which button in scale corner was clicked
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScaleCornerButton {
    /// A/M toggle button
    AutoManual,
    /// Mode cycle button (lin/log/%)
    Mode,
    /// No button
    None,
}

// =============================================================================
// Frame Theme
// =============================================================================

/// Theme for frame rendering (toolbars + borders)
#[derive(Clone, Debug)]
pub struct FrameTheme {
    /// Toolbar background color
    pub toolbar_bg: String,
    /// Toolbar border color (ui_border)
    pub toolbar_border: String,
    /// Chart area border color (all 4 sides of chart)
    pub chart_border: String,
    /// Frame border color (outer edges of scales)
    pub frame_border: String,
    /// Whether to show internal separators between chart and scales
    pub show_scale_separators: bool,
}

impl Default for FrameTheme {
    fn default() -> Self {
        Self {
            toolbar_bg: "#131722".to_string(),
            toolbar_border: "#363a45".to_string(),
            chart_border: "#363a45".to_string(),
            frame_border: "#2a2e39".to_string(),
            show_scale_separators: true,
        }
    }
}

// =============================================================================
// Helper Functions
// =============================================================================

// Note: format_price_smart is defined in the Indicator Drawing section below.
// It is also used by draw_sub_pane_price_scale (which uses its own inline format).

/// Draw series based on chart_type
pub fn draw_series(ctx: &mut dyn RenderContext, state: &ChartRenderState, chart_type: &str) {
    // Clip all chart types to chart_rect so lines/area/baseline don't
    // bleed onto the price scale, chrome toolbar, or neighboring windows.
    let chart = &state.chart_rect;
    ctx.save();
    if !state.disable_clip {
        ctx.begin_path();
        ctx.rect(chart.x, chart.y, chart.width, chart.height);
        ctx.clip();
    }

    match chart_type {
        "candles" => draw_candles(ctx, state),
        "bars" => draw_bars(ctx, state),
        "hollow_candles" => draw_hollow_candles(ctx, state),
        "heikin_ashi" => draw_heikin_ashi(ctx, state),
        "line" => draw_line_series(ctx, state),
        "step_line" => draw_step_line(ctx, state),
        "line_markers" => draw_line_with_markers(ctx, state),
        "area" => draw_area_series(ctx, state),
        "hlc_area" => draw_hlc_area(ctx, state),
        "baseline" => draw_baseline_series(ctx, state),
        "histogram" => draw_histogram(ctx, state),
        "columns" => draw_columns(ctx, state),
        _ => draw_candles(ctx, state), // Default fallback
    }

    ctx.restore();
}

// =============================================================================
// Chart Legend & Tooltip
// =============================================================================

/// Draw legend overlay showing OHLC values
///
/// This renders the legend in the corner of the chart showing symbol, timeframe,
/// and OHLC values for the bar under the cursor (or last bar if cursor not on chart).
pub fn draw_chart_legend(
    ctx: &mut dyn RenderContext,
    state: &ChartRenderState,
    symbol: Option<&str>,
    timeframe: &str,
) {
    if !state.legend.visible || state.bars.is_empty() {
        return;
    }

    // Get the bar to display (under cursor or last bar)
    let bar_idx = state.crosshair.bar_idx.unwrap_or(state.bars.len().saturating_sub(1));
    let bar = match state.bars.get(bar_idx) {
        Some(b) => b.clone(),
        None => return,
    };

    // Calculate change from previous bar
    let prev_bar = if bar_idx > 0 {
        state.bars.get(bar_idx - 1)
    } else {
        None
    };

    let (change, percent) = if let Some(prev) = prev_bar {
        let change = bar.close - prev.close;
        let percent = if prev.close != 0.0 {
            (change / prev.close) * 100.0
        } else {
            0.0
        };
        (change, percent)
    } else {
        // Use open to close change for first bar
        let change = bar.close - bar.open;
        let percent = if bar.open != 0.0 {
            (change / bar.open) * 100.0
        } else {
            0.0
        };
        (change, percent)
    };

    let is_bullish = bar.close >= bar.open;

    let legend_data = LegendData {
        symbol: symbol.unwrap_or("").to_string(),
        timeframe: timeframe.to_string(),
        bar,
        change,
        percent,
        is_bullish,
    };

    draw_legend(
        ctx,
        &state.chart_rect,
        state.legend,
        &legend_data,
        &state.theme.legend_value_up,
        &state.theme.legend_value_down,
    );
}

/// Draw tooltip overlay showing OHLC values near cursor
///
/// This renders a floating tooltip box near the cursor with OHLC values
/// for the bar under the cursor. Colors are taken from the chart theme.
pub fn draw_chart_tooltip(
    ctx: &mut dyn RenderContext,
    state: &ChartRenderState,
    tooltip: &Tooltip,
) {
    if !tooltip.visible || state.bars.is_empty() {
        return;
    }

    // Get the bar under cursor
    let bar_idx = match state.crosshair.bar_idx {
        Some(idx) => idx,
        None => return, // No bar under cursor
    };

    let bar = match state.bars.get(bar_idx) {
        Some(b) => b,
        None => return,
    };

    // Build tooltip content
    let content = TooltipLines::from_bar(bar);

    // Calculate position based on follow_cursor setting
    let (x, y) = if tooltip.follow_cursor {
        // Follow cursor position
        (state.crosshair.x + tooltip.offset_x, state.crosshair.y + tooltip.offset_y)
    } else {
        // Fixed position: top-left corner of chart
        (8.0, 8.0)
    };

    // Create themed tooltip - use theme colors instead of tooltip defaults
    let themed_tooltip = Tooltip {
        visible: tooltip.visible,
        follow_cursor: tooltip.follow_cursor,
        bar_idx: tooltip.bar_idx,
        x: tooltip.x,
        y: tooltip.y,
        content: tooltip.content.clone(),
        offset_x: tooltip.offset_x,
        offset_y: tooltip.offset_y,
        // Theme colors: background with alpha, text, border from scale_border
        background_color: format!("{}ee", &state.theme.background), // Add alpha
        text_color: state.theme.text.clone(),
        border_color: state.theme.scale_border.clone(),
        font_size: tooltip.font_size,
        padding: tooltip.padding,
    };

    draw_tooltip(
        ctx,
        &state.chart_rect,
        &themed_tooltip,
        &content,
        x,
        y,
    );
}

// =============================================================================
// Indicator Drawing
// =============================================================================

/// Format price smartly - removes trailing zeros after decimal point
/// Examples: 180.10 -> "180.1", 21323.00 -> "21323", 0.00123000 -> "0.00123"
/// For prices below 1e-8, uses scientific notation: 1.23e-10
pub fn format_price_smart(price: f64) -> String {
    // Scientific notation for extremely small prices
    if price != 0.0 && price.abs() < 1e-8 {
        let exp = price.abs().log10().floor() as i32;
        let mantissa = price / 10f64.powi(exp);
        return format!("{:.2}e{}", mantissa, exp);
    }

    let precision = if price >= 10000.0 {
        2
    } else if price >= 1000.0 {
        2
    } else if price >= 100.0 {
        3
    } else if price >= 1.0 {
        4
    } else if price >= 0.01 {
        6
    } else {
        8
    };

    let formatted = format!("{:.prec$}", price, prec = precision);

    if formatted.contains('.') {
        let trimmed = formatted.trim_end_matches('0');
        // Keep at least 2 decimal places (e.g. "87000.00", not "87000")
        let dot_pos = trimmed.find('.').unwrap();
        let decimals_len = trimmed.len() - dot_pos - 1;
        if decimals_len < 2 {
            format!("{:.2}", price)
        } else {
            trimmed.to_string()
        }
    } else {
        format!("{:.2}", price)
    }
}

/// Draw a single indicator line on the chart
pub fn draw_indicator_line(
    ctx: &mut dyn RenderContext,
    state: &ChartRenderState,
    values: &[f64],
    color: &str,
    line_width: f32,
) {
    let visible = state.viewport.visible_range();

    let (start_bar, end_bar) = if state.disable_clip {
        let extra_bars = (100.0 / state.viewport.bar_spacing).ceil() as usize;
        let start = (visible.0.max(0) as usize).saturating_sub(extra_bars);
        let end = ((visible.1 as usize) + extra_bars).min(values.len());
        (start, end)
    } else {
        (visible.0.max(0) as usize, (visible.1 as usize).min(values.len()))
    };

    if start_bar >= end_bar {
        return;
    }

    ctx.set_stroke_color(color);
    ctx.set_stroke_width(line_width as f64);
    ctx.begin_path();

    let mut started = false;
    for i in start_bar..end_bar {
        let value = values[i];
        if value.is_nan() || value.is_infinite() {
            started = false;
            continue;
        }

        let x = state.chart_rect.x + state.viewport.bar_to_x(i);
        let y = state.chart_rect.y + state.viewport.price_to_y(
            value,
            state.price_scale.price_min,
            state.price_scale.price_max,
        );

        if !started {
            ctx.move_to(x, y);
            started = true;
        } else {
            ctx.line_to(x, y);
        }
    }

    ctx.stroke();
}

/// Draw an indicator band (filled area between two lines)
pub fn draw_indicator_band(
    ctx: &mut dyn RenderContext,
    state: &ChartRenderState,
    upper: &[f64],
    lower: &[f64],
    fill_color: &str,
) {
    let visible = state.viewport.visible_range();
    let len = upper.len().min(lower.len());

    let (start_bar, end_bar) = if state.disable_clip {
        let extra_bars = (100.0 / state.viewport.bar_spacing).ceil() as usize;
        let start = (visible.0.max(0) as usize).saturating_sub(extra_bars);
        let end = ((visible.1 as usize) + extra_bars).min(len);
        (start, end)
    } else {
        (visible.0.max(0) as usize, (visible.1 as usize).min(len))
    };

    if start_bar >= end_bar {
        return;
    }

    ctx.set_fill_color(fill_color);
    ctx.begin_path();

    let mut started = false;
    for i in start_bar..end_bar {
        let value = upper[i];
        if value.is_nan() || value.is_infinite() {
            continue;
        }

        let x = state.chart_rect.x + state.viewport.bar_to_x(i);
        let y = state.chart_rect.y + state.viewport.price_to_y(
            value,
            state.price_scale.price_min,
            state.price_scale.price_max,
        );

        if !started {
            ctx.move_to(x, y);
            started = true;
        } else {
            ctx.line_to(x, y);
        }
    }

    for i in (start_bar..end_bar).rev() {
        let value = lower[i];
        if value.is_nan() || value.is_infinite() {
            continue;
        }

        let x = state.chart_rect.x + state.viewport.bar_to_x(i);
        let y = state.chart_rect.y + state.viewport.price_to_y(
            value,
            state.price_scale.price_min,
            state.price_scale.price_max,
        );

        ctx.line_to(x, y);
    }

    ctx.close_path();
    ctx.fill();
}

/// Draw volume overlay histogram at bottom 15% of chart
pub fn draw_volume_overlay(
    ctx: &mut dyn RenderContext,
    state: &ChartRenderState,
    values: &[f64],
    color_by_direction: bool,
    up_color: &str,
    down_color: &str,
) {
    let visible = state.viewport.visible_range();
    let len = values.len().min(state.bars.len());

    let (start, end) = if state.disable_clip {
        let extra_bars = (100.0 / state.viewport.bar_spacing).ceil() as usize;
        let s = (visible.0.max(0) as usize).saturating_sub(extra_bars);
        let e = ((visible.1 as usize) + extra_bars).min(len);
        (s, e)
    } else {
        (visible.0.max(0) as usize, (visible.1 as usize).min(len))
    };

    if start >= end {
        return;
    }

    let mut max_volume = 0.0f64;
    for i in start..end {
        let v = values[i];
        if !v.is_nan() && !v.is_infinite() && v > max_volume {
            max_volume = v;
        }
    }

    if max_volume <= 0.0 {
        return;
    }

    let volume_height = state.chart_rect.height * 0.15;
    let base_y = state.chart_rect.y + state.chart_rect.height;
    let bar_width = (state.viewport.bar_width() * 0.7).max(2.0);

    for i in start..end {
        let v = values[i];
        if v.is_nan() || v.is_infinite() {
            continue;
        }

        let x = state.chart_rect.x + state.viewport.bar_to_x(i);
        let ratio = v / max_volume;
        let bar_height = (ratio * volume_height).max(1.0);

        let color = if color_by_direction && i < state.bars.len() {
            if state.bars[i].is_bullish() { up_color } else { down_color }
        } else {
            up_color
        };

        ctx.set_fill_color(color);
        ctx.fill_rect(x - bar_width / 2.0, base_y - bar_height, bar_width, bar_height);
    }
}

/// Draw overlay indicators (pane == 0) on the main chart
///
/// Renders indicator lines on top of the candle/bar series.
/// Only draws overlay indicators (pane 0: SMA, EMA, Bollinger Bands, etc.).
pub fn draw_overlay_indicators(
    ctx: &mut dyn RenderContext,
    state: &ChartRenderState,
    indicator_source: &dyn crate::indicator_source::IndicatorSource,
    symbol: &str,
    selected_indicator_id: Option<u64>,
) {
    use crate::indicator_source::IndicatorOutputRenderType;

    // Clip overlay indicators to chart_rect so they don't bleed onto
    // the price scale, neighboring windows, or the chrome toolbar.
    let chart = &state.chart_rect;
    ctx.save();
    if !state.disable_clip {
        ctx.begin_path();
        ctx.rect(chart.x, chart.y, chart.width, chart.height);
        ctx.clip();
    }

    let instances = indicator_source.get_render_instances_for_symbol(symbol);

    for instance in &instances {
        if instance.pane != 0 || !instance.visible {
            continue;
        }

        if let Some(current_tf) = state.current_timeframe {
            if let Some(ref tf_config) = instance.timeframe_visibility {
                if !tf_config.is_visible_on_label(current_tf) {
                    continue;
                }
            }
        }

        let is_selected = selected_indicator_id == Some(instance.id);

        let mut marker_values: Vec<(&Vec<f64>, String)> = Vec::new();

        for output_def in &instance.output_defs {
            let output_config = instance.output_configs.get(&output_def.name);
            let is_visible = output_config.map(|c| c.visible).unwrap_or(true);
            if !is_visible {
                continue;
            }

            let values = match instance.values.get(&output_def.name) {
                Some(v) if !v.is_empty() => v,
                _ => continue,
            };

            let color = output_config
                .and_then(|c| c.color.as_ref())
                .map(|s| s.as_str())
                .unwrap_or(&output_def.color);

            let line_width = output_config
                .and_then(|c| c.line_width)
                .unwrap_or(output_def.line_width);

            match output_def.output_type {
                IndicatorOutputRenderType::Line => {
                    draw_indicator_line(ctx, state, values, color, line_width);
                    if is_selected {
                        marker_values.push((values, color.to_string()));
                    }
                }
                IndicatorOutputRenderType::Band => {
                    // handled separately for BB below
                }
                IndicatorOutputRenderType::Histogram => {
                    let color_by_direction = instance.bool_params
                        .get("color_by_direction")
                        .copied()
                        .unwrap_or(true);
                    let up_color = instance.color_params
                        .get("up_color")
                        .map(|s| s.as_str())
                        .unwrap_or("#26A69A80");
                    let down_color = instance.color_params
                        .get("down_color")
                        .map(|s| s.as_str())
                        .unwrap_or("#EF535080");
                    draw_volume_overlay(ctx, state, values, color_by_direction, up_color, down_color);
                }
                _ => {}
            }
        }

        // Special handling for Bollinger Bands (type_id == "bb")
        if instance.type_id == "bb" {
            if let (Some(upper), Some(lower)) = (
                instance.values.get("upper"),
                instance.values.get("lower"),
            ) {
                let fill_color = instance.color_params
                    .get("fill_color")
                    .map(|s| s.as_str())
                    .unwrap_or("rgba(33, 150, 243, 0.1)");
                draw_indicator_band(ctx, state, upper, lower, fill_color);
            }
        }

        if is_selected {
            for (values, color) in &marker_values {
                draw_indicator_selection_markers(ctx, state, values, color);
            }
        }

        if instance.signals_enabled && !instance.signals.is_empty() {
            let chart_rect = &state.chart_rect;
            let price_scale = state.price_scale;
            draw_indicator_signals(
                ctx,
                state,
                &instance.signals,
                chart_rect.x,
                chart_rect.y,
                chart_rect.height,
                price_scale.price_min,
                price_scale.price_max,
            );
        }
    }

    ctx.restore();
}

/// Draw selection markers (small circles) on an indicator line
pub fn draw_indicator_selection_markers(
    ctx: &mut dyn RenderContext,
    state: &ChartRenderState,
    values: &[f64],
    color: &str,
) {
    use std::f64::consts::TAU;

    let viewport = state.viewport;
    let price_scale = state.price_scale;
    let chart_rect = state.chart_rect;

    let visible = viewport.visible_range();

    let (visible_start, visible_end) = if state.disable_clip {
        let extra_bars = (100.0 / viewport.bar_spacing).ceil() as usize;
        let start = (visible.0 as usize).saturating_sub(extra_bars);
        let end = ((visible.1 as usize) + extra_bars).min(values.len());
        (start, end)
    } else {
        (visible.0 as usize, (visible.1 as usize).min(values.len()))
    };

    if visible_start >= visible_end {
        return;
    }

    let price_range = price_scale.price_max - price_scale.price_min;
    if price_range <= 0.0 {
        return;
    }

    let marker_interval = ((200.0 / viewport.bar_spacing).ceil() as usize).max(20);
    let marker_radius = 4.0;

    // Anchor markers to absolute bar positions that are multiples of marker_interval,
    // so panning does not shift them.
    let anchored_start = if visible_start % marker_interval == 0 {
        visible_start
    } else {
        visible_start + (marker_interval - visible_start % marker_interval)
    };

    ctx.set_fill_color(color);
    ctx.set_stroke_color("#1e222d");
    ctx.set_stroke_width(1.5);

    for i in (anchored_start..visible_end).step_by(marker_interval) {
        let value = values[i];
        if value.is_nan() || value.is_infinite() {
            continue;
        }

        let x = viewport.bar_to_x(i) + chart_rect.x;
        let y = chart_rect.y + ((price_scale.price_max - value) / price_range) * chart_rect.height;

        ctx.begin_path();
        ctx.arc(x, y, marker_radius, 0.0, TAU);
        ctx.fill();
        ctx.stroke();
    }
}

/// Draw signal markers for an indicator
///
/// Renders small triangles at bar positions where signals occurred.
pub fn draw_indicator_signals(
    ctx: &mut dyn RenderContext,
    state: &ChartRenderState,
    signals: &[crate::indicator_source::SignalRenderData],
    chart_x: f64,
    chart_y: f64,
    chart_height: f64,
    price_min: f64,
    price_max: f64,
) {
    use crate::ui::icons::{ICON_ARROW_UP, ICON_ARROW_DOWN};
    use crate::render::draw_svg_icon;

    if signals.is_empty() {
        return;
    }

    let viewport = state.viewport;
    let (visible_start, visible_end) = viewport.visible_range();
    let visible_start = visible_start as usize;
    let visible_end = visible_end as usize;

    let price_range = price_max - price_min;
    if price_range <= 0.0 {
        return;
    }

    let icon_size = 12.0;
    let offset = 4.0;

    let is_sub_pane = price_max <= 150.0 && price_min >= -150.0;

    for signal in signals {
        if signal.bar_index < visible_start || signal.bar_index > visible_end {
            continue;
        }

        let x = viewport.bar_to_x(signal.bar_index) + chart_x;
        let direction = signal.direction;
        let is_bullish = direction >= 0;

        let color = if direction > 0 { "#26a69a" } else if direction < 0 { "#ef5350" } else { "#2962ff" };

        let (icon, icon_y) = if is_sub_pane {
            if is_bullish {
                (ICON_ARROW_UP, chart_y + chart_height - icon_size - offset)
            } else {
                (ICON_ARROW_DOWN, chart_y + offset)
            }
        } else {
            let bar = state.bars.get(signal.bar_index);
            let (bar_high, bar_low) = bar
                .map(|b| (b.high, b.low))
                .unwrap_or((signal.price, signal.price));

            if is_bullish {
                let low_y = chart_y + ((price_max - bar_low) / price_range) * chart_height;
                (ICON_ARROW_UP, low_y + offset)
            } else {
                let high_y = chart_y + ((price_max - bar_high) / price_range) * chart_height;
                (ICON_ARROW_DOWN, high_y - icon_size - offset)
            }
        };

        draw_svg_icon(ctx, icon, x - icon_size / 2.0, icon_y, icon_size, icon_size, color);
    }
}

// =============================================================================
// Alert Lines
// =============================================================================

/// Draw alert level lines on the chart
///
/// Alert lines are horizontal dashed lines at the alert price level.
pub fn draw_alert_lines(
    ctx: &mut dyn RenderContext,
    state: &ChartRenderState,
    alert_items: &[crate::indicator_source::AlertRenderData],
) {
    use crate::indicator_source::AlertRenderStatus;
    use crate::chart::render::draw_styled_line;
    use crate::chart::annotations::LineStyle;

    let rect = &state.chart_rect;
    let price_scale = state.price_scale;
    let viewport = state.viewport;

    for alert in alert_items {
        let price = alert.price;

        if price < price_scale.price_min || price > price_scale.price_max {
            continue;
        }

        let y = viewport.price_to_y(price, price_scale.price_min, price_scale.price_max);
        let screen_y = rect.y + y;

        let (line_color, label_bg, label_text) = match alert.status {
            AlertRenderStatus::Active    => ("#FF9800", "#FF9800", "#000000"),
            AlertRenderStatus::Triggered => ("#4CAF50", "#4CAF50", "#FFFFFF"),
            AlertRenderStatus::Paused    => ("#9E9E9E", "#9E9E9E", "#FFFFFF"),
            AlertRenderStatus::Expired   => ("#757575", "#757575", "#FFFFFF"),
        };

        ctx.set_stroke_color(line_color);
        ctx.set_stroke_width(1.0);
        draw_styled_line(ctx, rect.x, screen_y, rect.right(), screen_y, &LineStyle::Dashed);

        let label_width = 60.0;
        let label_height = 16.0;
        let label_x = rect.right() - label_width;
        let label_y = screen_y - label_height / 2.0;

        ctx.set_fill_color(label_bg);
        ctx.fill_rect(label_x, label_y, label_width, label_height);

        ctx.set_fill_color(label_text);
        ctx.set_font("11px sans-serif");
        ctx.set_text_align(crate::render::TextAlign::Center);
        ctx.set_text_baseline(crate::render::TextBaseline::Middle);

        let price_str = format_price_smart(price);
        ctx.fill_text(&price_str, label_x + label_width / 2.0, screen_y);

        let icon_size = 12.0;
        let icon_x = rect.x + 4.0;

        ctx.set_fill_color(line_color);
        ctx.begin_path();
        ctx.arc(icon_x + icon_size / 2.0, screen_y, icon_size / 3.0, 0.0, std::f64::consts::TAU);
        ctx.fill();
    }
}

// =============================================================================
// Sub-Pane Drawing
// =============================================================================

/// Draw a line indicator in a sub-pane
pub fn draw_sub_pane_line(
    ctx: &mut dyn RenderContext,
    state: &ChartRenderState,
    values: &[f64],
    pane_x: f64,
    pane_y: f64,
    _pane_width: f64,
    pane_height: f64,
    pane_min: f64,
    pane_max: f64,
    color: &str,
    line_width: f32,
) {
    let visible = state.viewport.visible_range();

    let (start_bar, end_bar) = if state.disable_clip {
        let extra_bars = (100.0 / state.viewport.bar_spacing).ceil() as usize;
        let start = (visible.0.max(0) as usize).saturating_sub(extra_bars);
        let end = ((visible.1 as usize) + extra_bars).min(values.len());
        (start, end)
    } else {
        (visible.0.max(0) as usize, (visible.1 as usize).min(values.len()))
    };

    if start_bar >= end_bar {
        return;
    }

    ctx.set_stroke_color(color);
    ctx.set_stroke_width(line_width as f64);
    ctx.begin_path();

    let mut started = false;
    for i in start_bar..end_bar {
        let v = values[i];
        if v.is_nan() || v.is_infinite() {
            started = false;
            continue;
        }

        let x = pane_x + state.viewport.bar_to_x(i);
        let ratio = if pane_max > pane_min {
            (v - pane_min) / (pane_max - pane_min)
        } else {
            0.5
        };
        let y = pane_y + pane_height - (ratio * pane_height);

        if !started {
            ctx.move_to(x, y);
            started = true;
        } else {
            ctx.line_to(x, y);
        }
    }

    ctx.stroke();
}

/// Draw a histogram indicator in a sub-pane
pub fn draw_sub_pane_histogram(
    ctx: &mut dyn RenderContext,
    state: &ChartRenderState,
    values: &[f64],
    pane_x: f64,
    pane_y: f64,
    _pane_width: f64,
    pane_height: f64,
    pane_min: f64,
    pane_max: f64,
    color: &str,
    histogram_style: crate::indicator_source::HistogramStyle,
) {
    use crate::indicator_source::HistogramStyle;

    let visible = state.viewport.visible_range();

    let (start_bar, end_bar) = if state.disable_clip {
        let extra_bars = (100.0 / state.viewport.bar_spacing).ceil() as usize;
        let start = (visible.0.max(0) as usize).saturating_sub(extra_bars);
        let end = ((visible.1 as usize) + extra_bars).min(values.len());
        (start, end)
    } else {
        (visible.0.max(0) as usize, (visible.1 as usize).min(values.len()))
    };

    if start_bar >= end_bar {
        return;
    }

    let bar_width = (state.viewport.bar_width() * 0.5).max(2.0);

    match histogram_style {
        HistogramStyle::Centered => {
            let pane_range = pane_max - pane_min;
            if pane_range <= 0.0 {
                return;
            }

            // Map zero to pixel Y using pane_min/pane_max coordinate system.
            // Clamp zero to [pane_min, pane_max] so it stays inside the pane.
            let zero_clamped = 0.0_f64.clamp(pane_min, pane_max);
            let zero_ratio = (zero_clamped - pane_min) / pane_range;
            let zero_y = pane_y + pane_height - (zero_ratio * pane_height);

            for i in start_bar..end_bar {
                let v = values[i];
                if v.is_nan() || v.is_infinite() {
                    continue;
                }

                let x = pane_x + state.viewport.bar_to_x(i) - bar_width / 2.0;

                // Map value to pixel Y using pane_min/pane_max coordinate system.
                let val_ratio = (v - pane_min) / pane_range;
                let val_y = pane_y + pane_height - (val_ratio * pane_height);

                let bar_color = if v >= 0.0 { "#26A69A" } else { "#EF5350" };
                ctx.set_fill_color(bar_color);

                if v >= 0.0 {
                    let h = (zero_y - val_y).abs().max(1.0);
                    ctx.fill_rect(x, val_y, bar_width, h);
                } else {
                    let h = (val_y - zero_y).abs().max(1.0);
                    ctx.fill_rect(x, zero_y, bar_width, h);
                }
            }
        }
        HistogramStyle::FromBottom => {
            ctx.set_fill_color(color);

            for i in start_bar..end_bar {
                let v = values[i];
                if v.is_nan() || v.is_infinite() {
                    continue;
                }

                let x = pane_x + state.viewport.bar_to_x(i) - bar_width / 2.0;
                let ratio = if pane_max > pane_min {
                    (v - pane_min) / (pane_max - pane_min)
                } else {
                    0.0
                };
                let bar_height = ratio * pane_height;

                ctx.fill_rect(x, pane_y + pane_height - bar_height, bar_width, bar_height);
            }
        }
    }
}

/// Render a single sub-pane using its computed layout (with indicator content)
pub fn render_sub_pane(
    ctx: &mut dyn RenderContext,
    pane_layout: &SubPaneLayout,
    pane_index: usize,
    state: &ChartRenderState,
    indicator_source: &dyn crate::indicator_source::IndicatorSource,
    scale_theme: &ScaleTheme,
    scale_config: &ScaleConfig,
    crosshair_config: &CrosshairConfig,
    frame_theme: &FrameTheme,
    is_dragging: bool,
    drawing_manager: Option<&DrawingManager>,
    sub_pane_auto_scale: bool,
    sub_pane_price_min: f64,
    sub_pane_price_max: f64,
    selected_indicator_id: Option<u64>,
) {
    use crate::indicator_source::IndicatorOutputRenderType;

    let instance = match indicator_source.get_render_instance(pane_layout.instance_id) {
        Some(i) => i,
        None => return,
    };

    if !instance.visible {
        return;
    }

    // Timeframe visibility enforcement
    if let Some(current_tf) = state.current_timeframe {
        if let Some(ref tf_config) = instance.timeframe_visibility {
            if !tf_config.is_visible_on_label(current_tf) {
                return;
            }
        }
    }

    let content = &pane_layout.content;
    let price_scale_rect = &pane_layout.price_scale;
    let separator = &pane_layout.separator;

    // 1. Draw separator
    ctx.set_fill_color(&frame_theme.toolbar_border);
    ctx.fill_rect(separator.x, separator.y, separator.width, separator.height);

    // 2. Draw pane background
    ctx.draw_blur_background(content.x, content.y, content.width, content.height);
    ctx.set_fill_color(&state.theme.sub_pane_bg);
    ctx.fill_rect(content.x, content.y, content.width, content.height);

    // 3. Draw pane title
    ctx.set_font("10px sans-serif");
    ctx.set_fill_color(&state.theme.text);
    ctx.set_text_align(crate::render::TextAlign::Left);
    ctx.set_text_baseline(crate::render::TextBaseline::Top);
    ctx.fill_text(&instance.title, content.x + 8.0, content.y + 4.0);

    let (visible_start, visible_end_raw) = state.viewport.visible_range();
    let visible_end = visible_end_raw.min(state.bars.len());

    let (pane_min, pane_max) = if !sub_pane_auto_scale {
        (sub_pane_price_min, sub_pane_price_max)
    } else {
        indicator_source
            .calculate_pane_range(instance.id, visible_start as usize, visible_end)
            .unwrap_or((0.0, 100.0))
    };

    // 4. Draw grid lines
    draw_sub_pane_grid(ctx, content.x, content.y, content.width, content.height, &state.theme.grid_line);

    // 5. Draw indicator outputs
    ctx.save();
    if !state.disable_clip {
        ctx.begin_path();
        ctx.rect(content.x, content.y, content.width, content.height);
        ctx.clip();
    }

    for output_def in &instance.output_defs {
        let output_config = instance.output_configs.get(&output_def.name);
        if let Some(cfg) = output_config {
            if !cfg.visible {
                continue;
            }
        }

        let values = match instance.values.get(&output_def.name) {
            Some(v) if !v.is_empty() => v,
            _ => continue,
        };

        let color = output_config
            .and_then(|c| c.color.as_ref())
            .map(|s| s.as_str())
            .unwrap_or(&output_def.color);

        let line_width = output_config
            .and_then(|c| c.line_width)
            .unwrap_or(output_def.line_width);

        match output_def.output_type {
            IndicatorOutputRenderType::Line => {
                draw_sub_pane_line(
                    ctx, state, values,
                    content.x, content.y, content.width, content.height,
                    pane_min, pane_max,
                    color, line_width,
                );
            }
            IndicatorOutputRenderType::Histogram => {
                draw_sub_pane_histogram(
                    ctx, state, values,
                    content.x, content.y, content.width, content.height,
                    pane_min, pane_max,
                    color, instance.histogram_style,
                );
            }
            _ => {}
        }
    }

    // 5.5 Draw signals
    if instance.signals_enabled && !instance.signals.is_empty() {
        draw_indicator_signals(
            ctx,
            state,
            &instance.signals,
            content.x,
            content.y,
            content.height,
            pane_min,
            pane_max,
        );
    }

    ctx.restore();

    // 5.6 Draw selection markers if this sub-pane indicator is selected
    if selected_indicator_id == Some(pane_layout.instance_id) {
        use std::f64::consts::TAU;
        let viewport = state.viewport;
        let visible = viewport.visible_range();
        let (vis_s, vis_e) = (visible.0 as usize, (visible.1 as usize).min(state.bars.len()));
        let pane_range = pane_max - pane_min;
        if vis_s < vis_e && pane_range > 0.0 {
            let interval = ((200.0 / viewport.bar_spacing).ceil() as usize).max(20);
            let anchored = if vis_s % interval == 0 { vis_s } else { vis_s + (interval - vis_s % interval) };
            for output_def in &instance.output_defs {
                let output_config = instance.output_configs.get(&output_def.name);
                if let Some(cfg) = output_config {
                    if !cfg.visible { continue; }
                }
                let values = match instance.values.get(&output_def.name) {
                    Some(v) if !v.is_empty() => v,
                    _ => continue,
                };
                let color = output_config
                    .and_then(|c| c.color.as_ref())
                    .map(|s| s.as_str())
                    .unwrap_or(&output_def.color);
                ctx.set_fill_color(color);
                ctx.set_stroke_color("#1e222d");
                ctx.set_stroke_width(1.5);
                for i in (anchored..vis_e).step_by(interval) {
                    if i >= values.len() { break; }
                    let v = values[i];
                    if v.is_nan() || v.is_infinite() { continue; }
                    let mx = viewport.bar_to_x(i) + content.x;
                    let my = content.y + ((pane_max - v) / pane_range) * content.height;
                    ctx.begin_path();
                    ctx.arc(mx, my, 4.0, 0.0, TAU);
                    ctx.fill();
                    ctx.stroke();
                }
            }
        }
    }

    // 6. Draw price scale
    draw_sub_pane_price_scale(
        ctx,
        price_scale_rect.x,
        price_scale_rect.y,
        price_scale_rect.width,
        price_scale_rect.height,
        pane_min,
        pane_max,
        scale_theme,
        scale_config,
    );

    // 7. Draw horizontal crosshair line and price label on Y-axis
    let crosshair = state.crosshair;
    if crosshair.enabled && crosshair.visible && crosshair.pane_index == Some(pane_index) {
        let pane_rect = ChartRect {
            x: content.x,
            y: content.y,
            width: content.width,
            height: content.height,
        };

        draw_pane_crosshair(
            ctx,
            &pane_rect,
            crosshair.y,
            crosshair_config,
            &state.theme.crosshair,
            is_dragging,
        );

        // Draw price label on the sub-pane Y-axis at the crosshair position.
        draw_sub_pane_crosshair_price_label(
            ctx,
            crosshair.y,
            content.height,
            price_scale_rect.x,
            price_scale_rect.y,
            price_scale_rect.width,
            price_scale_rect.height,
            pane_min,
            pane_max,
            scale_theme,
            scale_config,
        );
    }

    // 8. Render drawing primitives
    if let Some(dm) = drawing_manager {
        render_sub_pane_primitives(
            ctx,
            content,
            state,
            dm,
            pane_layout.instance_id,
            pane_min,
            pane_max,
        );
    }
}

// =============================================================================
// Sub-Pane Drawing
// =============================================================================

/// Draw price scale (Y-axis labels) for a sub-pane
///
/// Uses same styling as main chart price scale:
/// - Centered text horizontally
/// - Same font size (11px)
/// - Same background and border colors
pub fn draw_sub_pane_price_scale(
    ctx: &mut dyn RenderContext,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    price_min: f64,
    price_max: f64,
    theme: &ScaleTheme,
    config: &ScaleConfig,
) {
    // Blur background (FrostedGlass/LiquidGlass) - draws before solid background
    ctx.draw_blur_background(x, y, width, height);

    // Draw background (semi-transparent when blur style is active)
    ctx.set_fill_color(&theme.scale_bg);
    ctx.fill_rect(x, y, width, height);

    // No separate border - will be drawn by content borders

    // Center text horizontally in price scale (same as main chart)
    let text_x = x + width / 2.0;

    // Generate price labels (fewer for small pane)
    let num_labels = if height > 80.0 { 3 } else { 2 };
    let price_range = price_max - price_min;

    ctx.set_font(&format!("{}px sans-serif", config.font_size));
    ctx.set_fill_color(&theme.scale_text);
    ctx.set_text_align(crate::render::TextAlign::Center);
    ctx.set_text_baseline(crate::render::TextBaseline::Middle);

    for i in 0..=num_labels {
        let ratio = i as f64 / num_labels as f64;
        let label_y = y + height - (ratio * height);

        // Skip labels too close to edges
        if label_y < y + 8.0 || label_y > y + height - 8.0 {
            continue;
        }

        let price = price_min + ratio * price_range;

        // Format price (use appropriate precision)
        let label = if price_range < 1.0 {
            format!("{:.4}", price)
        } else if price_range < 10.0 {
            format!("{:.2}", price)
        } else if price_range < 100.0 {
            format!("{:.1}", price)
        } else {
            format!("{:.0}", price)
        };

        ctx.fill_text(&label, text_x, label_y);
    }
}

/// Draw a crosshair price label on the sub-pane Y-axis.
///
/// Mirrors the main chart's crosshair price indicator in `draw_price_scale`,
/// but adapted for sub-pane coordinate space and compact inline price formatting.
///
/// # Parameters
/// - `crosshair_y` - Crosshair Y offset within the pane content rect (0 = top of pane)
/// - `pane_content_height` - Height of the pane content rect (used for bounds check)
/// - `scale_x / scale_y / scale_width / scale_height` - Price scale rect geometry
/// - `pane_min / pane_max` - Value range displayed in the pane
/// - `theme / config` - Scale theme and config (same as passed to draw_sub_pane_price_scale)
fn draw_sub_pane_crosshair_price_label(
    ctx: &mut dyn RenderContext,
    crosshair_y: f64,
    pane_content_height: f64,
    scale_x: f64,
    scale_y: f64,
    scale_width: f64,
    scale_height: f64,
    pane_min: f64,
    pane_max: f64,
    theme: &ScaleTheme,
    config: &ScaleConfig,
) {
    // Check that the crosshair is within the pane content bounds.
    if crosshair_y < 0.0 || crosshair_y > pane_content_height {
        return;
    }

    let pane_range = pane_max - pane_min;
    if pane_range <= 0.0 {
        return;
    }

    // Compute the price corresponding to the crosshair Y offset.
    // crosshair_y == 0 corresponds to pane_max, crosshair_y == pane_content_height corresponds to pane_min.
    let price = pane_max - (crosshair_y / pane_content_height) * pane_range;

    // Format price using the same precision logic as draw_sub_pane_price_scale.
    let label = if pane_range < 1.0 {
        format!("{:.4}", price)
    } else if pane_range < 10.0 {
        format!("{:.2}", price)
    } else if pane_range < 100.0 {
        format!("{:.1}", price)
    } else {
        format!("{:.0}", price)
    };

    // The label Y in screen space: scale_y is aligned with the pane content top.
    let screen_y = scale_y + crosshair_y;

    let label_width = scale_width - 2.0;
    let label_height = 20.0;
    let label_x = scale_x + 1.0;
    let label_y = screen_y - label_height / 2.0;

    // Clip to price scale rect so the label never overflows vertically.
    ctx.save();
    ctx.begin_path();
    ctx.rect(scale_x, scale_y, scale_width, scale_height);
    ctx.clip();

    // Draw blur background (for FrostedGlass/LiquidGlass styles).
    ctx.draw_blur_background(label_x, label_y, label_width, label_height);

    // Draw label background using crosshair label style.
    ctx.set_fill_color(&theme.crosshair_label_bg_styled);
    ctx.fill_rect(label_x, label_y, label_width, label_height);

    // Draw label text.
    ctx.set_font(&format!("{}px sans-serif", config.crosshair_font_size));
    ctx.set_fill_color(&theme.crosshair_label_text);
    ctx.set_text_align(crate::render::TextAlign::Center);
    ctx.set_text_baseline(crate::render::TextBaseline::Middle);
    ctx.fill_text(&label, scale_x + scale_width / 2.0, screen_y);

    ctx.restore();
}

/// Draw grid lines in a sub-pane
pub fn draw_sub_pane_grid(
    ctx: &mut dyn RenderContext,
    pane_x: f64,
    pane_y: f64,
    pane_width: f64,
    pane_height: f64,
    color: &str,
) {
    ctx.set_stroke_color(color);
    ctx.set_stroke_width(1.0);

    // Draw 3 horizontal grid lines
    let num_lines = 3;
    for i in 1..num_lines {
        let y = pane_y + pane_height * i as f64 / num_lines as f64;
        ctx.begin_path();
        ctx.move_to(pane_x, y);
        ctx.line_to(pane_x + pane_width, y);
        ctx.stroke();
    }
}

// =============================================================================
// Sub-Pane Base Rendering
// =============================================================================
// The chart library provides base sub-pane rendering (background, grid, scales,
// crosshair, primitives). Terminal adds indicator content on top.
//
// Functions provided by chart:
// - render_sub_pane_base() - background, separator, grid, price scale, crosshair
// - render_sub_pane_primitives() - drawing tools in sub-panes
// - draw_sub_pane_price_scale() - Y-axis labels
// - draw_sub_pane_grid() - horizontal grid lines
//
// Functions in terminal (indicator-specific):
// - draw_sub_pane_line
// - draw_sub_pane_histogram
// - render_sub_pane (complete sub-pane with indicator content)

/// Render the base parts of a sub-pane (background, separator, grid, price scale, crosshair)
///
/// This renders everything except indicator data. Terminal can call this and then
/// add indicator rendering on top using `pane_layout.content` for the content area.
pub fn render_sub_pane_base(
    ctx: &mut dyn RenderContext,
    pane_layout: &SubPaneLayout,
    pane_index: usize,
    state: &ChartRenderState,
    pane_min: f64,
    pane_max: f64,
    title: &str,
    scale_theme: &ScaleTheme,
    scale_config: &ScaleConfig,
    crosshair_config: &CrosshairConfig,
    frame_theme: &FrameTheme,
    is_dragging: bool,
) {
    let content = &pane_layout.content;
    let price_scale_rect = &pane_layout.price_scale;
    let separator = &pane_layout.separator;

    // 1. Draw separator (standard 1px border)
    ctx.set_fill_color(&frame_theme.toolbar_border);
    ctx.fill_rect(separator.x, separator.y, separator.width, separator.height);

    // 2. Draw pane background (with blur for FrostedGlass/LiquidGlass)
    ctx.draw_blur_background(content.x, content.y, content.width, content.height);
    ctx.set_fill_color(&state.theme.sub_pane_bg);
    ctx.fill_rect(content.x, content.y, content.width, content.height);

    // 3. Draw pane title
    ctx.set_font("10px sans-serif");
    ctx.set_fill_color(&state.theme.text);
    ctx.set_text_align(crate::render::TextAlign::Left);
    ctx.set_text_baseline(crate::render::TextBaseline::Top);
    ctx.fill_text(title, content.x + 8.0, content.y + 4.0);

    // 4. Draw grid lines
    draw_sub_pane_grid(ctx, content.x, content.y, content.width, content.height, &state.theme.grid_line);

    // 5. Draw price scale for this pane
    draw_sub_pane_price_scale(
        ctx,
        price_scale_rect.x,
        price_scale_rect.y,
        price_scale_rect.width,
        price_scale_rect.height,
        pane_min,
        pane_max,
        scale_theme,
        scale_config,
    );

    // 6. Draw horizontal crosshair line if cursor is in this pane
    let crosshair = state.crosshair;
    if crosshair.enabled && crosshair.visible && crosshair.pane_index == Some(pane_index) {
        let pane_rect = ChartRect {
            x: content.x,
            y: content.y,
            width: content.width,
            height: content.height,
        };

        let y_position = crosshair.y;

        draw_pane_crosshair(
            ctx,
            &pane_rect,
            y_position,
            crosshair_config,
            &state.theme.crosshair,
            is_dragging,
        );

        // Draw price label on the sub-pane Y-axis at the crosshair position.
        draw_sub_pane_crosshair_price_label(
            ctx,
            y_position,
            content.height,
            price_scale_rect.x,
            price_scale_rect.y,
            price_scale_rect.width,
            price_scale_rect.height,
            pane_min,
            pane_max,
            scale_theme,
            scale_config,
        );
    }
}

/// Helper function to render drawing primitives for a sub-pane
pub fn render_sub_pane_primitives(
    ctx: &mut dyn RenderContext,
    content: &LayoutRect,
    state: &ChartRenderState,
    dm: &DrawingManager,
    instance_id: u64,
    pane_min: f64,
    pane_max: f64,
) {
    if !dm.is_visible() {
        return;
    }

    // Guard: skip rendering primitives when coordinate space is not yet valid.
    // This prevents glitches when switching tabs (presets) before bars arrive.
    if state.viewport.bar_count == 0 {
        return;
    }
    let pane_range = pane_max - pane_min;
    if pane_range <= 0.0 || !pane_range.is_finite() {
        return;
    }

    // Save context state
    ctx.save();

    // Set coordinate space for this sub-pane's viewport and price range
    // Uses the same X-axis (bar) as main chart, but different Y-axis (pane's value range)
    ctx.set_coordinate_space(
        content.width,
        content.height,
        state.viewport.view_start,
        state.viewport.bar_spacing,
        pane_min,
        pane_max,
    );

    // Clip to content area (skip for Glass styles)
    if !state.disable_clip {
        ctx.begin_path();
        ctx.rect(content.x, content.y, content.width, content.height);
        ctx.clip();
    }

    // Translate to content origin
    ctx.translate(content.x, content.y);

    // Create a viewport for this pane (same X-axis, different height)
    let pane_viewport = crate::Viewport {
        view_start: state.viewport.view_start,
        bar_spacing: state.viewport.bar_spacing,
        bar_width_ratio: state.viewport.bar_width_ratio,
        chart_width: content.width,
        chart_height: content.height,
        bar_count: state.viewport.bar_count,
    };

    // Create a price scale for this pane
    let pane_price_scale = crate::PriceScale {
        price_min: pane_min,
        price_max: pane_max,
        ..Default::default()
    };

    // Render primitives that belong to this sub-pane (matching instance_id)
    let sorted_indices = dm.primitives_sorted_by_z_order();
    let selected_idx = dm.selected();

    for idx in sorted_indices {
        if let Some(prim) = dm.primitives().get(idx) {
            let data = prim.data();

            // Only render primitives that belong to this sub-pane
            if data.pane_id != Some(instance_id) {
                continue;
            }

            // Filter by timeframe visibility
            if let Some(current_tf) = state.current_timeframe {
                if let Some(ref tf_config) = data.timeframe_visibility {
                    if !tf_config.is_visible_on_label(current_tf) {
                        continue;
                    }
                }
            }

            let is_selected = selected_idx == Some(idx);
            prim.render(ctx, is_selected);

            // Draw control points for selected primitive
            if is_selected {
                let control_points = prim.control_points(
                    &pane_viewport,
                    &pane_price_scale,
                );
                let screen_points: Vec<(f64, f64)> = control_points
                    .iter()
                    .map(|cp| (cp.x, cp.y))
                    .collect();
                draw_control_points(ctx, &screen_points);
            }
        }
    }

    // Render drawing preview for this sub-pane
    if dm.is_drawing() && dm.current_pane() == Some(instance_id) {
        let cursor_bar = state.crosshair.bar_f64;
        let cursor_price = state.crosshair.price;

        // For freehand tools (brush/highlighter), draw the accumulated points as a live stroke
        if dm.is_freehand_tool() {
            if let Some(points) = dm.drawing_points() {
                if points.len() >= 2 {
                    let is_highlighter = dm.current_tool() == Some("highlighter");
                    let effective_color = dm.effective_color();
                    let stroke_color = if is_highlighter {
                        apply_opacity(&effective_color, 0.4)
                    } else {
                        effective_color
                    };
                    let stroke_width = if is_highlighter { 20.0 } else { 3.0 };

                    ctx.set_stroke_color(&stroke_color);
                    ctx.set_stroke_width(stroke_width);
                    ctx.set_line_cap("round");
                    ctx.set_line_join("round");

                    let screen_pts: Vec<(f64, f64)> = points.iter()
                        .map(|&(bar, price)| {
                            let x = pane_viewport.bar_to_x_f64(bar);
                            let y = pane_price_scale.price_to_y(price, content.height);
                            (x, y)
                        })
                        .collect();

                    ctx.begin_path();
                    ctx.move_to(screen_pts[0].0, screen_pts[0].1);

                    if screen_pts.len() == 2 {
                        ctx.line_to(screen_pts[1].0, screen_pts[1].1);
                    } else {
                        let mid_x = (screen_pts[0].0 + screen_pts[1].0) / 2.0;
                        let mid_y = (screen_pts[0].1 + screen_pts[1].1) / 2.0;
                        ctx.line_to(mid_x, mid_y);

                        for i in 1..screen_pts.len() - 1 {
                            let next_mid_x = (screen_pts[i].0 + screen_pts[i + 1].0) / 2.0;
                            let next_mid_y = (screen_pts[i].1 + screen_pts[i + 1].1) / 2.0;
                            ctx.quadratic_curve_to(screen_pts[i].0, screen_pts[i].1, next_mid_x, next_mid_y);
                        }

                        let last = screen_pts.last().unwrap();
                        ctx.line_to(last.0, last.1);
                    }
                    ctx.stroke();
                }
            }
        } else {
            // For non-freehand tools, use standard preview
            if let Some(preview_prim) = dm.create_preview(cursor_bar, cursor_price) {
                preview_prim.render(ctx, false);


                // Draw anchor points for multi-click tools
                if let Some(points) = dm.drawing_points() {
                    ctx.set_fill_color("#ffff00");
                    ctx.set_stroke_color("#000000");
                    ctx.set_stroke_width(1.0);

                    for (bar, price) in points {
                        let x = pane_viewport.bar_to_x_f64(*bar);
                        let y = pane_price_scale.price_to_y(*price, content.height);

                        ctx.begin_path();
                        ctx.arc(x, y, 5.0, 0.0, std::f64::consts::TAU);
                        ctx.fill();
                        ctx.begin_path();
                        ctx.arc(x, y, 5.0, 0.0, std::f64::consts::TAU);
                        ctx.stroke();
                    }
                }
            }
        }
    }

    // Restore context state
    ctx.restore();
}

/// Render drawing primitives for the main chart area (pane_id == None).
///
/// This is the chart-crate equivalent of `render_window_primitives` in core.
/// It is used by `render_chart_splits` to draw primitives for each sub-chart
/// leaf so that drawings created in split views are visible.
///
/// # Arguments
/// * `ctx`           – Render context for drawing operations
/// * `chart_area`    – Layout rectangles for this sub-chart
/// * `state`         – Render state (viewport, price scale, crosshair, etc.)
/// * `dm`            – Drawing manager holding the primitives
pub fn render_main_chart_primitives(
    ctx: &mut dyn RenderContext,
    chart_area: &ChartAreaLayout,
    state: &ChartRenderState,
    dm: &DrawingManager,
) {
    // Debug: log once when primitives exist but might be skipped
    let prim_count = dm.primitives().len();
    if prim_count > 0 {
        static LOGGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if !LOGGED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            eprintln!("[RENDER_PRIMS] {} primitives, visible={}, bar_count={}, price_range=[{:.12e}..{:.12e}]",
                prim_count, dm.is_visible(), state.viewport.bar_count,
                state.price_scale.price_min, state.price_scale.price_max);
        }
    }

    if !dm.is_visible() {
        return;
    }

    // Guard: skip rendering primitives when coordinate space is not yet valid.
    // This prevents glitches when switching tabs (presets) before bars arrive.
    if state.viewport.bar_count == 0 {
        return;
    }
    let price_range = state.price_scale.price_max - state.price_scale.price_min;
    if price_range <= 0.0 || !price_range.is_finite() {
        return;
    }

    let chart = &chart_area.chart;

    // Save context state
    ctx.save();

    // Set coordinate space for this window's viewport and price scale.
    // Use chart layout dimensions to ensure primitives align with crosshair.
    ctx.set_coordinate_space(
        chart.width,
        chart.height,
        state.viewport.view_start,
        state.viewport.bar_spacing,
        state.price_scale.price_min,
        state.price_scale.price_max,
    );

    // Clip to chart area
    if !state.disable_clip {
        ctx.begin_path();
        ctx.rect(chart.x, chart.y, chart.width, chart.height);
        ctx.clip();
    }

    // Translate to chart origin
    ctx.translate(chart.x, chart.y);

    // Build a viewport using the layout chart dimensions so control-point
    // calculations match the coordinate space set above.
    let layout_vp = crate::Viewport {
        chart_width: chart.width,
        chart_height: chart.height,
        ..*state.viewport
    };

    // Render main chart primitives sorted by z-order (only those with pane_id == None)
    let sorted_indices = dm.primitives_sorted_by_z_order();
    let selected_idx = dm.selected();

    for idx in sorted_indices {
        if let Some(prim) = dm.primitives().get(idx) {
            let data = prim.data();

            // Only render primitives that belong to the main chart (pane_id == None)
            if data.pane_id.is_some() {
                continue;
            }

            // Filter by timeframe visibility
            if let Some(current_tf) = state.current_timeframe {
                if let Some(ref tf_config) = data.timeframe_visibility {
                    if !tf_config.is_visible_on_label(current_tf) {
                        continue;
                    }
                }
            }

            // Viewport culling: skip primitives whose coordinates are entirely
            // outside the visible area.  This prevents million-pixel-wide lines
            // when a primitive was drawn at a price/bar far from the current view
            // (e.g. a line at $40,000 while the chart shows $5).
            //
            // The check uses generous margins (±100 bars, ×10 / ×0.1 price) so
            // lines that merely start outside but cross into the viewport are
            // never accidentally culled.
            let points = prim.points();
            if !points.is_empty() {
                let view_start = state.viewport.view_start;
                let view_end   = view_start + state.viewport.visible_bars() as f64;
                let price_min  = state.price_scale.price_min;
                let price_max  = state.price_scale.price_max;

                let all_left  = points.iter().all(|(bar, _)| *bar < view_start - 100.0);
                let all_right = points.iter().all(|(bar, _)| *bar > view_end   + 100.0);
                let all_above = points.iter().all(|(_, price)| *price > price_max * 10.0);
                let all_below = points.iter().all(|(_, price)| *price < price_min * 0.1);

                if all_left || all_right || all_above || all_below {
                    continue;
                }
            }

            let is_selected = selected_idx == Some(idx);
            prim.render(ctx, is_selected);

            // Draw control points for selected primitive
            if is_selected {
                let control_points = prim.control_points(&layout_vp, state.price_scale);
                let screen_points: Vec<(f64, f64)> = control_points
                    .iter()
                    .map(|cp| (cp.x, cp.y))
                    .collect();
                draw_control_points(ctx, &screen_points);
            }
        }
    }

    // Render drawing preview
    if dm.is_drawing() {
        // Snap to bar centre (matching crosshair coordinate system)
        let cursor_bar = if let Some(idx) = state.crosshair.bar_idx {
            idx as f64
        } else {
            state.crosshair.bar_f64
        };
        // Use snapped price in magnet mode so preview matches where primitive will be placed
        let cursor_price = state.crosshair.effective_price(false);

        // For freehand tools (brush/highlighter), draw the accumulated stroke live
        if dm.is_freehand_tool() {
            if let Some(points) = dm.drawing_points() {
                if points.len() >= 2 {
                    let is_highlighter = dm.current_tool() == Some("highlighter");
                    let effective_color = dm.effective_color();
                    let stroke_color = if is_highlighter {
                        apply_opacity(&effective_color, 0.4)
                    } else {
                        effective_color
                    };
                    let stroke_width = if is_highlighter { 20.0 } else { 3.0 };

                    ctx.set_stroke_color(&stroke_color);
                    ctx.set_stroke_width(stroke_width);
                    ctx.set_line_cap("round");
                    ctx.set_line_join("round");

                    let screen_pts: Vec<(f64, f64)> = points
                        .iter()
                        .map(|&(bar, price)| {
                            let x = state.viewport.bar_to_x_f64(bar);
                            let y = state.price_scale.price_to_y(price, chart.height);
                            (x, y)
                        })
                        .collect();

                    ctx.begin_path();
                    ctx.move_to(screen_pts[0].0, screen_pts[0].1);

                    if screen_pts.len() == 2 {
                        ctx.line_to(screen_pts[1].0, screen_pts[1].1);
                    } else {
                        let mid_x = (screen_pts[0].0 + screen_pts[1].0) / 2.0;
                        let mid_y = (screen_pts[0].1 + screen_pts[1].1) / 2.0;
                        ctx.line_to(mid_x, mid_y);

                        for i in 1..screen_pts.len() - 1 {
                            let next_mid_x = (screen_pts[i].0 + screen_pts[i + 1].0) / 2.0;
                            let next_mid_y = (screen_pts[i].1 + screen_pts[i + 1].1) / 2.0;
                            ctx.quadratic_curve_to(
                                screen_pts[i].0,
                                screen_pts[i].1,
                                next_mid_x,
                                next_mid_y,
                            );
                        }

                        let last = screen_pts.last().unwrap();
                        ctx.line_to(last.0, last.1);
                    }
                    ctx.stroke();
                }
            }
        } else {
            // For non-freehand tools, use standard preview
            if let Some(preview_prim) = dm.create_preview(cursor_bar, cursor_price) {
                preview_prim.render(ctx, false);

                // Draw anchor points for multi-click tools (not freehand)
                if let Some(points) = dm.drawing_points() {
                    ctx.set_fill_color("#ffff00");
                    ctx.set_stroke_color("#000000");
                    ctx.set_stroke_width(1.0);

                    for (bar, price) in points {
                        let x = state.viewport.bar_to_x_f64(*bar);
                        let y = state.price_scale.price_to_y(*price, chart.height);

                        ctx.begin_path();
                        ctx.arc(x, y, 5.0, 0.0, std::f64::consts::TAU);
                        ctx.fill();
                        ctx.begin_path();
                        ctx.arc(x, y, 5.0, 0.0, std::f64::consts::TAU);
                        ctx.stroke();
                    }
                }
            }
        }
    }

    // Restore context state
    ctx.restore();
}

// =============================================================================
// Main Chart Rendering
// =============================================================================

/// Render only the chart area (without scales)
///
/// This renders:
/// 1. Background
/// 2. Grid
/// 3. Series (candles, bars, line, area, etc.)
/// 4. Crosshair
///
/// Used internally by render_chart_window.
pub fn render_chart(
    ctx: &mut dyn RenderContext,
    layout: &ChartAreaLayout,
    state: &ChartRenderState,
    config: &ChartRenderConfig,
) {
    // 1. Background
    ctx.set_fill_color(&state.theme.background);
    ctx.fill_rect(layout.chart.x, layout.chart.y, layout.chart.width, layout.chart.height);

    // 2. Grid
    draw_grid(ctx, state);

    // 3. Series (candles, bars, line, area, etc.)
    draw_series(ctx, state, config.chart_type);

    // 4. Crosshair (on chart area)
    // For simple charts (no sub-panes), vertical line spans just the chart
    let total_chart_top = layout.chart.y;
    let total_chart_bottom = layout.chart.bottom();
    draw_crosshair(ctx, state, &config.crosshair_config, config.is_dragging, total_chart_top, total_chart_bottom);
}

/// Render complete chart window (chart + scales + corner)
///
/// This renders a full chart window:
/// 1. Chart area (via render_chart)
/// 2. Price scale
/// 3. Time scale
/// 4. Scale corner with interactive buttons
///
/// Returns hit zones for scale corner buttons.
pub fn render_chart_window(
    ctx: &mut dyn RenderContext,
    layout: &ChartAreaLayout,
    state: &ChartRenderState,
    config: &ChartRenderConfig,
    corner_state: &ScaleCornerState,
) -> ScaleCornerHitZones {
    // 1. Render chart area (background, grid, series, crosshair)
    render_chart(ctx, layout, state, config);

    // 2. Price scale
    draw_price_scale(
        ctx,
        state,
        &config.scale_config,
        &config.scale_theme,
        layout.price_scale.x,
        layout.price_scale.y,
    );

    // 3. Time scale
    draw_time_scale(
        ctx,
        state,
        &config.scale_config,
        &config.scale_theme,
        layout.time_scale.x,
        layout.time_scale.y,
    );

    // 4. Scale corner with buttons
    draw_scale_corner_with_buttons(ctx, &layout.scale_corner, &config.scale_theme, corner_state)
}

/// Draw the scale corner with buttons and return hit zones
///
/// The scale corner is seamlessly integrated with price and time scales.
/// It only contains 2 centered buttons (A/M toggle and mode) without
/// separate borders - the scales already provide visual continuity.
///
/// Returns hit zones that can be used to detect clicks on buttons.
pub fn draw_scale_corner_with_buttons(
    ctx: &mut dyn RenderContext,
    rect: &LayoutRect,
    theme: &ScaleTheme,
    state: &ScaleCornerState,
) -> ScaleCornerHitZones {
    // Blur background (FrostedGlass/LiquidGlass) - draws before solid background
    ctx.draw_blur_background(rect.x, rect.y, rect.width, rect.height);

    // Background - same as scale background for seamless integration
    // The scales already draw their borders, so we don't need any here
    ctx.set_fill_color(&theme.scale_bg);
    ctx.fill_rect(rect.x, rect.y, rect.width, rect.height);

    // Button layout - 2 centered buttons
    let spacing = 4.0;
    let am_width = 14.0;
    let mode_width = 20.0;
    let total_width = am_width + spacing + mode_width;
    let start_x = rect.center_x() - total_width / 2.0;

    // A/M button rect
    let am_rect = LayoutRect::new(start_x, rect.y, am_width, rect.height);

    // Mode button rect
    let mode_rect = LayoutRect::new(start_x + am_width + spacing, rect.y, mode_width, rect.height);

    // Draw A/M label
    let am_label = state.scale_mode.short_label();
    let am_color = if state.am_hovered {
        &theme.scale_text
    } else {
        &theme.scale_text_muted
    };
    ctx.set_fill_color(am_color);
    ctx.set_font("12px sans-serif");
    ctx.fill_text_centered(am_label, am_rect.center_x(), am_rect.center_y());

    // Draw mode label
    let mode_color = if state.mode_hovered {
        &theme.scale_text
    } else {
        &theme.scale_text_muted
    };
    ctx.set_fill_color(mode_color);
    ctx.fill_text_centered(&state.mode_label, mode_rect.center_x(), mode_rect.center_y());

    ScaleCornerHitZones {
        am_button: am_rect,
        mode_button: mode_rect,
    }
}

/// Draw just the scale corner (intersection of price and time scales)
/// Simple version without interactive buttons
pub fn draw_scale_corner(
    ctx: &mut dyn RenderContext,
    rect: &LayoutRect,
    theme: &ScaleTheme,
) {
    draw_scale_corner_with_buttons(ctx, rect, theme, &ScaleCornerState::default());
}

/// Draw chart background only (for custom rendering)
pub fn draw_chart_background(
    ctx: &mut dyn RenderContext,
    layout: &ChartAreaLayout,
    theme: &ChartTheme,
) {
    ctx.set_fill_color(&theme.background);
    ctx.fill_rect(layout.chart.x, layout.chart.y, layout.chart.width, layout.chart.height);
}

/// Render scales only (price + time + corner)
pub fn render_scales(
    ctx: &mut dyn RenderContext,
    layout: &ChartAreaLayout,
    state: &ChartRenderState,
    config: &ChartRenderConfig,
) {
    draw_price_scale(
        ctx,
        state,
        &config.scale_config,
        &config.scale_theme,
        layout.price_scale.x,
        layout.price_scale.y,
    );

    draw_time_scale(
        ctx,
        state,
        &config.scale_config,
        &config.scale_theme,
        layout.time_scale.x,
        layout.time_scale.y,
    );

    draw_scale_corner(ctx, &layout.scale_corner, &config.scale_theme);
}

// =============================================================================
// Frame Rendering
// =============================================================================

/// Render complete frame with chart
///
/// This renders the chart within its frame layout:
/// 1. Chart area with candles, scales, crosshair
/// 2. Scale corner with buttons
///
/// Toolbar rendering is the terminal's responsibility - use `draw_toolbar_backgrounds`
/// with explicit toolbar rects if needed.
///
/// Returns hit zones for scale corner buttons.
pub fn render_frame(
    ctx: &mut dyn RenderContext,
    layout: &FrameLayout,
    chart_state: &ChartRenderState,
    chart_config: &ChartRenderConfig,
    corner_state: &ScaleCornerState,
    _frame_theme: &FrameTheme,
) -> ScaleCornerHitZones {
    // Render chart window (chart + scales + corner)
    render_chart_window(
        ctx,
        &layout.chart_area,
        chart_state,
        chart_config,
        corner_state,
    )
}

// =============================================================================
// Legacy Aliases
// =============================================================================

/// Legacy alias for render_chart_window
#[deprecated(note = "Use render_chart_window instead")]
pub fn render_chart_area_with_buttons(
    ctx: &mut dyn RenderContext,
    layout: &ChartAreaLayout,
    state: &ChartRenderState,
    config: &ChartRenderConfig,
    corner_state: &ScaleCornerState,
) -> ScaleCornerHitZones {
    render_chart_window(ctx, layout, state, config, corner_state)
}

/// Legacy alias - renders chart window with simple (non-interactive) corner
#[deprecated(note = "Use render_chart_window instead")]
pub fn render_chart_area(
    ctx: &mut dyn RenderContext,
    layout: &ChartAreaLayout,
    state: &ChartRenderState,
    config: &ChartRenderConfig,
) {
    // Render chart
    render_chart(ctx, layout, state, config);

    // Render scales
    draw_price_scale(
        ctx,
        state,
        &config.scale_config,
        &config.scale_theme,
        layout.price_scale.x,
        layout.price_scale.y,
    );

    draw_time_scale(
        ctx,
        state,
        &config.scale_config,
        &config.scale_theme,
        layout.time_scale.x,
        layout.time_scale.y,
    );

    // Simple corner (no buttons)
    draw_scale_corner(ctx, &layout.scale_corner, &config.scale_theme);
}

// =============================================================================
// Border Drawing (moved from core to break circular dependency)
// =============================================================================

/// Draw separator lines between chart area and its scales.
///
/// Draws a 1px horizontal line between chart and time scale, and a 1px vertical
/// line between chart and price scale.  Respects `FrameTheme::show_scale_separators`
/// to optionally extend separators through the scale corner.
pub fn draw_content_borders(
    ctx: &mut dyn RenderContext,
    layout: &ChartAreaLayout,
    theme: &FrameTheme,
) {
    let chart = &layout.chart;
    let price_scale = &layout.price_scale;
    let time_scale = &layout.time_scale;

    ctx.set_fill_color(&theme.toolbar_border);

    // Determine relative positions
    let price_on_left = price_scale.x < chart.x;
    let time_on_top = time_scale.y < chart.y;

    let vert_x = if price_on_left {
        chart.x - 1.0
    } else {
        chart.x + chart.width
    };

    let horz_y = if time_on_top {
        chart.y - 1.0
    } else {
        chart.y + chart.height
    };

    if theme.show_scale_separators {
        let horz_x = chart.x.min(price_scale.x);
        let horz_w = chart.width + price_scale.width;
        ctx.fill_rect(horz_x, horz_y, horz_w, 1.0);
        let vert_y = chart.y.min(time_scale.y);
        let vert_h = chart.height + time_scale.height;
        ctx.fill_rect(vert_x, vert_y, 1.0, vert_h);
    } else {
        ctx.fill_rect(chart.x, horz_y, chart.width, 1.0);
        ctx.fill_rect(vert_x, chart.y, 1.0, chart.height);
    }
}

/// Draw a simple 1px stroke border around a rectangle.
///
/// Used when sub-panes are present, replacing the per-scale separator lines
/// with a single outer frame border.
pub fn draw_frame_borders(
    ctx: &mut dyn RenderContext,
    rect: &LayoutRect,
    theme: &FrameTheme,
) {
    ctx.set_stroke_color(&theme.toolbar_border);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rect(rect.x, rect.y, rect.width, rect.height);
}

// =============================================================================
// Full Chart Panel Rendering
// =============================================================================

/// Complete render data for a single chart panel.
///
/// This is the chart crate's equivalent of core's `ChartWindowRenderData`.
/// It bundles everything `render_full_chart_panel` needs, using only types
/// that live in (or are re-exported by) the chart crate, so core is not
/// required as a dependency.
pub struct ChartPanelRenderData<'a> {
    /// Chart render state (viewport, bars, theme, etc.)
    pub state: &'a ChartRenderState<'a>,
    /// Render configuration (chart type, scale config/theme, crosshair config)
    pub config: &'a ChartRenderConfig,
    /// State for the scale-corner A/M and mode buttons
    pub corner_state: &'a ScaleCornerState,
    /// Drawing manager for interactive primitives (optional)
    pub drawing_manager: Option<&'a DrawingManager>,
    /// Indicator source for overlay indicators and sub-panes (optional)
    pub indicator_source: Option<&'a dyn crate::indicator_source::IndicatorSource>,
    /// Symbol string used to filter indicators (optional)
    pub symbol: Option<&'a str>,
    /// Per-sub-pane Y-axis states for manual zoom/pan (optional)
    pub sub_panes: Option<&'a [SubPane]>,
    /// Symbol comparison overlay (optional)
    pub compare_overlay: Option<&'a CompareOverlay>,
    /// Watermark displayed behind chart content (optional)
    pub watermark: Option<&'a crate::chart::Watermark>,
    /// Tooltip displayed near the cursor (optional)
    pub tooltip: Option<&'a Tooltip>,
    /// Alert price levels to render as horizontal lines
    pub alert_render_data: &'a [crate::indicator_source::AlertRenderData],
    /// Scale visibility / position / corner settings
    pub scale_settings: &'a ScaleSettings,
    /// Currently selected indicator ID (for selection highlighting, optional)
    pub selected_indicator_id: Option<u64>,
    /// Frame theme for borders and toolbar backgrounds
    pub frame_theme: &'a FrameTheme,
    /// Toolbar configuration used to carve the content rect from `window_rect`.
    ///
    /// `render_full_chart_panel` receives the full panel rect (including toolbar
    /// areas) as `window_rect` and uses this config to compute the content rect
    /// internally.  Pass `&ToolbarConfig::minimal()` when the caller has already
    /// stripped toolbar space from the rect it passes in.
    pub toolbar_config: &'a crate::panel_app::ToolbarConfig,
}

/// Render a complete chart panel — background, grid, series, scales, indicators,
/// alerts, drawing primitives, sub-panes, crosshair, and borders.
///
/// This function is the chart crate's self-contained replacement for the
/// terminal-assembled rendering that used to live in
/// `core::layout::render_frame::render_single_chart_panel`.
///
/// The caller is responsible for:
/// 1. Computing the `content_rect` (full panel rect after carving out toolbars).
/// 2. Rendering toolbars **after** this call so they appear on top of the chart.
///
/// # Returns
///
/// [`ScaleCornerHitZones`] for hit-testing clicks on the A/M and mode buttons.
pub fn render_full_chart_panel(
    ctx: &mut dyn RenderContext,
    window_rect: &LayoutRect,
    data: &ChartPanelRenderData,
) -> ScaleCornerHitZones {
    // Skip rendering if the panel is too small (e.g. collapsed during expand)
    if window_rect.width < 2.0 || window_rect.height < 2.0 {
        return ScaleCornerHitZones::default();
    }

    // Collect sub-pane instance IDs so layout can allocate vertical space.
    let sub_pane_ids: Vec<u64> = if let (Some(im), Some(symbol)) =
        (data.indicator_source, data.symbol)
    {
        im.get_instances_for_symbol(symbol)
            .into_iter()
            .filter(|i| i.visible && i.pane_index > 0)
            .map(|i| i.id)
            .collect()
    } else {
        Vec::new()
    };

    // Compute the panel layout by carving toolbar rects from window_rect.
    // The content_rect is the area available for chart rendering after subtracting
    // toolbar space.  All scale/grid/series rendering uses content_rect so that
    // price/time scales don't overlap with toolbars.
    let panel_layout = crate::panel_app::ChartPanelLayout::compute(window_rect, data.toolbar_config);
    let content_rect = &panel_layout.content_rect;

    // Build per-pane heights from the SubPane list (height_ratio encodes user-set sizes).
    let sub_pane_heights: Vec<f64> = if let Some(panes) = data.sub_panes {
        crate::layout::sub_pane_heights_from_panes(panes, content_rect.height, 100.0)
    } else {
        crate::layout::default_sub_pane_heights(sub_pane_ids.len(), 100.0)
    };

    let extended_layout = crate::layout::ExtendedFrameLayout::compute_from_chart_panel(
        content_rect,
        &sub_pane_ids,
        data.scale_settings,
        &sub_pane_heights,
        1.0, // separator_height
    );
    let main_chart = &extended_layout.main_chart;

    // Bug 2 fix: update viewport dimensions to match the computed layout so
    // that bar_to_x() and price_to_y() operate in the correct coordinate space.
    // Without this, stale defaults (e.g. 800x400 from construction) are used.
    let mut corrected_viewport = data.state.viewport.clone();
    corrected_viewport.chart_width = main_chart.chart.width;
    corrected_viewport.chart_height = main_chart.chart.height;

    // Bug 3 fix: build a ChartRect that matches the layout we just computed so
    // that all draw calls (series, overlays, scales, …) use the correct origin.
    let corrected_chart_rect = crate::chart::render::ChartRect {
        x: main_chart.chart.x,
        y: main_chart.chart.y,
        width: main_chart.chart.width,
        height: main_chart.chart.height,
    };

    // Assemble a corrected render state referencing the local corrected values.
    let corrected_state = crate::chart::render::ChartRenderState {
        viewport: &corrected_viewport,
        chart_rect: corrected_chart_rect,
        price_scale: data.state.price_scale,
        time_scale: data.state.time_scale,
        bars: data.state.bars,
        grid: data.state.grid,
        crosshair: data.state.crosshair,
        legend: data.state.legend,
        theme: data.state.theme,
        time_ticks: data.state.time_ticks,
        current_timeframe: data.state.current_timeframe,
        disable_clip: data.state.disable_clip,
        time_format_settings: data.state.time_format_settings,
        timeframe_minutes: data.state.timeframe_minutes,
        scale_settings: data.state.scale_settings,
        body_enabled: data.state.body_enabled,
        border_enabled: data.state.border_enabled,
        wick_enabled: data.state.wick_enabled,
        use_prev_close: data.state.use_prev_close,
    };

    // 1. Background — fill the full window rect to avoid gaps when scales are
    //    repositioned.
    ctx.set_fill_color(&corrected_state.theme.background);
    ctx.fill_rect(window_rect.x, window_rect.y, window_rect.width, window_rect.height);

    // Loading state — when there are no bars yet (e.g. after a symbol or
    // timeframe switch), show a centred "Loading..." label and skip all
    // chart content so primitives don't render over an empty canvas.
    if corrected_state.bars.is_empty() {
        let cx = corrected_chart_rect.x + corrected_chart_rect.width / 2.0;
        let cy = corrected_chart_rect.y + corrected_chart_rect.height / 2.0;
        ctx.set_fill_color(&corrected_state.theme.text);
        ctx.set_font("14px sans-serif");
        ctx.set_text_align(crate::render::TextAlign::Center);
        ctx.set_text_baseline(crate::render::TextBaseline::Middle);
        ctx.fill_text("Loading...", cx, cy);
        return ScaleCornerHitZones::default();
    }

    // 2. Grid — extended variant covers the content area (under scales, excluding toolbars).
    draw_grid_extended(ctx, &corrected_state, content_rect);

    // 3. Series (candles, bars, line, etc.)
    draw_series(ctx, &corrected_state, data.config.chart_type);

    // 4. Last-price projection line.
    draw_last_price_line(ctx, &corrected_state);

    // 5. Watermark (behind other overlays).
    if let Some(watermark) = data.watermark {
        draw_watermark(ctx, &corrected_state.chart_rect, watermark);
    }

    // 6. Compare overlay (symbol comparisons).
    if let Some(compare_overlay) = data.compare_overlay {
        draw_compare_overlay(ctx, &corrected_state, compare_overlay);
    }

    // 7. Tooltip (near cursor).
    if let Some(tooltip) = data.tooltip {
        draw_chart_tooltip(ctx, &corrected_state, tooltip);
    }

    // NOTE: Crosshair is drawn AFTER sub-panes so the vertical line is not
    // covered by sub-pane backgrounds.

    // 8. Price scale (skip when hidden).
    if data.scale_settings.price_scale_position.is_visible() {
        draw_price_scale(
            ctx,
            &corrected_state,
            &data.config.scale_config,
            &data.config.scale_theme,
            main_chart.price_scale.x,
            main_chart.price_scale.y,
        );
    }

    // 9. Overlay indicators (pane == 0).
    if let (Some(im), Some(symbol)) = (data.indicator_source, data.symbol) {
        draw_overlay_indicators(ctx, &corrected_state, im, symbol, data.selected_indicator_id);
    }

    // 10. Alert level lines.
    if !data.alert_render_data.is_empty() {
        draw_alert_lines(ctx, &corrected_state, data.alert_render_data);
    }

    // 11. Drawing primitives for the main chart.
    if let Some(dm) = data.drawing_manager {
        render_main_chart_primitives(ctx, main_chart, &corrected_state, dm);
    }

    // 12. Sub-panes (indicators with pane_index > 0).
    if let Some(im) = data.indicator_source {
        for (pane_idx, pane_layout) in extended_layout.sub_panes.iter().enumerate() {
            let sub_pane_state = data.sub_panes.and_then(|panes| panes.get(pane_idx));
            let (auto_scale, price_min, price_max) = match sub_pane_state {
                Some(sp) => (sp.auto_scale, sp.price_min, sp.price_max),
                None => (true, 0.0, 100.0),
            };
            render_sub_pane(
                ctx,
                pane_layout,
                pane_idx,
                &corrected_state,
                im,
                &data.config.scale_theme,
                &data.config.scale_config,
                &data.config.crosshair_config,
                data.frame_theme,
                data.config.is_dragging,
                data.drawing_manager,
                auto_scale,
                price_min,
                price_max,
                data.selected_indicator_id,
            );
        }
    }

    // 13. Crosshair — spans all chart content (main + sub-panes) but not time scale.
    let total_chart_top = main_chart.chart.y;
    let total_chart_bottom = if extended_layout.sub_panes.is_empty() {
        main_chart.chart.y + main_chart.chart.height
    } else {
        let last_pane = extended_layout.sub_panes.last().unwrap();
        last_pane.content.y + last_pane.content.height
    };
    draw_crosshair(
        ctx,
        &corrected_state,
        &data.config.crosshair_config,
        data.config.is_dragging,
        total_chart_top,
        total_chart_bottom,
    );

    // 14. Time scale (skip when hidden).
    if data.scale_settings.time_scale_position.is_visible() {
        draw_time_scale(
            ctx,
            &corrected_state,
            &data.config.scale_config,
            &data.config.scale_theme,
            main_chart.time_scale.x,
            main_chart.time_scale.y,
        );
    }

    // 15. Scale corner with A/M and mode buttons.
    let show_corner = data.scale_settings.corner_visibility.should_show(false)
        && data.scale_settings.price_scale_position.is_visible()
        && data.scale_settings.time_scale_position.is_visible();

    let corner_zones = if show_corner {
        draw_scale_corner_with_buttons(
            ctx,
            &main_chart.scale_corner,
            &data.config.scale_theme,
            data.corner_state,
        )
    } else {
        ScaleCornerHitZones::default()
    };

    // 16. Content/frame borders.
    // Draw separator lines that span the full chart height (main + sub-panes).
    {
        let chart = &main_chart.chart;
        let price_scale = &main_chart.price_scale;
        let time_scale = &main_chart.time_scale;

        ctx.set_fill_color(&data.frame_theme.toolbar_border);

        // Vertical separator between chart content and price scale.
        // Extends from top of main chart to bottom of last sub-pane (or main chart),
        // and through the time scale.
        let price_on_left = price_scale.x < chart.x;
        let vert_x = if price_on_left {
            chart.x - 1.0
        } else {
            chart.x + chart.width
        };
        let vert_top = chart.y.min(time_scale.y);
        let vert_bottom = if time_scale.y + time_scale.height > 0.0 {
            time_scale.y + time_scale.height
        } else {
            total_chart_bottom
        };
        ctx.fill_rect(vert_x, vert_top, 1.0, vert_bottom - vert_top);

        // Horizontal separator between chart content (+ sub-panes) and time scale.
        // Sits at the bottom of all chart content, spanning full width.
        let horz_y = total_chart_bottom;
        let horz_x = chart.x.min(price_scale.x);
        let horz_w = chart.width + price_scale.width;
        ctx.fill_rect(horz_x, horz_y, horz_w, 1.0);

        // Sub-pane separator lines (between each sub-pane and its price scale).
        for sub_pane in &extended_layout.sub_panes {
            ctx.fill_rect(vert_x, sub_pane.separator.y, 1.0, sub_pane.separator.height);
        }
    }

    corner_zones
}

// =============================================================================
// Split Chart Rendering
// =============================================================================

/// Render all sub-charts managed by a `ChartPanelGrid` within `area`.
///
/// This is the entry point for chart-internal split rendering.  Core calls this
/// when a chart panel has active sub-splits, passing in its own
/// `ChartPanelGrid` and the content rectangle allocated to the panel.
///
/// For each leaf in the split tree, this function calls `render_full_chart_panel`
/// so that scales, hit zones, and theme colors match the normal single-window
/// rendering path exactly.
///
/// **Layout must be pre-computed**: call `panel_grid.layout(area)` before
/// calling this function.  The panel rects from the last `layout()` call are
/// used directly without recomputing them, so this function accepts an
/// immutable reference.
pub fn render_chart_splits(
    ctx: &mut dyn RenderContext,
    panel_grid: &crate::state::ChartPanelGrid,
    area: LayoutRect,
    theme: &ChartTheme,
    config: &ChartRenderConfig,
    frame_theme: &FrameTheme,
) {
    // Snapshot leaf → rect pairs (uses pre-computed rects from last layout() call).
    let leaf_rects: Vec<_> = panel_grid
        .panel_rects()
        .iter()
        .map(|(&leaf_id, &sub_rect)| (leaf_id, sub_rect))
        .collect();

    for (leaf_id, sub_rect) in leaf_rects {
        let window = match panel_grid.window_for_leaf(leaf_id) {
            Some(w) => w,
            None => continue,
        };

        // Convert sub_rect (f32, 0,0-based relative) to absolute LayoutRect (f64)
        // by adding the content area offset so rendering hits the correct screen position.
        let available = LayoutRect {
            x: area.x + sub_rect.x as f64,
            y: area.y + sub_rect.y as f64,
            width: sub_rect.width as f64,
            height: sub_rect.height as f64,
        };

        // Clip rendering to this sub-window's allocated rectangle so that
        // primitives and candles do not bleed across sub-window boundaries.
        ctx.save();
        ctx.begin_path();
        ctx.rect(available.x, available.y, available.width, available.height);
        ctx.clip();

        // Build a minimal ChartRenderState from the ChartWindow's data.
        // render_full_chart_panel will correct the viewport dimensions and
        // chart_rect internally, so we pass the raw values here.
        let render_state = ChartRenderState {
            viewport: &window.viewport,
            price_scale: &window.price_scale,
            time_scale: &window.time_scale,
            bars: &window.bars,
            grid: &window.grid_options,
            crosshair: &window.crosshair,
            legend: &window.legend,
            chart_rect: ChartRect {
                x: available.x,
                y: available.y,
                width: available.width,
                height: available.height,
            },
            theme,
            time_ticks: None,
            current_timeframe: Some(&window.timeframe.name),
            disable_clip: false,
            time_format_settings: Some(&window.scale_settings.time_format),
            timeframe_minutes: Some(window.timeframe.minutes),
            scale_settings: Some(&window.scale_settings),
            body_enabled: true,
            border_enabled: true,
            wick_enabled: true,
            use_prev_close: false,
        };

        // Build a config for this leaf, overriding chart_type from the window.
        // All other config values (scale_theme, scale_config, crosshair_config)
        // come from the caller's config so colors match the main rendering path.
        let leaf_config = ChartRenderConfig {
            chart_type: window.chart_type,
            ..config.clone()
        };

        let corner_state = window.to_corner_state();

        let panel_data = ChartPanelRenderData {
            state: &render_state,
            config: &leaf_config,
            corner_state: &corner_state,
            drawing_manager: Some(&window.drawing_manager),
            indicator_source: None,
            symbol: None,
            sub_panes: None,
            compare_overlay: None,
            watermark: None,
            tooltip: None,
            alert_render_data: &[],
            scale_settings: &window.scale_settings,
            selected_indicator_id: None,
            frame_theme,
            // Split sub-windows have no toolbars — the caller already allocated
            // the full sub-rect to chart content.  Pass minimal so that
            // render_full_chart_panel computes layout from the full rect.
            toolbar_config: &window.toolbar_config,
        };

        render_full_chart_panel(ctx, &available, &panel_data);

        // Restore the clip region saved at the start of this sub-window iteration.
        ctx.restore();
    }

    // Draw separators on top of rendered sub-charts.
    //
    // We use a subtle border color matching the chart theme's scale_border so
    // the dividers look integrated with the chart style.  The visual thickness
    // is 2 px when idle (the separator's `thickness_for_state()`); the wider
    // 8 px hit area is invisible — only the 2 px stripe is painted.
    for sep in panel_grid.docking().separators() {
        let thickness = sep.thickness_for_state() as f64;
        // Separator positions are (0,0)-based (relative to content area).
        // Add content area offset to convert to absolute screen coordinates.
        let (rx, ry, rw, rh) = match sep.orientation {
            SeparatorOrientation::Vertical => {
                // Vertical divider: a thin vertical rectangle.
                let x = area.x + sep.position as f64 - thickness / 2.0;
                let y = area.y + sep.start as f64;
                (x, y, thickness, sep.length as f64)
            }
            SeparatorOrientation::Horizontal => {
                // Horizontal divider: a thin horizontal rectangle.
                let x = area.x + sep.start as f64;
                let y = area.y + sep.position as f64 - thickness / 2.0;
                (x, y, sep.length as f64, thickness)
            }
        };

        // Color: use a state-aware color — brighter on hover/drag, subtle when idle.
        let color = match sep.state {
            uzor::panels::SeparatorState::Idle => &theme.scale_border,
            uzor::panels::SeparatorState::Hover | uzor::panels::SeparatorState::Dragging => {
                &theme.crosshair
            }
        };

        ctx.set_fill_color(color);
        ctx.fill_rect(rx, ry, rw, rh);
    }
}
