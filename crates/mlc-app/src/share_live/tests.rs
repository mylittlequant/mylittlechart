//! ChartApp-level tests of the live socket link: no browser, no network.
//! The shell's part is played by the test itself: it feeds tickets, welcome
//! frames and close codes in, and reads the commands back out.

use std::sync::Arc;

use mlc_backend_data_bridge::DataBridge;

use super::*;
use crate::share_viewer::SharedChartMeta;

pub(super) const AUTHOR_CHART: &str = "chart_author";
pub(super) const AUTHOR_TOKEN: &str = "tok-author";
pub(super) const LINK_TAB: &str = "shared_link";
pub(super) const LINK_TOKEN: &str = "tok-link";
pub(super) const WELCOME_PUBLISH: &str = r#"{"t":"welcome","publish":true}"#;
pub(super) const WELCOME_VIEW: &str = r#"{"t":"welcome","publish":false}"#;

pub(super) async fn new_app() -> ChartApp {
    let (bridge, _live_rx, _ready_rx) = DataBridge::new().await;
    let core = mlc_backend::BackendCore::new(Arc::new(bridge), None);
    ChartApp::new_empty(core)
}

/// An author tab bound to `AUTHOR_TOKEN`, active, Live off.
pub(super) async fn author_app() -> ChartApp {
    let mut app = new_app().await;
    app.panel_app.active_preset_id = AUTHOR_CHART.to_string();
    app.note_chart_share_token(AUTHOR_CHART, AUTHOR_TOKEN);
    app
}

/// Turn Live on at `now_ms` for the active author tab.
pub(super) fn go_live(app: &mut ChartApp, now_ms: f64) {
    app.live_link_step(now_ms);
    app.set_share_live(true);
}

/// Drive the link from Live-on to an open socket with the given welcome.
pub(super) fn open_socket(app: &mut ChartApp, now_ms: f64, ticket: &str, welcome: &str) {
    let token = app.edge_live_token().expect("Live is on");
    assert!(matches!(
        app.live_link_step(now_ms).as_slice(),
        [LiveCommand::FetchTicket { .. }]
    ));
    app.live_link_ticket(&token, Ok(ticket.to_string()), now_ms);
    assert_eq!(
        app.live_link_step(now_ms),
        vec![LiveCommand::Connect { token, ticket: ticket.to_string() }]
    );
    app.apply_edge_frame(welcome);
}

/// A link tab that only receives, open on `LINK_TAB`.
pub(super) fn open_link_tab(app: &mut ChartApp, rights: &[&str]) {
    app.panel_app.active_preset_id = LINK_TAB.to_string();
    app.readonly_preset_ids.insert(LINK_TAB.to_string());
    app.shared_chart_meta.insert(
        LINK_TAB.to_string(),
        SharedChartMeta {
            token: LINK_TOKEN.to_string(),
            title: "t".to_string(),
            owner_label: "o".to_string(),
            kind: "live".to_string(),
            rights: rights.iter().map(|r| (*r).to_string()).collect(),
            expires_at: String::new(),
            revision: 1,
        },
    );
}

pub(super) fn turn_viewport_streaming_on(app: &mut ChartApp) {
    let stream = app.share_streams.get_mut(AUTHOR_CHART).expect("author stream exists");
    stream.live_viewport = true;
}

#[test]
fn back_off_doubles_from_one_second_and_stops_at_thirty() {
    let waits: Vec<f64> = (1..=9).map(reconnect_delay_ms).collect();
    assert_eq!(
        waits,
        vec![1_000.0, 2_000.0, 4_000.0, 8_000.0, 16_000.0, 30_000.0, 30_000.0, 30_000.0, 30_000.0]
    );
    assert_eq!(reconnect_delay_ms(0), RECONNECT_FIRST_MS);
    assert_eq!(reconnect_delay_ms(u32::MAX), RECONNECT_MAX_MS);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn failed_ticket_fetches_wait_the_back_off_and_a_welcome_resets_it() {
    let mut app = author_app().await;
    go_live(&mut app, 1_000.0);
    let mut now = 1_000.0;
    for wait in [1_000.0, 2_000.0, 4_000.0, 8_000.0, 16_000.0, 30_000.0, 30_000.0] {
        assert!(matches!(
            app.live_link_step(now).as_slice(),
            [LiveCommand::FetchTicket { .. }]
        ));
        app.live_link_ticket(AUTHOR_TOKEN, Err("503".to_string()), now);
        // One millisecond early: still waiting.
        assert!(app.live_link_step(now + wait - 1.0).is_empty(), "retry came before {wait} ms");
        now += wait;
    }
    // A good attempt, then a welcome, starts the sequence over at one second.
    assert!(matches!(
        app.live_link_step(now).as_slice(),
        [LiveCommand::FetchTicket { .. }]
    ));
    app.live_link_ticket(AUTHOR_TOKEN, Ok("t-ok".to_string()), now);
    assert!(matches!(
        app.live_link_step(now).as_slice(),
        [LiveCommand::Connect { .. }]
    ));
    app.apply_edge_frame(WELCOME_PUBLISH);
    assert_eq!(app.live_link.failures(), 0);
    app.live_link_closed(1001, now);
    assert!(app.live_link_step(now + 999.0).is_empty());
    assert!(matches!(
        app.live_link_step(now + 1_000.0).as_slice(),
        [LiveCommand::FetchTicket { .. }]
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_welcome_frame_decides_publishing_or_viewing() {
    let mut app = author_app().await;
    go_live(&mut app, 1_000.0);
    assert_eq!(app.share_stream_button_state(), ShareStreamButtonState::Connecting);
    open_socket(&mut app, 1_000.0, "t1", WELCOME_PUBLISH);
    assert!(app.live_link_publishing());
    assert_eq!(app.share_stream_button_state(), ShareStreamButtonState::Live);

    let mut viewer = new_app().await;
    open_link_tab(&mut viewer, &["read"]);
    viewer.apply_share_ops(LINK_TOKEN, false, false, true, &[]);
    open_socket(&mut viewer, 1_000.0, "t9", WELCOME_VIEW);
    assert!(!viewer.live_link_publishing());
    assert_eq!(viewer.share_stream_button_state(), ShareStreamButtonState::Viewing);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_viewer_never_enqueues_an_outbound_frame() {
    // An author tab whose welcome says "viewer": the local rights say it may
    // send and every stream toggle is on, the server's word still wins.
    let mut app = author_app().await;
    go_live(&mut app, 1_000.0);
    turn_viewport_streaming_on(&mut app);
    open_socket(&mut app, 2_000.0, "t1", WELCOME_VIEW);
    for step in 0..5 {
        let cmds = app.live_link_step(2_100.0 + f64::from(step) * 100.0);
        assert!(cmds.is_empty(), "a viewer socket got commands: {cmds:?}");
    }
    assert!(app.take_edge_frames().is_none());

    // Same tab, same toggles, but the welcome says publish: frames flow.
    let mut publisher = author_app().await;
    go_live(&mut publisher, 1_000.0);
    turn_viewport_streaming_on(&mut publisher);
    open_socket(&mut publisher, 2_000.0, "t1", WELCOME_PUBLISH);
    let sent = publisher.live_link_step(2_100.0);
    assert!(
        matches!(sent.as_slice(), [LiveCommand::Send(frames)] if !frames.is_empty()),
        "the publisher produced no frames: {sent:?}"
    );

    // A link tab without an edit right is a viewer even if the server were
    // to say publish: the local right is checked too.
    let mut link = new_app().await;
    open_link_tab(&mut link, &["read"]);
    link.apply_share_ops(LINK_TOKEN, false, false, true, &[]);
    open_socket(&mut link, 1_000.0, "t2", WELCOME_PUBLISH);
    assert!(link.take_edge_frames().is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rights_changed_reconnects_at_once_with_a_fresh_ticket() {
    let mut app = author_app().await;
    go_live(&mut app, 1_000.0);
    open_socket(&mut app, 1_000.0, "ticket-1", WELCOME_VIEW);

    app.live_link_closed(CLOSE_RIGHTS_CHANGED, 5_000.0);
    // Same frame, no wait: a new ticket is requested.
    assert_eq!(
        app.live_link_step(5_000.0),
        vec![LiveCommand::FetchTicket { token: AUTHOR_TOKEN.to_string() }]
    );
    app.live_link_ticket(AUTHOR_TOKEN, Ok("ticket-2".to_string()), 5_000.0);
    assert_eq!(
        app.live_link_step(5_000.0),
        vec![LiveCommand::Connect { token: AUTHOR_TOKEN.to_string(), ticket: "ticket-2".to_string() }]
    );
    // The new welcome carries the new right.
    app.apply_edge_frame(WELCOME_PUBLISH);
    assert!(app.live_link_publishing());

    // A second 4003 with no welcome in between waits like any failure.
    app.live_link_closed(CLOSE_RIGHTS_CHANGED, 6_000.0);
    assert!(matches!(app.live_link_step(6_000.0).as_slice(), [LiveCommand::FetchTicket { .. }]));
    app.live_link_ticket(AUTHOR_TOKEN, Ok("ticket-3".to_string()), 6_000.0);
    app.live_link_step(6_000.0);
    app.live_link_closed(CLOSE_RIGHTS_CHANGED, 6_100.0);
    assert!(app.live_link_step(6_100.0).is_empty());
    assert!(matches!(app.live_link_step(7_100.0).as_slice(), [LiveCommand::FetchTicket { .. }]));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn policy_closes_wait_at_least_the_policy_wait() {
    let mut app = author_app().await;
    go_live(&mut app, 1_000.0);
    open_socket(&mut app, 1_000.0, "t1", WELCOME_PUBLISH);
    app.live_link_closed(CLOSE_POLICY, 10_000.0);
    assert!(app.live_link_step(10_000.0 + POLICY_WAIT_MS - 1.0).is_empty());
    assert!(matches!(
        app.live_link_step(10_000.0 + POLICY_WAIT_MS).as_slice(),
        [LiveCommand::FetchTicket { .. }]
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn repeated_failures_show_the_error_state_and_a_welcome_clears_it() {
    let mut app = author_app().await;
    go_live(&mut app, 1_000.0);
    app.live_link_step(1_000.0);
    app.live_link_ticket(AUTHOR_TOKEN, Err("404".to_string()), 1_000.0);
    // The first failure is routine.
    assert_eq!(app.share_stream_button_state(), ShareStreamButtonState::Connecting);
    app.live_link_step(2_000.0);
    app.live_link_ticket(AUTHOR_TOKEN, Err("404".to_string()), 2_000.0);
    assert_eq!(app.share_stream_button_state(), ShareStreamButtonState::Error);
    assert_eq!(app.panel_app.toolbar_state.share_stream, ShareStreamButtonState::Connecting);
    app.live_link_step(2_001.0);
    assert_eq!(app.panel_app.toolbar_state.share_stream, ShareStreamButtonState::Error);

    app.live_link_step(4_000.0);
    app.live_link_ticket(AUTHOR_TOKEN, Ok("t".to_string()), 4_000.0);
    app.live_link_step(4_000.0);
    app.apply_edge_frame(WELCOME_PUBLISH);
    assert_eq!(app.share_stream_button_state(), ShareStreamButtonState::Live);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_socket_without_a_welcome_is_given_up_on() {
    let mut app = author_app().await;
    go_live(&mut app, 1_000.0);
    app.live_link_step(1_000.0);
    app.live_link_ticket(AUTHOR_TOKEN, Ok("t".to_string()), 1_000.0);
    assert!(matches!(app.live_link_step(1_000.0).as_slice(), [LiveCommand::Connect { .. }]));
    assert!(app.live_link_step(1_000.0 + WELCOME_TIMEOUT_MS).is_empty());
    let cmds = app.live_link_step(1_001.0 + WELCOME_TIMEOUT_MS);
    assert_eq!(cmds, vec![LiveCommand::Close]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn leaving_live_says_goodbye_and_closes_the_socket() {
    let mut app = author_app().await;
    go_live(&mut app, 1_000.0);
    open_socket(&mut app, 1_000.0, "t1", WELCOME_PUBLISH);

    app.set_share_live(false);
    let cmds = app.live_link_step(2_000.0);
    match cmds.as_slice() {
        [LiveCommand::Send(frames), LiveCommand::Close] => {
            assert!(frames.iter().any(|f| f.contains("\"end\"")), "no end frame in {frames:?}");
        }
        other => panic!("expected stop frames then close, got {other:?}"),
    }
    assert_eq!(app.share_stream_button_state(), ShareStreamButtonState::Off);
    assert!(app.live_link_step(3_000.0).is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn switching_to_a_tab_without_a_channel_closes_the_socket() {
    let mut app = author_app().await;
    go_live(&mut app, 1_000.0);
    open_socket(&mut app, 1_000.0, "t1", WELCOME_VIEW);

    app.panel_app.active_preset_id = "some_other_chart".to_string();
    assert_eq!(app.live_link_step(2_000.0), vec![LiveCommand::Close]);
    assert_eq!(app.share_stream_button_state(), ShareStreamButtonState::Off);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pull_cannot_flip_the_tabs_live_state_back() {
    let mut app = author_app().await;
    go_live(&mut app, 1_000.0);
    assert!(app.share_live_on());

    // A pull answered before the PATCH landed still says "not live".
    app.live_link_step(1_500.0);
    app.apply_share_ops(AUTHOR_TOKEN, false, true, false, &[]);
    assert!(app.share_live_on(), "a stale pull turned Live off");

    // The server agrees: the click is confirmed and the hold is released.
    app.apply_share_ops(AUTHOR_TOKEN, false, true, true, &[]);
    app.apply_share_ops(AUTHOR_TOKEN, false, true, false, &[]);
    assert!(!app.share_live_on(), "a confirmed state must follow the server again");

    // Without a confirmation the hold simply runs out.
    app.live_link_step(10_000.0);
    app.set_share_live(true);
    app.live_link_step(10_000.0 + LIVE_HOLD_FOR_TEST - 1.0);
    app.apply_share_ops(AUTHOR_TOKEN, false, true, false, &[]);
    assert!(app.share_live_on());
    app.live_link_step(10_000.0 + LIVE_HOLD_FOR_TEST);
    app.apply_share_ops(AUTHOR_TOKEN, false, true, false, &[]);
    assert!(!app.share_live_on());
}

/// `LIVE_INTENT_HOLD_MS`, re-exported under a test-local name.
const LIVE_HOLD_FOR_TEST: f64 = crate::share_stream::LIVE_INTENT_HOLD_MS;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn another_tabs_pull_does_not_touch_this_tabs_live_state() {
    let mut app = author_app().await;
    go_live(&mut app, 1_000.0);
    assert!(app.share_live_on());

    // The author's tab is not the active one: its token's pull is not applied,
    // and the link tab's pull never reaches the author's stream.
    open_link_tab(&mut app, &["read"]);
    app.apply_share_ops(AUTHOR_TOKEN, false, true, false, &[]);
    app.apply_share_ops(LINK_TOKEN, false, false, false, &[]);
    assert!(!app.share_live_on(), "the link tab has its own state");

    app.panel_app.active_preset_id = AUTHOR_CHART.to_string();
    assert!(app.share_live_on(), "the author's tab kept its Live state");
    assert_eq!(app.edge_live_token(), Some(AUTHOR_TOKEN.to_string()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_authors_token_survives_a_reload_and_restores_the_channel() {
    let before = author_app().await;
    assert!(before.profile_dirty, "binding a token must schedule a profile save");
    let saved = before.build_window_state().author_shares;
    assert_eq!(
        saved,
        vec![mlc_core::PersistedAuthorShare {
            chart_id: AUTHOR_CHART.to_string(),
            token: AUTHOR_TOKEN.to_string(),
        }]
    );

    // The state survives the profile's JSON round trip.
    let json = serde_json::to_string(&saved).expect("author shares serialize");
    let restored: Vec<mlc_core::PersistedAuthorShare> = serde_json::from_str(&json).expect("and load back");

    // Reload: a fresh app knows nothing until the profile is applied.
    let mut after = new_app().await;
    after.panel_app.active_preset_id = AUTHOR_CHART.to_string();
    assert_eq!(after.share_follow_target().map(|f| f.token), None);
    after.restore_author_shares(&restored);
    assert_eq!(after.share_follow_target().map(|f| f.token), Some(AUTHOR_TOKEN.to_string()));

    // The first pull reports the server-side Live state and the socket follows.
    let now = 100_000.0;
    assert_eq!(after.share_pull_due(now), Some((AUTHOR_TOKEN.to_string(), 0)));
    after.apply_share_ops(AUTHOR_TOKEN, false, true, true, &[]);
    assert_eq!(after.edge_live_token(), Some(AUTHOR_TOKEN.to_string()));
    assert_eq!(
        after.live_link_step(now),
        vec![LiveCommand::FetchTicket { token: AUTHOR_TOKEN.to_string() }]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_share_list_binds_the_newest_live_row_of_a_chart() {
    fn row(token: &str, created_at: &str) -> mlc_app_layout::ui::modal_settings::ShareAdminRow {
        mlc_app_layout::ui::modal_settings::ShareAdminRow {
            token: token.to_string(),
            kind: "live".to_string(),
            chart_id: AUTHOR_CHART.to_string(),
            created_at: created_at.to_string(),
            ..Default::default()
        }
    }
    let older = row("tok-old", "2026-09-01T10:00:00+00:00");
    let newer = row("tok-new", "2026-09-02T10:00:00+00:00");

    // Newest first, as the server sends it, and the other way round.
    for rows in [vec![newer.clone(), older.clone()], vec![older, newer]] {
        let mut app = new_app().await;
        app.apply_share_rows(rows);
        assert_eq!(app.panel_app.chart_share_tokens.get(AUTHOR_CHART).map(String::as_str), Some("tok-new"));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn revoking_a_link_drops_the_authors_binding() {
    let mut app = author_app().await;
    app.pending_share_admin = Some(crate::ShareAdminCmd::Revoke { token: AUTHOR_TOKEN.to_string() });
    let _ = app.take_share_admin();
    assert!(app.panel_app.chart_share_tokens.is_empty());
    assert!(app.collect_author_shares().is_empty());
}

#[test]
fn every_button_state_has_an_icon_and_a_tooltip_in_english_and_russian() {
    use mlc_core::i18n::{Language, ShareViewerKey};
    for state in [
        ShareStreamButtonState::Connecting,
        ShareStreamButtonState::Live,
        ShareStreamButtonState::Viewing,
        ShareStreamButtonState::Error,
    ] {
        let icon = state.icon_name().expect("a live state swaps the icon");
        assert!(mlc_core::state::icons::icon_svg(icon).is_some(), "no svg for {icon}");
    }
    assert_eq!(ShareStreamButtonState::Off.icon_name(), None);
    assert!(mlc_core::state::icons::icon_svg("Video").is_some());
    for key in [
        ShareViewerKey::StreamStateConnecting,
        ShareViewerKey::StreamStateLive,
        ShareViewerKey::StreamStateViewing,
        ShareViewerKey::StreamStateError,
    ] {
        assert!(!key.get(Language::En).is_empty());
        assert!(!key.get(Language::Ru).is_empty());
        assert_ne!(key.get(Language::En), key.get(Language::Ru));
    }
    assert!(ShareStreamButtonState::Live.is_pressed());
    assert!(ShareStreamButtonState::Viewing.is_pressed());
    assert!(!ShareStreamButtonState::Error.is_pressed());
    assert!(!ShareStreamButtonState::Connecting.is_pressed());
}
