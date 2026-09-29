//! Centralized scroll input dispatch
//!
//! Provides helper functions for routing scroll interactions (wheel, handle drag,
//! track click) to the correct ScrollState, eliminating per-scrollbar copy-paste
//! in input handlers.

use mlc_core::state::scroll_state::ScrollState;
use uzor::types::Rect as WidgetRect;

use crate::ChartApp;

/// Info about one scrollable area, collected from render results.
pub struct ScrollableInfo {
    /// Scrollbar handle rect (if visible)
    pub handle_rect: Option<WidgetRect>,
    /// Scrollbar track rect (if visible)
    pub track_rect: Option<WidgetRect>,
    /// Total content height
    pub content_height: f64,
    /// Viewport height
    pub viewport_height: f64,
    /// Viewport rect (for wheel hit-testing)
    pub viewport_rect: Option<WidgetRect>,
}

/// Try to start a scrollbar drag if click hits a handle.
///
/// Checks each (info, scroll_state) pair. If the click position hits a scrollbar
/// handle (with ±5px horizontal tolerance), starts the drag on that scroll state.
///
/// Returns `true` if a drag was started (caller should `return` from the handler).
pub fn try_start_scrollbar_drag(
    x: f64,
    y: f64,
    entries: &mut [(&ScrollableInfo, &mut ScrollState)],
) -> bool {
    for (info, state) in entries.iter_mut() {
        if let Some(ref handle_rect) = info.handle_rect {
            let hit = x >= handle_rect.x - 5.0
                && x <= handle_rect.x + handle_rect.width + 5.0
                && y >= handle_rect.y
                && y <= handle_rect.y + handle_rect.height;
            if hit {
                state.start_drag(y);
                return true;
            }
        }
    }
    false
}

/// Continue a scrollbar drag if any scroll state is currently dragging.
///
/// Finds the first dragging scroll state and applies handle_drag.
///
/// Returns `true` if a drag was handled (caller should `return`).
pub fn try_handle_scrollbar_drag(
    y: f64,
    entries: &mut [(&ScrollableInfo, &mut ScrollState)],
) -> bool {
    for (info, state) in entries.iter_mut() {
        if state.is_dragging {
            if let Some(ref track_rect) = info.track_rect {
                state.handle_drag(y, track_rect.height, info.content_height, info.viewport_height);
                return true;
            }
        }
    }
    false
}

/// End all active scrollbar drags.
///
/// Returns `true` if any drag was ended (caller should `return`).
pub fn try_end_scrollbar_drag(entries: &mut [&mut ScrollState]) -> bool {
    let mut any_ended = false;
    for state in entries.iter_mut() {
        if state.is_dragging {
            state.end_drag();
            any_ended = true;
        }
    }
    any_ended
}

/// Route a wheel scroll to the correct scrollable area.
///
/// Checks if the mouse position is inside any viewport_rect and applies handle_wheel.
///
/// Returns `true` if scroll was handled.
pub fn try_handle_wheel(
    x: f64,
    y: f64,
    delta_y: f64,
    entries: &mut [(&ScrollableInfo, &mut ScrollState)],
) -> bool {
    for (info, state) in entries.iter_mut() {
        if let Some(ref vp) = info.viewport_rect {
            if vp.contains(x, y) {
                state.handle_wheel(delta_y, info.content_height, info.viewport_height);
                return true;
            }
        }
    }
    false
}

/// True while any modal's `ScrollState` is still easing its painted offset
/// toward its wheel target — the repaint gate both shells poll once a frame
/// (same shape as the mascot hat's `MascotAnim::is_live()`, see
/// `mascot_animating` in `mlc-app-shell-vello/src/main.rs`'s `about_to_wait`)
/// so the ease actually paints instead of freezing on an intermediate frame.
///
/// Enumerates every `ScrollState` field wired into [`crate::ChartApp::
/// on_scroll`]'s modal routing (recon: `docs/mlc/research/owner-review-
/// 2026-09-23/input-hotkeys-and-modal-wheel.md` item 2). A closed modal's
/// scroll can never diverge from its own target — nothing feeds it wheel
/// input while hidden — so checking all of them unconditionally, open or
/// not, is correct and cheap (each check is an `Option` test plus, at most,
/// one `Instant::elapsed()`).
pub fn any_modal_scroll_animating(chart: &ChartApp) -> bool {
    chart.panel_app.chart_settings_state.scroll.is_animating()
        || chart.tc_modal.scroll.is_animating()
        || chart.panel_app.indicator_settings_state.scroll.is_animating()
        || chart.panel_app.primitive_settings_state.scroll.is_animating()
        || chart.panel_app.alert_settings_state.list_scroll.is_animating()
        || chart.panel_app.user_settings_state.profile_list_scroll.is_animating()
        || chart.panel_app.user_settings_state.general_tab_scroll.is_animating()
        || chart.panel_app.user_settings_state.sync_tab_scroll.is_animating()
        || chart.panel_app.user_settings_state.vault_tab_scroll.is_animating()
        || chart.panel_app.user_settings_state.persistence_tab_scroll.is_animating()
        || chart.panel_app.user_settings_state.server_tab_scroll.is_animating()
        || chart.panel_app.user_settings_state.performance_tab_scroll.is_animating()
        || chart.panel_app.user_settings_state.account_tab_scroll.is_animating()
        || chart.panel_app.user_settings_state.shares_tab_scroll.is_animating()
        || chart.standalone_panels.watchlist_modal.scroll.is_animating()
        || chart.panel_app.chart_browser.scroll.is_animating()
        || chart.panel_app.tags_tabs_state.tabs_scroll.is_animating()
        || chart.panel_app.tags_tabs_state.tags_groups_scroll.is_animating()
        || chart.panel_app.tags_tabs_state.tags_details_scroll.is_animating()
        || any_cloud_scroll_animating(chart)
        || chart.modal_state.scroll.is_animating()
}

/// Cloud/Community overlay's 3 scroll regions (release catalogue + News
/// "All" tab, Community's hosted content pane, Community's boards/
/// conversations list) — cloud-build-only, mirrors `ChartApp::on_scroll`'s
/// `#[cfg(feature = "cloud")]` `"cloud_overlay:scroll_viewport"` arm.
#[cfg(feature = "cloud")]
fn any_cloud_scroll_animating(chart: &ChartApp) -> bool {
    chart.panel_app.cloud_overlay_state.scroll.is_animating()
        || chart.panel_app.community_state.scroll.is_animating()
        || chart.panel_app.community_state.list_scroll.is_animating()
}

#[cfg(not(feature = "cloud"))]
fn any_cloud_scroll_animating(_chart: &ChartApp) -> bool {
    false
}

/// Route a track click to the correct scrollable area.
///
/// If click lands on a track rect (but NOT on the handle), jumps scroll position.
///
/// Returns `true` if click was handled.
pub fn try_handle_track_click(
    x: f64,
    y: f64,
    entries: &mut [(&ScrollableInfo, &mut ScrollState)],
) -> bool {
    for (info, state) in entries.iter_mut() {
        if let Some(ref track_rect) = info.track_rect {
            // Use ±5px horizontal tolerance (same as handle drag) so clicks
            // near the scrollbar strip are captured instead of falling through
            // to text selection.
            let hit = x >= track_rect.x - 5.0
                && x <= track_rect.x + track_rect.width + 5.0
                && y >= track_rect.y
                && y <= track_rect.y + track_rect.height;
            if hit {
                // Don't handle if click is on the handle (that's a drag start)
                if let Some(ref handle_rect) = info.handle_rect {
                    let on_handle = x >= handle_rect.x - 5.0
                        && x <= handle_rect.x + handle_rect.width + 5.0
                        && y >= handle_rect.y
                        && y <= handle_rect.y + handle_rect.height;
                    if on_handle {
                        continue;
                    }
                }
                state.handle_track_click(
                    y,
                    track_rect.y,
                    track_rect.height,
                    info.content_height,
                    info.viewport_height,
                );
                return true;
            }
        }
    }
    false
}
