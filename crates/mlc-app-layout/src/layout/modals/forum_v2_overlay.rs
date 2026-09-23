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
//! opens into a flat, numbered post-card view (`draw_right_thread`; states
//! 3/4 of the same concept — see that function's own doc for the rewrite).
//! A breadcrumb bar under the tab strip (`draw_crumb_bar`, `Forum › Board ›
//! Thread`) replaces both the old rail's permanent visibility and every
//! per-view "← Forum" row (`draw_back_row`, deleted) with ONE consistent way
//! back, at every depth; the compact search toggle and (thread-list only)
//! "New thread" button now live on that same bar instead of a per-view pane
//! header (`draw_pane_header`, deleted).
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
//! with only the posts above it scrolling — same split `dm_overlay.rs`'s own
//! conversation view uses for its send box. The composer itself, and every
//! per-post "↩ Reply" link, now renders only when
//! [`thread_composer_visible`] says the caller can actually write here
//! (owner report 2026-09-23: a signed-out reader used to see both) — a
//! signed-out caller gets one quiet [`ForumKey::SignInToReply`] line instead,
//! a signed-in caller without this board's write class gets nothing at all.
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
    BoardUi, CommunityState, ForumBodySelection, ForumReplyV2Ui, ForumSearchHitUi,
    ForumThreadV2Detail, ForumThreadV2Ui, ForumV2State, ForumV2View,
};
use crate::ui::scroll_state::ScrollState;
use crate::ui::scroll_widget::ScrollableContainer;
use crate::ui::toolbar_theme::ToolbarTheme;
use crate::ui::widgets::{
    draw_avatar_placeholder, draw_input, draw_input_cursor, InputConfig, SETTINGS_BUTTON_RADIUS,
};
use crate::ui::widgets::types::{WidgetState, WidgetTheme};
use crate::ui::Icon;

use super::cloud_overlay::{
    draw_action_row, draw_expand_row, draw_link_row, draw_status, parse_rfc3339_unix_secs,
    register_scrollbar_hit_zones, relative_time, scroll_config, wrap_text, CloudOverlayResult,
    ACTION_BTN_GAP, FIELD_H, FORUM_BODY_FIELD, FORUM_REPLY_FIELD, FORUM_SEARCH_FIELD,
    FORUM_TITLE_FIELD, ROW_GAP, ROW_H,
};
use super::community::CommunityListPaneResult;
use std::collections::HashMap;

const SEARCH_HIT_ROW_H: f64 = 64.0;
const LINK_ROW_H: f64 = ROW_H;

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

/// A small neutral-surface button with a centred label — used for the
/// thread moderation row's three toggles (the per-post "Reply"/"Delete"
/// controls are plain text links, [`draw_text_link`], not buttons — owner
/// report 2026-09-23: "not a button on a full-width row").
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

/// A small text link — no background, no border, just muted text that
/// brightens on hover — the concept's own `.plink` (`forum_concept_v1.html`
/// states 3/4): a post card's "↩ Reply"/"Delete" footer, one of these each,
/// never a full-width button. `hover_color` lets the caller pick the hover
/// tint (`tt.item_text_hover` for Reply, `tt.danger` for Delete) while idle
/// always reads `tt.item_text_muted`. Returns the text width consumed, so a
/// right-aligned footer can lay out its links back to front.
fn draw_text_link(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    widget_id: &str,
    x: f64,
    y: f64,
    label: &str,
    hover_color: &str,
    tt: &ToolbarTheme,
) -> f64 {
    let hovered = coord.is_hovered(&WidgetId::from(widget_id));
    ctx.set_fill_color(if hovered { hover_color } else { &tt.item_text_muted });
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(label, x, y);
    let text_w = ctx.measure_text(label);
    let rect = WidgetRect::new(x - 3.0, y - 3.0, text_w + 6.0, 16.0);
    coord.register_on_layer(widget_id, rect, Sense::CLICK | Sense::HOVER, layer_id);
    text_w
}

/// One clipped red error line — the last request's refusal
/// (`ForumV2State::last_error`). Returns 20.0, matched by every `*_h`
/// function's own `err_h`. `tt.danger` — the same token `draw_text_link`'s
/// own Delete hover already reads in this file (owner ruling: no hardcoded
/// colours, theme tokens only).
fn draw_error_line(ctx: &mut dyn RenderContext, tt: &ToolbarTheme, w: f64, x: f64, y: f64, msg: &str) -> f64 {
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(&tt.danger);
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

/// Draws `text` as wrapped paragraphs (`blocks`), records one
/// [`super::cloud_overlay::ForumLineHit`] per WRAPPED TEXT line into
/// `result.forum_line_rects` for HOST-SIDE text selection (owner ruling
/// 2026-09-10, extended to Forum post bodies — same mechanism
/// `dm_overlay.rs`'s own `draw_message` uses for its bubbles, keyed by this
/// post's own `post_number` instead of a message index), and paints
/// `selection`'s highlight over whichever of THIS post's own lines the
/// ordered range covers — same paint-order idiom `draw_message`'s own
/// selection overlay uses (translucent `tt.selection`, drawn after every
/// line has painted). A lone-URL paragraph renders as a link row instead
/// and is not selectable, same as before this pass.
#[allow(clippy::too_many_arguments)]
fn draw_blocks(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    widget_prefix: &str,
    x: f64,
    y: f64,
    w: f64,
    post_number: i64,
    text: &str,
    selection: Option<ForumBodySelection>,
    tt: &ToolbarTheme,
    result: &mut CloudOverlayResult,
) -> f64 {
    let mut ly = y;
    let mut line_idx: u16 = 0;
    // (line_idx, y_top, text) for this post's own text lines — collected
    // first so the selection-highlight pass below and the final push into
    // `result.forum_line_rects` can both walk it without re-measuring text
    // that has already scrolled past.
    let mut pushed: Vec<(u16, f64, String)> = Vec::new();
    for block in blocks(text) {
        match block {
            Block::Text(lines) => {
                ctx.set_font("12px sans-serif");
                ctx.set_fill_color(&tt.item_text);
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Top);
                for line in &lines {
                    ctx.fill_text(line, x, ly);
                    pushed.push((line_idx, ly, line.clone()));
                    line_idx += 1;
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

    if let Some(sel) = selection.filter(|s| !s.is_empty()) {
        let ((lo_post, lo_line, lo_char), (hi_post, hi_line, hi_char)) = sel.ordered();
        ctx.set_font("12px sans-serif");
        ctx.set_fill_color(&tt.selection);
        ctx.set_global_alpha(0.35);
        for (li, line_y, line) in &pushed {
            let pos = (post_number, *li);
            if pos < (lo_post, lo_line) || pos > (hi_post, hi_line) {
                continue;
            }
            let is_first = pos == (lo_post, lo_line);
            let is_last = pos == (hi_post, hi_line);
            let char_prefix = |n: usize| -> String { line.chars().take(n).collect() };
            let (sel_x, sel_w) = if is_first && is_last {
                let prefix_w = ctx.measure_text(&char_prefix(lo_char as usize));
                let end_w = ctx.measure_text(&char_prefix(hi_char as usize));
                (x + prefix_w, (end_w - prefix_w).max(0.0))
            } else if is_first {
                let prefix_w = ctx.measure_text(&char_prefix(lo_char as usize));
                let full_w = ctx.measure_text(line);
                (x + prefix_w, (full_w - prefix_w).max(4.0))
            } else if is_last {
                let end_w = ctx.measure_text(&char_prefix(hi_char as usize));
                (x, end_w.max(4.0))
            } else {
                (x, ctx.measure_text(line).max(4.0))
            };
            if sel_w > 0.0 {
                ctx.fill_rect(sel_x, *line_y, sel_w, 16.0);
            }
        }
        ctx.set_global_alpha(1.0);
    }

    for (li, line_y, line) in pushed {
        result.forum_line_rects.push(super::cloud_overlay::ForumLineHit {
            post_number,
            line_idx: li,
            y_top: line_y,
            y_bot: line_y + 16.0,
            text: line,
            text_x: x,
        });
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

/// The open thread's own board `can_write`, when it can be resolved
/// client-side at all — `None` when [`ForumV2State::open_thread_board_slug`]
/// itself is `None` (a thread reached via a search hit, see that field's own
/// doc), which is NOT the same thing as "cannot write": [`thread_composer_visible`]
/// treats the two differently.
fn resolve_board_can_write(f: &ForumV2State) -> Option<bool> {
    f.open_thread_board_slug.as_deref().map(|slug| board_can_write(f, slug))
}

/// Whether the docked reply composer — and every per-post "↩ Reply" link —
/// should render at all (owner report 2026-09-23, closed by
/// `forum_concept_v1.html` states 3/4: a signed-out reader used to see both
/// a "Reply" link under every post and an empty input field). NOT the
/// server's final word — `panel_click.rs`'s own `"reply_submit"` arm stays
/// the authoritative gate; this is only the client-side render gate that
/// keeps those controls from ever painting for a caller who cannot use them.
///
/// `board_can_write` is `None` when the write class of this thread's own
/// board cannot be resolved client-side at all ([`resolve_board_can_write`]),
/// in which case a signed-in caller still sees the composer — the same
/// leniency `panel_click.rs`'s own `reply_submit` gate already applies
/// (checking only `caller_signed_in`) for exactly that reason, deferring to
/// the server as the final authority.
pub(crate) fn thread_composer_visible(caller_signed_in: bool, board_can_write: Option<bool>) -> bool {
    caller_signed_in && board_can_write.unwrap_or(true)
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

/// Draws `text` left-aligned at `(x, y)` (font/align/baseline already set by
/// the caller — this only ever touches fill colour), highlighting every
/// case-insensitive occurrence of `query` in `tt.accent` — the concept's own
/// `<mark>` (`forum_concept_v1.html:174,176` — gold in the mockup, but this
/// crate's own drill-down rewrite already moved every other permanent
/// emphasis mark, pin glyph and current-breadcrumb-segment off gold onto
/// `tt.accent`, the one token this file reserves for "the noteworthy one" —
/// see this file's own top-of-file doc). Char-indexed rather than byte-
/// indexed so a lowercase expansion (e.g. a German "ß") can never land a
/// split off a UTF-8 boundary. `query` empty, or matching nothing, draws
/// `text` plain in `base_color`, same single `fill_text` call as before this
/// pass.
fn draw_highlighted_text(
    ctx: &mut dyn RenderContext,
    x: f64,
    y: f64,
    text: &str,
    query: &str,
    base_color: &str,
    highlight_color: &str,
) {
    let chars: Vec<char> = text.chars().collect();
    let query_chars: Vec<char> = query.chars().collect();
    let n = chars.len();
    let m = query_chars.len();
    if m == 0 || m > n {
        ctx.set_fill_color(base_color);
        ctx.fill_text(text, x, y);
        return;
    }
    let matches_at = |i: usize| (0..m).all(|k| chars[i + k].to_lowercase().eq(query_chars[k].to_lowercase()));

    let mut cx = x;
    let mut i = 0usize;
    let mut run_start = 0usize;
    while i < n {
        if i + m <= n && matches_at(i) {
            if run_start < i {
                let plain: String = chars[run_start..i].iter().collect();
                ctx.set_fill_color(base_color);
                ctx.fill_text(&plain, cx, y);
                cx += ctx.measure_text(&plain);
            }
            let hi: String = chars[i..i + m].iter().collect();
            ctx.set_fill_color(highlight_color);
            ctx.fill_text(&hi, cx, y);
            cx += ctx.measure_text(&hi);
            i += m;
            run_start = i;
        } else {
            i += 1;
        }
    }
    if run_start < n {
        let plain: String = chars[run_start..n].iter().collect();
        ctx.set_fill_color(base_color);
        ctx.fill_text(&plain, cx, y);
    }
}

/// The search-hit "kind" tag (concept's own `.kind`, `forum_concept_v1.
/// html:171` — a small bordered pill reading `THREAD`/`REPLY`, drawn before
/// the heading). Returns the width consumed (pill + trailing gap) so the
/// caller can offset the heading text past it.
fn draw_kind_pill(ctx: &mut dyn RenderContext, x: f64, y: f64, kind: &str, tt: &ToolbarTheme) -> f64 {
    const PILL_H: f64 = 13.0;
    const PILL_PAD: f64 = 4.0;
    ctx.set_font("9px monospace");
    let label = kind.to_uppercase();
    let w = ctx.measure_text(&label) + PILL_PAD * 2.0;
    ctx.set_stroke_color(&tt.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rect(x, y, w, PILL_H);
    ctx.set_fill_color(&tt.item_text_muted);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(&label, x + w / 2.0, y + PILL_H / 2.0);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    w + 6.0
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
    let Some(query) = f.quick_search_query.as_deref() else { return };
    if !f.quick_search_open {
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
            let pill_w = draw_kind_pill(ctx, rect.x + 6.0, rect.y + 2.0, &hit.kind, tt);
            let heading = hit.title.clone().unwrap_or_else(|| format!("Reply in thread #{}", hit.thread_id));
            ctx.set_font("600 12px sans-serif");
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Top);
            let heading_color = if hovered { &tt.item_text_hover } else { &tt.item_text };
            let heading = crate::ui::widgets::fit_text_to_width(ctx, &heading, rect.width - 12.0 - pill_w);
            draw_highlighted_text(ctx, rect.x + 6.0 + pill_w, rect.y + 2.0, &heading, query, heading_color, &tt.accent);

            ctx.set_font("11px sans-serif");
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Top);
            let snippet = crate::ui::widgets::fit_text_to_width(ctx, &hit.snippet, rect.width - 12.0);
            draw_highlighted_text(ctx, rect.x + 6.0, rect.y + 18.0, &snippet, query, &tt.item_text_muted, &tt.accent);

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
        py += draw_error_line(ctx, tt, w, x, py, err);
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
        py += draw_error_line(ctx, tt, w, x, py, err);
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
// Thread view (`ForumV2View::Thread`): title/meta header, optional mod row,
// then flat, numbered post CARDS in chronological order (owner-approved
// concept `forum_concept_v1.html` states 3/4, 2026-09-23 rewrite) scroll
// ABOVE a docked composer fixed to the bottom of the pane — same split
// `dm_overlay.rs`'s own conversation view uses for its send box. The OP is
// post #1; every reply is flattened out of the server's tree (not
// indented/nested — `flatten_replies_chronological`) from #2. The composer,
// and every post's own "↩ Reply" link, render only when
// `thread_composer_visible` allows it (owner report 2026-09-23: a
// signed-out reader used to see both).
// =============================================================================

const CARD_PAD: f64 = 10.0;
const CARD_GAP: f64 = 10.0;
const AUTH_COL_W: f64 = 66.0;
const AVATAR_D: f64 = 34.0;
const AVATAR_NICK_GAP: f64 = 6.0;
const NICK_LINE_H: f64 = 14.0;
const RANK_GAP: f64 = 4.0;
const RANK_LINE_H: f64 = 12.0;
const POST_ROW1_H: f64 = 16.0;
const POST_ROW1_GAP: f64 = 7.0;
const REPLYTO_H: f64 = 14.0;
const REPLYTO_GAP: f64 = 7.0;
const HIDDEN_LINE_H: f64 = 16.0;
const FOOT_GAP: f64 = 8.0;
const FOOT_H: f64 = 16.0;
const CHIP_H: f64 = 26.0;
const CHIP_GAP: f64 = 8.0;
const SIGNIN_LINE_H: f64 = 36.0;
const TITLE_LINE_H: f64 = 22.0;
const META_LINE_H: f64 = 18.0;
const HEADER_GAP: f64 = 12.0;

/// One reply, flattened out of the server's reply TREE into chronological
/// order across the WHOLE thread — not tree pre-order: a reply nested under
/// an old post can still land ahead of a brand new top-level reply if it was
/// posted first, same as any other post; only its own [`Self::in_reply_to`]
/// marks the nesting the tree carried. Numbering starts at `first_number`
/// (2 in `draw_right_thread` — the OP is always post #1, and is always
/// chronologically first by construction, so it never competes for a number
/// here). Pure function, no rendering — unit-tested directly
/// (`tests::flatten_orders_chronologically_with_in_reply_to`).
pub(crate) struct FlatReply<'a> {
    pub number: i64,
    pub reply: &'a ForumReplyV2Ui,
    /// `Some((parent_number, parent_author))` when this reply nests under
    /// ANOTHER reply (`parent_reply_id: Some`) — a plain top-level reply to
    /// the thread itself carries `None`, same as the concept's own state 3
    /// (only a reply-to-reply gets an "in reply to" line; a reply to the
    /// thread does not).
    pub in_reply_to: Option<(i64, &'a str)>,
}

pub(crate) fn flatten_replies_chronological(tree: &[ForumReplyV2Ui], first_number: i64) -> Vec<FlatReply<'_>> {
    fn collect<'a>(nodes: &'a [ForumReplyV2Ui], out: &mut Vec<&'a ForumReplyV2Ui>) {
        for n in nodes {
            out.push(n);
            collect(&n.children, out);
        }
    }
    let mut flat: Vec<&ForumReplyV2Ui> = Vec::new();
    collect(tree, &mut flat);
    // A reply's own `created_at` is always later than its parent's (the
    // server refuses a reply targeting a reply that does not exist yet), so
    // sorting the whole flat set by time alone guarantees every parent is
    // already numbered by the time its child is reached below.
    flat.sort_by_key(|n| (parse_rfc3339_unix_secs(&n.created_at).unwrap_or(i64::MAX), n.id));

    let mut numbered: HashMap<i64, (i64, &str)> = HashMap::with_capacity(flat.len());
    let mut out = Vec::with_capacity(flat.len());
    for (i, reply) in flat.into_iter().enumerate() {
        let number = first_number + i as i64;
        let in_reply_to = reply.parent_reply_id.and_then(|pid| numbered.get(&pid).copied());
        numbered.insert(reply.id, (number, reply.author.as_str()));
        out.push(FlatReply { number, reply, in_reply_to });
    }
    out
}

/// The post card's own inner content height (excluding the [`CARD_PAD`]
/// frame and the trailing [`CARD_GAP`] — see [`post_card_h`] for the full
/// slot). Takes the same booleans [`draw_post_card`] derives from its own
/// arguments, so the two can never compute a different number for the same
/// post.
fn post_card_inner_h(body: &str, hidden: bool, has_in_reply_to: bool, has_footer: bool, has_rank: bool) -> f64 {
    let auth_h =
        AVATAR_D + AVATAR_NICK_GAP + NICK_LINE_H + if has_rank { RANK_GAP + RANK_LINE_H } else { 0.0 };
    let mut body_h = POST_ROW1_H + POST_ROW1_GAP;
    if has_in_reply_to {
        body_h += REPLYTO_H + REPLYTO_GAP;
    }
    body_h += if hidden { HIDDEN_LINE_H } else { blocks_h(body) };
    if has_footer {
        body_h += FOOT_GAP + FOOT_H;
    }
    auth_h.max(body_h)
}

/// Full height of one post card's own slot — frame padding plus content
/// plus the trailing gap before the next card ([`CARD_GAP`], matching the
/// concept's own `.posts{gap:10px}`).
fn post_card_h(body: &str, hidden: bool, has_in_reply_to: bool, has_footer: bool, has_rank: bool) -> f64 {
    CARD_PAD * 2.0 + post_card_inner_h(body, hidden, has_in_reply_to, has_footer, has_rank) + CARD_GAP
}

/// "2026-09-20 14:02 · 3d ago" — the concept's own `.ptime` (states 3/4):
/// absolute date+time (the RFC3339 timestamp's own first 16 bytes, all
/// ASCII digits/separators, so slicing there never lands off a char
/// boundary) plus [`relative_time`]. Falls back to the bare timestamp when
/// it is shorter than that.
fn post_absolute_and_relative(created_at: &str, now_ms: u64) -> String {
    match created_at.get(0..16) {
        Some(head) => format!("{} · {}", head.replace('T', " "), relative_time(created_at, now_ms)),
        None => created_at.to_string(),
    }
}

/// "N replies · opened by AUTHOR · 2026-09-20 14:02" — the concept's own
/// `.thread-meta` (states 3/4). `reply_count` is the FLATTENED total (every
/// reply in the tree, not just root-level ones —
/// `flatten_replies_chronological(...).len()`), unlike the heading this
/// replaces, which under-counted by reading only `replies_tree.len()`.
fn thread_meta_line(reply_count: usize, author: &str, created_at: &str) -> String {
    let replies_word = match reply_count {
        0 => "No replies".to_string(),
        1 => "1 reply".to_string(),
        n => format!("{n} replies"),
    };
    let when = created_at.get(0..16).map(|h| h.replace('T', " ")).unwrap_or_else(|| created_at.to_string());
    format!("{replies_word} · opened by {author} · {when}")
}

/// A post card's footer — small text links, right-aligned, back to front
/// (the concept's own `.pfoot{justify-content:flex-end;gap:16px}`): Delete
/// (own reply only) then "↩ Reply"/"Replying…". Either may be absent; the
/// caller only invokes this at all when at least one is present
/// (`draw_post_card`'s own `has_footer`).
#[allow(clippy::too_many_arguments)]
fn draw_post_footer(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    reply_widget_id: Option<&str>,
    reply_label: &str,
    delete_widget_id: Option<&str>,
    tt: &ToolbarTheme,
) {
    ctx.set_font("11px sans-serif");
    let mut rx = x + w;
    if let Some(did) = delete_widget_id {
        let label = "Delete";
        rx -= ctx.measure_text(label);
        draw_text_link(ctx, coord, layer_id, did, rx, y, label, &tt.danger, tt);
        rx -= 16.0;
    }
    if let Some(rid) = reply_widget_id {
        rx -= ctx.measure_text(reply_label);
        draw_text_link(ctx, coord, layer_id, rid, rx, y, reply_label, &tt.item_text_hover, tt);
    }
}

/// One post card (concept states 3/4): avatar/nick/rank author column, a
/// `#N` + absolute/relative time row, an optional "in reply to @nick #N"
/// line, the body (hidden posts read "[hidden by moderator]" instead,
/// regardless of caller — same rule the old per-node view already applied),
/// and a text-link footer. `rank_label` is this POST's own author's rank
/// (`ForumReplyV2Ui::author_rank`/`ForumThreadV2Detail::author_rank`, off
/// the server's `resolve_author_ranks`) — `None` when the server has not
/// resolved one for this author. Returns the full slot height consumed,
/// [`post_card_h`].
#[allow(clippy::too_many_arguments)]
fn draw_post_card(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    number: i64,
    author: &str,
    rank_label: Option<&str>,
    created_at: &str,
    hidden: bool,
    body: &str,
    in_reply_to: Option<(i64, &str)>,
    reply_widget_id: Option<&str>,
    reply_label: &str,
    delete_widget_id: Option<&str>,
    current_time_ms: u64,
    selection: Option<ForumBodySelection>,
    tt: &ToolbarTheme,
    result: &mut CloudOverlayResult,
) -> f64 {
    let has_footer = reply_widget_id.is_some() || delete_widget_id.is_some();
    let has_rank = rank_label.is_some();
    let has_in_reply_to = in_reply_to.is_some();
    let inner_h = post_card_inner_h(body, hidden, has_in_reply_to, has_footer, has_rank);
    let box_h = CARD_PAD * 2.0 + inner_h;

    ctx.set_fill_color(&tt.dropdown_bg);
    ctx.fill_rounded_rect(x, y, w, box_h, SETTINGS_BUTTON_RADIUS);

    // Author column — avatar, nick, rank badge (the concept's own `.authcol`).
    let auth_cx = x + CARD_PAD + AUTH_COL_W / 2.0;
    let mut acy = y + CARD_PAD;
    draw_avatar_placeholder(ctx, auth_cx, acy + AVATAR_D / 2.0, AVATAR_D / 2.0, author, &tt.item_bg_active);
    acy += AVATAR_D + AVATAR_NICK_GAP;
    ctx.set_font("600 12px sans-serif");
    ctx.set_fill_color(&tt.item_text);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Top);
    let nick = crate::ui::widgets::fit_text_to_width(ctx, author, AUTH_COL_W);
    ctx.fill_text(&nick, auth_cx, acy);
    acy += NICK_LINE_H;
    if let Some(rank) = rank_label {
        acy += RANK_GAP;
        let is_staff = rank == "moderator" || rank == "admin";
        let label = rank.to_uppercase();
        ctx.set_font("9px monospace");
        let pill_w = ctx.measure_text(&label) + 10.0;
        let pill_x = auth_cx - pill_w / 2.0;
        ctx.set_fill_color(&tt.dropdown_bg);
        ctx.fill_rounded_rect(pill_x, acy, pill_w, RANK_LINE_H, 3.0);
        ctx.set_fill_color(if is_staff { &tt.accent } else { &tt.item_text_muted });
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(&label, pill_x + pill_w / 2.0, acy + RANK_LINE_H / 2.0);
    }

    // Body column — post number + time, optional "in reply to", body,
    // footer (the concept's own `.pbody`).
    let body_x = x + CARD_PAD + AUTH_COL_W + CARD_PAD;
    let body_w = (x + w - CARD_PAD - body_x).max(40.0);
    let mut ly = y + CARD_PAD;

    ctx.set_font("11px monospace");
    ctx.set_fill_color(&tt.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(&format!("#{number}"), body_x, ly);
    let time_label = post_absolute_and_relative(created_at, current_time_ms);
    ctx.set_text_align(TextAlign::Right);
    ctx.fill_text(&time_label, body_x + body_w, ly);
    ctx.set_text_align(TextAlign::Left);
    ly += POST_ROW1_H + POST_ROW1_GAP;

    if let Some((parent_number, parent_author)) = in_reply_to {
        let lang = current_language();
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(&tt.item_text_muted);
        let prefix = format!("\u{21b3} {} ", ForumKey::InReplyTo.get(lang));
        ctx.fill_text(&prefix, body_x, ly);
        let prefix_w = ctx.measure_text(&prefix);
        ctx.set_font("600 11px sans-serif");
        ctx.set_fill_color(&tt.item_text);
        ctx.fill_text(&format!("@{parent_author} #{parent_number}"), body_x + prefix_w, ly);
        ly += REPLYTO_H + REPLYTO_GAP;
    }

    if hidden {
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(&tt.item_text_muted);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text("[hidden by moderator]", body_x, ly);
        ly += HIDDEN_LINE_H;
    } else {
        ly += draw_blocks(
            ctx, coord, layer_id, &format!("cloud_overlay:forum2:post:{number}"), body_x, ly, body_w,
            number, body, selection, tt, result,
        );
    }

    if has_footer {
        ly += FOOT_GAP;
        draw_post_footer(ctx, coord, layer_id, body_x, ly, body_w, reply_widget_id, reply_label, delete_widget_id, tt);
    }

    box_h + CARD_GAP
}

/// The composer's own "Replying to #N · nick ×" chip (concept state 3) —
/// drawn only while a reply is targeted; its × clears back to a top-level
/// reply (`"cloud_overlay:forum2:clear_reply_to"`, handled in
/// `panel_click.rs`). `target` is `(post_number, author)`, resolved by the
/// caller from the same flattened list the post cards themselves number
/// from — `None` only if the targeted reply is no longer in that list (a
/// delete raced the click), in which case the chip still offers × with no
/// numbers rather than stale ones. Returns the height consumed
/// ([`CHIP_H`] + [`CHIP_GAP`]).
fn draw_reply_chip(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    target: Option<(i64, &str)>,
    tt: &ToolbarTheme,
) -> f64 {
    let lang = current_language();
    let label = match target {
        Some((n, author)) => format!("{} #{n} · {author}", ForumKey::ReplyingTo.get(lang)),
        None => format!("{}…", ForumKey::ReplyingTo.get(lang)),
    };
    ctx.set_font("11px monospace");
    let text_w = ctx.measure_text(&label);
    let close_w = 16.0;
    let chip_w = 8.0 + text_w + 8.0 + close_w + 6.0;
    let rect = WidgetRect::new(x, y, chip_w, CHIP_H);
    ctx.set_fill_color(&tt.dropdown_bg);
    ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, SETTINGS_BUTTON_RADIUS);
    ctx.set_fill_color(&tt.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(&label, rect.x + 8.0, rect.center_y());

    let close_rect = WidgetRect::new(rect.x + rect.width - close_w - 4.0, rect.y + (CHIP_H - 16.0) / 2.0, close_w, 16.0);
    let close_hovered = coord.is_hovered(&WidgetId::from("cloud_overlay:forum2:clear_reply_to"));
    ctx.set_fill_color(if close_hovered { &tt.item_text_hover } else { &tt.item_text_muted });
    ctx.set_text_align(TextAlign::Center);
    ctx.fill_text("\u{d7}", close_rect.center_x(), close_rect.center_y());
    coord.register_on_layer("cloud_overlay:forum2:clear_reply_to", close_rect, Sense::CLICK | Sense::HOVER, layer_id);

    CHIP_H + CHIP_GAP
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
/// composer: title/meta header, optional mod row, then post CARDS in
/// chronological order (the OP as post #1, every reply flattened via
/// [`flatten_replies_chronological`] from #2). Matches exactly what
/// `draw_right_thread`'s own scroll region draws, same doctrine every
/// `*_h`/`draw_*` pair in this file follows.
fn thread_scroll_content_h(t: &ForumThreadV2Detail, replies_tree: &[ForumReplyV2Ui], f: &ForumV2State) -> f64 {
    let header_h = TITLE_LINE_H + META_LINE_H + HEADER_GAP;
    let mod_h = if f.is_moderator_or_admin() { ROW_H } else { 0.0 };
    let composer_visible = thread_composer_visible(f.caller_signed_in, resolve_board_can_write(f));

    let op_h = post_card_h(&t.body, t.hidden, false, composer_visible, t.author_rank.is_some());
    let flat = flatten_replies_chronological(replies_tree, 2);
    let replies_h: f64 = flat
        .iter()
        .map(|fr| {
            post_card_h(
                &fr.reply.body, fr.reply.hidden, fr.in_reply_to.is_some(),
                composer_visible || fr.reply.author_is_me, fr.reply.author_rank.is_some(),
            )
        })
        .sum();

    header_h + mod_h + op_h + replies_h
}

/// The docked composer's own height — reply-to chip (if any), the field,
/// and the submit button; `0.0` when [`thread_composer_visible`] is `false`
/// and the caller is signed in (owner report 2026-09-23: no composer AND no
/// placeholder row for a caller who simply cannot write here);
/// [`SIGNIN_LINE_H`] for the one quiet line a signed-out caller gets
/// instead. Fixed to the bottom of the pane regardless of how tall the
/// scrollable part above it is.
fn thread_composer_h(f: &ForumV2State, reply_to: Option<i64>) -> f64 {
    if thread_composer_visible(f.caller_signed_in, resolve_board_can_write(f)) {
        let err_h = if f.last_error.is_some() { 20.0 } else { 0.0 };
        let chip_h = if reply_to.is_some() { CHIP_H + CHIP_GAP } else { 0.0 };
        err_h + chip_h + FIELD_H + 8.0 + ROW_H
    } else if !f.caller_signed_in {
        SIGNIN_LINE_H
    } else {
        0.0
    }
}

/// The Thread view (`ForumV2View::Thread`) — flat, numbered post CARDS in
/// chronological order (owner-approved concept `forum_concept_v1.html`
/// states 3/4, 2026-09-23 rewrite): the OP is post #1, every reply is
/// flattened out of the server's tree via [`flatten_replies_chronological`]
/// from #2 — NOT nested/indented; a reply-to-reply instead carries its own
/// small "in reply to @nick #N" line. Above the docked composer: title +
/// meta line, then the (unchanged) moderator row, then the cards.
///
/// The composer — and every post's own "↩ Reply" link — renders only when
/// [`thread_composer_visible`] allows it, closing the owner's report that a
/// signed-out reader saw both a "Reply" link under every post and an empty,
/// unusable input field: a signed-out caller instead reads one quiet
/// [`ForumKey::SignInToReply`] line; a signed-in caller without this
/// board's write class gets nothing extra at all (`thread_composer_h`'s own
/// three-way split mirrors this exactly, so the scroll region is sized
/// correctly in all three cases).
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

    let f = &state.forum2;
    let composer_visible = thread_composer_visible(f.caller_signed_in, resolve_board_can_write(f));
    let flat_replies = flatten_replies_chronological(replies_tree, 2);

    let composer_h = thread_composer_h(f, reply_to);
    let scroll_h = (viewport_h - composer_h).max(0.0);
    let content_h = thread_scroll_content_h(t, replies_tree, f);

    draw_right_scrollable(
        ctx, coord, layer_id, x, y, w, scroll_h, &state.scroll, content_h, result,
        |ctx, coord, layer_id, sx, sy, sw, result| {
            let mut ly = sy;

            let mut title_x = sx;
            if t.pinned {
                draw_svg_icon(ctx, Icon::Pin.svg(), title_x, ly + 2.0, 16.0, 16.0, &tt.accent);
                title_x += 20.0;
            }
            if t.locked {
                draw_svg_icon(ctx, Icon::Lock.svg(), title_x, ly + 2.0, 16.0, 16.0, &tt.item_text_muted);
                title_x += 20.0;
            }
            ctx.set_font("600 14px sans-serif");
            ctx.set_fill_color(&tt.accent);
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Top);
            ctx.fill_text(&t.title, title_x, ly);
            ly += TITLE_LINE_H;

            ctx.set_font("11px monospace");
            ctx.set_fill_color(&tt.item_text_muted);
            let meta = thread_meta_line(flat_replies.len(), &t.author, &t.created_at);
            let meta = crate::ui::widgets::fit_text_to_width(ctx, &meta, sw);
            ctx.fill_text(&meta, sx, ly);
            ly += META_LINE_H + HEADER_GAP;

            if f.is_moderator_or_admin() {
                draw_mod_row(ctx, coord, layer_id, sx, ly, sw, t, tt);
                ly += ROW_H;
            }

            let op_reply_id: Option<&str> = composer_visible.then_some("cloud_overlay:forum2:reply_to_op");
            ly += draw_post_card(
                ctx, coord, layer_id, sx, ly, sw,
                1, &t.author, t.author_rank.as_deref(), &t.created_at, t.hidden, &t.body, None,
                op_reply_id, "\u{21a9} Reply", None, current_time_ms, f.selection, tt, result,
            );

            for fr in &flat_replies {
                let reply_id = format!("cloud_overlay:forum2:reply_to:{}", fr.reply.id);
                let targeted = reply_to == Some(fr.reply.id);
                let reply_label = if targeted { "Replying…" } else { "\u{21a9} Reply" };
                let delete_id = (fr.reply.author_is_me || f.is_moderator_or_admin())
                    .then(|| format!("cloud_overlay:forum2:delete_reply:{}", fr.reply.id));
                ly += draw_post_card(
                    ctx, coord, layer_id, sx, ly, sw,
                    fr.number, &fr.reply.author, fr.reply.author_rank.as_deref(), &fr.reply.created_at,
                    fr.reply.hidden, &fr.reply.body, fr.in_reply_to,
                    composer_visible.then_some(reply_id.as_str()), reply_label,
                    delete_id.as_deref(), current_time_ms, f.selection, tt, result,
                );
            }
        },
    );

    // Docked composer / sign-in prompt — fixed to the bottom of the pane,
    // outside the scrollable clip above.
    let composer_y = y + (viewport_h - composer_h).max(0.0);
    let mut cy = composer_y;
    if composer_visible {
        if let Some(err) = &f.last_error {
            cy += draw_error_line(ctx, tt, w, x, cy, err);
        }
        if reply_to.is_some() {
            let target = reply_to.and_then(|id| {
                flat_replies.iter().find(|fr| fr.reply.id == id).map(|fr| (fr.number, fr.reply.author.as_str()))
            });
            cy += draw_reply_chip(ctx, coord, layer_id, x, cy, target, tt);
        }

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
            WidgetRect::new(x, cy, w, ROW_H - ROW_GAP), "Post reply", tt,
        );
    } else if !f.caller_signed_in {
        let lang = current_language();
        ctx.set_font("12px sans-serif");
        ctx.set_fill_color(&tt.item_text_muted);
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(ForumKey::SignInToReply.get(lang), x + w / 2.0, cy + SIGNIN_LINE_H / 2.0);
    }
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
        ly += draw_error_line(ctx, tt, w, x, ly, err);
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
        ly += draw_error_line(ctx, tt, w, x, ly, err);
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

// =============================================================================
// Unit tests — the two pure functions the thread-view flat-cards rewrite
// (2026-09-23, `forum_concept_v1.html` states 3/4) introduced:
// `flatten_replies_chronological` (ordering + "in reply to" resolution) and
// `thread_composer_visible` (the signed-out/no-write-right/can-write gate).
// Neither touches rendering, so both are tested directly, no `RenderContext`
// double needed.
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(id: i64, parent: Option<i64>, depth: i64, author: &str, created_at: &str, children: Vec<ForumReplyV2Ui>) -> ForumReplyV2Ui {
        ForumReplyV2Ui {
            id,
            parent_reply_id: parent,
            depth,
            author: author.to_string(),
            author_is_me: false,
            author_rank: None,
            body: "body".to_string(),
            created_at: created_at.to_string(),
            hidden: false,
            children,
        }
    }

    /// Mirrors `forum_concept_v1.html` state 3's own example thread:
    /// quiet_deska (top-level) → j_novak (nested reply-to-reply) →
    /// Nemo Trading (a LATER top-level reply) — numbered #2/#3/#4 in exactly
    /// that order, with only the nested one carrying an `in_reply_to`.
    #[test]
    fn flatten_orders_chronologically_with_in_reply_to() {
        let tree = vec![
            reply(10, None, 0, "quiet_deska", "2026-09-20T15:10:00+00:00", vec![
                reply(11, Some(10), 1, "j_novak", "2026-09-20T15:22:00+00:00", vec![]),
            ]),
            reply(12, None, 0, "Nemo Trading", "2026-09-21T09:05:00+00:00", vec![]),
        ];

        let flat = flatten_replies_chronological(&tree, 2);

        let numbers: Vec<i64> = flat.iter().map(|fr| fr.number).collect();
        let ids: Vec<i64> = flat.iter().map(|fr| fr.reply.id).collect();
        assert_eq!(numbers, vec![2, 3, 4]);
        assert_eq!(ids, vec![10, 11, 12]);

        assert_eq!(flat[0].in_reply_to, None, "a top-level reply carries no in_reply_to");
        assert_eq!(flat[1].in_reply_to, Some((2, "quiet_deska")), "nested reply resolves its parent's assigned number");
        assert_eq!(flat[2].in_reply_to, None, "a later top-level reply still carries no in_reply_to");
    }

    /// Chronological order is across the WHOLE thread, not tree pre-order: a
    /// nested reply posted before a sibling top-level reply still sorts
    /// before it, even though the tree nests it two levels down.
    #[test]
    fn flatten_orders_by_time_not_tree_position() {
        let tree = vec![
            reply(20, None, 0, "a", "2026-09-20T10:00:00+00:00", vec![
                reply(21, Some(20), 1, "b", "2026-09-20T10:05:00+00:00", vec![]),
            ]),
            reply(22, None, 0, "c", "2026-09-20T10:03:00+00:00", vec![]),
        ];

        let flat = flatten_replies_chronological(&tree, 2);
        let ids: Vec<i64> = flat.iter().map(|fr| fr.reply.id).collect();
        assert_eq!(ids, vec![20, 22, 21], "10:00 < 10:03 < 10:05, regardless of nesting");
    }

    #[test]
    fn composer_visible_signed_out_is_never_true() {
        assert!(!thread_composer_visible(false, Some(true)));
        assert!(!thread_composer_visible(false, Some(false)));
        assert!(!thread_composer_visible(false, None));
    }

    #[test]
    fn composer_visible_signed_in_without_write_right_is_false() {
        assert!(!thread_composer_visible(true, Some(false)));
    }

    #[test]
    fn composer_visible_signed_in_can_write_is_true() {
        assert!(thread_composer_visible(true, Some(true)));
        // Board write-class unresolvable client-side (a thread reached via
        // a search hit — `ForumV2State::open_thread_board_slug` doc) defers
        // to the server, same leniency `panel_click.rs`'s own
        // `"reply_submit"` arm already applies.
        assert!(thread_composer_visible(true, None));
    }
}
