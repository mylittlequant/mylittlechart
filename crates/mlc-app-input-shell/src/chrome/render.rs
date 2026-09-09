//! Chrome strip render functions — require `dyn RenderContext`.
//!
//! Shared by both the native (vello) shell and the web (canvas2d) shell.
//! Platform-neutral types and geometry logic live in `chrome::types` / `chrome::logic`.

use uzor::WidgetId;
use uzor::types::Rect as WidgetRect;
use uzor::render::{draw_svg_icon, RenderContext, TextAlign, TextBaseline};
use mlc_core::engine::render::RenderContext as MlcRenderContext;
use mlc_core::state::icons::icon_svg;
use mlc_app_layout::ui::widgets::{
    draw_input, draw_input_cursor, InputConfig, WidgetState, WidgetTheme,
};
// Only the account button (cloud builds) fits a label with an ellipsis today.
#[cfg(feature = "cloud")]
use mlc_app_layout::ui::widgets::fit_text_to_width;

use super::types::{
    CHROME_HEIGHT, ChromeColors, ChromeContextMenu, ChromeFlavor, ChromeHit, ChromeState,
};
#[cfg(feature = "cloud")]
use super::types::ChromeUpdate;
use crate::tooltip::TooltipState;
use crate::tooltip::calculate_tooltip_position;

// ── Layout constants ──────────────────────────────────────────────────────────

const BUTTON_WIDTH: f64 = 46.0;
const BUTTON_HEIGHT: f64 = CHROME_HEIGHT;
const CLOSE_WINDOW_BUTTON_WIDTH: f64 = 36.0;
/// Width of the rightmost cluster that mirrors the OS window controls —
/// minimise, maximise, close — each `BUTTON_WIDTH` wide (see "Window-control
/// buttons" below). `render_backdrop_dim` leaves exactly this strip out of
/// the chrome dim on `ChromeFlavor::Native` (owner's ruling 2026-08-31);
/// derived from `BUTTON_WIDTH` rather than hand-copied so the two can never
/// drift apart.
pub const SYSTEM_BUTTONS_WIDTH: f64 = BUTTON_WIDTH * 3.0;
const MASCOT_BUTTON_WIDTH: f64 = 0.0;
const MENU_BUTTON_WIDTH: f64 = 36.0;
const NEW_WINDOW_BUTTON_WIDTH: f64 = 36.0;
const NEW_TAB_BUTTON_WIDTH: f64 = 28.0;
/// Icon-sized cloud buttons (Web flavor, cloud builds).
#[cfg(feature = "cloud")]
const CLOUD_ICON_BUTTON_WIDTH: f64 = 30.0;
const TAB_LEFT_MARGIN: f64 = 4.0;
// Real separation between tabs so compressed neighbors never appear to
// touch (owner report 2026-09: tabs "collide" when many are open). Mirrored
// in logic.rs, which budgets for it in `fit_tab_widths`'s `reserved`.
const TAB_GAP: f64 = 4.0;
/// Active/hover marker under a tab: height and the one-pixel gap that keeps
/// it off the strip's bottom separator (Arc H2). Shared by the resting tabs
/// AND the floating tab drawn during drag&drop — the floating copy used to
/// carry its own formula and merged with the separator again.
const TAB_MARKER_H: f64 = 2.0;
const TAB_MARKER_GAP: f64 = 1.0;
fn tab_marker_y() -> f64 {
    CHROME_HEIGHT - 1.0 - TAB_MARKER_GAP - TAB_MARKER_H
}
const TAB_PADDING_H: f64 = 6.0;
const TAB_CLOSE_SIZE: f64 = 16.0;
/// Width of the "⋯" overflow button shown left of "+" when tabs overflow.
const TAB_OVERFLOW_WIDTH: f64 = 18.0;

const CONTEXT_MENU_WIDTH: f64 = 160.0;
const CONTEXT_MENU_ITEM_HEIGHT: f64 = 28.0;
const CONTEXT_MENU_PADDING: f64 = 4.0;
const CONTEXT_MENU_ITEMS: &[(&str, bool)] = &[
    ("Close Window", false),
    ("Delete Window", true),
];

// ── ButtonPositions ───────────────────────────────────────────────────────────

struct ButtonPositions {
    /// Only valid for Native flavor.
    close_x: f64,
    /// Only valid for Native flavor.
    maximize_x: f64,
    /// Only valid for Native flavor.
    minimize_x: f64,
    /// Only valid for Native flavor.
    close_window_left: f64,
    /// Valid for both flavors (Web: rightmost button).
    menu_left: f64,
    /// Only valid for Native flavor.
    new_window_left: f64,
    /// Web flavor: left edge of the "Sign in" / account button (cloud builds).
    #[cfg(feature = "cloud")]
    login_left: f64,
    #[cfg(feature = "cloud")]
    download_left: f64,
    #[cfg(feature = "cloud")]
    news_left: f64,
}

impl ButtonPositions {
    /// `update_w` is `logic::update_button_width`'s answer — zero when the
    /// update control is not drawn, which closes its slot up. `signed_in`
    /// closes the gear's own slot the same way (`logic::gear_width`) and
    /// widens the account button into it (`logic::account_button_width`) —
    /// owner ruling 2026-09-10.
    fn compute(width: f64, flavor: ChromeFlavor, #[allow(unused)] update_w: f64, signed_in: bool) -> Self {
        let gear_w = super::logic::gear_width(signed_in);
        match flavor {
            ChromeFlavor::Native => {
                let close_x           = width - BUTTON_WIDTH;
                let maximize_x        = width - BUTTON_WIDTH * 2.0;
                let minimize_x        = width - BUTTON_WIDTH * 3.0;
                let close_window_left = minimize_x - CLOSE_WINDOW_BUTTON_WIDTH;
                let mascot_left       = close_window_left - MASCOT_BUTTON_WIDTH;
                let menu_left         = mascot_left - gear_w;
                let new_window_left   = menu_left - NEW_WINDOW_BUTTON_WIDTH;
                // Native cloud cluster, left of "new window": "Sign in"/
                // account, then Download / News — see the same computation
                // in `logic.rs`, which the hit test reads.
                #[cfg(feature = "cloud")]
                let login_left = new_window_left - super::logic::account_button_width(signed_in);
                #[cfg(feature = "cloud")]
                let download_left = login_left - update_w;
                #[cfg(feature = "cloud")]
                let news_left = download_left - CLOUD_ICON_BUTTON_WIDTH;
                Self {
                    close_x, maximize_x, minimize_x, close_window_left, menu_left, new_window_left,
                    #[cfg(feature = "cloud")]
                    login_left,
                    #[cfg(feature = "cloud")]
                    download_left,
                    #[cfg(feature = "cloud")]
                    news_left,
                }
            }
            ChromeFlavor::Web => {
                // Web: right-to-left — gear, "Sign in"/account, Download,
                // News. Gear is zero-width while signed in, which hides it.
                let menu_left = width - gear_w;
                #[cfg(feature = "cloud")]
                let login_left = menu_left - super::logic::account_button_width(signed_in);
                #[cfg(feature = "cloud")]
                let download_left = login_left - update_w;
                #[cfg(feature = "cloud")]
                let news_left = download_left - CLOUD_ICON_BUTTON_WIDTH;
                Self {
                    close_x:           0.0,
                    maximize_x:        0.0,
                    minimize_x:        0.0,
                    close_window_left: 0.0,
                    menu_left,
                    new_window_left:   0.0,
                    #[cfg(feature = "cloud")]
                    login_left,
                    #[cfg(feature = "cloud")]
                    download_left,
                    #[cfg(feature = "cloud")]
                    news_left,
                }
            }
        }
    }
}

// ── update_tab_widths ─────────────────────────────────────────────────────────

/// Pre-compute tab widths using the render context for accurate text measurement.
///
/// Must be called once per frame before `render`.
///
/// A tab being renamed inline sizes to the LIVE field text, so the tab grows
/// as the name is typed instead of clipping it. The strip then compresses
/// (`logic::fit_tab_widths`): on overflow tabs equalize (the live-renaming
/// tab keeps its natural width so the typed name stays visible) so everything
/// — including the always-visible "+" — stays left of the right-side controls.
pub fn update_tab_widths(
    ctx: &mut dyn RenderContext,
    state: &mut ChromeState,
    width: f64,
    input_coord: &std::cell::RefCell<uzor::input::InputCoordinator>,
) {
    ctx.set_font("12px sans-serif");
    state.tab_widths.clear();
    for (i, tab) in state.tabs.iter().enumerate() {
        let live_name;
        let text = if state.renaming_tab == Some(i) {
            live_name = input_coord
                .borrow()
                .text_fields()
                .text(&WidgetId::from(mlc_app::text_input::CHROME_TAB_RENAME))
                .to_string();
            live_name.as_str()
        } else {
            tab.name.as_str()
        };
        let text_w = ctx.measure_text(text);
        let w = TAB_PADDING_H + text_w + TAB_CLOSE_SIZE + TAB_PADDING_H;
        state.tab_widths.push(w);
    }
    super::logic::fit_tab_widths(state, width);
    // The scroll offset can outlive its content (tabs closed, window
    // widened) — clamp it now that the widths are final.
    let max_scroll = super::logic::max_tab_scroll(state, width);
    if state.tab_scroll > max_scroll {
        state.tab_scroll = max_scroll;
    }
    // Keep the tab being renamed in view while typing grows it — scroll the
    // strip so its right edge never disappears under the "+" / controls.
    if let Some(ri) = state.renaming_tab {
        let right_edge_of_strip = super::logic::tabs_right_limit(width, state)
            - NEW_TAB_BUTTON_WIDTH - 1.0 - 4.0;
        let mut cursor = TAB_LEFT_MARGIN - state.tab_scroll;
        for (i, w) in state.tab_widths.iter().enumerate() {
            let right = cursor + w;
            if i == ri {
                if right > right_edge_of_strip {
                    super::logic::scroll_tab_strip(state, width, right - right_edge_of_strip);
                }
                break;
            }
            cursor = right + TAB_GAP;
        }
    }
}

// ── render ────────────────────────────────────────────────────────────────────

/// Render the chrome strip.
///
/// `skeleton_mode` suppresses tabs and right-side icon rendering (used while
/// the window is being resized on native). On Web pass `false`.
///
/// `input_coord` is the chart's shared input coordinator: while a tab is
/// being renamed inline (`state.renaming_tab`) the tab's live text / caret /
/// selection are read from its `TextFieldStore` and the field's geometry is
/// pushed back via `update_field` (the `render_inline_prim_text_editor`
/// division of labor — render pushes geometry, the store's generic pointer
/// handling consumes it).
pub fn render(
    ctx: &mut dyn MlcRenderContext,
    state: &ChromeState,
    width: f64,
    skeleton_mode: bool,
    input_coord: &std::cell::RefCell<uzor::input::InputCoordinator>,
) {
    let c = &state.colors;
    let bp = ButtonPositions::compute(
        width, state.flavor, super::logic::positions_update_w(state), state.account_label.is_some(),
    );

    // Background.
    ctx.set_fill_color(&c.background);
    ctx.fill_rect(0.0, 0.0, width, CHROME_HEIGHT);

    // Close button hover highlight (Native only).
    if state.flavor == ChromeFlavor::Native {
        if state.hovered == ChromeHit::CloseButton {
            ctx.set_fill_color(&c.close_hover);
            ctx.fill_rect(bp.close_x, 0.0, BUTTON_WIDTH, BUTTON_HEIGHT);
        }
    }

    if !skeleton_mode {
        // ── Tabs ──────────────────────────────────────────────────────────────
        // The strip ends at the left edge of the right-side controls; tabs
        // are already shrunk to fit (fit_tab_widths), the clip is the hard
        // guarantee that nothing ever bleeds under the buttons — and when
        // the strip overflows, that nothing peeks between the tabs and the
        // "⋯"/"+" cluster.
        let tabs_limit = super::logic::tabs_right_limit(width, state);
        let overflow = super::logic::max_tab_scroll(state, width) > 0.5;
        let strip_end = {
            let n = state.tab_widths.len();
            let natural: f64 = state.tab_widths.iter().sum::<f64>()
                + TAB_GAP * (n as f64 - 1.0).max(0.0);
            TAB_LEFT_MARGIN - state.tab_scroll + natural
        };
        let new_tab_x = strip_end.min(tabs_limit - NEW_TAB_BUTTON_WIDTH);
        let tab_area_right = if overflow {
            new_tab_x - TAB_OVERFLOW_WIDTH
        } else {
            tabs_limit
        };
        ctx.save();
        ctx.clip_rect(0.0, 0.0, tab_area_right, CHROME_HEIGHT);
        let mut cursor = TAB_LEFT_MARGIN - state.tab_scroll;
        for (i, tab) in state.tabs.iter().enumerate() {
            let tw        = state.tab_widths.get(i).copied().unwrap_or(80.0);
            let tab_left  = cursor;
            let tab_right = cursor + tw;

            // The marker sits one pixel clear of the strip's own bottom rule.
            //
            // It used to end exactly where that rule begins, and with nothing
            // between them the two read as a single thick band: the tab stopped
            // looking selected and started looking like a heavier border.
            // Nobody saw it in the dark themes, where both colours are close to
            // the background; High Contrast paints the separator white against
            // a blue accent and the merge is unmistakable (owner, 2026-09-03).
            //
            // Widening the accent was the other suggestion and it makes the
            // band thicker, which is the complaint. Moving it onto the
            // separator's row would mean breaking that rule under the active
            // tab — it is drawn after the tab loop and spans the whole strip,
            // so it would need reordering and would leave the strip's bottom
            // edge discontinuous. A gap costs one pixel and neither.
            let marker_y = tab_marker_y();
            if tab.active {
                ctx.set_fill_color(&c.tab_accent);
                ctx.fill_rect(tab_left, marker_y, tw, TAB_MARKER_H);
            } else if state.hovered == ChromeHit::Tab(i) {
                // Only the label space lights the hover underline — hovering
                // the overlay buttons (pencil/close) is not hovering the tab.
                ctx.set_fill_color(&c.button_hover);
                ctx.fill_rect(tab_left, marker_y, tw, TAB_MARKER_H);
            }

            if state.renaming_tab == Some(i) {
                // Inline rename through the shared input widget — the same
                // text/selection rendering as every other field in the app,
                // with no box (chrome = false): the tab itself is the frame.
                // Pencil and close × are hidden while editing (and hit-tested
                // away in logic.rs).
                let wid = WidgetId::from(mlc_app::text_input::CHROME_TAB_RENAME);
                // The store owns the blink cadence (same `cursor_visible`
                // the modal fields read); a coarse wall-clock read is enough
                // for it (same trade-off as `granularity_edit_render`).
                let now_ms = web_time::SystemTime::now()
                    .duration_since(web_time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0);
                let (live_text, text_cursor, selection, caret_visible) = {
                    let coord = input_coord.borrow();
                    let store = coord.text_fields();
                    (
                        store.text(&wid).to_string(),
                        store.cursor(&wid),
                        store.selection_range(&wid),
                        store.cursor_visible(now_ms),
                    )
                };
                // Same ink as the tab name itself — the rename is already
                // announced by the caret and the missing ×; recoloring the
                // text on top of that reads as a glitch.
                let field_theme = WidgetTheme {
                    text_normal: c.icon_normal.clone(),
                    accent: c.tab_accent.clone(),
                    ..WidgetTheme::default()
                };
                let config = InputConfig {
                    value: live_text.clone(),
                    focused: true,
                    cursor: text_cursor,
                    selection_start: selection.map(|(s, _)| s),
                    selection_end: selection.map(|(_, e)| e),
                    font_size: 12.0,
                    padding: 0.0,
                    radius: 0.0,
                    chrome: false,
                    ..Default::default()
                };
                // The field starts exactly where the centered tab name was
                // — entering rename must not shift the text. The rect keeps
                // all the room to the right edge so growing text isn't
                // clipped (the tab itself grows with the live text anyway,
                // see update_tab_widths).
                ctx.set_font("12px sans-serif");
                let live_text_w = ctx.measure_text(&live_text);
                let field_x = tab_left + (tw - live_text_w).max(0.0) / 2.0;
                let field_rect = WidgetRect::new(
                    field_x,
                    2.0,
                    (tab_right - field_x - 4.0).max(20.0),
                    CHROME_HEIGHT - 4.0,
                );
                let result = draw_input(ctx, &config, WidgetState::Normal, field_rect, &field_theme);
                if caret_visible {
                    draw_input_cursor(
                        ctx,
                        result.cursor_x,
                        result.cursor_y,
                        result.cursor_height,
                        &c.icon_normal,
                    );
                }
                // Push fresh geometry so a click inside the tab places the
                // caret. Store coords are chart-local (the chart's origin is
                // just below the chrome strip), hence the negative y.
                input_coord.borrow_mut().text_fields_mut().update_field(
                    &wid,
                    (tab_left, -CHROME_HEIGHT, tw, CHROME_HEIGHT),
                    result.char_x_positions,
                );
            } else {
            // Needed before the name is laid out (below) as well as by the
            // close patch itself: while any of these is true the × shows,
            // and the name must give up the space the × sits in.
            let tab_hovered = state.hovered == ChromeHit::Tab(i)
                || state.hovered == ChromeHit::TabClose(i)
                || state.hovered == ChromeHit::TabRename(i);
            {
                // The tab name centers in the tab body — but while the × is
                // showing it centers in the space LEFT of the × instead, so
                // a long name is clipped there and never slides underneath
                // it (name and × must never share pixels).
                ctx.set_font("12px sans-serif");
                let text_w = ctx.measure_text(&tab.name);
                let name_w = if tab_hovered { (tw - TAB_CLOSE_SIZE).max(0.0) } else { tw };
                let text_x = tab_left + (name_w - text_w).max(0.0) / 2.0;
                ctx.set_fill_color(&c.icon_normal);
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.save();
                ctx.clip_rect(tab_left, 0.0, name_w, CHROME_HEIGHT);
                ctx.fill_text(&tab.name, text_x, CHROME_HEIGHT / 2.0);
                ctx.restore();
            }

            // Close × at the RIGHT edge. Same rounded patch, kept clear of
            // the underline strip; the button-hover face only lights when
            // the × itself is hovered.
            if tab_hovered {
                let close_rect_left = tab_right - TAB_CLOSE_SIZE;
                let icon_size = 14.0;
                let close_hovered = state.hovered == ChromeHit::TabClose(i);
                let icon_color = if close_hovered {
                    &c.close_hover
                } else {
                    &c.icon_normal
                };
                if close_hovered {
                    ctx.set_fill_color(&c.button_hover);
                } else {
                    // The strip background IS this tab's body: a tab has no
                    // body fill at all, only a 2px underline at its bottom
                    // edge, so `tab_accent`/`button_hover` are underline
                    // colours and painting the patch with either would put a
                    // coloured block on a flat body. What made a neighbour
                    // seem to show through was never this fill — it was the
                    // name centred across the whole body and sliding under
                    // the ×, fixed above.
                    ctx.set_fill_color(&c.background);
                }
                ctx.fill_rounded_rect(
                    close_rect_left - 1.0,
                    3.0,
                    TAB_CLOSE_SIZE + 1.0,
                    CHROME_HEIGHT - 9.0,
                    6.0,
                );
                let icon_x = close_rect_left + (TAB_CLOSE_SIZE - icon_size) / 2.0;
                let icon_y = (CHROME_HEIGHT - icon_size) / 2.0;
                if let Some(svg) = icon_svg("close") {
                    draw_svg_icon(ctx, svg, icon_x, icon_y, icon_size, icon_size, icon_color);
                }
            }
            }

            cursor = tab_right + TAB_GAP;
        }

        // ── Tab drag&drop visuals ─────────────────────────────────────────────
        // While a drag is active the dragged tab floats with the cursor and an
        // accent insertion line marks the drop slot between the resting tabs.
        if let Some(drag) = state.tab_drag.filter(|d| d.active) {
            let drop_idx = super::logic::tab_drop_index(state, drag.cur_x);
            // Insertion line at the left edge of the drop slot.
            let ins_x = if drop_idx >= state.tab_widths.len() {
                strip_end
            } else {
                super::logic::tab_left_x(state, drop_idx) - TAB_GAP / 2.0
            };
            ctx.set_fill_color(&c.tab_accent);
            ctx.fill_rect(ins_x - 1.0, 3.0, 2.0, CHROME_HEIGHT - 9.0);

            if let (Some(tab), Some(tw)) = (
                state.tabs.get(drag.index),
                state.tab_widths.get(drag.index).copied(),
            ) {
                let fx = drag.cur_x - drag.grab_dx;
                ctx.set_fill_color(&c.background);
                ctx.fill_rect(fx, 0.0, tw, CHROME_HEIGHT);
                draw_rect_outline(ctx, fx, 0.0, tw, CHROME_HEIGHT - 1.0, 1.0, &c.separator);
                ctx.set_font("12px sans-serif");
                let text_w = ctx.measure_text(&tab.name);
                let text_x = fx + (tw - text_w).max(0.0) / 2.0;
                ctx.set_fill_color(&c.icon_hover);
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.save();
                ctx.clip_rect(fx, 0.0, tw, CHROME_HEIGHT);
                ctx.fill_text(&tab.name, text_x, CHROME_HEIGHT / 2.0);
                ctx.restore();
                if tab.active {
                    ctx.set_fill_color(&c.tab_accent);
                    ctx.fill_rect(fx, tab_marker_y(), tw, TAB_MARKER_H);
                }
            }
        }
        ctx.restore();

        // ── New-tab (+) button ─────────────────────────────────────────────────
        // Pinned inside the strip: never pushed under the right-side controls
        // (position computed before the tab loop, shared with the clip).

        let div_h = CHROME_HEIGHT * 0.6;
        let div_y = (CHROME_HEIGHT - div_h) / 2.0;
        ctx.set_fill_color(&c.separator);
        ctx.fill_rect(new_tab_x, div_y, 1.0, div_h);

        if state.hovered == ChromeHit::NewTabButton {
            ctx.set_fill_color(&c.button_hover);
            ctx.fill_rect(new_tab_x + 1.0, 0.0, NEW_TAB_BUTTON_WIDTH - 1.0, BUTTON_HEIGHT);
        }

        // Hover lights the background patch only — the icon itself never
        // recolors (owner's rule; the window-control cluster below is the
        // sole exception).
        let plus_color = &c.icon_normal;
        let plus_cx = (new_tab_x + NEW_TAB_BUTTON_WIDTH / 2.0).floor() + 0.5;
        let plus_cy = (CHROME_HEIGHT / 2.0).floor() + 0.5;
        let arm = 5.0;
        ctx.set_stroke_color(plus_color);
        ctx.set_stroke_width(1.5);
        ctx.begin_path();
        ctx.move_to(plus_cx - arm, plus_cy);
        ctx.line_to(plus_cx + arm, plus_cy);
        ctx.stroke();
        ctx.begin_path();
        ctx.move_to(plus_cx, plus_cy - arm);
        ctx.line_to(plus_cx, plus_cy + arm);
        ctx.stroke();

        // ── "⋯" overflow button ──────────────────────────────────────────────
        // Shown only when the strip overflows even at MIN_TAB_WIDTH (tabs are
        // then scrollable with the wheel). Sits immediately left of "+",
        // drawn over the clipped strip; hover/click opens the tab list
        // (render_tab_list below).
        let max_scroll = super::logic::max_tab_scroll(state, width);
        if max_scroll > 0.5 {
            let ov_left = new_tab_x - TAB_OVERFLOW_WIDTH;
            let ov_hovered = state.hovered == ChromeHit::TabOverflow;
            ctx.set_fill_color(if ov_hovered { &c.button_hover } else { &c.background });
            ctx.fill_rounded_rect(ov_left, 3.0, TAB_OVERFLOW_WIDTH, CHROME_HEIGHT - 9.0, 6.0);
            ctx.set_fill_color(&c.icon_normal);
            ctx.set_font("12px sans-serif");
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text("…", ov_left + TAB_OVERFLOW_WIDTH / 2.0, CHROME_HEIGHT / 2.0 - 1.0);
        }

        // ── Right-side buttons (flavor-dependent) ─────────────────────────────
        match state.flavor {
            ChromeFlavor::Native => {
                // new_window cell + icon.
                {
                    if state.hovered == ChromeHit::NewWindowButton {
                        ctx.set_fill_color(&c.button_hover);
                        ctx.fill_rect(bp.new_window_left, 0.0, NEW_WINDOW_BUTTON_WIDTH, BUTTON_HEIGHT);
                    }
                    let icon_color = &c.icon_normal;
                    let icon_x = bp.new_window_left + (NEW_WINDOW_BUTTON_WIDTH - 18.0) / 2.0;
                    let icon_y = (CHROME_HEIGHT - 18.0) / 2.0;
                    if let Some(svg) = icon_svg("new_window") {
                        draw_svg_icon(ctx, svg, icon_x, icon_y, 18.0, 18.0, icon_color);
                    }
                }

                // menu cell + icon — hidden entirely while signed in (owner
                // ruling 2026-09-10): the gear's slot then belongs to the
                // widened account button (`draw_account_button`) instead.
                if state.account_label.is_none() {
                    if state.hovered == ChromeHit::MenuButton {
                        ctx.set_fill_color(&c.button_hover);
                        ctx.fill_rect(bp.menu_left, 0.0, MENU_BUTTON_WIDTH, BUTTON_HEIGHT);
                    }
                    let icon_color = &c.icon_normal;
                    let icon_x = bp.menu_left + (MENU_BUTTON_WIDTH - 18.0) / 2.0;
                    let icon_y = (CHROME_HEIGHT - 18.0) / 2.0;
                    if let Some(svg) = icon_svg("settings") {
                        draw_svg_icon(ctx, svg, icon_x, icon_y, 18.0, 18.0, icon_color);
                    }
                }

                // Sign-in / account button — one drawing for both flavors.
                // It used to be written twice and the copies disagreed: the
                // native one read `account_label`, the web one printed the
                // literal "Sign in" and never looked at the field at all. So
                // the web strip went on inviting a signed-in visitor to sign
                // in even after the shell started publishing the label
                // (owner, 2026-09-04). One body, so they cannot drift again.
                #[cfg(feature = "cloud")]
                draw_account_button(ctx, state, c, &bp);

                // Cloud icon buttons: Download / News (left of Sign in) —
                // one drawing for both flavors, see `draw_cloud_icon_buttons`.
                #[cfg(feature = "cloud")]
                draw_cloud_icon_buttons(ctx, state, c, &bp);
            }
            ChromeFlavor::Web => {
                // Sign-in / account button — one drawing for both flavors.
                // It used to be written twice and the copies disagreed: the
                // native one read `account_label`, the web one printed the
                // literal "Sign in" and never looked at the field at all. So
                // the web strip went on inviting a signed-in visitor to sign
                // in even after the shell started publishing the label
                // (owner, 2026-09-04). One body, so they cannot drift again.
                #[cfg(feature = "cloud")]
                draw_account_button(ctx, state, c, &bp);

                // Cloud icon buttons: Download / News (left of Sign in) —
                // one drawing for both flavors, see `draw_cloud_icon_buttons`.
                #[cfg(feature = "cloud")]
                draw_cloud_icon_buttons(ctx, state, c, &bp);

                // Menu button (gear) — rightmost. Same hover rule: background
                // patch only, the icon itself does not recolor. Hidden
                // entirely while signed in (owner ruling 2026-09-10) — the
                // widened account button (`draw_account_button`) takes its
                // slot instead.
                if state.account_label.is_none() {
                    if state.hovered == ChromeHit::MenuButton {
                        ctx.set_fill_color(&c.button_hover);
                        ctx.fill_rect(bp.menu_left, 0.0, MENU_BUTTON_WIDTH, BUTTON_HEIGHT);
                    }
                    let icon_x = bp.menu_left + (MENU_BUTTON_WIDTH - 18.0) / 2.0;
                    let icon_y = (CHROME_HEIGHT - 18.0) / 2.0;
                    if let Some(svg) = icon_svg("settings") {
                        draw_svg_icon(ctx, svg, icon_x, icon_y, 18.0, 18.0, &c.icon_normal);
                    }
                }
            }
        }
    }

    // ── Window-control buttons (Native only) ──────────────────────────────────
    if state.flavor == ChromeFlavor::Native {
        // Separator before close_window.
        {
            let div_h = CHROME_HEIGHT * 0.6;
            let div_y = (CHROME_HEIGHT - div_h) / 2.0;
            ctx.set_fill_color(&c.separator);
            ctx.fill_rect(bp.close_window_left, div_y, 1.0, div_h);
        }

        // close_window X icon.
        {
            let icon_color = if state.hovered == ChromeHit::CloseWindowButton {
                &c.icon_hover
            } else {
                &c.icon_normal
            };
            let cx = bp.close_window_left + CLOSE_WINDOW_BUTTON_WIDTH / 2.0;
            let cy = CHROME_HEIGHT / 2.0;
            let s = 3.5;
            ctx.set_stroke_color(icon_color);
            ctx.set_stroke_width(1.0);
            ctx.begin_path();
            ctx.move_to(cx - s, cy - s);
            ctx.line_to(cx + s, cy + s);
            ctx.stroke();
            ctx.begin_path();
            ctx.move_to(cx - s, cy + s);
            ctx.line_to(cx + s, cy - s);
            ctx.stroke();
        }

        // Separator before minimize.
        {
            let div_h = CHROME_HEIGHT * 0.6;
            let div_y = (CHROME_HEIGHT - div_h) / 2.0;
            ctx.set_fill_color(&c.separator);
            ctx.fill_rect(bp.minimize_x, div_y, 1.0, div_h);
        }

        // Minimize icon.
        {
            let icon_color = if state.hovered == ChromeHit::MinimizeButton {
                &c.icon_hover
            } else {
                &c.icon_normal
            };
            let cx = bp.minimize_x + BUTTON_WIDTH / 2.0;
            let cy = CHROME_HEIGHT / 2.0;
            ctx.set_fill_color(icon_color);
            ctx.fill_rect(cx - 5.0, cy, 10.0, 1.0);
        }

        // Maximize / restore icon.
        {
            let icon_color = if state.hovered == ChromeHit::MaximizeButton {
                &c.icon_hover
            } else {
                &c.icon_normal
            };
            let cx = bp.maximize_x + BUTTON_WIDTH / 2.0;
            let cy = CHROME_HEIGHT / 2.0;
            let half = 5.0;

            if state.is_maximized {
                draw_rect_outline(ctx, cx - half + 2.0, cy - half - 2.0, half * 2.0, half * 2.0, 1.0, icon_color);
                ctx.set_fill_color(&c.background);
                ctx.fill_rect(cx - half, cy - half, half * 2.0, half * 2.0);
                draw_rect_outline(ctx, cx - half, cy - half, half * 2.0, half * 2.0, 1.0, icon_color);
            } else {
                draw_rect_outline(ctx, cx - half, cy - half, half * 2.0, half * 2.0, 1.0, icon_color);
            }
        }

        // Close (X) icon.
        {
            let icon_color = &c.icon_normal;
            let cx = bp.close_x + BUTTON_WIDTH / 2.0;
            let cy = CHROME_HEIGHT / 2.0;
            let s = 5.0;
            ctx.set_stroke_color(icon_color);
            ctx.set_stroke_width(1.5);
            ctx.begin_path();
            ctx.move_to(cx - s, cy - s);
            ctx.line_to(cx + s, cy + s);
            ctx.stroke();
            ctx.begin_path();
            ctx.move_to(cx - s, cy + s);
            ctx.line_to(cx + s, cy - s);
            ctx.stroke();
        }
    }

    // Bottom separator line.
    ctx.set_fill_color(&c.separator);
    ctx.fill_rect(0.0, CHROME_HEIGHT - 1.0, width, 1.0);
}

// ── render_backdrop_dim ──────────────────────────────────────────────────────

/// Dim the chrome strip to match an open `dim_backdrop` modal.
///
/// `render_modal_layer`'s own backdrop fill stops at the top of the chart
/// content — `ChartApp` never sees the chrome strip at all, since the shell
/// subtracts `CHROME_HEIGHT` from the window size before handing it resize
/// events. Call this AFTER [`render`] each frame a `dim_backdrop` modal is
/// open (`ChartPanelApp::dim_backdrop_color`) so the fill lands on top of the
/// freshly-painted chrome, same as the modal's own veil lands on top of the
/// chart beneath it.
///
/// `ChromeFlavor::Native` leaves [`SYSTEM_BUTTONS_WIDTH`] undimmed at the
/// right edge — minimise, maximise, close (owner's ruling 2026-08-31: those
/// stay live even while the rest of the window is dimmed). `ChromeFlavor::Web`
/// has no such cluster (a browser tab owns its own window chrome), so it
/// dims edge to edge.
pub fn render_backdrop_dim(
    ctx: &mut dyn MlcRenderContext,
    width: f64,
    flavor: ChromeFlavor,
    backdrop_color: &str,
) {
    let dim_w = match flavor {
        ChromeFlavor::Native => (width - SYSTEM_BUTTONS_WIDTH).max(0.0),
        ChromeFlavor::Web => width,
    };
    if dim_w <= 0.0 {
        return;
    }
    ctx.set_fill_color(backdrop_color);
    ctx.fill_rect(0.0, 0.0, dim_w, CHROME_HEIGHT);
}

// ── render_tooltip ────────────────────────────────────────────────────────────

/// Render the chrome tooltip if one is currently visible.
pub fn render_tooltip(
    ctx: &mut dyn RenderContext,
    state: &ChromeState,
    screen_width: f64,
    screen_height: f64,
) {
    render_tooltip_themed(
        ctx,
        &state.tooltip,
        &state.colors.tooltip_bg,
        &state.colors.tooltip_text,
        screen_width,
        screen_height,
    );
}

/// Render a tooltip using explicit theme colors.
pub fn render_tooltip_themed(
    ctx: &mut dyn RenderContext,
    tooltip: &TooltipState,
    bg_color: &str,
    text_color: &str,
    screen_width: f64,
    screen_height: f64,
) {
    let active = match tooltip.get_active() {
        Some(t) => t,
        None => return,
    };

    let opacity = tooltip.get_opacity();
    if opacity <= 0.0 {
        return;
    }

    let text = &active.text;
    ctx.set_font("12px sans-serif");
    let text_width = ctx.measure_text(text);
    let pad = 6.0;
    let tw = text_width + pad * 2.0;
    let th = 12.0 + pad * 2.0;

    let (tx, ty) = calculate_tooltip_position(
        active.position,
        (tw, th),
        (screen_width, screen_height),
        (0.0, 20.0),
    );

    ctx.save();
    ctx.set_global_alpha(opacity);

    ctx.set_fill_color("#00000060");
    ctx.fill_rounded_rect(tx + 1.0, ty + 1.0, tw, th, 4.0);

    ctx.set_fill_color(bg_color);
    ctx.fill_rounded_rect(tx, ty, tw, th, 4.0);

    ctx.set_fill_color(text_color);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(text, tx + pad, ty + th / 2.0);

    ctx.restore();
}

// ── render_context_menu ───────────────────────────────────────────────────────

/// Render the chrome right-click context menu if open.
pub fn render_context_menu(
    ctx: &mut dyn RenderContext,
    menu: &ChromeContextMenu,
    colors: &ChromeColors,
) {
    if !menu.open {
        return;
    }

    let item_count = CONTEXT_MENU_ITEMS.len() as f64;
    let menu_h = CONTEXT_MENU_PADDING * 2.0 + item_count * CONTEXT_MENU_ITEM_HEIGHT;

    ctx.set_fill_color("rgba(0,0,0,0.3)");
    ctx.fill_rect(menu.x + 2.0, menu.y + 2.0, CONTEXT_MENU_WIDTH, menu_h);

    ctx.set_fill_color(&colors.background);
    ctx.fill_rect(menu.x, menu.y, CONTEXT_MENU_WIDTH, menu_h);

    draw_rect_outline(ctx, menu.x, menu.y, CONTEXT_MENU_WIDTH, menu_h, 1.0, &colors.separator);

    ctx.set_font("12px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);

    for (i, (label, is_danger)) in CONTEXT_MENU_ITEMS.iter().enumerate() {
        let item_y = menu.y + CONTEXT_MENU_PADDING + i as f64 * CONTEXT_MENU_ITEM_HEIGHT;

        if menu.hovered_index == i as i32 {
            let hover_color = if *is_danger { &colors.close_hover } else { &colors.button_hover };
            ctx.set_fill_color(hover_color);
            ctx.fill_rect(menu.x + 1.0, item_y, CONTEXT_MENU_WIDTH - 2.0, CONTEXT_MENU_ITEM_HEIGHT);
        }

        let text_color = if *is_danger && menu.hovered_index == i as i32 {
            &colors.icon_hover
        } else if *is_danger {
            &colors.close_hover
        } else {
            &colors.icon_normal
        };
        ctx.set_fill_color(text_color);
        ctx.fill_text(label, menu.x + 12.0, item_y + CONTEXT_MENU_ITEM_HEIGHT / 2.0);
    }
}

// ── render_tab_list ───────────────────────────────────────────────────────────

const TAB_LIST_ITEM_HEIGHT: f64 = 26.0;
const TAB_LIST_PADDING: f64 = 4.0;

/// Render the tab-list dropdown (opened from the "⋯" overflow button).
/// Overlay pass — call where `render_context_menu` is called so it paints
/// above the chart. Window-space coordinates (same as the context menu).
pub fn render_tab_list(
    ctx: &mut dyn RenderContext,
    state: &ChromeState,
    width: f64,
) {
    if !state.tab_list_open {
        return;
    }
    let (mx, my, mw, mh) = super::logic::tab_list_rect(state, width);
    let c = &state.colors;

    ctx.set_fill_color("rgba(0,0,0,0.3)");
    ctx.fill_rect(mx + 2.0, my + 2.0, mw, mh);

    ctx.set_fill_color(&c.background);
    ctx.fill_rect(mx, my, mw, mh);

    draw_rect_outline(ctx, mx, my, mw, mh, 1.0, &c.separator);

    ctx.set_font("12px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);

    for (i, tab) in state.tabs.iter().enumerate() {
        let item_y = my + TAB_LIST_PADDING + i as f64 * TAB_LIST_ITEM_HEIGHT;

        if state.tab_list_hovered == i as i32 {
            ctx.set_fill_color(&c.button_hover);
            ctx.fill_rect(mx + 1.0, item_y, mw - 2.0, TAB_LIST_ITEM_HEIGHT);
        }
        // Active-tab marker: the same accent the strip underline uses.
        if tab.active {
            ctx.set_fill_color(&c.tab_accent);
            ctx.fill_rect(mx + 1.0, item_y + 4.0, 2.0, TAB_LIST_ITEM_HEIGHT - 8.0);
        }

        ctx.set_fill_color(if tab.active { &c.icon_hover } else { &c.icon_normal });
        ctx.save();
        ctx.clip_rect(mx + 8.0, item_y, mw - 16.0, TAB_LIST_ITEM_HEIGHT);
        ctx.fill_text(&tab.name, mx + 10.0, item_y + TAB_LIST_ITEM_HEIGHT / 2.0);
        ctx.restore();
    }
}

// ── Private helpers ───────────────────────────────────────────────────────────

fn draw_rect_outline(ctx: &mut dyn RenderContext, x: f64, y: f64, w: f64, h: f64, t: f64, color: &str) {
    ctx.set_fill_color(color);
    ctx.fill_rect(x, y, w, t);
    ctx.fill_rect(x, y + h - t, w, t);
    ctx.fill_rect(x, y + t, t, h - t * 2.0);
    ctx.fill_rect(x + w - t, y + t, t, h - t * 2.0);
}

/// A round avatar placeholder: a filled circle in the theme accent with the
/// label's first letter, centered. Stands in for the real avatar set the
/// owner has not picked yet (2026-09-10 ruling) — one drawing helper so
/// swapping it for a picked image later touches this one place, the same
/// reason [`draw_account_button`] itself is one body instead of two.
#[cfg(feature = "cloud")]
fn draw_avatar_placeholder(ctx: &mut dyn MlcRenderContext, cx: f64, cy: f64, r: f64, label: &str, bg: &str) {
    ctx.set_fill_color(bg);
    ctx.begin_path();
    ctx.arc(cx, cy, r, 0.0, std::f64::consts::TAU);
    ctx.fill();
    if let Some(letter) = label.chars().next() {
        ctx.set_fill_color(contrasting_text_color(bg));
        ctx.set_font("600 11px sans-serif");
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        let upper: String = letter.to_uppercase().collect();
        ctx.fill_text(&upper, cx, cy + 0.5);
    }
}

/// The sign-in / account button: one control with two readings — "Sign in"
/// while nobody is, the account's own name once somebody is, so the button the
/// user pressed is the one that now says who they are.
///
/// Signed out it is the single accent action in the strip, a fixed-width
/// centred CTA pill.
///
/// Signed in it is an ordinary control, and a wider one: the gear
/// (`ChromeHit::MenuButton`) is hidden while signed in (owner ruling
/// 2026-09-10) and this button absorbs its slot instead, so the full nick
/// has room without truncating — avatar placeholder, left-aligned nick, then
/// a caret so the button reads as a menu, the same layout every other
/// header dropdown trigger in the app already uses.
///
/// Shared by both flavors deliberately. This lived as two copies that
/// disagreed — the web one ignored `account_label` entirely — which is exactly
/// the drift a duplicated widget produces.
#[cfg(feature = "cloud")]
fn draw_account_button(
    ctx: &mut dyn MlcRenderContext,
    state: &ChromeState,
    c: &ChromeColors,
    bp: &ButtonPositions,
) {
    let signed_in = state.account_label.is_some();
    let hovered = state.hovered == ChromeHit::LoginButton;
    let inset_x = 2.0;
    let inset_y = 4.0;
    let btn_w = super::logic::account_button_width(signed_in);

    if let Some(label) = state.account_label.as_deref().filter(|_| signed_in) {
        if hovered {
            ctx.set_fill_color(&c.button_hover);
            ctx.fill_rounded_rect(
                bp.login_left + inset_x,
                inset_y,
                btn_w - inset_x * 2.0,
                CHROME_HEIGHT - inset_y * 2.0,
                7.0,
            );
        }

        const AVATAR_D: f64 = 20.0;
        const AVATAR_LEFT_PAD: f64 = 8.0;
        const CARET_SIZE: f64 = 12.0;
        const CARET_SLOT_W: f64 = 18.0;
        const TEXT_GAP: f64 = 6.0;

        let avatar_cx = bp.login_left + AVATAR_LEFT_PAD + AVATAR_D / 2.0;
        let avatar_cy = CHROME_HEIGHT / 2.0;
        draw_avatar_placeholder(ctx, avatar_cx, avatar_cy, AVATAR_D / 2.0, label, &c.cta_bg);

        // Caret — a tiny "this opens a menu" tell at the button's right end.
        if let Some(svg) = icon_svg("chevron_down") {
            let caret_x = bp.login_left + btn_w - CARET_SLOT_W + (CARET_SLOT_W - CARET_SIZE) / 2.0;
            let caret_y = (CHROME_HEIGHT - CARET_SIZE) / 2.0;
            draw_svg_icon(ctx, svg, caret_x, caret_y, CARET_SIZE, CARET_SIZE, &c.icon_normal);
        }

        // Nick, left-aligned right after the avatar. Fixed reserved width
        // regardless of the nick's length (no tab-strip jitter — the same
        // discipline `LOGIN_BUTTON_WIDTH` already held for the signed-out
        // CTA) — an ellipsis applies only past the widest allowed width the
        // slot leaves for text (avatar and caret already claimed theirs).
        let text_x = avatar_cx + AVATAR_D / 2.0 + TEXT_GAP;
        let max_text_w = (bp.login_left + btn_w - CARET_SLOT_W - text_x).max(0.0);
        ctx.set_font("600 12px sans-serif");
        ctx.set_fill_color(&c.icon_normal);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.save();
        ctx.clip_rect(bp.login_left, 0.0, btn_w, CHROME_HEIGHT);
        let text_w = ctx.measure_text(label);
        if text_w <= max_text_w {
            ctx.fill_text(label, text_x, CHROME_HEIGHT / 2.0);
        } else {
            let fitted = fit_text_to_width(ctx, label, max_text_w);
            ctx.fill_text(&fitted, text_x, CHROME_HEIGHT / 2.0);
        }
        ctx.restore();
        return;
    }

    // Signed out: the single accent CTA in the strip, unchanged.
    let cta = if hovered { &c.cta_bg_hover } else { &c.cta_bg };
    ctx.set_fill_color(cta);
    ctx.fill_rounded_rect(
        bp.login_left + inset_x,
        inset_y,
        btn_w - inset_x * 2.0,
        CHROME_HEIGHT - inset_y * 2.0,
        7.0,
    );
    ctx.set_fill_color(contrasting_text_color(cta));
    let label = "Sign in";
    ctx.set_font("600 12px sans-serif");
    ctx.set_text_baseline(TextBaseline::Middle);
    // The button's width never grows with the label (`btn_w` is a fixed
    // cap, mirrored in logic.rs's `ButtonPositions`) — a long account
    // nickname does not push the News/Download icons. What used to go wrong
    // is the fit: centring `label` on the button's midpoint and relying on
    // `clip_rect` to hide the overrun clips it on BOTH sides symmetrically
    // (owner report 2026-09-05) instead of shortening it. A label that fits
    // stays centred like "Sign in" always has; one that doesn't is fit to
    // the padded width with an ellipsis and left-aligned, the same rule
    // `fit_text_to_width` applies to every other fitted label in the app.
    const TEXT_PADDING_X: f64 = 6.0;
    let max_text_w = (btn_w - TEXT_PADDING_X * 2.0).max(0.0);
    let text_w = ctx.measure_text(label);
    ctx.save();
    ctx.clip_rect(bp.login_left, 0.0, btn_w, CHROME_HEIGHT);
    if text_w <= max_text_w {
        ctx.set_text_align(TextAlign::Center);
        ctx.fill_text(label, bp.login_left + btn_w / 2.0, CHROME_HEIGHT / 2.0);
    } else {
        let fitted = fit_text_to_width(ctx, label, max_text_w);
        ctx.set_text_align(TextAlign::Left);
        ctx.fill_text(&fitted, bp.login_left + TEXT_PADDING_X, CHROME_HEIGHT / 2.0);
    }
    ctx.restore();
}

/// Cloud icon buttons — Download, News (left of "Sign in") — one drawing for
/// both flavors. Used to be written twice, once per flavor arm, with
/// identical bodies; kept as one function for the same reason
/// [`draw_account_button`] is: a duplicated widget is exactly the shape a
/// shipped bug took tonight (the account button's two copies disagreed).
///
/// While an installed update waits for a restart the Download button stops
/// meaning "download" and becomes the notice instead: a restart glyph in the
/// theme's accent. The updater reaches that state and the overlay says so —
/// but only while the overlay is open, and closing it left no trace at all
/// while the app went on running the old binary. The control that offered
/// the update is where "it is ready" belongs, and the accent is the one
/// colour in this strip that means something is waiting on you.
#[cfg(feature = "cloud")]
fn draw_cloud_icon_buttons(
    ctx: &mut dyn MlcRenderContext,
    state: &ChromeState,
    c: &ChromeColors,
    bp: &ButtonPositions,
) {
    // The blog panel. A globe, not a bell: the panel behind this button
    // stopped being a notification feed when the links moved into it, and a
    // bell promises something is waiting when usually nothing is.
    {
        let hit = ChromeHit::NewsButton;
        if state.hovered == hit {
            ctx.set_fill_color(&c.button_hover);
            ctx.fill_rounded_rect(
                bp.news_left + 2.0, 4.0,
                CLOUD_ICON_BUTTON_WIDTH - 4.0, CHROME_HEIGHT - 8.0, 6.0,
            );
        }
        let icon_x = bp.news_left + (CLOUD_ICON_BUTTON_WIDTH - 16.0) / 2.0;
        let icon_y = (CHROME_HEIGHT - 16.0) / 2.0;
        if let Some(svg) = icon_svg("globe") {
            draw_svg_icon(ctx, svg, icon_x, icon_y, 16.0, 16.0, &c.icon_normal);
        }
    }

    draw_update_control(ctx, state, c, bp);
}

/// The update control: one button with four readings, and no button at all
/// when there is nothing to say.
///
/// This replaces the toast that used to announce a pending restart. A toast
/// fades and the thing it announced does not — the app went on running the old
/// binary long after the line was gone. The control that would have offered
/// the update is where "it is ready" belongs, and it stays there until it is
/// answered (owner, 2026-09-04).
///
/// On the web this is not an updater at all: the button is the release
/// catalogue, so it is always drawn in its plain state.
///
/// Gated exactly like its only caller (`draw_cloud_icon_buttons`): the
/// control reads `bp.download_left` and calls `logic::update_button_width`,
/// both cloud-only, so without the feature this whole function is dead code.
#[cfg(feature = "cloud")]
fn draw_update_control(
    ctx: &mut dyn MlcRenderContext,
    state: &ChromeState,
    c: &ChromeColors,
    bp: &ButtonPositions,
) {
    let w = super::logic::update_button_width(state.update, state.flavor);
    if w <= 0.0 {
        return;
    }
    let left = bp.download_left;
    let hovered = state.hovered == ChromeHit::DownloadButton;
    let icon_y = (CHROME_HEIGHT - 16.0) / 2.0;

    // Ready to restart: the one accent action in the strip, and it says so in
    // words. The same treatment a signed-out "Sign in" gets — in this strip an
    // accent fill already means the app is asking something of you.
    if state.update == ChromeUpdate::Restart {
        let bg = if hovered { &c.cta_bg_hover } else { &c.cta_bg };
        ctx.set_fill_color(bg);
        ctx.fill_rounded_rect(left + 2.0, 4.0, w - 4.0, CHROME_HEIGHT - 8.0, 7.0);
        let ink = contrasting_text_color(bg);
        if let Some(svg) = icon_svg("refresh") {
            draw_svg_icon(ctx, svg, left + 9.0, icon_y, 16.0, 16.0, ink);
        }
        ctx.set_font("600 12px sans-serif");
        ctx.set_fill_color(ink);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text("Restart", left + 30.0, CHROME_HEIGHT / 2.0);
        return;
    }

    if hovered {
        ctx.set_fill_color(&c.button_hover);
        ctx.fill_rounded_rect(left + 2.0, 4.0, w - 4.0, CHROME_HEIGHT - 8.0, 6.0);
    }

    // Failed: the glyph carries it, the panel behind it carries the message.
    // Loud enough to notice, quiet enough not to nag.
    let ink = if state.update == ChromeUpdate::Failed {
        &c.close_hover
    } else {
        &c.icon_normal
    };
    let icon_x = left + (w - 16.0) / 2.0;
    if let Some(svg) = icon_svg("cloud_download") {
        draw_svg_icon(ctx, svg, icon_x, icon_y, 16.0, 16.0, ink);
    }

    // Working: the progress lives in the button itself rather than in a
    // separate surface. The percentage is known during the download, so the
    // bar fills honestly; verify and install are short and unmeasurable, and
    // there the bar simply sits full.
    if let ChromeUpdate::Working { percent } = state.update {
        let track_w = w - 12.0;
        let track_x = left + 6.0;
        let track_y = CHROME_HEIGHT - 7.0;
        ctx.set_fill_color(&c.separator);
        ctx.fill_rounded_rect(track_x, track_y, track_w, 2.0, 1.0);
        let frac = percent.map(|p| (p as f64 / 100.0).clamp(0.0, 1.0)).unwrap_or(1.0);
        if frac > 0.0 {
            ctx.set_fill_color(&c.cta_bg);
            ctx.fill_rounded_rect(track_x, track_y, track_w * frac, 2.0, 1.0);
        }
    }
}

/// Pick black or white label text for readability over a `#rrggbb` fill,
/// via relative luminance (ITU-R BT.601) — same rule as
/// `mlc_core::drawing::primitives_v2::contrasting_text_color`, kept local
/// because that one is crate-private. Used for the "Sign in" CTA, whose
/// fill is the theme accent (arbitrary per skin).
///
/// Gated exactly like its only caller: the CTA is a cloud-build control, so
/// without the feature this helper is dead code and the mandatory
/// `-D warnings` gate fails on it.
#[cfg(feature = "cloud")]
fn contrasting_text_color(bg_hex: &str) -> &'static str {
    let hex = bg_hex.trim_start_matches('#');
    if hex.len() < 6 {
        return "#ffffff";
    }
    let parse = |s: &str| u8::from_str_radix(s, 16).unwrap_or(128) as f64;
    let r = parse(&hex[0..2]);
    let g = parse(&hex[2..4]);
    let b = parse(&hex[4..6]);
    let luminance = 0.299 * r + 0.587 * g + 0.114 * b;
    if luminance > 150.0 { "#000000" } else { "#ffffff" }
}
