//! Bottom toolbar: selected-object name (always "Nothing selected" in
//! wave 1 — no primitives exist), a fixed row of object-editing icon
//! buttons, and a right-aligned expand button + live clock. Composition
//! mirrors `sim/index.html`'s `#bot` toolbar exactly.

use mlc_core::i18n::{t_shell, ShellKey};
use mlc_core::render::{RenderContext, TextAlign, TextBaseline};
use mlc_core::theme::RuntimeTheme;
use uzor::input::{InputCoordinator, LayerId, Sense};
use uzor::types::Rect;

use super::{flow_btn, ShellRect, GAP};

/// Widget id of the clock text — click opens `AppModalKind::Clock`
/// (wave 3; `crate::lib`'s response loop special-cases this id, same as
/// every other shell button that owns an application modal).
pub const CLOCK_ID: &str = "shell:bottom:clock";

pub(super) fn draw(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer: &LayerId,
    rt: &RuntimeTheme,
    rect: ShellRect,
    selected_name: Option<&str>,
) {
    let (rx, ry, rw, rh) = rect;

    ctx.set_fill_color(&rt.colors.toolbar_bg);
    ctx.fill_rect(rx, ry, rw, rh);
    ctx.set_stroke_color(&rt.colors.toolbar_divider);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(rx, ry);
    ctx.line_to(rx + rw, ry);
    ctx.stroke();

    let pad = rt.sizing.button_padding_x as f64;
    let btn_h = rt.sizing.button_height as f64;
    let by = ry + (rh - btn_h) / 2.0;
    let mut x = rx + pad;

    // Selected-object name — plain label, no hit zone. Object Tree's own
    // selection drives this (see `shell::draw_shell`'s own `selected_name`
    // computation); falls back to the "nothing selected" copy wave 1 shipped.
    let name = selected_name.unwrap_or_else(|| t_shell(ShellKey::NothingSelected));
    ctx.set_font(&format!("{}px {}", rt.fonts.size_normal as i32, rt.fonts.family));
    ctx.set_fill_color(&rt.colors.text_primary);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(name, x, ry + rh / 2.0);
    x += ctx.measure_text(name) + pad * 2.0 + GAP;

    for (id, icon) in [
        ("shell:bottom:settings", "settings"),
        ("shell:bottom:fill", "color_fill"),
        ("shell:bottom:textcolor", "text_color"),
        ("shell:bottom:width", "line_width_2"),
        ("shell:bottom:style", "line_solid"),
        ("shell:bottom:alert", "alert"),
        ("shell:bottom:lock", "unlock"),
        ("shell:bottom:delete", "delete"),
    ] {
        x += flow_btn(ctx, coord, layer, rt, id, icon, None, x, by, btn_h, false) + GAP;
    }

    // Right side: clock first (fixed width, right-aligned), then the
    // expand button flowing left from it.
    let now = js_sys::Date::new_0();
    let clock_text = format!(
        "[UTC+0] {:02}:{:02}:{:02}",
        now.get_utc_hours(),
        now.get_utc_minutes(),
        now.get_utc_seconds(),
    );
    ctx.set_font(&format!("{}px {}", rt.fonts.size_normal as i32, rt.fonts.family));
    let clock_w = ctx.measure_text(&clock_text);
    let clock_x = rx + rw - pad - clock_w;
    ctx.set_fill_color(&rt.colors.text_secondary);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(&clock_text, clock_x, ry + rh / 2.0);
    coord.register_on_layer(
        CLOCK_ID,
        Rect::new(clock_x - 4.0, ry, clock_w + 8.0, rh),
        Sense::CLICK,
        layer,
    );

    let expand_x = clock_x - GAP - btn_h;
    flow_btn(ctx, coord, layer, rt, "shell:bottom:expand", "expand", None, expand_x, by, btn_h, false);
}
