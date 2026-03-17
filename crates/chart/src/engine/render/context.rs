//! Platform-agnostic rendering context trait for charts
//!
//! This module extends uzor-render's RenderContext with chart-specific
//! coordinate conversion methods for bar/price data.

/// Chart-specific rendering context
///
/// Extends the base RenderContext from uzor-render with coordinate conversion
/// methods needed for rendering OHLCV charts and technical indicators.
pub trait RenderContext: uzor::render::RenderContext {
    // =========================================================================
    // Dimensions (moved from uzor-render)
    // =========================================================================

    /// Get chart area width
    fn chart_width(&self) -> f64;

    /// Get chart area height
    fn chart_height(&self) -> f64;

    /// Canvas dimensions (full canvas, not just chart area)
    fn canvas_width(&self) -> f64 {
        self.chart_width()
    }

    fn canvas_height(&self) -> f64 {
        self.chart_height()
    }

    // =========================================================================
    // Coordinate Conversion (for chart primitives)
    // =========================================================================

    /// Convert bar index to X coordinate
    fn bar_to_x(&self, bar: f64) -> f64;

    /// Convert price to Y coordinate
    fn price_to_y(&self, price: f64) -> f64;

    /// Update coordinate conversion parameters for a specific window
    /// This is called before rendering primitives for each window in multi-window layouts
    /// Parameters are: chart_width, chart_height, view_start, bar_spacing, price_min, price_max
    fn set_coordinate_space(
        &mut self,
        chart_width: f64,
        chart_height: f64,
        view_start: f64,
        bar_spacing: f64,
        price_min: f64,
        price_max: f64,
    );

    // =========================================================================
    // Gradient Fill (re-exposed from supertrait for dyn dispatch)
    // =========================================================================

    /// Fill the current path with a linear gradient.
    ///
    /// Explicitly declared here so `fill_linear_gradient` is part of the
    /// `dyn RenderContext` vtable.  The default mirrors the uzor supertrait
    /// fallback (flat fill with first stop color).  Concrete backends that
    /// support real gradients should override this method.
    fn fill_linear_gradient(
        &mut self,
        stops: &[(f32, &str)],
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
    ) {
        let _ = (x1, y1, x2, y2);
        if let Some((_, color)) = stops.first() {
            self.set_fill_color(color);
            self.fill();
        }
    }

    // =========================================================================
    // Line Style Helper (chart-specific)
    // =========================================================================

    /// Set line style from LineStyle enum
    fn set_line_style(&mut self, style: crate::chart::annotations::LineStyle) {
        match style {
            crate::chart::annotations::LineStyle::Solid => self.set_line_dash(&[]),
            crate::chart::annotations::LineStyle::Dashed => self.set_line_dash(&[8.0, 4.0]),
            crate::chart::annotations::LineStyle::Dotted => self.set_line_dash(&[2.0, 2.0]),
            crate::chart::annotations::LineStyle::LargeDashed => self.set_line_dash(&[12.0, 6.0]),
            crate::chart::annotations::LineStyle::SparseDotted => self.set_line_dash(&[2.0, 8.0]),
        }
    }
}

// =============================================================================
// Text Rendering Helpers
// =============================================================================

// Re-export types from uzor-render for use in this module
use uzor::render::{TextAlign, TextBaseline};

use crate::drawing::primitives_v2::{PrimitiveText, TextAlign as PrimitiveTextAlign};

/// Render text from PrimitiveText configuration
pub fn render_primitive_text(
    ctx: &mut dyn RenderContext,
    text: &PrimitiveText,
    x: f64,
    y: f64,
    fallback_color: &str,
) {
    render_primitive_text_rotated(ctx, text, x, y, fallback_color, 0.0);
}

/// Render text from PrimitiveText configuration with rotation
pub fn render_primitive_text_rotated(
    ctx: &mut dyn RenderContext,
    text: &PrimitiveText,
    x: f64,
    y: f64,
    fallback_color: &str,
    rotation: f64,
) {
    if text.content.is_empty() {
        return;
    }

    // Build font string
    let mut font_parts = Vec::new();
    if text.italic {
        font_parts.push("italic".to_string());
    }
    if text.bold {
        font_parts.push("bold".to_string());
    }
    font_parts.push(format!("{}px", text.font_size as i32));
    font_parts.push("sans-serif".to_string());
    let font = font_parts.join(" ");

    ctx.set_font(&font);

    // Set alignment
    let h_align = match text.h_align {
        PrimitiveTextAlign::Start => TextAlign::Left,
        PrimitiveTextAlign::Center => TextAlign::Center,
        PrimitiveTextAlign::End => TextAlign::Right,
    };
    ctx.set_text_align(h_align);

    // Set vertical alignment (baseline)
    let baseline = match text.v_align {
        PrimitiveTextAlign::Start => TextBaseline::Top,
        PrimitiveTextAlign::Center => TextBaseline::Middle,
        PrimitiveTextAlign::End => TextBaseline::Bottom,
    };
    ctx.set_text_baseline(baseline);

    // Set color
    let color = text.color.as_deref().unwrap_or(fallback_color);
    ctx.set_fill_color(color);

    // Render text lines with optional rotation
    let line_height = text.font_size * 1.2;
    let lines: Vec<&str> = text.content.lines().collect();

    for (i, line) in lines.iter().enumerate() {
        let line_y = y + (i as f64 * line_height);
        ctx.fill_text_rotated(line, x, line_y, rotation);
    }
}

/// Measure text dimensions from PrimitiveText configuration
/// Returns (width, height)
pub fn measure_primitive_text(ctx: &dyn RenderContext, text: &PrimitiveText) -> (f64, f64) {
    if text.content.is_empty() {
        return (0.0, 0.0);
    }

    let lines: Vec<&str> = text.content.lines().collect();
    let line_height = text.font_size * 1.2;
    let height = lines.len() as f64 * line_height;

    let mut max_width = 0.0f64;
    for line in &lines {
        let w = ctx.measure_text(line);
        if w > max_width {
            max_width = w;
        }
    }

    (max_width, height)
}

/// Render text with optional background
pub fn render_text_with_background(
    ctx: &mut dyn RenderContext,
    text: &PrimitiveText,
    x: f64,
    y: f64,
    fallback_color: &str,
    bg_color: Option<&str>,
    padding: f64,
) {
    if text.content.is_empty() {
        return;
    }

    // Setup font first for measurement
    let mut font_parts = Vec::new();
    if text.italic {
        font_parts.push("italic".to_string());
    }
    if text.bold {
        font_parts.push("bold".to_string());
    }
    font_parts.push(format!("{}px", text.font_size as i32));
    font_parts.push("sans-serif".to_string());
    let font = font_parts.join(" ");
    ctx.set_font(&font);

    // Measure text
    let (text_width, text_height) = measure_primitive_text(ctx, text);

    // Calculate background rect position based on alignment
    let bg_x = match text.h_align {
        PrimitiveTextAlign::Start => x - padding,
        PrimitiveTextAlign::Center => x - text_width / 2.0 - padding,
        PrimitiveTextAlign::End => x - text_width - padding,
    };
    let bg_y = match text.v_align {
        PrimitiveTextAlign::Start => y - padding,
        PrimitiveTextAlign::Center => y - text_height / 2.0 - padding,
        PrimitiveTextAlign::End => y - text_height - padding,
    };

    // Draw background if specified
    if let Some(bg) = bg_color {
        ctx.set_fill_color(bg);
        ctx.fill_rect(
            bg_x,
            bg_y,
            text_width + padding * 2.0,
            text_height + padding * 2.0,
        );
    }

    // Draw text
    render_primitive_text(ctx, text, x, y, fallback_color);
}

// =============================================================================
// SVG Icon Rendering
// =============================================================================

/// Draw an SVG icon scaled to fit within the given rectangle.
///
/// The SVG is parsed and rendered using the stroke color.
/// Supports: path, circle, rect, line, polyline, polygon elements.
///
/// # Arguments
/// * `ctx` - Render context
/// * `svg` - SVG string content
/// * `x`, `y` - Top-left corner position
/// * `width`, `height` - Target dimensions
/// * `color` - Stroke color (hex string)
pub fn draw_svg_icon(ctx: &mut dyn RenderContext, svg: &str, x: f64, y: f64, width: f64, height: f64, color: &str) {
    // Parse viewBox to get source dimensions (default 24x24)
    let (vb_width, vb_height) = parse_viewbox(svg).unwrap_or((24.0, 24.0));

    // Calculate scale and offset for centering
    let scale_x = width / vb_width;
    let scale_y = height / vb_height;
    let scale = scale_x.min(scale_y); // Uniform scale to fit

    let offset_x = x + (width - vb_width * scale) / 2.0;
    let offset_y = y + (height - vb_height * scale) / 2.0;

    // Check if root SVG has fill="none" - if so, children default to stroke-only
    // This is the SVG inheritance model: fill="none" on root means no fill unless overridden
    let has_fill_none = svg_root_has_fill_none(svg);
    let default_filled = !has_fill_none;

    // Fixed stroke width for crisp rendering (1.5px works well for 16-32px icons)
    let stroke_width = 1.5 * scale;

    // Set stroke style
    ctx.set_stroke_color(color);
    ctx.set_stroke_width(stroke_width);
    ctx.set_line_cap("round");
    ctx.set_line_join("round");
    ctx.set_line_dash(&[]);

    // Parse and render all path elements
    for path_info in parse_svg_paths(svg, default_filled) {
        ctx.begin_path();
        render_path_data(ctx, &path_info.d, offset_x, offset_y, scale);

        // Fill first, then stroke (so stroke is on top)
        if path_info.filled {
            ctx.set_fill_color(color);
            ctx.fill();
        }
        if path_info.stroked {
            // Apply dash array if present (scaled)
            if let Some(ref dash) = path_info.dash_array {
                let scaled_dash: Vec<f64> = dash.iter().map(|d| d * scale).collect();
                ctx.set_line_dash(&scaled_dash);
            }
            ctx.stroke();
            // Reset dash after stroke
            if path_info.dash_array.is_some() {
                ctx.set_line_dash(&[]);
            }
        }
    }

    // Parse and render all circle elements
    for (cx, cy, r, filled) in parse_svg_circles(svg, default_filled) {
        let tx = offset_x + cx * scale;
        let ty = offset_y + cy * scale;
        let tr = r * scale;

        ctx.begin_path();
        ctx.arc(tx, ty, tr, 0.0, std::f64::consts::PI * 2.0);
        if filled {
            ctx.set_fill_color(color);
            ctx.fill();
        } else {
            ctx.stroke();
        }
    }

    // Parse and render all rect elements
    for (rx, ry, rw, rh, rounding, filled) in parse_svg_rects(svg, default_filled) {
        let tx = offset_x + rx * scale;
        let ty = offset_y + ry * scale;
        let tw = rw * scale;
        let th = rh * scale;
        let tr = rounding * scale;

        if filled {
            ctx.set_fill_color(color);
            if tr > 0.0 {
                ctx.fill_rounded_rect(tx, ty, tw, th, tr);
            } else {
                ctx.fill_rect(tx, ty, tw, th);
            }
        } else {
            if tr > 0.0 {
                ctx.stroke_rounded_rect(tx, ty, tw, th, tr);
            } else {
                ctx.stroke_rect(tx, ty, tw, th);
            }
        }
    }

    // Parse and render all line elements
    for (x1, y1, x2, y2) in parse_svg_lines(svg) {
        let tx1 = offset_x + x1 * scale;
        let ty1 = offset_y + y1 * scale;
        let tx2 = offset_x + x2 * scale;
        let ty2 = offset_y + y2 * scale;

        ctx.begin_path();
        ctx.move_to(tx1, ty1);
        ctx.line_to(tx2, ty2);
        ctx.stroke();
    }

    // Parse and render all polyline elements
    for (points, closed) in parse_svg_polylines(svg) {
        if points.len() >= 2 {
            ctx.begin_path();
            let (px, py) = points[0];
            ctx.move_to(offset_x + px * scale, offset_y + py * scale);
            for &(px, py) in &points[1..] {
                ctx.line_to(offset_x + px * scale, offset_y + py * scale);
            }
            if closed {
                ctx.close_path();
            }
            ctx.stroke();
        }
    }
}

/// Render a multi-color SVG, preserving each path element's original `fill` color
/// attribute instead of overriding with a single color. Suitable for mascot/logo SVGs
/// that use multiple fill colors across their paths.
///
/// # Arguments
/// * `ctx` - Render context
/// * `svg` - SVG string content
/// * `x`, `y` - Top-left position
/// * `width`, `height` - Target dimensions
pub fn draw_svg_multicolor(ctx: &mut dyn RenderContext, svg: &str, x: f64, y: f64, width: f64, height: f64) {
    let (vb_width, vb_height) = parse_viewbox(svg).unwrap_or((24.0, 24.0));
    let scale_x = width / vb_width;
    let scale_y = height / vb_height;
    let scale = scale_x.min(scale_y);
    let offset_x = x + (width - vb_width * scale) / 2.0;
    let offset_y = y + (height - vb_height * scale) / 2.0;
    let has_fill_none = svg_root_has_fill_none(svg);
    let default_filled = !has_fill_none;

    // Render path elements preserving each path's fill and stroke colors
    for path_info in parse_svg_paths(svg, default_filled) {
        ctx.begin_path();
        render_path_data(ctx, &path_info.d, offset_x, offset_y, scale);

        if path_info.filled {
            if let Some(ref color) = path_info.fill_color {
                if color.starts_with("url(#") {
                    let grad_id = color.strip_prefix("url(#").and_then(|s| s.strip_suffix(')'));
                    if let Some(id) = grad_id {
                        if let Some(grad) = parse_gradient(svg, id) {
                            // Transform gradient coordinates from SVG space to screen space
                            let gx1 = offset_x + grad.x1 * scale;
                            let gy1 = offset_y + grad.y1 * scale;
                            let gx2 = offset_x + grad.x2 * scale;
                            let gy2 = offset_y + grad.y2 * scale;
                            let stops_refs: Vec<(f32, &str)> = grad
                                .stops
                                .iter()
                                .map(|(o, c)| (*o, c.as_str()))
                                .collect();
                            ctx.fill_linear_gradient(&stops_refs, gx1, gy1, gx2, gy2);
                        } else {
                            // Gradient not found — flat black fallback
                            ctx.set_fill_color("black");
                            ctx.fill();
                        }
                    } else {
                        ctx.set_fill_color("black");
                        ctx.fill();
                    }
                } else {
                    ctx.set_fill_color(color);
                    ctx.fill();
                }
            } else {
                ctx.set_fill_color("black");
                ctx.fill();
            }
        }

        if path_info.stroked {
            let sc = path_info.stroke_color.as_deref().unwrap_or("black");
            let sw = path_info.stroke_width.unwrap_or(1.0) * scale;
            ctx.set_stroke_color(sc);
            ctx.set_stroke_width(sw);
            ctx.set_line_cap("round");
            ctx.set_line_join("round");
            if let Some(ref dash) = path_info.dash_array {
                let scaled_dash: Vec<f64> = dash.iter().map(|d| d * scale).collect();
                ctx.set_line_dash(&scaled_dash);
            } else {
                ctx.set_line_dash(&[]);
            }
            ctx.stroke();
            if path_info.dash_array.is_some() {
                ctx.set_line_dash(&[]);
            }
        }
    }

    // Render rect elements (skip full-size background rects)
    for (rx, ry, rw, rh, rounding, filled) in parse_svg_rects(svg, default_filled) {
        if filled {
            // Skip full-size background rects that cover the entire viewbox
            if rw >= vb_width * 0.95 && rh >= vb_height * 0.95 {
                continue;
            }
            let tx = offset_x + rx * scale;
            let ty = offset_y + ry * scale;
            let tw = rw * scale;
            let th = rh * scale;
            ctx.set_fill_color("black");
            if rounding > 0.0 {
                ctx.fill_rounded_rect(tx, ty, tw, th, rounding * scale);
            } else {
                ctx.fill_rect(tx, ty, tw, th);
            }
        }
    }

    // Render circle elements
    for (cx_val, cy_val, r, filled) in parse_svg_circles(svg, default_filled) {
        if filled {
            let tx = offset_x + cx_val * scale;
            let ty = offset_y + cy_val * scale;
            let tr = r * scale;
            ctx.begin_path();
            ctx.arc(tx, ty, tr, 0.0, std::f64::consts::PI * 2.0);
            ctx.set_fill_color("black");
            ctx.fill();
        }
    }
}

// =============================================================================
// SVG Path Parsing
// =============================================================================

/// Resolve a gradient URL reference to a fallback solid color by extracting
/// the first stop-color from the gradient definition in the SVG.
///
/// `url_ref` is like `"url(#paint0_linear_1_13)"`.
fn resolve_gradient_color(svg: &str, url_ref: &str) -> Option<String> {
    let id = url_ref.strip_prefix("url(#")?.strip_suffix(')')?;
    let search = format!("id=\"{}\"", id);
    let grad_pos = svg.find(&search)?;
    let after_grad = &svg[grad_pos..];
    let stop_pos = after_grad.find("stop-color=\"")?;
    let color_start = stop_pos + 12; // len("stop-color=\"")
    let color_end = after_grad[color_start..].find('"')?;
    Some(after_grad[color_start..color_start + color_end].to_string())
}

/// Parsed gradient info extracted from an SVG `<linearGradient>` element.
struct GradientInfo {
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    /// (offset 0.0..=1.0, color hex/name)
    stops: Vec<(f32, String)>,
}

/// Parse a `<linearGradient>` element from the SVG source by its `id` attribute.
///
/// Returns `None` if the gradient is not found or cannot be parsed.
fn parse_gradient(svg: &str, gradient_id: &str) -> Option<GradientInfo> {
    let search = format!("id=\"{}\"", gradient_id);
    let grad_pos = svg.find(&search)?;
    let after = &svg[grad_pos..];

    let grad_end = after.find("</linearGradient>")?;
    let grad_text = &after[..grad_end];

    // Parse coordinate attributes; default to a top→bottom gradient when missing
    let x1 = extract_attr(grad_text, "x1=\"")
        .and_then(|s| s.trim_end_matches('%').parse::<f64>().ok())
        .unwrap_or(0.0);
    let y1 = extract_attr(grad_text, "y1=\"")
        .and_then(|s| s.trim_end_matches('%').parse::<f64>().ok())
        .unwrap_or(0.0);
    let x2 = extract_attr(grad_text, "x2=\"")
        .and_then(|s| s.trim_end_matches('%').parse::<f64>().ok())
        .unwrap_or(0.0);
    let y2 = extract_attr(grad_text, "y2=\"")
        .and_then(|s| s.trim_end_matches('%').parse::<f64>().ok())
        .unwrap_or(1.0);

    // Parse all <stop …/> elements within the gradient block
    let mut stops: Vec<(f32, String)> = Vec::new();
    let mut search_from = 0usize;
    while let Some(stop_rel) = grad_text[search_from..].find("<stop") {
        let abs = search_from + stop_rel;
        let remaining = &grad_text[abs..];
        let stop_end = match remaining.find("/>") {
            Some(e) => e,
            None => break,
        };
        let stop_tag = &remaining[..stop_end + 2];

        let offset = extract_attr(stop_tag, "offset=\"")
            .and_then(|s| {
                // offset may be a bare number ("0.5") or a percentage ("50%")
                let s = s.trim_end_matches('%');
                s.parse::<f32>().ok().map(|v| if v > 1.0 { v / 100.0 } else { v })
            })
            .unwrap_or(0.0);

        // stop-color can appear as an attribute or inside a style="..." attribute
        let color = extract_attr(stop_tag, "stop-color=\"")
            .or_else(|| {
                // Try style="stop-color:#xxx"
                extract_attr(stop_tag, "style=\"").and_then(|style| {
                    let sc_pos = style.find("stop-color:")?;
                    let after_sc = style[sc_pos + 11..].trim_start();
                    let end = after_sc.find(|c: char| c == ';' || c == '"').unwrap_or(after_sc.len());
                    Some(after_sc[..end].trim().to_string())
                })
            })
            .unwrap_or_else(|| "black".to_string());

        stops.push((offset, color));
        search_from = abs + stop_end + 2;
    }

    if stops.is_empty() {
        return None;
    }

    Some(GradientInfo { x1, y1, x2, y2, stops })
}

/// Extract the value of a quoted attribute from a tag snippet.
///
/// `attr_prefix` should include the opening quote, e.g. `"x1=\""`.
fn extract_attr(text: &str, attr_prefix: &str) -> Option<String> {
    let start = text.find(attr_prefix)? + attr_prefix.len();
    let end = text[start..].find('"')?;
    Some(text[start..start + end].to_string())
}

/// Parse viewBox from SVG string
/// Returns (width, height) or None if not found
fn parse_viewbox(svg: &str) -> Option<(f64, f64)> {
    // Look for viewBox="x y w h"
    let vb_start = svg.find("viewBox=\"")?;
    let vb_content_start = vb_start + 9;
    let vb_end = svg[vb_content_start..].find('"')?;
    let vb_str = &svg[vb_content_start..vb_content_start + vb_end];

    let parts: Vec<&str> = vb_str.split_whitespace().collect();
    if parts.len() >= 4 {
        let w = parts[2].parse::<f64>().ok()?;
        let h = parts[3].parse::<f64>().ok()?;
        Some((w, h))
    } else {
        None
    }
}

/// Path rendering info
struct PathInfo {
    d: String,
    filled: bool,
    stroked: bool,
    dash_array: Option<Vec<f64>>,
    fill_color: Option<String>,    // Actual fill color from attribute (for multicolor SVGs)
    stroke_color: Option<String>,  // Actual stroke color from attribute (for multicolor SVGs)
    stroke_width: Option<f64>,     // Stroke width from attribute
}

/// Extract all path elements from SVG with fill/stroke info
/// `default_filled` is inherited from parent SVG element
fn parse_svg_paths(svg: &str, default_filled: bool) -> Vec<PathInfo> {
    let mut paths = Vec::new();
    let mut search_from = 0;

    while let Some(start) = svg[search_from..].find("<path") {
        let abs_start = search_from + start;
        // Find end of tag
        let tag_end = if let Some(end) = svg[abs_start..].find("/>") {
            abs_start + end + 2
        } else if let Some(end) = svg[abs_start..].find('>') {
            abs_start + end + 1
        } else {
            break;
        };

        let tag_content = &svg[abs_start..tag_end];

        // Extract d attribute
        if let Some(d_start) = tag_content.find(" d=\"") {
            let d_content_start = d_start + 4;
            if let Some(d_end) = tag_content[d_content_start..].find('"') {
                let d = tag_content[d_content_start..d_content_start + d_end].to_string();

                // Check fill attribute
                let (filled, fill_color) = if let Some(fill_start) = tag_content.find("fill=\"") {
                    let fill_content_start = fill_start + 6;
                    if let Some(fill_end) = tag_content[fill_content_start..].find('"') {
                        let fill_value = &tag_content[fill_content_start..fill_content_start + fill_end];
                        if fill_value != "none" {
                            (true, Some(fill_value.to_string()))
                        } else {
                            (false, None)
                        }
                    } else {
                        (false, None)
                    }
                } else {
                    (default_filled, None) // Use inherited default from root SVG
                };

                // Check stroke attribute (default is stroked for icons)
                let (stroked, stroke_color) = if let Some(stroke_start) = tag_content.find("stroke=\"") {
                    let stroke_content_start = stroke_start + 8;
                    if let Some(stroke_end) = tag_content[stroke_content_start..].find('"') {
                        let stroke_value = &tag_content[stroke_content_start..stroke_content_start + stroke_end];
                        if stroke_value != "none" {
                            (true, Some(stroke_value.to_string()))
                        } else {
                            (false, None)
                        }
                    } else {
                        (true, None)
                    }
                } else {
                    (!filled, None) // If not filled, assume stroked
                };

                // Check stroke-width attribute
                let stroke_width = if let Some(sw_start) = tag_content.find("stroke-width=\"") {
                    let sw_content_start = sw_start + 14;
                    if let Some(sw_end) = tag_content[sw_content_start..].find('"') {
                        tag_content[sw_content_start..sw_content_start + sw_end].parse::<f64>().ok()
                    } else {
                        None
                    }
                } else {
                    None
                };

                // Check stroke-dasharray attribute (e.g., "4 2" for dashed lines)
                let dash_array = if let Some(dash_start) = tag_content.find("stroke-dasharray=\"") {
                    let dash_content_start = dash_start + 18;
                    if let Some(dash_end) = tag_content[dash_content_start..].find('"') {
                        let dash_value = &tag_content[dash_content_start..dash_content_start + dash_end];
                        // Parse "4 2" or "4,2" format
                        let values: Vec<f64> = dash_value
                            .split(|c| c == ' ' || c == ',')
                            .filter_map(|s| s.trim().parse::<f64>().ok())
                            .collect();
                        if !values.is_empty() {
                            Some(values)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                };

                paths.push(PathInfo { d, filled, stroked, dash_array, fill_color, stroke_color, stroke_width });
            }
        }

        search_from = tag_end;
    }

    paths
}

/// Number of segments for arc approximation
const ARC_SEGMENTS: usize = 16;

/// Convert SVG arc parameters to a series of points
/// Based on the SVG arc to bezier algorithm
fn arc_to_points(
    start_x: f64,
    start_y: f64,
    mut rx: f64,
    mut ry: f64,
    x_rotation: f64,
    large_arc: bool,
    sweep: bool,
    end_x: f64,
    end_y: f64,
) -> Vec<(f64, f64)> {
    let mut points = Vec::new();

    // Handle degenerate cases
    if (start_x - end_x).abs() < 0.001 && (start_y - end_y).abs() < 0.001 {
        return points;
    }

    rx = rx.abs();
    ry = ry.abs();

    if rx < 0.001 || ry < 0.001 {
        // Straight line
        points.push((end_x, end_y));
        return points;
    }

    let phi = x_rotation.to_radians();
    let cos_phi = phi.cos();
    let sin_phi = phi.sin();

    // Step 1: Compute (x1', y1')
    let dx = (start_x - end_x) / 2.0;
    let dy = (start_y - end_y) / 2.0;
    let x1p = cos_phi * dx + sin_phi * dy;
    let y1p = -sin_phi * dx + cos_phi * dy;

    // Step 2: Compute (cx', cy')
    let x1p2 = x1p * x1p;
    let y1p2 = y1p * y1p;
    let rx2 = rx * rx;
    let ry2 = ry * ry;

    // Correct radii if needed
    let lambda = x1p2 / rx2 + y1p2 / ry2;
    if lambda > 1.0 {
        let sqrt_lambda = lambda.sqrt();
        rx *= sqrt_lambda;
        ry *= sqrt_lambda;
    }

    let rx2 = rx * rx;
    let ry2 = ry * ry;

    let num = rx2 * ry2 - rx2 * y1p2 - ry2 * x1p2;
    let denom = rx2 * y1p2 + ry2 * x1p2;

    let factor = if denom > 0.0 && num > 0.0 {
        let mut f = (num / denom).sqrt();
        if large_arc == sweep {
            f = -f;
        }
        f
    } else {
        0.0
    };

    let cxp = factor * rx * y1p / ry;
    let cyp = -factor * ry * x1p / rx;

    // Step 3: Compute (cx, cy) from (cx', cy')
    let cx = cos_phi * cxp - sin_phi * cyp + (start_x + end_x) / 2.0;
    let cy = sin_phi * cxp + cos_phi * cyp + (start_y + end_y) / 2.0;

    // Step 4: Compute angles
    let ux = (x1p - cxp) / rx;
    let uy = (y1p - cyp) / ry;
    let vx = (-x1p - cxp) / rx;
    let vy = (-y1p - cyp) / ry;

    // Angle start
    let n = (ux * ux + uy * uy).sqrt();
    let theta1 = if uy < 0.0 { -1.0 } else { 1.0 } * (ux / n).clamp(-1.0, 1.0).acos();

    // Angle extent
    let n = ((ux * ux + uy * uy) * (vx * vx + vy * vy)).sqrt();
    let dot = ux * vx + uy * vy;
    let mut dtheta = if ux * vy - uy * vx < 0.0 { -1.0 } else { 1.0 } * (dot / n).clamp(-1.0, 1.0).acos();

    if !sweep && dtheta > 0.0 {
        dtheta -= 2.0 * std::f64::consts::PI;
    } else if sweep && dtheta < 0.0 {
        dtheta += 2.0 * std::f64::consts::PI;
    }

    // Generate points along the arc
    for i in 1..=ARC_SEGMENTS {
        let t = i as f64 / ARC_SEGMENTS as f64;
        let theta = theta1 + dtheta * t;

        let cos_theta = theta.cos();
        let sin_theta = theta.sin();

        // Point on unit circle, scaled by radii
        let px = rx * cos_theta;
        let py = ry * sin_theta;

        // Rotate and translate
        let x = cos_phi * px - sin_phi * py + cx;
        let y = sin_phi * px + cos_phi * py + cy;

        points.push((x, y));
    }

    points
}

/// Render SVG path data onto a RenderContext
fn render_path_data(ctx: &mut dyn RenderContext, path_data: &str, offset_x: f64, offset_y: f64, scale: f64) {
    let mut current_x = 0.0;
    let mut current_y = 0.0;
    let mut start_x = 0.0;
    let mut start_y = 0.0;
    let mut last_control: Option<(f64, f64)> = None; // For smooth curves (S, T)

    let mut chars = path_data.chars().peekable();
    let mut current_cmd = 'M';

    while chars.peek().is_some() {
        // Skip whitespace and commas
        while chars.peek().map(|c| c.is_whitespace() || *c == ',').unwrap_or(false) {
            chars.next();
        }

        // Check for command
        if let Some(&c) = chars.peek() {
            if c.is_alphabetic() {
                current_cmd = c;
                chars.next();
                // Skip whitespace after command
                while chars.peek().map(|c| c.is_whitespace() || *c == ',').unwrap_or(false) {
                    chars.next();
                }
            }
        }

        match current_cmd {
            'M' => {
                // Absolute move
                if let Some((x, y)) = parse_two_numbers(&mut chars) {
                    current_x = x;
                    current_y = y;
                    start_x = x;
                    start_y = y;
                    ctx.move_to(offset_x + x * scale, offset_y + y * scale);
                    current_cmd = 'L'; // Subsequent coordinates are line-to
                    last_control = None;
                }
            }
            'm' => {
                // Relative move
                if let Some((dx, dy)) = parse_two_numbers(&mut chars) {
                    current_x += dx;
                    current_y += dy;
                    start_x = current_x;
                    start_y = current_y;
                    ctx.move_to(offset_x + current_x * scale, offset_y + current_y * scale);
                    current_cmd = 'l'; // Subsequent coordinates are relative line-to
                    last_control = None;
                }
            }
            'L' => {
                // Absolute line
                if let Some((x, y)) = parse_two_numbers(&mut chars) {
                    current_x = x;
                    current_y = y;
                    ctx.line_to(offset_x + x * scale, offset_y + y * scale);
                    last_control = None;
                }
            }
            'l' => {
                // Relative line
                if let Some((dx, dy)) = parse_two_numbers(&mut chars) {
                    current_x += dx;
                    current_y += dy;
                    ctx.line_to(offset_x + current_x * scale, offset_y + current_y * scale);
                    last_control = None;
                }
            }
            'H' => {
                // Absolute horizontal line
                if let Some(x) = parse_number(&mut chars) {
                    current_x = x;
                    ctx.line_to(offset_x + x * scale, offset_y + current_y * scale);
                    last_control = None;
                }
            }
            'h' => {
                // Relative horizontal line
                if let Some(dx) = parse_number(&mut chars) {
                    current_x += dx;
                    ctx.line_to(offset_x + current_x * scale, offset_y + current_y * scale);
                    last_control = None;
                }
            }
            'V' => {
                // Absolute vertical line
                if let Some(y) = parse_number(&mut chars) {
                    current_y = y;
                    ctx.line_to(offset_x + current_x * scale, offset_y + y * scale);
                    last_control = None;
                }
            }
            'v' => {
                // Relative vertical line
                if let Some(dy) = parse_number(&mut chars) {
                    current_y += dy;
                    ctx.line_to(offset_x + current_x * scale, offset_y + current_y * scale);
                    last_control = None;
                }
            }
            'C' => {
                // Absolute cubic bezier
                if let Some((c1x, c1y, c2x, c2y, x, y)) = parse_six_numbers(&mut chars) {
                    ctx.bezier_curve_to(
                        offset_x + c1x * scale,
                        offset_y + c1y * scale,
                        offset_x + c2x * scale,
                        offset_y + c2y * scale,
                        offset_x + x * scale,
                        offset_y + y * scale,
                    );
                    current_x = x;
                    current_y = y;
                    last_control = Some((c2x, c2y));
                }
            }
            'c' => {
                // Relative cubic bezier
                if let Some((dc1x, dc1y, dc2x, dc2y, dx, dy)) = parse_six_numbers(&mut chars) {
                    let c1x = current_x + dc1x;
                    let c1y = current_y + dc1y;
                    let c2x = current_x + dc2x;
                    let c2y = current_y + dc2y;
                    let x = current_x + dx;
                    let y = current_y + dy;
                    ctx.bezier_curve_to(
                        offset_x + c1x * scale,
                        offset_y + c1y * scale,
                        offset_x + c2x * scale,
                        offset_y + c2y * scale,
                        offset_x + x * scale,
                        offset_y + y * scale,
                    );
                    current_x = x;
                    current_y = y;
                    last_control = Some((c2x, c2y));
                }
            }
            'S' => {
                // Smooth cubic bezier (absolute)
                if let Some((c2x, c2y, x, y)) = parse_four_numbers(&mut chars) {
                    // Reflect last control point
                    let (c1x, c1y) = match last_control {
                        Some((lx, ly)) => (2.0 * current_x - lx, 2.0 * current_y - ly),
                        None => (current_x, current_y),
                    };
                    ctx.bezier_curve_to(
                        offset_x + c1x * scale,
                        offset_y + c1y * scale,
                        offset_x + c2x * scale,
                        offset_y + c2y * scale,
                        offset_x + x * scale,
                        offset_y + y * scale,
                    );
                    current_x = x;
                    current_y = y;
                    last_control = Some((c2x, c2y));
                }
            }
            's' => {
                // Smooth cubic bezier (relative)
                if let Some((dc2x, dc2y, dx, dy)) = parse_four_numbers(&mut chars) {
                    let (c1x, c1y) = match last_control {
                        Some((lx, ly)) => (2.0 * current_x - lx, 2.0 * current_y - ly),
                        None => (current_x, current_y),
                    };
                    let c2x = current_x + dc2x;
                    let c2y = current_y + dc2y;
                    let x = current_x + dx;
                    let y = current_y + dy;
                    ctx.bezier_curve_to(
                        offset_x + c1x * scale,
                        offset_y + c1y * scale,
                        offset_x + c2x * scale,
                        offset_y + c2y * scale,
                        offset_x + x * scale,
                        offset_y + y * scale,
                    );
                    current_x = x;
                    current_y = y;
                    last_control = Some((c2x, c2y));
                }
            }
            'Q' => {
                // Absolute quadratic bezier
                if let Some((cx, cy, x, y)) = parse_four_numbers(&mut chars) {
                    ctx.quadratic_curve_to(
                        offset_x + cx * scale,
                        offset_y + cy * scale,
                        offset_x + x * scale,
                        offset_y + y * scale,
                    );
                    current_x = x;
                    current_y = y;
                    last_control = Some((cx, cy));
                }
            }
            'q' => {
                // Relative quadratic bezier
                if let Some((dcx, dcy, dx, dy)) = parse_four_numbers(&mut chars) {
                    let cx = current_x + dcx;
                    let cy = current_y + dcy;
                    let x = current_x + dx;
                    let y = current_y + dy;
                    ctx.quadratic_curve_to(
                        offset_x + cx * scale,
                        offset_y + cy * scale,
                        offset_x + x * scale,
                        offset_y + y * scale,
                    );
                    current_x = x;
                    current_y = y;
                    last_control = Some((cx, cy));
                }
            }
            'T' => {
                // Smooth quadratic bezier (absolute)
                if let Some((x, y)) = parse_two_numbers(&mut chars) {
                    let (cx, cy) = match last_control {
                        Some((lx, ly)) => (2.0 * current_x - lx, 2.0 * current_y - ly),
                        None => (current_x, current_y),
                    };
                    ctx.quadratic_curve_to(
                        offset_x + cx * scale,
                        offset_y + cy * scale,
                        offset_x + x * scale,
                        offset_y + y * scale,
                    );
                    current_x = x;
                    current_y = y;
                    last_control = Some((cx, cy));
                }
            }
            't' => {
                // Smooth quadratic bezier (relative)
                if let Some((dx, dy)) = parse_two_numbers(&mut chars) {
                    let (cx, cy) = match last_control {
                        Some((lx, ly)) => (2.0 * current_x - lx, 2.0 * current_y - ly),
                        None => (current_x, current_y),
                    };
                    let x = current_x + dx;
                    let y = current_y + dy;
                    ctx.quadratic_curve_to(
                        offset_x + cx * scale,
                        offset_y + cy * scale,
                        offset_x + x * scale,
                        offset_y + y * scale,
                    );
                    current_x = x;
                    current_y = y;
                    last_control = Some((cx, cy));
                }
            }
            'A' | 'a' => {
                // Arc command: rx ry x-rotation large-arc-flag sweep-flag x y
                let is_relative = current_cmd == 'a';
                if let Some((rx, ry, rotation, large, sweep, x, y)) = parse_arc_params(&mut chars) {
                    let (end_x, end_y) = if is_relative {
                        (current_x + x, current_y + y)
                    } else {
                        (x, y)
                    };

                    // Convert arc to points and draw them
                    let arc_points = arc_to_points(
                        current_x, current_y,
                        rx, ry,
                        rotation,
                        large != 0.0,
                        sweep != 0.0,
                        end_x, end_y,
                    );

                    for (px, py) in arc_points {
                        ctx.line_to(offset_x + px * scale, offset_y + py * scale);
                    }

                    current_x = end_x;
                    current_y = end_y;
                    last_control = None;
                }
            }
            'Z' | 'z' => {
                // Close path
                ctx.close_path();
                current_x = start_x;
                current_y = start_y;
                last_control = None;
            }
            _ => {
                // Unknown command, skip
                chars.next();
            }
        }
    }
}

fn parse_number(chars: &mut std::iter::Peekable<std::str::Chars>) -> Option<f64> {
    // Skip whitespace and commas
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() || c == ',' {
            chars.next();
        } else {
            break;
        }
    }

    let mut num_str = String::new();

    // Handle sign
    if let Some(&c) = chars.peek() {
        if c == '-' || c == '+' {
            num_str.push(chars.next().unwrap());
        }
    }

    // Collect digits and decimal point
    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() || c == '.' {
            num_str.push(chars.next().unwrap());
        } else {
            break;
        }
    }

    num_str.parse::<f64>().ok()
}

fn parse_two_numbers(chars: &mut std::iter::Peekable<std::str::Chars>) -> Option<(f64, f64)> {
    let x = parse_number(chars)?;
    let y = parse_number(chars)?;
    Some((x, y))
}

fn parse_four_numbers(chars: &mut std::iter::Peekable<std::str::Chars>) -> Option<(f64, f64, f64, f64)> {
    let a = parse_number(chars)?;
    let b = parse_number(chars)?;
    let c = parse_number(chars)?;
    let d = parse_number(chars)?;
    Some((a, b, c, d))
}

fn parse_six_numbers(chars: &mut std::iter::Peekable<std::str::Chars>) -> Option<(f64, f64, f64, f64, f64, f64)> {
    let a = parse_number(chars)?;
    let b = parse_number(chars)?;
    let c = parse_number(chars)?;
    let d = parse_number(chars)?;
    let e = parse_number(chars)?;
    let f = parse_number(chars)?;
    Some((a, b, c, d, e, f))
}

fn parse_arc_params(chars: &mut std::iter::Peekable<std::str::Chars>) -> Option<(f64, f64, f64, f64, f64, f64, f64)> {
    let rx = parse_number(chars)?;
    let ry = parse_number(chars)?;
    let rotation = parse_number(chars)?;
    let large_arc = parse_number(chars)?;
    let sweep = parse_number(chars)?;
    let x = parse_number(chars)?;
    let y = parse_number(chars)?;
    Some((rx, ry, rotation, large_arc, sweep, x, y))
}

// =============================================================================
// SVG Element Parsing (circle, rect, line, polyline, polygon)
// =============================================================================

/// Extract attribute value from SVG tag content
fn extract_svg_attr(content: &str, attr: &str) -> Option<f64> {
    let pattern = format!("{}=\"", attr);
    if let Some(start) = content.find(&pattern) {
        let value_start = start + pattern.len();
        if let Some(end) = content[value_start..].find('"') {
            return content[value_start..value_start + end].parse().ok();
        }
    }
    None
}

/// Check if element has fill="none" (stroked) or not (filled)
/// `default_filled` is the inherited fill state from parent SVG element
fn is_svg_filled_with_default(content: &str, default_filled: bool) -> bool {
    if let Some(start) = content.find("fill=\"") {
        let value_start = start + 6;
        if let Some(end) = content[value_start..].find('"') {
            let fill_value = &content[value_start..value_start + end];
            return fill_value != "none";
        }
    }
    // No fill attribute - use inherited default
    default_filled
}

/// Check if root SVG element has fill="none" (meaning children default to stroke-only)
fn svg_root_has_fill_none(svg: &str) -> bool {
    // Find the root <svg> tag
    if let Some(start) = svg.find("<svg") {
        if let Some(end) = svg[start..].find('>') {
            let svg_tag = &svg[start..start + end + 1];
            if let Some(fill_start) = svg_tag.find("fill=\"") {
                let value_start = fill_start + 6;
                if let Some(fill_end) = svg_tag[value_start..].find('"') {
                    let fill_value = &svg_tag[value_start..value_start + fill_end];
                    return fill_value == "none";
                }
            }
        }
    }
    false
}

/// Parse all <circle> elements from SVG
/// Returns Vec of (cx, cy, r, filled)
/// `default_filled` is inherited from parent SVG element
fn parse_svg_circles(svg: &str, default_filled: bool) -> Vec<(f64, f64, f64, bool)> {
    let mut circles = Vec::new();
    let mut search_from = 0;

    while let Some(start) = svg[search_from..].find("<circle") {
        let abs_start = search_from + start;
        // Find end of tag
        if let Some(end) = svg[abs_start..].find("/>") {
            let tag_content = &svg[abs_start..abs_start + end + 2];
            let cx = extract_svg_attr(tag_content, "cx").unwrap_or(0.0);
            let cy = extract_svg_attr(tag_content, "cy").unwrap_or(0.0);
            let r = extract_svg_attr(tag_content, "r").unwrap_or(0.0);
            let filled = is_svg_filled_with_default(tag_content, default_filled);

            if r > 0.0 {
                circles.push((cx, cy, r, filled));
            }
            search_from = abs_start + end + 2;
        } else if let Some(end) = svg[abs_start..].find('>') {
            let tag_content = &svg[abs_start..abs_start + end + 1];
            let cx = extract_svg_attr(tag_content, "cx").unwrap_or(0.0);
            let cy = extract_svg_attr(tag_content, "cy").unwrap_or(0.0);
            let r = extract_svg_attr(tag_content, "r").unwrap_or(0.0);
            let filled = is_svg_filled_with_default(tag_content, default_filled);

            if r > 0.0 {
                circles.push((cx, cy, r, filled));
            }
            search_from = abs_start + end + 1;
        } else {
            break;
        }
    }

    circles
}

/// Parse all <rect> elements from SVG
/// Returns Vec of (x, y, width, height, rx/rounding, filled)
/// `default_filled` is inherited from parent SVG element
fn parse_svg_rects(svg: &str, default_filled: bool) -> Vec<(f64, f64, f64, f64, f64, bool)> {
    let mut rects = Vec::new();
    let mut search_from = 0;

    while let Some(start) = svg[search_from..].find("<rect") {
        let abs_start = search_from + start;
        // Find end of tag
        if let Some(end) = svg[abs_start..].find("/>") {
            let tag_content = &svg[abs_start..abs_start + end + 2];
            let x = extract_svg_attr(tag_content, "x").unwrap_or(0.0);
            let y = extract_svg_attr(tag_content, "y").unwrap_or(0.0);
            let w = extract_svg_attr(tag_content, "width").unwrap_or(0.0);
            let h = extract_svg_attr(tag_content, "height").unwrap_or(0.0);
            let rx = extract_svg_attr(tag_content, "rx").unwrap_or(0.0);
            let filled = is_svg_filled_with_default(tag_content, default_filled);

            if w > 0.0 && h > 0.0 {
                rects.push((x, y, w, h, rx, filled));
            }
            search_from = abs_start + end + 2;
        } else if let Some(end) = svg[abs_start..].find('>') {
            let tag_content = &svg[abs_start..abs_start + end + 1];
            let x = extract_svg_attr(tag_content, "x").unwrap_or(0.0);
            let y = extract_svg_attr(tag_content, "y").unwrap_or(0.0);
            let w = extract_svg_attr(tag_content, "width").unwrap_or(0.0);
            let h = extract_svg_attr(tag_content, "height").unwrap_or(0.0);
            let rx = extract_svg_attr(tag_content, "rx").unwrap_or(0.0);
            let filled = is_svg_filled_with_default(tag_content, default_filled);

            if w > 0.0 && h > 0.0 {
                rects.push((x, y, w, h, rx, filled));
            }
            search_from = abs_start + end + 1;
        } else {
            break;
        }
    }

    rects
}

/// Parse all <line> elements from SVG
/// Returns Vec of (x1, y1, x2, y2)
fn parse_svg_lines(svg: &str) -> Vec<(f64, f64, f64, f64)> {
    let mut lines = Vec::new();
    let mut search_from = 0;

    while let Some(start) = svg[search_from..].find("<line") {
        let abs_start = search_from + start;
        // Find end of tag
        if let Some(end) = svg[abs_start..].find("/>") {
            let tag_content = &svg[abs_start..abs_start + end + 2];
            let x1 = extract_svg_attr(tag_content, "x1").unwrap_or(0.0);
            let y1 = extract_svg_attr(tag_content, "y1").unwrap_or(0.0);
            let x2 = extract_svg_attr(tag_content, "x2").unwrap_or(0.0);
            let y2 = extract_svg_attr(tag_content, "y2").unwrap_or(0.0);

            lines.push((x1, y1, x2, y2));
            search_from = abs_start + end + 2;
        } else if let Some(end) = svg[abs_start..].find('>') {
            let tag_content = &svg[abs_start..abs_start + end + 1];
            let x1 = extract_svg_attr(tag_content, "x1").unwrap_or(0.0);
            let y1 = extract_svg_attr(tag_content, "y1").unwrap_or(0.0);
            let x2 = extract_svg_attr(tag_content, "x2").unwrap_or(0.0);
            let y2 = extract_svg_attr(tag_content, "y2").unwrap_or(0.0);

            lines.push((x1, y1, x2, y2));
            search_from = abs_start + end + 1;
        } else {
            break;
        }
    }

    lines
}

/// Extract points attribute from polyline/polygon
fn extract_svg_points(content: &str) -> Vec<(f64, f64)> {
    let mut points = Vec::new();

    if let Some(start) = content.find("points=\"") {
        let value_start = start + 8;
        if let Some(end) = content[value_start..].find('"') {
            let points_str = &content[value_start..value_start + end];
            // Parse "x1,y1 x2,y2 x3,y3" format
            let mut chars = points_str.chars().peekable();

            loop {
                // Skip whitespace
                while chars.peek().map(|c| c.is_whitespace()).unwrap_or(false) {
                    chars.next();
                }

                if chars.peek().is_none() {
                    break;
                }

                // Parse number for x
                let mut num_str = String::new();
                if chars.peek() == Some(&'-') || chars.peek() == Some(&'+') {
                    num_str.push(chars.next().unwrap());
                }
                while chars.peek().map(|c| c.is_ascii_digit() || *c == '.').unwrap_or(false) {
                    num_str.push(chars.next().unwrap());
                }
                let x: f64 = match num_str.parse() {
                    Ok(v) => v,
                    Err(_) => break,
                };

                // Skip comma or space
                while chars.peek().map(|c| c.is_whitespace() || *c == ',').unwrap_or(false) {
                    chars.next();
                }

                // Parse number for y
                let mut num_str = String::new();
                if chars.peek() == Some(&'-') || chars.peek() == Some(&'+') {
                    num_str.push(chars.next().unwrap());
                }
                while chars.peek().map(|c| c.is_ascii_digit() || *c == '.').unwrap_or(false) {
                    num_str.push(chars.next().unwrap());
                }
                let y: f64 = match num_str.parse() {
                    Ok(v) => v,
                    Err(_) => break,
                };

                points.push((x, y));

                // Skip comma or space
                while chars.peek().map(|c| c.is_whitespace() || *c == ',').unwrap_or(false) {
                    chars.next();
                }
            }
        }
    }

    points
}

/// Parse all <polyline> and <polygon> elements from SVG
/// Returns Vec of (points, closed)
fn parse_svg_polylines(svg: &str) -> Vec<(Vec<(f64, f64)>, bool)> {
    let mut polylines = Vec::new();

    // Parse polylines (not closed)
    let mut search_from = 0;
    while let Some(start) = svg[search_from..].find("<polyline") {
        let abs_start = search_from + start;
        if let Some(end) = svg[abs_start..].find("/>") {
            let tag_content = &svg[abs_start..abs_start + end + 2];
            let points = extract_svg_points(tag_content);
            if !points.is_empty() {
                polylines.push((points, false));
            }
            search_from = abs_start + end + 2;
        } else if let Some(end) = svg[abs_start..].find('>') {
            let tag_content = &svg[abs_start..abs_start + end + 1];
            let points = extract_svg_points(tag_content);
            if !points.is_empty() {
                polylines.push((points, false));
            }
            search_from = abs_start + end + 1;
        } else {
            break;
        }
    }

    // Parse polygons (closed)
    search_from = 0;
    while let Some(start) = svg[search_from..].find("<polygon") {
        let abs_start = search_from + start;
        if let Some(end) = svg[abs_start..].find("/>") {
            let tag_content = &svg[abs_start..abs_start + end + 2];
            let points = extract_svg_points(tag_content);
            if !points.is_empty() {
                polylines.push((points, true));
            }
            search_from = abs_start + end + 2;
        } else if let Some(end) = svg[abs_start..].find('>') {
            let tag_content = &svg[abs_start..abs_start + end + 1];
            let points = extract_svg_points(tag_content);
            if !points.is_empty() {
                polylines.push((points, true));
            }
            search_from = abs_start + end + 1;
        } else {
            break;
        }
    }

    polylines
}
