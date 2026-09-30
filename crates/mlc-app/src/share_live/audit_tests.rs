//! ChartApp-level tests for the client-audit fixes and the dead-link
//! handling: per-tab ack, viewer release, overlay preview, stable ids,
//! timestamps on the wire, per-chart rights, and links that are gone.

use std::sync::Arc;

use mlc_backend_data_bridge::DataBridge;
use mlc_core::drawing::primitives_v2::PrimitiveRegistry;
use mlc_core::ids::BubbleId;
use mlc_core::state::Timeframe;
use mlc_core::user_profile::profile::PersistedSharedTab;
use mlc_core::Bar;

use super::tests::{
    author_app, go_live, new_app, open_link_tab, open_socket, turn_viewport_streaming_on, AUTHOR_CHART,
    AUTHOR_TOKEN, LINK_TAB, LINK_TOKEN, WELCOME_PUBLISH, WELCOME_VIEW,
};
use super::*;
use crate::active_series::ActiveSeries;

const HOUR_S: i64 = 3_600;
const HOUR_MS: i64 = 3_600_000;

/// `count` hourly bars, the first one `start_hour` hours after the epoch.
fn hourly_bars(start_hour: i64, count: i64) -> Vec<Bar> {
    (0..count)
        .map(|i| Bar::new((start_hour + i) * HOUR_S, 10.0, 12.0, 9.0, 11.0))
        .collect()
}

/// An app whose active window shows a real bubble holding `bars`, bound as
/// the author of `AUTHOR_CHART` with `AUTHOR_TOKEN`.
async fn charted_author(bars: Vec<Bar>) -> (ChartApp, BubbleId) {
    let (bridge, _live_rx, _ready_rx) = DataBridge::new().await;
    let core = mlc_backend::BackendCore::new(Arc::new(bridge), None);
    let mut app = ChartApp::new_empty(core);
    let chart_id = app.panel_app.panel_grid.active_container_id().expect("new_empty seeds a window");
    let bid = app.create_auto_bubble([0.0, 0.0, 0.0, 0.0], "BTCUSDT".to_string(), Timeframe::h1());
    app.attach_chart_to_tag(chart_id, bid);
    {
        let window = app
            .panel_app
            .panel_grid
            .containers_mut()
            .get_mut(&chart_id)
            .expect("the active window");
        window.auto_tag_id = bid;
        window.current_tag_id = bid;
    }
    app.core.bubbles_mut().set_bid_instrument_full(
        bid,
        "BTCUSDT".to_string(),
        "binance".to_string(),
        "S".to_string(),
        Timeframe::h1(),
    );
    app.core.bubbles_mut().replace_bars(bid, bars);
    app.panel_app.active_preset_id = AUTHOR_CHART.to_string();
    app.note_chart_share_token(AUTHOR_CHART, AUTHOR_TOKEN);
    (app, bid)
}

fn new_line() -> Box<dyn mlc_core::drawing::primitives_v2::Primitive> {
    PrimitiveRegistry::global()
        .read()
        .ok()
        .and_then(|registry| registry.create("trend_line", &[(1_000, 10.0), (2_000, 20.0)], None))
        .expect("trend_line is registered")
}

fn stored_lines(app: &ChartApp, bid: BubbleId) -> usize {
    app.core.drawings().primitives(bid).len()
}

/// A `preview` frame carrying `prim`, as the sender builds it.
fn preview_frame(prim: &dyn mlc_core::drawing::primitives_v2::Primitive) -> String {
    let body = serde_json::json!({ "type_id": prim.type_id(), "json": prim.to_json() }).to_string();
    serde_json::json!({ "t": "preview", "b": body }).to_string()
}

fn frames_of(cmds: &[LiveCommand]) -> Vec<String> {
    cmds.iter()
        .flat_map(|cmd| match cmd {
            LiveCommand::Send(frames) => frames.clone(),
            _ => Vec::new(),
        })
        .collect()
}

fn frame_kind(frame: &str) -> String {
    serde_json::from_str::<serde_json::Value>(frame)
        .ok()
        .and_then(|v| v.get("t").and_then(|t| t.as_str()).map(str::to_string))
        .unwrap_or_default()
}

// ---- A1: the ack goes to the tab the push was built for ------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_ack_reaches_the_tab_the_push_was_built_for() {
    let (mut app, bid) = charted_author(hourly_bars(0, 50)).await;
    app.core.drawings_mut().add_external(bid, new_line());

    let push = app.share_ops_due(1_000_000.0).expect("the first push carries the chart");
    assert_eq!(push.preset_id, AUTHOR_CHART);
    assert_eq!(push.token, AUTHOR_TOKEN);
    assert!(app.share_streams[AUTHOR_CHART].pending.is_some());

    // The person switches tabs while the post is in flight.
    open_link_tab(&mut app, &["read"]);
    app.ack_share_push(&push.preset_id, true);
    let stream = &app.share_streams[AUTHOR_CHART];
    assert!(stream.pending.is_none(), "the ack must not be swallowed by the active tab");
    assert!(!stream.last.is_empty(), "an acknowledged push becomes the new baseline");

    // A failed post frees the stream too, so it can push again.
    app.panel_app.active_preset_id = AUTHOR_CHART.to_string();
    app.core.drawings_mut().add_external(bid, new_line());
    let second = app.share_ops_due(2_000_000.0).expect("the new line is due");
    open_link_tab(&mut app, &["read"]);
    app.ack_share_push(&second.preset_id, false);
    assert!(app.share_streams[AUTHOR_CHART].pending.is_none());
}

// ---- A2: stop frames and viewer release -----------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stop_frames_go_out_even_when_the_authors_tab_is_gone() {
    let (mut app, _bid) = charted_author(hourly_bars(0, 50)).await;
    go_live(&mut app, 1_000.0);
    open_socket(&mut app, 1_000.0, "t1", WELCOME_PUBLISH);

    // The tab is closed and its binding dropped: nothing names the chart now.
    app.panel_app.chart_share_tokens.clear();
    app.panel_app.active_preset_id = "some_other_chart".to_string();
    let cmds = app.live_link_step(2_000.0);
    let frames = frames_of(&cmds);
    assert!(frames.iter().any(|f| f.contains("\"end\"")), "no end frame in {frames:?}");
    assert!(frames.iter().any(|f| f.contains("\"v\"") && f.contains("off")), "no viewport release in {frames:?}");
    assert!(frames.iter().any(|f| f.contains("\"c\"") && f.contains("off")), "no cursor release in {frames:?}");
    assert_eq!(cmds.last(), Some(&LiveCommand::Close));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_viewer_is_released_when_its_socket_or_the_stream_ends() {
    let (mut viewer, _bid) = charted_author(hourly_bars(0, 50)).await;
    viewer.panel_app.chart_share_tokens.clear();
    open_link_tab(&mut viewer, &["read"]);
    viewer.apply_share_ops(LINK_TOKEN, false, false, true, &[]);
    open_socket(&mut viewer, 1_000.0, "t1", WELCOME_VIEW);

    let lock = |viewer: &mut ChartApp| {
        viewer.apply_edge_frame(r#"{"t":"v","b":{"view_ts":3600000,"bar_spacing":8.0}}"#);
        viewer.apply_edge_frame(r#"{"t":"c","b":"{\"ts\":3600000,\"price\":11.0,\"visible\":true}"}"#);
        let window = viewer.panel_app.panel_grid.active_container().expect("window");
        assert!(window.follow_remote_view, "the viewport frame must lock the view");
        assert!(window.crosshair.follow_remote, "the cursor frame must lock the cursor");
    };
    let locked = |viewer: &ChartApp| {
        let window = viewer.panel_app.panel_grid.active_container().expect("window");
        window.follow_remote_view || window.crosshair.follow_remote
    };

    // The socket dies: nothing would ever release the lock, so it is released.
    lock(&mut viewer);
    viewer.live_link_closed(1006, 5_000.0);
    assert!(!locked(&viewer));

    // The stream ends by the pull's word, before any stop frame arrives.
    viewer.live_link_step(5_000.0 + 60_000.0);
    viewer.live_link_ticket(LINK_TOKEN, Ok("t2".to_string()), 65_000.0);
    viewer.live_link_step(65_000.0);
    viewer.apply_edge_frame(WELCOME_VIEW);
    lock(&mut viewer);
    viewer.apply_share_ops(LINK_TOKEN, false, false, false, &[]);
    assert!(!locked(&viewer));
    assert_eq!(viewer.stream_ended.as_deref(), Some(LINK_TAB));
}

// ---- A3: the live preview never enters the drawing store ------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_remote_preview_is_painted_from_an_overlay_and_never_stored() {
    let (mut app, bid) = charted_author(hourly_bars(0, 50)).await;
    go_live(&mut app, 1_000.0);
    open_socket(&mut app, 1_000.0, "t1", WELCOME_VIEW);
    let stored = stored_lines(&app, bid);

    // A `preview` frame: painted from the overlay, the store is untouched.
    let prim = new_line();
    app.apply_edge_frame(&preview_frame(prim.as_ref()));
    assert_eq!(stored_lines(&app, bid), stored, "the preview must not enter the store");
    assert_eq!(app.remote_preview_overlay(bid).len(), 1);
    assert!(app.remote_preview_overlay(BubbleId(987_654)).is_empty(), "only the drawn chart paints it");
    assert!(
        app.share_streams[AUTHOR_CHART].local_to_key.is_empty(),
        "the preview must not get a stream key"
    );

    // Empty body clears it.
    app.apply_edge_frame(r#"{"t":"preview","b":""}"#);
    assert!(app.remote_preview_overlay(bid).is_empty());

    // The gesture's draw phase paints it the same way, and idle removes it.
    app.apply_edge_frame(
        r#"{"t":"g","b":{"phase":"draw","tool":"trend_line","selected":"","dragging":"","points":[[1000,10.0],[2000,20.0]]}}"#,
    );
    assert_eq!(stored_lines(&app, bid), stored);
    assert_eq!(app.remote_preview_overlay(bid).len(), 1);
    app.apply_edge_frame(r#"{"t":"g","b":{"phase":"idle","tool":"","selected":"","dragging":"","points":[]}}"#);
    assert!(app.remote_preview_overlay(bid).is_empty());
    assert_eq!(stored_lines(&app, bid), stored);

    // Another tab does not paint this tab's preview.
    app.apply_edge_frame(&preview_frame(new_line().as_ref()));
    app.panel_app.active_preset_id = "some_other_chart".to_string();
    assert!(app.remote_preview_overlay(bid).is_empty());
}

// ---- A4: the author's cold load keeps primitive ids ------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ids_are_kept_for_a_chart_with_a_link_and_for_a_link_tab() {
    let mut app = new_app().await;
    assert!(!app.keeps_primitive_ids(AUTHOR_CHART), "a chart nobody follows is renumbered as before");
    app.note_chart_share_token(AUTHOR_CHART, AUTHOR_TOKEN);
    assert!(app.keeps_primitive_ids(AUTHOR_CHART));
    app.share_keep_ids.insert(LINK_TAB.to_string());
    assert!(app.keeps_primitive_ids(LINK_TAB));
    // The link is revoked: the chart goes back to being an ordinary one.
    app.pending_share_admin = Some(crate::ShareAdminCmd::Revoke { token: AUTHOR_TOKEN.to_string() });
    let _ = app.take_share_admin();
    assert!(!app.keeps_primitive_ids(AUTHOR_CHART));
}

/// The object keys a first push carries for the drawn lines.
fn line_keys(push: &crate::share_stream::SharePush) -> Vec<String> {
    let mut keys: Vec<String> = push
        .ops
        .iter()
        .filter(|op| op.kind == "primitive_upsert")
        .map(|op| op.object_id.clone())
        .collect();
    keys.sort();
    keys
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_reloaded_author_keeps_the_ids_the_viewers_already_hold() {
    let (mut before, bid) = charted_author(hourly_bars(0, 50)).await;
    let line_id = before.core.drawings_mut().add_external(bid, new_line());
    let published = line_keys(&before.share_ops_due(1_000_000.0).expect("the chart is pushed"));
    assert_eq!(published, vec![format!("p:{line_id}")]);
    before.collect_state_into_preset(AUTHOR_CHART, "chart");
    let preset = before.core.presets().get(AUTHOR_CHART).cloned().expect("the chart was collected");

    // Reload without a republish: a fresh app, the binding restored, the
    // saved chart loaded cold.
    let mut after = new_app().await;
    after.panel_app.active_preset_id = AUTHOR_CHART.to_string();
    after.restore_author_shares(&[mlc_core::PersistedAuthorShare {
        chart_id: AUTHOR_CHART.to_string(),
        token: AUTHOR_TOKEN.to_string(),
    }]);
    after.apply_preset_cold(preset, web_time::Instant::now(), 0, 0, 0, 0, 0);

    let loaded_bid = after.panel_app.panel_grid.active_container().expect("window").current_tag_id;
    let ids: Vec<u64> = after.core.drawings().primitives(loaded_bid).iter().map(|p| p.data().id).collect();
    assert!(ids.contains(&line_id), "the line lost its id: {ids:?}, wanted {line_id}");
    // The keys the reloaded author speaks are the keys the viewers hold, so
    // the next upsert of that line updates theirs and does not add a second.
    let after_keys = line_keys(&after.share_ops_due(2_000_000.0).expect("the reloaded chart is pushed"));
    assert_eq!(after_keys, published);
}

// ---- A5: viewport and cursor travel as timestamps --------------------------

#[test]
fn positions_map_between_histories_by_timestamp() {
    let author = hourly_bars(0, 100);
    let viewer = hourly_bars(5, 100); // the viewer's history starts 5 bars later
    let a = ActiveSeries::Bars(&author);
    let v = ActiveSeries::Bars(&viewer);

    let ts = a.ts_at_position(20.25).expect("in range");
    assert_eq!(ts, 20 * HOUR_MS + HOUR_MS / 4);
    let mapped = v.position_at_ts(ts).expect("viewer has bars");
    assert!((mapped - 15.25).abs() < 1e-9, "20.25 on the author is 15.25 on the viewer, got {mapped}");

    // Before the viewer's first bar the position is negative, not clamped.
    let early = a.ts_at_position(2.0).expect("in range");
    assert!((v.position_at_ts(early).expect("mapped") + 3.0).abs() < 1e-9);
    // Past the last bar it extrapolates by the bar gap.
    let late = a.ts_at_position(104.0).expect("extrapolated");
    assert!((a.position_at_ts(late).expect("round trip") - 104.0).abs() < 1e-9);
    assert!(ActiveSeries::Bars(&[]).ts_at_position(1.0).is_none());
    assert!(ActiveSeries::Bars(&[]).position_at_ts(1).is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_viewport_reaches_the_viewer_at_the_same_moment_not_the_same_index() {
    // Author: 100 bars from hour 0, publishing the viewport.
    let (mut author, _bid) = charted_author(hourly_bars(0, 100)).await;
    go_live(&mut author, 1_000.0);
    turn_viewport_streaming_on(&mut author);
    open_socket(&mut author, 1_000.0, "t1", WELCOME_PUBLISH);
    author
        .panel_app
        .panel_grid
        .active_container_mut()
        .expect("window")
        .apply_shared_view(20.25, 9.0, None);
    let frames = frames_of(&author.live_link_step(2_000.0));
    let viewport = frames.iter().find(|f| frame_kind(f) == "v").expect("a viewport frame was sent");
    assert!(viewport.contains("view_ts"), "the frame must carry a timestamp: {viewport}");

    // Viewer: its history starts 5 hours later.
    let (mut viewer, _vbid) = charted_author(hourly_bars(5, 100)).await;
    viewer.panel_app.chart_share_tokens.clear();
    open_link_tab(&mut viewer, &["read"]);
    viewer.apply_share_ops(LINK_TOKEN, false, false, true, &[]);
    open_socket(&mut viewer, 1_000.0, "t9", WELCOME_VIEW);
    viewer.apply_edge_frame(viewport);
    let window = viewer.panel_app.panel_grid.active_container().expect("window");
    assert!(window.follow_remote_view);
    assert!(
        (window.viewport().view_start - 15.25).abs() < 1e-6,
        "expected 15.25, got {}",
        window.viewport().view_start
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_frame_without_a_timestamp_is_read_as_the_old_index_form() {
    let (mut viewer, _bid) = charted_author(hourly_bars(5, 100)).await;
    viewer.panel_app.chart_share_tokens.clear();
    open_link_tab(&mut viewer, &["read"]);
    viewer.apply_share_ops(LINK_TOKEN, false, false, true, &[]);
    open_socket(&mut viewer, 1_000.0, "t9", WELCOME_VIEW);

    viewer.apply_edge_frame(r#"{"t":"v","b":{"view_start":7.5,"bar_spacing":8.0,"price":null}}"#);
    let start = viewer.panel_app.panel_grid.active_container().expect("window").viewport().view_start;
    assert!((start - 7.5).abs() < 1e-9, "old form is an index: got {start}");

    viewer.apply_edge_frame(r#"{"t":"c","b":"{\"bar\":3.0,\"price\":11.0,\"visible\":true}"}"#);
    let bar = viewer.panel_app.panel_grid.active_container().expect("window").crosshair.bar_f64;
    assert!((bar - 3.0).abs() < 1e-9, "old cursor form is an index: got {bar}");

    // With a timestamp the index field is ignored, whatever it says.
    viewer.apply_edge_frame(&format!(
        r#"{{"t":"c","b":"{{\"ts\":{},\"bar\":99.0,\"price\":11.0,\"visible\":true}}"}}"#,
        10 * HOUR_MS
    ));
    let bar = viewer.panel_app.panel_grid.active_container().expect("window").crosshair.bar_f64;
    assert!((bar - 5.0).abs() < 1e-9, "hour 10 is bar 5 on this history, got {bar}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_cursor_is_sent_as_a_timestamp() {
    let (mut author, _bid) = charted_author(hourly_bars(0, 100)).await;
    go_live(&mut author, 1_000.0);
    author.share_streams.get_mut(AUTHOR_CHART).expect("stream").live_cursor = true;
    open_socket(&mut author, 1_000.0, "t1", WELCOME_PUBLISH);
    author
        .panel_app
        .panel_grid
        .active_container_mut()
        .expect("window")
        .set_crosshair_from_bar(20.5, 100.0, true, None);

    let frames = frames_of(&author.live_link_step(2_000.0));
    let cursor = frames.iter().find(|f| frame_kind(f) == "c").expect("a cursor frame was sent");
    let frame: serde_json::Value = serde_json::from_str(cursor).expect("frame is JSON");
    let body: serde_json::Value =
        serde_json::from_str(frame["b"].as_str().expect("cursor body is a string")).expect("body is JSON");
    assert_eq!(body["ts"].as_i64(), Some(20 * HOUR_MS + HOUR_MS / 2));
    assert_eq!(body["bar"].as_f64(), Some(20.5), "the index stays for one release");
}

// ---- A6: rights ------------------------------------------------------------

fn default_preset(app: &ChartApp) -> String {
    app.core.user().profile().ui.share_rights_preset.clone()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_rights_click_changes_only_that_charts_share() {
    let mut app = author_app().await;
    app.set_default_share_rights("view_only");
    assert_eq!(default_preset(&app), "view_only");

    // A chart with a link: the click patches THAT link, the default stays.
    app.set_link_rights("private");
    match app.pending_share_admin.take() {
        Some(crate::ShareAdminCmd::Patch { token, body }) => {
            assert_eq!(token, AUTHOR_TOKEN);
            assert!(body.contains("\"private\""), "unexpected patch {body}");
        }
        other => panic!("expected a patch of the chart's link, got {other:?}"),
    }
    assert_eq!(default_preset(&app), "view_only", "a per-chart click rewrote the profile default");

    // A chart without a link: remembered for its next Share only.
    app.panel_app.active_preset_id = "fresh_chart".to_string();
    app.set_link_rights("private_view");
    assert!(app.pending_share_admin.is_none(), "there is no link to patch");
    assert_eq!(default_preset(&app), "view_only");
    assert_eq!(app.share_publish_flags(), (false, "view", "private"));

    // Any other chart still starts from the profile default.
    app.panel_app.active_preset_id = "third_chart".to_string();
    assert_eq!(app.share_publish_flags(), (false, "view", "public"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn only_the_default_control_changes_the_profile_default() {
    let mut app = new_app().await;
    app.panel_app.active_preset_id = "some_chart".to_string();
    app.profile_dirty = false;
    app.set_default_share_rights("private");
    assert_eq!(default_preset(&app), "private");
    assert!(app.profile_dirty, "the default must be saved");
    assert_eq!(app.share_publish_flags(), (true, "view", "private"));

    // Unknown values fall back to the public default, never to garbage.
    app.set_default_share_rights("nonsense");
    assert_eq!(default_preset(&app), "public_clone");
}

// ---- B: dead and foreign links ---------------------------------------------

fn unavailable_notices(app: &ChartApp) -> usize {
    let title = mlc_core::i18n::t_share_viewer(mlc_core::i18n::ShareViewerKey::SharedChartGoneTitle);
    app.system_events().iter_newest_first().filter(|event| event.title == title).count()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dead_link_makes_no_tab_and_saves_nothing() {
    let mut app = new_app().await;
    let tabs = app.panel_app.open_tabs.clone();
    let presets = app.core.presets().len();

    app.share_link_unavailable();
    assert_eq!(app.panel_app.open_tabs, tabs, "no tab for a dead link");
    assert_eq!(app.core.presets().len(), presets, "no placeholder preset either");
    assert!(app.pending_share_tokens.is_empty());
    assert!(app.collect_open_shared_tabs().is_empty(), "nothing to persist into open_shared_tabs");
    assert_eq!(unavailable_notices(&app), 1, "the person is told once");

    // A link that could not be checked right now is kept for the next boot,
    // still without a tab, and asking twice does not save it twice.
    app.share_link_deferred("tok-later");
    app.share_link_deferred("tok-later");
    assert_eq!(app.panel_app.open_tabs, tabs);
    let saved = app.collect_open_shared_tabs();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].token, "tok-later");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_saved_tab_that_is_gone_is_told_once_and_dropped() {
    let saved = |token: &str, index: usize| PersistedSharedTab {
        token: token.to_string(),
        window_id: "win".to_string(),
        index,
        was_active: false,
    };
    let mut app = new_app().await;
    app.install_shared_tab_placeholders(&[saved("tok-gone", 0), saved("tok-alive", 1)]);
    let queued = app.take_pending_reopen_shares();
    assert_eq!(queued.len(), 2);
    let gone = queued.iter().find(|q| q.token == "tok-gone").expect("queued");
    let alive = queued.iter().find(|q| q.token == "tok-alive").expect("queued");
    assert!(app.has_open_tab(&gone.placeholder_id));
    app.profile_dirty = false;

    app.share_reopen_gone(&gone.placeholder_id);

    assert!(!app.has_open_tab(&gone.placeholder_id), "the dead tab must go");
    assert!(app.has_open_tab(&alive.placeholder_id), "the other saved tab stays");
    assert!(app.core.presets().get(&gone.placeholder_id).is_none());
    assert!(app.profile_dirty, "the smaller tab list must be saved");
    assert_eq!(unavailable_notices(&app), 1, "told once");
    let tokens: Vec<String> = app.collect_open_shared_tabs().into_iter().map(|t| t.token).collect();
    assert_eq!(tokens, vec!["tok-alive".to_string()], "the dead token is no longer saved");

    // The next boot restores only what is still saved.
    let mut next_boot = new_app().await;
    next_boot.install_shared_tab_placeholders(&app.collect_open_shared_tabs());
    let queued: Vec<String> = next_boot.take_pending_reopen_shares().into_iter().map(|q| q.token).collect();
    assert_eq!(queued, vec!["tok-alive".to_string()]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_restored_binding_whose_share_is_gone_is_pruned_at_once() {
    let mut app = new_app().await;
    app.panel_app.active_preset_id = AUTHOR_CHART.to_string();
    app.restore_author_shares(&[
        mlc_core::PersistedAuthorShare { chart_id: AUTHOR_CHART.to_string(), token: AUTHOR_TOKEN.to_string() },
        mlc_core::PersistedAuthorShare { chart_id: "other_chart".to_string(), token: "tok-other".to_string() },
    ]);
    let now = 100_000.0;
    assert_eq!(app.share_pull_due(now), Some((AUTHOR_TOKEN.to_string(), 0)), "the binding is polled");
    app.profile_dirty = false;

    // The pull answered 404.
    app.share_pull_gone(AUTHOR_TOKEN);
    assert!(app.share_follow_target().is_none(), "the chart has no channel any more");
    assert!(app.share_pull_due(now + 1_000_000.0).is_none(), "no re-polls of a dead share");
    assert!(app.profile_dirty, "the pruned binding must be saved");
    let saved: Vec<String> = app.collect_author_shares().into_iter().map(|s| s.chart_id).collect();
    assert_eq!(saved, vec!["other_chart".to_string()], "only the dead binding goes");
    assert!(app.live_link_step(now + 2_000_000.0).is_empty(), "no socket for a dead share");
}
