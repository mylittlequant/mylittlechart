//! Platform-agnostic rendering context trait for charts
//!
//! This module extends uzor-render's RenderContext with chart-specific
//! coordinate conversion methods for bar/price data.

use crate::{Bar, timestamp_ms_to_bar_f64};

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

    /// Provide the bar slice used for timestamp → X conversion.
    ///
    /// Implementations that carry a bars reference override this to return it.
    /// The default returns an empty slice; callers fall back to `bar_to_x(0.0)`.
    fn bars(&self) -> &[Bar] {
        &[]
    }

    /// Store a bar slice so that `ts_to_x_ms` works correctly.
    ///
    /// Called before rendering primitives for each window so that the context
    /// has the correct bar data for timestamp → bar-index conversion.
    /// The default implementation is a no-op; backends that own a `Vec<Bar>`
    /// field override this to clone the slice in.
    fn set_bars(&mut self, _bars: &[Bar]) {}


    /// Convert a Unix timestamp in **milliseconds** to an X screen coordinate.
    ///
    /// Internally converts ms → fractional bar index using `timestamp_ms_to_bar_f64`,
    /// then delegates to `bar_to_x`.  When `bars()` is empty the position is
    /// approximated as `bar_to_x(0.0)`.
    fn ts_to_x_ms(&self, ts_ms: i64) -> f64 {
        let bar = timestamp_ms_to_bar_f64(self.bars(), ts_ms);
        self.bar_to_x(bar)
    }

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

    record_label_text_rect(ctx, text, x, y, rotation);
}

// =============================================================================
// User-text label registry (inline label editing arc, 2026-08-27)
// =============================================================================
//
// Double-click editing of ANY primitive's user text (`PrimitiveData::text`),
// not only the text-first family: a labeled trend line's label must be
// editable exactly where it is drawn. Rather than re-deriving each
// primitive's label geometry a second time in a hit-test, the two text
// helpers above RECORD the axis-aligned bbox of every user-text label they
// paint, tagged with (bid, pane, prim_id). The render loops in
// `mlc-app-layout::render_chart` arm the "current primitive" slot only for
// primitives that carry a non-empty `PrimitiveData::text`, and the record
// fires only when the rendered content EQUALS that text — so system labels
// (Elliott wave tags, Fib level values, SMC auto-labels) never register
// even when they ride the same primitive's render pass.
//
// Rects are in the pane's LOCAL coords (main chart-local / sub-pane
// content-local), the same spaces `DrawingManager::hit_test` works in, and
// live for exactly one frame: the app clears the list when a window's
// primitive pass begins. Single-threaded renderers only (wasm + the native
// shells all render on one thread), hence thread_local, not a Mutex.

use std::cell::{Cell, RefCell};

/// One painted user-text label's hit/anchor rectangle.
#[derive(Clone, Copy, Debug)]
pub struct LabelTextRect {
    pub bid: crate::BubbleId,
    /// None = main chart, Some(instance_id) = sub-pane (mirrors
    /// `PrimitiveData::pane_id`).
    pub pane: Option<u64>,
    pub prim_id: u64,
    /// Axis-aligned bbox (min_x, min_y, max_x, max_y), pane-local coords.
    pub rect: (f64, f64, f64, f64),
}

/// One painted Fib/Gann (or other level-ladder) VALUE label's hit/anchor
/// rectangle. Distinct from [`LabelTextRect`]: those match `data.text`
/// content, these carry the raw stored `level` so a double-click can seed
/// `level_value_label` / commit via `parse_level_value`.
#[derive(Clone, Copy, Debug)]
pub struct LevelLabelRect {
    pub bid: crate::BubbleId,
    pub pane: Option<u64>,
    pub prim_id: u64,
    pub level: f64,
    /// Axis-aligned bbox (min_x, min_y, max_x, max_y), pane-local coords.
    pub rect: (f64, f64, f64, f64),
}

thread_local! {
    static CURRENT_BID: Cell<Option<crate::BubbleId>> = const { Cell::new(None) };
    /// (pane, prim_id, expected content) — set by the primitive render loop.
    static CURRENT_PRIM: RefCell<Option<(Option<u64>, u64, String)>> = const { RefCell::new(None) };
    static LABEL_RECTS: RefCell<Vec<LabelTextRect>> = const { RefCell::new(Vec::new()) };
    static CURRENT_LEVEL: Cell<Option<f64>> = const { Cell::new(None) };
    static LEVEL_RECTS: RefCell<Vec<LevelLabelRect>> = const { RefCell::new(Vec::new()) };
}

/// Arm/clear the bubble tag for the current primitive render pass. Called
/// by the render loops from `dm.render_bid()` once per pass.
pub fn set_label_text_bid(bid: Option<crate::BubbleId>) {
    CURRENT_BID.with(|c| c.set(bid));
}

/// Drop every recorded rect — call when a window's primitive pass begins,
/// before the first `prim.render`.
pub fn clear_label_text_rects() {
    LABEL_RECTS.with(|l| l.borrow_mut().clear());
    LEVEL_RECTS.with(|l| l.borrow_mut().clear());
}

/// Arm the level-value slot for the next [`crate::drawing::primitives_v2`]
/// label plate. Pair with [`clear_current_level_label`] immediately after
/// the plate draws. `None` means "this plate is not a level value".
pub fn set_current_level_label(level: f64) {
    CURRENT_LEVEL.with(|c| c.set(Some(level)));
}

/// Disarm the level-value slot — system / point / user-text plates must
/// not attribute themselves as a level.
pub fn clear_current_level_label() {
    CURRENT_LEVEL.with(|c| c.set(None));
}

/// Arm the "current primitive" slot for a primitive that carries user text
/// (`data.text` non-empty). Pair with [`clear_current_label_text_prim`]
/// immediately after its `render` call.
pub fn set_current_label_text_prim(pane: Option<u64>, prim_id: u64, content: &str) {
    CURRENT_PRIM.with(|c| *c.borrow_mut() = Some((pane, prim_id, content.to_string())));
}

/// Disarm the slot — anything rendered outside an armed primitive (previews,
/// overlays, system passes) must not attribute its text to the last prim.
pub fn clear_current_label_text_prim() {
    CURRENT_PRIM.with(|c| *c.borrow_mut() = None);
}

/// Topmost-first hit over the recorded rects of one (bid, pane), in
/// pane-local coords. `slop` grows each rect on every side.
pub fn find_label_text_at(
    bid: crate::BubbleId,
    pane: Option<u64>,
    x: f64,
    y: f64,
    slop: f64,
) -> Option<LabelTextRect> {
    LABEL_RECTS.with(|l| {
        l.borrow()
            .iter()
            .rev()
            .find(|e| {
                e.bid == bid && e.pane == pane
                    && x >= e.rect.0 - slop && x <= e.rect.2 + slop
                    && y >= e.rect.1 - slop && y <= e.rect.3 + slop
            })
            .copied()
    })
}

/// Fresh rect for one primitive's label (the inline editor re-reads it each
/// frame so its plate follows the label while the chart pans mid-edit).
pub fn label_text_rect_for(
    bid: crate::BubbleId,
    pane: Option<u64>,
    prim_id: u64,
) -> Option<(f64, f64, f64, f64)> {
    LABEL_RECTS.with(|l| {
        l.borrow()
            .iter()
            .rev()
            .find(|e| e.bid == bid && e.pane == pane && e.prim_id == prim_id)
            .map(|e| e.rect)
    })
}

/// Topmost-first hit over recorded Fib/Gann level-value plates of one
/// (bid, pane), in pane-local coords. `slop` grows each rect on every side.
pub fn find_level_label_at(
    bid: crate::BubbleId,
    pane: Option<u64>,
    x: f64,
    y: f64,
    slop: f64,
) -> Option<LevelLabelRect> {
    LEVEL_RECTS.with(|l| {
        l.borrow()
            .iter()
            .rev()
            .find(|e| {
                e.bid == bid && e.pane == pane
                    && x >= e.rect.0 - slop && x <= e.rect.2 + slop
                    && y >= e.rect.1 - slop && y <= e.rect.3 + slop
            })
            .copied()
    })
}

/// Fresh rect for one primitive's level-value plate (the inline editor
/// re-reads it each frame so the plate follows the line while the chart
/// pans mid-edit).
pub fn level_label_rect_for(
    bid: crate::BubbleId,
    pane: Option<u64>,
    prim_id: u64,
    level: f64,
) -> Option<(f64, f64, f64, f64)> {
    LEVEL_RECTS.with(|l| {
        l.borrow()
            .iter()
            .rev()
            .find(|e| {
                e.bid == bid && e.pane == pane && e.prim_id == prim_id
                    && (e.level - level).abs() < 1e-9
            })
            .map(|e| e.rect)
    })
}

/// Record the just-painted level-value plate when CURRENT_LEVEL is armed.
/// Called from `draw_label_plate` after the text is measured so the rect
/// matches the painted plate (including padding).
pub fn record_level_label_rect(
    x: f64,
    y: f64,
    text_width: f64,
    font_size: f64,
    padding: f64,
) {
    let Some(level) = CURRENT_LEVEL.with(|c| c.get()) else { return };
    let armed = CURRENT_PRIM.with(|c| c.borrow().clone());
    let Some((pane, prim_id, _)) = armed else { return };
    let bid = CURRENT_BID.with(|c| c.get());
    let Some(bid) = bid else { return };
    let bg_x = x - text_width / 2.0 - padding;
    let bg_y = y - font_size / 2.0 - padding;
    let bg_w = text_width + padding * 2.0;
    let bg_h = font_size + padding * 2.0;
    LEVEL_RECTS.with(|l| l.borrow_mut().push(LevelLabelRect {
        bid,
        pane,
        prim_id,
        level,
        rect: (bg_x, bg_y, bg_x + bg_w, bg_y + bg_h),
    }));
}

/// Record the just-painted text's bbox when it IS the armed primitive's
/// user text. Runs at the tail of `render_primitive_text_rotated`, after
/// the font is set, so `ctx.measure_text` gives true widths.
fn record_label_text_rect(
    ctx: &dyn RenderContext,
    text: &PrimitiveText,
    x: f64,
    y: f64,
    rotation: f64,
) {
    let armed = CURRENT_PRIM.with(|c| c.borrow().clone());
    let Some((pane, prim_id, expected)) = armed else { return };
    if text.content != expected {
        return;
    }
    let bid = CURRENT_BID.with(|c| c.get());
    let Some(bid) = bid else { return };

    let line_height = text.font_size * 1.2;
    let lines: Vec<&str> = text.content.lines().collect();
    let (cos_r, sin_r) = (rotation.cos(), rotation.sin());
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for (i, line) in lines.iter().enumerate() {
        let w = ctx.measure_text(line);
        // Mirror the align/baseline mapping of the render loop above:
        // each line rotates around its own origin (x, line_y).
        let lx = match text.h_align {
            PrimitiveTextAlign::Start => 0.0,
            PrimitiveTextAlign::Center => -w / 2.0,
            PrimitiveTextAlign::End => -w,
        };
        let (ly0, ly1) = match text.v_align {
            PrimitiveTextAlign::Start => (0.0, line_height),
            PrimitiveTextAlign::Center => (-line_height / 2.0, line_height / 2.0),
            PrimitiveTextAlign::End => (-line_height, 0.0),
        };
        let line_y = y + i as f64 * line_height;
        for (cx, cy) in [(lx, ly0), (lx + w, ly0), (lx + w, ly1), (lx, ly1)] {
            let rx = x + cx * cos_r - cy * sin_r;
            let ry = line_y + cx * sin_r + cy * cos_r;
            min_x = min_x.min(rx);
            min_y = min_y.min(ry);
            max_x = max_x.max(rx);
            max_y = max_y.max(ry);
        }
    }
    if !min_x.is_finite() {
        return;
    }
    LABEL_RECTS.with(|l| l.borrow_mut().push(LabelTextRect {
        bid,
        pane,
        prim_id,
        rect: (min_x, min_y, max_x, max_y),
    }));
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
