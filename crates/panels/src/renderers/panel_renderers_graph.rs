//! Graph, Network, and Hierarchy Panel Renderers
//!
//! Rendering functions for 14 advanced visualization panel types:
//! - Network: Sankey, Chord, Alluvial, Blockchain Graph
//! - Hierarchy: Sunburst, Icicle, Circular Packing
//! - Realtime: Force Graph, Flow Field, Particle System
//! - Specialized: Flame Graph, Stream Graph, Radar Chart, Bubble Chart

use crate::render::RenderContext;
use crate::visual::network::sankey::SankeyState;
use crate::visual::network::chord::ChordState;
use crate::visual::network::alluvial::AlluvialState;
use crate::visual::network::blockchain_graph::BlockchainGraphState;
use crate::visual::hierarchy::sunburst::SunburstState;
use crate::visual::hierarchy::icicle::IcicleState;
use crate::visual::hierarchy::circular_packing::CircularPackingState;
use crate::visual::realtime::force_graph::ForceGraphState;
use crate::visual::realtime::flow_field::FlowFieldState;
use crate::visual::realtime::particle_system::ParticleSystemState;
use crate::visual::specialized::flame_graph::FlameGraphState;
use crate::visual::specialized::stream_graph::StreamGraphState;
use crate::visual::specialized::radar_chart::RadarChartState;
use crate::visual::specialized::bubble_chart::BubbleChartState;

/// Convert RGBA array [0.0-1.0] to hex color string
fn rgba_to_hex(rgba: [f32; 4]) -> String {
    let r = (rgba[0].clamp(0.0, 1.0) * 255.0) as u8;
    let g = (rgba[1].clamp(0.0, 1.0) * 255.0) as u8;
    let b = (rgba[2].clamp(0.0, 1.0) * 255.0) as u8;
    let a = (rgba[3].clamp(0.0, 1.0) * 255.0) as u8;
    format!("#{:02x}{:02x}{:02x}{:02x}", r, g, b, a)
}

/// Approximate cubic bezier curve with line segments
fn cubic_bezier_approx(
    ctx: &mut dyn RenderContext,
    x0: f32, y0: f32,      // Start point
    cx1: f32, cy1: f32,    // Control point 1
    cx2: f32, cy2: f32,    // Control point 2
    x1: f32, y1: f32,      // End point
    segments: usize        // Number of line segments (20 recommended)
) {
    for i in 0..=segments {
        let t = i as f32 / segments as f32;
        let t2 = t * t;
        let t3 = t2 * t;
        let mt = 1.0 - t;
        let mt2 = mt * mt;
        let mt3 = mt2 * mt;

        let x = mt3 * x0 + 3.0 * mt2 * t * cx1 + 3.0 * mt * t2 * cx2 + t3 * x1;
        let y = mt3 * y0 + 3.0 * mt2 * t * cy1 + 3.0 * mt * t2 * cy2 + t3 * y1;

        if i == 0 {
            ctx.move_to(x as f64, y as f64);
        } else {
            ctx.line_to(x as f64, y as f64);
        }
    }
}

// ============================================================================
// NETWORK PANELS
// ============================================================================

/// Render Sankey diagram with nodes and flow paths
pub fn render_sankey_panel(
    ctx: &mut dyn RenderContext,
    state: &SankeyState,
    w: f32,
    h: f32,
) {
    // Background
    ctx.set_fill_color("#1a1a1a");
    ctx.fill_rect(0.0, 0.0, w as f64, h as f64);

    // LAYER 1: Draw links (curved flows)
    for flow in state.flow_paths(w, h) {
        let (from_x, from_y) = flow.from;
        let (to_x, to_y) = flow.to;
        let width = flow.width;

        // Calculate control points for horizontal bezier
        let mid_x = (from_x + to_x) / 2.0;
        let cx1 = mid_x;
        let cy1 = from_y;
        let cx2 = mid_x;
        let cy2 = to_y;

        // Draw filled bezier path (top edge + bottom edge)
        ctx.begin_path();
        cubic_bezier_approx(ctx, from_x, from_y - width/2.0, cx1, cy1 - width/2.0,
                            cx2, cy2 - width/2.0, to_x, to_y - width/2.0, 20);
        ctx.line_to(to_x as f64, (to_y + width/2.0) as f64);
        cubic_bezier_approx(ctx, to_x, to_y + width/2.0, cx2, cy2 + width/2.0,
                            cx1, cy1 + width/2.0, from_x, from_y + width/2.0, 20);
        ctx.close_path();

        // Fill with semi-transparent color
        let mut fill_color = flow.color;
        fill_color[3] = 0.3; // Opacity 0.3
        ctx.set_fill_color(&rgba_to_hex(fill_color));
        ctx.fill();
    }

    // LAYER 2: Draw nodes (rectangles)
    for (x, y, width, height, color, label) in state.node_rects(w, h) {
        ctx.set_fill_color(&rgba_to_hex(color));
        ctx.fill_rect(x as f64, y as f64, width as f64, height as f64);

        // Node label
        if width > 30.0 && height > 10.0 {
            ctx.set_fill_color("#e0e0e0");
            ctx.set_font("10px sans-serif");
            ctx.fill_text(label, (x + 4.0) as f64, (y + height / 2.0 + 4.0) as f64);
        }
    }
}

/// Render Chord diagram with arcs and ribbons
pub fn render_chord_panel(
    ctx: &mut dyn RenderContext,
    state: &ChordState,
    w: f32,
    h: f32,
) {
    let cx = w / 2.0;
    let cy = h / 2.0;
    let radius = w.min(h) / 2.0 - 40.0;
    let arc_width = 20.0;

    // Background
    ctx.set_fill_color("#0d1117");
    ctx.fill_rect(0.0, 0.0, w as f64, h as f64);

    // LAYER 1: Draw ribbons (bezier paths connecting arcs)
    for ribbon in &state.ribbons {
        if ribbon.source_index >= state.groups.len() || ribbon.target_index >= state.groups.len() {
            continue;
        }
        let source_arc = &state.groups[ribbon.source_index];
        let inner_r = radius - arc_width;

        // Source arc points
        let sx0 = cx + inner_r * (ribbon.source_start_angle as f32).cos();
        let sy0 = cy + inner_r * (ribbon.source_start_angle as f32).sin();
        let sx1 = cx + inner_r * (ribbon.source_end_angle as f32).cos();
        let sy1 = cy + inner_r * (ribbon.source_end_angle as f32).sin();

        // Target arc points
        let tx0 = cx + inner_r * (ribbon.target_start_angle as f32).cos();
        let ty0 = cy + inner_r * (ribbon.target_start_angle as f32).sin();
        let tx1 = cx + inner_r * (ribbon.target_end_angle as f32).cos();
        let ty1 = cy + inner_r * (ribbon.target_end_angle as f32).sin();

        // Draw ribbon using bezier approximation
        ctx.begin_path();
        ctx.move_to(sx0 as f64, sy0 as f64);
        ctx.line_to(sx1 as f64, sy1 as f64);
        cubic_bezier_approx(ctx, sx1, sy1, cx, cy, cx, cy, tx0, ty0, 20);
        ctx.line_to(tx1 as f64, ty1 as f64);
        cubic_bezier_approx(ctx, tx1, ty1, cx, cy, cx, cy, sx0, sy0, 20);
        ctx.close_path();

        // Fill with source color at 0.3 opacity
        let ribbon_color = [
            source_arc.color[0] as f32 / 255.0,
            source_arc.color[1] as f32 / 255.0,
            source_arc.color[2] as f32 / 255.0,
            0.3,
        ];
        ctx.set_fill_color(&rgba_to_hex(ribbon_color));
        ctx.fill();
    }

    // LAYER 2: Draw arc segments
    for (start_angle, end_angle, color, label) in state.arcs(cx, cy, radius) {
        ctx.begin_path();
        ctx.arc(cx as f64, cy as f64, radius as f64, start_angle, end_angle);
        ctx.arc(cx as f64, cy as f64, (radius - arc_width) as f64, end_angle, start_angle);
        ctx.close_path();
        ctx.set_fill_color(&rgba_to_hex(color));
        ctx.fill();

        // Label outside arc
        let mid_angle = ((start_angle + end_angle) / 2.0) as f32;
        let label_r = radius + 15.0;
        let lx = cx + label_r * mid_angle.cos();
        let ly = cy + label_r * mid_angle.sin();
        ctx.set_fill_color("#c9d1d9");
        ctx.set_font("10px sans-serif");
        ctx.fill_text(label, lx as f64, ly as f64);
    }
}

/// Render Alluvial diagram with blocks and flow curves
pub fn render_alluvial_panel(
    ctx: &mut dyn RenderContext,
    state: &AlluvialState,
    w: f32,
    h: f32,
) {
    // Background
    ctx.set_fill_color("#121212");
    ctx.fill_rect(0.0, 0.0, w as f64, h as f64);

    // LAYER 1: Draw flow paths (smooth curves)
    for ((from_x, from_y), (to_x, to_y), width, color) in state.flow_curves(w, h) {
        // Control points for smooth curve
        let cx1 = from_x + (to_x - from_x) * 0.5;
        let cy1 = from_y;
        let cx2 = from_x + (to_x - from_x) * 0.5;
        let cy2 = to_y;

        ctx.begin_path();
        // Top edge
        cubic_bezier_approx(ctx, from_x, from_y - width/2.0, cx1, cy1 - width/2.0,
                            cx2, cy2 - width/2.0, to_x, to_y - width/2.0, 20);
        ctx.line_to(to_x as f64, (to_y + width/2.0) as f64);
        // Bottom edge
        cubic_bezier_approx(ctx, to_x, to_y + width/2.0, cx2, cy2 + width/2.0,
                            cx1, cy1 + width/2.0, from_x, from_y + width/2.0, 20);
        ctx.close_path();

        let mut flow_color = color;
        flow_color[3] = 0.5; // Opacity
        ctx.set_fill_color(&rgba_to_hex(flow_color));
        ctx.fill();
    }

    // LAYER 2: Draw column blocks (stacked rectangles)
    for (x, y, width, height, color, label) in state.column_blocks(w, h) {
        ctx.set_fill_color(&rgba_to_hex(color));
        ctx.fill_rect(x as f64, y as f64, width as f64, height as f64);

        // Block label
        if height > 15.0 && width > 40.0 {
            ctx.set_fill_color("#e0e0e0");
            ctx.set_font("10px sans-serif");
            ctx.fill_text(label, (x + 4.0) as f64, (y + height / 2.0 + 4.0) as f64);
        }
    }

    // LAYER 3: Column labels
    for (i, column_name) in state.columns.iter().enumerate() {
        let x = state.column_x(i, w);
        ctx.set_fill_color("#e0e0e0");
        ctx.set_font("12px sans-serif");
        ctx.fill_text(column_name, (x + 20.0) as f64, (h - 10.0) as f64);
    }
}

/// Render Blockchain graph with nodes and edges
pub fn render_blockchain_graph_panel(
    ctx: &mut dyn RenderContext,
    state: &BlockchainGraphState,
    w: f32,
    h: f32,
) {
    // Background
    ctx.set_fill_color("#0a0e14");
    ctx.fill_rect(0.0, 0.0, w as f64, h as f64);

    // LAYER 1: Draw edges (straight lines)
    for ((from_x, from_y), (to_x, to_y), width, color) in state.edge_lines() {
        ctx.begin_path();
        ctx.move_to(from_x as f64, from_y as f64);
        ctx.line_to(to_x as f64, to_y as f64);
        ctx.set_stroke_color(&rgba_to_hex(color));
        ctx.set_stroke_width(width as f64);
        ctx.stroke();
    }

    // LAYER 2: Draw nodes (circles)
    for (x, y, radius, color, label) in state.node_positions() {
        // Circle
        ctx.begin_path();
        ctx.arc(x as f64, y as f64, radius as f64, 0.0, std::f64::consts::TAU);
        ctx.set_fill_color(&rgba_to_hex(color));
        ctx.fill();

        // Border
        ctx.set_stroke_color("#ffffff");
        ctx.set_stroke_width(1.5);
        ctx.stroke();

        // Label (shortened address)
        if radius > 8.0 {
            let short_label = state.format_address(label);
            ctx.set_fill_color("#9ca3af");
            ctx.set_font("9px sans-serif");
            ctx.fill_text(&short_label, x as f64, (y + radius + 12.0) as f64);
        }
    }
}

// ============================================================================
// HIERARCHY PANELS
// ============================================================================

/// Render Sunburst diagram with radial arcs
pub fn render_sunburst_panel(
    ctx: &mut dyn RenderContext,
    state: &SunburstState,
    w: f32,
    h: f32,
) {
    let cx = w / 2.0;
    let cy = h / 2.0;
    let radius = w.min(h) / 2.0 - 20.0;

    // Background
    ctx.set_fill_color("#1e1e1e");
    ctx.fill_rect(0.0, 0.0, w as f64, h as f64);

    // Draw arcs from innermost to outermost
    for arc in state.visible_arcs() {
        let inner_r = arc.inner_r * radius;
        let outer_r = arc.outer_r * radius;
        let start_angle = arc.start_angle as f32;
        let end_angle = arc.end_angle as f32;

        ctx.begin_path();
        ctx.arc(cx as f64, cy as f64, outer_r as f64, start_angle as f64, end_angle as f64);
        ctx.arc(cx as f64, cy as f64, inner_r as f64, end_angle as f64, start_angle as f64); // Reverse for inner
        ctx.close_path();
        ctx.set_fill_color(&rgba_to_hex(arc.color));
        ctx.fill();

        // Label along arc (if arc is large enough)
        let angle_span = end_angle - start_angle;
        if angle_span > 0.1 && (outer_r - inner_r) > 15.0 {
            let mid_angle = (start_angle + end_angle) / 2.0;
            let mid_r = (inner_r + outer_r) / 2.0;
            let lx = cx + mid_r * mid_angle.cos();
            let ly = cy + mid_r * mid_angle.sin();

            ctx.save();
            ctx.translate(lx as f64, ly as f64);
            ctx.rotate((mid_angle + std::f32::consts::FRAC_PI_2) as f64);
            ctx.set_fill_color("#ffffff");
            ctx.set_font("10px sans-serif");
            ctx.fill_text(&arc.label, 0.0, 0.0);
            ctx.restore();
        }
    }
}

/// Render Icicle chart with rectangular partitions
pub fn render_icicle_panel(
    ctx: &mut dyn RenderContext,
    state: &IcicleState,
    w: f32,
    h: f32,
) {
    // Background
    ctx.set_fill_color("#1a1a1a");
    ctx.fill_rect(0.0, 0.0, w as f64, h as f64);

    // Draw rectangles (left to right, top to bottom)
    for rect in state.visible_rects() {
        let x = rect.x * w;
        let y = rect.y * h;
        let width = rect.w * w;
        let height = rect.h * h;

        ctx.set_fill_color(&rgba_to_hex(rect.color));
        ctx.fill_rect(x as f64, y as f64, width as f64, height as f64);

        // Label (if fits)
        if width > 30.0 && height > 16.0 {
            let label = state.format_label(&rect.label, width);
            if !label.is_empty() {
                ctx.set_fill_color("#ffffff");
                ctx.set_font("10px sans-serif");
                ctx.fill_text(&label, (x + 4.0) as f64, (y + 13.0) as f64);
            }
        }
    }

    // Borders (1px gaps)
    ctx.set_stroke_color("#1a1a1a");
    ctx.set_stroke_width(1.0);
    for rect in state.visible_rects() {
        let x = rect.x * w;
        let y = rect.y * h;
        let width = rect.w * w;
        let height = rect.h * h;
        ctx.begin_path();
        ctx.move_to(x as f64, y as f64);
        ctx.line_to((x + width) as f64, y as f64);
        ctx.line_to((x + width) as f64, (y + height) as f64);
        ctx.line_to(x as f64, (y + height) as f64);
        ctx.close_path();
        ctx.stroke();
    }
}

/// Render Circular packing with nested circles
pub fn render_circular_packing_panel(
    ctx: &mut dyn RenderContext,
    state: &CircularPackingState,
    w: f32,
    h: f32,
) {
    // Background
    ctx.set_fill_color("#0d1117");
    ctx.fill_rect(0.0, 0.0, w as f64, h as f64);

    // Draw circles from largest to smallest (depth-first)
    let mut sorted_circles = state.circles.clone();
    sorted_circles.sort_by(|a, b| b.r.partial_cmp(&a.r).unwrap_or(std::cmp::Ordering::Equal));

    for circle in &sorted_circles {
        let color = [
            circle.color[0] as f32 / 255.0,
            circle.color[1] as f32 / 255.0,
            circle.color[2] as f32 / 255.0,
            if circle.depth == 0 { 0.3 } else { 0.7 }, // Parent opacity 0.3
        ];

        // Fill circle
        ctx.begin_path();
        ctx.arc(circle.x as f64, circle.y as f64, circle.r as f64, 0.0, std::f64::consts::TAU);
        ctx.set_fill_color(&rgba_to_hex(color));
        ctx.fill();

        // Stroke border
        ctx.set_stroke_color("#c9d1d9");
        ctx.set_stroke_width(1.5);
        ctx.stroke();

        // Label (if large enough)
        if circle.r > 15.0 {
            let label = state.format_label(&circle.label, circle.r);
            if !label.is_empty() {
                ctx.set_fill_color("#ffffff");
                ctx.set_font("10px sans-serif");
                ctx.fill_text(&label, circle.x as f64, (circle.y + 4.0) as f64);
            }
        }
    }
}

// ============================================================================
// REALTIME PANELS
// ============================================================================

/// Render Force graph with physics simulation
pub fn render_force_graph_panel(
    ctx: &mut dyn RenderContext,
    state: &ForceGraphState,
    w: f32,
    h: f32,
) {
    // Background
    ctx.set_fill_color("#1a1a1a");
    ctx.fill_rect(0.0, 0.0, w as f64, h as f64);

    // LAYER 1: Draw edges
    for ((from_x, from_y), (to_x, to_y), color) in state.edge_lines() {
        ctx.begin_path();
        ctx.move_to(from_x as f64, from_y as f64);
        ctx.line_to(to_x as f64, to_y as f64);
        let mut edge_color = color;
        edge_color[3] = 0.6; // Opacity
        ctx.set_stroke_color(&rgba_to_hex(edge_color));
        ctx.set_stroke_width(1.5);
        ctx.stroke();
    }

    // LAYER 2: Draw nodes
    for (x, y, radius, color) in state.node_circles() {
        ctx.begin_path();
        ctx.arc(x as f64, y as f64, radius as f64, 0.0, std::f64::consts::TAU);
        ctx.set_fill_color(&rgba_to_hex(color));
        ctx.fill();

        // Stroke border
        ctx.set_stroke_color("#ffffff");
        ctx.set_stroke_width(1.5);
        ctx.stroke();
    }
}

/// Render Flow field with particle trails
pub fn render_flow_field_panel(
    ctx: &mut dyn RenderContext,
    state: &FlowFieldState,
    w: f32,
    h: f32,
) {
    // Trail fade (partial clear)
    ctx.set_fill_color("rgba(0, 0, 0, 0.05)");
    ctx.fill_rect(0.0, 0.0, w as f64, h as f64);

    // Draw particles as small dots
    for (x, y, speed, _color) in state.particle_positions() {
        // Color by speed (blue -> purple -> red)
        let speed_norm = speed.min(5.0) / 5.0;
        let particle_color = if speed_norm < 0.3 {
            [0.145, 0.388, 0.922, 1.0] // Blue
        } else if speed_norm < 0.7 {
            [0.545, 0.361, 0.965, 1.0] // Purple
        } else {
            [0.863, 0.149, 0.149, 1.0] // Red
        };

        ctx.begin_path();
        ctx.arc(x as f64, y as f64, 2.0, 0.0, std::f64::consts::TAU);
        ctx.set_fill_color(&rgba_to_hex(particle_color));
        ctx.fill();
    }
}

/// Render Particle system with emitters
pub fn render_particle_system_panel(
    ctx: &mut dyn RenderContext,
    state: &ParticleSystemState,
    w: f32,
    h: f32,
) {
    // Background
    ctx.set_fill_color("#000000");
    ctx.fill_rect(0.0, 0.0, w as f64, h as f64);

    // Draw active particles as dots
    for (x, y, size, color) in state.active_particles() {
        ctx.begin_path();
        ctx.arc(x as f64, y as f64, size as f64, 0.0, std::f64::consts::TAU);
        ctx.set_fill_color(&rgba_to_hex(color));
        ctx.fill();
    }
}

// ============================================================================
// SPECIALIZED PANELS
// ============================================================================

/// Render Flame graph with stacked frames
pub fn render_flame_graph_panel(
    ctx: &mut dyn RenderContext,
    state: &FlameGraphState,
    w: f32,
    h: f32,
) {
    // Background
    ctx.set_fill_color("#2a2a2a");
    ctx.fill_rect(0.0, 0.0, w as f64, h as f64);

    // Draw frames (bottom to top)
    for (x, y, width, height, color, label) in state.frame_rects(w, h) {
        // Rounded rectangle
        ctx.set_fill_color(&rgba_to_hex(color));
        ctx.fill_rounded_rect(x as f64, y as f64, width as f64, height as f64, 2.0);

        // Label (if fits)
        if width > 30.0 {
            let max_chars = (width / 7.0) as usize;
            let display_label = if label.len() > max_chars && max_chars > 2 {
                format!("{}...", &label[..max_chars - 2])
            } else if label.len() <= max_chars {
                label.to_string()
            } else {
                String::new()
            };

            if !display_label.is_empty() {
                ctx.set_fill_color("#000000");
                ctx.set_font("12px Verdana");
                ctx.fill_text(&display_label, (x + 3.0) as f64, (y + height / 2.0 + 4.0) as f64);
            }
        }
    }
}

/// Render Stream graph with flowing layers
pub fn render_stream_graph_panel(
    ctx: &mut dyn RenderContext,
    state: &StreamGraphState,
    w: f32,
    h: f32,
) {
    // Background
    ctx.set_fill_color("#1a1a1a");
    ctx.fill_rect(0.0, 0.0, w as f64, h as f64);

    let center_y = h / 2.0;

    // Draw layers (bottom to top)
    for (polygon_points, color) in state.layer_paths(w, h) {
        if polygon_points.len() < 3 {
            continue;
        }

        ctx.begin_path();

        // Draw top edge with smooth curves
        for (i, &(x, y)) in polygon_points.iter().take(polygon_points.len() / 2).enumerate() {
            let screen_y = center_y + (y - center_y);
            if i == 0 {
                ctx.move_to(x as f64, screen_y as f64);
            } else {
                ctx.line_to(x as f64, screen_y as f64);
            }
        }

        // Draw bottom edge (reversed)
        for &(x, y) in polygon_points.iter().skip(polygon_points.len() / 2).rev() {
            let screen_y = center_y + (y - center_y);
            ctx.line_to(x as f64, screen_y as f64);
        }

        ctx.close_path();

        let mut layer_color = color;
        layer_color[3] = 0.85; // Opacity
        ctx.set_fill_color(&rgba_to_hex(layer_color));
        ctx.fill();
    }

    // X-axis
    ctx.begin_path();
    ctx.move_to(0.0, center_y as f64);
    ctx.line_to(w as f64, center_y as f64);
    ctx.set_stroke_color("#e0e0e0");
    ctx.set_stroke_width(1.0);
    ctx.stroke();
}

/// Render Radar chart with polygons
pub fn render_radar_chart_panel(
    ctx: &mut dyn RenderContext,
    state: &RadarChartState,
    w: f32,
    h: f32,
) {
    let cx = w / 2.0;
    let cy = h / 2.0;
    let radius = w.min(h) / 2.0 - 60.0;

    // Background
    ctx.set_fill_color("#1a1a1a");
    ctx.fill_rect(0.0, 0.0, w as f64, h as f64);

    // LAYER 1: Draw concentric grid circles
    ctx.set_stroke_color("#2a2a2a");
    ctx.set_stroke_width(0.5);
    for level in 1..=5 {
        let r = radius * (level as f32 / 5.0);
        ctx.begin_path();
        ctx.arc(cx as f64, cy as f64, r as f64, 0.0, std::f64::consts::TAU);
        ctx.stroke();
    }

    // LAYER 2: Draw radial axes
    ctx.set_stroke_color("#4b5563");
    ctx.set_stroke_width(1.0);
    for ((cx_start, cy_start), (cx_end, cy_end)) in state.axis_lines(cx, cy, radius) {
        ctx.begin_path();
        ctx.move_to(cx_start as f64, cy_start as f64);
        ctx.line_to(cx_end as f64, cy_end as f64);
        ctx.stroke();
    }

    // LAYER 3: Draw data polygons
    for (dataset_idx, dataset) in state.datasets.iter().enumerate() {
        let points = state.polygon_points(dataset_idx, cx, cy, radius);
        if points.is_empty() {
            continue;
        }

        // Fill polygon
        ctx.begin_path();
        for (i, &(x, y)) in points.iter().enumerate() {
            if i == 0 {
                ctx.move_to(x as f64, y as f64);
            } else {
                ctx.line_to(x as f64, y as f64);
            }
        }
        ctx.close_path();

        let fill_color = [
            dataset.color[0] as f32 / 255.0,
            dataset.color[1] as f32 / 255.0,
            dataset.color[2] as f32 / 255.0,
            0.25, // Opacity
        ];
        ctx.set_fill_color(&rgba_to_hex(fill_color));
        ctx.fill();

        // Stroke polygon
        ctx.begin_path();
        for (i, &(x, y)) in points.iter().enumerate() {
            if i == 0 {
                ctx.move_to(x as f64, y as f64);
            } else {
                ctx.line_to(x as f64, y as f64);
            }
        }
        ctx.close_path();

        let stroke_color = [
            dataset.color[0] as f32 / 255.0,
            dataset.color[1] as f32 / 255.0,
            dataset.color[2] as f32 / 255.0,
            1.0,
        ];
        ctx.set_stroke_color(&rgba_to_hex(stroke_color));
        ctx.set_stroke_width(2.0);
        ctx.stroke();
    }

    // LAYER 4: Axis labels
    for (x, y, label) in state.axis_labels(cx, cy, radius) {
        ctx.set_fill_color("#e0e0e0");
        ctx.set_font("11px sans-serif");
        ctx.fill_text(label, x as f64, y as f64);
    }
}

/// Render Bubble chart with scaled circles
pub fn render_bubble_chart_panel(
    ctx: &mut dyn RenderContext,
    state: &BubbleChartState,
    w: f32,
    h: f32,
) {
    // Background
    ctx.set_fill_color("#1a1a1a");
    ctx.fill_rect(0.0, 0.0, w as f64, h as f64);

    // Margins for axes
    let margin_left = 50.0;
    let margin_bottom = 40.0;
    let margin_right = 20.0;
    let margin_top = 20.0;

    let plot_w = w - margin_left - margin_right;
    let plot_h = h - margin_bottom - margin_top;

    // Draw grid
    ctx.set_stroke_color("#2a2a2a");
    ctx.set_stroke_width(0.5);
    for i in 0..=5 {
        let y = margin_top + (i as f32 / 5.0) * plot_h;
        ctx.begin_path();
        ctx.move_to(margin_left as f64, y as f64);
        ctx.line_to((margin_left + plot_w) as f64, y as f64);
        ctx.stroke();
    }

    // Draw axes
    ctx.set_stroke_color("#e0e0e0");
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(margin_left as f64, margin_top as f64);
    ctx.line_to(margin_left as f64, (margin_top + plot_h) as f64);
    ctx.line_to((margin_left + plot_w) as f64, (margin_top + plot_h) as f64);
    ctx.stroke();

    // Draw bubbles (small to large)
    let mut sorted_bubbles = state.bubble_circles(plot_w, plot_h);
    sorted_bubbles.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));

    for (x, y, radius, color, label) in sorted_bubbles {
        let screen_x = margin_left + x;
        let screen_y = margin_top + (plot_h - y);

        // Fill bubble
        ctx.begin_path();
        ctx.arc(screen_x as f64, screen_y as f64, radius as f64, 0.0, std::f64::consts::TAU);
        let mut fill_color = color;
        fill_color[3] = 0.7; // Opacity
        ctx.set_fill_color(&rgba_to_hex(fill_color));
        ctx.fill();

        // Stroke border
        ctx.set_stroke_color("#ffffff");
        ctx.set_stroke_width(1.5);
        ctx.stroke();

        // Label (if large enough)
        if radius > 15.0 {
            ctx.set_fill_color("#ffffff");
            ctx.set_font("10px sans-serif");
            ctx.fill_text(label, screen_x as f64, (screen_y + 4.0) as f64);
        }
    }

    // Axis labels
    ctx.set_fill_color("#e0e0e0");
    ctx.set_font("12px sans-serif");
    ctx.fill_text("X Axis", (margin_left + plot_w / 2.0) as f64, (h - 10.0) as f64);

    ctx.save();
    ctx.translate(15.0, (margin_top + plot_h / 2.0) as f64);
    ctx.rotate(-(std::f32::consts::FRAC_PI_2 as f64));
    ctx.fill_text("Y Axis", 0.0, 0.0);
    ctx.restore();
}
