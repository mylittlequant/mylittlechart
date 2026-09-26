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

use mlc_core::render::RenderContext;
use crate::chart_render::{
    ChartRenderState, ChartTheme, ChartRect,
    draw_grid, draw_grid_extended, draw_data_gaps, draw_candles, draw_volume_profile_overlay,
    draw_tpo_profile_overlay,
    draw_dom_heatmap_conditional, draw_liquidation_levels_conditional,
    draw_liquidation_projection_conditional,
    draw_liquidation_prints_conditional,
    draw_price_scale, draw_time_scale,
    draw_price_scale_cursor_label, draw_time_scale_cursor_label,
    draw_crosshair, draw_pane_crosshair,
    draw_legend, draw_tooltip, LegendData, TooltipLines,
    draw_watermark, draw_last_price_line, draw_compare_overlay,
    ScaleConfig, ScaleTheme, CrosshairConfig,
};
use mlc_core::chart::Tooltip;
use mlc_core::chart::types::compare::CompareOverlay;
use mlc_core::chart::types::pane_scale::{cull_axis_labels, sub_pane_ticks, sub_pane_value_range, AxisLabel};
use mlc_core::drawing::{DrawingRenderView, draw_control_points};
use mlc_core::apply_opacity;
use mlc_core::scale_settings::ScaleSettings;
use mlc_core::state::sub_pane_y_state::SubPaneYState;
use crate::ui::icons::Icon;
use crate::ui::modal_settings::SubPaneOverlayState;
use super::rects::{ChartAreaLayout, FrameLayout, LayoutRect, SubPaneLayout};
use mlc_core::state::sub_pane_overlay::SubPaneOverlayResult;

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
// Scale Corner — types live in state::scale_corner, re-exported here.
// =============================================================================

pub use mlc_core::state::scale_corner::{ScaleCornerState, ScaleCornerHitZones, ScaleCornerButton};

/// Result returned by [`render_full_chart_panel`].
#[derive(Clone, Debug)]
pub struct ChartPanelRenderResult {
    /// Hit zones for the scale corner A/M and mode buttons.
    pub corner_zones: ScaleCornerHitZones,
    /// Computed sub-pane ranges for auto-scaled panes: `(pane_index, min, max)`.
    ///
    /// Only populated for panes where `auto_scale == true`.  Callers should
    /// write these values back to the stored [`SubPane`] so that the stored
    /// range always matches what was actually rendered (symmetrization + padding
    /// already applied).
    pub sub_pane_ranges: Vec<(usize, f64, f64)>,
    /// Sub-pane overlay hit zones for button interaction.
    pub sub_pane_overlays: Vec<SubPaneOverlayResult>,
    /// Loading spinner geometry: `Some((cx, cy, radius))` when this panel is
    /// currently in the loading state (bars empty, no data_error). Renderer
    /// composites the spinner as an OVERLAY layer (not inside chart-scene) so
    /// it animates independently and is retired by a host-level scene-ready
    /// signal — not by the bars.is_empty() check the layout layer makes here.
    pub loading_spinner: Option<(f64, f64, f64)>,
}

impl ChartPanelRenderResult {
    fn empty() -> Self {
        Self {
            corner_zones: ScaleCornerHitZones::default(),
            sub_pane_ranges: Vec::new(),
            sub_pane_overlays: Vec::new(),
            loading_spinner: None,
        }
    }
}

// =============================================================================
// Frame Theme
// =============================================================================

/// Theme for frame rendering (toolbars + borders)
#[derive(Clone, Debug)]
pub struct FrameTheme {
    /// Toolbar background color
    pub toolbar_bg: String,
    /// Modal / popup / overlay chrome fill. Glass styles this with alpha so
    /// the chart shows through; toolbars stay on `toolbar_bg`.
    pub modal_bg: String,
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
            modal_bg: "#131722".to_string(),
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

    // chart_type → draw fn dispatch: static DrawOps table (Step 9 of the
    // chart-type registry arc), replacing the former `match chart_type` block.
    // See `crate::chart_type::draw_ops` for the table and per-type notes
    // (López de Prado bars falling through to draw_candles, dom/liquidation
    // heatmap conditional-on-ring-buffer draw fns).
    match crate::chart_type::draw_ops_by_id(chart_type) {
        Some(ops) => (ops.draw)(ctx, state),
        None => draw_candles(ctx, state), // Default fallback for unknown chart_type
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
        Some(b) => *b,
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

    let precision = if price >= 1000.0 {
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
        let start = visible.0.max(0).saturating_sub(extra_bars);
        let end = (visible.1 + extra_bars).min(values.len());
        (start, end)
    } else {
        (visible.0.max(0), visible.1.min(values.len()))
    };

    if start_bar >= end_bar {
        return;
    }

    ctx.set_stroke_color(color);
    ctx.set_stroke_width(line_width as f64);
    ctx.begin_path();

    let mut started = false;
    for (i, &value) in values.iter().enumerate().take(end_bar).skip(start_bar) {
        if value.is_nan() || value.is_infinite() {
            started = false;
            continue;
        }

        let bar_ts_ms = state.bars.get(i).map(|b| b.ts_ms()).unwrap_or(0);
        let x = state.chart_rect.x + state.viewport.x_for_bar(i, bar_ts_ms);
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
        let start = visible.0.max(0).saturating_sub(extra_bars);
        let end = (visible.1 + extra_bars).min(len);
        (start, end)
    } else {
        (visible.0.max(0), visible.1.min(len))
    };

    if start_bar >= end_bar {
        return;
    }

    ctx.set_fill_color(fill_color);
    ctx.begin_path();

    let mut started = false;
    for (i, &value) in upper.iter().enumerate().take(end_bar).skip(start_bar) {
        if value.is_nan() || value.is_infinite() {
            continue;
        }

        let bar_ts_ms = state.bars.get(i).map(|b| b.ts_ms()).unwrap_or(0);
        let x = state.chart_rect.x + state.viewport.x_for_bar(i, bar_ts_ms);
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

        let bar_ts_ms = state.bars.get(i).map(|b| b.ts_ms()).unwrap_or(0);
        let x = state.chart_rect.x + state.viewport.x_for_bar(i, bar_ts_ms);
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
        let s = visible.0.max(0).saturating_sub(extra_bars);
        let e = (visible.1 + extra_bars).min(len);
        (s, e)
    } else {
        (visible.0.max(0), visible.1.min(len))
    };

    if start >= end {
        return;
    }

    let mut max_volume = 0.0f64;
    for &v in values.iter().take(end).skip(start) {
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

    for (i, &v) in values.iter().enumerate().take(end).skip(start) {
        if v.is_nan() || v.is_infinite() {
            continue;
        }

        let bar_ts_ms = state.bars.get(i).map(|b| b.ts_ms()).unwrap_or(0);
        let x = state.chart_rect.x + state.viewport.x_for_bar(i, bar_ts_ms);
        let ratio = v / max_volume;
        let bar_height = (ratio * volume_height).max(1.0);

        let color = if color_by_direction && i < state.bars.len() {
            if state.bars[i].is_bullish() { up_color } else { down_color }
        } else {
            up_color
        };

        let (bx, by, bw, bh) = (x - bar_width / 2.0, base_y - bar_height, bar_width, bar_height);
        // Volume bars wear the skin's texture layer like the candles do — a
        // glow-on skin that lights the bodies but leaves the volume flat
        // reads as two different charts stacked. Shared ring set with the
        // candle glow (`chart_render::draw_glow_rings`) so the two can't
        // drift out of sync.
        if state.theme.candle_glow {
            crate::chart_render::draw_glow_rings(ctx, color, bx, by, bw, bh);
        }
        ctx.set_fill_color(color);
        ctx.fill_rect(bx, by, bw, bh);
    }
}

/// Draw overlay indicators (pane == 0) on the main chart
///
/// Renders indicator lines on top of the candle/bar series.
/// Only draws overlay indicators (pane 0: SMA, EMA, Bollinger Bands, etc.).
pub fn draw_overlay_indicators(
    ctx: &mut dyn RenderContext,
    state: &ChartRenderState,
    indicator_source: &dyn mlc_core::indicator_source::IndicatorSource,
    symbol: &str,
    selected_indicator_id: Option<u64>,
) {
    use mlc_core::indicator_source::IndicatorOutputRenderType;

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
                    // The skin's own volume colours. This used to paste a
                    // `80` alpha suffix onto the candle colour, i.e. hard-code
                    // 50% transparency — the bars read as washed-out next to
                    // the candles and the panels the same skin paints.
                    let up_fallback = state.theme.volume_up.clone();
                    let down_fallback = state.theme.volume_down.clone();
                    let up_color = instance.color_params
                        .get("up_color")
                        .map(|s| s.as_str())
                        .unwrap_or(up_fallback.as_str());
                    let down_color = instance.color_params
                        .get("down_color")
                        .map(|s| s.as_str())
                        .unwrap_or(down_fallback.as_str());
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
                &instance.signal_display,
                chart_rect.x,
                chart_rect.y,
                chart_rect.height,
                price_scale.price_min,
                price_scale.price_max,
                false, // main chart — icons pin near each bar's high/low
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
        let start = (visible.0).saturating_sub(extra_bars);
        let end = (visible.1 + extra_bars).min(values.len());
        (start, end)
    } else {
        (visible.0, visible.1.min(values.len()))
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

        let bar_ts_ms = state.bars.get(i).map(|b| b.ts_ms()).unwrap_or(0);
        let x = viewport.x_for_bar(i, bar_ts_ms) + chart_rect.x;
        let y = chart_rect.y + ((price_scale.price_max - value) / price_range) * chart_rect.height;

        ctx.begin_path();
        ctx.arc(x, y, marker_radius, 0.0, TAU);
        ctx.fill();
        ctx.stroke();
    }
}

/// Draw signal markers for an indicator
///
/// Renders shaped markers at bar positions where signals occurred.
/// The visual appearance is controlled by `signal_display`.
pub fn draw_indicator_signals(
    ctx: &mut dyn RenderContext,
    state: &ChartRenderState,
    signals: &[mlc_core::indicator_source::SignalRenderData],
    signal_display: &mlc_core::indicator_source::SignalDisplayConfig,
    chart_x: f64,
    chart_y: f64,
    chart_height: f64,
    price_min: f64,
    price_max: f64,
    // Explicit flag set by caller — true when rendering into a sub-pane (icons
    // pin to top/bottom of the pane rect), false when on the main price chart
    // (icons pin near each bar's high/low).  Replaces a magic-number heuristic
    // (`price_max <= 150.0 && price_min >= -150.0`) that misclassified
    // sub-panes with large cumulative ranges (AD, OBV) as if they were the
    // main chart, sending arrow icons off-screen.
    is_in_sub_pane: bool,
) {
    use mlc_core::indicator_source::SignalShape;

    if signals.is_empty() {
        return;
    }

    let viewport = state.viewport;
    let (visible_start, visible_end) = viewport.visible_range();

    let price_range = price_max - price_min;
    if price_range <= 0.0 {
        return;
    }

    let icon_size = signal_display.size;
    let offset = signal_display.offset;

    let is_sub_pane = is_in_sub_pane;

    for signal in signals {
        if signal.bar_index < visible_start || signal.bar_index > visible_end {
            continue;
        }

        let bar_ts_ms = state.bars.get(signal.bar_index).map(|b| b.ts_ms()).unwrap_or(0);
        let x = viewport.x_for_bar(signal.bar_index, bar_ts_ms) + chart_x;
        let direction = signal.direction;
        let is_bullish = direction >= 0;

        let color: &str = if direction > 0 {
            &signal_display.bullish_color
        } else if direction < 0 {
            &signal_display.bearish_color
        } else {
            &signal_display.neutral_color
        };

        // Compute icon_y position (outside shape match so it is shared)
        let icon_y = if is_sub_pane {
            if is_bullish {
                chart_y + chart_height - icon_size - offset
            } else {
                chart_y + offset
            }
        } else {
            let bar = state.bars.get(signal.bar_index);
            let (bar_high, bar_low) = bar
                .map(|b| (b.high, b.low))
                .unwrap_or((signal.price, signal.price));

            if is_bullish {
                let low_y = chart_y + ((price_max - bar_low) / price_range) * chart_height;
                low_y + offset
            } else {
                let high_y = chart_y + ((price_max - bar_high) / price_range) * chart_height;
                high_y - icon_size - offset
            }
        };

        match signal_display.shape {
            SignalShape::Arrow => {
                // Real arrow with stem + triangular head
                let half = icon_size / 2.0;
                let stem_w = icon_size * 0.15; // stem thickness
                let head_h = icon_size * 0.45; // arrowhead height
                let head_w = half;             // arrowhead half-width
                ctx.set_fill_color(color);
                ctx.begin_path();
                if is_bullish {
                    // Arrow pointing UP: tip at top
                    let tip_y = icon_y;
                    let base_y = icon_y + icon_size;
                    let head_base_y = tip_y + head_h;
                    // Arrowhead
                    ctx.move_to(x, tip_y);
                    ctx.line_to(x + head_w, head_base_y);
                    ctx.line_to(x + stem_w, head_base_y);
                    // Stem
                    ctx.line_to(x + stem_w, base_y);
                    ctx.line_to(x - stem_w, base_y);
                    ctx.line_to(x - stem_w, head_base_y);
                    // Other side of arrowhead
                    ctx.line_to(x - head_w, head_base_y);
                } else {
                    // Arrow pointing DOWN: tip at bottom
                    let tip_y = icon_y + icon_size;
                    let top_y = icon_y;
                    let head_base_y = tip_y - head_h;
                    // Stem top
                    ctx.move_to(x - stem_w, top_y);
                    ctx.line_to(x + stem_w, top_y);
                    ctx.line_to(x + stem_w, head_base_y);
                    // Arrowhead
                    ctx.line_to(x + head_w, head_base_y);
                    ctx.line_to(x, tip_y);
                    ctx.line_to(x - head_w, head_base_y);
                    ctx.line_to(x - stem_w, head_base_y);
                }
                ctx.close_path();
                ctx.fill();
            }
            SignalShape::Triangle => {
                ctx.set_fill_color(color);
                ctx.begin_path();
                if is_bullish {
                    // Upward-pointing triangle
                    ctx.move_to(x, icon_y);
                    ctx.line_to(x - icon_size / 2.0, icon_y + icon_size);
                    ctx.line_to(x + icon_size / 2.0, icon_y + icon_size);
                } else {
                    // Downward-pointing triangle
                    ctx.move_to(x - icon_size / 2.0, icon_y);
                    ctx.line_to(x + icon_size / 2.0, icon_y);
                    ctx.line_to(x, icon_y + icon_size);
                }
                ctx.close_path();
                ctx.fill();
            }
            SignalShape::Circle => {
                ctx.set_fill_color(color);
                let r = icon_size / 2.0;
                let cx = x;
                let cy = icon_y + r;
                ctx.begin_path();
                ctx.arc(cx, cy, r, 0.0, std::f64::consts::TAU);
                ctx.fill();
            }
            SignalShape::Diamond => {
                ctx.set_fill_color(color);
                let half = icon_size / 2.0;
                let cy = icon_y + half;
                ctx.begin_path();
                ctx.move_to(x, cy - half);
                ctx.line_to(x + half, cy);
                ctx.line_to(x, cy + half);
                ctx.line_to(x - half, cy);
                ctx.close_path();
                ctx.fill();
            }
        }
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
    alert_items: &[mlc_core::indicator_source::AlertRenderData],
) {
    use mlc_core::indicator_source::AlertRenderStatus;
    use crate::chart_render::draw_styled_line;
    use mlc_core::chart::annotations::LineStyle;

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
        ctx.set_text_align(mlc_core::render::TextAlign::Center);
        ctx.set_text_baseline(mlc_core::render::TextBaseline::Middle);

        let price_str = format_price_smart(price);
        ctx.fill_text(&price_str, label_x + label_width / 2.0, screen_y.round());

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
/// Tint the area between a sub-pane series and its zero line — one colour
/// above, another below.
///
/// A signed series (open-interest change, funding) reads as "which side is
/// it on" long before it reads as a value, and a single-colour line hides
/// that. Drawn UNDER the line as translucent columns, one per bar, so the
/// line itself stays the precise reading.
#[allow(clippy::too_many_arguments)]
pub fn draw_sub_pane_zero_zones(
    ctx: &mut dyn RenderContext,
    state: &ChartRenderState,
    values: &[f64],
    pane_x: f64,
    pane_y: f64,
    pane_height: f64,
    pane_min: f64,
    pane_max: f64,
    up_color: &str,
    down_color: &str,
) {
    if pane_max <= pane_min {
        return;
    }
    let visible = state.viewport.visible_range();
    let (start_bar, end_bar) = (visible.0.max(0), visible.1.min(values.len()));
    if start_bar >= end_bar {
        return;
    }
    let y_for = |v: f64| pane_y + pane_height * (1.0 - (v - pane_min) / (pane_max - pane_min));
    // Zero may sit outside the pane's range (an all-positive window); clamp
    // so the fill still means "distance from zero" rather than inverting.
    let zero_y = y_for(0.0).clamp(pane_y, pane_y + pane_height);
    let bar_w = state.viewport.bar_spacing.max(1.0);
    for (i, &v) in values.iter().enumerate().take(end_bar).skip(start_bar) {
        if !v.is_finite() {
            continue;
        }
        let bar_ts_ms = state.bars.get(i).map(|b| b.ts_ms()).unwrap_or(0);
        let x = pane_x + state.viewport.x_for_bar(i, bar_ts_ms);
        let vy = y_for(v).clamp(pane_y, pane_y + pane_height);
        let (top, h) = if vy < zero_y { (vy, zero_y - vy) } else { (zero_y, vy - zero_y) };
        if h < 0.5 {
            continue;
        }
        let base = if v >= 0.0 { up_color } else { down_color };
        ctx.set_fill_color(&mlc_core::utils::color::apply_opacity(base, 0.28));
        ctx.fill_rect(x - bar_w * 0.5, top, bar_w, h);
    }
}

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
        let start = visible.0.max(0).saturating_sub(extra_bars);
        let end = (visible.1 + extra_bars).min(values.len());
        (start, end)
    } else {
        (visible.0.max(0), visible.1.min(values.len()))
    };

    if start_bar >= end_bar {
        return;
    }

    ctx.set_stroke_color(color);
    ctx.set_stroke_width(line_width as f64);
    ctx.begin_path();

    let mut started = false;
    for (i, &v) in values.iter().enumerate().take(end_bar).skip(start_bar) {
        if v.is_nan() || v.is_infinite() {
            started = false;
            continue;
        }

        let bar_ts_ms = state.bars.get(i).map(|b| b.ts_ms()).unwrap_or(0);
        let x = pane_x + state.viewport.x_for_bar(i, bar_ts_ms);
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

/// Like [`draw_sub_pane_line`], but each segment is colored by its own
/// direction: rising → `up_color`, falling → `down_color` (CVD reads by
/// delta sign — parity with the retired hand-drawn footprint strip).
#[allow(clippy::too_many_arguments)]
pub fn draw_sub_pane_line_directional(
    ctx: &mut dyn RenderContext,
    state: &ChartRenderState,
    values: &[f64],
    pane_x: f64,
    pane_y: f64,
    _pane_width: f64,
    pane_height: f64,
    pane_min: f64,
    pane_max: f64,
    up_color: &str,
    down_color: &str,
    line_width: f32,
) {
    let visible = state.viewport.visible_range();

    let (start_bar, end_bar) = if state.disable_clip {
        let extra_bars = (100.0 / state.viewport.bar_spacing).ceil() as usize;
        let start = visible.0.max(0).saturating_sub(extra_bars);
        let end = (visible.1 + extra_bars).min(values.len());
        (start, end)
    } else {
        (visible.0.max(0), visible.1.min(values.len()))
    };

    if start_bar >= end_bar {
        return;
    }

    ctx.set_stroke_width(line_width as f64);

    let mut prev: Option<(f64, f64, f64)> = None; // (x, y, value)
    for (i, &v) in values.iter().enumerate().take(end_bar).skip(start_bar) {
        if v.is_nan() || v.is_infinite() {
            prev = None;
            continue;
        }

        let bar_ts_ms = state.bars.get(i).map(|b| b.ts_ms()).unwrap_or(0);
        let x = pane_x + state.viewport.x_for_bar(i, bar_ts_ms);
        let ratio = if pane_max > pane_min {
            (v - pane_min) / (pane_max - pane_min)
        } else {
            0.5
        };
        let y = pane_y + pane_height - (ratio * pane_height);

        if let Some((px, py, pv)) = prev {
            ctx.set_stroke_color(if v >= pv { up_color } else { down_color });
            ctx.begin_path();
            ctx.move_to(px, py);
            ctx.line_to(x, y);
            ctx.stroke();
        }
        prev = Some((x, y, v));
    }
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
    histogram_style: mlc_core::indicator_source::HistogramStyle,
) {
    use mlc_core::indicator_source::HistogramStyle;

    let visible = state.viewport.visible_range();

    let (start_bar, end_bar) = if state.disable_clip {
        let extra_bars = (100.0 / state.viewport.bar_spacing).ceil() as usize;
        let start = visible.0.max(0).saturating_sub(extra_bars);
        let end = (visible.1 + extra_bars).min(values.len());
        (start, end)
    } else {
        (visible.0.max(0), visible.1.min(values.len()))
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

            for (i, &v) in values.iter().enumerate().take(end_bar).skip(start_bar) {
                if v.is_nan() || v.is_infinite() {
                    continue;
                }

                let bar_ts_ms = state.bars.get(i).map(|b| b.ts_ms()).unwrap_or(0);
                let x = pane_x + state.viewport.x_for_bar(i, bar_ts_ms) - bar_width / 2.0;

                // Map value to pixel Y using pane_min/pane_max coordinate system.
                let val_ratio = (v - pane_min) / pane_range;
                let val_y = pane_y + pane_height - (val_ratio * pane_height);

                let bar_color = if v >= 0.0 {
                    &state.theme.candle_up
                } else {
                    &state.theme.candle_down
                };
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

            for (i, &v) in values.iter().enumerate().take(end_bar).skip(start_bar) {
                if v.is_nan() || v.is_infinite() {
                    continue;
                }

                let bar_ts_ms = state.bars.get(i).map(|b| b.ts_ms()).unwrap_or(0);
                let x = pane_x + state.viewport.x_for_bar(i, bar_ts_ms) - bar_width / 2.0;
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
    _pane_index: usize,
    state: &ChartRenderState,
    indicator_source: &dyn mlc_core::indicator_source::IndicatorSource,
    scale_theme: &ScaleTheme,
    scale_config: &ScaleConfig,
    _crosshair_config: &CrosshairConfig,
    frame_theme: &FrameTheme,
    _is_dragging: bool,
    drawing_manager: Option<&dyn DrawingRenderView>,
    sub_pane_auto_scale: bool,
    sub_pane_price_min: f64,
    sub_pane_price_max: f64,
    selected_indicator_id: Option<u64>,
) -> (f64, f64) {
    use mlc_core::indicator_source::IndicatorOutputRenderType;

    let instance = match indicator_source.get_render_instance(pane_layout.instance_id) {
        Some(i) => i,
        None => return (0.0, 100.0),
    };

    if !instance.visible {
        return (0.0, 100.0);
    }

    // Timeframe visibility enforcement
    if let Some(current_tf) = state.current_timeframe {
        if let Some(ref tf_config) = instance.timeframe_visibility {
            if !tf_config.is_visible_on_label(current_tf) {
                return (0.0, 100.0);
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

    // OI Delta and Funding Rate draw through the ORDINARY output loop below:
    // the host resamples their bubble data onto the bar grid and publishes it
    // as a normal series (`frame_lifecycle::subpane_series`), so they are a
    // line like any other sub-pane indicator. They used to be painted here by
    // bespoke renderers — cells and a histogram — which is why they looked
    // nothing like the rest of the sub-pane family.

    let (visible_start, visible_end_raw) = state.viewport.visible_range();
    let visible_end = visible_end_raw.min(state.bars.len());

    let (pane_min, pane_max) = if !sub_pane_auto_scale {
        (sub_pane_price_min, sub_pane_price_max)
    } else {
        let (raw_min, raw_max) = indicator_source
            .calculate_pane_range(instance.id, visible_start, visible_end)
            .unwrap_or((0.0, 100.0));
        // Bounded oscillators (RSI, Stoch, ...) scale EXACTLY to their
        // declared bounds — no value-space padding stretches 0..100 into
        // -5..105 (owner ruling 2026-09-24, R24). Unbounded panes
        // symmetrize around zero when the data crosses it (so the
        // zero-line always sits at the vertical midpoint) and get a small
        // value-space padding so a line/histogram never touches the pane
        // edge. See `sub_pane_value_range`.
        sub_pane_value_range(instance.bounds, raw_min, raw_max)
    };

    // 4. Draw grid lines
    draw_sub_pane_grid(ctx, content.x, content.y, content.width, content.height, &state.theme.grid_line);

    // 4b. Draw a distinct zero baseline when the pane range straddles zero.
    let pane_range = pane_max - pane_min;
    if pane_min < 0.0 && pane_max > 0.0 && pane_range > 0.0 {
        let zero_ratio = (0.0_f64 - pane_min) / pane_range;
        let zero_y = content.y + content.height - (zero_ratio * content.height);
        ctx.set_stroke_color("rgba(255,255,255,0.25)");
        ctx.set_stroke_width(1.0);
        ctx.begin_path();
        ctx.move_to(content.x, zero_y);
        ctx.line_to(content.x + content.width, zero_y);
        ctx.stroke();
    }

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
                // CVD reads by delta direction — rising segments green,
                // falling red (parity with the retired hand-drawn strip;
                // owner report 2026-07-05 #4: single-color CVD is a
                // regression). Every other line keeps its flat color.
                // Optional signed-zone tint, under the line.
                if instance.bool_params.get("zones").copied().unwrap_or(false) {
                    draw_sub_pane_zero_zones(
                        ctx, state, values,
                        content.x, content.y, content.height,
                        pane_min, pane_max,
                        &state.theme.candle_up, &state.theme.candle_down,
                    );
                }
                if instance.type_id.eq_ignore_ascii_case("cvd") {
                    draw_sub_pane_line_directional(
                        ctx, state, values,
                        content.x, content.y, content.width, content.height,
                        pane_min, pane_max,
                        &state.theme.candle_up, &state.theme.candle_down,
                        line_width,
                    );
                } else {
                    draw_sub_pane_line(
                        ctx, state, values,
                        content.x, content.y, content.width, content.height,
                        pane_min, pane_max,
                        color, line_width,
                    );
                }
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
            &instance.signal_display,
            content.x,
            content.y,
            content.height,
            pane_min,
            pane_max,
            true, // sub-pane — icons pin to top/bottom of the pane rect
        );
    }

    ctx.restore();

    // 5.6 Draw selection markers if this sub-pane indicator is selected
    if selected_indicator_id == Some(pane_layout.instance_id) {
        use std::f64::consts::TAU;
        let viewport = state.viewport;
        let visible = viewport.visible_range();
        let (vis_s, vis_e) = (visible.0, visible.1.min(state.bars.len()));
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
                    let bar_ts_ms = state.bars.get(i).map(|b| b.ts_ms()).unwrap_or(0);
                    let mx = viewport.x_for_bar(i, bar_ts_ms) + content.x;
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

    // 7. Sub-pane crosshair — moved to render_chart_overlay (cursor-dependent).

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

    (pane_min, pane_max)
}

// =============================================================================
// Sub-Pane Drawing
// =============================================================================

/// Pixel clearance a sub-pane axis label keeps from its own pane's top/
/// bottom edge, beyond half its own text height. This is the "pixel
/// padding" ruling 1 (2026-09-24, R24) asks for on bounded oscillators —
/// the VALUE range stays exactly `[bounds.0, bounds.1]` (0/100 for RSI),
/// but the LABEL for 0/100 is inset a few pixels so its own glyph is never
/// sliced by the pane's clip rect, and — since every pane reserves this
/// same margin independently — two adjacent panes' labels can never reach
/// far enough to collide across their shared boundary.
const SUB_PANE_LABEL_EDGE_MARGIN: f64 = 3.0;

/// Draw price scale (Y-axis labels) for a sub-pane
///
/// Uses same styling as main chart price scale:
/// - Centered text horizontally
/// - Same font size (11px)
/// - Same background and border colors
///
/// Tick values are "nice" round numbers (`sub_pane_ticks`), not a raw
/// linear interpolation of `price_min..price_max` — and any candidate
/// whose label would cross the pane's own edge, or collide with another
/// kept label, is dropped (`cull_axis_labels`) rather than clipped or
/// clamped into place (owner ruling 2026-09-24, R24).
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
    let text_x = (x + width / 2.0).round();

    let price_range = price_max - price_min;
    if price_range <= 0.0 {
        return;
    }

    // Fewer target ticks for a small pane.
    let target_ticks = if height > 80.0 { 4 } else { 3 };
    let ticks = sub_pane_ticks(price_min, price_max, target_ticks);

    let candidates: Vec<AxisLabel> = ticks
        .iter()
        .map(|&price| {
            let ratio = (price - price_min) / price_range;
            let label_y = (y + height - (ratio * height)).round();
            AxisLabel { value: price, y: label_y }
        })
        .collect();

    let half_label_height = config.font_size / 2.0 + SUB_PANE_LABEL_EDGE_MARGIN;
    let kept = cull_axis_labels(&candidates, y, y + height, &[], half_label_height);

    // Clip tick labels to the sub-pane price scale as a defense-in-depth
    // backstop — `cull_axis_labels` above already keeps every surviving
    // label's own footprint fully inside [y, y+height].
    ctx.save();
    ctx.begin_path();
    ctx.rect(x, y, width, height);
    ctx.clip();

    ctx.set_font(&format!("{}px sans-serif", config.font_size));
    ctx.set_fill_color(&theme.scale_text);
    ctx.set_text_align(mlc_core::render::TextAlign::Center);
    ctx.set_text_baseline(mlc_core::render::TextBaseline::Middle);

    for label in &kept {
        let text = if price_range < 1.0 {
            format!("{:.4}", label.value)
        } else if price_range < 10.0 {
            format!("{:.2}", label.value)
        } else if price_range < 100.0 {
            format!("{:.1}", label.value)
        } else {
            format!("{:.0}", label.value)
        };

        ctx.fill_text(&text, text_x, label.y);
    }

    ctx.restore();
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
    ctx.set_text_align(mlc_core::render::TextAlign::Center);
    ctx.set_text_baseline(mlc_core::render::TextBaseline::Middle);
    ctx.fill_text(&label, scale_x + scale_width / 2.0, screen_y.round());

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
    _pane_index: usize,
    state: &ChartRenderState,
    pane_min: f64,
    pane_max: f64,
    _title: &str,
    scale_theme: &ScaleTheme,
    scale_config: &ScaleConfig,
    _crosshair_config: &CrosshairConfig,
    frame_theme: &FrameTheme,
    _is_dragging: bool,
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

    // 4. Draw grid lines
    draw_sub_pane_grid(ctx, content.x, content.y, content.width, content.height, &state.theme.grid_line);

    // 4b. Draw a distinct zero baseline when the pane range straddles zero.
    // This makes the zero level immediately readable on oscillators (MACD,
    // histogram, etc.) without relying on the regular grid spacing happening
    // to land there.
    let pane_range = pane_max - pane_min;
    if pane_min < 0.0 && pane_max > 0.0 && pane_range > 0.0 {
        let zero_ratio = (0.0_f64 - pane_min) / pane_range;
        let zero_y = content.y + content.height - (zero_ratio * content.height);
        ctx.set_stroke_color("rgba(255,255,255,0.25)");
        ctx.set_stroke_width(1.0);
        ctx.begin_path();
        ctx.move_to(content.x, zero_y);
        ctx.line_to(content.x + content.width, zero_y);
        ctx.stroke();
    }

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

    // 6. Sub-pane crosshair — moved to render_chart_overlay (cursor-dependent).
}

/// Helper function to render drawing primitives for a sub-pane
pub fn render_sub_pane_primitives(
    ctx: &mut dyn RenderContext,
    content: &LayoutRect,
    state: &ChartRenderState,
    dm: &dyn DrawingRenderView,
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

    // User-text label registry: re-tag with this bubble (idempotent per
    // pane pass — the pane passes follow the main pass within a window).
    mlc_core::engine::render::set_label_text_bid(dm.render_bid());

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
    ctx.set_bars(state.bars);

    // Clip to content area (skip for Glass styles)
    if !state.disable_clip {
        ctx.begin_path();
        ctx.rect(content.x, content.y, content.width, content.height);
        ctx.clip();
    }

    // Translate to content origin
    ctx.translate(content.x, content.y);

    // Create a viewport for this pane (same X-axis, different height)
    let pane_viewport = mlc_core::Viewport {
        view_start: state.viewport.view_start,
        bar_spacing: state.viewport.bar_spacing,
        bar_width_ratio: state.viewport.bar_width_ratio,
        chart_width: content.width,
        chart_height: content.height,
        bar_count: state.viewport.bar_count,
    };

    // Create a price scale for this pane
    let pane_price_scale = mlc_core::PriceScale {
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

            let is_selected = selected_idx == Some(data.id) || dm.remote_highlight() == Some(data.id);
            // Always arm identity so Fib/Gann level plates can attribute;
            // user-text recording still content-matches against `expected`
            // (empty string when there is no `data.text` — system labels
            // never equal that, so they stay unregistered).
            let expected = data.text.as_ref().map(|t| t.content.as_str()).unwrap_or("");
            mlc_core::engine::render::set_current_label_text_prim(
                Some(instance_id), data.id, expected);
            prim.render(ctx, is_selected);
            mlc_core::engine::render::clear_current_label_text_prim();

            // Draw control points for selected primitive
            if is_selected {
                let control_points = prim.control_points(
                    state.bars,
                    &pane_viewport,
                    &pane_price_scale,
                );
                let screen_points: Vec<(f64, f64)> = control_points
                    .iter()
                    .map(|cp| (cp.x, cp.y))
                    .collect();
                // Handle border takes the theme's foreground token and the fill
                // its surface token, so a handle reads as a chip punched out of
                // the chart in every preset. `crosshair` was the wrong border —
                // it is a dimmed guide colour, not a UI-chrome one.
                draw_control_points(ctx, &screen_points, &state.theme.text, &state.theme.background);
            }
        }
    }

    // Render drawing preview for this sub-pane — cursor-dependent, delegates to standalone fn.
    draw_sub_pane_drawing_preview(ctx, state, dm, instance_id, &pane_viewport, &pane_price_scale, content.height);

    // Restore context state
    ctx.restore();
}

// =============================================================================
// Sub-Pane Overlay Button Bar
// =============================================================================

/// Render the overlay button bar for a single sub-pane.
///
/// Shows 4 small icon buttons in the top-right corner of the sub-pane content area.
/// Buttons (left to right): Delete | Hide/Show | MoveUp | Expand/Restore
///
/// Returns [`SubPaneOverlayResult`] with button rects for hit-testing.
pub fn render_sub_pane_overlay(
    ctx: &mut dyn RenderContext,
    content: &LayoutRect,
    instance_id: u64,
    state: &SubPaneOverlayState,
    is_hidden: bool,
    is_maximized: bool,
    frame_theme: &FrameTheme,
    text_color: &str,
) -> SubPaneOverlayResult {
    use crate::ui::modal_settings::SubPaneButton;
    use mlc_core::render::draw_svg_icon;

    // Don't render if pane is too small or overlay not visible.
    // 20px is enough for a single row of buttons.
    if content.height < 20.0 || !state.visible {
        return SubPaneOverlayResult {
            instance_id,
            ..Default::default()
        };
    }

    let icon_size = 14.0;
    let button_size = 18.0;
    let gap = 2.0;
    let num_buttons: u32 = 5;
    let bar_width = num_buttons as f64 * (button_size + gap) - gap;
    let bar_height = button_size;
    let margin = 4.0;

    // Position: top-right corner of content area.
    // bar_y matches the title text Y (content.y + 4.0) and the left overlay bar_y.
    let bar_x = content.x + content.width - bar_width - margin;
    let bar_y = content.y + margin;

    // Draw background pill.
    let bg_color = apply_opacity(&frame_theme.toolbar_bg, 0.80);
    ctx.set_fill_color(&bg_color);
    ctx.fill_rounded_rect(bar_x - 3.0, bar_y - 2.0, bar_width + 6.0, bar_height + 4.0, 3.0);

    // Draw border.
    let border_color = apply_opacity(&frame_theme.toolbar_border, 0.6);
    ctx.set_stroke_color(&border_color);
    ctx.set_stroke_width(0.5);
    ctx.stroke_rounded_rect(bar_x - 3.0, bar_y - 2.0, bar_width + 6.0, bar_height + 4.0, 3.0);

    // Helper: compute button rect at index.
    let button_rect = |idx: u32| -> LayoutRect {
        LayoutRect::new(
            bar_x + idx as f64 * (button_size + gap),
            bar_y,
            button_size,
            button_size,
        )
    };

    // Center the icon within the button.
    let icon_offset = (button_size - icon_size) / 2.0;

    // Define the 5 buttons: (index, icon, button_type).
    let buttons: [(u32, Icon, SubPaneButton); 5] = [
        (0, Icon::Delete, SubPaneButton::Delete),
        (
            1,
            if is_hidden { Icon::EyeOff } else { Icon::Eye },
            SubPaneButton::Hide,
        ),
        (2, Icon::ArrowUp, SubPaneButton::MoveUp),
        (3, Icon::ArrowDown, SubPaneButton::MoveDown),
        (
            4,
            if is_maximized { Icon::Collapse } else { Icon::Expand },
            if is_maximized { SubPaneButton::Restore } else { SubPaneButton::Expand },
        ),
    ];

    let mut result = SubPaneOverlayResult {
        instance_id,
        delete_rect: None,
        hide_rect: None,
        move_up_rect: None,
        move_down_rect: None,
        expand_rect: None,
        restore_rect: None,
        left_eye_rect: None,
        left_alert_rect: None,
        left_settings_rect: None,
        left_delete_rect: None,
    };

    for (idx, icon, button_type) in &buttons {
        let rect = button_rect(*idx);
        let is_hovered = state.hovered_button.as_ref() == Some(button_type);

        // Determine icon color: red for delete on hover, accent for others, muted default.
        let color = if is_hovered {
            match button_type {
                SubPaneButton::Delete => "#f23645".to_string(),
                _ => "#2962ff".to_string(),
            }
        } else {
            apply_opacity(text_color, 0.6)
        };

        // Draw hover background.
        if is_hovered {
            let hover_bg = apply_opacity(&frame_theme.toolbar_border, 0.4);
            ctx.set_fill_color(&hover_bg);
            ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, 2.0);
        }

        // Draw icon.
        draw_svg_icon(
            ctx,
            icon.svg(),
            rect.x + icon_offset,
            rect.y + icon_offset,
            icon_size,
            icon_size,
            &color,
        );

        // Store rect for hit-testing.
        match button_type {
            SubPaneButton::Delete => result.delete_rect = Some(rect),
            SubPaneButton::Hide => result.hide_rect = Some(rect),
            SubPaneButton::MoveUp => result.move_up_rect = Some(rect),
            SubPaneButton::MoveDown => result.move_down_rect = Some(rect),
            SubPaneButton::Expand => result.expand_rect = Some(rect),
            SubPaneButton::Restore => result.restore_rect = Some(rect),
            SubPaneButton::IndicatorEye => result.left_eye_rect = Some(rect),
            SubPaneButton::IndicatorAlert => result.left_alert_rect = Some(rect),
            SubPaneButton::IndicatorSettings => result.left_settings_rect = Some(rect),
            SubPaneButton::IndicatorDelete => result.left_delete_rect = Some(rect),
        }
    }

    result
}

/// Render the LEFT-side per-indicator control buttons for a sub-pane.
///
/// Four buttons appear to the RIGHT of the indicator title text:
/// Eye (visibility), Alert (bell), Settings (gear), Delete (trash).
///
/// Visibility is shared with the right-side overlay — both overlays appear and
/// disappear together when the mouse enters/leaves the sub-pane.
///
/// # Returns
///
/// A tuple of four `Option<LayoutRect>` values:
/// `(eye_rect, alert_rect, settings_rect, delete_rect)`
pub fn render_sub_pane_left_overlay(
    ctx: &mut dyn RenderContext,
    content: &LayoutRect,
    instance_id: u64,
    state: &crate::ui::modal_settings::SubPaneOverlayState,
    indicator_visible: bool,
    indicator_title: &str,
    frame_theme: &FrameTheme,
    text_color: &str,
) -> (Option<LayoutRect>, Option<LayoutRect>, Option<LayoutRect>, Option<LayoutRect>) {
    use crate::ui::modal_settings::SubPaneButton;
    use mlc_core::render::draw_svg_icon;

    // Don't render if pane is too small.
    if content.height < 20.0 {
        return (None, None, None, None);
    }

    let icon_size = 14.0;
    let button_size = 18.0;
    let gap = 2.0;
    let margin_left = 8.0; // matches the old title x-offset
    let title_x = (content.x + margin_left).round();
    // Pill geometry (used for both hover and non-hover text alignment).
    let pill_y_raw = content.y + 2.0;
    let pill_height = 22.0;
    // Text Y = pill vertical center, rounded for crisp rendering.
    let title_center_y = (pill_y_raw + pill_height / 2.0).round();
    let pill_y = title_center_y - pill_height / 2.0;

    ctx.set_font("10px sans-serif");
    ctx.set_text_align(mlc_core::render::TextAlign::Left);
    ctx.set_text_baseline(mlc_core::render::TextBaseline::Middle);

    let _ = instance_id; // used by caller for hit-test resolution

    if !state.visible {
        // Not hovering — draw the indicator name in the muted theme text color,
        // same visual position as the old standalone title block.
        ctx.set_fill_color(text_color);
        ctx.fill_text(indicator_title, title_x, title_center_y);
        return (None, None, None, None);
    }

    // Hovering — draw a unified pill containing: title text + 4 action buttons.

    let num_buttons: u32 = 4;
    let buttons_width = num_buttons as f64 * (button_size + gap) - gap;

    // Measure title to size the pill correctly.
    let title_width = ctx.measure_text(indicator_title);

    let text_to_buttons_gap = 6.0;
    let pill_padding = 4.0;
    let pill_x = title_x - pill_padding;
    let pill_width = pill_padding + title_width + text_to_buttons_gap + buttons_width + pill_padding;

    // Clip: don't draw if the pill would overlap the right-side overlay area.
    let right_margin = 5.0 * (button_size + gap) + 10.0;
    if pill_x + pill_width > content.x + content.width - right_margin {
        // Pane too narrow — still draw text only, no pill/buttons.
        ctx.set_fill_color(text_color);
        ctx.fill_text(indicator_title, title_x, title_center_y);
        return (None, None, None, None);
    }

    // Draw pill background.
    let bg_color = apply_opacity(&frame_theme.toolbar_bg, 0.80);
    ctx.set_fill_color(&bg_color);
    ctx.fill_rounded_rect(pill_x, pill_y, pill_width, pill_height, 3.0);

    // Draw pill border.
    let border_color = apply_opacity(&frame_theme.toolbar_border, 0.6);
    ctx.set_stroke_color(&border_color);
    ctx.set_stroke_width(0.5);
    ctx.stroke_rounded_rect(pill_x, pill_y, pill_width, pill_height, 3.0);

    // Draw title text inside the pill.
    ctx.set_fill_color(text_color);
    ctx.fill_text(indicator_title, title_x, title_center_y);

    // Position buttons after the title text.
    let buttons_start_x = title_x + title_width + text_to_buttons_gap;
    let bar_y = pill_y + (pill_height - button_size) / 2.0;

    // Helper: compute button rect at index.
    let button_rect = |idx: u32| -> LayoutRect {
        LayoutRect::new(
            buttons_start_x + idx as f64 * (button_size + gap),
            bar_y,
            button_size,
            button_size,
        )
    };

    let icon_offset = (button_size - icon_size) / 2.0;

    // Define 4 buttons: (index, icon, button_variant).
    let buttons: [(u32, Icon, SubPaneButton); 4] = [
        (
            0,
            if indicator_visible { Icon::Eye } else { Icon::EyeOff },
            SubPaneButton::IndicatorEye,
        ),
        (1, Icon::Alert, SubPaneButton::IndicatorAlert),
        (2, Icon::Settings, SubPaneButton::IndicatorSettings),
        (3, Icon::Delete, SubPaneButton::IndicatorDelete),
    ];

    let mut eye_rect: Option<LayoutRect> = None;
    let mut alert_rect: Option<LayoutRect> = None;
    let mut settings_rect: Option<LayoutRect> = None;
    let mut delete_rect: Option<LayoutRect> = None;

    for (idx, icon, button_type) in &buttons {
        let rect = button_rect(*idx);
        let is_hovered = state.hovered_left_button.as_ref() == Some(button_type);

        let color = if is_hovered {
            match button_type {
                SubPaneButton::IndicatorDelete => "#f23645".to_string(),
                _ => "#2962ff".to_string(),
            }
        } else {
            apply_opacity(text_color, 0.6)
        };

        if is_hovered {
            let hover_bg = apply_opacity(&frame_theme.toolbar_border, 0.4);
            ctx.set_fill_color(&hover_bg);
            ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, 2.0);
        }

        draw_svg_icon(
            ctx,
            icon.svg(),
            rect.x + icon_offset,
            rect.y + icon_offset,
            icon_size,
            icon_size,
            &color,
        );

        match button_type {
            SubPaneButton::IndicatorEye => eye_rect = Some(rect),
            SubPaneButton::IndicatorAlert => alert_rect = Some(rect),
            SubPaneButton::IndicatorSettings => settings_rect = Some(rect),
            SubPaneButton::IndicatorDelete => delete_rect = Some(rect),
            _ => {}
        }
    }

    (eye_rect, alert_rect, settings_rect, delete_rect)
}

/// Draw the in-progress drawing preview for the MAIN chart.
///
/// Must be called inside the same `ctx.save()`/`ctx.restore()` block where
/// `set_coordinate_space` + `translate(chart.x, chart.y)` have already been
/// applied (i.e., from within `render_main_chart_primitives`).
///
/// Draws nothing when `dm.is_drawing()` is false or when the current stroke
/// belongs to a sub-pane (`dm.current_pane().is_some()`).
///
/// # Arguments
/// * `ctx`        – Render context (coordinate space already set by caller)
/// * `state`      – Chart render state (crosshair, bars, price_scale)
/// * `dm`         – Drawing manager
/// * `chart_height` – Height of the chart area (for price_to_y)
pub(crate) fn draw_main_chart_drawing_preview(
    ctx: &mut dyn RenderContext,
    state: &ChartRenderState,
    dm: &dyn DrawingRenderView,
    chart_height: f64,
) {
    // Only draw when actively drawing on the main chart (not a sub-pane)
    if !dm.is_drawing() || dm.current_pane().is_some() {
        return;
    }

    // Snap to bar centre (matching crosshair coordinate system)
    let cursor_bar = if let Some(idx) = state.crosshair.bar_idx {
        idx as f64
    } else {
        state.crosshair.bar_f64
    };
    // Use snapped price in magnet mode so preview matches where primitive will be placed
    let cursor_price = state.crosshair.effective_price(false);

    if dm.is_drawing_freehand() {
        {
            let points = dm.drawing_points();
            if points.len() >= 2 {
                let is_highlighter = dm.drawing_tool_id().as_deref() == Some("highlighter");
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
                    .map(|&(ts_ms, price)| {
                        let x = state.viewport.x_for_ts(ts_ms, state.bars);
                        let y = state.price_scale.price_to_y(price, chart_height);
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
        let cursor_ts_ms = mlc_core::bar_f64_to_timestamp_ms(state.bars, cursor_bar);
        let (cursor_ts_ms, cursor_price) = if dm.shift_constrain() {
            if let (Some(tool), Some((ts0, p0))) = (dm.drawing_tool_id(), dm.drawing_points().first().copied()) {
                mlc_core::drawing::snap_drawing_data(
                    &tool, state.bars, state.viewport,
                    state.price_scale.price_min, state.price_scale.price_max,
                    ts0, p0, cursor_ts_ms, cursor_price,
                )
            } else { (cursor_ts_ms, cursor_price) }
        } else { (cursor_ts_ms, cursor_price) };
        if let Some(preview_prim) = dm.create_preview(cursor_ts_ms, cursor_price) {
            preview_prim.render(ctx, false);

            // Draw anchor points for multi-click tools (not freehand).
            // Routed through the same `draw_control_points` choke point the
            // selected-primitive handles use, so shape/size/corner-radius
            // always match the configured `ControlPointStyle` — see report
            // that caused this change, 2026-08-14 (preview anchors were
            // hardcoded circles, disagreeing with a square-configured
            // style). The yellow fill is kept as the one deliberate
            // difference: it is the only visual cue that these points are
            // "placed but not yet a finished primitive".
            let points = dm.drawing_points();
            if !points.is_empty() {
                let screen_points: Vec<(f64, f64)> = points
                    .iter()
                    .map(|&(ts_ms, price)| {
                        let x = state.viewport.x_for_ts(ts_ms, state.bars);
                        let y = state.price_scale.price_to_y(price, chart_height);
                        (x, y)
                    })
                    .collect();
                draw_control_points(ctx, &screen_points, "#000000", "#ffff00");
            }
        }
    }
}

/// Draw the in-progress drawing preview for a SUB-PANE.
///
/// Must be called inside the same `ctx.save()`/`ctx.restore()` block where
/// the sub-pane coordinate space and `translate(content.x, content.y)` have
/// already been applied (i.e., from within `render_sub_pane_primitives`).
///
/// Draws nothing when `dm.is_drawing()` is false or when the current stroke
/// does not belong to `instance_id` (`dm.current_pane() != Some(instance_id)`).
///
/// # Arguments
/// * `ctx`              – Render context (coordinate space already set by caller)
/// * `state`            – Chart render state (crosshair, bars)
/// * `dm`               – Drawing manager
/// * `instance_id`      – Sub-pane instance ID (must match `dm.current_pane()`)
/// * `pane_viewport`    – Sub-pane viewport (for bar_to_x_f64)
/// * `pane_price_scale` – Sub-pane price scale (for price_to_y)
/// * `content_height`   – Height of the sub-pane content rect (for price_to_y)
pub(crate) fn draw_sub_pane_drawing_preview(
    ctx: &mut dyn RenderContext,
    state: &ChartRenderState,
    dm: &dyn DrawingRenderView,
    instance_id: u64,
    pane_viewport: &mlc_core::Viewport,
    pane_price_scale: &mlc_core::PriceScale,
    content_height: f64,
) {
    if !dm.is_drawing() || dm.current_pane() != Some(instance_id) {
        return;
    }

    let cursor_bar = state.crosshair.bar_f64;
    let cursor_price = state.crosshair.price;

    if dm.is_drawing_freehand() {
        {
            let points = dm.drawing_points();
            if points.len() >= 2 {
                let is_highlighter = dm.drawing_tool_id().as_deref() == Some("highlighter");
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
                    .map(|&(ts_ms, price)| {
                        let x = pane_viewport.x_for_ts(ts_ms, state.bars);
                        let y = pane_price_scale.price_to_y(price, content_height);
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
        let cursor_ts_ms = mlc_core::bar_f64_to_timestamp_ms(state.bars, cursor_bar);
        let (cursor_ts_ms, cursor_price) = if dm.shift_constrain() {
            if let (Some(tool), Some((ts0, p0))) = (dm.drawing_tool_id(), dm.drawing_points().first().copied()) {
                mlc_core::drawing::snap_drawing_data(
                    &tool, state.bars, state.viewport,
                    state.price_scale.price_min, state.price_scale.price_max,
                    ts0, p0, cursor_ts_ms, cursor_price,
                )
            } else { (cursor_ts_ms, cursor_price) }
        } else { (cursor_ts_ms, cursor_price) };
        if let Some(preview_prim) = dm.create_preview(cursor_ts_ms, cursor_price) {
            preview_prim.render(ctx, false);

            // See the main-chart preview above for why this goes through
            // `draw_control_points` instead of a hand-rolled circle.
            let points = dm.drawing_points();
            if !points.is_empty() {
                let screen_points: Vec<(f64, f64)> = points
                    .iter()
                    .map(|&(ts_ms, price)| {
                        let x = pane_viewport.x_for_ts(ts_ms, state.bars);
                        let y = pane_price_scale.price_to_y(price, content_height);
                        (x, y)
                    })
                    .collect();
                draw_control_points(ctx, &screen_points, "#000000", "#ffff00");
            }
        }
    }
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
    dm: &dyn DrawingRenderView,
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

    // User-text label registry (inline label editing): tag this pass with
    // its bubble so recorded label rects attribute to the right chart.
    mlc_core::engine::render::set_label_text_bid(dm.render_bid());

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
    ctx.set_bars(state.bars);

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
    let layout_vp = mlc_core::Viewport {
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
            // (e.g. a HORIZONTAL line at $40,000 while the chart shows $5).
            //
            // The AABB-of-points test is only valid for SEGMENTS / shapes —
            // rays, extended lines, and any tool with ExtendMode != None keep
            // drawing past their anchors. Culling those as soon as both
            // points leave the padded view is why a ray/line vanished on
            // pan (owner report 2026-08-28).
            let points = prim.points();
            if !points.is_empty() {
                let view_start = state.viewport.view_start;
                let view_end   = view_start + state.viewport.visible_bars() as f64;
                let price_min  = state.price_scale.price_min;
                let price_max  = state.price_scale.price_max;
                // Margin of 100 bars expressed in bar-index space for ts-based culling.
                let bar_margin = 100.0;

                let (skip_left, skip_right, skip_above, skip_below) =
                    unbounded_cull_skips(prim.as_ref());

                // A timestamp of exactly 0 is the same kind of sentinel one
                // axis over: a horizontal line has no time, it spans the whole
                // width. Bar-index for ts=0 lands far to the left of any real
                // chart, so `all_left` was true and the most-used primitive in
                // the app never painted at all.
                let stamped = points.iter().filter(|(ts, _)| *ts != 0).count();
                let fully_stamped = stamped == points.len();
                let all_left = !skip_left && fully_stamped
                    && points.iter().all(|(ts_ms, _)| {
                        let b = mlc_core::timestamp_ms_to_bar_f64(state.bars, *ts_ms);
                        b < view_start - bar_margin
                    });
                let all_right = !skip_right && fully_stamped
                    && points.iter().all(|(ts_ms, _)| {
                        let b = mlc_core::timestamp_ms_to_bar_f64(state.bars, *ts_ms);
                        b > view_end + bar_margin
                    });
                // A price of exactly 0.0 is the "this point carries no price"
                // sentinel: anchored VWAP, both volume profiles and the bars
                // pattern take their y-extent from the BARS, not from their own
                // points, so nothing here can judge where they sit vertically.
                // Culling them as `all_below` is why they never appeared —
                // `0.0 < price_min * 0.1` is true for every real instrument.
                let priced = points.iter().filter(|(_, p)| *p != 0.0).count();
                let fully_priced = priced == points.len();
                let all_above = !skip_above && fully_priced
                    && points.iter().all(|(_, price)| *price > price_max * 10.0);
                let all_below = !skip_below && fully_priced
                    && points.iter().all(|(_, price)| *price < price_min * 0.1);

                if all_left || all_right || all_above || all_below {
                    continue;
                }
            }

            let is_selected = selected_idx == Some(data.id) || dm.remote_highlight() == Some(data.id);
            // Always arm identity so Fib/Gann level plates can attribute;
            // user-text recording still content-matches against `expected`
            // (empty string when there is no `data.text` — system labels
            // never equal that, so they stay unregistered).
            let expected = data.text.as_ref().map(|t| t.content.as_str()).unwrap_or("");
            mlc_core::engine::render::set_current_label_text_prim(
                None, data.id, expected);
            prim.render(ctx, is_selected);
            mlc_core::engine::render::clear_current_label_text_prim();

            // Draw control points for selected primitive
            if is_selected {
                let control_points = prim.control_points(state.bars, &layout_vp, state.price_scale);
                let screen_points: Vec<(f64, f64)> = control_points
                    .iter()
                    .map(|cp| (cp.x, cp.y))
                    .collect();
                // Handle border takes the theme's foreground token and the fill
                // its surface token, so a handle reads as a chip punched out of
                // the chart in every preset. `crosshair` was the wrong border —
                // it is a dimmed guide colour, not a UI-chrome one.
                draw_control_points(ctx, &screen_points, &state.theme.text, &state.theme.background);
            }
        }
    }

    // Render drawing preview — cursor-dependent, delegates to standalone fn.
    draw_main_chart_drawing_preview(ctx, state, dm, chart.height);

    // Author's unfinished drawing. Not a primitive on this chart.
    if let Some(ghost) = dm.remote_ghost() {
        ghost.render(ctx, false);
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
/// 3. Background heat overlays (DOM / Liquidation heatmap, when enabled)
/// 4. Series (candles, bars, line, area, etc.)
/// 5. Volume Profile overlay (when enabled)
/// 6. TPO Profile overlay (when enabled)
///
/// Used internally by render_chart_window.
/// Crosshair is cursor-dependent and is drawn by render_chart_overlay.
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
    draw_data_gaps(ctx, state, config.chart_type);

    // 3. DOM Heatmap / Liquidation Heatmap background overlays (chart-type →
    // overlay migration) — draw BEFORE the main series so the heat layers
    // paint UNDER candles/bars/line/etc on ANY chart type, same visual as
    // when these were their own full-pane chart types. Both fns no-op
    // silently when their ring-buffer ref is `None` (overlay off or no data
    // yet) — the presence check lives inside `draw_{dom,liquidation}_heatmap_conditional`,
    // reading `state.dom_heatmap_ref`/`state.liq_heatmap_ref`, which
    // `build_chart_render_state` already gates on the window's
    // `dom_heatmap_show`/`liq_heatmap_show` flags.
    draw_dom_heatmap_conditional(ctx, state);
    draw_liquidation_projection_conditional(ctx, state);
    draw_liquidation_levels_conditional(ctx, state);

    // 4. Series (candles, bars, line, area, etc.)
    draw_series(ctx, state, config.chart_type);

    // 4a. Realized-liquidation prints — over the series (see the twin in
    // `render_full_chart_panel`, which is the pipeline with callers).
    draw_liquidation_prints_conditional(ctx, state);

    // 5. Volume Profile overlay (when enabled).
    if let Some(vp) = state.vp_overlay {
        draw_volume_profile_overlay(ctx, state, vp);
    }

    // 6. TPO Profile overlay (item D — when enabled). Same right-edge strip
    // slot as Volume Profile; both draw independently so an owner who
    // enables both sees them layered (mirrors the pre-existing OI/Funding
    // pair, which has the same non-exclusive-toggle tradeoff).
    if let Some(tpo_profile) = state.tpo_profile_overlay {
        draw_tpo_profile_overlay(ctx, state, tpo_profile);
    }
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
    // Per-chart skeleton: while there is no series data yet (cold preset,
    // TF/symbol switch, freshly-spawned window, or an own-slot chart type
    // whose derived data hasn't landed) render ONLY the chart-rect
    // background + empty price/time/corner frames. NO grid, NO bars, NO
    // crosshair, NO ticks, NO labels, NO last-price chip — just the three
    // boxes outlined so the chart's geometry is present from frame 1.
    if !state.has_any_series_data() {
        let theme_bg = &state.theme.background;
        let border = &config.scale_theme.scale_border;
        let scale_bg = &config.scale_theme.scale_bg;

        // 1. Chart area background.
        ctx.set_fill_color(theme_bg);
        ctx.fill_rect(layout.chart.x, layout.chart.y, layout.chart.width, layout.chart.height);

        // 2. Price scale frame (right strip).
        ctx.set_fill_color(scale_bg);
        ctx.fill_rect(layout.price_scale.x, layout.price_scale.y, layout.price_scale.width, layout.price_scale.height);

        // 3. Time scale frame (bottom strip).
        ctx.fill_rect(layout.time_scale.x, layout.time_scale.y, layout.time_scale.width, layout.time_scale.height);

        // 4. Scale corner frame (bottom-right).
        ctx.fill_rect(layout.scale_corner.x, layout.scale_corner.y, layout.scale_corner.width, layout.scale_corner.height);

        // 5. Borders around the three scale strips so they're visibly framed.
        ctx.set_stroke_color(border);
        ctx.set_stroke_width(1.0);
        ctx.set_line_dash(&[]);
        // Left edge of price scale (right edge of chart).
        let psx = layout.price_scale.x + 0.5;
        ctx.begin_path();
        ctx.move_to(psx, layout.chart.y);
        ctx.line_to(psx, layout.chart.y + layout.chart.height);
        ctx.stroke();
        // Top edge of time scale (bottom edge of chart).
        let tsy = layout.time_scale.y + 0.5;
        ctx.begin_path();
        ctx.move_to(layout.chart.x, tsy);
        ctx.line_to(layout.chart.x + layout.chart.width + layout.price_scale.width, tsy);
        ctx.stroke();
        // Left edge of scale corner (continues the price-scale border).
        let cnx = layout.scale_corner.x + 0.5;
        ctx.begin_path();
        ctx.move_to(cnx, layout.scale_corner.y);
        ctx.line_to(cnx, layout.scale_corner.y + layout.scale_corner.height);
        ctx.stroke();

        // Empty hit zones — no scale-corner buttons in skeleton state.
        return ScaleCornerHitZones::default();
    }

    // 1. Render chart area (background, grid, series, crosshair)
    render_chart(ctx, layout, state, config);

    // Build a per-leaf ScaleConfig with widths/heights clamped to the layout
    // rects so nothing can overflow into neighbouring leaves when the leaf is
    // squeezed below the configured scale size (sidebar separator drag).
    let mut leaf_scale_cfg = config.scale_config.clone();
    leaf_scale_cfg.price_scale_width = layout.price_scale.width;
    leaf_scale_cfg.time_scale_height = layout.time_scale.height;

    // 2. Price scale
    draw_price_scale(
        ctx,
        state,
        &leaf_scale_cfg,
        &config.scale_theme,
        layout.price_scale.x,
        layout.price_scale.y,
    );

    // 3. Time scale
    draw_time_scale(
        ctx,
        state,
        &leaf_scale_cfg,
        &config.scale_theme,
        layout.time_scale.x,
        layout.time_scale.y,
    );

    // 4. Scale corner with buttons
    let hit_zones = draw_scale_corner_with_buttons(ctx, &layout.scale_corner, &config.scale_theme, corner_state);

    // 5. Scale borders — thin separator lines between chart and scales, so the
    // chart area is always framed even when bars are still loading (the empty
    // chart_rect would otherwise blend into the background). Drawn last so they
    // sit on top of bg + grid + last-price chip + cursor labels.
    let border_color = config.scale_theme.scale_border.clone();
    ctx.set_stroke_color(&border_color);
    ctx.set_stroke_width(1.0);
    ctx.set_line_dash(&[]);
    // Left edge of price scale (right edge of chart).
    let psx = layout.price_scale.x + 0.5;
    ctx.begin_path();
    ctx.move_to(psx, layout.chart.y);
    ctx.line_to(psx, layout.chart.y + layout.chart.height);
    ctx.stroke();
    // Top edge of time scale (bottom edge of chart).
    let tsy = layout.time_scale.y + 0.5;
    ctx.begin_path();
    ctx.move_to(layout.chart.x, tsy);
    ctx.line_to(layout.chart.x + layout.chart.width + layout.price_scale.width, tsy);
    ctx.stroke();
    // Left edge of scale corner (continues the price-scale border down past
    // the time scale into the corner box).
    let cnx = layout.scale_corner.x + 0.5;
    ctx.begin_path();
    ctx.move_to(cnx, layout.scale_corner.y);
    ctx.line_to(cnx, layout.scale_corner.y + layout.scale_corner.height);
    ctx.stroke();

    hit_zones
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
    let mut leaf_scale_cfg = config.scale_config.clone();
    leaf_scale_cfg.price_scale_width = layout.price_scale.width;
    leaf_scale_cfg.time_scale_height = layout.time_scale.height;

    draw_price_scale(
        ctx,
        state,
        &leaf_scale_cfg,
        &config.scale_theme,
        layout.price_scale.x,
        layout.price_scale.y,
    );

    draw_time_scale(
        ctx,
        state,
        &leaf_scale_cfg,
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

/// Build an `ExtendedFrameLayout` where `maximized_id` receives the full
/// `content_rect` height (minus time scale) and all other sub-panes and the
/// main chart are collapsed to zero height.
///
/// Called by `render_full_chart_panel` when `maximized_sub_pane_id` is set.
/// The input builders in `mlc-app` mirror this logic so click geometry agrees.
fn build_maximized_layout(
    content_rect: &super::rects::LayoutRect,
    maximized_id: u64,
    sub_pane_ids: &[u64],
    scale_settings: &mlc_core::scale_settings::ScaleSettings,
) -> crate::layout::ExtendedFrameLayout {
    let _ = sub_pane_ids; // full list not needed; we render only max pane
    // Maximized = overlay: the pane covers the whole container content area,
    // main chart gets a true zero-height rect (guarded by main_chart_visible
    // at render time). No 26 px InnerLayout min strip.
    crate::layout::ExtendedFrameLayout::maximized(
        content_rect,
        maximized_id,
        scale_settings,
    )
}

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
    pub drawing_manager: Option<&'a dyn DrawingRenderView>,
    /// Indicator source for overlay indicators and sub-panes (optional)
    pub indicator_source: Option<&'a dyn mlc_core::indicator_source::IndicatorSource>,
    /// Symbol string used to filter indicators (optional)
    pub symbol: Option<&'a str>,
    /// Per-instance Y-axis state (price_min/max, auto_scale), keyed by instance_id.
    pub sub_pane_y_state: Option<&'a std::collections::HashMap<u64, SubPaneYState>>,
    /// Symbol comparison overlay (optional)
    pub compare_overlay: Option<&'a CompareOverlay>,
    /// Watermark displayed behind chart content (optional)
    pub watermark: Option<&'a mlc_core::chart::Watermark>,
    /// Tooltip displayed near the cursor (optional)
    pub tooltip: Option<&'a Tooltip>,
    /// Alert price levels to render as horizontal lines
    pub alert_render_data: &'a [mlc_core::indicator_source::AlertRenderData],
    /// Scale visibility / position / corner settings
    pub scale_settings: &'a ScaleSettings,
    /// Currently selected indicator ID (for selection highlighting, optional)
    pub selected_indicator_id: Option<u64>,
    /// Frame theme for borders and toolbar backgrounds
    pub frame_theme: &'a FrameTheme,
    /// Sub-pane overlay button states, indexed by sub-pane position in the sub_panes slice.
    ///
    /// Each entry corresponds to a sub-pane at the same index.  A missing entry (index
    /// out of bounds) is treated as a default (not visible) overlay state.
    pub sub_pane_overlay_states: &'a [SubPaneOverlayState],
    /// Toolbar configuration used to carve the content rect from `window_rect`.
    ///
    /// `render_full_chart_panel` receives the full panel rect (including toolbar
    /// areas) as `window_rect` and uses this config to compute the content rect
    /// internally.  Pass `&ToolbarConfig::minimal()` when the caller has already
    /// stripped toolbar space from the rect it passes in.
    pub toolbar_config: &'a crate::panel_app::ToolbarConfig,
    /// Whether this leaf is rendered inside a multi-leaf split layout.
    /// When true, top/bottom frame borders are suppressed (the split separator handles them).
    pub is_split: bool,
    /// Container's inner-layout root proportions [main, sub_0, sub_1, ...].
    /// Empty -> uniform fallback.
    pub inner_proportions: &'a [f64],
    /// Container's inner-layout sub-pane instance ids parallel to
    /// `inner_proportions[1..]`. The renderer uses THIS list (not the
    /// legacy `sub_panes` Vec) as the order and identity source for
    /// sub-panes, intersected with `indicator_source` visibility.
    pub inner_sub_instance_ids: &'a [u64],
    /// Last data-fetch error for this window, if any.
    ///
    /// When `Some` and bars are empty, the error text is rendered centred in
    /// the panel instead of the generic "Loading..." label.  Cleared by the
    /// app on any successful bar delivery.
    pub data_error: Option<&'a str>,
    /// When `Some`, this sub-pane instance id is currently maximized (fills
    /// the sub-pane area; all other panes have near-zero proportions).  The
    /// overlay button for this pane renders as Restore instead of Expand.
    /// `None` means no pane is maximized (normal mode).
    pub maximized_sub_pane_id: Option<u64>,
    /// Set of sub-pane instance_ids that should render ABOVE the main chart.
    /// Pass `None` (or `Some` of an empty set) for all-below behaviour.
    pub above_main_ids: Option<&'a std::collections::HashSet<u64>>,
}

/// Word-wrap `text` to at most `max_lines` lines that each fit within
/// `max_width` pixels (measured via `ctx.measure_text`, so the caller's
/// already-set font applies — see `record_label_text_rect` in
/// `mlc-core/src/engine/render/context.rs` for the same measure-after-
/// set-font convention). Whatever still doesn't fit after `max_lines` is
/// ellipsized on the last line, backed off to a word boundary the same way
/// `mlc-app-sidebar-content::render::truncate_to_width_word_boundary` does
/// for the watchlist row — never cut mid-word except for a single token
/// that alone overflows `max_width` (no boundary exists to back off to).
///
/// Used by the chart's empty-state error text (owner spec 2026-08-30): a
/// fixed character budget cannot know the pane's actual width, so it either
/// overshoots the pane (text clipped off both edges) or undershoots it
/// (needless clipping on a wide pane) — only a real measurement fits.
fn wrap_and_ellipsize(ctx: &dyn RenderContext, text: &str, max_width: f64, max_lines: usize) -> Vec<String> {
    if max_width <= 0.0 || max_lines == 0 {
        return Vec::new();
    }
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.is_empty() {
        return Vec::new();
    }
    let mut lines: Vec<String> = Vec::new();
    let mut idx = 0usize;
    while idx < words.len() && lines.len() < max_lines {
        let mut line = words[idx].to_string();
        idx += 1;
        while idx < words.len() {
            let candidate = format!("{} {}", line, words[idx]);
            if ctx.measure_text(&candidate) > max_width {
                break;
            }
            line = candidate;
            idx += 1;
        }
        lines.push(line);
    }
    let overflowed_last_line = lines.last().is_some_and(|l| ctx.measure_text(l) > max_width);
    if idx < words.len() || overflowed_last_line {
        if let Some(last) = lines.last_mut() {
            let ellipsis = "\u{2026}";
            let ellipsis_w = ctx.measure_text(ellipsis);
            let available = (max_width - ellipsis_w).max(0.0);
            while !last.is_empty() && ctx.measure_text(last) > available {
                last.pop();
            }
            if let Some(pos) = last.trim_end().rfind(char::is_whitespace) {
                last.truncate(pos);
            }
            let trimmed = last.trim_end().to_string();
            *last = format!("{}{}", trimmed, ellipsis);
        }
    }
    lines
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
) -> ChartPanelRenderResult {
    // Skip rendering if the panel is too small (e.g. collapsed during expand)
    if window_rect.width < 2.0 || window_rect.height < 2.0 {
        return ChartPanelRenderResult::empty();
    }

    // Build a set of visible instance IDs from indicator_source (single source of truth).
    let visible_ids: std::collections::HashSet<u64> = if let (Some(im), Some(symbol)) = (data.indicator_source, data.symbol) {
        im.get_instances_for_symbol(symbol)
            .into_iter()
            .filter(|i| i.visible && i.pane_index > 0)
            .map(|i| i.id)
            .collect()
    } else {
        std::collections::HashSet::new()
    };

    // Sub-pane order and identity come from the container's InnerLayout
    // (single source of truth), intersected with indicator visibility.
    let sub_pane_ids: Vec<u64> = data
        .inner_sub_instance_ids
        .iter()
        .filter(|id| visible_ids.contains(id))
        .copied()
        .collect();

    // Compute the panel layout by carving toolbar rects from window_rect.
    // The content_rect is the area available for chart rendering after subtracting
    // toolbar space.  All scale/grid/series rendering uses content_rect so that
    // price/time scales don't overlap with toolbars.
    let panel_layout = crate::panel_app::ChartPanelLayout::compute(window_rect, data.toolbar_config);
    let content_rect = &panel_layout.content_rect;

    // Sub-pane geometry comes from the container's inner uzor docking
    // tree's root proportions. `inner_proportions` is `[main_share,
    // sub_1_share, sub_2_share, ...]`. Empty -> uniform fallback.
    let sub_props: Vec<f64> = if data.inner_proportions.len() > 1 {
        data.inner_proportions.iter().skip(1).copied().collect()
    } else {
        Vec::new()
    };

    // Determine whether a sub-pane is currently maximized.  When maximized,
    // the layout gives the full container area to that pane — main chart and
    // all other panes collapse to zero height.  The normal from_proportions
    // path is bypassed in this case so InnerLayout min-height guards never
    // prevent the main chart from being fully hidden.
    let maximized_id: Option<u64> = data.maximized_sub_pane_id
        .filter(|iid| sub_pane_ids.contains(iid));

    // Resolve above-main set. When maximized, above-main is ignored — the
    // maximized pane covers the whole area regardless.
    let empty_above: std::collections::HashSet<u64> = std::collections::HashSet::new();
    let above_main_ids = data.above_main_ids.unwrap_or(&empty_above);

    let extended_layout = if let Some(max_iid) = maximized_id {
        build_maximized_layout(content_rect, max_iid, &sub_pane_ids, data.scale_settings)
    } else {
        crate::layout::ExtendedFrameLayout::from_proportions(
            content_rect,
            &sub_pane_ids,
            &sub_props,
            data.inner_proportions.first().copied()
                .filter(|_| data.inner_proportions.len() > 1),
            data.scale_settings,
            1.0,
            above_main_ids,
        )
    };
    let main_chart = &extended_layout.main_chart;
    // Guard: when the main chart area has no usable height (maximized sub-pane
    // mode), skip all main-chart rendering passes.
    let main_chart_visible = main_chart.chart.height >= 1.0;

    // Bug 2 fix: update viewport dimensions to match the computed layout so
    // that bar_to_x() and price_to_y() operate in the correct coordinate space.
    // Without this, stale defaults (e.g. 800x400 from construction) are used.
    let mut corrected_viewport = data.state.viewport.clone();
    corrected_viewport.chart_width = main_chart.chart.width;
    corrected_viewport.chart_height = main_chart.chart.height;

    // Bug 3 fix: build a ChartRect that matches the layout we just computed so
    // that all draw calls (series, overlays, scales, …) use the correct origin.
    let corrected_chart_rect = crate::chart_render::ChartRect {
        x: main_chart.chart.x,
        y: main_chart.chart.y,
        width: main_chart.chart.width,
        height: main_chart.chart.height,
    };

    // Assemble a corrected render state referencing the local corrected values.
    let corrected_state = crate::chart_render::ChartRenderState {
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
        timeframe_interval_ms: data.state.timeframe_interval_ms,
        scale_settings: data.state.scale_settings,
        body_enabled: data.state.body_enabled,
        border_enabled: data.state.border_enabled,
        wick_enabled: data.state.wick_enabled,
        use_prev_close: data.state.use_prev_close,
        footprint_bars: data.state.footprint_bars,
        footprint_display: data.state.footprint_display,
        tick_intensity: data.state.tick_intensity,
        renko_bars: data.state.renko_bars,
        three_line_break: data.state.three_line_break,
        pnf_columns: data.state.pnf_columns,
        kagi_segments: data.state.kagi_segments,
        tpo_sessions: data.state.tpo_sessions,
        tpo_display: data.state.tpo_display,
        line_family_display: data.state.line_family_display,
        ha_cache: data.state.ha_cache,
        vp_overlay: data.state.vp_overlay,
        tpo_profile_overlay: data.state.tpo_profile_overlay,
        oi_delta_ref: data.state.oi_delta_ref,
        funding_rate_ref: data.state.funding_rate_ref,
        dom_heatmap_ref: data.state.dom_heatmap_ref,
        dom_trade_dots_ref: data.state.dom_trade_dots_ref,
        liq_tape_ref: data.state.liq_tape_ref,
        liq_ink: data.state.liq_ink,
        liq_projection_ref: data.state.liq_projection_ref,
        liq_projection_ink: data.state.liq_projection_ink,
        vp_ink: data.state.vp_ink,
        tpo_ink: data.state.tpo_ink,
        dom_params: data.state.dom_params,
        non_temporal_axis: data.state.non_temporal_axis,
        hide_time_grid: data.state.hide_time_grid,
        history_depth: data.state.history_depth,
        cvd_paint_deltas: data.state.cvd_paint_deltas.clone(),
        trade_archive_available: data.state.trade_archive_available,
        bars_trade_built: data.state.bars_trade_built,
    };

    // 1. Background — fill the full window rect to avoid gaps when scales are
    //    repositioned.
    ctx.set_fill_color(&corrected_state.theme.background);
    ctx.fill_rect(window_rect.x, window_rect.y, window_rect.width, window_rect.height);

    // Loading / error state — when there is no series data yet (neither
    // klines NOR an own-slot for chart types that render from a derived
    // series — footprint/renko/PnF/Kagi/three-line-break/TPO/heatmaps),
    // draw ONLY the three scale border lines (price scale separator, time
    // scale separator, scale corner separator) so the chart skeleton has the
    // same visible frame as the active path. NO scale fills, NO grid, NO
    // bars, NO crosshair, NO ticks, NO labels — only the border strokes. The
    // loading SPINNER is NOT drawn here — it lives as an overlay layer in the
    // host renderer (`scene_builder.rs`) so it has its own redraw cadence and
    // a host-level retire signal independent of the check this layer sees.
    if !corrected_state.has_any_series_data() {
        let border = &data.config.scale_theme.scale_border;
        ctx.set_stroke_color(border);
        ctx.set_stroke_width(1.0);
        ctx.set_line_dash(&[]);
        let psx = main_chart.price_scale.x + 0.5;
        ctx.begin_path();
        ctx.move_to(psx, main_chart.chart.y);
        ctx.line_to(psx, main_chart.chart.y + main_chart.chart.height);
        ctx.stroke();
        let tsy = main_chart.time_scale.y + 0.5;
        ctx.begin_path();
        ctx.move_to(main_chart.chart.x, tsy);
        ctx.line_to(main_chart.chart.x + main_chart.chart.width + main_chart.price_scale.width, tsy);
        ctx.stroke();
        let cnx = main_chart.scale_corner.x + 0.5;
        ctx.begin_path();
        ctx.move_to(cnx, main_chart.scale_corner.y);
        ctx.line_to(cnx, main_chart.scale_corner.y + main_chart.scale_corner.height);
        ctx.stroke();

        let cx = main_chart.chart.x + main_chart.chart.width / 2.0;
        let cy = main_chart.chart.y + main_chart.chart.height / 2.0;
        let mut result = ChartPanelRenderResult::empty();
        if let Some(err_msg) = data.data_error {
            // Error path keeps text — the user needs to see WHAT failed.
            //
            // Wrapped to the PANE's actual measured width, not a fixed
            // character budget (owner spec 2026-08-30): a character count
            // cannot know the pane's real width, so it either overshoots
            // (text clipped off both edges by the renderer, worse than no
            // wrap at all — the live regression this replaces) or
            // undershoots (needless clipping on a wide pane). The font is
            // set BEFORE `measure_text` below runs, so line-fit is true.
            let font_size = 14.0;
            ctx.set_font("14px sans-serif");
            ctx.set_text_align(mlc_core::render::TextAlign::Center);
            ctx.set_text_baseline(mlc_core::render::TextBaseline::Middle);
            ctx.set_fill_color(&corrected_state.theme.candle_down);
            const TEXT_SIDE_PADDING: f64 = 24.0;
            let max_text_width = (main_chart.chart.width - TEXT_SIDE_PADDING * 2.0).max(40.0);
            let lines = wrap_and_ellipsize(ctx, err_msg, max_text_width, 3);
            let line_height = font_size * 1.2;
            let top_y = cy - (lines.len() as f64 - 1.0) * line_height / 2.0;
            for (i, line) in lines.iter().enumerate() {
                ctx.fill_text(line, cx, top_y + i as f64 * line_height);
            }
        } else {
            // Hand the spinner geometry up to the host renderer.
            let pane_min = main_chart.chart.width.min(main_chart.chart.height);
            let radius = (pane_min * 0.08).clamp(14.0, 36.0);
            result.loading_spinner = Some((cx, cy, radius));
        }
        return result;
    }

    // Steps 2-11 render the main chart area.  When a sub-pane is maximized the
    // main chart height is 0, so all these passes are skipped to avoid drawing
    // series, scales, and primitives in a zero-height rect.
    let mut leaf_scale_cfg = data.config.scale_config.clone();
    leaf_scale_cfg.price_scale_width = main_chart.price_scale.width;
    leaf_scale_cfg.time_scale_height = main_chart.time_scale.height;

    if main_chart_visible {
        // 2. Grid — extended variant covers the content area (under scales, excluding toolbars).
        draw_grid_extended(ctx, &corrected_state, content_rect);
        // 2b. Data-gap bands (archive-day holes) — this panel pipeline is
        // the LIVE web render path; the single-pane render_chart path has
        // its own call.
        draw_data_gaps(ctx, &corrected_state, data.config.chart_type);

        // 2c. DOM Heatmap — resting book depth UNDER the series, as in the
        // reference (candles stay readable through the heat). No-ops unless
        // the `overlay_dom_heatmap` instance exists and a snapshot has
        // landed. This call was missing entirely: the C12/C13 chart-type →
        // overlay migration left the draw wired only into the dead
        // `render_chart` pipeline, so the overlay could never paint on the
        // surface the app actually renders.
        draw_dom_heatmap_conditional(ctx, &corrected_state);

        // 2d. Realized liquidations — prints + levels, UNDER the series for
        // the same reason. This call was missing while the twin above was
        // present: the layer painted only into `render_chart`, the pipeline
        // with no callers, so the overlay could be on, the tape could be
        // filling, and the chart still showed nothing. Exactly the defect the
        // DOM heat had before 2026-08-02 — when a layer is silent, check that
        // the LIVE pipeline draws it before touching anything else.
        // 2c-bis. Projected liquidation bands — modelled standing
        // liquidity, under everything else it shares the chart with.
        draw_liquidation_projection_conditional(ctx, &corrected_state);

        draw_liquidation_levels_conditional(ctx, &corrected_state);

        // 3. Series (candles, bars, line, etc.)
        draw_series(ctx, &corrected_state, data.config.chart_type);

        // 3a. Realized-liquidation PRINTS — over the series. A forced close
        // is an event at a price, and the one that matters most sits at the
        // current price, where its own candle would cover it.
        draw_liquidation_prints_conditional(ctx, &corrected_state);

        // 3b. Volume Profile overlay (when enabled).
        if let Some(vp) = corrected_state.vp_overlay {
            draw_volume_profile_overlay(ctx, &corrected_state, vp);
        }

        // 3c. TPO Profile overlay. It was drawn ONLY from `render_chart`
        // while Volume Profile was drawn from both there and here — and
        // THIS is the path the app actually renders through, so TPO never
        // appeared no matter how much data it had (measured 2026-08-03:
        // `tpo_rows: 53`, instance visible, nothing on screen). Same
        // right-edge strip as VP; enabling both layers them.
        if let Some(tpo_profile) = corrected_state.tpo_profile_overlay {
            draw_tpo_profile_overlay(ctx, &corrected_state, tpo_profile);
        }

        // OI Delta and the Funding Rate strip used to be drawn here, inside
        // the price plot. They are sub-pane INDICATORS and now render from
        // `render_sub_pane` into the pane the layout gives them.

        // CVD is a first-class indicator instance (mli `BarIndicatorId::
        // Cvd`, catalog id "CVD") rendered through the normal indicator
        // sub-pane system (see `render_sub_pane` dispatch) — no hand-drawn
        // strip here. The data plane (`bubble.cvd_line`, `CvdSampleAppended`)
        // now exists purely to feed `IndicatorsMut::recalc_bubble`'s delta
        // builder; the view-plane toggle pipeline was removed 2026-07-05.

        // 4. Last-price projection line.
        draw_last_price_line(ctx, &corrected_state);

        // 5. Watermark (behind other overlays).
        if let Some(watermark) = data.watermark {
            draw_watermark(ctx, &corrected_state.chart_rect, watermark);
        }

        // History-depth badge REMOVED by owner order 2026-07-13 («никакой
        // оверлейной хуйни за пределами места разрыва») — degraded depth is
        // communicated by the gap bands / dropdown marking only.
        // `draw_history_depth_badge` stays exported for the dropdown-side
        // callers; this render call was its only on-chart use.

        // 6. Compare overlay (symbol comparisons).
        if let Some(compare_overlay) = data.compare_overlay {
            draw_compare_overlay(ctx, &corrected_state, compare_overlay);
        }

        // NOTE: Tooltip and crosshair are cursor-dependent — they are drawn by
        // render_chart_overlay, not the static pass.

        // NOTE: Crosshair is drawn AFTER sub-panes so the vertical line is not
        // covered by sub-pane backgrounds. (moved to render_chart_overlay)

        // 8. Price scale (skip when hidden).
        if data.scale_settings.price_scale_position.is_visible() {
            draw_price_scale(
                ctx,
                &corrected_state,
                &leaf_scale_cfg,
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
    }

    // 12. Sub-panes (indicators with pane_index > 0).
    let mut computed_ranges: Vec<(usize, f64, f64)> = Vec::new();
    let mut sub_pane_overlay_results: Vec<SubPaneOverlayResult> = Vec::new();
    if let Some(im) = data.indicator_source {
        for (pane_idx, pane_layout) in extended_layout.sub_panes.iter().enumerate() {
            let sub_pane_state = data.sub_pane_y_state
                .and_then(|m| m.get(&pane_layout.instance_id));
            let (auto_scale, price_min, price_max) = match sub_pane_state {
                Some(sp) => (sp.auto_scale, sp.price_min, sp.price_max),
                None => (true, 0.0, 100.0),
            };
            let (final_min, final_max) = render_sub_pane(
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
            if auto_scale {
                computed_ranges.push((pane_idx, final_min, final_max));
            }

            // Render the overlay button bar for this sub-pane.
            // overlay_states are indexed in tree/layout order (same as pane_idx).
            let overlay_state_idx = pane_idx;
            let overlay_state = data.sub_pane_overlay_states
                .get(overlay_state_idx)
                .cloned()
                .unwrap_or_default();
            // is_hidden is always false here — hidden panes are filtered out of
            // sub_pane_ids via instance.visible check, so only visible panes render.
            let is_maximized = data.maximized_sub_pane_id == Some(pane_layout.instance_id);
            let overlay_result = render_sub_pane_overlay(
                ctx,
                &pane_layout.content,
                pane_layout.instance_id,
                &overlay_state,
                false, // is_hidden: always false — hidden panes don't reach render
                is_maximized,
                data.frame_theme,
                &data.state.theme.text,
            );
            // Render the LEFT-side per-indicator control buttons.
            let render_inst = im.get_render_instance(pane_layout.instance_id);
            let ind_visible = render_inst.as_ref().is_none_or(|ri| ri.visible);
            let ind_title = render_inst.as_ref().map_or("", |ri| ri.title.as_str());
            let (left_eye, left_alert, left_settings, left_delete) =
                render_sub_pane_left_overlay(
                    ctx,
                    &pane_layout.content,
                    pane_layout.instance_id,
                    &overlay_state,
                    ind_visible,
                    ind_title,
                    data.frame_theme,
                    &data.state.theme.text,
                );
            // Merge left overlay rects into the overlay result.
            let mut overlay_result = overlay_result;
            overlay_result.left_eye_rect = left_eye;
            overlay_result.left_alert_rect = left_alert;
            overlay_result.left_settings_rect = left_settings;
            overlay_result.left_delete_rect = left_delete;

            sub_pane_overlay_results.push(overlay_result);
        }
    }

    // 13. Crosshair — moved to render_chart_overlay (cursor-dependent).

    // 14. Time scale (skip when hidden).
    if data.scale_settings.time_scale_position.is_visible() {
        draw_time_scale(
            ctx,
            &corrected_state,
            &leaf_scale_cfg,
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

        // Compute total chart bounds for border drawing (same as overlay uses for crosshair).
        let total_chart_top = extended_layout.sub_panes.iter()
            .map(|p| p.content.y)
            .fold(main_chart.chart.y, f64::min);
        let total_chart_bottom = extended_layout.sub_panes.iter()
            .map(|p| p.content.y + p.content.height)
            .fold(main_chart.chart.y + main_chart.chart.height, f64::max);

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
        // Vertical line spans from topmost content (including above-main) to bottom.
        let vert_top = total_chart_top.min(time_scale.y);
        let vert_bottom = if time_scale.y + time_scale.height > 0.0 {
            time_scale.y + time_scale.height
        } else {
            total_chart_bottom
        };
        ctx.fill_rect(vert_x, vert_top, 1.0, vert_bottom - vert_top);

        // Horizontal separators spanning full width (chart + price scale).
        let horz_x = chart.x.min(price_scale.x);
        let horz_w = chart.width + price_scale.width;
        // No top border here. `total_chart_top` is the toolbar's bottom edge,
        // and the toolbar already draws its own rule there
        // (`panel_app.rs`'s "Bottom border of control strip"). Both landed with
        // a zero gap, so the boundary was a 2px band pretending to be a 1px
        // line — invisible in the dark themes, obvious the moment the owner
        // opened High Contrast (2026-09-03).
        //
        // The comment this replaces said the line was "visible when above-main
        // panes exist", but it was gated on `!data.is_split`, which is a
        // different question. The above-main case is genuinely handled ten
        // lines below, where each such pane draws the rule at its own
        // `content.y` — that is the gate the comment was describing.
        // Bottom border between chart content and time scale (always drawn).
        ctx.fill_rect(horz_x, total_chart_bottom, horz_w, 1.0);

        // Sub-pane separator lines across the price scale area.
        for sub_pane in &extended_layout.sub_panes {
            // Separator rect (between pane and main chart or adjacent pane).
            ctx.fill_rect(vert_x, sub_pane.separator.y, 1.0, sub_pane.separator.height);
            // For above-main panes, separator is at bottom — also draw top border.
            if sub_pane.content.y < chart.y {
                ctx.fill_rect(horz_x, sub_pane.content.y, horz_w, 1.0);
            }
        }
    }

    ChartPanelRenderResult {
        corner_zones,
        sub_pane_ranges: computed_ranges,
        sub_pane_overlays: sub_pane_overlay_results,
        loading_spinner: None,
    }
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

// =============================================================================
// Overlay (Cursor-Dependent) Rendering
// =============================================================================

/// Draw the crosshair (dashed price/time lines) and its price/time axis
/// cursor labels for a single chart panel.
///
/// This is the LOWEST cursor-dependent overlay layer: drawn immediately
/// above the static chart content and BELOW every popup surface (toolbar
/// dropdowns, modals, context menu, inline settings bars, tooltip). Callers
/// MUST draw this before any popup chrome paints in the same frame —
/// `ChartApp::render_to_scene` (mlc-app) calls it right after each leaf's
/// [`render_full_chart_panel`] pass, ahead of the toolbar/dropdown draw
/// (owner-reported bug, fixed 2026-07-25: the crosshair used to bleed on
/// top of the chart-type dropdown because it was only painted later, via
/// the separate cursor-overlay pass that ran after the toolbar).
///
/// # Parameters
/// - `ctx`              – Render context
/// - `state`            – Chart render state (same one used for the static pass)
/// - `layout`           – Pre-computed `ExtendedFrameLayout` for this panel
///                        (the geometry is passed in — do NOT recompute it here)
/// - `config`           – Chart render configuration (crosshair config, scale config/theme)
/// - `sub_pane_ranges`  – Optional sub-pane value ranges as `(pane_idx, min, max)`.
///                        Passed from `ChartPanelRenderResult.sub_pane_ranges`.  Used for
///                        accurate price labels on the sub-pane Y-axis at the crosshair
///                        position.  When `None` or when a pane index is not found, the
///                        label is omitted to avoid showing an incorrect value.
pub fn render_chart_crosshair_layer(
    ctx: &mut dyn RenderContext,
    state: &ChartRenderState,
    layout: &crate::layout::ExtendedFrameLayout,
    config: &ChartRenderConfig,
    sub_pane_ranges: Option<&[(usize, f64, f64)]>,
) {
    // Skip if crosshair is not enabled at all
    if !state.crosshair.enabled {
        return;
    }

    let main_chart = &layout.main_chart;

    // Build corrected viewport and chart_rect matching what render_full_chart_panel used.
    let mut corrected_viewport = state.viewport.clone();
    corrected_viewport.chart_width = main_chart.chart.width;
    corrected_viewport.chart_height = main_chart.chart.height;

    let corrected_chart_rect = ChartRect {
        x: main_chart.chart.x,
        y: main_chart.chart.y,
        width: main_chart.chart.width,
        height: main_chart.chart.height,
    };

    let corrected_state = ChartRenderState {
        viewport: &corrected_viewport,
        chart_rect: corrected_chart_rect,
        price_scale: state.price_scale,
        time_scale: state.time_scale,
        bars: state.bars,
        grid: state.grid,
        crosshair: state.crosshair,
        legend: state.legend,
        theme: state.theme,
        time_ticks: state.time_ticks,
        current_timeframe: state.current_timeframe,
        disable_clip: state.disable_clip,
        time_format_settings: state.time_format_settings,
        timeframe_interval_ms: state.timeframe_interval_ms,
        scale_settings: state.scale_settings,
        body_enabled: state.body_enabled,
        border_enabled: state.border_enabled,
        wick_enabled: state.wick_enabled,
        use_prev_close: state.use_prev_close,
        footprint_bars: state.footprint_bars,
        footprint_display: state.footprint_display,
        tick_intensity: state.tick_intensity,
        renko_bars: state.renko_bars,
        three_line_break: state.three_line_break,
        pnf_columns: state.pnf_columns,
        kagi_segments: state.kagi_segments,
        tpo_sessions: state.tpo_sessions,
        tpo_display: state.tpo_display,
        line_family_display: state.line_family_display,
        ha_cache: state.ha_cache,
        vp_overlay: state.vp_overlay,
        tpo_profile_overlay: state.tpo_profile_overlay,
        oi_delta_ref: state.oi_delta_ref,
        funding_rate_ref: state.funding_rate_ref,
        dom_heatmap_ref: state.dom_heatmap_ref,
        dom_trade_dots_ref: state.dom_trade_dots_ref,
        liq_tape_ref: state.liq_tape_ref,
        liq_ink: state.liq_ink,
        liq_projection_ref: state.liq_projection_ref,
        liq_projection_ink: state.liq_projection_ink,
        vp_ink: state.vp_ink,
        tpo_ink: state.tpo_ink,
        dom_params: state.dom_params,
        non_temporal_axis: state.non_temporal_axis,
        hide_time_grid: state.hide_time_grid,
        history_depth: state.history_depth,
        cvd_paint_deltas: state.cvd_paint_deltas.clone(),
        trade_archive_available: state.trade_archive_available,
        bars_trade_built: state.bars_trade_built,
    };

    // Compute total chart bounds (main + all sub-panes) for the vertical crosshair span.
    let total_chart_top = layout.sub_panes.iter()
        .map(|p| p.content.y)
        .fold(main_chart.chart.y, f64::min);
    let total_chart_bottom = layout.sub_panes.iter()
        .map(|p| p.content.y + p.content.height)
        .fold(main_chart.chart.y + main_chart.chart.height, f64::max);

    // 1. Main chart crosshair (vertical + horizontal for main chart)
    draw_crosshair(
        ctx,
        &corrected_state,
        &config.crosshair_config,
        config.is_dragging,
        total_chart_top,
        total_chart_bottom,
    );

    // 2. Per-sub-pane horizontal crosshair + price label
    for (pane_idx, pane_layout) in layout.sub_panes.iter().enumerate() {
        let crosshair = state.crosshair;
        if crosshair.visible && crosshair.pane_index == Some(pane_idx) {
            let content = &pane_layout.content;
            let price_scale_rect = &pane_layout.price_scale;

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
                &config.crosshair_config,
                &state.theme.crosshair,
                config.is_dragging,
            );

            // Draw price label only when we have the sub-pane value range.
            if let Some(ranges) = sub_pane_ranges {
                if let Some(&(_, pane_min, pane_max)) = ranges.iter().find(|&&(idx, _, _)| idx == pane_idx) {
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
                        &config.scale_theme,
                        &config.scale_config,
                    );
                }
            }
        }
    }

    // 3. Price-scale cursor label
    // Suppress while the chart is in loading state — no series data means no
    // price axis is meaningful, so the cursor labels would chase an
    // undefined value. Own-slot chart types (footprint/renko/etc.) have a
    // meaningful price axis as soon as their derived data lands, even before
    // klines arrive.
    if state.crosshair.visible && state.has_any_series_data() {
        let mut leaf_scale_cfg = config.scale_config.clone();
        leaf_scale_cfg.price_scale_width = main_chart.price_scale.width;
        leaf_scale_cfg.time_scale_height = main_chart.time_scale.height;

        let text_x = main_chart.price_scale.x + leaf_scale_cfg.price_scale_width / 2.0;
        let dynamic_font_size = state.price_scale.calc_font_size(corrected_viewport.chart_height);

        draw_price_scale_cursor_label(
            ctx,
            state.price_scale,
            &corrected_viewport,
            &leaf_scale_cfg,
            &config.scale_theme,
            state.crosshair,
            main_chart.price_scale.x,
            main_chart.price_scale.y,
            text_x,
            dynamic_font_size,
        );

        // 4. Time-scale cursor label
        let label_y = main_chart.time_scale.y + leaf_scale_cfg.time_scale_height / 2.0;
        draw_time_scale_cursor_label(
            ctx,
            &corrected_viewport,
            state.bars,
            &leaf_scale_cfg,
            &config.scale_theme,
            state.crosshair,
            state.time_format_settings,
            main_chart.time_scale.x,
            main_chart.time_scale.y,
            label_y,
        );
    }
}

/// Render the remaining cursor-dependent overlay for a single chart panel:
/// in-progress drawing preview and the near-cursor tooltip.
///
/// These paint ABOVE the crosshair (see [`render_chart_crosshair_layer`])
/// but the relative order versus toolbar/dropdown chrome is unchanged from
/// before the 2026-07-25 split — `ChartApp::render_overlay` still calls this
/// after the toolbar/dropdown pass, so preview + tooltip keep painting on
/// top of them exactly as they did previously.
///
/// # Parameters
/// - `ctx`              – Render context
/// - `state`            – Chart render state (same one used for the static pass)
/// - `layout`           – Pre-computed `ExtendedFrameLayout` for this panel
///                        (the geometry is passed in — do NOT recompute it here)
/// - `drawing_manager`  – Optional drawing manager (for in-progress preview)
/// - `tooltip`          – Optional tooltip to render near the cursor
/// - `symbol`           – Optional symbol name (enables OHLC legend)
/// - `timeframe`        – Timeframe label (e.g. `"1H"`) for the OHLC legend
pub fn render_chart_overlay(
    ctx: &mut dyn RenderContext,
    state: &ChartRenderState,
    layout: &crate::layout::ExtendedFrameLayout,
    drawing_manager: Option<&dyn DrawingRenderView>,
    tooltip: Option<&mlc_core::chart::Tooltip>,
    symbol: Option<&str>,
    timeframe: &str,
) {
    // Skip if crosshair is not enabled at all — same master gate as the
    // crosshair layer, preserved so tooltip/preview never outlive it.
    if !state.crosshair.enabled {
        return;
    }

    let main_chart = &layout.main_chart;

    // Build corrected viewport and chart_rect matching what render_full_chart_panel used.
    let mut corrected_viewport = state.viewport.clone();
    corrected_viewport.chart_width = main_chart.chart.width;
    corrected_viewport.chart_height = main_chart.chart.height;

    let corrected_chart_rect = ChartRect {
        x: main_chart.chart.x,
        y: main_chart.chart.y,
        width: main_chart.chart.width,
        height: main_chart.chart.height,
    };

    let corrected_state = ChartRenderState {
        viewport: &corrected_viewport,
        chart_rect: corrected_chart_rect,
        price_scale: state.price_scale,
        time_scale: state.time_scale,
        bars: state.bars,
        grid: state.grid,
        crosshair: state.crosshair,
        legend: state.legend,
        theme: state.theme,
        time_ticks: state.time_ticks,
        current_timeframe: state.current_timeframe,
        disable_clip: state.disable_clip,
        time_format_settings: state.time_format_settings,
        timeframe_interval_ms: state.timeframe_interval_ms,
        scale_settings: state.scale_settings,
        body_enabled: state.body_enabled,
        border_enabled: state.border_enabled,
        wick_enabled: state.wick_enabled,
        use_prev_close: state.use_prev_close,
        footprint_bars: state.footprint_bars,
        footprint_display: state.footprint_display,
        tick_intensity: state.tick_intensity,
        renko_bars: state.renko_bars,
        three_line_break: state.three_line_break,
        pnf_columns: state.pnf_columns,
        kagi_segments: state.kagi_segments,
        tpo_sessions: state.tpo_sessions,
        tpo_display: state.tpo_display,
        line_family_display: state.line_family_display,
        ha_cache: state.ha_cache,
        vp_overlay: state.vp_overlay,
        tpo_profile_overlay: state.tpo_profile_overlay,
        oi_delta_ref: state.oi_delta_ref,
        funding_rate_ref: state.funding_rate_ref,
        dom_heatmap_ref: state.dom_heatmap_ref,
        dom_trade_dots_ref: state.dom_trade_dots_ref,
        liq_tape_ref: state.liq_tape_ref,
        liq_ink: state.liq_ink,
        liq_projection_ref: state.liq_projection_ref,
        liq_projection_ink: state.liq_projection_ink,
        vp_ink: state.vp_ink,
        tpo_ink: state.tpo_ink,
        dom_params: state.dom_params,
        non_temporal_axis: state.non_temporal_axis,
        hide_time_grid: state.hide_time_grid,
        history_depth: state.history_depth,
        cvd_paint_deltas: state.cvd_paint_deltas.clone(),
        trade_archive_available: state.trade_archive_available,
        bars_trade_built: state.bars_trade_built,
    };

    // 5. In-progress drawing preview — main chart
    if let Some(dm) = drawing_manager {
        // Preview for main chart: set up same coordinate space as render_main_chart_primitives
        if dm.is_drawing() && dm.current_pane().is_none() {
            let price_range = state.price_scale.price_max - state.price_scale.price_min;
            if state.viewport.bar_count > 0 && price_range > 0.0 && price_range.is_finite() {
                ctx.save();
                ctx.set_coordinate_space(
                    main_chart.chart.width,
                    main_chart.chart.height,
                    corrected_viewport.view_start,
                    corrected_viewport.bar_spacing,
                    state.price_scale.price_min,
                    state.price_scale.price_max,
                );
                ctx.set_bars(state.bars);
                if !state.disable_clip {
                    ctx.begin_path();
                    ctx.rect(main_chart.chart.x, main_chart.chart.y, main_chart.chart.width, main_chart.chart.height);
                    ctx.clip();
                }
                ctx.translate(main_chart.chart.x, main_chart.chart.y);
                draw_main_chart_drawing_preview(ctx, &corrected_state, dm, main_chart.chart.height);
                ctx.restore();
            }
        }

        // Preview for sub-panes
        if dm.is_drawing() && dm.current_pane().is_some() {
            for pane_layout in &layout.sub_panes {
                let instance_id = pane_layout.instance_id;
                if dm.current_pane() != Some(instance_id) {
                    continue;
                }
                let content = &pane_layout.content;
                // We don't know pane_min/pane_max here without re-running indicator range.
                // Use 0.0/100.0 as fallback (same as the label fallback).
                let pane_min = 0.0_f64;
                let pane_max = 100.0_f64;

                let pane_viewport = mlc_core::Viewport {
                    view_start: corrected_viewport.view_start,
                    bar_spacing: corrected_viewport.bar_spacing,
                    bar_width_ratio: corrected_viewport.bar_width_ratio,
                    chart_width: content.width,
                    chart_height: content.height,
                    bar_count: corrected_viewport.bar_count,
                };
                let pane_price_scale = mlc_core::PriceScale {
                    price_min: pane_min,
                    price_max: pane_max,
                    ..Default::default()
                };

                ctx.save();
                ctx.set_coordinate_space(
                    content.width,
                    content.height,
                    pane_viewport.view_start,
                    pane_viewport.bar_spacing,
                    pane_min,
                    pane_max,
                );
                ctx.set_bars(state.bars);
                if !state.disable_clip {
                    ctx.begin_path();
                    ctx.rect(content.x, content.y, content.width, content.height);
                    ctx.clip();
                }
                ctx.translate(content.x, content.y);
                draw_sub_pane_drawing_preview(ctx, &corrected_state, dm, instance_id, &pane_viewport, &pane_price_scale, content.height);
                ctx.restore();
            }
        }
    }

    // 6. Tooltip (near cursor)
    if let Some(tt) = tooltip {
        if tt.visible {
            draw_chart_tooltip(ctx, &corrected_state, tt);
        }
    }

    // NOTE: draw_chart_legend (OHLC-at-cursor row) is intentionally NOT called.
    // It was dead code in the pre-overlay static path (defined, never invoked) —
    // the overlay refactor must not resurrect it, or stale OHLC numbers render
    // unclipped over the scales/tab area. Kept defined for potential future use.
    let _ = (symbol, timeframe);
}

/// Which AABB-of-points culls to skip for a primitive whose drawn geometry
/// continues past its control points.
///
/// Returns `(skip_left, skip_right, skip_above, skip_below)`.
///
/// `extend_mode_raw`: 0=segment, 1=right, 2=left, 3=both. Inherent
/// unbounded tools (ray / extended line / horiz / cross / vertical) are
/// mapped onto that encoding so a pan that leaves both anchors off-screen
/// does not drop a line that still crosses the chart.
fn unbounded_cull_skips(prim: &dyn mlc_core::drawing::primitives_v2::Primitive) -> (bool, bool, bool, bool) {
    // Authority is the primitive: `extend_mode_raw` (time) and
    // `unbounded_in_price`. Segment (0) keeps AABB cull.
    //
    // A RAY/EXTENDED LINE has an arbitrary slope: "skip left only" is
    // wrong — both anchors can leave to the right while the ray still
    // crosses the view (owner report: ray vanishes, price-range does not).
    // When the drawn geometry is unbounded in PRICE it is also unbounded
    // in TIME regardless of the 1/2 encoding, which is only meaningful
    // for axis-aligned tools (horizontal ray, price-range levels).
    let ext = prim.extend_mode_raw();
    let skip_price = prim.unbounded_in_price();
    let skip_time_both = skip_price && ext != 0;
    let skip_left = skip_time_both || ext == 1 || ext == 3;
    let skip_right = skip_time_both || ext == 2 || ext == 3;
    (skip_left, skip_right, skip_price, skip_price)
}
