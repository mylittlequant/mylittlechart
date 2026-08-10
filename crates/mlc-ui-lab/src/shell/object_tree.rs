//! Object Tree side panel: header (icon + title + close), instrument-key
//! subheader, and one row per Object Tree primitive — type icon, name, and
//! the five row actions (unlock / eye / alert / gear / close). Mirrors
//! `sim/index.html`'s `#side`/`#tree` panel.

use std::sync::OnceLock;

use mlc_core::drawing::primitives_v2::Primitive;
use mlc_core::i18n::{t_primitive_name, t_toolbar, PrimitiveNameKey, ToolbarTooltipKey};
use mlc_app_layout::ui::scroll_widget::{ScrollableContainer, ScrollState};
use mlc_app_layout::ui::widgets::types::WidgetTheme;
use mlc_core::render::{RenderContext, TextAlign, TextBaseline};
use mlc_core::theme::RuntimeTheme;
use mlc_core::types::TOP_TOOLBAR_HEIGHT;
use uzor::input::{InputCoordinator, LayerId, Sense};
use uzor::types::Rect;

use super::{icon_btn, ShellRect, GAP, STUB_INSTRUMENT_KEY};

/// The 14 approved-design object archetypes
/// (`crate::concept::spec::types()`), in the spec's own order — the Object
/// Tree's own startup roster (`crate::LabState::new` creates one of each,
/// mirroring `sim.js`'s own `spawnAll()`), the launcher menu's own "Object
/// modals" section (`shell::launcher_menu`), and the agent control plane's
/// `objects` list (`crate::lab_modals`/`crate::lab_open_type`).
///
/// Read from `concept::spec::types()` at first call and cached — that data
/// is itself a fixed, process-lifetime `OnceLock`, so re-deriving this
/// mapping every frame would only repeat work, never see something new.
///
/// **2026-08-10, concept wiring**: narrowed from the full registered-type
/// universe (~121 types, every `PrimitiveRegistry` entry) back to the 14
/// APPROVED archetypes, so the Object Tree, the launcher menu and the
/// agent control plane all show exactly what `crate::concept::render`'s
/// concept modal can actually open — nothing wider. This happens to
/// restore the pre-wave-5 relationship with `left_rail::TOOLS` (the two
/// are hand-kept in sync at 14 entries again), but that is a side effect,
/// not the reason: the source of truth is `spec.rs`, not the left rail.
pub(crate) fn spawnable_types() -> &'static [&'static str] {
    static TYPES: OnceLock<Vec<&'static str>> = OnceLock::new();
    TYPES.get_or_init(|| crate::concept::spec::types().iter().map(|t| t.id).collect())
}

/// A row's own display label — the approved design's `name`
/// (`crate::concept::spec::ConceptType::name`), NOT the live `Primitive`'s
/// own `display_name()` (task requirement: the Object Tree row shows the
/// DESIGN's name, not whatever the registered factory happened to title
/// it). Falls back to the raw `type_id` on the never-expected case a
/// spawnable type has no spec entry — [`spawnable_types`] is itself
/// sourced from `spec::types()`, so this only matters if the two ever
/// drift apart.
fn spec_display_name(type_id: &'static str) -> &'static str {
    crate::concept::spec::types().iter().find(|t| t.id == type_id).map(|t| t.name).unwrap_or(type_id)
}

/// A row's own display label, following the UI language like every other
/// name in the app: [`PrimitiveNameKey::from_type_id`] first (the same
/// mapping `mlc_core::i18n::localize_primitive_name` uses), falling back to
/// the spec's own English `name` ([`spec_display_name`]) only for a
/// type_id that has no `PrimitiveNameKey` entry.
pub(crate) fn row_display_name(type_id: &'static str) -> &'static str {
    PrimitiveNameKey::from_type_id(type_id).map(t_primitive_name).unwrap_or_else(|| spec_display_name(type_id))
}

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

/// The list's viewport and content heights, as measured by the last
/// [`draw`]. `ScrollState::handle_wheel` needs both to clamp, and the wheel
/// response is handled outside the draw — this is how the two meet, the same
/// way the settings modal passes its own geometry out to its dispatcher.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ListGeometry {
    pub content_height: f64,
    pub viewport_height: f64,
}

/// Draws the panel and reports the list geometry the wheel handler needs.
#[allow(clippy::too_many_arguments)]
pub(super) fn draw(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer: &LayerId,
    rt: &RuntimeTheme,
    rect: ShellRect,
    objects: &[Box<dyn Primitive>],
    selected: Option<u64>,
    scroll: &mut ScrollState,
) -> ListGeometry {
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
    let viewport_h = (ry + rh - list_top).max(0.0);
    let row_h = rt.sizing.button_height as f64;
    let content_h = objects.len() as f64 * row_h;
    scroll.clamp(content_h, viewport_h);

    // The wheel zone covers the whole list viewport. It is registered BEFORE
    // the rows so a row's own CLICK zone still wins the pointer — same
    // registration-order rule the rows already rely on for their actions.
    coord.register_on_layer(
        "shell:objtree:viewport".to_string(),
        Rect::new(rx, list_top, rw, viewport_h),
        Sense::SCROLL,
        layer,
    );

    // The app's own scroll container: it owns the clip, the content offset and
    // the scrollbar. Hand-drawing a track here would be a second scrollbar in
    // the same crate that already has one.
    let viewport = Rect::new(rx, list_top, rw, viewport_h);
    let container = ScrollableContainer::new(viewport, scroll, None);
    let content_top = container.content_y();
    container.begin(ctx);
    draw_rows(ctx, coord, layer, rt, rx, content_top, rw, objects, selected, (list_top, viewport_h));
    let widget_theme = WidgetTheme::default();
    container.end(ctx, content_h, &widget_theme);

    ListGeometry { content_height: content_h, viewport_height: viewport_h }
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
    viewport: (f64, f64),
) {
    let (view_top, view_h) = viewport;
    let view_bottom = view_top + view_h;
    let row_h = rt.sizing.button_height as f64;
    let action_size = rt.sizing.icon_size as f64;
    let chip_size = rt.sizing.icon_size as f64;
    let pad = 10.0;

    let n_actions = ROW_ACTIONS.len() as f64;
    let actions_w = n_actions * action_size + (n_actions - 1.0) * GAP;
    let actions_x0 = rx + rw - pad - actions_w;

    let mut y = top_y;
    for prim in objects {
        // A row scrolled out of the viewport is neither drawn nor registered:
        // registering it would leave a clickable ghost under the toolbars.
        if y + row_h <= view_top || y >= view_bottom {
            y += row_h;
            continue;
        }
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
        ctx.fill_text(row_display_name(prim.type_id()), name_x, y + row_h / 2.0);
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
