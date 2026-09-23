//! Community modal (Forum + Messages) — W1 of the community-modal-and-forum-
//! ladder arc (`docs/mlc/plans/community-modal-and-forum-ladder-2026-09-09.md`).
//!
//! Owner rulings this shell answers to: Forum and direct messages get their
//! OWN modal, not tabs inside "What's new" (the News overlay keeps only
//! Posts/Patch notes — see `cloud_overlay.rs`); the modal is resizable from
//! day one via the SAME mechanism `chart_settings.rs` uses
//! (`ModalShellSpec::resizable`, [`ResizableModal`] on [`CommunityState`]).
//!
//! Escape/click-outside semantics mirror `chart_settings.rs` too:
//! `dim_backdrop` is OFF (unlike the News/Download overlay's dimmed style) —
//! a click that misses every registered widget while this modal's layer is
//! open falls through to `mlc-app`'s generic `is_point_in_modal_layer` +
//! `close_topmost_known_modal_layer` path, the same one chart settings
//! relies on, and the modal must not steal chart input while closed (nothing
//! here registers anything when `state.is_open` is false — the caller in
//! `chart_panel_app.rs` only calls this while it is).
//!
//! Hosts Forum-v2 (`forum_v2_overlay`) and Direct Messages (`dm_overlay`)
//! rendering — both take `&CommunityState` (moved out of `CloudOverlayState`
//! alongside this modal) and keep every widget id they had (`cloud_overlay:
//! forum2:*` / `cloud_overlay:dm:*`), so `mlc-app`'s click dispatch needed
//! only a Rust field-access path change, not an id rename.
//!
//! W2 of the community-modal-and-forum-ladder arc (`docs/mlc/plans/
//! community-modal-and-forum-ladder-2026-09-09.md`) turned each pane into a
//! "familiar forum + messenger" TWO-COLUMN layout: a fixed-width, own-
//! scroll left column (boards / conversations) and a right pane (board
//! header + threads, or an open thread/compose/search; the open
//! conversation's messages) — both `forum_v2_overlay::draw`/
//! `dm_overlay::draw` now fill BOTH `CommunityResult::content` (the right
//! pane, reusing `CloudOverlayResult` exactly as W1 did for the single-pane
//! body) and [`CommunityResult::list`] (the left column's own scroll
//! geometry, new this pass).

use mlc_core::engine::render::{RenderContext, TextAlign, TextBaseline};
use uzor::input::{InputCoordinator, LayerId, Sense};
use uzor::types::Rect as WidgetRect;
use uzor::WidgetId;

use crate::layout::render_chart::FrameTheme;
use crate::ui::modal_settings::{CommunityState, CommunityTab, ResizableModal};
use crate::ui::toolbar_theme::ToolbarTheme;
use crate::ui::widgets::avatar::contrasting_text_color;
use crate::ui::widgets::{
    begin_modal_shell, end_modal_shell, ModalFooter, ModalPlacement, ModalShellSpec, ModalTheme,
    SETTINGS_BUTTON_RADIUS,
};
use crate::ui::z_order::ZLayer;

use super::cloud_overlay::CloudOverlayResult;
use super::{dm_overlay, forum_v2_overlay};

const MODAL_W: f64 = 760.0;

/// Header tab-pill geometry (messenger concept v1, `messenger_concept_v1.
/// html:46-53,307`: title "Community", Forum/Messages small PILLS inside
/// the header row, to the right of the title — no separate full-width tab
/// bar). See [`draw_header_tabs`]'s own doc for why these are painted
/// locally rather than through `ModalShellSpec::with_tabs`.
const PILL_H: f64 = 22.0;
const PILL_PAD_H: f64 = 10.0;
const PILL_GAP: f64 = 4.0;
const PILL_GROUP_GAP: f64 = 18.0;

/// Geometry the input side needs — header/resize drag, tab rects, plus the
/// hosted forum-v2/DM content's own scroll + text-field rects. `content`
/// reuses [`CloudOverlayResult`] rather than a bespoke type: that struct's
/// fields (`scroll_viewport_rect`, `forum_title_rect`, `dm_draft_rect`, …)
/// were never News/Download-specific — `forum_v2_overlay::draw`/
/// `dm_overlay::draw` fill it exactly as they did when hosted in the News
/// overlay's own Forum/Messages tabs.
#[derive(Clone, Debug, Default)]
pub struct CommunityResult {
    pub modal_rect: WidgetRect,
    pub header_rect: WidgetRect,
    pub close_btn_rect: WidgetRect,
    pub tab_rects: Vec<(String, WidgetRect)>,
    pub content: CloudOverlayResult,
    /// Left-column (boards/conversations) scroll geometry — see this
    /// type's own doc for why it is separate from `content`. `None` fields
    /// mean the column drew with nothing to scroll (short list), same
    /// "absent means nothing to wire up" convention `CloudOverlayResult`'s
    /// own optional fields already use.
    pub list: CommunityListPaneResult,
}

/// The left column's own scroll/viewport geometry — a single scroll region
/// alongside `content`'s own (community-modal arc W2). Registered under the
/// SAME `"cloud_overlay:scroll_viewport"`/`"cloud_overlay:scrollbar_*"` ids
/// as `content`'s own viewport (and the News/Download overlay's single one)
/// — `mlc-app`'s `try_handle_wheel`/`try_start_scrollbar_drag` already
/// disambiguate same-id candidates by rect containment, so this is exactly
/// the shape those call sites already expect as a third candidate.
#[derive(Clone, Debug, Default)]
pub struct CommunityListPaneResult {
    pub viewport_rect: Option<WidgetRect>,
    pub scrollbar_handle_rect: Option<WidgetRect>,
    pub scrollbar_track_rect: Option<WidgetRect>,
    pub total_content_height: Option<f64>,
    pub viewport_height: Option<f64>,
}

/// Natural (unclamped) content height for whichever pane is active — the
/// taller of the left column and the right pane (W2's two-pane layout has
/// two independent scroll regions on screen at once, so a single number can
/// only ever be a reasonable upper bound, not an exact match for both). Feeds
/// only `ModalShellSpec`'s inert `content_height` (see that call site's own
/// doc for why it has no observable effect today) — never the two panes'
/// OWN `ScrollableContainer::end` calls, which each use their own `*_h`
/// helper directly (`forum_v2_overlay`/`dm_overlay`'s own `draw`).
fn content_height(state: &CommunityState, tab: CommunityTab) -> f64 {
    match tab {
        CommunityTab::Forum => forum_v2_overlay::list_height(state).max(forum_v2_overlay::height(state)),
        CommunityTab::Messages => dm_overlay::list_height(state).max(dm_overlay::height(state)),
    }
}

/// Render the Community modal (Forum + Messages, cloud builds only).
/// `current_time_ms` drives both panes' relative timestamps and their text
/// fields' caret blink — same clock every other cloud modal compares
/// against.
pub fn render_community_modal(
    ctx: &mut dyn RenderContext,
    window_w: f64,
    window_h: f64,
    state: &CommunityState,
    frame_theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
    current_time_ms: u64,
    input_coordinator: &mut InputCoordinator,
    // True when this modal was opened MORE RECENTLY than an already-open
    // User Settings modal — see `render_user_settings_modal`'s own
    // `promote_to_overlay` doc for the full "last opened is on top"/
    // same-z tie-break reasoning this mirrors (owner ruling 2026-09-10).
    promote_to_overlay: bool,
) -> CommunityResult {
    let theme = ModalTheme::from_toolbar(
        &frame_theme.modal_bg,
        &toolbar_theme.separator,
        toolbar_theme,
    );
    // Which tab actually draws (falls back to Forum when `state.tab` names
    // Messages but it is not visible for this caller right now — a stale
    // `tab` surviving a sign-out or an account switch, see
    // `CommunityState::active_tab`'s own doc) and whether the Messages tab
    // exists at all this frame (owner ruling 2026-09-09: `paid` and above).
    let active_tab = state.active_tab();
    let messages_visible = state.messages_tab_visible();
    // The FULL, unclamped content height — not just what the default frame
    // shows. `state.size` is never `None` (see `CommunityState::default`),
    // so this never drives the frame's own size (that always comes from
    // `state.size`/`resizable`); its only remaining job is `shell.overflow`,
    // which the `ScrollableContainer` in `forum_v2_overlay`/`dm_overlay`
    // needs to know how much of a TALL board/thread/conversation list is
    // still reachable by scrolling past the current viewport — capping it
    // here (the way `cloud_overlay.rs`'s own `NEWS_LIST_MAX_BODY_H` caps a
    // content-driven frame) would silently make the excess unreachable.
    //
    // No `.with_tabs(...)` here (messenger concept v1: the title never
    // changes and the tab switch is two small pills INSIDE the header row,
    // not a separate full-width bar under it) — the split `begin_modal_
    // shell`/`end_modal_shell` form is used instead of the closure form so
    // this function can paint the pills itself, between the shell's own
    // chrome and the hosted pane's body, on the SAME layer both need
    // (`modal.rs` itself recommends this split for bodies with extra chrome
    // to add; here the reason is a header addition rather than a long tab
    // body, same mechanism).
    let spec = ModalShellSpec {
        placement: ModalPlacement::At(state.position),
        layer: if promote_to_overlay { ZLayer::ModalOverlay } else { ZLayer::Modal },
        ..ModalShellSpec::new("community", "Community", MODAL_W, content_height(state, active_tab))
        // The composer at the bottom of whichever pane is showing is
        // already this modal's own "final action" row (Post/Reply/Send) —
        // a second, modal-level OK button below it would be redundant
        // chrome, so this modal carries no footer at all (unlike the
        // Editor-footer default most settings modals get).
        .with_footer(ModalFooter::None)
        .resizable(state.size, state.min_size())
    };
    let mut result = CommunityResult::default();
    let (shell, layer_id) =
        begin_modal_shell(ctx, input_coordinator, &spec, &theme, (window_w, window_h));

    // Client-side sum of every loaded conversation's own `unread_count` —
    // the Messages pill's badge (`messenger_concept_v1.html`'s own `.c`
    // span). No aggregate "total unread" field exists on the wire; summing
    // the per-conversation counts already fetched for the left column is
    // cheap and needs no new one (audit `community-modal-and-forum-vs-
    // concept.md` §1).
    let unread: i64 = state.dm.conversations.iter().map(|c| c.unread_count).sum();
    result.tab_rects = draw_header_tabs(
        ctx, input_coordinator, &layer_id, shell.header, shell.close_btn,
        active_tab, messages_visible, unread, toolbar_theme,
    );

    let x = shell.content.x;
    let y = shell.content.y;
    let w = shell.content.width;
    let viewport_h = shell.content.height;
    match active_tab {
        CommunityTab::Forum => forum_v2_overlay::draw(
            ctx, input_coordinator, &layer_id, x, y, w, viewport_h, state, frame_theme, toolbar_theme,
            current_time_ms, &mut result.content, &mut result.list,
        ),
        CommunityTab::Messages => dm_overlay::draw(
            ctx, input_coordinator, &layer_id, x, y, w, viewport_h, state, frame_theme, toolbar_theme,
            current_time_ms, &mut result.content, &mut result.list,
        ),
    }

    end_modal_shell(ctx, input_coordinator, &spec, &theme, &shell, &layer_id, &[]);

    result.modal_rect = shell.frame;
    result.header_rect = shell.header;
    result.close_btn_rect = shell.close_btn.unwrap_or_default();
    result
}

/// Forum/Messages pills, drawn inside `header` — right of the title (which
/// `begin_modal_shell` already painted at `header.x + FOOTER_PAD`), left of
/// the close button. Registered under the SAME `"community:tab:forum"`/
/// `"community:tab:messages"` ids the shared modal chrome's own tab bar used
/// to register (`panel_click.rs`'s `"tab:"` arm, `CommunityTab::from_id`) —
/// only the paint+registration site moved, so the click-handling side needs
/// no change at all. The Messages pill is omitted entirely (not shown
/// disabled) when `messages_visible` is false, same "quiet when they can't"
/// rule the rest of this modal follows. Returns the pill rects, same shape
/// `ModalShell::tab_rects` reports for a shell-drawn tab bar.
#[allow(clippy::too_many_arguments)]
fn draw_header_tabs(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    header: WidgetRect,
    close_btn: Option<WidgetRect>,
    active_tab: CommunityTab,
    messages_visible: bool,
    unread: i64,
    tt: &ToolbarTheme,
) -> Vec<(String, WidgetRect)> {
    // Title width, so the pill group starts right after it rather than
    // guessing a fixed offset — same font `begin_modal_shell` drew the
    // title in.
    ctx.set_font("13px sans-serif");
    let title_w = ctx.measure_text("Community");
    let mut px = header.x + 16.0 + title_w + PILL_GROUP_GAP;
    let right_bound = close_btn.map(|r| r.x).unwrap_or(header.right() - 40.0);
    let py = header.center_y() - PILL_H / 2.0;

    let mut items: Vec<(CommunityTab, &str, Option<i64>)> = vec![(CommunityTab::Forum, "Forum", None)];
    if messages_visible {
        items.push((CommunityTab::Messages, "Messages", (unread > 0).then_some(unread)));
    }

    let mut placed = Vec::with_capacity(items.len());
    ctx.set_font("500 12px sans-serif");
    for (tab, label, badge) in items {
        let badge_label = badge.map(|n| n.to_string());
        let mut w = PILL_PAD_H * 2.0 + ctx.measure_text(label);
        if let Some(b) = &badge_label {
            w += 6.0 + ctx.measure_text(b);
        }
        w = w.min((right_bound - px).max(0.0));
        let rect = WidgetRect::new(px, py, w, PILL_H);
        let id = format!("community:tab:{}", tab.id());
        let is_active = tab == active_tab;
        let hovered = coord.is_hovered(&WidgetId::from(id.as_str()));

        if is_active {
            ctx.set_fill_color(&tt.accent);
            ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, SETTINGS_BUTTON_RADIUS);
        } else if hovered {
            ctx.set_fill_color(&tt.item_bg_hover);
            ctx.fill_rounded_rect(rect.x, rect.y, rect.width, rect.height, SETTINGS_BUTTON_RADIUS);
        }

        let text_color = if is_active { contrasting_text_color(&tt.accent) } else { &tt.item_text_muted };
        ctx.set_font("500 12px sans-serif");
        ctx.set_fill_color(text_color);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        let mut lx = rect.x + PILL_PAD_H;
        ctx.fill_text(label, lx, rect.center_y());
        lx += ctx.measure_text(label);
        if let Some(b) = &badge_label {
            lx += 6.0;
            ctx.set_font("600 10px sans-serif");
            ctx.set_global_alpha(0.85);
            ctx.fill_text(b, lx, rect.center_y());
            ctx.set_global_alpha(1.0);
        }

        coord.register_on_layer(id.as_str(), rect, Sense::CLICK | Sense::HOVER, layer_id);
        placed.push((tab.id().to_string(), rect));
        px += w + PILL_GAP;
    }
    placed
}
