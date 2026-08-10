//! Right panel rail: watchlist / alerts / object tree / signals / metrics /
//! agent / trading / DOM / tape / L2 / big trades / liquidations / screener
//! — in the exact order `sim/index.html`'s `#right` rail lists them.
//!
//! Only `object_tree` actually opens a panel body in wave 1 (see
//! `super::object_tree`) — the rest just take the "selected" rail
//! highlight, matching the task's scope (rail buttons only; panel bodies
//! other than Object Tree are future waves).

use mlc_core::render::RenderContext;
use mlc_core::theme::RuntimeTheme;
use uzor::input::{InputCoordinator, LayerId};

use super::{icon_btn, ShellRect, GAP};

/// Panel ids, in rail order — also the icon name for every one of them.
pub(super) const PANELS: &[&str] = &[
    "watchlist",
    "alert",
    "object_tree",
    "signal",
    "gauge",
    "bot",
    "trading",
    "slot_dom",
    "slot_tape",
    "slot_l2",
    "slot_big_trades",
    "slot_liq",
    "screener",
];

pub(super) fn draw(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer: &LayerId,
    rt: &RuntimeTheme,
    rect: ShellRect,
    active_panel: Option<&'static str>,
) {
    let (rx, ry, rw, rh) = rect;

    ctx.set_fill_color(&rt.colors.toolbar_bg);
    ctx.fill_rect(rx, ry, rw, rh);
    ctx.set_stroke_color(&rt.colors.toolbar_divider);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(rx, ry);
    ctx.line_to(rx, ry + rh);
    ctx.stroke();

    let btn = rt.sizing.button_height as f64;
    let inset = ((rw - btn) / 2.0).max(0.0);
    let mut y = ry + 6.0;

    for panel in PANELS {
        let id = format!("shell:right:panel:{panel}");
        icon_btn(ctx, coord, layer, rt, &id, panel, (rx + inset, y, btn, btn), active_panel == Some(*panel));
        y += btn + GAP;
    }
}
