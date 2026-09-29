//! Community modal lab page — hosts the REAL `render_community_modal`
//! (Forum + Messages, `mlc-app-layout`'s `layout::modals::community`)
//! against hand-built `CommunityState` fixtures, so the coordinator can see
//! the actual production renderer without signing in anywhere. Selected via
//! `?page=community` in the URL query string (`index.html`'s own bootstrap
//! script branches `start()` here instead of booting the primitive-settings
//! lab; see `crate::start`), `&state=<name>` picks which fixture draws
//! (`PageState::from_query`), and `&theme=<preset>` picks a
//! `ThemeManager` preset (default: `dark`, same as every other page in this
//! crate — `ThemeManager::new()`'s own doc).
//!
//! A SEPARATE render loop from `crate::lib`'s own `LabState`: the Community
//! modal draws itself full-window with no chart/toolbar/rail chrome around
//! it (unlike the primitive-settings grid, which needs the stub shell for
//! scale), so this page needs none of that machinery — only a
//! `ThemeManager` and a plain root `InputCoordinator` (no scoped region:
//! `render_community_modal` draws at the window origin, never translated,
//! so the coordinate spaces already match). Static content, no DOM event
//! polling — the four fixtures are picked once at boot from the URL, not by
//! clicking anything on screen.
//!
//! Every field this module sets on `CommunityState`/`ForumV2State`/
//! `DmState` is public — nothing this task needed was blocked from the
//! outside. `DmConversationUi::last_message_preview`/`last_message_mine`
//! feed the chat-list row's own preview line via `dm_messenger_provider.rs`'s
//! `reads::rooms` (`RoomSummary::last_event`, fixed 2026-09-25 — that method
//! used to hardcode `None` regardless of these fields; see its own doc for
//! the current mapping). `DmConversationUi::peer_rank`/`peer_fingerprint`
//! are read a SECOND way, straight off `state.dm.conversations` by the open
//! conversation's own header — not through the provider at all.
//!
//! Only four states are wired (`forum-index`, `forum-thread`,
//! `messages-locked`, `messages-chat`, `forum-index` is the default for an
//! unrecognised or missing `state=`) — `ForumV2View::BoardThreads` (a
//! board's own thread list) exists in the state model too, but is not one
//! of the four fixed states this page exposes.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;

use mlc_app_layout::layout::modals::community::render_community_modal;
use mlc_app_layout::ui::modal_settings::{
    BoardUi, CommunityState, CommunityTab, DmConversationUi, DmMessageBody, DmMessageUi, DmState,
    DmUnavailableReason, DmView, ForumReplyV2Ui, ForumThreadV2Detail, ForumV2State, ForumV2View,
};
use mlc_core::theme::ThemeManager;
use uzor::input::{InputCoordinator, InputState};
use uzor::render::{Painter, ShapeHelpers};

use crate::render_context::LabRenderContext;
use crate::theme;

/// Fixed viewport this page renders at — see this module's own top doc.
/// `index.html`'s own bootstrap script sizes `#chart` to match when
/// `?page=community` is present, so the canvas is exactly this size
/// regardless of the hosting browser window.
const WINDOW_W: f64 = 1280.0;
const WINDOW_H: f64 = 800.0;

const MINUTE_MS: f64 = 60_000.0;
const HOUR_MS: f64 = 3_600_000.0;
const DAY_MS: f64 = 86_400_000.0;

/// Whether `crate::start` should boot this page instead of the primitive
/// lab — `?page=community` in the URL query string.
pub fn wants_community_page() -> bool {
    query_param("page").as_deref() == Some("community")
}

fn query_param(name: &str) -> Option<String> {
    let win = web_sys::window()?;
    let search = win.location().search().ok()?;
    let params = web_sys::UrlSearchParams::new_with_str(&search).ok()?;
    params.get(name)
}

/// Which fixture [`CommunityState`] this page shows — `?state=...`, see
/// this module's own top doc for the four names. `messages-group` (UI-1,
/// 2026-09-25) is a fifth: it does NOT feed `DmState` at all — its
/// `CommunityState.dm` stays a bare shell (just enough for `messenger_
/// overlay.rs`'s own UI-side reads: which room is open, the draft field id)
/// — the actual room/timeline data comes from
/// `community_messenger_fixture::MessengerFixture`, passed to
/// `render_community_modal`'s own `messenger_provider_override`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PageState {
    ForumIndex,
    ForumThread,
    MessagesLocked,
    MessagesChat,
    MessagesGroup,
    /// Concept v1 screen "2 · Канал" — the "BTC levels" channel open,
    /// subscriber footer (Mute/Unmute). Same `MessengerFixture`-backed
    /// shell as [`Self::MessagesGroup`] (see that variant's own doc).
    MessagesChannel,
    /// Concept v1 screen "2 · меню чата" — the chat-list row context menu
    /// open (right-click on "ETH flows") with its own "Add to folder"
    /// submenu open beside it. Same shell as [`Self::MessagesGroup`], plus
    /// a static [`mlc_app_layout::ui::context_menu::ContextMenuState`]
    /// this page renders on top with [`mlc_app_layout::layout::modals::
    /// context_menu::render_context_menu`] directly — no click dispatch
    /// exists in this crate (this module's own top doc), so the menu is
    /// simply always open for this state.
    MessagesMenu,
}

impl PageState {
    fn from_query() -> Self {
        match query_param("state").as_deref() {
            Some("forum-thread") => Self::ForumThread,
            Some("messages-locked") => Self::MessagesLocked,
            Some("messages-chat") => Self::MessagesChat,
            Some("messages-group") => Self::MessagesGroup,
            Some("messages-channel") => Self::MessagesChannel,
            Some("messages-menu") => Self::MessagesMenu,
            _ => Self::ForumIndex,
        }
    }

    /// Whether this state hosts its room/timeline data from
    /// [`crate::community_messenger_fixture::MessengerFixture`] rather than
    /// `DmState` — see [`Self::MessagesGroup`]'s own doc.
    fn uses_messenger_fixture(self) -> bool {
        matches!(self, Self::MessagesGroup | Self::MessagesChannel | Self::MessagesMenu)
    }
}

// ─── Fixtures ───────────────────────────────────────────────────────────────

/// An RFC3339 timestamp `offset_ms` before `now_ms` — every fixture
/// timestamp is built this way (rather than a hardcoded date) so the day
/// separators and "N ago" labels the real renderer computes off
/// `current_time_ms` always read sensibly, no matter which real day this
/// page is opened on.
fn ts_before(now_ms: f64, offset_ms: f64) -> String {
    let date = js_sys::Date::new(&JsValue::from_f64(now_ms - offset_ms));
    date.to_iso_string().as_string().unwrap_or_default()
}

/// Board index — content mirrors `designs/2026-09-23-forum/forum_concept_v1.html`'s
/// own state 1 (board names, descriptions, per-board write class).
fn boards(now_ms: f64) -> Vec<BoardUi> {
    vec![
        BoardUi {
            slug: "announcements".to_string(),
            title: "Announcements".to_string(),
            description: "Official updates, releases and patch notes.".to_string(),
            read_class: "public".to_string(),
            write_class: "admin".to_string(),
            can_write: false,
            thread_count: 12,
            reply_count: 58,
            last_post_at: Some(ts_before(now_ms, 3.0 * HOUR_MS)),
            last_thread_title: Some("0.5.607 — native wave notes".to_string()),
        },
        BoardUi {
            slug: "general".to_string(),
            title: "General Discussion".to_string(),
            description: "Anything chart-related that doesn't fit elsewhere.".to_string(),
            read_class: "public".to_string(),
            write_class: "user".to_string(),
            can_write: true,
            thread_count: 214,
            reply_count: 1842,
            last_post_at: Some(ts_before(now_ms, 4.0 * HOUR_MS)),
            last_thread_title: Some("How do I set up alerts on Renko bars?".to_string()),
        },
        BoardUi {
            slug: "feature-requests".to_string(),
            title: "Feature Requests".to_string(),
            description: "Vote and discuss what ships next.".to_string(),
            read_class: "public".to_string(),
            write_class: "user".to_string(),
            can_write: true,
            thread_count: 96,
            reply_count: 640,
            last_post_at: Some(ts_before(now_ms, 41.0 * MINUTE_MS)),
            last_thread_title: Some("Multi-timeframe watchlist column".to_string()),
        },
        BoardUi {
            slug: "strategies".to_string(),
            title: "Trading Strategies".to_string(),
            description: "Strategy breakdowns, backtests, walk-forwards.".to_string(),
            read_class: "public".to_string(),
            write_class: "paid".to_string(),
            can_write: true,
            thread_count: 58,
            reply_count: 412,
            last_post_at: Some(ts_before(now_ms, DAY_MS)),
            last_thread_title: Some("Footprint POC mean-reversion, 3mo results".to_string()),
        },
        BoardUi {
            slug: "bug-reports".to_string(),
            title: "Bug Reports".to_string(),
            description: "Reproduce it, attach a screenshot, we triage daily.".to_string(),
            read_class: "public".to_string(),
            write_class: "user".to_string(),
            can_write: true,
            thread_count: 140,
            reply_count: 703,
            last_post_at: Some(ts_before(now_ms, 2.0 * HOUR_MS)),
            last_thread_title: Some("Alerts fire twice on reconnect".to_string()),
        },
    ]
}

/// The open thread's own header — content mirrors the concept's own state 3
/// (`forum_concept_v1.html`, "How do I set up alerts on Renko bars?",
/// opened by `j_novak`).
fn thread_detail(now_ms: f64) -> ForumThreadV2Detail {
    ForumThreadV2Detail {
        id: 5102,
        board_id: 2,
        title: "How do I set up alerts on Renko bars?".to_string(),
        author: "j_novak".to_string(),
        author_is_me: false,
        author_rank: Some("user".to_string()),
        body: "I've got a Renko chart set up (box size 25 ticks) but the alert dialog only lets me \
               pick price crosses on the underlying price axis, not brick closes. Is there a way to \
               fire an alert when a new Renko brick prints in a given direction?"
            .to_string(),
        created_at: ts_before(now_ms, 3.0 * DAY_MS + HOUR_MS),
        updated_at: ts_before(now_ms, 2.0 * DAY_MS + 5.0 * HOUR_MS),
        pinned: false,
        locked: false,
        hidden: false,
    }
}

/// Posts #2-#4 under [`thread_detail`] — #3 nests under #2 (`in reply to
/// @quiet_deska #2` in the concept) and is authored by the signed-in
/// fixture caller (`author_is_me: true`), #4 is the moderator's own close-out.
fn replies_tree(now_ms: f64) -> Vec<ForumReplyV2Ui> {
    vec![
        ForumReplyV2Ui {
            id: 2,
            parent_reply_id: None,
            depth: 0,
            author: "quiet_deska".to_string(),
            author_is_me: false,
            author_rank: Some("trusted".to_string()),
            body: "Not through the alert dialog directly — but you can drop an indicator that plots \
                   a signal on brick close and alert off the indicator crossing 0.5, works the same \
                   as any other series alert."
                .to_string(),
            created_at: ts_before(now_ms, 3.0 * DAY_MS - 8.0 * MINUTE_MS),
            hidden: false,
            children: vec![ForumReplyV2Ui {
                id: 3,
                parent_reply_id: Some(2),
                depth: 1,
                author: "j_novak".to_string(),
                author_is_me: true,
                author_rank: Some("user".to_string()),
                body: "That's clever, didn't think about routing it through an indicator plot. I'll \
                       wire an EMA-of-direction indicator and alert on the zero cross tonight."
                    .to_string(),
                created_at: ts_before(now_ms, 3.0 * DAY_MS - 20.0 * MINUTE_MS),
                hidden: false,
                children: Vec::new(),
            }],
        },
        ForumReplyV2Ui {
            id: 4,
            parent_reply_id: None,
            depth: 0,
            author: "Nemo Trading".to_string(),
            author_is_me: false,
            author_rank: Some("moderator".to_string()),
            body: "Native Renko-brick alert trigger is on the roadmap (0.6 line) — the indicator-plot \
                   route above is the right workaround until then."
                .to_string(),
            created_at: ts_before(now_ms, 2.0 * DAY_MS + 5.0 * HOUR_MS),
            hidden: false,
            children: Vec::new(),
        },
    ]
}

/// Shared `ForumV2State` baseline every fixture starts from: signed in,
/// `paid`-and-above (so the Messages pill shows regardless of which tab the
/// URL picked), with a nick set (so the composer never shows the
/// "choose a nick" gate).
fn forum_state_base(now_ms: f64) -> ForumV2State {
    ForumV2State {
        boards: boards(now_ms),
        boards_fetched_at: Some(now_ms as u64),
        caller_signed_in: true,
        caller_classes: vec!["public".to_string(), "user".to_string(), "paid".to_string()],
        caller_nick: Some("riverflow".to_string()),
        me_fetched_at: Some(now_ms as u64),
        ..ForumV2State::default()
    }
}

/// The DM conversation list — content mirrors
/// `designs/2026-09-24-messenger/messenger_concept_v1.html`'s own chat-list
/// rows (`zeng3ld`, `moderator`, `kvant_42`). Shared by both Messages
/// fixtures (locked/unlocked) — only which conversation is OPEN and its
/// message bodies differ between them.
fn conversations(now_ms: f64) -> Vec<DmConversationUi> {
    vec![
        DmConversationUi {
            id: 1,
            peer_label: "zeng3ld".to_string(),
            peer_fingerprint: Some("7F3A 91C2 0B6E 44D8".to_string()),
            last_message_at: Some(ts_before(now_ms, 10.0 * MINUTE_MS)),
            unread_count: 0,
            peer_rank: Some("paid".to_string()),
            last_message_preview: Some("новый ключ, переставил винду".to_string()),
            last_message_mine: false,
        },
        DmConversationUi {
            id: 2,
            peer_label: "moderator".to_string(),
            peer_fingerprint: Some("A11C 5D02 99FE 7731".to_string()),
            last_message_at: Some(ts_before(now_ms, 3.0 * DAY_MS)),
            unread_count: 2,
            peer_rank: Some("moderator".to_string()),
            last_message_preview: Some("закрепил тред с правилами".to_string()),
            last_message_mine: false,
        },
        DmConversationUi {
            id: 3,
            peer_label: "kvant_42".to_string(),
            peer_fingerprint: Some("3B90 6E14 C2AA 00F5".to_string()),
            last_message_at: Some(ts_before(now_ms, 12.0 * DAY_MS)),
            unread_count: 0,
            peer_rank: Some("paid".to_string()),
            last_message_preview: Some("You: спасибо, пресет работает".to_string()),
            last_message_mine: true,
        },
    ]
}

/// Plain-text messages for the unlocked `zeng3ld` conversation — two day
/// groups (yesterday/today), so the day-separator draws.
fn dm_messages_plain(now_ms: f64) -> Vec<DmMessageUi> {
    vec![
        DmMessageUi {
            id: 101,
            mine: false,
            body: DmMessageBody::Plain("BTC держит 64к?".to_string()),
            created_at: ts_before(now_ms, DAY_MS + 2.0 * HOUR_MS),
            read_at: Some(ts_before(now_ms, DAY_MS + 2.0 * HOUR_MS - MINUTE_MS)),
        },
        DmMessageUi {
            id: 102,
            mine: false,
            body: DmMessageBody::Plain("на 4h ордерблок BTCUSDT 64 150-64 420".to_string()),
            created_at: ts_before(now_ms, DAY_MS + HOUR_MS + 58.0 * MINUTE_MS),
            read_at: Some(ts_before(now_ms, DAY_MS + HOUR_MS + 57.0 * MINUTE_MS)),
        },
        DmMessageUi {
            id: 103,
            mine: true,
            body: DmMessageBody::Plain("если закроем 4h ниже 64 150 — шорт до FVG".to_string()),
            created_at: ts_before(now_ms, 5.0 * HOUR_MS),
            read_at: None,
        },
        DmMessageUi {
            id: 104,
            mine: false,
            body: DmMessageBody::Plain("новый ключ, переставил винду".to_string()),
            created_at: ts_before(now_ms, 10.0 * MINUTE_MS),
            read_at: None,
        },
    ]
}

/// Ciphertext messages for the vault-locked `zeng3ld` conversation —
/// content mirrors the concept's own state 5 (base64-looking nonce/
/// ciphertext pairs, never real key material).
fn dm_messages_ciphertext(now_ms: f64) -> Vec<DmMessageUi> {
    vec![
        DmMessageUi {
            id: 201,
            mine: true,
            body: DmMessageBody::Ciphertext {
                nonce_b64: "CLvD88fp6AP2Nwhu".to_string(),
                ciphertext_b64: "TVawlHb79DyEqa/ZbVm9mMlh".to_string(),
            },
            created_at: ts_before(now_ms, DAY_MS + 2.0 * HOUR_MS),
            read_at: Some(ts_before(now_ms, DAY_MS + 2.0 * HOUR_MS - MINUTE_MS)),
        },
        DmMessageUi {
            id: 202,
            mine: true,
            body: DmMessageBody::Ciphertext {
                nonce_b64: "JWe/KgYA/g93hMgTs".to_string(),
                ciphertext_b64: "ENU51FGtA=".to_string(),
            },
            created_at: ts_before(now_ms, DAY_MS + HOUR_MS + 58.0 * MINUTE_MS),
            read_at: None,
        },
        DmMessageUi {
            id: 203,
            mine: false,
            body: DmMessageBody::Ciphertext {
                nonce_b64: "q8Rk2Lw0Z1pV3mYt6HcD9sNe4xA7uBf".to_string(),
                ciphertext_b64: "OiGjK5vWqE=".to_string(),
            },
            created_at: ts_before(now_ms, DAY_MS + HOUR_MS + 47.0 * MINUTE_MS),
            read_at: None,
        },
    ]
}

/// Build the fixture `CommunityState` for `page`, at `now_ms` (see
/// [`ts_before`]'s own doc for why every timestamp is relative to it).
fn community_state_for(page: PageState, now_ms: f64) -> CommunityState {
    let mut forum2 = forum_state_base(now_ms);
    let mut dm = DmState::default();
    let tab = match page {
        PageState::ForumIndex => {
            forum2.view = ForumV2View::Boards;
            CommunityTab::Forum
        }
        PageState::ForumThread => {
            forum2.open_thread_board_slug = Some("general".to_string());
            forum2.view = ForumV2View::Thread {
                id: 5102,
                thread: Some(thread_detail(now_ms)),
                replies_tree: replies_tree(now_ms),
                reply_to: None,
            };
            CommunityTab::Forum
        }
        PageState::MessagesLocked => {
            dm = DmState {
                identity_fingerprint: Some("9C21 44FE AB03 7712".to_string()),
                conversations: conversations(now_ms),
                view: DmView::Conversation {
                    id: 1,
                    messages: dm_messages_ciphertext(now_ms),
                    draft_field_id: "dm_draft".to_string(),
                    has_older: false,
                },
                available: false,
                unavailable_reason: DmUnavailableReason::VaultLocked,
                last_error: Some("Unlock your vault to use messages".to_string()),
                ..DmState::default()
            };
            CommunityTab::Messages
        }
        PageState::MessagesChat => {
            dm = DmState {
                identity_fingerprint: Some("9C21 44FE AB03 7712".to_string()),
                conversations: conversations(now_ms),
                view: DmView::Conversation {
                    id: 1,
                    messages: dm_messages_plain(now_ms),
                    draft_field_id: "dm_draft".to_string(),
                    has_older: false,
                },
                available: true,
                unavailable_reason: DmUnavailableReason::None,
                ..DmState::default()
            };
            CommunityTab::Messages
        }
        PageState::MessagesGroup | PageState::MessagesChannel | PageState::MessagesMenu => {
            // The messenger data itself comes from `MessengerFixture`, not
            // `DmState` — this shell only carries what `messenger_overlay.
            // rs` reads straight off `CommunityState.dm` for UI-side facts
            // no `MessengerProvider` read covers (which room is open, the
            // draft field id; see that file's own top doc). `MessagesMenu`
            // opens the SAME "Trading desk" room as `MessagesGroup` — the
            // context menu it draws on top targets the CHAT-LIST ROW
            // ("ETH flows"), not the open conversation, so which room is
            // open does not matter for that state.
            let open_room_id = if page == PageState::MessagesChannel { 3 } else { 1 };
            dm = DmState {
                view: DmView::Conversation {
                    id: open_room_id,
                    messages: Vec::new(),
                    draft_field_id: "dm_draft".to_string(),
                    has_older: false,
                },
                available: true,
                unavailable_reason: DmUnavailableReason::None,
                ..DmState::default()
            };
            CommunityTab::Messages
        }
    };

    let messenger_reply_preview = matches!(page, PageState::MessagesGroup).then(|| {
        mlc_app_layout::ui::modal_settings::MessengerReplyPreview {
            sender_label: "moderator".to_string(),
            preview: "@admin закрепил правила — глянь, пожалуйста".to_string(),
        }
    });

    CommunityState { is_open: true, tab, forum2, dm, messenger_reply_preview, ..CommunityState::default() }
}

// ─── Render loop ────────────────────────────────────────────────────────────

struct CommunityLabState {
    theme_manager: ThemeManager,
    coordinator: InputCoordinator,
    community: CommunityState,
    /// `Some` for every [`PageState::uses_messenger_fixture`] state — see
    /// [`PageState::MessagesGroup`]'s own doc for why those states'
    /// `CommunityState.dm` carries no real room data at all.
    messenger_fixture: Option<crate::community_messenger_fixture::MessengerFixture>,
    page: PageState,
}

/// Boot this page in place of the primitive lab — see this module's own top
/// doc. Called from `crate::start` when [`wants_community_page`] is true.
pub fn start() -> Result<(), JsValue> {
    let now_ms = js_sys::Date::now();

    let mut theme_manager = ThemeManager::new();
    if let Some(preset) = query_param("theme") {
        theme_manager.set_preset(&preset);
    }

    let page = PageState::from_query();
    let community = community_state_for(page, now_ms);
    let messenger_fixture = page
        .uses_messenger_fixture()
        .then(|| crate::community_messenger_fixture::MessengerFixture::new(now_ms));
    let lab = CommunityLabState {
        theme_manager,
        coordinator: InputCoordinator::new(),
        community,
        messenger_fixture,
        page,
    };
    run_render_loop(lab)
}

fn run_render_loop(lab: CommunityLabState) -> Result<(), JsValue> {
    let state = Rc::new(RefCell::new(lab));

    let closure: Rc<RefCell<Option<Closure<dyn FnMut()>>>> = Rc::new(RefCell::new(None));
    let closure_clone = closure.clone();
    let state_clone = state.clone();

    *closure.borrow_mut() = Some(Closure::wrap(Box::new(move || {
        if let Err(e) = render_one_frame(&state_clone) {
            web_sys::console::error_1(&format!("[mlc-ui-lab/community] frame error: {:?}", e).into());
        }
        if let Some(win) = web_sys::window() {
            let _ = win.request_animation_frame(
                closure_clone.borrow().as_ref().unwrap().as_ref().unchecked_ref(),
            );
        }
    }) as Box<dyn FnMut()>));

    web_sys::window()
        .ok_or_else(|| JsValue::from_str("no window"))?
        .request_animation_frame(closure.borrow().as_ref().unwrap().as_ref().unchecked_ref())?;

    web_sys::console::log_1(&"[mlc-ui-lab/community] render loop running".into());
    Ok(())
}

fn render_one_frame(state: &Rc<RefCell<CommunityLabState>>) -> Result<(), JsValue> {
    use wasm_bindgen::JsCast;

    let win = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let doc = win.document().ok_or_else(|| JsValue::from_str("no document"))?;
    let canvas = doc
        .get_element_by_id("chart")
        .ok_or_else(|| JsValue::from_str("canvas not found"))?
        .dyn_into::<web_sys::HtmlCanvasElement>()?;

    let dpr = win.device_pixel_ratio();
    let css_w = canvas.client_width() as u32;
    let css_h = canvas.client_height() as u32;
    let phys_w = (css_w as f64 * dpr) as u32;
    let phys_h = (css_h as f64 * dpr) as u32;
    if canvas.width() != phys_w { canvas.set_width(phys_w); }
    if canvas.height() != phys_h { canvas.set_height(phys_h); }

    let raw_ctx = canvas
        .get_context("2d")?
        .ok_or_else(|| JsValue::from_str("no 2d context"))?
        .dyn_into::<web_sys::CanvasRenderingContext2d>()?;
    raw_ctx.set_transform(dpr, 0.0, 0.0, dpr, 0.0, 0.0)?;
    let mut ctx = LabRenderContext::new(raw_ctx, dpr, css_w, css_h);
    let screen_w = css_w as f64;
    let screen_h = css_h as f64;

    let mut lab_guard = state.borrow_mut();
    let lab: &mut CommunityLabState = &mut lab_guard;

    let bg = lab.theme_manager.current().chart.background.clone();
    ctx.set_fill_color(&bg);
    ctx.fill_rect(0.0, 0.0, screen_w.max(WINDOW_W), screen_h.max(WINDOW_H));

    let current_time_ms = js_sys::Date::now() as u64;
    let frame_theme = theme::frame_theme(&lab.theme_manager);
    let toolbar_theme = theme::toolbar_theme(&lab.theme_manager);

    lab.coordinator.begin_frame(InputState::new());
    let provider = lab.messenger_fixture.as_ref().map(|f| f as &dyn mlc_messenger_model::MessengerProvider);
    render_community_modal(
        &mut ctx,
        screen_w.max(WINDOW_W),
        screen_h.max(WINDOW_H),
        &lab.community,
        &frame_theme,
        &toolbar_theme,
        current_time_ms,
        &mut lab.coordinator,
        false,
        provider,
    );

    // Concept v1 screen "2 · меню чата" — a static context-menu overlay
    // (this crate hosts no click dispatch at all, this module's own top
    // doc) reproducing the row menu open on "ETH flows" plus its own
    // "Add to folder" submenu, both at once, matching the concept's own
    // screenshot.
    if lab.page == PageState::MessagesMenu {
        draw_messages_menu_overlay(&mut ctx, &toolbar_theme, &mut lab.coordinator);
    }

    let _ = lab.coordinator.end_frame();

    Ok(())
}

/// Build and draw the chat-row context menu + its own open "Add to folder"
/// submenu for [`PageState::MessagesMenu`] — see [`render_one_frame`]'s own
/// call site doc.
fn draw_messages_menu_overlay(
    ctx: &mut LabRenderContext,
    toolbar_theme: &mlc_app_layout::ui::toolbar_theme::ToolbarTheme,
    coordinator: &mut InputCoordinator,
) {
    use mlc_app_layout::layout::modals::context_menu::render_context_menu;
    use mlc_app_layout::ui::context_menu::{build_dm_chat_row_context_menu, ContextMenuState, ContextMenuTarget};
    use mlc_app_layout::ui::dropdown::DropdownTheme;
    use mlc_messenger_model::{Capabilities, Folder, FolderExclude, FolderIcon, FolderInclude, RoomKind, TagName};

    let caps = Capabilities {
        groups: true,
        channels: true,
        folders: true,
        reactions: true,
        edits: true,
        replies: true,
        typing: true,
        receipts: true,
        forward: true,
        stickers: true,
        chart_card_send: true,
    };
    let folders = vec![
        Folder {
            id: Some(TagName::from("u.trading-desk")),
            name: "Trading desk".to_string(),
            icon: FolderIcon::Candles,
            include: FolderInclude::default(),
            exclude: FolderExclude::default(),
            order: 1.0,
        },
        Folder {
            id: Some(TagName::from("u.research")),
            name: "Research".to_string(),
            icon: FolderIcon::Folder,
            include: FolderInclude::default(),
            exclude: FolderExclude::default(),
            order: 2.0,
        },
    ];
    let room_tags = vec![TagName::from("u.trading-desk")];
    let mut items = build_dm_chat_row_context_menu(caps, true, RoomKind::Channel, false, false, &folders, &room_tags);
    // The concept's own screenshot shows the row menu AND its "Add to
    // folder" submenu open at once — flip that one item's own
    // `submenu_open` for this static page (a later click-dispatch piece
    // sets this from a real hover, not a hardcoded page fixture).
    if let Some(add_folder) = items.iter_mut().find(|i| i.action == "dm_chat_row:add_folder") {
        add_folder.submenu_open = true;
    }

    let mut menu_state = ContextMenuState::new();
    menu_state.open(118.0, 190.0, ContextMenuTarget::DmChatRow("3".to_string()), items);
    let dropdown_theme = DropdownTheme::from_toolbar(toolbar_theme);
    render_context_menu(ctx, &menu_state, &dropdown_theme, None, coordinator);
}
