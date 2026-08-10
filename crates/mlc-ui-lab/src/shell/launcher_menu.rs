//! The "Modals" launcher — a three-section dropdown listing every
//! connected modal. Mirrors `sim/sim.js`'s own `#launcher` click handler
//! (wave 3), later widened to the full ~121-type registry (wave 5,
//! 2026-08-10) and then narrowed BACK to the 14 approved-design
//! archetypes the same day (concept wiring — `super::object_tree::
//! spawnable_types()` is itself now sourced from `crate::concept::spec::
//! types()`, see that function's own doc): "Object modals" lists exactly
//! those 14, grouped into one sub-section per `PrimitiveKind` with its own
//! header ([`grouped_object_types`]/[`kind_label`]) — clicking a row opens
//! the CONCEPT modal (`crate::concept::render::render_concept_modal`) for
//! that archetype, the same surface the Object Tree's own row gear opens.
//! A NEW third section, "Production modals (current code)"
//! ([`production_section_label`]), lists the SAME 14 types flat (no
//! per-kind grouping — this section exists purely for side-by-side
//! comparison, not for browsing) and opens the PRODUCTION,
//! live-`Primitive`-backed modal (`mlc_app_layout::layout::modals::
//! primitive_settings::render_primitive_settings_modal`, via
//! `crate::open_object_modal_for_type` — unchanged) instead. "Application
//! modals" is unchanged — the 16 `crate::app_modals::AppModalKind`
//! entries, one flat list (there is no per-kind grouping to derive it
//! from — application modals are not registry-driven primitive types).
//! Same "click a row = act on it" behaviour throughout.
//!
//! Own state (`crate::LabState::launcher_open`, a plain bool — there is
//! nothing else to remember, the menu has no scroll or search of its own),
//! own scoped region ([`REGION_ID`], pushed full-screen every frame by
//! `crate::lib`, same convention as `objtree_modal` / `app_modals::
//! REGION_ID`), own response prefix (`"launcher_menu:"`).
//!
//! Both object-modal sections read their row label straight off the
//! Object Tree's own primitives (`tree_objects`, `Primitive::display_name`)
//! rather than a second, hand-maintained name table — `super::object_tree::
//! spawnable_types()` drives the ROW ORDER, one lookup into `tree_objects`
//! supplies the row LABEL. A type missing from `tree_objects` (the owner
//! deleted its only instance) falls back to its raw type id as the label —
//! still correct, just less pretty. Clicking the concept-section row does
//! NOT spawn a missing instance (the concept modal needs no live
//! `Primitive` at all); clicking the production-section row does, exactly
//! as it always has.
//!
//! No scrolling: at 14 + 14 + 16 rows plus per-kind headers the menu still
//! comfortably fits the viewport (it did not, briefly, during the ~121-row
//! wave 5 window — that pressure is gone now that "Object modals" is back
//! to 14). `draw`'s own `y` clamp only repositions the menu's TOP, it does
//! not shrink it, so this would need revisiting if either section grows
//! again.

use mlc_core::drawing::primitives_v2::{Primitive, PrimitiveKind, PrimitiveRegistry};
use mlc_core::i18n::{t_shell, ShellKey};
use mlc_core::render::{RenderContext, TextAlign, TextBaseline};
use mlc_core::theme::RuntimeTheme;
use uzor::input::{InputCoordinator, LayerId, Sense};
use uzor::types::Rect;

use crate::app_modals::AppModalKind;

use super::object_tree::spawnable_types;

/// Scoped-region id this feature's state draws into.
pub const REGION_ID: &str = "launcher_menu";

/// Every kind that has at least one registered member, in `PrimitiveKind`'s
/// own declaration order (`Line, Channel, Shape, Fibonacci, Gann, Pattern,
/// Annotation, Measurement, Trading, Signal`) — a kind with zero members
/// today is simply absent, never an empty header. Rebuilt from
/// `PrimitiveRegistry::global()` metadata on every call rather than cached:
/// this only runs while the menu is open (menu-open-time cost, not a
/// per-frame one).
fn grouped_object_types() -> Vec<(PrimitiveKind, Vec<&'static str>)> {
    const KIND_ORDER: [PrimitiveKind; 10] = [
        PrimitiveKind::Line,
        PrimitiveKind::Channel,
        PrimitiveKind::Shape,
        PrimitiveKind::Fibonacci,
        PrimitiveKind::Gann,
        PrimitiveKind::Pattern,
        PrimitiveKind::Annotation,
        PrimitiveKind::Measurement,
        PrimitiveKind::Trading,
        PrimitiveKind::Signal,
    ];
    let Ok(registry) = PrimitiveRegistry::global().read() else { return Vec::new() };
    KIND_ORDER
        .into_iter()
        .filter_map(|kind| {
            let ids: Vec<&'static str> = spawnable_types()
                .iter()
                .copied()
                .filter(|id| registry.get(id).map(|m| m.kind) == Some(kind))
                .collect();
            if ids.is_empty() { None } else { Some((kind, ids)) }
        })
        .collect()
}

/// User-facing section-header label for one `PrimitiveKind` — via
/// `mlc_core::i18n`, never hardcoded.
fn kind_label(kind: PrimitiveKind) -> &'static str {
    t_shell(match kind {
        PrimitiveKind::Line => ShellKey::CategoryLine,
        PrimitiveKind::Channel => ShellKey::CategoryChannel,
        PrimitiveKind::Shape => ShellKey::CategoryShape,
        PrimitiveKind::Fibonacci => ShellKey::CategoryFibonacci,
        PrimitiveKind::Gann => ShellKey::CategoryGann,
        PrimitiveKind::Pattern => ShellKey::CategoryPattern,
        PrimitiveKind::Annotation => ShellKey::CategoryAnnotation,
        PrimitiveKind::Measurement => ShellKey::CategoryMeasurement,
        PrimitiveKind::Trading => ShellKey::CategoryTrading,
        PrimitiveKind::Signal => ShellKey::CategorySignal,
    })
}

/// Z-order above every modal layer this crate or `mlc-app-layout` can push
/// (colour pickers included, the highest at z=6 inside `mlc-app-layout`'s
/// own `ZLayer`) — below only the lab's own dev bar
/// (`crate::theme::LAB_BAR_Z`), so the launcher stays clickable over an
/// already-open modal but never fights the dev bar's own theme buttons.
const MENU_Z: u32 = 90;

/// Header for the production-comparison section (2026-08-10, concept
/// wiring): `ShellKey::LauncherProductionModals`, like every other label
/// in this menu. It was a bare English literal for one afternoon because
/// the key did not exist yet; the key exists now.
fn production_section_label() -> &'static str {
    t_shell(ShellKey::LauncherProductionModals)
}

const ROW_H: f64 = 24.0;
const HEADER_H: f64 = 20.0;
/// Per-`PrimitiveKind` sub-header height under "Object modals" (wave 5) —
/// shorter than [`HEADER_H`] so it reads as nested under the top-level
/// header rather than a sibling section.
const SUBHEADER_H: f64 = 18.0;
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
/// `"object_prod:{type_id}"` / `"app:{kind_id}"`); the scoped-region
/// mechanism itself prepends `"{REGION_ID}:"` when the parent coordinator
/// flattens this region's responses (same convention `objtree_modal`'s own
/// child coordinator uses — see `crate::lib`'s own doc on that pattern).
/// Prefixing it AGAIN here would double it.
///
/// `"object:{type_id}"` (the concept section) and `"object_prod:{type_id}"`
/// (the production section) are two DISTINCT id namespaces for the SAME 14
/// type ids — `crate::lib`'s own response loop routes the two to
/// `crate::open_concept_type_modal`/`crate::open_object_modal_for_type`
/// respectively.
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

    let object_groups = grouped_object_types();
    let object_rows: usize = object_groups.iter().map(|(_, ids)| ids.len()).sum();
    let production_rows = spawnable_types().len();
    let rows = object_rows + AppModalKind::ALL.len() + production_rows;
    let menu_h =
        HEADER_H * 3.0 + object_groups.len() as f64 * SUBHEADER_H + rows as f64 * ROW_H + PAD * 2.0;
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

    for (kind, type_ids) in &object_groups {
        row_y = draw_kind_header(ctx, x, row_y, kind_label(*kind), rt);
        for type_id in type_ids {
            let label = tree_objects
                .iter()
                .find(|p| p.type_id() == *type_id)
                .map(|p| p.display_name().to_string())
                .unwrap_or_else(|| (*type_id).to_string());
            draw_row(ctx, coord, &layer, rt, x, row_y, type_id, &label, &format!("object:{type_id}"));
            row_y += ROW_H;
        }
    }

    row_y = draw_section_header(ctx, x, row_y, t_shell(ShellKey::LauncherAppModals), rt);
    for kind in AppModalKind::ALL {
        draw_row(ctx, coord, &layer, rt, x, row_y, kind.icon(), kind.label(), &format!("app:{}", kind.id()));
        row_y += ROW_H;
    }

    // Production comparison section (2026-08-10, concept wiring) — the
    // SAME 14 types as "Object modals" above, flat (no per-kind grouping:
    // this section is for reaching a specific type's PRODUCTION modal by
    // name, not for browsing), opening the live-`Primitive`-backed modal
    // instead of the concept one — see this module's own doc.
    row_y = draw_section_header(ctx, x, row_y, production_section_label(), rt);
    for type_id in spawnable_types() {
        let label = tree_objects
            .iter()
            .find(|p| p.type_id() == *type_id)
            .map(|p| p.display_name().to_string())
            .unwrap_or_else(|| (*type_id).to_string());
        draw_row(ctx, coord, &layer, rt, x, row_y, type_id, &label, &format!("object_prod:{type_id}"));
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

/// Nested sub-header for one `PrimitiveKind` group under "Object modals"
/// (wave 5) — same role as [`draw_section_header`], indented and slightly
/// smaller so it reads as a child of the section it sits under rather than
/// a sibling of it.
fn draw_kind_header(ctx: &mut dyn RenderContext, x: f64, y: f64, label: &str, rt: &RuntimeTheme) -> f64 {
    ctx.set_font("10px sans-serif");
    ctx.set_fill_color(&rt.colors.text_secondary);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(label, x + PAD + 14.0, y + SUBHEADER_H / 2.0);
    y + SUBHEADER_H
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
