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

use mlc_core::engine::render::RenderContext;
use uzor::input::InputCoordinator;
use uzor::types::Rect as WidgetRect;

use crate::layout::render_chart::FrameTheme;
use crate::ui::modal_settings::{CommunityState, CommunityTab, ResizableModal};
use crate::ui::toolbar_theme::ToolbarTheme;
use crate::ui::widgets::{
    render_modal_shell, ModalFooter, ModalPlacement, ModalShellSpec, ModalTab, ModalTabs,
    ModalTheme, TabFit,
};

use super::cloud_overlay::CloudOverlayResult;
use super::{dm_overlay, forum_v2_overlay};

const MODAL_W: f64 = 760.0;

const TAB_ITEMS: [ModalTab<'static>; 2] = [
    ModalTab { id: "forum", label: "Forum", icon: None },
    ModalTab { id: "messages", label: "Messages", icon: None },
];

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
fn content_height(state: &CommunityState) -> f64 {
    match state.tab {
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
) -> CommunityResult {
    let theme = ModalTheme::from_toolbar(
        &frame_theme.modal_bg,
        &toolbar_theme.separator,
        toolbar_theme,
    );
    // The FULL, unclamped content height — not just what the default frame
    // shows. `state.size` is never `None` (see `CommunityState::default`),
    // so this never drives the frame's own size (that always comes from
    // `state.size`/`resizable`); its only remaining job is `shell.overflow`,
    // which the `ScrollableContainer` in `forum_v2_overlay`/`dm_overlay`
    // needs to know how much of a TALL board/thread/conversation list is
    // still reachable by scrolling past the current viewport — capping it
    // here (the way `cloud_overlay.rs`'s own `NEWS_LIST_MAX_BODY_H` caps a
    // content-driven frame) would silently make the excess unreachable.
    let spec = ModalShellSpec {
        placement: ModalPlacement::At(state.position),
        ..ModalShellSpec::new("community", "Forum & Messages", MODAL_W, content_height(state))
        .with_tabs(ModalTabs::Horizontal {
            items: &TAB_ITEMS,
            active: state.tab.index(),
            fit: TabFit::Equal,
        })
        // The composer at the bottom of whichever pane is showing is
        // already this modal's own "final action" row (Post/Reply/Send) —
        // a second, modal-level OK button below it would be redundant
        // chrome, so this modal carries no footer at all (unlike the
        // Editor-footer default most settings modals get).
        .with_footer(ModalFooter::None)
        .resizable(state.size, state.min_size())
    };
    let mut result = CommunityResult::default();
    let shell = render_modal_shell(
        ctx,
        input_coordinator,
        &spec,
        &theme,
        (window_w, window_h),
        |ctx, coord, layer_id, shell| {
            let x = shell.content.x;
            let y = shell.content.y;
            let w = shell.content.width;
            let viewport_h = shell.content.height;
            match state.tab {
                CommunityTab::Forum => forum_v2_overlay::draw(
                    ctx, coord, layer_id, x, y, w, viewport_h, state, frame_theme, toolbar_theme,
                    current_time_ms, &mut result.content, &mut result.list,
                ),
                CommunityTab::Messages => dm_overlay::draw(
                    ctx, coord, layer_id, x, y, w, viewport_h, state, frame_theme, toolbar_theme,
                    current_time_ms, &mut result.content, &mut result.list,
                ),
            }
        },
    );
    result.modal_rect = shell.frame;
    result.header_rect = shell.header;
    result.close_btn_rect = shell.close_btn.unwrap_or_default();
    result.tab_rects = shell.tab_rects;
    result
}
