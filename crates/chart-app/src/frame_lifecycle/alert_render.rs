//! Alert bell icon rendering and indicator value computation for alerts.

use crate::ChartApp;
use zengeld_chart::{
    LayoutRect,
    render::RenderContext,
};
use zengeld_chart::indicator_source::IndicatorSource;
use zengeld_terminal_indicators::IndicatorManager;
use alerts::{AlertManager, AlertStatus, AlertSource};

impl ChartApp {
    // -------------------------------------------------------------------------
    // Alert indicator-value helper
    // -------------------------------------------------------------------------

    /// Build the `indicator_values` slice required by
    /// [`AlertManager::check_crossings_dynamic`] and
    /// [`AlertManager::resolve_price_static`].
    ///
    /// Iterates all active `AlertSource::Indicator` alerts, de-duplicates
    /// `(indicator_id, output_index)` pairs, looks up the corresponding
    /// `IndicatorRenderInstance` from `indicator_manager`, and returns the
    /// output's value buffer.
    pub(crate) fn build_indicator_values_for_alerts(
        alert_manager: &AlertManager,
        indicator_manager: &IndicatorManager,
    ) -> Vec<(u64, usize, Vec<f64>)> {
        use std::collections::HashSet;
        let mut result: Vec<(u64, usize, Vec<f64>)> = Vec::new();
        let mut seen: HashSet<(u64, usize)> = HashSet::new();

        for alert in alert_manager.items() {
            if alert.status != AlertStatus::Active {
                continue;
            }
            if let AlertSource::Indicator { indicator_id, output_index, .. } = &alert.source {
                if !seen.insert((*indicator_id, *output_index)) {
                    continue;
                }
                if let Some(render_inst) = indicator_manager.get_render_instance(*indicator_id) {
                    if let Some(output_def) = render_inst.output_defs.get(*output_index) {
                        if let Some(values) = render_inst.values.get(&output_def.name) {
                            result.push((*indicator_id, *output_index, values.clone()));
                        }
                    }
                }
            }
        }
        result
    }

    // -------------------------------------------------------------------------
    // Alert bell rendering
    // -------------------------------------------------------------------------

    /// Draw small bell icons at the rightmost endpoint of drawing primitives and
    /// indicators that have bound Active alerts.
    ///
    /// Returns a list of `(widget_id, x, y, size)` tuples so the caller can
    /// register each bell as a clickable zone with `input_coordinator`.
    ///
    /// # Parameters
    /// * `ctx` - Render context to draw into.
    /// * `chart_area_rect` - The corrected main chart area rectangle (excluding
    ///   price/time scales).  Used to convert bar/price coordinates to screen
    ///   pixels and to clip bell icons that fall outside the visible area.
    /// * `viewport` - Viewport for bar→X conversions (must already be corrected
    ///   to match `chart_area_rect` dimensions).
    /// * `price_min` / `price_max` - Visible price range for price→Y conversion.
    /// * `drawing_manager` - Access to drawing primitives.
    /// * `window_id` - Active chart window id (for filtering primitives).
    pub(crate) fn draw_alert_bell_icons(
        ctx: &mut dyn RenderContext,
        chart_area_rect: LayoutRect,
        viewport: &zengeld_chart::Viewport,
        price_min: f64,
        price_max: f64,
        drawing_manager: &zengeld_chart::DrawingManager,
        indicator_manager: &IndicatorManager,
        alert_manager: &AlertManager,
        window_id: Option<u64>,
        symbol: &str,
        exchange: &str,
        account_type: &str,
    ) -> Vec<(String, f64, f64, f64)> {
        const BELL_SIZE: f64 = 12.0;
        const BELL_MARGIN: f64 = 3.0; // gap between right edge and bell center

        let chart_x = chart_area_rect.x;
        let chart_y = chart_area_rect.y;
        let chart_w = chart_area_rect.width;
        let chart_h = chart_area_rect.height;

        let mut bells: Vec<(String, f64, f64, f64)> = Vec::new();

        // Helpers to clamp a bell position inside the visible chart area.
        let clamp_bell_x = |x: f64| -> f64 {
            x.min(chart_x + chart_w - BELL_SIZE / 2.0 - BELL_MARGIN)
             .max(chart_x + BELL_SIZE / 2.0)
        };
        let clamp_bell_y = |y: f64| -> f64 {
            y.min(chart_y + chart_h - BELL_SIZE / 2.0)
             .max(chart_y + BELL_SIZE / 2.0)
        };

        for alert in alert_manager.items() {
            if alert.status != AlertStatus::Active {
                continue;
            }
            if !alert.matches_window(symbol, exchange, account_type) {
                continue;
            }

            match &alert.source {
                AlertSource::Drawing { primitive_id, .. } => {
                    // Find the primitive.
                    let prim = drawing_manager
                        .primitives()
                        .iter()
                        .find(|p| {
                            p.data().id == *primitive_id
                                && p.data().window_id == window_id
                        });

                    let prim = match prim {
                        Some(p) => p,
                        None => continue,
                    };

                    let points = prim.points();
                    if points.is_empty() {
                        continue;
                    }

                    // Use point 2 (index 1) as the anchor; fall back to point 1 if only one exists.
                    let (bar2, price2) = if points.len() >= 2 { points[1] } else { points[0] };

                    // Convert point 2 to screen coordinates (relative to chart origin).
                    let rel_x2 = viewport.bar_to_x_f64(bar2);
                    let rel_y2 = viewport.price_to_y(price2, price_min, price_max);

                    let type_id = prim.type_id();

                    let (raw_bell_x, raw_bell_y) = if (type_id == "ray" || type_id == "extended_line")
                        && points.len() >= 2
                    {
                        // For projecting primitives, extrapolate to the right edge of the chart.
                        let (bar1, price1) = points[0];
                        let rel_x1 = viewport.bar_to_x_f64(bar1);
                        let rel_y1 = viewport.price_to_y(price1, price_min, price_max);

                        let dx = rel_x2 - rel_x1;
                        let dy = rel_y2 - rel_y1;

                        if dx > 0.001 {
                            // Project to the right edge.
                            let t_right = (chart_w - rel_x1) / dx;
                            let proj_x = chart_w; // at the right edge in relative coords
                            let proj_y = rel_y1 + dy * t_right;
                            (
                                chart_x + proj_x.min(chart_w) - BELL_MARGIN,
                                chart_y + proj_y,
                            )
                        } else {
                            // Non-rightward ray: fall back to point 2 position.
                            (chart_x + rel_x2, chart_y + rel_y2)
                        }
                    } else {
                        // Trend line and all other types: bell at point 2.
                        (chart_x + rel_x2, chart_y + rel_y2)
                    };

                    let bell_x = clamp_bell_x(raw_bell_x);
                    let bell_y = clamp_bell_y(raw_bell_y);

                    // Skip if the price anchor is outside the visible price range.
                    if price2 < price_min || price2 > price_max {
                        continue;
                    }

                    // Determine if the primitive body arrives from above (screen Y
                    // decreases toward point 2) — if so, flip the bell below the
                    // anchor so it doesn't overlap the line.
                    let flip_below = if points.len() >= 2 {
                        let rel_y1 = viewport.price_to_y(points[0].1, price_min, price_max);
                        // Line goes downward on screen (y1 < y2) → body is above → bell below.
                        rel_y1 < rel_y2
                    } else {
                        false
                    };

                    let color = &prim.data().color.stroke;
                    let widget_id = format!("alert_bell_drw_{}", primitive_id);

                    let (icon_cx, icon_cy) = Self::draw_bell_icon(ctx, bell_x, bell_y, BELL_SIZE, color, flip_below);
                    bells.push((widget_id, icon_cx, icon_cy, BELL_SIZE));
                }

                AlertSource::Indicator { indicator_id, output_index, .. } => {
                    // Get render instance for this indicator.
                    let render_inst = indicator_manager.get_render_instance(*indicator_id);
                    let render_inst = match render_inst {
                        Some(ri) => ri,
                        None => continue,
                    };

                    // Indicators on sub-panes (pane > 0) are not on the main chart — skip.
                    if render_inst.pane > 0 {
                        continue;
                    }

                    // Check that indicator belongs to this symbol and window.
                    let symbol_instances = match window_id {
                        Some(wid) => indicator_manager.get_instances_for_symbol_in_window(symbol, wid),
                        None => indicator_manager.get_instances_for_symbol(symbol),
                    };
                    if !symbol_instances.iter().any(|i| i.id == *indicator_id) {
                        continue;
                    }

                    // Find the output by index.
                    let output_def = match render_inst.output_defs.get(*output_index) {
                        Some(def) => def,
                        None => continue,
                    };

                    let values = match render_inst.values.get(&output_def.name) {
                        Some(v) => v,
                        None => continue,
                    };

                    // Find the last non-NaN value within the visible range.
                    let (vis_start, vis_end) = viewport.visible_range();
                    let search_end = vis_end.min(values.len());

                    let last_valid = (vis_start..search_end)
                        .rev()
                        .find(|&i| !values[i].is_nan());

                    let (bar_idx, price) = match last_valid {
                        Some(i) => (i, values[i]),
                        None => continue,
                    };

                    // Bell X is at the bar of the last valid value.
                    let rel_x = viewport.bar_to_x_f64(bar_idx as f64);
                    let raw_bell_x = chart_x + rel_x;

                    // Convert price to screen Y.
                    let rel_y = viewport.price_to_y(price, price_min, price_max);
                    let raw_bell_y = chart_y + rel_y;

                    let bell_x = clamp_bell_x(raw_bell_x);
                    let bell_y = clamp_bell_y(raw_bell_y);

                    // Clip.
                    if price < price_min || price > price_max {
                        continue;
                    }

                    // Determine slope at the last bar: if indicator is rising
                    // (prev value < current → line comes from below on screen)
                    // the body approaches from above on screen → flip bell below.
                    let flip_below = if bar_idx > 0 {
                        let prev_val = values[bar_idx - 1];
                        !prev_val.is_nan() && prev_val > price // price dropped → line goes down screen → body above
                    } else {
                        false
                    };

                    let color = render_inst.output_defs
                        .get(*output_index)
                        .map(|d| d.color.as_str())
                        .unwrap_or("#FF9800");

                    let widget_id = format!("alert_bell_ind_{}", indicator_id);
                    let (icon_cx, icon_cy) = Self::draw_bell_icon(ctx, bell_x, bell_y, BELL_SIZE, color, flip_below);
                    bells.push((widget_id, icon_cx, icon_cy, BELL_SIZE));
                }

                AlertSource::Signal { indicator_id, .. } => {
                    // Position the bell near the last visible signal marker for this indicator,
                    // or fall back to the right edge of the first output line.
                    let render_inst = indicator_manager.get_render_instance(*indicator_id);
                    let render_inst = match render_inst {
                        Some(ri) => ri,
                        None => continue,
                    };

                    // Indicators on sub-panes (pane > 0) are not on the main chart — skip.
                    if render_inst.pane > 0 {
                        continue;
                    }

                    // Check that indicator belongs to this symbol and window.
                    let symbol_instances = match window_id {
                        Some(wid) => indicator_manager.get_instances_for_symbol_in_window(symbol, wid),
                        None => indicator_manager.get_instances_for_symbol(symbol),
                    };
                    if !symbol_instances.iter().any(|i| i.id == *indicator_id) {
                        continue;
                    }

                    // Try to find the last visible signal position for this indicator.
                    let (vis_start, vis_end) = viewport.visible_range();

                    let signal_pos = render_inst.signals.iter()
                        .filter(|s| s.bar_index >= vis_start && s.bar_index < vis_end)
                        .max_by_key(|s| s.bar_index)
                        .map(|s| (s.bar_index, s.price));

                    // Fall back to the last non-NaN value of the first output line.
                    let anchor = signal_pos.or_else(|| {
                        render_inst.output_defs.first().and_then(|def| {
                            render_inst.values.get(&def.name).and_then(|vals| {
                                let search_end = vis_end.min(vals.len());
                                (vis_start..search_end)
                                    .rev()
                                    .find(|&i| !vals[i].is_nan())
                                    .map(|i| (i, vals[i]))
                            })
                        })
                    });

                    let (bar_idx, price) = match anchor {
                        Some(p) => p,
                        None => continue,
                    };

                    if price < price_min || price > price_max {
                        continue;
                    }

                    let rel_x = viewport.bar_to_x_f64(bar_idx as f64);
                    let raw_bell_x = chart_x + rel_x;
                    let rel_y = viewport.price_to_y(price, price_min, price_max);
                    let raw_bell_y = chart_y + rel_y;

                    let bell_x = clamp_bell_x(raw_bell_x);
                    let bell_y = clamp_bell_y(raw_bell_y);

                    let color = render_inst.output_defs
                        .first()
                        .map(|d| d.color.as_str())
                        .unwrap_or("#FF9800");

                    let widget_id = format!("alert_bell_ind_{}", indicator_id);
                    let (icon_cx, icon_cy) = Self::draw_bell_icon(ctx, bell_x, bell_y, BELL_SIZE, color, false);
                    bells.push((widget_id, icon_cx, icon_cy, BELL_SIZE));
                }

                _ => {}
            }
        }

        bells
    }

    /// Draw a small bell icon near `(cx, cy)`.
    ///
    /// `flip_below` — when `true` the bell is placed *below* the anchor
    /// instead of above, so it never overlaps the primitive/indicator body.
    ///
    /// Returns `(icon_center_x, icon_center_y)` for the clickable-zone.
    pub(crate) fn draw_bell_icon(
        ctx: &mut dyn RenderContext,
        cx: f64,
        cy: f64,
        size: f64,
        color: &str,
        flip_below: bool,
    ) -> (f64, f64) {
        const OFFSET_X: f64 = -12.0; // left of anchor
        const OFFSET_Y_UP: f64 = -7.0; // above anchor (default)
        const OFFSET_Y_DOWN: f64 = 7.0; // below anchor (flipped)

        let offset_y = if flip_below { OFFSET_Y_DOWN } else { OFFSET_Y_UP };

        let icon_x = cx + OFFSET_X - size / 2.0;
        let icon_y = cy + offset_y - size / 2.0;

        zengeld_chart::render::draw_svg_icon(
            ctx,
            zengeld_chart::ui::Icon::Alert.svg(),
            icon_x,
            icon_y,
            size,
            size,
            color,
        );

        // Return the visual center for clickable-zone registration.
        (cx + OFFSET_X, cy + offset_y)
    }
}
