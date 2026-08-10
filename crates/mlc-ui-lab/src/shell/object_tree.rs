//! Object Tree side panel: header (icon + title + close), instrument-key
//! subheader, and one row per Object Tree primitive — type icon, name, and
//! the five row actions (unlock / eye / alert / gear / close). Mirrors
//! `sim/index.html`'s `#side`/`#tree` panel.

use mlc_core::drawing::primitives_v2::Primitive;
use mlc_core::i18n::{t_toolbar, ToolbarTooltipKey};
use mlc_core::render::{RenderContext, TextAlign, TextBaseline};
use mlc_core::theme::RuntimeTheme;
use mlc_core::types::TOP_TOOLBAR_HEIGHT;
use uzor::input::{InputCoordinator, LayerId, Sense};
use uzor::types::Rect;

use super::{icon_btn, ShellRect, GAP, STUB_INSTRUMENT_KEY};

/// Primitive type ids this lab's left rail can spawn, in rail order — also
/// the Object Tree's own startup roster (`crate::LabState::new` creates one
/// of each, mirroring `sim.js`'s own `spawnAll()`). Real ids read off
/// `PrimitiveRegistry::global()`'s own `metadata().type_id` for each type
/// (verified, not invented — every one of these matches
/// `left_rail::TOOLS`'s own spelling exactly).
pub(crate) const SPAWNABLE_TYPES: &[&str] = &[
    "fib_retracement",
    "gann_fan",
    "pitchfork",
    "elliott_impulse",
    "xabcd_pattern",
    "long_position",
    "price_date_range",
    "callout",
    "table",
    "rectangle",
    "parallel_channel",
    "regression_trend",
    "brush",
    "flag",
];

/// One row's five action buttons: `(action id, icon name)`, left-to-right —
/// mirrors `sim.js`'s own `renderTree()` action list exactly
/// (`[['lock','unlock'],['eye','eye'],['alert','alert'],['gear','settings'],['del','close']]`).
/// Only `gear` (opens the settings modal) and `del` (removes the row) do
/// anything — `unlock`/`eye`/`alert` are registered for the same reason
/// `sim.js`'s own rows leave them unwired: no behaviour to mirror yet.
const ROW_ACTIONS: [(&str, &str); 5] = [
    ("unlock", "unlock"),
    ("eye", "eye"),
    ("alert", "alert"),
    ("gear", "settings"),
    ("del", "close"),
];

#[allow(clippy::too_many_arguments)]
pub(super) fn draw(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer: &LayerId,
    rt: &RuntimeTheme,
    rect: ShellRect,
    objects: &[Box<dyn Primitive>],
    selected: Option<u64>,
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

    // Header row.
    let header_h = TOP_TOOLBAR_HEIGHT.min(rh);
    ctx.set_stroke_color(&rt.colors.toolbar_divider);
    ctx.begin_path();
    ctx.move_to(rx, ry + header_h);
    ctx.line_to(rx + rw, ry + header_h);
    ctx.stroke();

    let icon_size = rt.sizing.icon_size as f64;
    let pad = 10.0;
    let icon_y = ry + (header_h - icon_size) / 2.0;
    super::draw_icon(ctx, "object_tree", rx + pad, icon_y, icon_size, &rt.colors.text_secondary);

    ctx.set_font(&format!("{}px {}", rt.fonts.size_normal as i32, rt.fonts.family));
    ctx.set_fill_color(&rt.colors.text_primary);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(
        t_toolbar(ToolbarTooltipKey::ObjectTree),
        rx + pad + icon_size + 8.0,
        ry + header_h / 2.0,
    );

    let close_size = rt.sizing.button_height as f64;
    let close_x = rx + rw - pad - close_size;
    let close_y = ry + (header_h - close_size) / 2.0;
    icon_btn(ctx, coord, layer, rt, "shell:objtree:close", "close", (close_x, close_y, close_size, close_size), false);

    // Instrument-key subheader.
    ctx.set_font(&format!("{}px {}", rt.fonts.size_small as i32, rt.fonts.family));
    ctx.set_fill_color(&rt.colors.text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(STUB_INSTRUMENT_KEY, rx + pad, ry + header_h + 8.0);

    // Object list — one row per `objects` entry, in creation order (matches
    // `sim.js`'s own `state.objects` array order: newest spawns append).
    let list_top = ry + header_h + rt.sizing.button_height as f64;
    draw_rows(ctx, coord, layer, rt, rx, list_top, rw, objects, selected);
}

#[allow(clippy::too_many_arguments)]
fn draw_rows(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer: &LayerId,
    rt: &RuntimeTheme,
    rx: f64,
    top_y: f64,
    rw: f64,
    objects: &[Box<dyn Primitive>],
    selected: Option<u64>,
) {
    let row_h = rt.sizing.button_height as f64;
    let action_size = rt.sizing.icon_size as f64;
    let chip_size = rt.sizing.icon_size as f64;
    let pad = 10.0;

    let n_actions = ROW_ACTIONS.len() as f64;
    let actions_w = n_actions * action_size + (n_actions - 1.0) * GAP;
    let actions_x0 = rx + rw - pad - actions_w;

    let mut y = top_y;
    for prim in objects {
        let id = prim.data().id;
        let is_selected = selected == Some(id);

        if is_selected {
            ctx.set_fill_color(&rt.colors.button_bg_active);
            ctx.fill_rect(rx, y, rw, row_h);
        }

        // Row-select hit zone, registered FIRST: the action icons
        // (registered below, after) resolve on top of it at their own
        // rects — `InputCoordinator` resolves overlapping widgets on one
        // layer by registration order, last registered wins (see
        // `hit_test_at_filtered`'s own doc in `uzor`).
        coord.register_on_layer(
            format!("shell:objtree:row:{id}"),
            Rect::new(rx, y, rw, row_h),
            Sense::CLICK,
            layer,
        );

        let text_color = if is_selected { rt.colors.text_active.as_str() } else { rt.colors.text_primary.as_str() };
        let chip_x = rx + pad;
        let chip_y = y + (row_h - chip_size) / 2.0;
        super::draw_icon(ctx, prim.type_id(), chip_x, chip_y, chip_size, text_color);

        let name_x = chip_x + chip_size + 8.0;
        let name_w = (actions_x0 - GAP - name_x).max(0.0);
        ctx.set_font(&format!("{}px {}", rt.fonts.size_normal as i32, rt.fonts.family));
        ctx.set_fill_color(text_color);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.save();
        ctx.begin_path();
        ctx.rect(name_x, y, name_w, row_h);
        ctx.clip();
        ctx.fill_text(prim.display_name(), name_x, y + row_h / 2.0);
        ctx.restore();

        let mut ax = actions_x0;
        for (action, icon) in ROW_ACTIONS {
            let ay = y + (row_h - action_size) / 2.0;
            super::draw_icon(ctx, icon, ax, ay, action_size, &rt.colors.text_secondary);
            coord.register_on_layer(
                format!("shell:objtree:act:{action}:{id}"),
                Rect::new(ax, ay, action_size, action_size),
                Sense::CLICK,
                layer,
            );
            ax += action_size + GAP;
        }

        y += row_h;
    }
}
