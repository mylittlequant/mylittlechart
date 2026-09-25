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
//! polling — the fixture is picked once at boot from the URL, not by
//! clicking anything on screen.
//!
//! Every field this module sets on `CommunityState`/`ForumV2State`/
//! `DmState` is public — nothing this task needed was blocked from the
//! outside. One finding worth flagging for whoever compares a screenshot
//! against the concept mockups: [`DmConversationUi::last_message_preview`]/
//! `last_message_mine` are real public fields this fixture populates, but
//! the chat-list row's own preview line never reads them —
//! `dm_messenger_provider.rs`'s `reads::rooms` hardcodes
//! `RoomSummary::last_event: None` regardless (that type's own doc already
//! names the gap: "today's `routes::dm::list_conversations` caller
//! populates no such field"). `DmConversationUi::peer_rank`/
//! `peer_fingerprint` are NOT affected by that gap — the open
//! conversation's own header reads those two straight off
//! `state.dm.conversations`, not through the provider, and do show up.
//!
//! Seven states are wired: `forum-index`, `forum-board`, `forum-thread`,
//! `forum-thread-signed-out`, `forum-search` (the five
//! `designs/2026-09-23-forum/forum_concept_v1.html` screens, one fixture
//! each), plus `messages-locked`/`messages-chat` — `forum-index` is the
//! default for an unrecognised or missing `state=`.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;

use mlc_app_layout::layout::modals::community::render_community_modal;
use mlc_app_layout::ui::modal_settings::{
    BoardUi, CommunityState, CommunityTab, DmConversationUi, DmMessageBody, DmMessageUi, DmState,
    DmUnavailableReason, DmView, ForumReplyV2Ui, ForumSearchHitUi, ForumThreadV2Detail,
    ForumThreadV2Ui, ForumV2State, ForumV2View,
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
/// this module's own top doc for the full name list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PageState {
    ForumIndex,
    ForumBoard,
    ForumThread,
    ForumThreadSignedOut,
    ForumSearch,
    MessagesLocked,
    MessagesChat,
}

impl PageState {
    fn from_query() -> Self {
        match query_param("state").as_deref() {
            Some("forum-board") => Self::ForumBoard,
            Some("forum-thread") => Self::ForumThread,
            Some("forum-thread-signed-out") => Self::ForumThreadSignedOut,
            Some("forum-search") => Self::ForumSearch,
            Some("messages-locked") => Self::MessagesLocked,
            Some("messages-chat") => Self::MessagesChat,
            _ => Self::ForumIndex,
        }
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
            // Concept's own Posts column reads 58 = thread_count + replies
            // (`BoardUi::reply_count`'s own doc: replies ONLY, excluding
            // the OP) — 58 - 12 = 46.
            reply_count: 46,
            last_post_at: Some(ts_before(now_ms, 3.0 * HOUR_MS)),
            last_thread_title: Some("0.5.607 — native wave notes".to_string()),
            last_post_author: Some("Nemo Trading".to_string()),
        },
        BoardUi {
            slug: "general".to_string(),
            title: "General Discussion".to_string(),
            description: "Anything chart-related that doesn't fit elsewhere.".to_string(),
            read_class: "public".to_string(),
            write_class: "user".to_string(),
            can_write: true,
            thread_count: 214,
            // Concept Posts = 1,842; 1,842 - 214 = 1,628 replies.
            reply_count: 1628,
            last_post_at: Some(ts_before(now_ms, 4.0 * HOUR_MS)),
            last_thread_title: Some("How do I set up alerts on Renko bars?".to_string()),
            last_post_author: Some("j_novak".to_string()),
        },
        BoardUi {
            slug: "feature-requests".to_string(),
            title: "Feature Requests".to_string(),
            description: "Vote and discuss what ships next.".to_string(),
            read_class: "public".to_string(),
            write_class: "user".to_string(),
            can_write: true,
            thread_count: 96,
            // Concept Posts = 640; 640 - 96 = 544 replies.
            reply_count: 544,
            last_post_at: Some(ts_before(now_ms, 41.0 * MINUTE_MS)),
            last_thread_title: Some("Multi-timeframe watchlist column".to_string()),
            last_post_author: Some("qbot_maria".to_string()),
        },
        BoardUi {
            slug: "strategies".to_string(),
            title: "Trading Strategies".to_string(),
            description: "Strategy breakdowns, backtests, walk-forwards.".to_string(),
            read_class: "public".to_string(),
            write_class: "paid".to_string(),
            // `false` regardless of `forum_state_base`'s own `caller_classes`
            // (which does include "paid") — concept state 1 shows this
            // board's own "write: paid" tag, i.e. illustrates a caller who
            // canNOT write here; `can_write` is fixture-literal data, not
            // re-derived from `caller_classes` anywhere in this file.
            can_write: false,
            thread_count: 58,
            // Concept Posts = 412; 412 - 58 = 354 replies.
            reply_count: 354,
            last_post_at: Some(ts_before(now_ms, DAY_MS)),
            last_thread_title: Some("Footprint POC mean-reversion, 3mo results".to_string()),
            last_post_author: Some("desk_yuri".to_string()),
        },
        BoardUi {
            slug: "bug-reports".to_string(),
            title: "Bug Reports".to_string(),
            description: "Reproduce it, attach a screenshot, we triage daily.".to_string(),
            read_class: "public".to_string(),
            write_class: "user".to_string(),
            can_write: true,
            thread_count: 140,
            // Concept Posts = 703; 703 - 140 = 563 replies.
            reply_count: 563,
            last_post_at: Some(ts_before(now_ms, 2.0 * HOUR_MS)),
            last_thread_title: Some("Alerts fire twice on reconnect".to_string()),
            last_post_author: Some("night_owl".to_string()),
        },
    ]
}

/// "General Discussion" board's own thread list — content mirrors
/// `designs/2026-09-23-forum/forum_concept_v1.html`'s own state 2 (pinned +
/// locked welcome thread first, five ordinary threads, a "Load more" row —
/// `next_before: Some(_)` in [`community_state_for`] exercises that row).
fn board_threads(now_ms: f64) -> Vec<ForumThreadV2Ui> {
    vec![
        ForumThreadV2Ui {
            id: 5001,
            title: "Welcome — forum rules & how boards work".to_string(),
            author: "Nemo Trading".to_string(),
            author_is_me: false,
            author_rank: Some("moderator".to_string()),
            created_at: ts_before(now_ms, 3.0 * DAY_MS),
            updated_at: ts_before(now_ms, 3.0 * DAY_MS),
            last_reply_at: Some(ts_before(now_ms, 3.0 * DAY_MS)),
            reply_count: 0,
            pinned: true,
            locked: true,
        },
        ForumThreadV2Ui {
            id: 5002,
            title: "Renko vs range bars for scalping?".to_string(),
            author: "trader_ivan".to_string(),
            author_is_me: false,
            author_rank: Some("user".to_string()),
            created_at: ts_before(now_ms, DAY_MS),
            updated_at: ts_before(now_ms, 12.0 * MINUTE_MS),
            last_reply_at: Some(ts_before(now_ms, 12.0 * MINUTE_MS)),
            reply_count: 14,
            pinned: false,
            locked: false,
        },
        ForumThreadV2Ui {
            id: 5003,
            title: "Anyone else seeing gaps on the BTC perp chart after reconnect?".to_string(),
            author: "flow_max".to_string(),
            author_is_me: false,
            author_rank: Some("user".to_string()),
            created_at: ts_before(now_ms, DAY_MS),
            updated_at: ts_before(now_ms, 38.0 * MINUTE_MS),
            last_reply_at: Some(ts_before(now_ms, 38.0 * MINUTE_MS)),
            reply_count: 6,
            pinned: false,
            locked: false,
        },
        ForumThreadV2Ui {
            id: 5004,
            title: "Best hotkeys for quick fib placement".to_string(),
            author: "sasha_trd".to_string(),
            author_is_me: false,
            author_rank: Some("user".to_string()),
            created_at: ts_before(now_ms, DAY_MS),
            updated_at: ts_before(now_ms, HOUR_MS),
            last_reply_at: Some(ts_before(now_ms, HOUR_MS)),
            reply_count: 3,
            pinned: false,
            locked: false,
        },
        ForumThreadV2Ui {
            id: 5005,
            title: "Dark theme accent color feels too blue for me".to_string(),
            author: "mira_k".to_string(),
            author_is_me: false,
            author_rank: Some("user".to_string()),
            created_at: ts_before(now_ms, DAY_MS),
            updated_at: ts_before(now_ms, 2.0 * HOUR_MS),
            last_reply_at: Some(ts_before(now_ms, 2.0 * HOUR_MS)),
            reply_count: 9,
            pinned: false,
            locked: false,
        },
        ForumThreadV2Ui {
            id: 5102,
            title: "How do I set up alerts on Renko bars?".to_string(),
            author: "j_novak".to_string(),
            author_is_me: false,
            author_rank: Some("user".to_string()),
            created_at: ts_before(now_ms, 3.0 * DAY_MS + HOUR_MS),
            updated_at: ts_before(now_ms, 4.0 * HOUR_MS),
            last_reply_at: Some(ts_before(now_ms, 4.0 * HOUR_MS)),
            reply_count: 4,
            pinned: false,
            locked: false,
        },
    ]
}

/// Compact quick-search dropdown hits — content mirrors the concept's own
/// state 5 (query `"renko alert"`, two thread hits and one reply hit).
fn search_hits(now_ms: f64) -> Vec<ForumSearchHitUi> {
    vec![
        ForumSearchHitUi {
            kind: "thread".to_string(),
            board_id: 2,
            thread_id: 5102,
            reply_id: None,
            title: Some("How do I set up alerts on Renko bars?".to_string()),
            snippet: "...fire an alert when a new Renko brick prints in a given direction...".to_string(),
            author: "j_novak".to_string(),
            created_at: ts_before(now_ms, 3.0 * DAY_MS),
            board_title: Some("General Discussion".to_string()),
        },
        ForumSearchHitUi {
            kind: "reply".to_string(),
            board_id: 2,
            thread_id: 482,
            reply_id: Some(1024),
            title: None,
            snippet: "...route it through an indicator plot and alert on the zero cross...".to_string(),
            author: "j_novak".to_string(),
            created_at: ts_before(now_ms, 3.0 * DAY_MS),
            board_title: Some("General Discussion".to_string()),
        },
        ForumSearchHitUi {
            kind: "thread".to_string(),
            board_id: 2,
            thread_id: 5002,
            reply_id: None,
            title: Some("Renko vs range bars for scalping?".to_string()),
            snippet: "...box size and tick sensitivity for renko bricks on fast scalps...".to_string(),
            author: "trader_ivan".to_string(),
            created_at: ts_before(now_ms, 12.0 * MINUTE_MS),
            board_title: Some("General Discussion".to_string()),
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
        PageState::ForumBoard => {
            forum2.view = ForumV2View::BoardThreads {
                slug: "general".to_string(),
                threads: board_threads(now_ms),
                // `Some(_)` — the concept's own state 2 shows a "Load more"
                // row under the thread list.
                next_before: Some("cursor-5102".to_string()),
            };
            CommunityTab::Forum
        }
        PageState::ForumThread => {
            forum2.open_thread_board_slug = Some("general".to_string());
            forum2.view = ForumV2View::Thread {
                id: 5102,
                thread: Some(thread_detail(now_ms)),
                replies_tree: replies_tree(now_ms),
                // Concept state 3 shows the composer already targeting a
                // reply ("Replying to #2 · quiet_deska ×").
                reply_to: Some(2),
            };
            CommunityTab::Forum
        }
        PageState::ForumThreadSignedOut => {
            // Concept state 4: the same thread, read-only — no composer, no
            // per-post "Reply" link, one quiet sign-in line at the bottom
            // ([`mlc_app_layout::layout::modals::forum_v2_overlay::
            // thread_composer_visible`] gates all three off `caller_signed_in`
            // alone).
            forum2.caller_signed_in = false;
            forum2.caller_nick = None;
            forum2.caller_classes = vec!["public".to_string()];
            forum2.open_thread_board_slug = Some("general".to_string());
            forum2.view = ForumV2View::Thread {
                id: 5102,
                thread: Some(thread_detail(now_ms)),
                replies_tree: replies_tree(now_ms),
                reply_to: None,
            };
            CommunityTab::Forum
        }
        PageState::ForumSearch => {
            // Concept state 5: the board index still showing (dimmed, see
            // `forum_v2_overlay::draw`'s own `dim` flag) behind the compact
            // quick-search dropdown.
            forum2.view = ForumV2View::Boards;
            forum2.quick_search_open = true;
            forum2.quick_search_query = Some("renko alert".to_string());
            forum2.quick_search_results = search_hits(now_ms);
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
    };

    CommunityState { is_open: true, tab, forum2, dm, ..CommunityState::default() }
}

// ─── Render loop ────────────────────────────────────────────────────────────

struct CommunityLabState {
    theme_manager: ThemeManager,
    coordinator: InputCoordinator,
    community: CommunityState,
}

/// Boot this page in place of the primitive lab — see this module's own top
/// doc. Called from `crate::start` when [`wants_community_page`] is true.
pub fn start() -> Result<(), JsValue> {
    let now_ms = js_sys::Date::now();

    let mut theme_manager = ThemeManager::new();
    if let Some(preset) = query_param("theme") {
        theme_manager.set_preset(&preset);
    }

    // `&lang=<code>` — lab-only override for parity screenshots against an
    // English-language concept mockup; every other page in this crate reads
    // whatever `uzor`'s global lang index already resolved to (usually the
    // browser's own locale), same as the production app.
    if let Some(code) = query_param("lang") {
        if let Some(lang) = mlc_core::i18n::Language::from_code(&code) {
            mlc_core::i18n::set_language(lang);
        }
    }

    let page = PageState::from_query();
    let community = community_state_for(page, now_ms);
    let mut coordinator = InputCoordinator::new();
    if page == PageState::ForumSearch {
        // The inline search box's own text lives in the coordinator's
        // `TextFieldStore`, not `ForumV2State` — `cloud_overlay::
        // FORUM_SEARCH_FIELD`'s own widget id (`"forum_search"`), `pub(crate)`
        // in `mlc-app-layout` and so unreachable from here by name; the
        // literal is stable (`forum_v2_overlay.rs`'s own crumb-bar field
        // uses it unchanged since the community-modal-drill-down arc).
        // `set_text` alone is a no-op on a field the store has never seen —
        // `register` first (same `mlc-app` shell-driver step this page
        // otherwise skips entirely, having no bootstrap of its own).
        let search_field = uzor::WidgetId::from("forum_search");
        coordinator.text_fields_mut().register(search_field.clone(), uzor::input::TextFieldConfig::text());
        coordinator.text_fields_mut().set_text(&search_field, "renko alert");
    }
    let lab = CommunityLabState { theme_manager, coordinator, community };
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
    );
    let _ = lab.coordinator.end_frame();

    Ok(())
}
