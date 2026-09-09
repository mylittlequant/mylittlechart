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
use crate::ui::scroll_widget::ScrollableContainer;
use crate::ui::toolbar_theme::ToolbarTheme;
use crate::ui::widgets::{draw_input, draw_input_cursor, InputConfig, SETTINGS_BUTTON_RADIUS};
use crate::ui::widgets::types::{WidgetState, WidgetTheme};
use crate::ui::Icon;

use super::cloud_overlay::{
    draw_action_row, draw_expand_row, draw_link_row, draw_status, register_scrollbar_hit_zones,
    relative_time, scroll_config, wrap_text, CloudOverlayResult, ACTION_BTN_GAP, FIELD_H,
    FORUM_BODY_FIELD, FORUM_REPLY_FIELD, FORUM_SEARCH_FIELD, FORUM_TITLE_FIELD, GOLD, ROW_GAP,
    ROW_H,
};

const BOARD_ROW_H: f64 = 60.0;
const THREAD_ROW_H: f64 = 48.0;
const SEARCH_HIT_ROW_H: f64 = 64.0;
const LINK_ROW_H: f64 = ROW_H;
/// Reply-tree indent per `ForumReplyV2Ui::depth`, capped at
/// [`REPLY_MAX_DEPTH`] so a deeply nested sub-thread cannot squeeze the
/// content column to nothing.
const REPLY_INDENT: f64 = 16.0;
const REPLY_MAX_DEPTH: i64 = 6;

// =============================================================================
// Entry points — mirror the old (deleted) v1 `forum_view_h`/`draw_forum_view`
// shape: `height` sizes the modal frame, `draw` paints inside a scroll
// container the same way every other unbounded body in `cloud_overlay.rs`
// does.
// =============================================================================

pub(crate) fn height(state: &CommunityState) -> f64 {
    match &state.forum2.view {
        ForumV2View::Boards => boards_h(&state.forum2),
        ForumV2View::BoardThreads { slug, threads, next_before } => {
            board_threads_h(&state.forum2, slug, threads, next_before)
        }
        ForumV2View::Thread { .. } => thread_view_h(&state.forum2),
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
) {
    let drawn_h = height(state);
    let viewport_rect = WidgetRect::new(x, y, w, viewport_h);
    coord.register_on_layer("cloud_overlay:scroll_viewport", viewport_rect, Sense::SCROLL, layer_id);
    let scrollable = ScrollableContainer::new(viewport_rect, &state.scroll, Some(scroll_config()));
    scrollable.begin(ctx);
    let sx = x;
    let sy = scrollable.content_y();
    let sw = scrollable.content_width();

    match &state.forum2.view {
        ForumV2View::Boards => draw_boards(ctx, coord, layer_id, sx, sy, sw, state, ft, tt, current_time_ms, result),
        ForumV2View::BoardThreads { slug, threads, next_before } => draw_board_threads(
            ctx, coord, layer_id, sx, sy, sw, state, slug, threads, next_before.as_deref(), current_time_ms, tt,
        ),
        ForumV2View::Thread { thread, replies_tree, reply_to, .. } => draw_thread(
            ctx, coord, layer_id, sx, sy, sw, state, thread.as_ref(), replies_tree, *reply_to, ft, tt,
            current_time_ms, result,
        ),
        ForumV2View::Search { q, hits } => {
            draw_search(ctx, coord, layer_id, sx, sy, sw, state, q, hits, current_time_ms, tt)
        }
        ForumV2View::Compose { slug } => {
            draw_compose(ctx, coord, layer_id, sx, sy, sw, state, slug, ft, tt, current_time_ms, result)
        }
    }

    let widget_theme = WidgetTheme::default();
    let scroll_result = scrollable.end(ctx, drawn_h, &widget_theme);
    result.total_content_height = Some(drawn_h);
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

/// The single "← Back" row every non-landing view (BoardThreads/Thread/
/// Search/Compose) draws — one shared hit id, same reuse v1's own back row
/// had across Thread/Compose. Which view "back" resolves to is for the
/// click handler (`mlc-app`) to decide from the current `ForumV2View`.
/// Returns the height consumed (36.0, matched by every `*_h` function's own
/// `back_h`).
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
// Boards view (`ForumV2View::Boards`) — search box + board rows.
// =============================================================================

fn search_box_h() -> f64 {
    16.0 + FIELD_H + 8.0 + ROW_H + 12.0
}

#[allow(clippy::too_many_arguments)]
fn draw_search_box(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    ft: &FrameTheme,
    tt: &ToolbarTheme,
    current_time_ms: u64,
    result: &mut CloudOverlayResult,
) -> f64 {
    let mut ly = y;
    ctx.set_font("11px monospace");
    ctx.set_fill_color(&tt.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("SEARCH", x, ly);
    ly += 16.0;
    let field_rect = WidgetRect::new(x, ly, w, FIELD_H);
    let (rect, chars) = draw_field(
        ctx, coord, layer_id, "cloud_overlay:forum2:search_input", FORUM_SEARCH_FIELD, field_rect,
        "Search boards…", ft, tt, current_time_ms,
    );
    result.forum_search_rect = Some(rect);
    result.forum_search_char_positions = chars;
    ly += FIELD_H + 8.0;
    draw_action_row(
        ctx, coord, layer_id, "cloud_overlay:forum2:search_submit",
        WidgetRect::new(x, ly, w, ROW_H - ROW_GAP), "Search", tt,
    );
    ly += ROW_H + 12.0;
    ly - y
}

fn draw_board_row(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    board: &BoardUi,
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
        // shell into `BoardUi::can_write`).
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
    ctx.fill_text(&desc, x + 8.0, y + 26.0);

    coord.register_on_layer(widget_id.as_str(), rect, Sense::CLICK | Sense::HOVER, layer_id);
    BOARD_ROW_H
}

fn boards_h(f: &ForumV2State) -> f64 {
    let sb = search_box_h();
    let err_h = if f.last_error.is_some() { 20.0 } else { 0.0 };
    let list_h = if f.boards.is_empty() { 24.0 } else { f.boards.len() as f64 * BOARD_ROW_H };
    sb + err_h + list_h
}

#[allow(clippy::too_many_arguments)]
fn draw_boards(
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
) {
    let mut py = y;
    py += draw_search_box(ctx, coord, layer_id, x, py, w, ft, tt, current_time_ms, result);
    if let Some(err) = &state.forum2.last_error {
        py += draw_error_line(ctx, w, x, py, err);
    }
    if state.forum2.boards.is_empty() {
        draw_status(ctx, tt, x, py, w, "No boards yet.", None);
    } else {
        for b in &state.forum2.boards {
            py += draw_board_row(ctx, coord, layer_id, x, py, w, b, tt);
        }
    }
}

// =============================================================================
// BoardThreads view (`ForumV2View::BoardThreads`) — back row, optional "New
// thread" (write-class gated), thread rows (pinned first), optional "Load
// more".
// =============================================================================

fn board_can_write(f: &ForumV2State, slug: &str) -> bool {
    f.boards.iter().find(|b| b.slug == slug).map(|b| b.can_write).unwrap_or(false)
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

fn board_threads_h(f: &ForumV2State, slug: &str, threads: &[ForumThreadV2Ui], next_before: &Option<String>) -> f64 {
    let back_h = 36.0;
    let new_thread_h = if board_can_write(f, slug) { ROW_H + 8.0 } else { 0.0 };
    let err_h = if f.last_error.is_some() { 20.0 } else { 0.0 };
    let list_h = if threads.is_empty() { 24.0 } else { threads.len() as f64 * THREAD_ROW_H };
    let more_h = if next_before.is_some() { ROW_H } else { 0.0 };
    back_h + new_thread_h + err_h + list_h + more_h
}

#[allow(clippy::too_many_arguments)]
fn draw_board_threads(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    state: &CommunityState,
    slug: &str,
    threads: &[ForumThreadV2Ui],
    next_before: Option<&str>,
    current_time_ms: u64,
    tt: &ToolbarTheme,
) {
    let mut ly = y;
    ly += draw_back_row(ctx, coord, layer_id, x, ly, w, tt);
    if board_can_write(&state.forum2, slug) {
        draw_action_row(
            ctx, coord, layer_id, "cloud_overlay:forum2:new_thread",
            WidgetRect::new(x, ly, w, ROW_H - ROW_GAP), "New thread", tt,
        );
        ly += ROW_H + 8.0;
    }
    if let Some(err) = &state.forum2.last_error {
        ly += draw_error_line(ctx, w, x, ly, err);
    }
    if threads.is_empty() {
        draw_status(ctx, tt, x, ly, w, "No threads yet.", None);
    } else {
        // Pinned first — `sort_by_key` is stable, so relative order within
        // each of the two groups (as the server returned it) survives.
        let mut ordered: Vec<&ForumThreadV2Ui> = threads.iter().collect();
        ordered.sort_by_key(|t| !t.pinned);
        for t in ordered {
            ly += draw_thread_row(ctx, coord, layer_id, x, ly, w, t, current_time_ms, tt);
        }
        if next_before.is_some() {
            draw_expand_row(
                ctx, coord, layer_id, "cloud_overlay:forum2:load_more",
                WidgetRect::new(x, ly, w, ROW_H - ROW_GAP), "Load more", false, tt,
            );
        }
    }
}

// =============================================================================
// Thread view (`ForumV2View::Thread`) — header, optional moderation row,
// body, reply tree, reply box.
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

fn thread_view_h(f: &ForumV2State) -> f64 {
    let ForumV2View::Thread { thread, replies_tree, reply_to, .. } = &f.view else { return 0.0 };
    let back_h = 36.0;
    let Some(t) = thread else { return back_h + 44.0 };
    let header_h = 16.0 + 26.0;
    let mod_h = if f.is_moderator_or_admin() { ROW_H } else { 0.0 };
    let body_h = blocks_h(&t.body) + 12.0;
    let replies_heading_h = 24.0;
    let replies_h = reply_tree_h(replies_tree);
    let err_h = if f.last_error.is_some() { 20.0 } else { 0.0 };
    let reply_to_h = if reply_to.is_some() { 20.0 } else { 0.0 };
    let reply_box_h = reply_to_h + 4.0 + 16.0 + FIELD_H + 8.0 + ROW_H;
    back_h + header_h + mod_h + body_h + replies_heading_h + replies_h + err_h + reply_box_h
}

#[allow(clippy::too_many_arguments)]
fn draw_thread(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    x: f64,
    y: f64,
    w: f64,
    state: &CommunityState,
    thread: Option<&ForumThreadV2Detail>,
    replies_tree: &[ForumReplyV2Ui],
    reply_to: Option<i64>,
    ft: &FrameTheme,
    tt: &ToolbarTheme,
    current_time_ms: u64,
    result: &mut CloudOverlayResult,
) {
    let mut ly = y;
    ly += draw_back_row(ctx, coord, layer_id, x, ly, w, tt);

    let Some(t) = thread else {
        if let Some(err) = &state.forum2.last_error {
            draw_status(ctx, tt, x, ly, w, "Failed to load thread", Some(err));
        } else {
            draw_status(ctx, tt, x, ly, w, "Loading thread…", None);
        }
        return;
    };

    ctx.set_text_baseline(TextBaseline::Top);
    ctx.set_font("10px monospace");
    ctx.set_fill_color(&tt.item_text_muted);
    ctx.fill_text(&format!("{} · {}", t.author, relative_time(&t.created_at, current_time_ms)), x, ly);
    ly += 16.0;

    let mut title_x = x;
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
        draw_mod_row(ctx, coord, layer_id, x, ly, w, t, tt);
        ly += ROW_H;
    }

    ly += draw_blocks(ctx, coord, layer_id, "cloud_overlay:forum2:body", x, ly, w, &t.body, tt);
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
    ctx.fill_text(&replies_label, x, ly);
    ly += 24.0;

    ly += draw_reply_tree(ctx, coord, layer_id, x, ly, w, replies_tree, reply_to, current_time_ms, tt);

    if let Some(err) = &state.forum2.last_error {
        ly += draw_error_line(ctx, w, x, ly, err);
    }

    if let Some(target) = reply_to {
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(&tt.item_text_muted);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text(&format!("Replying to reply #{target}"), x, ly);
        ly += 20.0;
    }

    ly += 4.0;
    ctx.set_font("11px monospace");
    ctx.set_fill_color(&tt.item_text_muted);
    ctx.fill_text("REPLY", x, ly);
    ly += 16.0;

    let field_rect = WidgetRect::new(x, ly, w, FIELD_H);
    let (rect, chars) = draw_field(
        ctx, coord, layer_id, "cloud_overlay:forum2:reply_input", FORUM_REPLY_FIELD, field_rect,
        "Write a reply…", ft, tt, current_time_ms,
    );
    result.forum_reply_rect = Some(rect);
    result.forum_reply_char_positions = chars;
    ly += FIELD_H + 8.0;

    draw_action_row(
        ctx, coord, layer_id, "cloud_overlay:forum2:reply_submit",
        WidgetRect::new(x, ly, w, ROW_H - ROW_GAP), "Reply", tt,
    );
}

// =============================================================================
// Search view (`ForumV2View::Search`) — back row, query echo, result rows.
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
// Compose view (`ForumV2View::Compose`) — back row, TITLE/BODY fields
// (reusing `mlc_app::text_input::{FORUM_TITLE, FORUM_BODY}`), Post/Cancel.
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
