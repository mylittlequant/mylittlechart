//! Left drawing-tool rail: crosshair plus 19 drawing tools, in the exact
//! order `sim/index.html`'s `#left` rail lists them. Selection is
//! presentation-only in wave 1 (no primitive is created by picking a tool).

use mlc_core::render::RenderContext;
use mlc_core::theme::RuntimeTheme;
use uzor::input::{InputCoordinator, LayerId};

use super::{icon_btn, ShellRect, GAP};

/// Tool ids, in rail order. Also the icon name for every one of them
/// (`mlc_core::state::icons::icon_svg` resolves each by this exact string).
pub(super) const TOOLS: &[&str] = &[
    "crosshair",
    "trend_line",
    "fib_retracement",
    "gann_fan",
    "pitchfork",
    "elliott_impulse",
    "xabcd_pattern",
    "long_position",
    "price_date_range",
    "rectangle",
    "parallel_channel",
    "regression_trend",
    "callout",
    "table",
    "brush",
    "flag",
    "magnet",
    "lock",
    "eye",
    "delete",
];

pub(super) fn draw(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer: &LayerId,
    rt: &RuntimeTheme,
    rect: ShellRect,
    active_tool: &str,
) {
    let (rx, ry, rw, rh) = rect;

    ctx.set_fill_color(&rt.colors.toolbar_bg);
    ctx.fill_rect(rx, ry, rw, rh);
    ctx.set_stroke_color(&rt.colors.toolbar_divider);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(rx + rw, ry);
    ctx.line_to(rx + rw, ry + rh);
    ctx.stroke();

    let btn = rt.sizing.button_height as f64;
    let inset = ((rw - btn) / 2.0).max(0.0);
    let mut y = ry + 6.0;

    for tool in TOOLS {
        let id = format!("shell:left:tool:{tool}");
        icon_btn(ctx, coord, layer, rt, &id, tool, (rx + inset, y, btn, btn), *tool == active_tool);
        y += btn + GAP;
    }
}
