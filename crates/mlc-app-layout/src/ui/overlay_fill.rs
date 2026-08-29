//! Overlay chrome fill (modals / dropdowns / popups).
//!
//! Alpha goes through `globalAlpha`, not `fillStyle`. Do not call
//! `draw_blur_background` from here: on wasm that helper is also invoked
//! for the chart pane, scales and toolbars.

use uzor::render::RenderContext;

pub fn fill_overlay_rounded(
    ctx: &mut (impl RenderContext + ?Sized),
    color: &str,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    radius: f64,
) {
    let (r, g, b, a) = mlc_core::parse_css_color(color);
    let rgb = format!("#{:02X}{:02X}{:02X}", r, g, b);
    let alpha = a as f64 / 255.0;
    if alpha >= 0.999 {
        ctx.set_fill_color(&rgb);
        if radius > 0.0 {
            ctx.fill_rounded_rect(x, y, w, h, radius);
        } else {
            ctx.fill_rect(x, y, w, h);
        }
        return;
    }
    ctx.save();
    if radius > 0.0 {
        ctx.begin_path();
        ctx.rounded_rect(x, y, w, h, radius);
        ctx.clip();
    }
    ctx.blur_overlay_backdrop(x, y, w, h);
    ctx.set_fill_color(&rgb);
    ctx.set_global_alpha(alpha);
    ctx.fill_rect(x, y, w, h);
    ctx.reset_alpha();
    ctx.restore();
}

/// True when `color` carries an alpha below opaque — Glass overlay fills.
pub fn color_is_translucent(color: &str) -> bool {
    mlc_core::parse_css_color(color).3 < 250
}

/// Control chrome inside a modal. Glass uses the overlay fill; Solid keeps
/// the original opaque colour so the settings surface does not shift.
pub fn fill_modal_control(
    ctx: &mut (impl RenderContext + ?Sized),
    glass_color: &str,
    solid_color: &str,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    radius: f64,
) {
    if color_is_translucent(glass_color) {
        fill_overlay_rounded(ctx, glass_color, x, y, w, h, radius);
    } else {
        ctx.set_fill_color(solid_color);
        if radius > 0.0 {
            ctx.fill_rounded_rect(x, y, w, h, radius);
        } else {
            ctx.fill_rect(x, y, w, h);
        }
    }
}

/// Checkbox / on-off chip fill. Checked uses the (already styled) active
/// colour; unchecked stays the Solid off-colour unless Glass is on.
pub fn fill_checkbox(
    ctx: &mut (impl RenderContext + ?Sized),
    checked: bool,
    on_color: &str,
    dropdown_bg: &str,
    solid_off: &str,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    radius: f64,
) {
    if checked {
        fill_overlay_rounded(ctx, on_color, x, y, w, h, radius);
    } else {
        fill_modal_control(ctx, dropdown_bg, solid_off, x, y, w, h, radius);
    }
}
