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

use super::types::{
    CHROME_HEIGHT, ChromeColors, ChromeContextMenu, ChromeFlavor, ChromeHit, ChromeState,
};
use crate::tooltip::TooltipState;
use crate::tooltip::calculate_tooltip_position;

// ── Layout constants ──────────────────────────────────────────────────────────

const BUTTON_WIDTH: f64 = 46.0;
const BUTTON_HEIGHT: f64 = CHROME_HEIGHT;
const CLOSE_WINDOW_BUTTON_WIDTH: f64 = 36.0;
const MASCOT_BUTTON_WIDTH: f64 = 0.0;
const MENU_BUTTON_WIDTH: f64 = 36.0;
const NEW_WINDOW_BUTTON_WIDTH: f64 = 36.0;
const NEW_TAB_BUTTON_WIDTH: f64 = 28.0;
/// Gold "Sign in" text button (Web flavor, cloud builds).
#[cfg(feature = "cloud")]
const LOGIN_BUTTON_WIDTH: f64 = 68.0;
/// Icon-sized cloud buttons (Web flavor, cloud builds).
#[cfg(feature = "cloud")]
const CLOUD_ICON_BUTTON_WIDTH: f64 = 30.0;
const TAB_LEFT_MARGIN: f64 = 4.0;
const TAB_GAP: f64 = 1.0;
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
    /// Web flavor: left edge of the "Sign in" button (cloud builds).
    #[cfg(feature = "cloud")]
    login_left: f64,
    #[cfg(feature = "cloud")]
    download_left: f64,
    #[cfg(feature = "cloud")]
    links_left: f64,
    #[cfg(feature = "cloud")]
    news_left: f64,
}

impl ButtonPositions {
    fn compute(width: f64, flavor: ChromeFlavor) -> Self {
        match flavor {
            ChromeFlavor::Native => {
                let close_x           = width - BUTTON_WIDTH;
                let maximize_x        = width - BUTTON_WIDTH * 2.0;
                let minimize_x        = width - BUTTON_WIDTH * 3.0;
                let close_window_left = minimize_x - CLOSE_WINDOW_BUTTON_WIDTH;
                let mascot_left       = close_window_left - MASCOT_BUTTON_WIDTH;
                let menu_left         = mascot_left - MENU_BUTTON_WIDTH;
                let new_window_left   = menu_left - NEW_WINDOW_BUTTON_WIDTH;
                Self {
                    close_x, maximize_x, minimize_x, close_window_left, menu_left, new_window_left,
                    #[cfg(feature = "cloud")]
                    login_left: 0.0,
                    #[cfg(feature = "cloud")]
                    download_left: 0.0,
                    #[cfg(feature = "cloud")]
                    links_left: 0.0,
                    #[cfg(feature = "cloud")]
                    news_left: 0.0,
                }
            }
            ChromeFlavor::Web => {
                // Web: right-to-left — gear, "Sign in", Download, Links, News.
                let menu_left = width - MENU_BUTTON_WIDTH;
                #[cfg(feature = "cloud")]
                let login_left = menu_left - LOGIN_BUTTON_WIDTH;
                #[cfg(feature = "cloud")]
                let download_left = login_left - CLOUD_ICON_BUTTON_WIDTH;
                #[cfg(feature = "cloud")]
                let links_left = download_left - CLOUD_ICON_BUTTON_WIDTH;
                #[cfg(feature = "cloud")]
                let news_left = links_left - CLOUD_ICON_BUTTON_WIDTH;
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
                    links_left,
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
/// (`logic::fit_tab_widths`): tabs shrink proportionally so everything —
/// including the always-visible "+" — stays left of the right-side controls.
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
        let right_edge_of_strip = super::logic::tabs_right_limit(width, state.flavor)
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
    let bp = ButtonPositions::compute(width, state.flavor);

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
        let tabs_limit = super::logic::tabs_right_limit(width, state.flavor);
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

            if tab.active {
                ctx.set_fill_color(&c.tab_accent);
                ctx.fill_rect(tab_left, CHROME_HEIGHT - 3.0, tw, 2.0);
            } else if state.hovered == ChromeHit::Tab(i) {
                // Only the label space lights the hover underline — hovering
                // the overlay buttons (pencil/close) is not hovering the tab.
                ctx.set_fill_color(&c.button_hover);
                ctx.fill_rect(tab_left, CHROME_HEIGHT - 3.0, tw, 2.0);
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
            {
                // The tab name is CENTERED in the full tab body and never
                // moves: the close × paints its own rounded patch on top of
                // it when it appears.
                ctx.set_font("12px sans-serif");
                let text_w = ctx.measure_text(&tab.name);
                let text_x = tab_left + (tw - text_w).max(0.0) / 2.0;
                ctx.set_fill_color(&c.icon_normal);
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.save();
                ctx.clip_rect(tab_left, 0.0, tw, CHROME_HEIGHT);
                ctx.fill_text(&tab.name, text_x, CHROME_HEIGHT / 2.0);
                ctx.restore();
            }

            // Close × at the RIGHT edge, drawn only while the tab is hovered
            // at all; paints over the name beneath it like the pencil does.
            // Same rounded patch, kept clear of the underline strip; the
            // button-hover face only lights when the × itself is hovered.
            let tab_hovered = state.hovered == ChromeHit::Tab(i)
                || state.hovered == ChromeHit::TabClose(i)
                || state.hovered == ChromeHit::TabRename(i);
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
                    ctx.fill_rect(fx, CHROME_HEIGHT - 3.0, tw, 2.0);
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

        let plus_color = if state.hovered == ChromeHit::NewTabButton {
            &c.icon_hover
        } else {
            &c.icon_normal
        };
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
            ctx.set_fill_color(if ov_hovered { &c.icon_hover } else { &c.icon_normal });
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
                    let icon_color = if state.hovered == ChromeHit::NewWindowButton {
                        &c.icon_hover
                    } else {
                        &c.icon_normal
                    };
                    let icon_x = bp.new_window_left + (NEW_WINDOW_BUTTON_WIDTH - 18.0) / 2.0;
                    let icon_y = (CHROME_HEIGHT - 18.0) / 2.0;
                    if let Some(svg) = icon_svg("new_window") {
                        draw_svg_icon(ctx, svg, icon_x, icon_y, 18.0, 18.0, icon_color);
                    }
                }

                // menu cell + icon.
                {
                    if state.hovered == ChromeHit::MenuButton {
                        ctx.set_fill_color(&c.button_hover);
                        ctx.fill_rect(bp.menu_left, 0.0, MENU_BUTTON_WIDTH, BUTTON_HEIGHT);
                    }
                    let icon_color = if state.hovered == ChromeHit::MenuButton {
                        &c.icon_hover
                    } else {
                        &c.icon_normal
                    };
                    let icon_x = bp.menu_left + (MENU_BUTTON_WIDTH - 18.0) / 2.0;
                    let icon_y = (CHROME_HEIGHT - 18.0) / 2.0;
                    if let Some(svg) = icon_svg("settings") {
                        draw_svg_icon(ctx, svg, icon_x, icon_y, 18.0, 18.0, icon_color);
                    }
                }
            }
            ChromeFlavor::Web => {
                // Gold "Sign in" button (cloud builds) — the single accent
                // action in the chrome strip, per the brandbook (gold =
                // primary CTA). Sits immediately left of the gear.
                #[cfg(feature = "cloud")]
                {
                    let gold = if state.hovered == ChromeHit::LoginButton {
                        "#f7d985"
                    } else {
                        "#f4cd63"
                    };
                    let inset_x = 2.0;
                    let inset_y = 4.0;
                    ctx.set_fill_color(gold);
                    ctx.fill_rounded_rect(
                        bp.login_left + inset_x,
                        inset_y,
                        LOGIN_BUTTON_WIDTH - inset_x * 2.0,
                        CHROME_HEIGHT - inset_y * 2.0,
                        7.0,
                    );
                    ctx.set_font("600 12px sans-serif");
                    ctx.set_fill_color("#0a0f1a");
                    ctx.set_text_align(TextAlign::Center);
                    ctx.set_text_baseline(TextBaseline::Middle);
                    ctx.fill_text("Sign in", bp.login_left + LOGIN_BUTTON_WIDTH / 2.0, CHROME_HEIGHT / 2.0);
                }

                // Cloud icon buttons: Download / Links / News (left of Sign in).
                #[cfg(feature = "cloud")]
                {
                    let icon_btns: [(f64, &str, ChromeHit); 3] = [
                        (bp.download_left, "cloud_download", ChromeHit::DownloadButton),
                        (bp.links_left, "globe", ChromeHit::LinksButton),
                        (bp.news_left, "bell", ChromeHit::NewsButton),
                    ];
                    for (left, icon, hit) in icon_btns {
                        if state.hovered == hit {
                            ctx.set_fill_color(&c.button_hover);
                            ctx.fill_rounded_rect(left + 2.0, 4.0, CLOUD_ICON_BUTTON_WIDTH - 4.0, CHROME_HEIGHT - 8.0, 6.0);
                        }
                        let icon_color = if state.hovered == hit {
                            &c.icon_hover
                        } else {
                            &c.icon_normal
                        };
                        let icon_x = left + (CLOUD_ICON_BUTTON_WIDTH - 16.0) / 2.0;
                        let icon_y = (CHROME_HEIGHT - 16.0) / 2.0;
                        if let Some(svg) = icon_svg(icon) {
                            draw_svg_icon(ctx, svg, icon_x, icon_y, 16.0, 16.0, icon_color);
                        }
                    }
                }

                // Menu button (gear) — rightmost.
                {
                    if state.hovered == ChromeHit::MenuButton {
                        ctx.set_fill_color(&c.button_hover);
                        ctx.fill_rect(bp.menu_left, 0.0, MENU_BUTTON_WIDTH, BUTTON_HEIGHT);
                    }
                    let icon_color = if state.hovered == ChromeHit::MenuButton {
                        &c.icon_hover
                    } else {
                        &c.icon_normal
                    };
                    let icon_x = bp.menu_left + (MENU_BUTTON_WIDTH - 18.0) / 2.0;
                    let icon_y = (CHROME_HEIGHT - 18.0) / 2.0;
                    if let Some(svg) = icon_svg("settings") {
                        draw_svg_icon(ctx, svg, icon_x, icon_y, 18.0, 18.0, icon_color);
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
