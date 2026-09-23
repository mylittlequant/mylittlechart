//! Forum v2 (paid boards/threads/search) — what the Community modal's Forum
//! tab draws (`crate::layout::modals::community`, community-modal arc W1;
//! moved out of the News overlay's own `NewsTab::Forum`, retired). Sibling of
//! `cloud_overlay.rs` under `layout::modals`; reuses that file's shared
//! row/status/scroll helpers (`pub(crate)`) rather than duplicating them.
//! State lives in `CommunityState::forum2` (`ForumV2State`); see that type's
//! own doc for the mailbox/freshness contract. Pure presentation — effects
//! (the actual HTTP calls) live in the shell driver; click dispatch lives in
//! `mlc-app`'s `panel_click.rs` (out of scope here). Widget ids keep their
//! pre-existing `cloud_overlay:forum2:*` prefix unchanged — only the Rust
//! type hosting them moved.
//!
//! Drill-down navigation arc (2026-09-23, owner-approved concept
//! `mlc-ui-lab/designs/2026-09-23-forum/forum_concept_v1.html`) replaced W2's
//! fixed-width two-column layout (a permanent boards rail beside every other
//! view — "not a forum, more like a feed", owner review) with a full-width
//! DRILL-DOWN: the Boards view is now a classic board INDEX TABLE spanning
//! the whole pane (`draw_boards_index`), a board opens into its own
//! full-width THREAD LIST table (`draw_board_threads_index`), and a thread
//! opens into the existing flat, numbered post view (`draw_right_thread`,
//! left as-is — a later pass rewrites its own body). A breadcrumb bar under
//! the tab strip (`draw_crumb_bar`, `Forum › Board › Thread`) replaces both
//! the old rail's permanent visibility and every per-view "← Forum" row
//! (`draw_back_row`, deleted) with ONE consistent way back, at every depth;
//! the compact search toggle and (thread-list only) "New thread" button now
//! live on that same bar instead of a per-view pane header
//! (`draw_pane_header`, deleted).
//!
//! Every colour in this file comes from `ToolbarTheme` — the hardcoded
//! `#f4cd63` "gold" this file used to import from `cloud_overlay::GOLD` is
//! gone (owner ruling 2026-09-23: "цвета делай сразу тематические — жёлтый
//! это хардкод, не тема"). Hover states use the same `item_text_hover` token
//! every other row-hover in this crate already reads (`dm_overlay.rs`,
//! `search_overlay.rs`, `toolbar_core.rs`, …); the handful of PERMANENT
//! emphasis marks that used to be gold regardless of hover — a pinned
//! thread's pin glyph, an open thread's own title, the breadcrumb's current
//! segment — read `tt.accent` instead, the one token this crate already
//! reserves for "this is the noteworthy one" (`dm_overlay.rs`'s own "mine"
//! bubble tint is the same idea, generalised here to Forum's own current-page
//! marker).
//!
//! An open thread's reply composer stays DOCKED at the bottom of the pane,
//! with only the thread body/reply tree above it scrolling — same split
//! `dm_overlay.rs`'s own conversation view uses for its send box; that part
//! of `draw_right_thread` is unchanged by this pass.
//!
//! The compact magnifier toggle (owner review 2026-09-09, item d) still
//! expands the shared `FORUM_SEARCH` field in place — now anchored on the
//! breadcrumb bar rather than a per-view header — and the answer still
//! renders as a small dropdown (`draw_quick_search_dropdown`) rather than
//! switching `view`; the full-page `ForumV2View::Search` stays reachable only
//! via the dropdown's own "More results" row.

use mlc_core::engine::render::{draw_svg_icon, RenderContext, TextAlign, TextBaseline};
use mlc_core::i18n::{current_language, ForumKey, UserSettingsKey};
use uzor::input::{InputCoordinator, LayerId, Sense};
use uzor::types::Rect as WidgetRect;
use uzor::WidgetId;

use crate::layout::render_chart::FrameTheme;
use crate::layout::render_ui::toolbar_to_widget_theme;
use crate::ui::modal_settings::{
    BoardUi, CommunityState, ForumReplyV2Ui, ForumSearchHitUi, ForumThreadV2Detail,
    ForumThreadV2Ui, ForumV2State, ForumV2View,
};
use crate::ui::scroll_state::ScrollState;
use crate::ui::scroll_widget::ScrollableContainer;
use crate::ui::toolbar_theme::ToolbarTheme;
use crate::ui::widgets::{draw_input, draw_input_cursor, InputConfig, SETTINGS_BUTTON_RADIUS};
use crate::ui::widgets::types::{WidgetState, WidgetTheme};
use crate::ui::Icon;

use super::cloud_overlay::{
    draw_action_row, draw_expand_row, draw_link_row, draw_status, register_scrollbar_hit_zones,
    relative_time, scroll_config, wrap_text, CloudOverlayResult, ACTION_BTN_GAP, FIELD_H,
    FORUM_BODY_FIELD, FORUM_REPLY_FIELD, FORUM_SEARCH_FIELD, FORUM_TITLE_FIELD, ROW_GAP, ROW_H,
};
use super::community::CommunityListPaneResult;

const SEARCH_HIT_ROW_H: f64 = 64.0;
const LINK_ROW_H: f64 = ROW_H;
/// Reply-tree indent per `ForumReplyV2Ui::depth`, capped at
/// [`REPLY_MAX_DEPTH`] so a deeply nested sub-thread cannot squeeze the
/// content column to nothing.
const REPLY_INDENT: f64 = 16.0;
const REPLY_MAX_DEPTH: i64 = 6;

// ── Breadcrumb bar (drill-down arc 2026-09-23) ───────────────────────────
// Board/thread crumb, the (conditional) "New thread" button, and the
// compact search toggle all share one fixed-height row under the tab strip
// — see `draw_crumb_bar`'s own doc.
const CRUMB_BAR_H: f64 = FIELD_H + 8.0;
const CRUMB_SEP_W: f64 = 14.0;
const QUICK_SEARCH_ICON_BOX: f64 = 28.0;
const QUICK_SEARCH_FIELD_W: f64 = 180.0;
const QUICK_RESULT_ROW_H: f64 = 36.0;
const QUICK_MORE_ROW_H: f64 = 28.0;
const QUICK_DROPDOWN_MAX_ROWS: usize = 8;

// =============================================================================
// Entry points — `list_height` used to size W2's left column; the
// drill-down layout has none, so it is a constant 0.0 kept only because
// `community.rs`'s own `content_height` calls it generically alongside
// `dm_overlay::list_height` (which still has a real left column). `height`
// is the pane's own "wanted" content height (crumb bar + whichever view is
// showing) — used only for `ModalShellSpec`'s inert `content_height` (see
// that call site's own doc for why it is inert); `draw`'s own per-view
// scaffolding computes the SAME numbers again from the same helpers below,
// so the two can never silently diverge from each other.
// =============================================================================

pub(crate) fn list_height(_state: &CommunityState) -> f64 {
    0.0
}

pub(crate) fn height(state: &CommunityState) -> f64 {
    let f = &state.forum2;
    CRUMB_BAR_H
        + match &f.view {
            ForumV2View::Boards => boards_index_h(f),
            ForumV2View::BoardThreads { slug, threads, next_before } => {
                board_threads_index_h(f, slug, threads, next_before)
            }
            ForumV2View::Thread { thread, replies_tree, reply_to, .. } => match thread {
                Some(t) => thread_scroll_content_h(t, replies_tree, f) + thread_composer_h(f, *reply_to),
                None => 44.0,
            },
            ForumV2View::Search { hits, .. } => search_h(f, hits),
            ForumV2View::Compose { .. } => compose_h(f),
        }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn draw(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    viewport_h: f64,
    state: &CommunityState,
    ft: &FrameTheme,
    tt: &ToolbarTheme,
    current_time_ms: u64,
    result: &mut CloudOverlayResult,
    _list_result: &mut CommunityListPaneResult,
) {
    let crumb_h = draw_crumb_bar(ctx, coord, layer_id, x, y, w, state, ft, tt, current_time_ms, result);
    let body_y = y + crumb_h;
    let body_viewport_h = (viewport_h - crumb_h).max(0.0);

    match &state.forum2.view {
        ForumV2View::Boards => draw_right_scrollable(
            ctx, coord, layer_id, x, body_y, w, body_viewport_h, &state.scroll,
            boards_index_h(&state.forum2), result,
            |ctx, coord, layer_id, sx, sy, sw, _result| {
                draw_boards_index(ctx, coord, layer_id, sx, sy, sw, &state.forum2, current_time_ms, tt);
            },
        ),
        ForumV2View::BoardThreads { slug, threads, next_before } => draw_right_scrollable(
            ctx, coord, layer_id, x, body_y, w, body_viewport_h, &state.scroll,
            board_threads_index_h(&state.forum2, slug, threads, next_before), result,
            |ctx, coord, layer_id, sx, sy, sw, _result| {
                draw_board_threads_index(
                    ctx, coord, layer_id, sx, sy, sw, &state.forum2, slug, threads,
                    next_before.as_deref(), current_time_ms, tt,
                );
            },
        ),
        ForumV2View::Thread { thread, replies_tree, reply_to, .. } => draw_right_thread(
            ctx, coord, layer_id, x, body_y, w, body_viewport_h, state, thread.as_ref(),
            replies_tree, *reply_to, ft, tt, current_time_ms, result,
        ),
        ForumV2View::Search { q, hits } => draw_right_scrollable(
            ctx, coord, layer_id, x, body_y, w, body_viewport_h, &state.scroll,
            search_h(&state.forum2, hits), result,
            |ctx, coord, layer_id, sx, sy, sw, _result| {
                draw_search(ctx, coord, layer_id, sx, sy, sw, state, q, hits, current_time_ms, tt);
            },
        ),
        ForumV2View::Compose { slug } => draw_right_scrollable(
            ctx, coord, layer_id, x, body_y, w, body_viewport_h, &state.scroll,
            compose_h(&state.forum2), result,
            |ctx, coord, layer_id, sx, sy, sw, result| {
                draw_compose(ctx, coord, layer_id, sx, sy, sw, state, slug, ft, tt, current_time_ms, result);
            },
        ),
    }

    // Quick-search dropdown — drawn LAST and unclipped (the crumb bar it
    // anchors to sits above every view's own scroll clip), so it overlays
    // whichever view is showing beneath it (concept state 5: the dimmed
    // board index behind a floating result list).
    draw_quick_search_dropdown(ctx, coord, layer_id, result.forum_search_rect, &state.forum2, tt);
}

/// Shared scaffolding for a view with NO docked footer (Boards, BoardThreads,
/// Search, Compose): register the viewport, clip+scroll, run `body`, then
/// publish the scroll geometry into `result`. `Thread`'s own composer-docking
/// split does not use this — it needs a SHORTER scroll region (viewport minus
/// the footer) plus a fixed block below it, so it scaffolds itself inline
/// (`draw_right_thread`).
#[allow(clippy::too_many_arguments)]
fn draw_right_scrollable(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    viewport_h: f64,
    scroll: &ScrollState,
    content_h: f64,
    result: &mut CloudOverlayResult,
    body: impl FnOnce(&mut dyn RenderContext, &mut InputCoordinator, &LayerId, f64, f64, f64, &mut CloudOverlayResult),
) {
    let viewport_rect = WidgetRect::new(x, y, w, viewport_h);
    coord.register_on_layer("cloud_overlay:scroll_viewport", viewport_rect, Sense::SCROLL, layer_id);
    let scrollable = ScrollableContainer::new(viewport_rect, scroll, Some(scroll_config()));
    scrollable.begin(ctx);
    let sx = x;
    let sy = scrollable.content_y();
    let sw = scrollable.content_width();
    body(&mut *ctx, &mut *coord, layer_id, sx, sy, sw, &mut *result);
    let widget_theme = WidgetTheme::default();
    let scroll_result = scrollable.end(ctx, content_h, &widget_theme);
    result.total_content_height = Some(content_h);
    result.viewport_height = Some(viewport_h);
    result.scroll_viewport_rect = Some(viewport_rect);
    result.scrollbar_handle_rect = scroll_result.handle_rect;
    result.scrollbar_track_rect = scroll_result.track_rect;
    register_scrollbar_hit_zones(coord, layer_id, &scroll_result);
}

// =============================================================================
// Shared small widgets — text field, delete row, mini action button, error
// line. Local to this file (unlike `cloud_overlay.rs`'s own
// `draw_action_row`/`draw_expand_row`/`draw_link_row`/`draw_status`, which
// are shared with `dm_overlay.rs` too and so live one level up).
// =============================================================================

/// Draw one `TextFieldStore`-backed field — same idiom `cloud_login.rs`'s
/// redeem-code field and v1's own (deleted) `draw_forum_field` used: an
/// input box plus a blinking caret, with a click zone registered for the
/// mousedown-claims-the-store path (`mlc-app`'s `on_drag_start`).
#[allow(clippy::too_many_arguments)]
fn draw_field(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    widget_id: &str,
    field_id: &str,
    rect: WidgetRect,
    placeholder: &str,
    ft: &FrameTheme,
    tt: &ToolbarTheme,
    current_time_ms: u64,
) -> (WidgetRect, Vec<f64>) {
    let wid = WidgetId::from(field_id);
    let text = coord.text_fields().text(&wid).to_string();
    let cursor = coord.text_fields().cursor(&wid);
    let (sel_start, sel_end) = match coord.text_fields().selection_range(&wid) {
        Some((lo, hi)) => (Some(lo), Some(hi)),
        None => (None, None),
    };
    let engaged = coord.text_fields().is_engaged(&wid);
    let widget_theme = toolbar_to_widget_theme(tt, ft);
    let cfg = InputConfig::new(&text)
        .with_focused(engaged)
        .with_cursor(cursor)
        .with_selection(sel_start, sel_end);
    let cfg = if engaged || !text.is_empty() { cfg } else { cfg.with_placeholder(placeholder) };
    let input_result = draw_input(ctx, &cfg, WidgetState::Normal, rect, &widget_theme);
    if engaged && coord.text_fields().cursor_visible(current_time_ms) {
        draw_input_cursor(ctx, input_result.cursor_x, input_result.cursor_y, input_result.cursor_height, &tt.item_text);
    }
    coord.register_on_layer(widget_id, rect, Sense::CLICK, layer_id);
    (rect, input_result.char_x_positions)
}

/// A destructive action row (own-reply Delete) — neutral surface with the
/// delete icon and a muted-red label. Mirrors v1's own (deleted)
/// `draw_delete_row`.
fn draw_delete_row(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    widget_id: &str,
    rect: WidgetRect,
    label: &str,
    tt: &ToolbarTheme,
) {
    let hovered = coord.is_hovered(&WidgetId::from(widget_id));
    ctx.set_fill_color(if hovered { &tt.item_bg_hover } else { &tt.dropdown_bg });
    ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, SETTINGS_BUTTON_RADIUS);
    draw_svg_icon(ctx, Icon::Delete.svg(), rect.x + 10.0, rect.y + (rect.height - 16.0) / 2.0, 16.0, 16.0, "#e5484d");
    ctx.set_font("13px sans-serif");
    ctx.set_fill_color("#e5484d");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(label, rect.x + 34.0, rect.center_y());
    coord.register_on_layer(widget_id, rect, Sense::CLICK | Sense::HOVER, layer_id);
}

/// A small neutral-surface button with a centred label — used for the
/// per-reply "Reply" row and the thread moderation row's three toggles.
fn draw_mini_button(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    widget_id: &str,
    rect: WidgetRect,
    label: &str,
    tt: &ToolbarTheme,
) {
    let hovered = coord.is_hovered(&WidgetId::from(widget_id));
    ctx.set_fill_color(if hovered { &tt.item_bg_hover } else { &tt.dropdown_bg });
    ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, SETTINGS_BUTTON_RADIUS);
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&tt.item_text);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(label, rect.center_x(), rect.center_y());
    coord.register_on_layer(widget_id, rect, Sense::CLICK | Sense::HOVER, layer_id);
}

/// One clipped red error line — the last request's refusal
/// (`ForumV2State::last_error`). Returns 20.0, matched by every `*_h`
/// function's own `err_h`.
fn draw_error_line(ctx: &mut dyn RenderContext, w: f64, x: f64, y: f64, msg: &str) -> f64 {
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color("#e5484d");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    let m = crate::ui::widgets::fit_text_to_width(ctx, msg, w);
    ctx.fill_text(&m, x, y);
    20.0
}

// =============================================================================
// Rich text blocks — a body/reply split on blank lines, with a lone
// `http(s)://` paragraph rendered as a clickable link row. Local copy of
// v1's own (deleted) `ForumBlock`/`forum_blocks`/`forum_blocks_h`/
// `draw_forum_blocks` — DM bodies are plain single-line bubbles and never
// need this, so it stays here rather than moving up to `cloud_overlay.rs`.
// =============================================================================

enum Block<'a> {
    Text(Vec<String>),
    Link(&'a str),
}

fn blocks(text: &str) -> Vec<Block<'_>> {
    text.split("\n\n")
        .map(|par| {
            let trimmed = par.trim();
            let is_link = trimmed.lines().count() == 1
                && (trimmed.starts_with("http://") || trimmed.starts_with("https://"))
                && !trimmed.contains(char::is_whitespace);
            if is_link {
                Block::Link(trimmed)
            } else {
                Block::Text(wrap_text(trimmed, 62))
            }
        })
        .collect()
}

fn blocks_h(text: &str) -> f64 {
    blocks(text)
        .iter()
        .map(|b| match b {
            Block::Text(lines) => lines.len() as f64 * 16.0 + 10.0,
            Block::Link(_) => LINK_ROW_H,
        })
        .sum()
}

fn draw_blocks(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    widget_prefix: &str,
    x: f64,
    y: f64,
    w: f64,
    text: &str,
    tt: &ToolbarTheme,
) -> f64 {
    let mut ly = y;
    for block in blocks(text) {
        match block {
            Block::Text(lines) => {
                ctx.set_font("12px sans-serif");
                ctx.set_fill_color(&tt.item_text);
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Top);
                for line in &lines {
                    ctx.fill_text(line, x, ly);
                    ly += 16.0;
                }
                ly += 10.0;
            }
            Block::Link(url) => {
                let rect = WidgetRect::new(x, ly, w, LINK_ROW_H - ROW_GAP);
                draw_link_row(ctx, coord, layer_id, &format!("{widget_prefix}:link:{url}"), rect, url, Icon::Globe, tt);
                ly += LINK_ROW_H;
            }
        }
    }
    ly - y
}

// =============================================================================
// Board lookups — shared by the breadcrumb bar and the BoardThreads view.
// =============================================================================

fn board_can_write(f: &ForumV2State, slug: &str) -> bool {
    f.boards.iter().find(|b| b.slug == slug).map(|b| b.can_write).unwrap_or(false)
}

fn board_title<'a>(f: &'a ForumV2State, slug: &'a str) -> &'a str {
    f.boards.iter().find(|b| b.slug == slug).map(|b| b.title.as_str()).unwrap_or(slug)
}

// =============================================================================
// Breadcrumb bar (drill-down arc 2026-09-23) — `Forum [› Board] [› Thread]`
// on the left, the compact search toggle (and, on the thread-list view only,
// "New thread") on the right. Replaces W2's per-view `draw_pane_header` AND
// every view's own "← Forum" row (`draw_back_row`, deleted) with one
// consistent way back, reachable from any depth. The CURRENT segment (the
// page the caller is actually on) reads `tt.accent`; every other segment is
// a real navigation link — muted, brightening to `item_text_hover` on hover,
// same as any other clickable row in this crate.
// =============================================================================

/// One breadcrumb segment: `widget_id = Some(id)` makes it a clickable link
/// (muted, brightens on hover); `None` marks the CURRENT page (accent,
/// static — the concept's own `.seg.cur`, which carries no hover style
/// either). Returns the text width consumed, so the caller can chain
/// segments left to right.
#[allow(clippy::too_many_arguments)]
fn draw_crumb_segment(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    widget_id: Option<&str>,
    x: f64,
    y: f64,
    h: f64,
    label: &str,
    max_w: f64,
    tt: &ToolbarTheme,
) -> f64 {
    let current = widget_id.is_none();
    ctx.set_font(if current { "600 11px monospace" } else { "11px monospace" });
    let label = crate::ui::widgets::fit_text_to_width(ctx, label, max_w);
    let hovered = widget_id.map(|id| coord.is_hovered(&WidgetId::from(id))).unwrap_or(false);
    if current {
        ctx.set_fill_color(&tt.accent);
    } else if hovered {
        ctx.set_fill_color(&tt.item_text_hover);
    } else {
        ctx.set_fill_color(&tt.item_text_muted);
    }
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(&label, x, y + h / 2.0);
    let text_w = ctx.measure_text(&label);
    if let Some(id) = widget_id {
        let rect = WidgetRect::new(x, y + 4.0, text_w.max(1.0), h - 8.0);
        coord.register_on_layer(id, rect, Sense::CLICK | Sense::HOVER, layer_id);
    }
    text_w
}

fn draw_crumb_sep(ctx: &mut dyn RenderContext, x: f64, y: f64, h: f64, tt: &ToolbarTheme) {
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(&tt.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("\u{203a}", x, y + h / 2.0);
}

/// Draws the breadcrumb bar and returns its height. `result.forum_search_rect`
/// carries the inline search field's rect while it is open (the dropdown's
/// own anchor) — left untouched (so it stays `None` on a fresh per-frame
/// result) when the field is closed.
#[allow(clippy::too_many_arguments)]
fn draw_crumb_bar(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    state: &CommunityState,
    ft: &FrameTheme,
    tt: &ToolbarTheme,
    current_time_ms: u64,
    result: &mut CloudOverlayResult,
) -> f64 {
    let h = CRUMB_BAR_H;
    let f = &state.forum2;
    let mut right_edge = x + w;

    // Search toggle — always the rightmost control.
    let icon_rect = WidgetRect::new(right_edge - QUICK_SEARCH_ICON_BOX, y + (h - QUICK_SEARCH_ICON_BOX) / 2.0, QUICK_SEARCH_ICON_BOX, QUICK_SEARCH_ICON_BOX);
    let icon_hovered = coord.is_hovered(&WidgetId::from("cloud_overlay:forum2:quick_search_toggle"));
    if icon_hovered || f.quick_search_open {
        ctx.set_fill_color(&tt.item_bg_hover);
        ctx.fill_rounded_rect(icon_rect.x, icon_rect.y, icon_rect.width, icon_rect.height, SETTINGS_BUTTON_RADIUS);
    }
    draw_svg_icon(ctx, Icon::Search.svg(), icon_rect.x + 6.0, icon_rect.y + 6.0, 16.0, 16.0, &tt.item_text);
    coord.register_on_layer("cloud_overlay:forum2:quick_search_toggle", icon_rect, Sense::CLICK | Sense::HOVER, layer_id);
    right_edge -= QUICK_SEARCH_ICON_BOX + 8.0;

    if f.quick_search_open {
        let field_w = QUICK_SEARCH_FIELD_W.min((right_edge - x).max(60.0));
        let field_rect = WidgetRect::new(right_edge - field_w, y + (h - FIELD_H) / 2.0, field_w, FIELD_H);
        let (rect, chars) = draw_field(
            ctx, coord, layer_id, "cloud_overlay:forum2:quick_search_input", FORUM_SEARCH_FIELD, field_rect,
            "Search…", ft, tt, current_time_ms,
        );
        result.forum_search_rect = Some(rect);
        result.forum_search_char_positions = chars;
        right_edge -= field_w + 8.0;
    }

    // "New thread" — the thread-list view only, and only when the board's
    // `can_write` is true (owner ruling: never for signed-out).
    if let ForumV2View::BoardThreads { slug, .. } = &f.view {
        if board_can_write(f, slug) {
            let btn_w = 92.0f64.min((right_edge - x).max(0.0));
            let btn_rect = WidgetRect::new(right_edge - btn_w, y + (h - (ROW_H - ROW_GAP)) / 2.0, btn_w, ROW_H - ROW_GAP);
            draw_action_row(ctx, coord, layer_id, "cloud_overlay:forum2:new_thread", btn_rect, "New thread", tt);
            right_edge -= btn_w + 8.0;
        }
    }

    // Breadcrumb segments, left to right.
    let mut cx = x + 8.0;
    let seg_max_w = |cx: f64, right_edge: f64| (right_edge - cx - 4.0).max(10.0);
    let on_boards = matches!(f.view, ForumV2View::Boards);
    cx += draw_crumb_segment(
        ctx, coord, layer_id, if on_boards { None } else { Some("cloud_overlay:forum2:crumb_forum") },
        cx, y, h, "Forum", seg_max_w(cx, right_edge), tt,
    );

    match &f.view {
        ForumV2View::Boards => {}
        ForumV2View::BoardThreads { slug, .. } => {
            draw_crumb_sep(ctx, cx, y, h, tt);
            cx += CRUMB_SEP_W;
            cx += draw_crumb_segment(ctx, coord, layer_id, None, cx, y, h, board_title(f, slug), seg_max_w(cx, right_edge), tt);
        }
        ForumV2View::Thread { thread, .. } => {
            if let Some(slug) = &f.open_thread_board_slug {
                draw_crumb_sep(ctx, cx, y, h, tt);
                cx += CRUMB_SEP_W;
                let id = format!("cloud_overlay:forum2:crumb_board:{slug}");
                cx += draw_crumb_segment(ctx, coord, layer_id, Some(&id), cx, y, h, board_title(f, slug), seg_max_w(cx, right_edge), tt);
            }
            draw_crumb_sep(ctx, cx, y, h, tt);
            cx += CRUMB_SEP_W;
            let title = thread.as_ref().map(|t| t.title.as_str()).unwrap_or("…");
            cx += draw_crumb_segment(ctx, coord, layer_id, None, cx, y, h, title, seg_max_w(cx, right_edge), tt);
        }
        ForumV2View::Search { .. } => {
            draw_crumb_sep(ctx, cx, y, h, tt);
            cx += CRUMB_SEP_W;
            cx += draw_crumb_segment(ctx, coord, layer_id, None, cx, y, h, "Search", seg_max_w(cx, right_edge), tt);
        }
        ForumV2View::Compose { slug } => {
            draw_crumb_sep(ctx, cx, y, h, tt);
            cx += CRUMB_SEP_W;
            let id = format!("cloud_overlay:forum2:crumb_board:{slug}");
            cx += draw_crumb_segment(ctx, coord, layer_id, Some(&id), cx, y, h, board_title(f, slug), seg_max_w(cx, right_edge), tt);
            draw_crumb_sep(ctx, cx, y, h, tt);
            cx += CRUMB_SEP_W;
            cx += draw_crumb_segment(ctx, coord, layer_id, None, cx, y, h, "New thread", seg_max_w(cx, right_edge), tt);
        }
    }
    let _ = cx;

    h
}

/// Drawn LAST over whatever view is showing — anchored under `field_rect`
/// (the breadcrumb bar's own field, `None` when the box is closed, in which
/// case this is a no-op). Unclipped: the breadcrumb bar sits above every
/// view's own scroll clip, so its dropdown is free to overlay the full body
/// beneath it (concept state 5).
fn draw_quick_search_dropdown(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    field_rect: Option<WidgetRect>,
    f: &ForumV2State,
    tt: &ToolbarTheme,
) {
    let Some(anchor) = field_rect else { return };
    if !f.quick_search_open || f.quick_search_query.is_none() {
        return;
    }
    let n = f.quick_search_results.len().min(QUICK_DROPDOWN_MAX_ROWS);
    let rows_h = if n == 0 { QUICK_RESULT_ROW_H } else { n as f64 * QUICK_RESULT_ROW_H };
    let box_h = rows_h + QUICK_MORE_ROW_H + 8.0;
    let box_w = (anchor.width * 1.6).max(260.0);
    let box_x = (anchor.x + anchor.width - box_w).max(0.0);
    let box_y = anchor.y + anchor.height + 4.0;

    ctx.set_fill_color(&tt.dropdown_bg);
    ctx.fill_rounded_rect(box_x, box_y, box_w, box_h, SETTINGS_BUTTON_RADIUS);

    let mut ry = box_y + 4.0;
    if n == 0 {
        draw_status(ctx, tt, box_x, ry, box_w, "No results.", None);
        ry += QUICK_RESULT_ROW_H;
    } else {
        for (i, hit) in f.quick_search_results.iter().take(n).enumerate() {
            let widget_id = format!("cloud_overlay:forum2:quick_result:{i}");
            let rect = WidgetRect::new(box_x + 4.0, ry, box_w - 8.0, QUICK_RESULT_ROW_H - 4.0);
            let hovered = coord.is_hovered(&WidgetId::from(widget_id.as_str()));
            if hovered {
                ctx.set_fill_color(&tt.item_bg_hover);
                ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, SETTINGS_BUTTON_RADIUS);
            }
            let heading = hit.title.clone().unwrap_or_else(|| format!("Reply in thread #{}", hit.thread_id));
            ctx.set_font("600 12px sans-serif");
            ctx.set_fill_color(if hovered { &tt.item_text_hover } else { &tt.item_text });
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Top);
            let heading = crate::ui::widgets::fit_text_to_width(ctx, &heading, rect.width - 12.0);
            ctx.fill_text(&heading, rect.x + 6.0, rect.y + 2.0);

            ctx.set_font("11px sans-serif");
            ctx.set_fill_color(&tt.item_text_muted);
            let snippet = crate::ui::widgets::fit_text_to_width(ctx, &hit.snippet, rect.width - 12.0);
            ctx.fill_text(&snippet, rect.x + 6.0, rect.y + 18.0);

            coord.register_on_layer(widget_id.as_str(), rect, Sense::CLICK | Sense::HOVER, layer_id);
            ry += QUICK_RESULT_ROW_H;
        }
    }

    let more_rect = WidgetRect::new(box_x + 4.0, ry, box_w - 8.0, QUICK_MORE_ROW_H - 4.0);
    let more_hovered = coord.is_hovered(&WidgetId::from("cloud_overlay:forum2:quick_more"));
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(if more_hovered { &tt.item_text_hover } else { &tt.item_text_muted });
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("More results…", more_rect.center_x(), more_rect.center_y());
    coord.register_on_layer("cloud_overlay:forum2:quick_more", more_rect, Sense::CLICK | Sense::HOVER, layer_id);
}

// =============================================================================
// Boards view (`ForumV2View::Boards`) — a full-width board INDEX TABLE
// (concept state 1): totals line, then one row per board (name, description,
// a "write: <class>" tag when the caller cannot write, thread/post counts,
// last post). Replaces W2's fixed-width rail (`draw_board_list`, deleted) —
// the boards list is no longer pinned beside every other view, it IS the
// Forum tab's own landing view, reached back via the breadcrumb bar's
// "Forum" segment.
// =============================================================================

const BOARD_IDX_ROW_H: f64 = 60.0;
const BOARD_IDX_THREADS_COL_W: f64 = 64.0;
const BOARD_IDX_POSTS_COL_W: f64 = 64.0;
const BOARD_IDX_LASTPOST_COL_W: f64 = 210.0;
const TABLE_HEADER_H: f64 = 20.0;
const TABLE_COL_GAP: f64 = 12.0;

fn totals_header_h() -> f64 {
    20.0
}

fn draw_totals_header(ctx: &mut dyn RenderContext, x: f64, y: f64, w: f64, f: &ForumV2State, tt: &ToolbarTheme) -> f64 {
    let boards = f.boards.len();
    let threads: i64 = f.boards.iter().map(|b| b.thread_count).sum();
    let posts: i64 = threads + f.boards.iter().map(|b| b.reply_count).sum::<i64>();
    ctx.set_font("11px monospace");
    ctx.set_fill_color(&tt.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    let label = format!("{boards} boards · {threads} threads · {posts} posts");
    let label = crate::ui::widgets::fit_text_to_width(ctx, &label, w);
    ctx.fill_text(&label, x, y);
    totals_header_h()
}

fn boards_index_h(f: &ForumV2State) -> f64 {
    let err_h = if f.last_error.is_some() { 20.0 } else { 0.0 };
    let header_h = totals_header_h() + 8.0 + TABLE_HEADER_H + 6.0;
    let rows_h = if f.boards.is_empty() { 24.0 } else { f.boards.len() as f64 * BOARD_IDX_ROW_H };
    err_h + header_h + rows_h
}

/// Column x-positions shared by the header row and every board row, so the
/// two can never drift apart.
struct BoardIdxCols {
    title_w: f64,
    threads_x: f64,
    posts_x: f64,
    last_post_x: f64,
}

fn board_idx_cols(x: f64, w: f64) -> BoardIdxCols {
    let last_post_x = x + w - BOARD_IDX_LASTPOST_COL_W;
    let posts_x = last_post_x - TABLE_COL_GAP - BOARD_IDX_POSTS_COL_W;
    let threads_x = posts_x - TABLE_COL_GAP - BOARD_IDX_THREADS_COL_W;
    let title_w = (threads_x - TABLE_COL_GAP - x).max(80.0);
    BoardIdxCols { title_w, threads_x, posts_x, last_post_x }
}

fn draw_board_idx_header(ctx: &mut dyn RenderContext, x: f64, y: f64, w: f64, tt: &ToolbarTheme) {
    let cols = board_idx_cols(x, w);
    let lang = current_language();
    ctx.set_font("500 10px sans-serif");
    ctx.set_fill_color(&tt.item_text_muted);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.set_text_align(TextAlign::Left);
    ctx.fill_text(ForumKey::ColBoard.get(lang), x + 8.0, y);
    ctx.set_text_align(TextAlign::Right);
    ctx.fill_text(ForumKey::ColThreads.get(lang), cols.threads_x + BOARD_IDX_THREADS_COL_W, y);
    ctx.fill_text(ForumKey::ColPosts.get(lang), cols.posts_x + BOARD_IDX_POSTS_COL_W, y);
    ctx.set_text_align(TextAlign::Left);
    ctx.fill_text(ForumKey::ColLastPost.get(lang), cols.last_post_x, y);
}

fn draw_board_idx_row(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    board: &BoardUi,
    current_time_ms: u64,
    tt: &ToolbarTheme,
) -> f64 {
    let cols = board_idx_cols(x, w);
    let widget_id = format!("cloud_overlay:forum2:board:{}", board.slug);
    let rect = WidgetRect::new(x, y, w, BOARD_IDX_ROW_H - ROW_GAP);
    let hovered = coord.is_hovered(&WidgetId::from(widget_id.as_str()));
    if hovered {
        ctx.set_fill_color(&tt.item_bg_hover);
        ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, SETTINGS_BUTTON_RADIUS);
    }

    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.set_font("600 13px sans-serif");
    ctx.set_fill_color(if hovered { &tt.item_text_hover } else { &tt.item_text });
    let title = crate::ui::widgets::fit_text_to_width(ctx, &board.title, cols.title_w - 8.0);
    ctx.fill_text(&title, x + 8.0, y + 2.0);

    // "write: <class>" tag — only when the caller CANNOT write (owner
    // review 2026-09-09: quiet when they can), drawn right after the title
    // on the same line rather than its own row (concept's own `.b-tag`).
    if !board.can_write {
        let title_w = ctx.measure_text(&title);
        let tag_x = x + 8.0 + title_w + 8.0;
        let tag_budget = (cols.title_w - 8.0 - title_w - 8.0).max(0.0);
        if tag_budget > 12.0 {
            ctx.set_font("10px monospace");
            let tag_label = crate::ui::widgets::fit_text_to_width(ctx, &format!("write: {}", board.write_class), tag_budget - 10.0);
            let tag_w = ctx.measure_text(&tag_label) + 10.0;
            ctx.set_fill_color(&tt.dropdown_bg);
            ctx.fill_rounded_rect(tag_x, y + 2.0, tag_w, 14.0, 3.0);
            ctx.set_fill_color(&tt.item_text_muted);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(&tag_label, tag_x + 5.0, y + 9.0);
            ctx.set_text_baseline(TextBaseline::Top);
        }
    }

    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&tt.item_text_muted);
    let desc = crate::ui::widgets::fit_text_to_width(ctx, &board.description, cols.title_w - 8.0);
    ctx.fill_text(&desc, x + 8.0, y + 20.0);

    ctx.set_font("12px monospace");
    ctx.set_fill_color(&tt.item_text_muted);
    ctx.set_text_align(TextAlign::Right);
    ctx.fill_text(&board.thread_count.to_string(), cols.threads_x + BOARD_IDX_THREADS_COL_W, y + 2.0);
    let posts = board.thread_count + board.reply_count;
    ctx.fill_text(&posts.to_string(), cols.posts_x + BOARD_IDX_POSTS_COL_W, y + 2.0);
    ctx.set_text_align(TextAlign::Left);

    match (&board.last_thread_title, &board.last_post_at) {
        (Some(title), Some(t)) => {
            ctx.set_font("12px sans-serif");
            ctx.set_fill_color(&tt.item_text);
            let title = crate::ui::widgets::fit_text_to_width(ctx, title, BOARD_IDX_LASTPOST_COL_W - 8.0);
            ctx.fill_text(&title, cols.last_post_x, y + 2.0);
            ctx.set_font("10px monospace");
            ctx.set_fill_color(&tt.item_text_muted);
            ctx.fill_text(&relative_time(t, current_time_ms), cols.last_post_x, y + 20.0);
        }
        _ => {
            ctx.set_font("11px sans-serif");
            ctx.set_fill_color(&tt.item_text_muted);
            ctx.fill_text("No posts yet", cols.last_post_x, y + 2.0);
        }
    }

    coord.register_on_layer(widget_id.as_str(), rect, Sense::CLICK | Sense::HOVER, layer_id);
    BOARD_IDX_ROW_H
}

fn draw_boards_index(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    f: &ForumV2State,
    current_time_ms: u64,
    tt: &ToolbarTheme,
) {
    let mut py = y;
    if let Some(err) = &f.last_error {
        py += draw_error_line(ctx, w, x, py, err);
    }
    py += draw_totals_header(ctx, x, py, w, f, tt);
    py += 8.0;
    if f.boards.is_empty() {
        draw_status(ctx, tt, x, py, w, "No boards yet.", None);
        return;
    }
    draw_board_idx_header(ctx, x, py, w, tt);
    py += TABLE_HEADER_H + 6.0;
    for b in &f.boards {
        py += draw_board_idx_row(ctx, coord, layer_id, x, py, w, b, current_time_ms, tt);
    }
}

// =============================================================================
// BoardThreads view (`ForumV2View::BoardThreads`) — a full-width THREAD LIST
// table (concept state 2): title (pin/lock badges), author, replies, last
// activity. "New thread" and the board's own description live on/under the
// breadcrumb bar (`draw_crumb_bar`), not here.
// =============================================================================

const THREAD_IDX_ROW_H: f64 = 40.0;
const THREAD_IDX_AUTHOR_COL_W: f64 = 130.0;
const THREAD_IDX_REPLIES_COL_W: f64 = 64.0;
const THREAD_IDX_ACT_COL_W: f64 = 180.0;

struct ThreadIdxCols {
    title_w: f64,
    author_x: f64,
    replies_x: f64,
    act_x: f64,
}

fn thread_idx_cols(x: f64, w: f64) -> ThreadIdxCols {
    let act_x = x + w - THREAD_IDX_ACT_COL_W;
    let replies_x = act_x - TABLE_COL_GAP - THREAD_IDX_REPLIES_COL_W;
    let author_x = replies_x - TABLE_COL_GAP - THREAD_IDX_AUTHOR_COL_W;
    let title_w = (author_x - TABLE_COL_GAP - x).max(80.0);
    ThreadIdxCols { title_w, author_x, replies_x, act_x }
}

fn draw_thread_idx_header(ctx: &mut dyn RenderContext, x: f64, y: f64, w: f64, tt: &ToolbarTheme) {
    let cols = thread_idx_cols(x, w);
    let lang = current_language();
    ctx.set_font("500 10px sans-serif");
    ctx.set_fill_color(&tt.item_text_muted);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.set_text_align(TextAlign::Left);
    ctx.fill_text(ForumKey::ColTitle.get(lang), x + 8.0, y);
    ctx.fill_text(ForumKey::ColAuthor.get(lang), cols.author_x, y);
    ctx.set_text_align(TextAlign::Right);
    ctx.fill_text(ForumKey::ColReplies.get(lang), cols.replies_x + THREAD_IDX_REPLIES_COL_W, y);
    ctx.set_text_align(TextAlign::Left);
    ctx.fill_text(ForumKey::ColLastActivity.get(lang), cols.act_x, y);
}

fn draw_thread_idx_row(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    t: &ForumThreadV2Ui,
    current_time_ms: u64,
    tt: &ToolbarTheme,
) -> f64 {
    let cols = thread_idx_cols(x, w);
    let widget_id = format!("cloud_overlay:forum2:thread:{}", t.id);
    let rect = WidgetRect::new(x, y, w, THREAD_IDX_ROW_H - ROW_GAP);
    let cy = rect.center_y();
    let hovered = coord.is_hovered(&WidgetId::from(widget_id.as_str()));
    if hovered {
        ctx.set_fill_color(&tt.item_bg_hover);
        ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, SETTINGS_BUTTON_RADIUS);
    }

    let mut title_x = x + 8.0;
    if t.pinned {
        draw_svg_icon(ctx, Icon::Pin.svg(), title_x, cy - 7.0, 14.0, 14.0, &tt.accent);
        title_x += 18.0;
    }
    if t.locked {
        draw_svg_icon(ctx, Icon::Lock.svg(), title_x, cy - 7.0, 14.0, 14.0, &tt.item_text_muted);
        title_x += 18.0;
    }
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.set_font("600 13px sans-serif");
    ctx.set_fill_color(if hovered { &tt.item_text_hover } else { &tt.item_text });
    let title_w = (cols.title_w - (title_x - x) - 8.0).max(20.0);
    let title = crate::ui::widgets::fit_text_to_width(ctx, &t.title, title_w);
    ctx.fill_text(&title, title_x, cy);

    ctx.set_font("12px monospace");
    ctx.set_fill_color(&tt.item_text_muted);
    let author = crate::ui::widgets::fit_text_to_width(ctx, &t.author, THREAD_IDX_AUTHOR_COL_W - 8.0);
    ctx.fill_text(&author, cols.author_x, cy);

    ctx.set_text_align(TextAlign::Right);
    ctx.set_fill_color(&tt.item_text_muted);
    ctx.fill_text(&t.reply_count.to_string(), cols.replies_x + THREAD_IDX_REPLIES_COL_W, cy);
    ctx.set_text_align(TextAlign::Left);

    // The server does not (yet) return who posted the LAST reply — only
    // `last_reply_at`'s timestamp — so this column, like v1's own meta line
    // before it, reads the thread's own author alongside the latest
    // activity time rather than a wrong or invented name.
    let activity = t.last_reply_at.as_deref().unwrap_or(&t.created_at);
    let act = format!("{} · {}", t.author, relative_time(activity, current_time_ms));
    ctx.set_font("11px monospace");
    ctx.set_fill_color(&tt.item_text_muted);
    let act = crate::ui::widgets::fit_text_to_width(ctx, &act, THREAD_IDX_ACT_COL_W - 8.0);
    ctx.fill_text(&act, cols.act_x, cy);

    coord.register_on_layer(widget_id.as_str(), rect, Sense::CLICK | Sense::HOVER, layer_id);
    THREAD_IDX_ROW_H
}

fn board_threads_index_h(f: &ForumV2State, slug: &str, threads: &[ForumThreadV2Ui], next_before: &Option<String>) -> f64 {
    let err_h = if f.last_error.is_some() { 20.0 } else { 0.0 };
    let desc_h = if f.boards.iter().any(|b| b.slug == slug) { 20.0 } else { 0.0 };
    let header_h = TABLE_HEADER_H + 6.0;
    let list_h = if threads.is_empty() { 24.0 } else { threads.len() as f64 * THREAD_IDX_ROW_H };
    let more_h = if next_before.is_some() { ROW_H } else { 0.0 };
    err_h + desc_h + header_h + list_h + more_h
}

#[allow(clippy::too_many_arguments)]
fn draw_board_threads_index(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    f: &ForumV2State,
    slug: &str,
    threads: &[ForumThreadV2Ui],
    next_before: Option<&str>,
    current_time_ms: u64,
    tt: &ToolbarTheme,
) {
    let mut py = y;
    if let Some(err) = &f.last_error {
        py += draw_error_line(ctx, w, x, py, err);
    }
    if let Some(board) = f.boards.iter().find(|b| b.slug == slug) {
        ctx.set_font("12px sans-serif");
        ctx.set_fill_color(&tt.item_text_muted);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
        let desc = crate::ui::widgets::fit_text_to_width(ctx, &board.description, w);
        ctx.fill_text(&desc, x, py);
        py += 20.0;
    }
    if threads.is_empty() {
        draw_status(ctx, tt, x, py, w, "No threads yet.", None);
        return;
    }
    draw_thread_idx_header(ctx, x, py, w, tt);
    py += TABLE_HEADER_H + 6.0;
    // Pinned first — `sort_by_key` is stable, so relative order within each
    // of the two groups (as the server returned it) survives.
    let mut ordered: Vec<&ForumThreadV2Ui> = threads.iter().collect();
    ordered.sort_by_key(|t| !t.pinned);
    for t in ordered {
        py += draw_thread_idx_row(ctx, coord, layer_id, x, py, w, t, current_time_ms, tt);
    }
    if next_before.is_some() {
        draw_expand_row(
            ctx, coord, layer_id, "cloud_overlay:forum2:load_more",
            WidgetRect::new(x, py, w, ROW_H - ROW_GAP), "Load more", false, tt,
        );
    }
}

// =============================================================================
// Thread view (`ForumV2View::Thread`): mod row, body, replies tree scroll
// ABOVE a docked composer (error line, reply-to indicator, REPLY field +
// submit) fixed to the bottom of the pane — same split `dm_overlay.rs`'s own
// conversation view uses for its send box. Left as-is by the drill-down
// pass (a later pass rewrites this view's own body) beyond the breadcrumb
// bar now sitting above it and the GOLD→`tt.accent` colour swap.
// =============================================================================

fn reply_node_h(node: &ForumReplyV2Ui) -> f64 {
    let mut h = 16.0; // meta line
    if node.hidden {
        h += 16.0 + 10.0;
    } else {
        h += blocks_h(&node.body);
        h += ROW_H; // Reply / Delete row
    }
    h += 10.0; // gap after this node
    h + node.children.iter().map(reply_node_h).sum::<f64>()
}

fn reply_tree_h(nodes: &[ForumReplyV2Ui]) -> f64 {
    nodes.iter().map(reply_node_h).sum()
}

#[allow(clippy::too_many_arguments)]
fn draw_reply_node(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    node: &ForumReplyV2Ui,
    reply_to: Option<i64>,
    current_time_ms: u64,
    tt: &ToolbarTheme,
) -> f64 {
    let indent = node.depth.clamp(0, REPLY_MAX_DEPTH) as f64 * REPLY_INDENT;
    let ix = x + indent;
    let iw = (w - indent).max(40.0);
    let mut ly = y;

    ctx.set_font("10px monospace");
    ctx.set_fill_color(&tt.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(&format!("{} · {}", node.author, relative_time(&node.created_at, current_time_ms)), ix, ly);
    ly += 16.0;

    if node.hidden {
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(&tt.item_text_muted);
        ctx.fill_text("[hidden by moderator]", ix, ly);
        ly += 16.0 + 10.0;
    } else {
        ly += draw_blocks(ctx, coord, layer_id, &format!("cloud_overlay:forum2:reply:{}", node.id), ix, ly, iw, &node.body, tt);

        let targeted = reply_to == Some(node.id);
        let reply_label = if targeted { "Replying…" } else { "Reply" };
        let reply_w = if node.author_is_me { (iw - ACTION_BTN_GAP) / 2.0 } else { iw };
        draw_mini_button(
            ctx, coord, layer_id, &format!("cloud_overlay:forum2:reply_to:{}", node.id),
            WidgetRect::new(ix, ly, reply_w, ROW_H - ROW_GAP), reply_label, tt,
        );
        if node.author_is_me {
            draw_delete_row(
                ctx, coord, layer_id, &format!("cloud_overlay:forum2:delete_reply:{}", node.id),
                WidgetRect::new(ix + reply_w + ACTION_BTN_GAP, ly, reply_w, ROW_H - ROW_GAP), "Delete", tt,
            );
        }
        ly += ROW_H;
    }
    ly += 10.0;

    for child in &node.children {
        ly += draw_reply_node(ctx, coord, layer_id, x, ly, w, child, reply_to, current_time_ms, tt);
    }
    ly - y
}

#[allow(clippy::too_many_arguments)]
fn draw_reply_tree(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    nodes: &[ForumReplyV2Ui],
    reply_to: Option<i64>,
    current_time_ms: u64,
    tt: &ToolbarTheme,
) -> f64 {
    let mut ly = y;
    for node in nodes {
        ly += draw_reply_node(ctx, coord, layer_id, x, ly, w, node, reply_to, current_time_ms, tt);
    }
    ly - y
}

/// The moderator/admin-only row: three toggles (Hide/Unhide, Lock/Unlock,
/// Pin/Unpin) — each posts `ForumV2Request::Moderate` with the matching
/// action string. Drawn only when `ForumV2State::is_moderator_or_admin`.
fn draw_mod_row(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    t: &ForumThreadV2Detail,
    tt: &ToolbarTheme,
) {
    let seg_w = (w - ACTION_BTN_GAP * 2.0) / 3.0;
    let (hide_label, hide_action) = if t.hidden { ("Unhide", "unhide") } else { ("Hide", "hide") };
    let (lock_label, lock_action) = if t.locked { ("Unlock", "unlock") } else { ("Lock", "lock") };
    let (pin_label, pin_action) = if t.pinned { ("Unpin", "unpin") } else { ("Pin", "pin") };
    draw_mini_button(
        ctx, coord, layer_id, &format!("cloud_overlay:forum2:mod:{hide_action}:{}", t.id),
        WidgetRect::new(x, y, seg_w, ROW_H - ROW_GAP), hide_label, tt,
    );
    draw_mini_button(
        ctx, coord, layer_id, &format!("cloud_overlay:forum2:mod:{lock_action}:{}", t.id),
        WidgetRect::new(x + seg_w + ACTION_BTN_GAP, y, seg_w, ROW_H - ROW_GAP), lock_label, tt,
    );
    draw_mini_button(
        ctx, coord, layer_id, &format!("cloud_overlay:forum2:mod:{pin_action}:{}", t.id),
        WidgetRect::new(x + 2.0 * (seg_w + ACTION_BTN_GAP), y, seg_w, ROW_H - ROW_GAP), pin_label, tt,
    );
}

/// The scrollable part of the Thread view — everything ABOVE the docked
/// composer (header meta, title, optional mod row, body, replies heading,
/// reply tree). Matches exactly what `draw_right_thread`'s own scroll
/// region draws, same doctrine every `*_h`/`draw_*` pair in this file
/// follows.
fn thread_scroll_content_h(t: &ForumThreadV2Detail, replies_tree: &[ForumReplyV2Ui], f: &ForumV2State) -> f64 {
    let header_h = 16.0 + 26.0;
    let mod_h = if f.is_moderator_or_admin() { ROW_H } else { 0.0 };
    let body_h = blocks_h(&t.body) + 12.0;
    let replies_heading_h = 24.0;
    let replies_h = reply_tree_h(replies_tree);
    header_h + mod_h + body_h + replies_heading_h + replies_h
}

/// The docked composer's own height — error line, reply-to indicator, the
/// REPLY label, the field, and the submit button. Fixed to the bottom of
/// the pane regardless of how tall the scrollable part above it is.
fn thread_composer_h(f: &ForumV2State, reply_to: Option<i64>) -> f64 {
    let err_h = if f.last_error.is_some() { 20.0 } else { 0.0 };
    let reply_to_h = if reply_to.is_some() { 20.0 } else { 0.0 };
    err_h + reply_to_h + 4.0 + 16.0 + FIELD_H + 8.0 + ROW_H
}

#[allow(clippy::too_many_arguments)]
fn draw_right_thread(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    viewport_h: f64,
    state: &CommunityState,
    thread: Option<&ForumThreadV2Detail>,
    replies_tree: &[ForumReplyV2Ui],
    reply_to: Option<i64>,
    ft: &FrameTheme,
    tt: &ToolbarTheme,
    current_time_ms: u64,
    result: &mut CloudOverlayResult,
) {
    let Some(t) = thread else {
        // Nothing to reply to yet — no composer, plain full-height status.
        draw_right_scrollable(
            ctx, coord, layer_id, x, y, w, viewport_h, &state.scroll, 44.0, result,
            |ctx, _coord, _layer_id, sx, sy, sw, _result| {
                if let Some(err) = &state.forum2.last_error {
                    draw_status(ctx, tt, sx, sy, sw, "Failed to load thread", Some(err));
                } else {
                    draw_status(ctx, tt, sx, sy, sw, "Loading thread…", None);
                }
            },
        );
        return;
    };

    let composer_h = thread_composer_h(&state.forum2, reply_to);
    let scroll_h = (viewport_h - composer_h).max(0.0);
    let content_h = thread_scroll_content_h(t, replies_tree, &state.forum2);

    draw_right_scrollable(
        ctx, coord, layer_id, x, y, w, scroll_h, &state.scroll, content_h, result,
        |ctx, coord, layer_id, sx, sy, sw, _result| {
            let mut ly = sy;
            ctx.set_text_baseline(TextBaseline::Top);
            ctx.set_font("10px monospace");
            ctx.set_fill_color(&tt.item_text_muted);
            ctx.set_text_align(TextAlign::Left);
            ctx.fill_text(&format!("{} · {}", t.author, relative_time(&t.created_at, current_time_ms)), sx, ly);
            ly += 16.0;

            let mut title_x = sx;
            if t.pinned {
                draw_svg_icon(ctx, Icon::Pin.svg(), title_x, ly + 3.0, 16.0, 16.0, &tt.accent);
                title_x += 20.0;
            }
            if t.locked {
                draw_svg_icon(ctx, Icon::Lock.svg(), title_x, ly + 3.0, 16.0, 16.0, &tt.item_text_muted);
                title_x += 20.0;
            }
            ctx.set_font("600 14px sans-serif");
            ctx.set_fill_color(&tt.accent);
            ctx.fill_text(&t.title, title_x, ly);
            ly += 26.0;

            if state.forum2.is_moderator_or_admin() {
                draw_mod_row(ctx, coord, layer_id, sx, ly, sw, t, tt);
                ly += ROW_H;
            }

            ly += draw_blocks(ctx, coord, layer_id, "cloud_overlay:forum2:body", sx, ly, sw, &t.body, tt);
            ly += 12.0;

            ctx.set_font("11px monospace");
            ctx.set_fill_color(&tt.item_text_muted);
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Top);
            let replies_label = if replies_tree.is_empty() {
                "NO REPLIES YET".to_string()
            } else if replies_tree.len() == 1 {
                "1 REPLY".to_string()
            } else {
                format!("{} REPLIES", replies_tree.len())
            };
            ctx.fill_text(&replies_label, sx, ly);
            ly += 24.0;

            draw_reply_tree(ctx, coord, layer_id, sx, ly, sw, replies_tree, reply_to, current_time_ms, tt);
        },
    );

    // Docked composer — fixed to the bottom of the pane, outside the
    // scrollable clip above.
    let composer_y = y + (viewport_h - composer_h).max(0.0);
    let mut cy = composer_y;
    if let Some(err) = &state.forum2.last_error {
        cy += draw_error_line(ctx, w, x, cy, err);
    }
    if let Some(target) = reply_to {
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(&tt.item_text_muted);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text(&format!("Replying to reply #{target}"), x, cy);
        cy += 20.0;
    }
    cy += 4.0;
    ctx.set_font("11px monospace");
    ctx.set_fill_color(&tt.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("REPLY", x, cy);
    cy += 16.0;

    let field_rect = WidgetRect::new(x, cy, w, FIELD_H);
    let (rect, chars) = draw_field(
        ctx, coord, layer_id, "cloud_overlay:forum2:reply_input", FORUM_REPLY_FIELD, field_rect,
        "Write a reply…", ft, tt, current_time_ms,
    );
    result.forum_reply_rect = Some(rect);
    result.forum_reply_char_positions = chars;
    cy += FIELD_H + 8.0;

    draw_action_row(
        ctx, coord, layer_id, "cloud_overlay:forum2:reply_submit",
        WidgetRect::new(x, cy, w, ROW_H - ROW_GAP), "Reply", tt,
    );
}

// =============================================================================
// Search view (`ForumV2View::Search`) — query echo + result rows. Reached
// only via the quick-search dropdown's own "More results" row; back to
// Boards goes through the breadcrumb bar's "Forum" segment now (the old
// per-view "← Forum" row, `draw_back_row`, is deleted).
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn draw_search_hit_row(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    index: usize,
    hit: &ForumSearchHitUi,
    current_time_ms: u64,
    tt: &ToolbarTheme,
) -> f64 {
    let widget_id = format!("cloud_overlay:forum2:search_hit:{index}");
    let rect = WidgetRect::new(x, y, w, SEARCH_HIT_ROW_H - ROW_GAP);
    let hovered = coord.is_hovered(&WidgetId::from(widget_id.as_str()));
    if hovered {
        ctx.set_fill_color(&tt.item_bg_hover);
        ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, SETTINGS_BUTTON_RADIUS);
    }

    let heading = hit.title.clone().unwrap_or_else(|| format!("Reply in thread #{}", hit.thread_id));
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.set_font("600 13px sans-serif");
    ctx.set_fill_color(if hovered { &tt.item_text_hover } else { &tt.item_text });
    let heading = crate::ui::widgets::fit_text_to_width(ctx, &heading, w - 16.0);
    ctx.fill_text(&heading, x + 8.0, y + 4.0);

    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&tt.item_text_muted);
    let snippet = crate::ui::widgets::fit_text_to_width(ctx, &hit.snippet, w - 16.0);
    ctx.fill_text(&snippet, x + 8.0, y + 24.0);

    ctx.set_font("11px monospace");
    ctx.set_fill_color(&tt.item_text_muted);
    let meta = format!("{} · {}", hit.author, relative_time(&hit.created_at, current_time_ms));
    ctx.fill_text(&meta, x + 8.0, y + 42.0);

    coord.register_on_layer(widget_id.as_str(), rect, Sense::CLICK | Sense::HOVER, layer_id);
    SEARCH_HIT_ROW_H
}

fn search_h(f: &ForumV2State, hits: &[ForumSearchHitUi]) -> f64 {
    let q_h = 24.0;
    let err_h = if f.last_error.is_some() { 20.0 } else { 0.0 };
    let list_h = if hits.is_empty() { 24.0 } else { hits.len() as f64 * SEARCH_HIT_ROW_H };
    q_h + err_h + list_h
}

#[allow(clippy::too_many_arguments)]
fn draw_search(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    state: &CommunityState,
    q: &str,
    hits: &[ForumSearchHitUi],
    current_time_ms: u64,
    tt: &ToolbarTheme,
) {
    let mut ly = y;
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&tt.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(&format!("Results for \"{q}\""), x, ly);
    ly += 24.0;
    if let Some(err) = &state.forum2.last_error {
        ly += draw_error_line(ctx, w, x, ly, err);
    }
    if hits.is_empty() {
        draw_status(ctx, tt, x, ly, w, "No results.", None);
    } else {
        for (i, hit) in hits.iter().enumerate() {
            ly += draw_search_hit_row(ctx, coord, layer_id, x, ly, w, i, hit, current_time_ms, tt);
        }
    }
}

// =============================================================================
// Compose view (`ForumV2View::Compose`) — TITLE/BODY fields (reusing
// `mlc_app::text_input::{FORUM_TITLE, FORUM_BODY}`), Post/Cancel. Back to the
// board's thread list goes through the breadcrumb bar's own "Board" segment
// now (the old per-view "← Forum" row, `draw_back_row`, is deleted).
// =============================================================================

fn compose_h(f: &ForumV2State) -> f64 {
    let heading_h = 20.0;
    // Disabled state (owner mandate 2026-09-09, `nick_required`): a hint
    // line + one trigger button — no title/body fields, no Post/Cancel.
    if f.nick_required() {
        let hint_h = 24.0;
        let action_h = ROW_H;
        return heading_h + hint_h + action_h;
    }
    let title_h = 16.0 + FIELD_H + 12.0;
    let body_h = 16.0 + FIELD_H + 12.0;
    let err_h = if f.last_error.is_some() { 20.0 } else { 0.0 };
    let action_h = ROW_H;
    heading_h + title_h + body_h + err_h + action_h
}

#[allow(clippy::too_many_arguments)]
fn draw_compose(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    state: &CommunityState,
    slug: &str,
    ft: &FrameTheme,
    tt: &ToolbarTheme,
    current_time_ms: u64,
    result: &mut CloudOverlayResult,
) {
    let mut ly = y;

    let board_label = state.forum2.boards.iter().find(|b| b.slug == slug).map(|b| b.title.as_str()).unwrap_or(slug);
    ctx.set_font("11px monospace");
    ctx.set_fill_color(&tt.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(&format!("NEW THREAD IN {}", board_label.to_uppercase()), x, ly);
    ly += 20.0;

    // Disabled state (owner mandate 2026-09-09): a signed-in caller with
    // no nickname yet cannot write here — one hint line + a trigger into
    // the Account tab's nick editor, no title/body fields, no Post.
    if state.forum2.nick_required() {
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(&tt.item_text_muted);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text(UserSettingsKey::AccountChooseNickHint.get(current_language()), x, ly);
        ly += 24.0;
        draw_action_row(
            ctx, coord, layer_id, "cloud_overlay:forum2:choose_nick",
            WidgetRect::new(x, ly, w, ROW_H - ROW_GAP),
            UserSettingsKey::AccountChooseNickBtn.get(current_language()), tt,
        );
        return;
    }

    ctx.fill_text("TITLE", x, ly);
    ly += 16.0;
    let title_rect = WidgetRect::new(x, ly, w, FIELD_H);
    let (rect, chars) = draw_field(
        ctx, coord, layer_id, "cloud_overlay:forum2:title_input", FORUM_TITLE_FIELD, title_rect,
        "3–120 characters", ft, tt, current_time_ms,
    );
    result.forum_title_rect = Some(rect);
    result.forum_title_char_positions = chars;
    ly += FIELD_H + 12.0;

    ctx.set_font("11px monospace");
    ctx.set_fill_color(&tt.item_text_muted);
    ctx.fill_text("BODY (single paragraph)", x, ly);
    ly += 16.0;
    let body_rect = WidgetRect::new(x, ly, w, FIELD_H);
    let (rect, chars) = draw_field(
        ctx, coord, layer_id, "cloud_overlay:forum2:body_input", FORUM_BODY_FIELD, body_rect,
        "What's on your mind?", ft, tt, current_time_ms,
    );
    result.forum_body_rect = Some(rect);
    result.forum_body_char_positions = chars;
    ly += FIELD_H + 12.0;

    if let Some(err) = &state.forum2.last_error {
        ly += draw_error_line(ctx, w, x, ly, err);
    }

    let post_w = (w - ACTION_BTN_GAP) / 2.0;
    draw_action_row(
        ctx, coord, layer_id, "cloud_overlay:forum2:compose_post",
        WidgetRect::new(x, ly, post_w, ROW_H - ROW_GAP), "Post", tt,
    );
    draw_expand_row(
        ctx, coord, layer_id, "cloud_overlay:forum2:compose_cancel",
        WidgetRect::new(x + post_w + ACTION_BTN_GAP, ly, post_w, ROW_H - ROW_GAP), "Cancel", false, tt,
    );
}
