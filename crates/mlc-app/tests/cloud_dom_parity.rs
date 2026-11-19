//! Item 15 — the cloud surfaces (News/Links/Download overlay + Forum/Messages
//! Community modal) as uzor DOM, at pixel parity with the app's own paint.
//!
//! The host is the REAL one (`mlc_shell_fw::HostedApp`, toolbars/sidebar/chrome
//! on DOM as `with_dom` leaves them) wrapped in a test-local `App` that appends
//! the cloud DOM layer after the panel — exactly where the shell's own
//! `cloud_overlay_layer` arm paints it. The old pass is the same host with the
//! takeover gate off (the app paints the surface inside `render_modal_layer`).
//!
//! Determinism recipe (mirrors `mlc-shell-fw/src/tests.rs::host`): fresh
//! scratch HOME per host, fixed clock, live drain paused, pointer parked away
//! from the surface, no `advance` between the two paints of a compare.
//!
//! Run: `cargo test -p mylittlechart-app --features cloud --test
//! cloud_dom_parity -- --ignored --test-threads=1` with
//! `MLC_TEST_HOME_BASE=/tmp/mlc-home`. `MLC_CLOUD_DUMP=<dir>` writes the
//! old/new PNG pairs; `MLC_CLOUD_MODE` picks the DOM variant under test.

#![cfg(feature = "cloud")]

use mlc_app_layout::ui::modal_settings::{
    BoardUi, CloudNewsPost, CloudOverlayData, CloudOverlayKind, CloudReleaseEntry, CommunityTab, NewsTab,
};
use mlc_messenger_model::{
    BackPagination, Capabilities, EventId, Folder, LastEventPreview, Member, MemberRole, MessageContent,
    MessengerIntent, MessengerProvider, PowerLevel, PreviewContent, PublicRoomEntry, RoomId, RoomKind,
    RoomSummary, SendState, Timestamp, TimelineItem, UserEntry, UserId, VaultState,
};
use mlc_shell_fw::{HostSpec, HostedApp};
use uzor::render::{Painter, ShapeHelpers};
use uzor_framework::{
    App, ChromeModel, HeadlessHost, Intent, IntentCx, OverlayCx, OverlayModel, PanelCx, WindowId,
};

const W: f64 = 1280.0;
const H: f64 = 800.0;
const CHROME_H: f64 = mlc_app_input_shell::chrome::CHROME_HEIGHT;

type Host = HeadlessHost<HostSpec, CloudHost>;

/// How the cloud surfaces paint in this host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    /// Gate off: the app paints the surface inside its modal layer.
    App,
    /// Gate on (`PaintOff`); the legacy renderer paints directly on the
    /// panel's render target after the whole panel pass (the blocked
    /// experiment's "post-build direct paint").
    PostPaint,
    /// Gate on; the legacy renderer paints into one whole-window paint slot.
    WholeSlot,
    /// Gate on; the mlc-app-layout DOM tree (dim + dialog shell) with the
    /// legacy body in its paint slot — the takeover itself.
    Dom,
}

/// The real hosted app plus the cloud DOM layer the shell wires after the
/// panel. Everything else delegates unchanged.
struct CloudHost {
    inner: HostedApp,
    mode: Mode,
}

impl App for CloudHost {
    type Spec = HostSpec;

    fn chrome_model(&self, win: WindowId) -> ChromeModel {
        self.inner.chrome_model(win)
    }
    fn overlay_model(&self, win: WindowId, id: <HostSpec as uzor_framework::Spec>::Overlay) -> OverlayModel {
        self.inner.overlay_model(win, id)
    }
    fn overlay_body(&mut self, cx: &mut OverlayCx<'_, HostSpec>) {
        self.inner.overlay_body(cx)
    }
    fn intent(&mut self, intent: Intent<HostSpec>, cx: &mut IntentCx<'_, HostSpec>) {
        self.inner.intent(intent, cx)
    }

    fn panel(&mut self, cx: &mut PanelCx<'_, HostSpec>) {
        let p = &mut self.inner.chart.panel_app;
        p.dom_hosted.cloud_overlay = self.mode != Mode::App;
        p.dom_hosted.community = self.mode != Mode::App;
        // The shell's own cloud DOM layer paints the takeover in `panel`; only
        // the Dom mode wants it. The blocked-experiment modes paint the surface
        // themselves below and must not get a second (DOM) paint from the shell.
        self.inner.cloud_dom_layer = self.mode == Mode::Dom;
        self.inner.panel(cx);
        let r = cx.rect;
        let chart = &self.inner.chart;
        let co_open = chart.panel_app.cloud_overlay_state.is_open;
        let cm_open = chart.panel_app.community_state.is_open;
        if self.mode == Mode::App || (!co_open && !cm_open) {
            return;
        }
        let ft = chart.panel_app.frame_theme_for_render();
        let tt = chart.panel_app.toolbar_theme_for_render();
        // The host feeds `paint_over` (and so `render_modal_layer`) the WALL
        // clock, not the app-elapsed one — the cloud panes' relative
        // timestamps read it. Same source here, or the two passes label the
        // same timestamp differently ("2w ago" vs "just now").
        let now = web_time::SystemTime::now()
            .duration_since(web_time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        // A DOM shell click, dispatched after the paint borrows end.
        let mut click: Option<(String, f64, f64)> = None;
        match self.mode {
            Mode::PostPaint => {
                // The app's own coordinate space: origin at the chrome
                // strip's bottom edge, screen = the chart content.
                cx.render.save();
                cx.render.translate(r.x, r.y + CHROME_H);
                let mut ctx = mlc_shell_fw::ctx::ChartCtx::new(&mut *cx.render);
                let mut coord = uzor::input::InputCoordinator::default();
                if co_open {
                    mlc_app_layout::layout::modals::render_cloud_overlay(
                        &mut ctx,
                        r.width,
                        r.height - CHROME_H,
                        &chart.panel_app.cloud_overlay_state,
                        &ft,
                        &tt,
                        &mut coord,
                    );
                }
                if cm_open {
                    mlc_app_layout::layout::modals::render_community_modal(
                        &mut ctx,
                        r.width,
                        r.height - CHROME_H,
                        &chart.panel_app.community_state,
                        &ft,
                        &tt,
                        now,
                        &mut coord,
                        false,
                        chart.panel_app.messenger_provider_view(),
                    );
                }
                cx.render.restore();
            }
            Mode::WholeSlot => {
                let sheet = uzor::ui::domkit::sheet_with("").expect("sheet");
                let tree = uzor::ui::dom::El::new("div.k-root")
                    .css("height: 100%")
                    .child(
                        uzor::ui::dom::El::new("div.co-all")
                            .paint_slot("co:all")
                            .css(format!("position: absolute; left: 0px; top: 0px; width: {}px; height: {}px", r.width, r.height)),
                    );
                let (ft, tt) = (ft.clone(), tt.clone());
                let st = chart.panel_app.cloud_overlay_state.clone();
                let cm = chart.panel_app.community_state.clone();
                let provider = chart.panel_app.messenger_provider_view();
                let mut painter = |ctx: &mut dyn uzor::render::RenderContext, f: &uzor_framework::widgets::kit::SlotFrame<'_>| {
                    if f.key != "co:all" {
                        return;
                    }
                    let mut c = mlc_shell_fw::ctx::ChartCtx::new(ctx);
                    let mut coord = uzor::input::InputCoordinator::default();
                    c.save();
                    c.translate(r.x, r.y + CHROME_H);
                    if co_open {
                        mlc_app_layout::layout::modals::render_cloud_overlay(
                            &mut c, r.width, r.height - CHROME_H, &st, &ft, &tt, &mut coord,
                        );
                    }
                    if cm_open {
                        mlc_app_layout::layout::modals::render_community_modal(
                            &mut c, r.width, r.height - CHROME_H, &cm, &ft, &tt, now, &mut coord, false, provider,
                        );
                    }
                    c.restore();
                };
                let _ = uzor_framework::widgets::kit::dom(r, &sheet, tree).paint_slots(&mut painter).build(cx);
            }
            Mode::Dom => {
                // The takeover tree (mlc-app-layout::dom) is painted by the
                // shell's own `cloud_overlay_layer` inside `inner.panel` above —
                // this mode exercises that production wiring, including its click
                // dispatch. Painting the tree again here would double the dim
                // veil and the dialog shell.
            }
            Mode::App => unreachable!(),
        }
        if let Some((action, x, y)) = click {
            self.inner.chart.dispatch_panel_click(&action, x, y);
        }
    }
}

/// Fresh scratch profile + frozen clock + paused live drain, as the shell's
/// own hosted tests do it.
fn host() -> (Host, WindowId) {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let base = std::env::var("MLC_TEST_HOME_BASE")
        .unwrap_or_else(|_| std::env::temp_dir().join("mlc-cloud-test").display().to_string());
    let home = std::path::Path::new(&base).join(format!("c{n}"));
    let roaming = home.join("AppData").join("Roaming");
    let local = home.join("AppData").join("Local");
    let _ = std::fs::create_dir_all(&roaming);
    let _ = std::fs::create_dir_all(&local);
    std::env::set_var("HOME", &home);
    std::env::set_var("USERPROFILE", &home);
    std::env::set_var("APPDATA", &roaming);
    std::env::set_var("LOCALAPPDATA", &local);
    mlc_app::set_fixed_clock(Some(1_700_000_000));
    mlc_app::set_live_paused(true);
    let app = CloudHost { inner: HostedApp::with_dom(mlc_app::ChartApp::new("BTCUSDT")), mode: Mode::App };
    let (mut h, _) = HeadlessHost::new(app, HostedApp::runtime_config(W as u32, H as u32));
    for _ in 0..3 {
        h.advance(0.017);
    }
    let win = h.snapshot().windows[0].id;
    (h, win)
}

fn move_to(h: &mut Host, win: WindowId, x: f64, y: f64) {
    let pos = uzor_framework::Point { x, y };
    h.input(win, uzor_framework::InputEvent::Pointer(uzor_framework::PointerInput::Moved { pos, mods: Default::default() }));
}

/// One whole-window raster of the current state snapshot with the cloud
/// surfaces painted by `mode`. No advance: both passes of a compare must see
/// the SAME snapshot (the shell tests' own rule).
fn pixels(h: &mut Host, win: WindowId, mode: Mode) -> Vec<u8> {
    h.runtime_mut().app_mut().mode = mode;
    if mode != Mode::App {
        // Drop the app-hosted pass's legacy modal zones so they don't overlay
        // the DOM hits (the us/cs tests' own rule).
        *h.runtime_mut().app_mut().inner.chart.input_coordinator.borrow_mut() = Default::default();
    }
    let mut ctx = uzor_render_urx::UrxRenderContext::new(1.0);
    ctx.set_fill_color("#131722");
    ctx.fill_rect(0.0, 0.0, W, H);
    h.paint_window(win, 1.0, &mut ctx).expect("window");
    let scene = ctx.take_scene();
    let mut pm = uzor_urx_cpu::pixmap::Pixmap::new(W as u32, H as u32);
    uzor_urx_cpu::backend::CpuBackend::new().render(&scene, &mut pm).expect("urx cpu render");
    pm.pixels().to_vec()
}

fn dump(name: &str, old: &[u8], new: &[u8]) {
    let Some(dir) = std::env::var_os("MLC_CLOUD_DUMP") else { return };
    for (n, px) in [("old", old), ("new", new)] {
        let path = std::path::Path::new(&dir).join(format!("cloud-{n}-{name}.png"));
        let f = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
        let mut enc = png::Encoder::new(f, W as u32, H as u32);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().unwrap().write_image_data(px).unwrap();
    }
}

/// over-8 / exact counts plus the divergent bounding box and a strip/frame
/// split, so a dump says WHERE the two passes disagree, not just how much.
fn stats(name: &str, old: &[u8], new: &[u8]) -> usize {
    let (mut over, mut exact) = (0usize, 0usize);
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0usize, 0usize);
    let (mut strip, mut rest) = (0usize, 0usize);
    for (i, (a, b)) in old.chunks_exact(4).zip(new.chunks_exact(4)).enumerate() {
        let d = a.iter().zip(b.iter()).map(|(p, q)| p.abs_diff(*q)).max().unwrap_or(0);
        exact += usize::from(d > 0);
        if d > 8 {
            over += 1;
            let (x, y) = (i % W as usize, i / W as usize);
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
            if y < CHROME_H as usize {
                strip += 1;
            } else {
                rest += 1;
            }
        }
    }
    if over > 0 {
        eprintln!("{name}: {over} px over 8, {exact} differ; bbox x {x0}..={x1}, y {y0}..={y1}; strip {strip}, below {rest}");
    } else {
        eprintln!("{name}: {over} px over 8, {exact} differ");
    }
    over
}

fn post(mode: Mode) -> Mode {
    match std::env::var("MLC_CLOUD_MODE").as_deref() {
        Ok("post") => Mode::PostPaint,
        Ok("slot") => Mode::WholeSlot,
        Ok("dom") => Mode::Dom,
        _ => mode,
    }
}

fn news_posts() -> Vec<CloudNewsPost> {
    vec![
        CloudNewsPost {
            id: "p1".into(),
            kind: "post".into(),
            version: None,
            date: "2026-11-01".into(),
            title: "Cloud charts are here".into(),
            summary: "Share a chart with a link; the viewer clones it into their own workspace.".into(),
            body: "Share a chart with a link.\n\nThe viewer clones it into their own workspace, drawings and all.".into(),
            url: "https://example.invalid/blog/cloud-charts".into(),
        },
        CloudNewsPost {
            id: "p2".into(),
            kind: "patch".into(),
            version: Some("0.5.656".into()),
            date: "2026-10-28".into(),
            title: "0.5.656".into(),
            summary: "Markup engines, watchlist columns and a faster cold start.".into(),
            body: "Markup engines.\n\nWatchlist columns.\n\nA faster cold start.".into(),
            url: "https://example.invalid/blog/0-5-656".into(),
        },
    ]
}

fn seed_news(h: &mut Host) {
    let st = &mut h.runtime_mut().app_mut().inner.chart.panel_app.cloud_overlay_state;
    st.open(CloudOverlayKind::News);
    st.data = CloudOverlayData::News { posts: news_posts() };
}

fn seed_download(h: &mut Host) {
    let st = &mut h.runtime_mut().app_mut().inner.chart.panel_app.cloud_overlay_state;
    st.open(CloudOverlayKind::Download);
    st.data = CloudOverlayData::Ready {
        platform: "macos-arm64".into(),
        version: "0.5.656".into(),
        notes: "Markup engines and a faster cold start.".into(),
        download_url: "https://example.invalid/dl".into(),
        releases: vec![CloudReleaseEntry {
            platform: "win64".into(),
            version: "0.5.656".into(),
            file_size: 24_000_000,
            download_url: "https://example.invalid/dl-win".into(),
            created_at: "2026-10-28T10:00:00Z".into(),
        }],
    };
}

fn seed_community(h: &mut Host, tab: CommunityTab) {
    let st = &mut h.runtime_mut().app_mut().inner.chart.panel_app.community_state;
    st.open();
    st.tab = tab;
}

/// One surface compare: open it, settle, park the pointer, paint old vs DOM.
fn compare(h: &mut Host, win: WindowId, name: &str, mode: Mode) -> usize {
    for _ in 0..3 {
        h.advance(0.017);
    }
    move_to(h, win, 10.0, 10.0);
    for _ in 0..3 {
        h.advance(0.017);
    }
    let old = pixels(h, win, Mode::App);
    let new = pixels(h, win, mode);
    dump(name, &old, &new);
    stats(name, &old, &new)
}

#[test]
#[ignore = "a real ChartApp: run with --ignored and HOME at a scratch dir"]
fn the_cloud_overlay_paints_what_the_app_painted() {
    let mode = post(Mode::Dom);
    let (mut h, win) = host();
    seed_news(&mut h);
    let news = compare(&mut h, win, "news", mode);
    // The reading view (one post open), the Patch notes tab, a dragged
    // frame, and the Download overlay (collapsed and expanded).
    h.runtime_mut().app_mut().inner.chart.panel_app.cloud_overlay_state.selected_post = Some(0);
    let read = compare(&mut h, win, "news-read", mode);
    h.runtime_mut().app_mut().inner.chart.panel_app.cloud_overlay_state.selected_post = None;
    h.runtime_mut().app_mut().inner.chart.panel_app.cloud_overlay_state.news_tab = NewsTab::PatchNotes;
    let patch = compare(&mut h, win, "news-patch", mode);
    h.runtime_mut().app_mut().inner.chart.panel_app.cloud_overlay_state.position = Some((100.0, 80.0));
    let dragged = compare(&mut h, win, "news-dragged", mode);
    h.runtime_mut().app_mut().inner.chart.panel_app.cloud_overlay_state.close();
    seed_download(&mut h);
    let dl = compare(&mut h, win, "download", mode);
    h.runtime_mut().app_mut().inner.chart.panel_app.cloud_overlay_state.expanded = true;
    let dlx = compare(&mut h, win, "download-expanded", mode);
    let worst = news.max(read).max(patch).max(dragged).max(dl).max(dlx);
    assert!(worst <= 40, "cloud overlay over8: news {news}, read {read}, patch {patch}, dragged {dragged}, download {dl}, expanded {dlx}");
}

#[test]
#[ignore = "a real ChartApp: run with --ignored and HOME at a scratch dir"]
fn the_community_modal_paints_what_the_app_painted() {
    let mode = post(Mode::Dom);
    let (mut h, win) = host();
    seed_community(&mut h, CommunityTab::Forum);
    let forum = compare(&mut h, win, "community-forum", mode);
    // A loaded board list, then the Messages tab backed by an OUTSIDE
    // provider — the override the DOM wrapper must plumb through.
    {
        let st = &mut h.runtime_mut().app_mut().inner.chart.panel_app.community_state;
        st.forum2.caller_signed_in = true;
        st.forum2.caller_classes = vec!["paid".to_string()];
        st.forum2.boards = vec![
            BoardUi {
                slug: "announcements".into(),
                title: "Announcements".into(),
                description: "Release news and project updates.".into(),
                read_class: "user".into(),
                write_class: "trusted".into(),
                can_write: false,
                thread_count: 12,
                reply_count: 48,
                last_post_at: Some("2026-11-01T10:00:00Z".into()),
                last_thread_title: Some("0.5.656 is out".into()),
                last_post_author: Some("knyaz".into()),
            },
            BoardUi {
                slug: "charts".into(),
                title: "Chart talk".into(),
                description: "Show off your layouts.".into(),
                read_class: "user".into(),
                write_class: "paid".into(),
                can_write: true,
                thread_count: 3,
                reply_count: 9,
                last_post_at: None,
                last_thread_title: None,
                last_post_author: None,
            },
        ];
    }
    let boards = compare(&mut h, win, "community-boards", mode);
    {
        let app = h.runtime_mut().app_mut();
        app.inner.chart.set_messenger_provider(Some(Box::new(FakeProvider::ready())));
        let st = &mut app.inner.chart.panel_app.community_state;
        st.tab = CommunityTab::Messages;
        st.open_messenger_room(RoomId::from("!room:example"));
    }
    let messages = compare(&mut h, win, "community-messages", mode);
    let worst = forum.max(boards).max(messages);
    assert!(worst <= 40, "community over8: forum {forum}, boards {boards}, messages {messages}");
}

#[test]
#[ignore = "a real ChartApp: run with --ignored and HOME at a scratch dir"]
fn the_dom_overlay_keeps_the_apps_input() {
    let (mut h, win) = host();
    h.runtime_mut().app_mut().mode = Mode::Dom;
    seed_news(&mut h);
    for _ in 0..3 {
        h.advance(0.017);
    }
    // A body row click falls through the embed slot to the app's live zone.
    let row = h.runtime().app().inner.chart.input_coordinator.borrow()
        .widget_rect(&uzor::WidgetId::from("cloud_overlay:post:0"))
        .expect("post row zone registered by the app's layout pass");
    raw_click(&mut h, win, row.x + row.width / 2.0, row.y + row.height / 2.0);
    assert_eq!(
        h.runtime().app().inner.chart.panel_app.cloud_overlay_state.selected_post,
        Some(0),
        "the body click reached the app's handler through the DOM slot"
    );
    // The DOM close button dispatches under the legacy id.
    let close = h.runtime().app().inner.chart.input_coordinator.borrow()
        .widget_rect(&uzor::WidgetId::from("cloud_overlay:close"))
        .expect("close zone");
    raw_click(&mut h, win, close.x + close.width / 2.0, close.y + close.height / 2.0);
    assert!(!h.runtime().app().inner.chart.panel_app.cloud_overlay_state.is_open, "the DOM close closes");
    // Click-outside dismisses through the app's dimmer strips, not a DOM hit.
    seed_news(&mut h);
    for _ in 0..3 {
        h.advance(0.017);
    }
    raw_click(&mut h, win, 10.0, 400.0);
    assert!(!h.runtime().app().inner.chart.panel_app.cloud_overlay_state.is_open, "the dimmer click closes");
}

fn raw_click(h: &mut Host, win: WindowId, x: f64, y: f64) {
    let pos = uzor_framework::Point { x, y };
    let b = uzor::input::MouseButton::Left;
    h.input(win, uzor_framework::InputEvent::Pointer(uzor_framework::PointerInput::Moved { pos, mods: Default::default() }));
    h.advance(0.017);
    h.input(win, uzor_framework::InputEvent::Pointer(uzor_framework::PointerInput::Down { pos, button: b, mods: Default::default() }));
    h.advance(0.017);
    h.input(win, uzor_framework::InputEvent::Pointer(uzor_framework::PointerInput::Up { pos, button: b, mods: Default::default() }));
    h.advance(0.017);
    h.advance(0.017);
}

/// A hand-built outside `MessengerProvider`: one room, two confirmed
/// messages — enough for the Messages pane to paint a list and a timeline
/// through the DOM wrapper's `messenger_provider_override`.
#[derive(Clone, Default)]
struct FakeProvider {
    ready: bool,
}

impl FakeProvider {
    fn ready() -> Self {
        Self { ready: true }
    }
}

impl MessengerProvider for FakeProvider {
    fn change_counter(&self) -> u64 {
        1
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities::default()
    }
    fn vault_state(&self) -> VaultState {
        if self.ready { VaultState::Ready } else { VaultState::Locked }
    }
    fn last_error(&self) -> Option<String> {
        None
    }
    fn rooms(&self) -> Vec<RoomSummary> {
        vec![RoomSummary {
            id: RoomId::from("!room:example"),
            kind: RoomKind::Group,
            name: "Chart talk".to_string(),
            initials: "CT".to_string(),
            is_encrypted: false,
            is_public: false,
            notification_count: 2,
            highlight_count: 0,
            last_event: Some(LastEventPreview {
                sender: UserId::from("@bob:example"),
                content: PreviewContent::Text("nice layout".to_string()),
                ts: Timestamp(1),
            }),
            muted: false,
            pinned: false,
            archived: false,
            tags: Vec::new(),
            typing_user_ids: Vec::new(),
            peer_last_active: None,
            my_role: MemberRole::Member,
            my_power_level: PowerLevel(0),
            can_post: true,
        }]
    }
    fn room(&self, id: &RoomId) -> Option<RoomSummary> {
        self.rooms().into_iter().find(|r| &r.id == id)
    }
    fn timeline(&self, _id: &RoomId) -> Vec<TimelineItem> {
        vec![
            TimelineItem {
                event_id: EventId::from("$a"),
                sender: UserId::from("@bob:example"),
                sender_rank: None,
                ts: Timestamp(1),
                content: MessageContent::Text("hello".to_string()),
                reply_to: None,
                edited: false,
                reactions: Vec::new(),
                redacted: false,
                send_state: SendState::Sent,
                read_by: Vec::new(),
                pinned: false,
                shared_chart: None,
                mentions_me: false,
                forwarded: None,
            },
            TimelineItem {
                event_id: EventId::from("$b"),
                sender: UserId::from("@me:example"),
                sender_rank: None,
                ts: Timestamp(2),
                content: MessageContent::Text("nice layout".to_string()),
                reply_to: None,
                edited: false,
                reactions: Vec::new(),
                redacted: false,
                send_state: SendState::Sent,
                read_by: Vec::new(),
                pinned: false,
                shared_chart: None,
                mentions_me: false,
                forwarded: None,
            },
        ]
    }
    fn back_pagination(&self, _room: &RoomId) -> BackPagination {
        BackPagination::default()
    }
    fn members(&self, _room: &RoomId) -> Vec<Member> {
        Vec::new()
    }
    fn folders(&self) -> Vec<Folder> {
        Vec::new()
    }
    fn public_rooms(&self) -> Vec<PublicRoomEntry> {
        Vec::new()
    }
    fn user_search(&self) -> Vec<UserEntry> {
        Vec::new()
    }
    fn enqueue(&mut self, _intent: MessengerIntent) {}
}
