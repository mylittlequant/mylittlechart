//! Chart and Heatmap Panel Renderers
//!
//! Render functions for 19 chart and heatmap visual panels.
//! All functions use the `uzor::render::RenderContext` API.

use crate::render::{RenderContext, TextAlign};
use crate::visual::financial::depth_chart::DepthChartState;
use crate::visual::financial::volatility_surface::VolatilitySurfaceState;
use crate::visual::financial::yield_curve::YieldCurveState;
use crate::visual::financial::treemap::TreemapState;
use crate::visual::financial::order_flow_heatmap::OrderFlowHeatmapState;
use crate::visual::financial::dom_surface::DomSurfaceState;
use crate::visual::financial::liquidation_heatmap::LiquidationHeatmapState;
use crate::visual::financial::pnl_surface::PnlSurfaceState;
use crate::visual::financial::horizon_chart::HorizonChartState;
use crate::visual::financial::calendar_heatmap::CalendarHeatmapState;
use crate::info::analytics::correlation_matrix::CorrelationMatrixState;
use crate::info::analytics::spread_chart::SpreadChartState;
use crate::info::analytics::sector_heatmap::SectorHeatmapState;
use crate::info::analytics::performance_analytics::PerformanceAnalyticsState;
use crate::info::analytics::pairs_trading::PairsTradingState;
use crate::info::options::payoff_diagram::PayoffDiagramState;
use crate::info::options::iv_surface::IvSurfaceState;
use crate::visual::realtime::gpu_timeseries::GpuTimeseriesState;
use crate::visual::realtime::streaming_heatmap::StreamingHeatmapState;

/// Convert RGBA array [0.0-1.0] to hex color string
fn rgba_to_hex(rgba: [f32; 4]) -> String {
    let r = (rgba[0].clamp(0.0, 1.0) * 255.0) as u8;
    let g = (rgba[1].clamp(0.0, 1.0) * 255.0) as u8;
    let b = (rgba[2].clamp(0.0, 1.0) * 255.0) as u8;
    let a = (rgba[3].clamp(0.0, 1.0) * 255.0) as u8;
    format!("#{:02x}{:02x}{:02x}{:02x}", r, g, b, a)
}

// ========== FINANCIAL PANELS ==========

/// Render Depth Chart Panel (Order Book Depth Visualization)
pub fn render_depth_chart_panel(
    ctx: &mut dyn RenderContext,
    state: &DepthChartState,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    const BG_COLOR: &str = "#0D1B2Aff";
    const GRID_COLOR: &str = "#1E2A384d";
    const BID_FILL_START: &str = "#00C85399";
    const ASK_FILL_START: &str = "#FF174499";
    const BID_LINE: &str = "#00C853ff";
    const ASK_LINE: &str = "#FF1744ff";
    const MID_PRICE_LINE: &str = "#FFC107ff";
    const AXIS_TEXT: &str = "#B0BEC5ff";

    // 1. Background
    ctx.set_fill_color(BG_COLOR);
    ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

    // 2. Horizontal grid lines (5 levels)
    ctx.set_fill_color(GRID_COLOR);
    for i in 1..5 {
        let grid_y = y + (h / 5.0) * i as f32;
        ctx.fill_rect(x as f64, grid_y as f64, w as f64, 1.0);
    }

    // 3. Vertical grid at mid-price
    let mid_x = x + w / 2.0;
    ctx.fill_rect(mid_x as f64, y as f64, 1.0, h as f64);

    // 4. Bid area fill
    let bid_points = state.bid_curve_points(w, h);
    if !bid_points.is_empty() {
        ctx.set_fill_color(BID_FILL_START);
        ctx.set_global_alpha(state.animation_progress as f64);
        ctx.begin_path();
        ctx.move_to((x + bid_points[0].0) as f64, (y + h) as f64);
        for (px, py) in &bid_points {
            ctx.line_to((x + px) as f64, (y + py) as f64);
        }
        ctx.line_to((x + bid_points.last().unwrap().0) as f64, (y + h) as f64);
        ctx.close_path();
        ctx.fill();
        ctx.set_global_alpha(1.0);
    }

    // 5. Ask area fill
    let ask_points = state.ask_curve_points(w, h);
    if !ask_points.is_empty() {
        ctx.set_fill_color(ASK_FILL_START);
        ctx.set_global_alpha(state.animation_progress as f64);
        ctx.begin_path();
        ctx.move_to((x + ask_points[0].0) as f64, (y + h) as f64);
        for (px, py) in &ask_points {
            ctx.line_to((x + px) as f64, (y + py) as f64);
        }
        ctx.line_to((x + ask_points.last().unwrap().0) as f64, (y + h) as f64);
        ctx.close_path();
        ctx.fill();
        ctx.set_global_alpha(1.0);
    }

    // 6. Bid line stroke
    if !bid_points.is_empty() {
        ctx.set_stroke_color(BID_LINE);
        ctx.set_stroke_width(2.0);
        ctx.begin_path();
        ctx.move_to((x + bid_points[0].0) as f64, (y + bid_points[0].1) as f64);
        for (px, py) in &bid_points[1..] {
            ctx.line_to((x + px) as f64, (y + py) as f64);
        }
        ctx.stroke();
    }

    // 7. Ask line stroke
    if !ask_points.is_empty() {
        ctx.set_stroke_color(ASK_LINE);
        ctx.set_stroke_width(2.0);
        ctx.begin_path();
        ctx.move_to((x + ask_points[0].0) as f64, (y + ask_points[0].1) as f64);
        for (px, py) in &ask_points[1..] {
            ctx.line_to((x + px) as f64, (y + py) as f64);
        }
        ctx.stroke();
    }

    // 8. Mid-price vertical line
    ctx.set_fill_color(MID_PRICE_LINE);
    let mid_price_x = x + state.mid_price_x(w);
    ctx.fill_rect(mid_price_x as f64, y as f64, 1.0, h as f64);

    // 9. Axis labels
    ctx.set_fill_color(AXIS_TEXT);
    ctx.set_font("11px sans-serif");
    ctx.set_text_align(TextAlign::Center);
    ctx.fill_text(
        &format!("{:.2}", state.mid_price()),
        mid_price_x as f64,
        (y + h + 15.0) as f64,
    );

    ctx.set_text_align(TextAlign::Right);
    ctx.fill_text(
        &format!("{:.0}K", state.max_depth_volume() / 1000.0),
        (x + w - 5.0) as f64,
        (y + 15.0) as f64,
    );
}

/// Render Volatility Surface Panel (3D IV Surface)
pub fn render_volatility_surface_panel(
    ctx: &mut dyn RenderContext,
    state: &VolatilitySurfaceState,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    const BG_GRADIENT_START: &str = "#0A1929ff";
    const AXIS_COLOR: &str = "#455A64ff";
    const WIREFRAME_COLOR: &str = "#37474F66";
    const AXIS_TEXT: &str = "#B0BEC5ff";

    // 1. Background
    ctx.set_fill_color(BG_GRADIENT_START);
    ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

    // 2. 3D axis lines
    ctx.set_stroke_color(AXIS_COLOR);
    ctx.set_stroke_width(2.0);

    // X-axis (strike)
    ctx.begin_path();
    ctx.move_to((x + 50.0) as f64, (y + h - 50.0) as f64);
    ctx.line_to((x + w - 50.0) as f64, (y + h - 50.0) as f64);
    ctx.stroke();

    // Y-axis (IV)
    ctx.begin_path();
    ctx.move_to((x + 50.0) as f64, (y + h - 50.0) as f64);
    ctx.line_to((x + 50.0) as f64, (y + 50.0) as f64);
    ctx.stroke();

    // Z-axis (expiry, diagonal)
    ctx.begin_path();
    ctx.move_to((x + 50.0) as f64, (y + h - 50.0) as f64);
    ctx.line_to((x + w - 50.0) as f64, (y + 100.0) as f64);
    ctx.stroke();

    // 3. Wireframe grid
    ctx.set_stroke_color(WIREFRAME_COLOR);
    ctx.set_stroke_width(1.0);
    let wireframe_lines = state.wireframe_lines(w - 100.0, h - 100.0);
    for ((x1, y1), (x2, y2)) in wireframe_lines {
        ctx.begin_path();
        ctx.move_to((x + 50.0 + x1) as f64, (y + 50.0 + y1) as f64);
        ctx.line_to((x + 50.0 + x2) as f64, (y + 50.0 + y2) as f64);
        ctx.stroke();
    }

    // 4. Surface points (colored by IV)
    let points = state.visible_grid_points(w - 100.0, h - 100.0);
    for (px, py, iv) in points {
        let color = state.iv_color(iv);
        let hex = rgba_to_hex(color);
        ctx.set_fill_color(&hex);
        ctx.fill_rect(
            (x + 50.0 + px - 2.0) as f64,
            (y + 50.0 + py - 2.0) as f64,
            4.0,
            4.0,
        );
    }

    // 5. Axis labels
    ctx.set_fill_color(AXIS_TEXT);
    ctx.set_font("11px sans-serif");
    ctx.set_text_align(TextAlign::Center);

    let strike_labels = state.strike_axis();
    for (i, (_strike, label)) in strike_labels.iter().enumerate() {
        let label_x = x + 50.0 + (w - 100.0) * (i as f32 / strike_labels.len().max(1) as f32);
        ctx.fill_text(&label, label_x as f64, (y + h - 30.0) as f64);
    }

    let expiry_labels = state.expiry_axis();
    ctx.set_text_align(TextAlign::Left);
    for (i, (_, label)) in expiry_labels.iter().enumerate() {
        let label_y = y + h - 50.0 - (h - 100.0) * (i as f32 / expiry_labels.len().max(1) as f32);
        ctx.fill_text(&label, (x + w - 40.0) as f64, label_y as f64);
    }
}

/// Render Yield Curve Panel
pub fn render_yield_curve_panel(
    ctx: &mut dyn RenderContext,
    state: &YieldCurveState,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    const BG_COLOR: &str = "#0D1B2Aff";
    const GRID_COLOR: &str = "#1E2A384d";
    const CURRENT_CURVE: &str = "#F44336ff";
    const TRAILING_CURVE: &str = "#21212133";
    const AXIS_TEXT: &str = "#B0BEC5ff";
    const DATA_POINT: &str = "#FFFFFFff";

    // 1. Background
    ctx.set_fill_color(BG_COLOR);
    ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

    // 2. Horizontal grid lines
    ctx.set_fill_color(GRID_COLOR);
    for i in 1..5 {
        let grid_y = y + (h / 5.0) * i as f32;
        ctx.fill_rect(x as f64, grid_y as f64, w as f64, 1.0);
    }

    // 3. Vertical grid at maturity points
    ctx.set_fill_color(GRID_COLOR);
    for i in 0..=7 {
        let grid_x = x + (w / 7.0) * i as f32;
        ctx.fill_rect(grid_x as f64, y as f64, 1.0, h as f64);
    }

    // 4. Trailing curves (faded historical)
    for curve_name in &state.selected_curves {
        if curve_name != "current" {
            let points = state.curve_points(curve_name, w, h);
            if !points.is_empty() {
                ctx.set_stroke_color(TRAILING_CURVE);
                ctx.set_stroke_width(1.5);
                ctx.begin_path();
                ctx.move_to((x + points[0].0) as f64, (y + points[0].1) as f64);
                for (px, py) in &points[1..] {
                    ctx.line_to((x + px) as f64, (y + py) as f64);
                }
                ctx.stroke();
            }
        }
    }

    // 5. Current curve
    if let Some(curve_name) = state.selected_curves.first() {
        let points = state.curve_points(curve_name, w, h);
        if !points.is_empty() {
            ctx.set_stroke_color(CURRENT_CURVE);
            ctx.set_stroke_width(3.0);
            ctx.begin_path();
            ctx.move_to((x + points[0].0) as f64, (y + points[0].1) as f64);
            for (px, py) in &points[1..] {
                ctx.line_to((x + px) as f64, (y + py) as f64);
            }
            ctx.stroke();

            // 6. Data points
            ctx.set_fill_color(CURRENT_CURVE);
            for (px, py) in &points {
                ctx.begin_path();
                ctx.arc(
                    (x + px) as f64,
                    (y + py) as f64,
                    5.0,
                    0.0,
                    std::f64::consts::PI * 2.0,
                );
                ctx.fill();

                // White stroke
                ctx.set_stroke_color(DATA_POINT);
                ctx.set_stroke_width(1.0);
                ctx.begin_path();
                ctx.arc(
                    (x + px) as f64,
                    (y + py) as f64,
                    5.0,
                    0.0,
                    std::f64::consts::PI * 2.0,
                );
                ctx.stroke();
            }
        }
    }

    // 7. Axis labels
    ctx.set_fill_color(AXIS_TEXT);
    ctx.set_font("11px sans-serif");
    ctx.set_text_align(TextAlign::Center);

    let maturities = ["1M", "3M", "6M", "1Y", "2Y", "5Y", "10Y", "30Y"];
    for (i, label) in maturities.iter().enumerate() {
        let label_x = x + (w / 7.0) * i as f32;
        ctx.fill_text(label, label_x as f64, (y + h + 15.0) as f64);
    }
}

/// Render Treemap Panel (Market Cap Treemap)
pub fn render_treemap_panel(
    ctx: &mut dyn RenderContext,
    state: &TreemapState,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    const BG_COLOR: &str = "#000000ff";
    const BORDER_COLOR: &str = "#1A1A1Aff";
    const STOCK_TEXT: &str = "#FFFFFFff";

    // 1. Background
    ctx.set_fill_color(BG_COLOR);
    ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

    // 2. Layout rectangles
    let rects = state.layout_rects();

    for rect in rects {
        // 3. Fill rectangle with color based on change %
        let color = state.node_color(rect.value);
        let hex = rgba_to_hex(color);
        ctx.set_fill_color(&hex);
        ctx.fill_rect(
            (x + rect.x) as f64,
            (y + rect.y) as f64,
            rect.w as f64,
            rect.h as f64,
        );

        // 4. Border
        ctx.set_fill_color(BORDER_COLOR);
        // Top
        ctx.fill_rect((x + rect.x) as f64, (y + rect.y) as f64, rect.w as f64, 1.0);
        // Bottom
        ctx.fill_rect(
            (x + rect.x) as f64,
            (y + rect.y + rect.h - 1.0) as f64,
            rect.w as f64,
            1.0,
        );
        // Left
        ctx.fill_rect((x + rect.x) as f64, (y + rect.y) as f64, 1.0, rect.h as f64);
        // Right
        ctx.fill_rect(
            (x + rect.x + rect.w - 1.0) as f64,
            (y + rect.y) as f64,
            1.0,
            rect.h as f64,
        );

        // 5. Label (if space available)
        if rect.w * rect.h > 1000.0 {
            let display_label = state.format_label(&rect.label, rect.w);
            if !display_label.is_empty() {
                ctx.set_fill_color(STOCK_TEXT);
                ctx.set_font("10px sans-serif");
                ctx.set_text_align(TextAlign::Left);
                ctx.fill_text(
                    &display_label,
                    (x + rect.x + 4.0) as f64,
                    (y + rect.y + 14.0) as f64,
                );
            }
        }
    }
}

/// Render Order Flow Heatmap Panel
pub fn render_order_flow_heatmap_panel(
    ctx: &mut dyn RenderContext,
    state: &OrderFlowHeatmapState,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    const BG_COLOR: &str = "#0A0E12ff";
    const GRID_COLOR: &str = "#1A1F2633";
    const CURRENT_PRICE_LINE: &str = "#FFC107ff";
    const AXIS_TEXT: &str = "#90A4AEff";

    // 1. Background
    ctx.set_fill_color(BG_COLOR);
    ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

    // 2. Horizontal grid lines (price levels)
    ctx.set_fill_color(GRID_COLOR);
    for i in 1..10 {
        let grid_y = y + (h / 10.0) * i as f32;
        ctx.fill_rect(x as f64, grid_y as f64, w as f64, 1.0);
    }

    // 3. Heatmap cells
    let cells = state.visible_cells(w, h);
    for (time_idx, price_idx, cell_x, cell_y, cell_w, cell_h) in cells {
        let color = state.intensity_color(time_idx, price_idx);
        if color[3] > 0.0 {
            let hex = rgba_to_hex(color);
            ctx.set_fill_color(&hex);
            ctx.fill_rect(
                (x + cell_x) as f64,
                (y + cell_y) as f64,
                cell_w as f64,
                cell_h as f64,
            );
        }
    }

    // 4. Current price horizontal line
    let (min_price, max_price) = state.price_range();
    let price_range = max_price - min_price;
    if price_range > 0.0 {
        let current_price = (min_price + max_price) / 2.0;
        let price_y = y + h * (1.0 - ((current_price - min_price) / price_range) as f32);
        ctx.set_fill_color(CURRENT_PRICE_LINE);
        ctx.fill_rect(x as f64, price_y as f64, w as f64, 2.0);
    }

    // 5. Axis labels
    ctx.set_fill_color(AXIS_TEXT);
    ctx.set_font("10px sans-serif");
    ctx.set_text_align(TextAlign::Right);

    for i in 0..=4 {
        let price = min_price + (max_price - min_price) * (i as f64 / 4.0);
        let label_y = y + h * (1.0 - i as f32 / 4.0);
        ctx.fill_text(&format!("{:.2}", price), (x + w - 5.0) as f64, label_y as f64);
    }
}

/// Render DOM Surface Panel (3D Order Book)
pub fn render_dom_surface_panel(
    ctx: &mut dyn RenderContext,
    state: &DomSurfaceState,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    const BG_GRADIENT_START: &str = "#0A1929ff";
    const AXIS_COLOR: &str = "#455A64ff";
    const AXIS_TEXT: &str = "#B0BEC5ff";

    // 1. Background
    ctx.set_fill_color(BG_GRADIENT_START);
    ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

    // 2. 3D axes
    ctx.set_stroke_color(AXIS_COLOR);
    ctx.set_stroke_width(2.0);

    // X-axis (price)
    ctx.begin_path();
    ctx.move_to((x + 50.0) as f64, (y + h - 50.0) as f64);
    ctx.line_to((x + w - 50.0) as f64, (y + h - 50.0) as f64);
    ctx.stroke();

    // Y-axis (volume/depth)
    ctx.begin_path();
    ctx.move_to((x + 50.0) as f64, (y + h - 50.0) as f64);
    ctx.line_to((x + 50.0) as f64, (y + 50.0) as f64);
    ctx.stroke();

    // Z-axis (time)
    ctx.begin_path();
    ctx.move_to((x + 50.0) as f64, (y + h - 50.0) as f64);
    ctx.line_to((x + w - 100.0) as f64, (y + 100.0) as f64);
    ctx.stroke();

    // 3. Surface points (colored by depth)
    let points = state.surface_points(w - 100.0, h - 100.0);
    for (px, py, depth) in points {
        let is_bid = py < h / 2.0;
        let color = state.depth_color(depth, is_bid);
        let hex = rgba_to_hex(color);
        ctx.set_fill_color(&hex);

        let point_size = (depth * 10.0).min(20.0) as f32;
        ctx.begin_path();
        ctx.arc(
            (x + 50.0 + px) as f64,
            (y + 50.0 + py) as f64,
            point_size as f64,
            0.0,
            std::f64::consts::PI * 2.0,
        );
        ctx.fill();
    }

    // 4. Axis labels
    ctx.set_fill_color(AXIS_TEXT);
    ctx.set_font("11px sans-serif");
    ctx.set_text_align(TextAlign::Center);
    ctx.fill_text("Price", (x + w / 2.0) as f64, (y + h - 20.0) as f64);

    ctx.set_text_align(TextAlign::Left);
    ctx.fill_text("Volume", (x + 10.0) as f64, (y + h / 2.0) as f64);
}

/// Render Liquidation Heatmap Panel
pub fn render_liquidation_heatmap_panel(
    ctx: &mut dyn RenderContext,
    state: &LiquidationHeatmapState,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    const BG_COLOR: &str = "#0D0D0Dff";
    const GRID_COLOR: &str = "#1A1A1A33";
    const CURRENT_PRICE_LINE: &str = "#00E676ff";
    const AXIS_TEXT: &str = "#9E9E9Eff";

    // 1. Background
    ctx.set_fill_color(BG_COLOR);
    ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

    // 2. Horizontal grid lines
    ctx.set_fill_color(GRID_COLOR);
    for i in 1..10 {
        let grid_y = y + (h / 10.0) * i as f32;
        ctx.fill_rect(x as f64, grid_y as f64, w as f64, 1.0);
    }

    // 3. Heatmap cells (liquidation intensity)
    let cells = state.visible_cells(w, h);
    for (_time_idx, _price_idx, cell_x, cell_y, cell_w, cell_h, liq_volume) in cells {
        let color = state.liquidation_color(liq_volume);
        if color[3] > 0.0 {
            let hex = rgba_to_hex(color);
            ctx.set_fill_color(&hex);
            ctx.fill_rect(
                (x + cell_x) as f64,
                (y + cell_y) as f64,
                cell_w as f64,
                cell_h as f64,
            );
        }
    }

    // 4. Current price line
    let (min_price, max_price) = state.price_range;
    let price_range = max_price - min_price;
    if price_range > 0.0 {
        let current_price = (min_price + max_price) / 2.0;
        let price_y = y + h * (1.0 - ((current_price - min_price) / price_range) as f32);
        ctx.set_fill_color(CURRENT_PRICE_LINE);
        ctx.fill_rect(x as f64, price_y as f64, w as f64, 2.0);
    }

    // 5. Axis labels
    ctx.set_fill_color(AXIS_TEXT);
    ctx.set_font("11px sans-serif");
    ctx.set_text_align(TextAlign::Right);

    for i in 0..=4 {
        let price = min_price + (max_price - min_price) * (i as f64 / 4.0);
        let label_y = y + h * (1.0 - i as f32 / 4.0);
        ctx.fill_text(&format!("{:.0}", price), (x + w - 10.0) as f64, label_y as f64);
    }
}

/// Render PnL Surface Panel (Options Payoff 3D)
pub fn render_pnl_surface_panel(
    ctx: &mut dyn RenderContext,
    state: &PnlSurfaceState,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    const BG_GRADIENT_START: &str = "#0F1419ff";
    const AXIS_COLOR: &str = "#546E7Aff";
    const ZERO_PLANE: &str = "#37474F33";
    const BREAKEVEN_LINE: &str = "#FFEB3Bff";
    const AXIS_TEXT: &str = "#B0BEC5ff";

    // 1. Background
    ctx.set_fill_color(BG_GRADIENT_START);
    ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

    // 2. 3D axes
    ctx.set_stroke_color(AXIS_COLOR);
    ctx.set_stroke_width(2.0);

    // X-axis (stock price)
    ctx.begin_path();
    ctx.move_to((x + 50.0) as f64, (y + h - 50.0) as f64);
    ctx.line_to((x + w - 50.0) as f64, (y + h - 50.0) as f64);
    ctx.stroke();

    // Y-axis (P&L)
    ctx.begin_path();
    ctx.move_to((x + 50.0) as f64, (y + h - 50.0) as f64);
    ctx.line_to((x + 50.0) as f64, (y + 50.0) as f64);
    ctx.stroke();

    // Z-axis (time to expiry)
    ctx.begin_path();
    ctx.move_to((x + 50.0) as f64, (y + h - 50.0) as f64);
    ctx.line_to((x + w - 100.0) as f64, (y + 100.0) as f64);
    ctx.stroke();

    // 3. Zero P&L plane (horizontal reference)
    ctx.set_fill_color(ZERO_PLANE);
    let zero_y = y + h / 2.0;
    ctx.fill_rect((x + 50.0) as f64, zero_y as f64, (w - 100.0) as f64, 1.0);

    // 4. Surface grid (colored by P&L)
    let grid = state.surface_grid(w - 100.0, h - 100.0);
    for (px, py, pnl) in grid {
        let color = state.pnl_color(pnl);
        let hex = rgba_to_hex(color);
        ctx.set_fill_color(&hex);
        ctx.fill_rect(
            (x + 50.0 + px - 2.0) as f64,
            (y + 50.0 + py - 2.0) as f64,
            4.0,
            4.0,
        );
    }

    // 5. Breakeven line (where P&L = 0)
    let breakeven_points = state.breakeven_line(w - 100.0, h - 100.0);
    if !breakeven_points.is_empty() {
        ctx.set_stroke_color(BREAKEVEN_LINE);
        ctx.set_stroke_width(3.0);
        ctx.begin_path();
        ctx.move_to(
            (x + 50.0 + breakeven_points[0].0) as f64,
            (y + 50.0 + breakeven_points[0].1) as f64,
        );
        for (px, py) in &breakeven_points[1..] {
            ctx.line_to((x + 50.0 + px) as f64, (y + 50.0 + py) as f64);
        }
        ctx.stroke();
    }

    // 6. Axis labels
    ctx.set_fill_color(AXIS_TEXT);
    ctx.set_font("12px sans-serif");
    ctx.set_text_align(TextAlign::Center);
    ctx.fill_text("Stock Price", (x + w / 2.0) as f64, (y + h - 20.0) as f64);

    ctx.set_text_align(TextAlign::Left);
    ctx.fill_text("P&L", (x + 10.0) as f64, (y + h / 2.0) as f64);
}

/// Render Horizon Chart Panel
pub fn render_horizon_chart_panel(
    ctx: &mut dyn RenderContext,
    state: &HorizonChartState,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    const BG_COLOR: &str = "#0D1B2Aff";
    const BASELINE_COLOR: &str = "#37474Fff";
    const AXIS_TEXT: &str = "#B0BEC5ff";

    // 1. Background
    ctx.set_fill_color(BG_COLOR);
    ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

    let row_count = state.series.len().max(1);
    let row_height = h / row_count as f32;

    for (series_idx, series) in state.series.iter().enumerate() {
        let row_y = y + series_idx as f32 * row_height;

        // 2. Baseline
        ctx.set_fill_color(BASELINE_COLOR);
        ctx.fill_rect(
            x as f64,
            (row_y + row_height / 2.0) as f64,
            w as f64,
            1.0,
        );

        // 3. Band data (split into positive and negative)
        let (positive, negative) = state.positive_negative_split(series_idx);

        // 4. Render negative bands (layered, mirrored upward)
        for band_idx in 0..state.bands {
            let band_color = state.band_color(band_idx, false);
            let hex = rgba_to_hex(band_color);
            ctx.set_fill_color(&hex);

            ctx.begin_path();
            ctx.move_to(x as f64, (row_y + row_height / 2.0) as f64);
            for (timestamp, value) in &negative {
                let time_x = x + w * (*timestamp as f32 / 86400.0);
                let band_height = value.abs() as f32 * row_height / state.bands as f32;
                ctx.line_to(
                    time_x as f64,
                    (row_y + row_height / 2.0 + band_height) as f64,
                );
            }
            ctx.line_to((x + w) as f64, (row_y + row_height / 2.0) as f64);
            ctx.close_path();
            ctx.fill();
        }

        // 5. Render positive bands (layered upward)
        for band_idx in 0..state.bands {
            let band_color = state.band_color(band_idx, true);
            let hex = rgba_to_hex(band_color);
            ctx.set_fill_color(&hex);

            ctx.begin_path();
            ctx.move_to(x as f64, (row_y + row_height / 2.0) as f64);
            for (timestamp, value) in &positive {
                let time_x = x + w * (*timestamp as f32 / 86400.0);
                let band_height = *value as f32 * row_height / state.bands as f32;
                ctx.line_to(
                    time_x as f64,
                    (row_y + row_height / 2.0 - band_height) as f64,
                );
            }
            ctx.line_to((x + w) as f64, (row_y + row_height / 2.0) as f64);
            ctx.close_path();
            ctx.fill();
        }

        // 6. Series label
        ctx.set_fill_color(AXIS_TEXT);
        ctx.set_font("9px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.fill_text(&series.label, (x + 5.0) as f64, (row_y + 12.0) as f64);
    }
}

/// Render Calendar Heatmap Panel
pub fn render_calendar_heatmap_panel(
    ctx: &mut dyn RenderContext,
    state: &CalendarHeatmapState,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    const BG_COLOR: &str = "#0D1117ff";
    const CELL_BORDER: &str = "#1B1F23ff";
    const WEEKDAY_TEXT: &str = "#7D8590ff";
    const MONTH_TEXT: &str = "#7D8590ff";

    // 1. Background
    ctx.set_fill_color(BG_COLOR);
    ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

    let cell_size = 12.0;
    let cell_spacing = 2.0;

    // 2. Grid cells
    let cells = state.visible_cells();
    for (_date_str, value, week, weekday) in cells {
        let cell_x = x + 30.0 + week as f32 * (cell_size + cell_spacing);
        let cell_y = y + 20.0 + weekday as f32 * (cell_size + cell_spacing);

        // Cell background color
        let color = state.value_color(value);
        let hex = rgba_to_hex(color);
        ctx.set_fill_color(&hex);
        ctx.fill_rounded_rect(
            cell_x as f64,
            cell_y as f64,
            cell_size as f64,
            cell_size as f64,
            2.0,
        );

        // Cell border
        ctx.set_stroke_color(CELL_BORDER);
        ctx.set_stroke_width(1.0);
        ctx.begin_path();
        ctx.move_to(cell_x as f64, cell_y as f64);
        ctx.line_to((cell_x + cell_size) as f64, cell_y as f64);
        ctx.line_to((cell_x + cell_size) as f64, (cell_y + cell_size) as f64);
        ctx.line_to(cell_x as f64, (cell_y + cell_size) as f64);
        ctx.close_path();
        ctx.stroke();
    }

    // 3. Weekday labels
    ctx.set_fill_color(WEEKDAY_TEXT);
    ctx.set_font("9px sans-serif");
    ctx.set_text_align(TextAlign::Right);

    let weekdays = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    for (i, label) in weekdays.iter().enumerate() {
        let label_y = y + 20.0 + i as f32 * (cell_size + cell_spacing) + cell_size / 2.0;
        ctx.fill_text(label, (x + 25.0) as f64, label_y as f64);
    }

    // 4. Month labels
    ctx.set_fill_color(MONTH_TEXT);
    ctx.set_font("10px sans-serif");
    ctx.set_text_align(TextAlign::Left);

    let month_labels = state.month_labels();
    for (i, label) in month_labels.iter().enumerate() {
        let label_x = x + 30.0 + (i * 4) as f32 * (cell_size + cell_spacing);
        ctx.fill_text(&label, label_x as f64, (y + 12.0) as f64);
    }
}

// ========== ANALYTICS PANELS ==========

/// Render Correlation Matrix Panel
pub fn render_correlation_matrix_panel(
    ctx: &mut dyn RenderContext,
    state: &CorrelationMatrixState,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    const BG_COLOR: &str = "#0D1B2Aff";
    const CELL_BORDER: &str = "#37474Fff";
    const AXIS_TEXT: &str = "#B0BEC5ff";
    const TEXT_ON_DARK: &str = "#FFFFFFff";
    const TEXT_ON_LIGHT: &str = "#000000ff";

    // 1. Background
    ctx.set_fill_color(BG_COLOR);
    ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

    let grid_size = state.grid_size();
    if grid_size == 0 {
        return;
    }

    let cell_size = (w.min(h) - 100.0) / grid_size as f32;

    // 2. Grid cells
    let cells = state.visible_cells();
    for (row, col, corr_value) in cells {
        let cell_x = x + 50.0 + col as f32 * cell_size;
        let cell_y = y + 50.0 + row as f32 * cell_size;

        // Cell background
        let color = state.correlation_color(corr_value);
        let hex = rgba_to_hex(color);
        ctx.set_fill_color(&hex);
        ctx.fill_rect(
            cell_x as f64,
            cell_y as f64,
            cell_size as f64,
            cell_size as f64,
        );

        // Cell border
        ctx.set_fill_color(CELL_BORDER);
        ctx.fill_rect(cell_x as f64, cell_y as f64, cell_size as f64, 1.0);
        ctx.fill_rect(
            cell_x as f64,
            (cell_y + cell_size - 1.0) as f64,
            cell_size as f64,
            1.0,
        );
        ctx.fill_rect(cell_x as f64, cell_y as f64, 1.0, cell_size as f64);
        ctx.fill_rect(
            (cell_x + cell_size - 1.0) as f64,
            cell_y as f64,
            1.0,
            cell_size as f64,
        );

        // Correlation value text (if cell is large enough)
        if cell_size > 40.0 {
            let text_color = if corr_value.abs() > 0.5 {
                TEXT_ON_DARK
            } else {
                TEXT_ON_LIGHT
            };
            ctx.set_fill_color(text_color);
            ctx.set_font("10px sans-serif");
            ctx.set_text_align(TextAlign::Center);
            ctx.fill_text(
                &state.format_value(corr_value),
                (cell_x + cell_size / 2.0) as f64,
                (cell_y + cell_size / 2.0 + 4.0) as f64,
            );
        }
    }

    // 3. Asset labels (X-axis)
    ctx.set_fill_color(AXIS_TEXT);
    ctx.set_font("11px sans-serif");
    ctx.set_text_align(TextAlign::Center);
    for (i, asset) in state.assets.iter().enumerate() {
        let label_x = x + 50.0 + i as f32 * cell_size + cell_size / 2.0;
        ctx.fill_text(asset, label_x as f64, (y + 40.0) as f64);
    }

    // 4. Asset labels (Y-axis)
    ctx.set_text_align(TextAlign::Right);
    for (i, asset) in state.assets.iter().enumerate() {
        let label_y = y + 50.0 + i as f32 * cell_size + cell_size / 2.0;
        ctx.fill_text(asset, (x + 45.0) as f64, (label_y + 4.0) as f64);
    }
}

/// Render Spread Chart Panel
pub fn render_spread_chart_panel(
    ctx: &mut dyn RenderContext,
    state: &SpreadChartState,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    const BG_COLOR: &str = "#0D1B2Aff";
    const GRID_COLOR: &str = "#1E2A384d";
    const SPREAD_LINE: &str = "#2196F3ff";
    const MEAN_LINE: &str = "#FFC107ff";
    const STD_DEV_BAND: &str = "#FFC1071a";
    const ZERO_LINE: &str = "#607D8Bff";
    const AXIS_TEXT: &str = "#B0BEC5ff";

    // 1. Background
    ctx.set_fill_color(BG_COLOR);
    ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

    // 2. Horizontal grid lines
    ctx.set_fill_color(GRID_COLOR);
    for i in 1..5 {
        let grid_y = y + (h / 5.0) * i as f32;
        ctx.fill_rect(x as f64, grid_y as f64, w as f64, 1.0);
    }

    // 3. Zero line
    let zero_y = state.zero_line_y(h);
    ctx.set_fill_color(ZERO_LINE);
    ctx.fill_rect(x as f64, (y + zero_y) as f64, w as f64, 2.0);

    // 4. Std dev bands (if enabled)
    let _mean = state.stats.mean;
    let std_dev = state.stats.std_dev;

    // Band: mean ± std_dev
    ctx.set_fill_color(STD_DEV_BAND);
    let band_top_y = y + zero_y - (std_dev as f32 * 20.0);
    let band_bottom_y = y + zero_y + (std_dev as f32 * 20.0);
    ctx.fill_rect(
        x as f64,
        band_top_y as f64,
        w as f64,
        (band_bottom_y - band_top_y) as f64,
    );

    // 5. Mean line
    ctx.set_fill_color(MEAN_LINE);
    let mean_y = y + zero_y;
    ctx.fill_rect(x as f64, mean_y as f64, w as f64, 1.0);

    // 6. Spread line
    let points = state.spread_points(w, h);
    if !points.is_empty() {
        ctx.set_stroke_color(SPREAD_LINE);
        ctx.set_stroke_width(2.0);
        ctx.begin_path();
        ctx.move_to((x + points[0].0) as f64, (y + points[0].1) as f64);
        for (px, py) in &points[1..] {
            ctx.line_to((x + px) as f64, (y + py) as f64);
        }
        ctx.stroke();
    }

    // 7. Axis labels
    ctx.set_fill_color(AXIS_TEXT);
    ctx.set_font("11px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.fill_text(
        &format!("{} / {}", state.instrument_a, state.instrument_b),
        (x + 10.0) as f64,
        (y + 20.0) as f64,
    );

    ctx.set_text_align(TextAlign::Right);
    ctx.fill_text(
        &format!("Z: {:.2}", state.stats.z_score_current),
        (x + w - 10.0) as f64,
        (y + 20.0) as f64,
    );
}

/// Render Sector Heatmap Panel
pub fn render_sector_heatmap_panel(
    ctx: &mut dyn RenderContext,
    state: &SectorHeatmapState,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    const BG_COLOR: &str = "#000000ff";
    const BORDER_COLOR: &str = "#1A1A1Aff";
    const SECTOR_TEXT: &str = "#FFFFFFff";

    // 1. Background
    ctx.set_fill_color(BG_COLOR);
    ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

    // 2. Sector rectangles (treemap layout)
    let rects = state.sector_rects(w, h);

    for (rect_x, rect_y, rect_w, rect_h, color, label) in rects {
        // 3. Fill rectangle
        let hex = rgba_to_hex(color);
        ctx.set_fill_color(&hex);
        ctx.fill_rect(
            (x + rect_x) as f64,
            (y + rect_y) as f64,
            rect_w as f64,
            rect_h as f64,
        );

        // 4. Border
        ctx.set_fill_color(BORDER_COLOR);
        ctx.fill_rect((x + rect_x) as f64, (y + rect_y) as f64, rect_w as f64, 2.0);
        ctx.fill_rect(
            (x + rect_x) as f64,
            (y + rect_y + rect_h - 2.0) as f64,
            rect_w as f64,
            2.0,
        );
        ctx.fill_rect((x + rect_x) as f64, (y + rect_y) as f64, 2.0, rect_h as f64);
        ctx.fill_rect(
            (x + rect_x + rect_w - 2.0) as f64,
            (y + rect_y) as f64,
            2.0,
            rect_h as f64,
        );

        // 5. Label
        if rect_w * rect_h > 1000.0 {
            ctx.set_fill_color(SECTOR_TEXT);
            ctx.set_font("14px sans-serif");
            ctx.set_text_align(TextAlign::Left);
            ctx.fill_text(&label, (x + rect_x + 8.0) as f64, (y + rect_y + 20.0) as f64);
        }
    }
}

/// Render Performance Analytics Panel
pub fn render_performance_analytics_panel(
    ctx: &mut dyn RenderContext,
    state: &PerformanceAnalyticsState,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    const BG_COLOR: &str = "#0D1B2Aff";
    const GRID_COLOR: &str = "#1E2A384d";
    const EQUITY_LINE: &str = "#00C853ff";
    const EQUITY_FILL: &str = "#00C85326";
    const DRAWDOWN_LINE: &str = "#FF1744ff";
    const DRAWDOWN_FILL: &str = "#FF174433";
    const ZERO_LINE: &str = "#607D8Bff";
    const METRIC_BG: &str = "#1A2332ff";
    const METRIC_TEXT: &str = "#B0BEC5ff";
    const METRIC_POSITIVE: &str = "#00C853ff";
    const METRIC_NEGATIVE: &str = "#FF1744ff";

    // Layout: Top 60% equity, Bottom 40% drawdown
    let equity_h = h * 0.6;
    let drawdown_h = h * 0.4;

    // === EQUITY CURVE PANEL ===
    let equity_y = y;

    // 1. Background
    ctx.set_fill_color(BG_COLOR);
    ctx.fill_rect(x as f64, equity_y as f64, w as f64, equity_h as f64);

    // 2. Grid lines
    ctx.set_fill_color(GRID_COLOR);
    for i in 1..5 {
        let grid_y = equity_y + (equity_h / 5.0) * i as f32;
        ctx.fill_rect(x as f64, grid_y as f64, w as f64, 1.0);
    }

    // 3. Equity area fill
    let equity_points = state.equity_points(w, equity_h);
    if !equity_points.is_empty() {
        ctx.set_fill_color(EQUITY_FILL);
        ctx.begin_path();
        ctx.move_to(x as f64, (equity_y + equity_h) as f64);
        for (px, py) in &equity_points {
            ctx.line_to((x + px) as f64, (equity_y + py) as f64);
        }
        ctx.line_to(
            (x + equity_points.last().unwrap().0) as f64,
            (equity_y + equity_h) as f64,
        );
        ctx.close_path();
        ctx.fill();
    }

    // 4. Equity line
    if !equity_points.is_empty() {
        ctx.set_stroke_color(EQUITY_LINE);
        ctx.set_stroke_width(2.0);
        ctx.begin_path();
        ctx.move_to(
            (x + equity_points[0].0) as f64,
            (equity_y + equity_points[0].1) as f64,
        );
        for (px, py) in &equity_points[1..] {
            ctx.line_to((x + px) as f64, (equity_y + py) as f64);
        }
        ctx.stroke();
    }

    // === DRAWDOWN PANEL ===
    let drawdown_y = equity_y + equity_h;

    // 5. Background
    ctx.set_fill_color(BG_COLOR);
    ctx.fill_rect(x as f64, drawdown_y as f64, w as f64, drawdown_h as f64);

    // 6. Zero line
    ctx.set_fill_color(ZERO_LINE);
    ctx.fill_rect(x as f64, drawdown_y as f64, w as f64, 2.0);

    // 7. Drawdown area fill (inverted)
    let drawdown_points = state.drawdown_points(w, drawdown_h);
    if !drawdown_points.is_empty() {
        ctx.set_fill_color(DRAWDOWN_FILL);
        ctx.begin_path();
        ctx.move_to(x as f64, drawdown_y as f64);
        for (px, py) in &drawdown_points {
            ctx.line_to((x + px) as f64, (drawdown_y + py) as f64);
        }
        ctx.line_to(
            (x + drawdown_points.last().unwrap().0) as f64,
            drawdown_y as f64,
        );
        ctx.close_path();
        ctx.fill();
    }

    // 8. Drawdown line
    if !drawdown_points.is_empty() {
        ctx.set_stroke_color(DRAWDOWN_LINE);
        ctx.set_stroke_width(2.0);
        ctx.begin_path();
        ctx.move_to(
            (x + drawdown_points[0].0) as f64,
            (drawdown_y + drawdown_points[0].1) as f64,
        );
        for (px, py) in &drawdown_points[1..] {
            ctx.line_to((x + px) as f64, (drawdown_y + py) as f64);
        }
        ctx.stroke();
    }

    // 9. Metrics panel (overlay on right side)
    let metrics_x = x + w - 200.0;
    let metrics_y = y + 10.0;
    let metrics_w = 190.0;
    let metrics_h = 150.0;

    ctx.set_fill_color(METRIC_BG);
    ctx.fill_rounded_rect(
        metrics_x as f64,
        metrics_y as f64,
        metrics_w as f64,
        metrics_h as f64,
        4.0,
    );

    // 10. Metrics text
    ctx.set_font("11px sans-serif");
    ctx.set_text_align(TextAlign::Left);

    let metrics_list = state.metrics_list();
    for (i, (label, value)) in metrics_list.iter().enumerate() {
        let metric_y = metrics_y + 20.0 + i as f32 * 18.0;

        ctx.set_fill_color(METRIC_TEXT);
        ctx.fill_text(label, (metrics_x + 10.0) as f64, metric_y as f64);

        let value_color = if value.starts_with('-') {
            METRIC_NEGATIVE
        } else {
            METRIC_POSITIVE
        };
        ctx.set_fill_color(value_color);
        ctx.set_text_align(TextAlign::Right);
        ctx.fill_text(&value, (metrics_x + metrics_w - 10.0) as f64, metric_y as f64);
        ctx.set_text_align(TextAlign::Left);
    }
}

/// Render Payoff Diagram Panel
pub fn render_payoff_diagram_panel(
    ctx: &mut dyn RenderContext,
    state: &PayoffDiagramState,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    const BG_COLOR: &str = "#0D1B2Aff";
    const GRID_COLOR: &str = "#1E2A384d";
    const ZERO_LINE: &str = "#607D8Bff";
    const PROFIT_FILL: &str = "#00C85326";
    const LOSS_FILL: &str = "#FF174426";
    const PAYOFF_LINE: &str = "#FFC107ff";
    const BREAKEVEN_LINE: &str = "#FFEB3Bff";
    const CURRENT_PRICE: &str = "#2196F3ff";
    const AXIS_TEXT: &str = "#B0BEC5ff";

    // 1. Background
    ctx.set_fill_color(BG_COLOR);
    ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

    // 2. Horizontal grid lines
    ctx.set_fill_color(GRID_COLOR);
    for i in 1..5 {
        let grid_y = y + (h / 5.0) * i as f32;
        ctx.fill_rect(x as f64, grid_y as f64, w as f64, 1.0);
    }

    // 4. Zero P&L line
    let zero_y = y + h / 2.0;
    ctx.set_fill_color(ZERO_LINE);
    ctx.fill_rect(x as f64, zero_y as f64, w as f64, 2.0);

    // 5. Profit zone fill
    let payoff_points = state.payoff_points(w, h);
    if !payoff_points.is_empty() {
        ctx.set_fill_color(PROFIT_FILL);
        ctx.begin_path();
        ctx.move_to((x + payoff_points[0].0) as f64, zero_y as f64);
        for (px, py) in &payoff_points {
            if *py < zero_y {
                ctx.line_to((x + px) as f64, (y + py) as f64);
            } else {
                ctx.line_to((x + px) as f64, zero_y as f64);
            }
        }
        ctx.line_to(
            (x + payoff_points.last().unwrap().0) as f64,
            zero_y as f64,
        );
        ctx.close_path();
        ctx.fill();
    }

    // 6. Loss zone fill
    if !payoff_points.is_empty() {
        ctx.set_fill_color(LOSS_FILL);
        ctx.begin_path();
        ctx.move_to((x + payoff_points[0].0) as f64, zero_y as f64);
        for (px, py) in &payoff_points {
            if *py > zero_y {
                ctx.line_to((x + px) as f64, (y + py) as f64);
            } else {
                ctx.line_to((x + px) as f64, zero_y as f64);
            }
        }
        ctx.line_to(
            (x + payoff_points.last().unwrap().0) as f64,
            zero_y as f64,
        );
        ctx.close_path();
        ctx.fill();
    }

    // 7. Payoff line
    if !payoff_points.is_empty() {
        ctx.set_stroke_color(PAYOFF_LINE);
        ctx.set_stroke_width(3.0);
        ctx.begin_path();
        ctx.move_to(
            (x + payoff_points[0].0) as f64,
            (y + payoff_points[0].1) as f64,
        );
        for (px, py) in &payoff_points[1..] {
            ctx.line_to((x + px) as f64, (y + py) as f64);
        }
        ctx.stroke();
    }

    // 8. Breakeven markers
    let breakeven_x_coords = state.breakeven_x(w);
    ctx.set_fill_color(BREAKEVEN_LINE);
    for be_x in breakeven_x_coords {
        ctx.fill_rect((x + be_x) as f64, y as f64, 2.0, h as f64);
    }

    // 9. Current price marker
    if !state.payoff_curve.is_empty() {
        let current_x = x
            + w * ((state.current_spot - state.payoff_curve[0].0)
                / (state.payoff_curve.last().unwrap().0 - state.payoff_curve[0].0)) as f32;
        ctx.set_fill_color(CURRENT_PRICE);
        ctx.fill_rect(current_x as f64, y as f64, 2.0, h as f64);
    }

    // 10. Axis labels
    ctx.set_fill_color(AXIS_TEXT);
    ctx.set_font("11px sans-serif");
    ctx.set_text_align(TextAlign::Center);
    ctx.fill_text(
        &format!("Max Profit: {}", state.max_profit()),
        (x + w / 2.0) as f64,
        (y + h + 15.0) as f64,
    );

    ctx.set_text_align(TextAlign::Left);
    ctx.fill_text(
        &format!("Max Loss: {}", state.max_loss()),
        (x + 10.0) as f64,
        (y + h + 15.0) as f64,
    );
}

/// Render IV Surface Panel (same as Volatility Surface)
pub fn render_iv_surface_panel(
    ctx: &mut dyn RenderContext,
    state: &IvSurfaceState,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    let x = x as f64;
    let y = y as f64;
    let w = w as f64;
    let h = h as f64;

    // Background
    ctx.set_fill_color(&rgba_to_hex([0.08, 0.08, 0.12, 0.95]));
    ctx.fill_rect(x, y, w, h);

    // Title
    ctx.set_font("13px sans-serif");
    ctx.set_fill_color(&rgba_to_hex([0.7, 0.7, 0.8, 1.0]));
    ctx.fill_text("IV Surface", x + 10.0, y + 20.0);

    // Render surface data as grid cells
    let num_strikes = state.strikes.len().max(1);
    let num_expiries = state.expiries.len().max(1);
    let cell_w = (w - 40.0) / num_strikes as f64;
    let cell_h = (h - 60.0) / num_expiries as f64;
    for (ei, row) in state.surface_data.iter().enumerate() {
        for (si, &iv) in row.iter().enumerate() {
            let px = x + 20.0 + si as f64 * cell_w;
            let py = y + 30.0 + ei as f64 * cell_h;
            let norm = if state.color_range.1 > state.color_range.0 {
                ((iv - state.color_range.0) / (state.color_range.1 - state.color_range.0)).clamp(0.0, 1.0)
            } else { 0.5 };
            let color = [norm as f32 * 0.2, 0.3 + norm as f32 * 0.5, 1.0 - norm as f32 * 0.6, 0.9];
            ctx.set_fill_color(&rgba_to_hex(color));
            ctx.fill_rect(px, py, cell_w.max(1.0), cell_h.max(1.0));
        }
    }

    // Border
    ctx.set_fill_color(&rgba_to_hex([0.2, 0.2, 0.3, 1.0]));
    ctx.fill_rect(x, y, w, 1.0);
    ctx.fill_rect(x, y + h - 1.0, w, 1.0);
    ctx.fill_rect(x, y, 1.0, h);
    ctx.fill_rect(x + w - 1.0, y, 1.0, h);
}

// ========== REALTIME PANELS ==========

/// Render GPU Timeseries Panel (High-Performance Line Chart)
pub fn render_gpu_timeseries_panel(
    ctx: &mut dyn RenderContext,
    state: &GpuTimeseriesState,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    const BG_COLOR: &str = "#0A0E12ff";
    const GRID_COLOR: &str = "#1A1F2633";
    const DATA_LINE: &str = "#00E676ff";
    const AXIS_TEXT: &str = "#90A4AEff";

    // 1. Background
    ctx.set_fill_color(BG_COLOR);
    ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

    // 2. Minimal horizontal grid lines (3-5 max for performance)
    ctx.set_fill_color(GRID_COLOR);
    for i in 1..4 {
        let grid_y = y + (h / 4.0) * i as f32;
        ctx.fill_rect(x as f64, grid_y as f64, w as f64, 1.0);
    }

    let data_points = state.visible_points(w, h);
    if data_points.is_empty() {
        return;
    }

    // 3. Data line (optimized rendering with single path)
    ctx.set_stroke_color(DATA_LINE);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();

    let first_x = x + data_points[0].0;
    let first_y = y + data_points[0].1;
    ctx.move_to(first_x as f64, first_y as f64);

    for (px, py) in &data_points[1..] {
        ctx.line_to((x + px) as f64, (y + py) as f64);
    }

    ctx.stroke();

    // 4. Axis labels
    let (min_value, max_value) = state.value_range;
    ctx.set_fill_color(AXIS_TEXT);
    ctx.set_font("10px sans-serif");
    ctx.set_text_align(TextAlign::Right);
    ctx.fill_text(
        &state.format_value(max_value),
        (x + w - 5.0) as f64,
        (y + 12.0) as f64,
    );
    ctx.fill_text(
        &state.format_value(min_value),
        (x + w - 5.0) as f64,
        (y + h - 5.0) as f64,
    );
}

/// Render Streaming Heatmap Panel
pub fn render_streaming_heatmap_panel(
    ctx: &mut dyn RenderContext,
    state: &StreamingHeatmapState,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    const BG_COLOR: &str = "#0A0E12ff";
    const CATEGORY_DIVIDER: &str = "#1A1F26ff";
    const NOW_LINE: &str = "#FFC107ff";
    const AXIS_TEXT: &str = "#90A4AEff";

    // 1. Background
    ctx.set_fill_color(BG_COLOR);
    ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

    let grid = &state.grid;

    if grid.is_empty() {
        return;
    }

    let time_buckets = grid.len();
    let row_count = grid.front().map_or(0, |row| row.len());

    if row_count == 0 {
        return;
    }

    let cell_w = w / time_buckets as f32;
    let cell_h = h / row_count as f32;

    // 2. Heatmap cells
    let visible_cells = state.visible_cells();
    for (col_idx, row_idx, color) in visible_cells {
        let cell_x = x + col_idx as f32 * cell_w;
        let cell_y = y + row_idx as f32 * cell_h;

        if color[3] > 0.0 {
            let hex = rgba_to_hex(color);
            ctx.set_fill_color(&hex);
            ctx.fill_rect(cell_x as f64, cell_y as f64, cell_w as f64, cell_h as f64);
        }
    }

    // 3. Row dividers
    ctx.set_fill_color(CATEGORY_DIVIDER);
    for i in 1..row_count {
        let divider_y = y + i as f32 * cell_h;
        ctx.fill_rect(x as f64, divider_y as f64, w as f64, 1.0);
    }

    // 4. "Now" line (right edge)
    ctx.set_fill_color(NOW_LINE);
    ctx.fill_rect((x + w - 2.0) as f64, y as f64, 2.0, h as f64);

    // 5. Fade gradient on left edge (old data)
    ctx.set_global_alpha(0.5);
    ctx.set_fill_color(BG_COLOR);
    ctx.fill_rect(x as f64, y as f64, (w * 0.1) as f64, h as f64);
    ctx.set_global_alpha(1.0);

    // 6. Row labels (use time labels as row identifiers)
    ctx.set_fill_color(AXIS_TEXT);
    ctx.set_font("10px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    let time_labels = state.time_labels();
    for (i, label) in time_labels.iter().enumerate().take(row_count) {
        let label_y = y + i as f32 * cell_h + cell_h / 2.0;
        ctx.fill_text(label, (x + 5.0) as f64, label_y as f64);
    }
}

/// Render Pairs Trading Panel
pub fn render_pairs_trading_panel(
    ctx: &mut dyn RenderContext,
    state: &PairsTradingState,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
) {
    const BG_COLOR: &str = "#0D1B2Aff";
    const GRID_COLOR: &str = "#1E2A384d";
    const SPREAD_LINE: &str = "#FF9800ff";
    const THRESHOLD_PLUS2: &str = "#FF1744ff";
    const THRESHOLD_PLUS1: &str = "#FFC107ff";
    const THRESHOLD_MINUS1: &str = "#FFC107ff";
    const THRESHOLD_MINUS2: &str = "#FF1744ff";
    const ZERO_LINE: &str = "#607D8Bff";
    const EXTREME_ZONE: &str = "#FF17440d";
    const CAUTION_ZONE: &str = "#FFC1070d";
    const ZSCORE_LINE_ST: &str = "#FF9800ff";
    const AXIS_TEXT: &str = "#B0BEC5ff";

    // Layout: Top 60% spread, Bottom 40% z-score
    let spread_h = h * 0.6;
    let zscore_h = h * 0.4;

    // === SPREAD PANEL ===
    let spread_y = y;

    // 1. Background
    ctx.set_fill_color(BG_COLOR);
    ctx.fill_rect(x as f64, spread_y as f64, w as f64, spread_h as f64);

    // 2. Grid lines
    ctx.set_fill_color(GRID_COLOR);
    for i in 1..5 {
        let grid_y = spread_y + (spread_h / 5.0) * i as f32;
        ctx.fill_rect(x as f64, grid_y as f64, w as f64, 1.0);
    }

    // 3. Spread line
    if let Some(spread_history) = state.selected_spread_history() {
        if !spread_history.is_empty() {
            let (min_time, max_time) = spread_history
                .iter()
                .fold((i64::MAX, i64::MIN), |(min, max), (t, _)| {
                    (min.min(*t), max.max(*t))
                });

            let (min_spread, max_spread) = spread_history
                .iter()
                .fold((f64::MAX, f64::MIN), |(min, max), (_, s)| {
                    (min.min(*s), max.max(*s))
                });

            let time_range = (max_time - min_time) as f64;
            let spread_range = max_spread - min_spread;

            if time_range > 0.0 && spread_range > 0.0 {
                ctx.set_stroke_color(SPREAD_LINE);
                ctx.set_stroke_width(2.0);
                ctx.begin_path();

                let first_x =
                    x + (((spread_history[0].0 - min_time) as f64 / time_range) * w as f64) as f32;
                let first_y = spread_y + spread_h
                    - (((spread_history[0].1 - min_spread) / spread_range) * spread_h as f64)
                        as f32;
                ctx.move_to(first_x as f64, first_y as f64);

                for (timestamp, spread_val) in spread_history.iter().skip(1) {
                    let px = x + (((*timestamp - min_time) as f64 / time_range) * w as f64) as f32;
                    let py = spread_y + spread_h
                        - (((spread_val - min_spread) / spread_range) * spread_h as f64) as f32;
                    ctx.line_to(px as f64, py as f64);
                }

                ctx.stroke();
            }
        }
    }

    // === Z-SCORE PANEL ===
    let zscore_y = spread_y + spread_h;

    // 4. Background
    ctx.set_fill_color(BG_COLOR);
    ctx.fill_rect(x as f64, zscore_y as f64, w as f64, zscore_h as f64);

    // 5. Threshold zone fills
    // Extreme zone (±2)
    ctx.set_fill_color(EXTREME_ZONE);
    ctx.fill_rect(x as f64, zscore_y as f64, w as f64, (zscore_h * 0.2) as f64);
    ctx.fill_rect(
        x as f64,
        (zscore_y + zscore_h * 0.8) as f64,
        w as f64,
        (zscore_h * 0.2) as f64,
    );

    // Caution zone (±1)
    ctx.set_fill_color(CAUTION_ZONE);
    ctx.fill_rect(
        x as f64,
        (zscore_y + zscore_h * 0.2) as f64,
        w as f64,
        (zscore_h * 0.1) as f64,
    );
    ctx.fill_rect(
        x as f64,
        (zscore_y + zscore_h * 0.7) as f64,
        w as f64,
        (zscore_h * 0.1) as f64,
    );

    // 6. Threshold lines
    ctx.set_fill_color(THRESHOLD_PLUS2);
    ctx.fill_rect(
        x as f64,
        (zscore_y + zscore_h * 0.2) as f64,
        w as f64,
        1.0,
    );

    ctx.set_fill_color(THRESHOLD_PLUS1);
    ctx.fill_rect(
        x as f64,
        (zscore_y + zscore_h * 0.3) as f64,
        w as f64,
        1.0,
    );

    ctx.set_fill_color(ZERO_LINE);
    ctx.fill_rect(
        x as f64,
        (zscore_y + zscore_h * 0.5) as f64,
        w as f64,
        2.0,
    );

    ctx.set_fill_color(THRESHOLD_MINUS1);
    ctx.fill_rect(
        x as f64,
        (zscore_y + zscore_h * 0.7) as f64,
        w as f64,
        1.0,
    );

    ctx.set_fill_color(THRESHOLD_MINUS2);
    ctx.fill_rect(
        x as f64,
        (zscore_y + zscore_h * 0.8) as f64,
        w as f64,
        1.0,
    );

    // 7. Z-score point
    if let Some(pair_idx) = state.selected_pair {
        if let Some(pair) = state.pairs.get(pair_idx) {
            let z_score = pair.z_score;
            let z_norm = ((z_score + 3.0) / 6.0).clamp(0.0, 1.0);
            let z_y = zscore_y + zscore_h * (1.0 - z_norm as f32);

            ctx.set_fill_color(ZSCORE_LINE_ST);
            ctx.begin_path();
            ctx.arc(
                (x + w - 10.0) as f64,
                z_y as f64,
                4.0,
                0.0,
                std::f64::consts::PI * 2.0,
            );
            ctx.fill();
        }
    }

    // 8. Axis labels
    ctx.set_fill_color(AXIS_TEXT);
    ctx.set_font("11px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.fill_text("Spread", (x + 10.0) as f64, (spread_y + 20.0) as f64);
    ctx.fill_text("Z-Score", (x + 10.0) as f64, (zscore_y + 20.0) as f64);
}
