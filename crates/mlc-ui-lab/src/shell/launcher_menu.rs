//! The "Modals" launcher — a two-section dropdown listing every connected
//! modal (wave 3). Mirrors `sim/sim.js`'s own `#launcher` click handler:
//! same two sections ("Object modals" over the 14 Object Tree primitive
//! types, "Application modals" over the 16 `crate::app_modals::AppModalKind`
//! entries), same "click a row = act on it" behaviour.
//!
//! Own state (`crate::LabState::launcher_open`, a plain bool — there is
//! nothing else to remember, the menu has no scroll or search of its own),
//! own scoped region ([`REGION_ID`], pushed full-screen every frame by
//! `crate::lib`, same convention as `objtree_modal` / `app_modals::
//! REGION_ID`), own response prefix (`"launcher_menu:"`).
//!
//! Object-modal rows read their label straight off the Object Tree's own
//! primitives (`tree_objects`, `Primitive::display_name`) rather than a
//! second, hand-maintained name table — the 14 canonical types
//! (`super::object_tree::SPAWNABLE_TYPES`) drive the ROW ORDER, one lookup
//! into `tree_objects` supplies the row LABEL. A type missing from
//! `tree_objects` (the owner deleted its only instance) falls back to its
//! raw type id as the label — still correct, just less pretty — and its
//! click spawns a fresh one rather than failing silently.

use mlc_core::drawing::primitives_v2::Primitive;
use mlc_core::i18n::{t_shell, ShellKey};
use mlc_core::render::{RenderContext, TextAlign, TextBaseline};
use mlc_core::theme::RuntimeTheme;
use uzor::input::{InputCoordinator, LayerId, Sense};
use uzor::types::Rect;

use crate::app_modals::AppModalKind;

use super::object_tree::SPAWNABLE_TYPES;

/// Scoped-region id this feature's state draws into.
pub const REGION_ID: &str = "launcher_menu";

/// Z-order above every modal layer this crate or `mlc-app-layout` can push
/// (colour pickers included, the highest at z=6 inside `mlc-app-layout`'s
/// own `ZLayer`) — below only the lab's own dev bar
/// (`crate::theme::LAB_BAR_Z`), so the launcher stays clickable over an
/// already-open modal but never fights the dev bar's own theme buttons.
const MENU_Z: u32 = 90;

const ROW_H: f64 = 24.0;
const HEADER_H: f64 = 20.0;
const MENU_W: f64 = 260.0;
const PAD: f64 = 6.0;
const ICON_SIZE: f64 = 14.0;

/// Draw the dropdown anchored at `anchor` (its own top-right corner —
/// matches where the "Modals" toolbar button sits). Always registers the
/// full-screen outside-click backdrop first, so any row registered after
/// it wins the hit test.
///
/// `coord` is the caller's OWN [`REGION_ID`] child coordinator — every id
/// registered here is UNPREFIXED (`"bg"` / `"object:{type_id}"` /
/// `"app:{kind_id}"`); the scoped-region mechanism itself prepends
/// `"{REGION_ID}:"` when the parent coordinator flattens this region's
/// responses (same convention `objtree_modal`'s own child coordinator
/// uses — see `crate::lib`'s own doc on that pattern). Prefixing it AGAIN
/// here would double it.
pub fn draw(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    rt: &RuntimeTheme,
    anchor: (f64, f64),
    screen_w: f64,
    screen_h: f64,
    tree_objects: &[Box<dyn Primitive>],
) {
    let layer = LayerId::new(REGION_ID);
    coord.push_layer(layer.clone(), MENU_Z, false);

    coord.register_on_layer("bg", Rect::new(0.0, 0.0, screen_w, screen_h), Sense::CLICK, &layer);

    let rows = SPAWNABLE_TYPES.len() + AppModalKind::ALL.len();
    let menu_h = HEADER_H * 2.0 + rows as f64 * ROW_H + PAD * 2.0;
    let (ax, ay) = anchor;
    let x = (ax - MENU_W).max(4.0).min(screen_w - MENU_W - 4.0);
    let y = ay.min(screen_h - menu_h - 4.0).max(4.0);

    let radius = rt.sizing.border_radius as f64;
    ctx.set_fill_color(&rt.colors.dropdown_bg);
    ctx.fill_rounded_rect(x, y, MENU_W, menu_h, radius);
    ctx.set_stroke_color(&rt.colors.toolbar_divider);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(x, y, MENU_W, menu_h, radius);

    let mut row_y = y + PAD;
    row_y = draw_section_header(ctx, x, row_y, t_shell(ShellKey::LauncherObjectModals), rt);

    for type_id in SPAWNABLE_TYPES {
        let label = tree_objects
            .iter()
            .find(|p| p.type_id() == *type_id)
            .map(|p| p.display_name().to_string())
            .unwrap_or_else(|| (*type_id).to_string());
        draw_row(ctx, coord, &layer, rt, x, row_y, type_id, &label, &format!("object:{type_id}"));
        row_y += ROW_H;
    }

    row_y = draw_section_header(ctx, x, row_y, t_shell(ShellKey::LauncherAppModals), rt);
    for kind in AppModalKind::ALL {
        draw_row(ctx, coord, &layer, rt, x, row_y, kind.icon(), kind.label(), &format!("app:{}", kind.id()));
        row_y += ROW_H;
    }

    coord.pop_layer(&layer);
}

fn draw_section_header(ctx: &mut dyn RenderContext, x: f64, y: f64, label: &str, rt: &RuntimeTheme) -> f64 {
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(&rt.colors.text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(label, x + PAD + 4.0, y + HEADER_H / 2.0);
    y + HEADER_H
}

#[allow(clippy::too_many_arguments)]
fn draw_row(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer: &LayerId,
    rt: &RuntimeTheme,
    x: f64,
    y: f64,
    icon_name: &str,
    label: &str,
    id: &str,
) {
    let hovered = coord.is_hovered(&uzor::WidgetId::from(id));
    if hovered {
        ctx.set_fill_color(&rt.colors.button_bg_hover);
        ctx.fill_rect(x + 2.0, y, MENU_W - 4.0, ROW_H);
    }
    let icon_y = y + (ROW_H - ICON_SIZE) / 2.0;
    super::draw_icon(ctx, icon_name, x + PAD + 4.0, icon_y, ICON_SIZE, &rt.colors.text_secondary);
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&rt.colors.text_primary);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(label, x + PAD + 4.0 + ICON_SIZE + 6.0, y + ROW_H / 2.0);
    coord.register_on_layer(id, Rect::new(x, y, MENU_W, ROW_H), Sense::CLICK | Sense::HOVER, layer);
}
