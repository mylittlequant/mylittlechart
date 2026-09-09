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
//! W2 of the community-modal-and-forum-ladder arc (`docs/mlc/plans/
//! community-modal-and-forum-ladder-2026-09-09.md`) turned this into a
//! "familiar forum" TWO-COLUMN layout: a fixed-width, own-scroll LEFT column
//! (the boards list, `draw_board_list`, with a totals header and per-board
//! thread/reply counts + last activity — owner review 2026-09-09: "a forum,
//! not a feed") and a RIGHT pane whose content depends on `ForumV2View`
//! (board header + threads, an open thread, search results, or the compose
//! form). An open thread's reply composer is DOCKED at the bottom of the
//! right pane, with only the thread body/reply tree above it scrolling —
//! same split `dm_overlay.rs`'s own conversation view uses for its send box.
//! `height`/`draw` no longer size a single shared scroll region:
//! `list_height` sizes the left column, `draw` sizes each pane's own region
//! internally (see this file's own per-view `*_h` helpers).
//!
//! Owner review 2026-09-09 (item d) replaced the old full-width search box
//! + big "Search" button with a compact magnifier toggle in the right
//! pane's own header row (`draw_pane_header`): typing debounces (or Enter
//! fires) the same `ForumV2Request::Search` the full-page view already
//! used, and the answer renders as a small dropdown anchored under the
//! field (`draw_quick_search_dropdown`) instead of switching `view` — the
//! full-page `ForumV2View::Search` stays reachable only via the dropdown's
//! own "More results" row.

use mlc_core::engine::render::{draw_svg_icon, RenderContext, TextAlign, TextBaseline};
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
    draw_action_row, draw_column_divider, draw_expand_row, draw_link_row, draw_status,
    register_scrollbar_hit_zones, relative_time, scroll_config, wrap_text, CloudOverlayResult,
    ACTION_BTN_GAP, COL_GAP, FIELD_H, FORUM_BODY_FIELD, FORUM_REPLY_FIELD, FORUM_SEARCH_FIELD,
    FORUM_TITLE_FIELD, GOLD, LIST_COL_W, ROW_GAP, ROW_H,
};
use super::community::CommunityListPaneResult;

/// Left column's per-board row height — title, description, the "write:
/// <class>" hint, the thread/post counts, and the last-activity line (five
/// short lines at [`LIST_COL_W`]'s width).
const BOARD_ROW_H: f64 = 84.0;
const THREAD_ROW_H: f64 = 48.0;
const SEARCH_HIT_ROW_H: f64 = 64.0;
const LINK_ROW_H: f64 = ROW_H;
/// Reply-tree indent per `ForumReplyV2Ui::depth`, capped at
/// [`REPLY_MAX_DEPTH`] so a deeply nested sub-thread cannot squeeze the
/// content column to nothing.
const REPLY_INDENT: f64 = 16.0;
const REPLY_MAX_DEPTH: i64 = 6;

// ── Right-pane header (owner review 2026-09-09, item d) ─────────────────
// Board title / "New thread" / the compact search toggle all share one
// fixed-height row — see `draw_pane_header`'s own doc.
const PANE_HEADER_H: f64 = FIELD_H + 8.0;
const QUICK_SEARCH_ICON_BOX: f64 = 28.0;
const QUICK_SEARCH_FIELD_W: f64 = 180.0;
const QUICK_RESULT_ROW_H: f64 = 36.0;
const QUICK_MORE_ROW_H: f64 = 28.0;
const QUICK_DROPDOWN_MAX_ROWS: usize = 8;

// =============================================================================
// Entry points — `list_height` sizes the LEFT column's own scroll container
// (`community.rs`'s `draw` call); the RIGHT pane's content height is computed
// per-view inline by `draw` itself (Thread's own composer-docking split needs
// two different heights — the scrollable part and the fixed footer — so a
// single shared number stopped being enough, unlike W1's one-pane shape).
// =============================================================================

pub(crate) fn list_height(state: &CommunityState) -> f64 {
    board_list_h(&state.forum2)
}

/// The right pane's own "wanted" content height, matching `community.rs`'s
/// old single-number contract — used only for `ModalShellSpec`'s inert
/// `content_height` (see that call site's own doc for why it is inert);
/// `draw`'s own per-view scaffolding computes the SAME numbers again from
/// the same helpers below, so the two can never silently diverge from each
/// other even though neither reads this function.
pub(crate) fn height(state: &CommunityState) -> f64 {
    match &state.forum2.view {
        ForumV2View::Boards => right_boards_h(&state.forum2),
        ForumV2View::BoardThreads { slug, threads, next_before } => {
            right_board_threads_h(&state.forum2, slug, threads, next_before)
        }
        ForumV2View::Thread { thread, replies_tree, reply_to, .. } => match thread {
            Some(t) => thread_scroll_content_h(t, replies_tree, &state.forum2) + thread_composer_h(&state.forum2, *reply_to),
            None => 44.0,
        },
        ForumV2View::Search { hits, .. } => search_h(&state.forum2, hits),
        ForumV2View::Compose { .. } => compose_h(&state.forum2),
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
    list_result: &mut CommunityListPaneResult,
) {
    let right_x = x + LIST_COL_W + COL_GAP;
    let right_w = (w - LIST_COL_W - COL_GAP).max(0.0);
    draw_column_divider(ctx, right_x - COL_GAP / 2.0, y, viewport_h, tt);

    // --- Left column: boards list, its own scroll region ---
    let list_drawn_h = board_list_h(&state.forum2);
    let list_viewport_rect = WidgetRect::new(x, y, LIST_COL_W, viewport_h);
    coord.register_on_layer("cloud_overlay:scroll_viewport", list_viewport_rect, Sense::SCROLL, layer_id);
    let list_scrollable = ScrollableContainer::new(list_viewport_rect, &state.list_scroll, Some(scroll_config()));
    list_scrollable.begin(ctx);
    draw_board_list(
        ctx, coord, layer_id, x, list_scrollable.content_y(), list_scrollable.content_width(),
        &state.forum2, current_time_ms, tt,
    );
    let widget_theme = WidgetTheme::default();
    let list_scroll_result = list_scrollable.end(ctx, list_drawn_h, &widget_theme);
    list_result.viewport_rect = Some(list_viewport_rect);
    list_result.total_content_height = Some(list_drawn_h);
    list_result.viewport_height = Some(viewport_h.min(list_drawn_h));
    list_result.scrollbar_handle_rect = list_scroll_result.handle_rect;
    list_result.scrollbar_track_rect = list_scroll_result.track_rect;
    register_scrollbar_hit_zones(coord, layer_id, &list_scroll_result);

    // --- Right pane: content depends on the current sub-view ---
    match &state.forum2.view {
        ForumV2View::Boards => draw_right_boards(
            ctx, coord, layer_id, right_x, y, right_w, viewport_h, state, ft, tt, current_time_ms, result,
        ),
        ForumV2View::BoardThreads { slug, threads, next_before } => draw_right_board_threads(
            ctx, coord, layer_id, right_x, y, right_w, viewport_h, state, slug, threads,
            next_before.as_deref(), ft, tt, current_time_ms, result,
        ),
        ForumV2View::Thread { thread, replies_tree, reply_to, .. } => draw_right_thread(
            ctx, coord, layer_id, right_x, y, right_w, viewport_h, state, thread.as_ref(),
            replies_tree, *reply_to, ft, tt, current_time_ms, result,
        ),
        ForumV2View::Search { q, hits } => draw_right_scrollable(
            ctx, coord, layer_id, right_x, y, right_w, viewport_h, &state.scroll,
            search_h(&state.forum2, hits), result,
            |ctx, coord, layer_id, sx, sy, sw, result| {
                draw_search(ctx, coord, layer_id, sx, sy, sw, state, q, hits, current_time_ms, tt);
                let _ = result;
            },
        ),
        ForumV2View::Compose { slug } => draw_right_scrollable(
            ctx, coord, layer_id, right_x, y, right_w, viewport_h, &state.scroll,
            compose_h(&state.forum2), result,
            |ctx, coord, layer_id, sx, sy, sw, result| {
                draw_compose(ctx, coord, layer_id, sx, sy, sw, state, slug, ft, tt, current_time_ms, result);
            },
        ),
    }
}

/// Shared scaffolding for a right-pane view with NO docked footer (Search,
/// Compose): register the viewport, clip+scroll, run `body`, then publish
/// the scroll geometry into `result`. `Thread`'s own composer-docking split
/// does not use this — it needs a SHORTER scroll region (viewport minus the
/// footer) plus a fixed block below it, so it scaffolds itself inline
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
// Shared small widgets — text field, delete row, mini action button, back
// row, error line. Local to this file (unlike `cloud_overlay.rs`'s own
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

/// The single "← Forum" row `Search` (and `Compose`, via its own Cancel
/// button reaching the same request) draws to leave a view not reachable
/// any other way. `BoardThreads`/`Thread` dropped their own copy of this
/// row in W2: the left column's permanent boards list already reaches
/// (and re-reaches) both, so a second "back" affordance was dead weight —
/// see this file's own top doc.
fn draw_back_row(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    tt: &ToolbarTheme,
) -> f64 {
    let rect = WidgetRect::new(x, y, w, 28.0);
    let hovered = coord.is_hovered(&WidgetId::from("cloud_overlay:forum2:back"));
    if hovered {
        ctx.set_fill_color(&tt.item_bg_hover);
        ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, SETTINGS_BUTTON_RADIUS);
    }
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(if hovered { &tt.item_text } else { &tt.item_text_muted });
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("← Forum", x + 8.0, y + 14.0);
    coord.register_on_layer("cloud_overlay:forum2:back", rect, Sense::CLICK | Sense::HOVER, layer_id);
    36.0
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
// Left column — boards list (`draw_board_list`): a totals header (boards /
// threads / posts) plus one row per board (title, description, the
// "write: <class>" hint, thread/post counts, last activity). Community-modal
// arc W2 (owner review 2026-09-09: "a forum, not a feed").
// =============================================================================

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

fn board_list_h(f: &ForumV2State) -> f64 {
    let header_h = totals_header_h() + 8.0;
    let list_h = if f.boards.is_empty() { 24.0 } else { f.boards.len() as f64 * BOARD_ROW_H };
    header_h + list_h
}

#[allow(clippy::too_many_arguments)]
fn draw_board_row(
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
    let widget_id = format!("cloud_overlay:forum2:board:{}", board.slug);
    let rect = WidgetRect::new(x, y, w, BOARD_ROW_H - ROW_GAP);
    let hovered = coord.is_hovered(&WidgetId::from(widget_id.as_str()));
    if hovered {
        ctx.set_fill_color(&tt.item_bg_hover);
        ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, SETTINGS_BUTTON_RADIUS);
    }

    let mut title_x = x + 8.0;
    if !board.can_write {
        // Lock glyph: the caller can read this board but its write_class is
        // above what they hold (`social_db::can_write`, resolved by the
        // shell into `BoardUi::can_write`). A board the caller cannot even
        // READ never reaches this row at all — the server already filters
        // it out of `boards` (`routes::forum_v2::list_boards`), which is
        // why there is no separate "locked for read" mark here.
        draw_svg_icon(ctx, Icon::Lock.svg(), title_x, y + 5.0, 14.0, 14.0, &tt.item_text_muted);
        title_x += 20.0;
    }
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.set_font("600 13px sans-serif");
    ctx.set_fill_color(if hovered { GOLD } else { &tt.item_text });
    let title = crate::ui::widgets::fit_text_to_width(ctx, &board.title, w - (title_x - x) - 8.0);
    ctx.fill_text(&title, title_x, y + 4.0);

    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&tt.item_text_muted);
    let desc = crate::ui::widgets::fit_text_to_width(ctx, &board.description, w - 16.0);
    ctx.fill_text(&desc, x + 8.0, y + 22.0);

    // "write: <class>" hint — only when the caller CANNOT write (owner
    // review 2026-09-09: quiet when they can; the lock glyph above already
    // says the same thing at a glance for the cannot-write case, this line
    // just names the class). Other lines keep their fixed y-offsets either
    // way — dropping this one does not reflow the row.
    if !board.can_write {
        ctx.set_font("10px monospace");
        ctx.set_fill_color(&tt.item_text_muted);
        let hint = crate::ui::widgets::fit_text_to_width(ctx, &format!("write: {}", board.write_class), w - 16.0);
        ctx.fill_text(&hint, x + 8.0, y + 38.0);
    }

    let counts = format!("{} threads · {} posts", board.thread_count, board.thread_count + board.reply_count);
    let counts = crate::ui::widgets::fit_text_to_width(ctx, &counts, w - 16.0);
    ctx.fill_text(&counts, x + 8.0, y + 52.0);

    let activity = match (&board.last_post_at, &board.last_thread_title) {
        (Some(t), Some(title)) => format!("{} — {}", relative_time(t, current_time_ms), title),
        (Some(t), None) => relative_time(t, current_time_ms),
        _ => "No posts yet".to_string(),
    };
    let activity = crate::ui::widgets::fit_text_to_width(ctx, &activity, w - 16.0);
    ctx.fill_text(&activity, x + 8.0, y + 66.0);

    coord.register_on_layer(widget_id.as_str(), rect, Sense::CLICK | Sense::HOVER, layer_id);
    BOARD_ROW_H
}

fn draw_board_list(
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
    py += draw_totals_header(ctx, x, py, w, f, tt);
    py += 8.0;
    if f.boards.is_empty() {
        draw_status(ctx, tt, x, py, w, "No boards yet.", None);
    } else {
        for b in &f.boards {
            py += draw_board_row(ctx, coord, layer_id, x, py, w, b, current_time_ms, tt);
        }
    }
}

// =============================================================================
// Right pane header — board title (`BoardThreads` only) + "New thread" (when
// `can_write`) + a compact search toggle, shared by every right-pane view
// (owner review 2026-09-09, item d: replaces the old full-width search box
// + big "Search" button, which took half the pane for one field). Clicking
// the magnifier expands the shared `FORUM_SEARCH` field in place; the
// answer renders as a dropdown anchored under it (`draw_quick_search_
// dropdown`) rather than switching `view` — the old full-page
// `ForumV2View::Search` stays reachable only via the dropdown's own "More
// results" row.
// =============================================================================

/// Draws the header row and returns its height. When the inline field is
/// open, `result.forum_search_rect` carries its rect (the dropdown's own
/// anchor) — left untouched (so it stays `None` on a fresh per-frame
/// result) when the field is closed.
#[allow(clippy::too_many_arguments)]
fn draw_pane_header(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    board_title_text: Option<&str>,
    show_new_thread: bool,
    state: &CommunityState,
    ft: &FrameTheme,
    tt: &ToolbarTheme,
    current_time_ms: u64,
    result: &mut CloudOverlayResult,
) -> f64 {
    let h = PANE_HEADER_H;
    let mut right_edge = x + w;

    // Search toggle — always the rightmost control.
    let icon_rect = WidgetRect::new(right_edge - QUICK_SEARCH_ICON_BOX, y + (h - QUICK_SEARCH_ICON_BOX) / 2.0, QUICK_SEARCH_ICON_BOX, QUICK_SEARCH_ICON_BOX);
    let icon_hovered = coord.is_hovered(&WidgetId::from("cloud_overlay:forum2:quick_search_toggle"));
    if icon_hovered || state.forum2.quick_search_open {
        ctx.set_fill_color(&tt.item_bg_hover);
        ctx.fill_rounded_rect(icon_rect.x, icon_rect.y, icon_rect.width, icon_rect.height, SETTINGS_BUTTON_RADIUS);
    }
    draw_svg_icon(ctx, Icon::Search.svg(), icon_rect.x + 6.0, icon_rect.y + 6.0, 16.0, 16.0, &tt.item_text);
    coord.register_on_layer("cloud_overlay:forum2:quick_search_toggle", icon_rect, Sense::CLICK | Sense::HOVER, layer_id);
    right_edge -= QUICK_SEARCH_ICON_BOX + 8.0;

    if state.forum2.quick_search_open {
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

    if show_new_thread {
        let btn_w = 92.0f64.min((right_edge - x).max(0.0));
        let btn_rect = WidgetRect::new(right_edge - btn_w, y + (h - (ROW_H - ROW_GAP)) / 2.0, btn_w, ROW_H - ROW_GAP);
        draw_action_row(ctx, coord, layer_id, "cloud_overlay:forum2:new_thread", btn_rect, "New thread", tt);
        right_edge -= btn_w + 8.0;
    }

    if let Some(title) = board_title_text {
        ctx.set_font("600 13px sans-serif");
        ctx.set_fill_color(GOLD);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        let avail_w = (right_edge - x).max(20.0);
        let title = crate::ui::widgets::fit_text_to_width(ctx, title, avail_w);
        ctx.fill_text(&title, x, y + h / 2.0);
    }

    h
}

/// Drawn LAST in whichever view's closure calls it, so it paints over
/// board/thread rows beneath it — anchored under `field_rect` (the header's
/// own field, `None` when the box is closed, in which case this is a
/// no-op). Clipped to the same scroll viewport the caller is already inside
/// (`ScrollableContainer::begin`'s clip) — a dropdown near the pane's
/// visible bottom edge is cut there, same as any other content would be.
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
            ctx.set_fill_color(if hovered { GOLD } else { &tt.item_text });
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
    ctx.set_fill_color(if more_hovered { GOLD } else { &tt.item_text_muted });
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("More results…", more_rect.center_x(), more_rect.center_y());
    coord.register_on_layer("cloud_overlay:forum2:quick_more", more_rect, Sense::CLICK | Sense::HOVER, layer_id);
}

// =============================================================================
// Right pane — Boards view (`ForumV2View::Boards`, nothing selected yet):
// header (search toggle only — no board title/"New thread") plus a
// placeholder pointing at the left column.
// =============================================================================

fn right_boards_h(f: &ForumV2State) -> f64 {
    let err_h = if f.last_error.is_some() { 20.0 } else { 0.0 };
    PANE_HEADER_H + err_h + 24.0
}

#[allow(clippy::too_many_arguments)]
fn draw_right_boards(
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
) {
    let content_h = right_boards_h(&state.forum2);
    draw_right_scrollable(
        ctx, coord, layer_id, x, y, w, viewport_h, &state.scroll, content_h, result,
        |ctx, coord, layer_id, sx, sy, sw, result| {
            let mut py = sy;
            py += draw_pane_header(ctx, coord, layer_id, sx, py, sw, None, false, state, ft, tt, current_time_ms, result);
            if let Some(err) = &state.forum2.last_error {
                py += draw_error_line(ctx, sw, sx, py, err);
            }
            draw_status(ctx, tt, sx, py, sw, "Select a board from the list on the left.", None);
            draw_quick_search_dropdown(ctx, coord, layer_id, result.forum_search_rect, &state.forum2, tt);
        },
    );
}

// =============================================================================
// Right pane — BoardThreads view (`ForumV2View::BoardThreads`): the shared
// header (board title + "New thread" when `can_write` + the compact search
// toggle — `draw_pane_header`), thread rows (pinned first), optional
// "Load more". No "← Forum" row (W2) — the left column's permanent boards
// list already reaches this view.
// =============================================================================

fn board_can_write(f: &ForumV2State, slug: &str) -> bool {
    f.boards.iter().find(|b| b.slug == slug).map(|b| b.can_write).unwrap_or(false)
}

fn board_title<'a>(f: &'a ForumV2State, slug: &'a str) -> &'a str {
    f.boards.iter().find(|b| b.slug == slug).map(|b| b.title.as_str()).unwrap_or(slug)
}

fn draw_thread_row(
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
    let widget_id = format!("cloud_overlay:forum2:thread:{}", t.id);
    let rect = WidgetRect::new(x, y, w, THREAD_ROW_H - ROW_GAP);
    let hovered = coord.is_hovered(&WidgetId::from(widget_id.as_str()));
    if hovered {
        ctx.set_fill_color(&tt.item_bg_hover);
        ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, SETTINGS_BUTTON_RADIUS);
    }

    let mut title_x = x + 8.0;
    if t.pinned {
        draw_svg_icon(ctx, Icon::Pin.svg(), title_x, y + 5.0, 14.0, 14.0, GOLD);
        title_x += 18.0;
    }
    if t.locked {
        draw_svg_icon(ctx, Icon::Lock.svg(), title_x, y + 5.0, 14.0, 14.0, &tt.item_text_muted);
        title_x += 18.0;
    }
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.set_font("600 13px sans-serif");
    ctx.set_fill_color(if hovered { GOLD } else { &tt.item_text });
    let title = crate::ui::widgets::fit_text_to_width(ctx, &t.title, w - (title_x - x) - 8.0);
    ctx.fill_text(&title, title_x, y + 4.0);

    let activity = t.last_reply_at.as_deref().unwrap_or(&t.created_at);
    let replies = if t.reply_count == 1 { "1 reply".to_string() } else { format!("{} replies", t.reply_count) };
    let meta = format!("{} · {} · {}", t.author, relative_time(activity, current_time_ms), replies);
    ctx.set_font("11px monospace");
    ctx.set_fill_color(&tt.item_text_muted);
    let meta = crate::ui::widgets::fit_text_to_width(ctx, &meta, w - 16.0);
    ctx.fill_text(&meta, x + 8.0, y + 24.0);

    coord.register_on_layer(widget_id.as_str(), rect, Sense::CLICK | Sense::HOVER, layer_id);
    THREAD_ROW_H
}

fn right_board_threads_h(f: &ForumV2State, _slug: &str, threads: &[ForumThreadV2Ui], next_before: &Option<String>) -> f64 {
    let err_h = if f.last_error.is_some() { 20.0 } else { 0.0 };
    let list_h = if threads.is_empty() { 24.0 } else { threads.len() as f64 * THREAD_ROW_H };
    let more_h = if next_before.is_some() { ROW_H } else { 0.0 };
    PANE_HEADER_H + err_h + list_h + more_h
}

#[allow(clippy::too_many_arguments)]
fn draw_right_board_threads(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    viewport_h: f64,
    state: &CommunityState,
    slug: &str,
    threads: &[ForumThreadV2Ui],
    next_before: Option<&str>,
    ft: &FrameTheme,
    tt: &ToolbarTheme,
    current_time_ms: u64,
    result: &mut CloudOverlayResult,
) {
    let next_before_owned = next_before.map(str::to_string);
    let content_h = right_board_threads_h(&state.forum2, slug, threads, &next_before_owned);
    draw_right_scrollable(
        ctx, coord, layer_id, x, y, w, viewport_h, &state.scroll, content_h, result,
        |ctx, coord, layer_id, sx, sy, sw, result| {
            let mut py = sy;
            let board_label = board_title(&state.forum2, slug);
            let can_write = board_can_write(&state.forum2, slug);
            py += draw_pane_header(
                ctx, coord, layer_id, sx, py, sw, Some(board_label), can_write, state, ft, tt, current_time_ms, result,
            );

            if let Some(err) = &state.forum2.last_error {
                py += draw_error_line(ctx, sw, sx, py, err);
            }
            if threads.is_empty() {
                draw_status(ctx, tt, sx, py, sw, "No threads yet.", None);
            } else {
                // Pinned first — `sort_by_key` is stable, so relative order
                // within each of the two groups (as the server returned it)
                // survives.
                let mut ordered: Vec<&ForumThreadV2Ui> = threads.iter().collect();
                ordered.sort_by_key(|t| !t.pinned);
                for t in ordered {
                    py += draw_thread_row(ctx, coord, layer_id, sx, py, sw, t, current_time_ms, tt);
                }
                if next_before.is_some() {
                    draw_expand_row(
                        ctx, coord, layer_id, "cloud_overlay:forum2:load_more",
                        WidgetRect::new(sx, py, sw, ROW_H - ROW_GAP), "Load more", false, tt,
                    );
                }
            }
            draw_quick_search_dropdown(ctx, coord, layer_id, result.forum_search_rect, &state.forum2, tt);
        },
    );
}

// =============================================================================
// Right pane — Thread view (`ForumV2View::Thread`): mod row, body, replies
// tree scroll ABOVE a docked composer (error line, reply-to indicator, REPLY
// field + submit) fixed to the bottom of the pane — same split
// `dm_overlay.rs`'s own conversation view uses for its send box (W2). No
// "← Forum" row (W2, see this file's own top doc) — a fresh click on this
// same board's row in the left column re-opens `BoardThreads`.
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
                draw_svg_icon(ctx, Icon::Pin.svg(), title_x, ly + 3.0, 16.0, 16.0, GOLD);
                title_x += 20.0;
            }
            if t.locked {
                draw_svg_icon(ctx, Icon::Lock.svg(), title_x, ly + 3.0, 16.0, 16.0, &tt.item_text_muted);
                title_x += 20.0;
            }
            ctx.set_font("600 14px sans-serif");
            ctx.set_fill_color(GOLD);
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
// Right pane — Search view (`ForumV2View::Search`) — back row, query echo,
// result rows. Unchanged from W1's single-pane shape (still the one view
// that needs an explicit "← Forum" row — see this file's own top doc).
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
    ctx.set_fill_color(if hovered { GOLD } else { &tt.item_text });
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
    let back_h = 36.0;
    let q_h = 24.0;
    let err_h = if f.last_error.is_some() { 20.0 } else { 0.0 };
    let list_h = if hits.is_empty() { 24.0 } else { hits.len() as f64 * SEARCH_HIT_ROW_H };
    back_h + q_h + err_h + list_h
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
    ly += draw_back_row(ctx, coord, layer_id, x, ly, w, tt);
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
// Right pane — Compose view (`ForumV2View::Compose`) — back row, TITLE/BODY
// fields (reusing `mlc_app::text_input::{FORUM_TITLE, FORUM_BODY}`),
// Post/Cancel. Unchanged from W1's single-pane shape: with no board list
// above it any more (that lives in the left column now), there is nothing
// left for a docked footer to sit below — the whole form already reads as
// the pane's only content.
// =============================================================================

fn compose_h(f: &ForumV2State) -> f64 {
    let back_h = 36.0;
    let heading_h = 20.0;
    let title_h = 16.0 + FIELD_H + 12.0;
    let body_h = 16.0 + FIELD_H + 12.0;
    let err_h = if f.last_error.is_some() { 20.0 } else { 0.0 };
    let action_h = ROW_H;
    back_h + heading_h + title_h + body_h + err_h + action_h
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
    ly += draw_back_row(ctx, coord, layer_id, x, ly, w, tt);

    let board_label = state.forum2.boards.iter().find(|b| b.slug == slug).map(|b| b.title.as_str()).unwrap_or(slug);
    ctx.set_font("11px monospace");
    ctx.set_fill_color(&tt.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(&format!("NEW THREAD IN {}", board_label.to_uppercase()), x, ly);
    ly += 20.0;

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
