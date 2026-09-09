//! Stub application shell: top/left/right/bottom toolbars, an empty chart
//! pane, and the Object Tree side panel — no chart engine, no data, no
//! primitives on the canvas. This is the frame the rest of the lab's
//! surfaces (the primitive-settings modal comparison grid, for now) draw
//! on top of, wave 1 of the raw-lab shell effort (see `nemo/CLAUDE.md`
//! carousel notes / the launching agent's brief for the full wave plan).
//!
//! Composition mirrors `sim/index.html` (the owner-approved JS simulator)
//! button-for-button — see each submodule's own doc for its slice.
//! Geometry comes from `mlc_core::types` layout constants; colour/font/sizing
//! from `theme_manager.current()` (a `RuntimeTheme`, same shape as
//! `UITheme::dark()`) — no hand-picked hex or literal px anywhere in this
//! module tree.
//!
//! Wave 1 scope: draw the frame, register every button's hit zone, and wire
//! the two pieces of state simple enough to belong in a stub (which drawing
//! tool / which sidebar panel is "selected", purely presentational — no
//! primitive is actually created, no panel actually renders content beyond
//! the Object Tree's empty shell). Real dispatch (opening menus, switching
//! symbol/timeframe, undo/redo, template management, panel bodies other
//! than Object Tree) is out of scope — see the crate's `README.md` /
//! the wave plan for what lands in waves 2-4.

pub(crate) mod bottom_bar;
mod left_rail;
pub(crate) mod launcher_menu;
pub(crate) mod object_tree;
mod pane;
mod right_rail;
mod top_bar;

use mlc_app_layout::ui::scroll_widget::ScrollState;
use mlc_core::drawing::primitives_v2::Primitive;
use mlc_core::render::RenderContext;
use mlc_core::state::icons::icon_svg;
use mlc_core::theme::RuntimeTheme;
use mlc_core::types::{
    BOTTOM_TOOLBAR_HEIGHT, LEFT_TOOLBAR_WIDTH, RIGHT_SIDEBAR_WIDTH, RIGHT_TOOLBAR_WIDTH,
    TOP_TOOLBAR_HEIGHT,
};
use uzor::input::{InputCoordinator, LayerId, Sense};
use uzor::render::{draw_svg_icon, draw_svg_multicolor};
use uzor::types::Rect;

/// Brand mark, embedded the same way `mlc-app-layout`'s own toolbar
/// renderer does for the "Bot" agent-panel icon (`toolbar_core.rs`) — same
/// relative depth (`crates/<this crate>/src/shell/` vs
/// `crates/mlc-app-layout/src/ui/`), same asset (owner ruling 2026-09-10:
/// the bull is retired, the magic hat replaces it everywhere). This lab
/// has no tab lifecycle of its own, so unlike the real toolbar it always
/// draws the hat at rest — no animation here.
const MAGIC_HAT_SVG: &str = include_str!("../../../../assets/mascot/magic_hat.svg");

/// Absolute-coordinate rect: `(x, y, width, height)`. Every `draw` fn in
/// this module tree takes/returns geometry in this shape rather than a
/// dedicated struct — there is no shared layout engine here (wave 1 is a
/// hand-laid-out stub), just plain tuples passed down from `draw_shell`.
pub(crate) type ShellRect = (f64, f64, f64, f64);

/// Horizontal gap between adjacent toolbar items, and vertical gap between
/// stacked rail buttons. Not a theme token — `RuntimeSizing` has no
/// "item spacing" field of its own (mirrors `mlc-app-layout`'s own
/// `ToolbarConfig::spacing` default of a few px).
const GAP: f64 = 4.0;

/// Placeholder instrument label shown by the top toolbar's symbol button —
/// STUB DATA (this lab has no data plane), not translatable UI copy, so it
/// is a plain literal like every other fake symbol/price this crate draws.
pub(crate) const STUB_SYMBOL_LABEL: &str = "BTCUSDT · BINANCE · S";
/// Placeholder timeframe label shown by the top toolbar's timeframe button.
pub(crate) const STUB_TIMEFRAME_LABEL: &str = "1H";
/// Placeholder chart legend text (top-left of the empty pane) — same
/// instrument as [`STUB_SYMBOL_LABEL`], different field order (matches
/// `sim/index.html`'s own `#legend` text exactly).
pub(crate) const STUB_LEGEND_LABEL: &str = "BTCUSDT · 1H · BINANCE · S";
/// Placeholder instrument key shown under the Object Tree panel's header.
pub(crate) const STUB_INSTRUMENT_KEY: &str = "BTCUSDT:BINANCE:S";

/// Widget id of the top toolbar's "Modals" button.
///
/// Wave 1 wired this to toggle the lab's own variant-comparison overlay
/// (`crate::variants`) — a placeholder, since nothing else in the shell
/// could be called a "modal" yet. Wave 3 gives the button its real,
/// named job: it opens `crate::shell::launcher_menu`, the two-section
/// dropdown listing every connected modal (mirrors `sim/sim.js`'s own
/// `#launcher`). The variant-comparison toggle moved to the lab's own dev
/// bar (`crate::theme::draw_lab_bar`'s "Design compare" button) rather
/// than being dropped — it is still reachable, just relocated off a button
/// whose name it was borrowing.
///
/// `crate::lib`'s response loop special-cases this id (the flag it flips,
/// `LabState::launcher_open`, lives on the lab harness, not on
/// [`ShellState`] — see that field's own doc for why).
pub const MODALS_TOGGLE_ID: &str = "shell:top:modals";

/// Everything the shell frame remembers between frames: which drawing tool
/// reads as selected on the left rail, and which panel reads as selected
/// (and, for `object_tree` specifically, actually open) on the right rail.
/// Both are presentation-only in wave 1 — no primitive is created by
/// selecting a tool, no panel other than Object Tree renders a body.
#[derive(Debug, Clone)]
pub struct ShellState {
    /// Left-rail drawing-tool id, one of [`left_rail::TOOLS`].
    pub active_tool: &'static str,
    /// Right-rail panel id, one of [`right_rail::PANELS`], or `None` if no
    /// rail button reads as selected. The Object Tree side panel is visible
    /// exactly when this is `Some("object_tree")`.
    pub active_right_panel: Option<&'static str>,
    /// Object Tree list scroll — the app's own `ScrollState`, the same type
    /// the settings modal's list uses, so wheel feel and clamping are one
    /// implementation rather than two. [`object_tree::draw`] clamps it against
    /// the live content/viewport heights and reports them back for the wheel
    /// handler.
    pub tree_scroll: ScrollState,
    /// Last measured list geometry, for the wheel response.
    pub tree_list: object_tree::ListGeometry,
}

impl Default for ShellState {
    fn default() -> Self {
        // Matches `sim/index.html`'s own initial state: the crosshair tool
        // and the Object Tree panel both carry the `.on` class from load.
        Self {
            active_tool: "crosshair",
            active_right_panel: Some("object_tree"),
            tree_scroll: ScrollState::default(),
            tree_list: object_tree::ListGeometry::default(),
        }
    }
}

/// The chart pane's own rect — top/left/right/bottom toolbar chrome and
/// (when open) the Object Tree sidebar all excluded. Pure geometry, no
/// drawing or hit-zone registration: [`draw_shell`] calls this for its own
/// return value, and `crate::lib`'s frame setup calls it a SECOND time,
/// before `draw_shell` itself runs, to size the `objtree_modal`/
/// `app_modal` scoped regions — which must exist, at their FINAL rect,
/// before `InputCoordinator::begin_frame` propagates this frame's pointer
/// position into them (same requirement `sync_sector_regions`'s own doc
/// states for the four design-compare sectors). Single source of truth
/// for this geometry so the two call sites can never drift apart.
///
/// `origin_y`/`width`/`height` are the same "area available to the shell"
/// [`draw_shell`] takes (the caller's own chrome, e.g. the lab's dev bar,
/// already excluded).
pub fn chart_pane_rect(origin_y: f64, width: f64, height: f64, sidebar_open: bool) -> uzor::types::Rect {
    let top_h = TOP_TOOLBAR_HEIGHT;
    let bottom_h = BOTTOM_TOOLBAR_HEIGHT;
    let left_w = LEFT_TOOLBAR_WIDTH;
    let right_w = RIGHT_TOOLBAR_WIDTH;
    let sidebar_w = if sidebar_open { RIGHT_SIDEBAR_WIDTH } else { 0.0 };

    let middle_y = origin_y + top_h;
    let middle_h = (height - top_h - bottom_h).max(0.0);
    let chart_w = (width - left_w - right_w - sidebar_w).max(0.0);

    uzor::types::Rect::new(left_w, middle_y, chart_w, middle_h)
}

/// Draw the whole shell frame — top/left/right/bottom toolbars, the empty
/// chart pane, and (when open) the Object Tree side panel — and register
/// every button's hit zone on `coord`. `origin_y`/`width`/`height` describe
/// the area available to the shell (the caller reserves its own chrome,
/// e.g. the lab's theme-preset dev bar, above `origin_y`).
///
/// `modals_active` drives only the top toolbar's own "Modals" button
/// highlight — see [`MODALS_TOGGLE_ID`]'s doc for why that flag lives on
/// the lab harness rather than on [`ShellState`].
///
/// Returns the chart pane's own rect (wave 3: the indicator-overlay
/// toggle — `crate::app_modals::AppModalsState::draw_indicator_overlay` —
/// anchors off it, wave 4: both modal families size/centre themselves
/// inside it too — the same rect the real app's chart canvas would sit
/// in) — [`chart_pane_rect`] computes it, here, rather than every caller
/// re-deriving it.
#[allow(clippy::too_many_arguments)]
pub fn draw_shell(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    state: &mut ShellState,
    tree_objects: &[Box<dyn Primitive>],
    tree_selected: Option<u64>,
    rt: &RuntimeTheme,
    origin_y: f64,
    width: f64,
    height: f64,
    modals_active: bool,
) -> uzor::types::Rect {
    let layer = LayerId::new("shell");
    // `chrome = true`: the shell's own buttons (in particular the "Modals"
    // toggle, which must stay reachable to CLOSE the overlay it opens)
    // remain hit-testable even while the variant-comparison overlay draws
    // its own modal layer on top — see `push_layer_ex`'s own doc in uzor.
    coord.push_layer_ex(layer.clone(), 1, false, true);

    let top_h = TOP_TOOLBAR_HEIGHT;
    let bottom_h = BOTTOM_TOOLBAR_HEIGHT;
    let left_w = LEFT_TOOLBAR_WIDTH;
    let right_w = RIGHT_TOOLBAR_WIDTH;
    let sidebar_open = state.active_right_panel == Some("object_tree");
    let sidebar_w = if sidebar_open { RIGHT_SIDEBAR_WIDTH } else { 0.0 };

    let pane_rect = chart_pane_rect(origin_y, width, height, sidebar_open);
    let middle_y = pane_rect.y;
    let middle_h = pane_rect.height;
    let chart_w = pane_rect.width;
    let bottom_y = origin_y + height - bottom_h;

    // Selected object's display name — drives the bottom toolbar's label
    // and the chart pane's schematic placeholder (tasks 5/ this wave).
    // Through the SAME translated name the tree row uses
    // (`object_tree::row_display_name`), not `Primitive::display_name()`:
    // that one is the type's own English name, so a Russian UI showed
    // "Веер Ганна" in the tree and "Gann Fan" in the status bar for the
    // same object.
    let selected_name: Option<&str> = tree_selected
        .and_then(|id| tree_objects.iter().find(|p| p.data().id == id))
        .map(|p| object_tree::row_display_name(p.type_id()));

    top_bar::draw(ctx, coord, &layer, rt, (0.0, origin_y, width, top_h), modals_active);
    left_rail::draw(ctx, coord, &layer, rt, (0.0, middle_y, left_w, middle_h), state.active_tool);
    pane::draw(ctx, rt, (left_w, middle_y, chart_w, middle_h), selected_name);
    if sidebar_open {
        state.tree_list = object_tree::draw(
            ctx, coord, &layer, rt,
            (left_w + chart_w, middle_y, sidebar_w, middle_h),
            tree_objects, tree_selected, &mut state.tree_scroll,
        );
    }
    right_rail::draw(
        ctx, coord, &layer, rt,
        (left_w + chart_w + sidebar_w, middle_y, right_w, middle_h),
        state.active_right_panel,
    );
    bottom_bar::draw(ctx, coord, &layer, rt, (0.0, bottom_y, width, bottom_h), selected_name);

    pane_rect
}

/// Route one clicked widget id from `coord.end_frame()` into [`ShellState`].
/// Returns `true` if `id` belonged to the shell and was handled — the
/// [`MODALS_TOGGLE_ID`] button is deliberately NOT handled here (see its
/// own doc); the caller checks that id first.
pub fn apply_shell_response(id: &str, state: &mut ShellState) -> bool {
    if let Some(tool) = id.strip_prefix("shell:left:tool:") {
        if let Some(found) = left_rail::TOOLS.iter().find(|&&t| t == tool) {
            state.active_tool = found;
            return true;
        }
    }
    if let Some(panel) = id.strip_prefix("shell:right:panel:") {
        if let Some(found) = right_rail::PANELS.iter().find(|&&t| t == panel) {
            state.active_right_panel =
                if state.active_right_panel == Some(*found) { None } else { Some(*found) };
            return true;
        }
    }
    if id == "shell:objtree:close" {
        state.active_right_panel = None;
        return true;
    }
    false
}

// =============================================================================
// Shared drawing helpers
// =============================================================================

/// Resolve and paint one icon at `(x, y, size, size)`. Special-cases the
/// agent mascot exactly like `mlc-app-layout::ui::toolbar_core::render_icon`
/// does (`draw_svg_multicolor`, no flat `color` — the mascot carries its own
/// palette) so the "bot" right-rail panel button matches the real app. Falls
/// back to [`draw_icon_placeholder`] for any `name` `icon_svg` cannot
/// resolve (2026-08-10, wave 5 — with the Object Tree and launcher menu now
/// listing every registered primitive type, not the curated 14, a row can
/// no longer assume its own type_id always resolves an icon).
fn draw_icon(ctx: &mut dyn RenderContext, name: &str, x: f64, y: f64, size: f64, color: &str) {
    if name == "bot" {
        draw_svg_multicolor(ctx, MAGIC_HAT_SVG, x, y, size, size);
        return;
    }
    match icon_svg(name) {
        Some(svg) => draw_svg_icon(ctx, svg, x, y, size, size, color),
        None => draw_icon_placeholder(ctx, x, y, size, color),
    }
}

/// Neutral placeholder for a `name` `icon_svg` cannot resolve — a small
/// centred dot, so a menu/tree row for a type with no dedicated glyph still
/// reads as "a row with an icon slot", not a silent blank. One fallback
/// shared by every unresolved name — never a per-type guess at what the
/// glyph should have been.
fn draw_icon_placeholder(ctx: &mut dyn RenderContext, x: f64, y: f64, size: f64, color: &str) {
    ctx.set_fill_color(color);
    ctx.begin_path();
    let r = (size * 0.18).max(1.5);
    ctx.arc(x + size / 2.0, y + size / 2.0, r, 0.0, std::f64::consts::TAU);
    ctx.fill();
}

/// Draw one square icon-only button (rail buttons, icon-only toolbar
/// buttons) and register its hit zone. `rect` is `(x, y, w, h)`; `w`/`h`
/// are expected equal (a square button) but nothing here requires it.
#[allow(clippy::too_many_arguments)]
pub(crate) fn icon_btn(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer: &LayerId,
    rt: &RuntimeTheme,
    id: &str,
    icon_name: &str,
    rect: ShellRect,
    active: bool,
) {
    let (x, y, w, h) = rect;
    let radius = rt.sizing.border_radius as f64;
    if active {
        ctx.set_fill_color(&rt.colors.button_bg_active);
        ctx.fill_rounded_rect(x, y, w, h, radius);
    }
    let color = if active { rt.colors.text_active.as_str() } else { rt.colors.text_secondary.as_str() };
    let icon_size = rt.sizing.icon_size as f64;
    let icon_x = x + (w - icon_size) / 2.0;
    let icon_y = y + (h - icon_size) / 2.0;
    draw_icon(ctx, icon_name, icon_x, icon_y, icon_size, color);
    coord.register_on_layer(id, Rect::new(x, y, w, h), Sense::CLICK, layer);
}

/// Draw one icon(+optional text) button flowing left-to-right from
/// `(x, y)` at fixed `height`, register its hit zone, and return the width
/// it occupied — the caller advances its own cursor by that width (+ its
/// own gap). Used by the top and bottom toolbars for both icon-only and
/// icon+label buttons (symbol/timeframe/"Modals").
#[allow(clippy::too_many_arguments)]
pub(crate) fn flow_btn(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer: &LayerId,
    rt: &RuntimeTheme,
    id: &str,
    icon_name: &str,
    text: Option<&str>,
    x: f64,
    y: f64,
    height: f64,
    active: bool,
) -> f64 {
    let pad_x = rt.sizing.button_padding_x as f64;
    let icon_size = rt.sizing.icon_size as f64;
    let radius = rt.sizing.border_radius as f64;

    let text_w = text.map(|t| {
        ctx.set_font(&format!("{}px {}", rt.fonts.size_normal as i32, rt.fonts.family));
        ctx.measure_text(t)
    });
    let width = match text_w {
        Some(tw) => pad_x + icon_size + 6.0 + tw + pad_x,
        None => height,
    };

    if active {
        ctx.set_fill_color(&rt.colors.button_bg_active);
        ctx.fill_rounded_rect(x, y, width, height, radius);
    }
    let color = if active { rt.colors.text_active.as_str() } else { rt.colors.text_secondary.as_str() };

    let icon_x = if text.is_some() { x + pad_x } else { x + (width - icon_size) / 2.0 };
    let icon_y = y + (height - icon_size) / 2.0;
    draw_icon(ctx, icon_name, icon_x, icon_y, icon_size, color);

    if let Some(t) = text {
        ctx.set_font(&format!("{}px {}", rt.fonts.size_normal as i32, rt.fonts.family));
        ctx.set_fill_color(color);
        ctx.set_text_align(mlc_core::render::TextAlign::Left);
        ctx.set_text_baseline(mlc_core::render::TextBaseline::Middle);
        ctx.fill_text(t, icon_x + icon_size + 6.0, y + height / 2.0);
    }

    coord.register_on_layer(id, Rect::new(x, y, width, height), Sense::CLICK, layer);
    width
}

/// Draw a thin vertical divider (top/bottom toolbars) inset a few px from
/// the toolbar's own top/bottom edge, and return the horizontal space it
/// occupies (including its own small margins).
pub(crate) fn separator(ctx: &mut dyn RenderContext, rt: &RuntimeTheme, x: f64, y: f64, height: f64) -> f64 {
    let inset = 8.0;
    ctx.set_stroke_color(&rt.colors.toolbar_divider);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(x, y + inset);
    ctx.line_to(x, y + height - inset);
    ctx.stroke();
    1.0
}
