//! Cloud session driver (cloud builds only).
//!
//! Owns the `mlc-cloud` transport and drives the login overlay state machine
//! ([`CloudLoginState`], rendered by `mlc-app-layout`). The split is
//! deliberate: `mlc-app` / `mlc-app-layout` hold pure UI state and one-shot
//! flags; every effect (fetch, localStorage, window.open) happens here, in
//! the shell, one `drive()` call per RAF frame.
//!
//! Login flow (device code):
//!   open overlay → POST /api/auth/device-code → show code → user confirms
//!   on chart.example/dashboard (cookie session) → we poll
//!   GET /api/auth/poll → Bearer token → localStorage + profile link.
//!
//! Attestation headers come from `window.MLC_BUILD` (injected into
//! app-boot.js by package-app.sh at signing time); without them the server
//! answers 403 on the attested endpoints once BUILD_ATTEST_ENFORCE=true.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::JsCast;
use wasm_bindgen_futures::{spawn_local, JsFuture};

use mlc_app::ChartApp;
use mlc_app_layout::ui::modal_settings::{
    dm_has_older, merge_dm_messages, BoardUi, CloudForumReply, CloudForumThread, CloudForumThreadSummary,
    CloudLoginPhase, CloudNewsPost, CloudOverlayData, CloudOverlayKind, CloudReleaseEntry, CommunityTab,
    DmConversationUi, DmLookupHitUi, DmMessageBody, DmMessagePage, DmMessageUi, DmRequest as UiDmRequest,
    DmUnavailableReason, DmView, ForumListState,
    ForumReplyV2Ui, ForumRequest, ForumSearchHitUi, ForumSubView, ForumThreadV2Detail,
    ForumThreadV2Ui, ForumV2Request, ForumV2View, RedeemStatus, UserSettingsTab, DM_MESSAGE_PAGE_LIMIT,
};
use mlc_cloud::{
    dm_error_message, forum_error_message, forum_v2_error_message, redeem_error_message,
    BuildAttestation, BlogPost, CloudConfig, DeviceAuthPoll, EntitlementsResponse, FetchTransport,
    ForumBoard, ForumMeResponse, ForumReplyNode, ForumSearchHit, ForumThreadResponse,
    ForumThreadSummary, ForumThreadV2Response, ForumThreadV2Summary, MeResponse, ReleaseEntry,
    StoredToken, VersionManifest,
};

mod checkout;

/// Notice shown once the vault is unlocked but the account is not paid.
const DM_UNPAID_MESSAGE: &str = "Messages need a paid account";

const TOKEN_KEY: &str = "mlc.cloud.token.v1";
const DEVICE_KEY: &str = "mlc.cloud.device_id.v1";
/// Last APPLIED entitlements answer this browser persisted, keyed by user —
/// read back on the first entitlements answer of a NEW session so a tier
/// change that landed while the page was closed still surfaces once (see
/// `drive()`'s entitlements block).
const TIER_KEY: &str = "mlc.cloud.tier.v1";
const POLL_INTERVAL_MS: f64 = 2500.0;
/// Backoff before retrying a failed entitlements fetch — short enough that a
/// transient network blip self-heals within a session, long enough that a
/// server outage does not turn into a fetch-every-frame loop.
const ENTITLEMENTS_RETRY_MS: f64 = 30_000.0;
/// Floor on the re-ask interval even if the server's own `expires_at` comes
/// back unreasonably soon (or in the past) — the endpoint answers the same
/// caller identity on every call, so there is nothing to gain from asking
/// more often than this regardless of what the server said.
const ENTITLEMENTS_MIN_REASK_MS: f64 = 60_000.0;

/// Completed async step, written by spawn_local futures and applied to the
/// chart state by the next `drive()` call (mailbox pattern).
enum FlowResult {
    Code(Result<String, String>),
    Poll(Result<DeviceAuthPoll, String>),
    Logout,
    /// This visitor's own platform id, their latest manifest, and the full
    /// release catalogue — fetched together so the Download overlay never
    /// shows the collapsed view without already holding what the expand
    /// button needs.
    Updates(Result<(&'static str, VersionManifest, Vec<ReleaseEntry>), String>),
    News(Result<Vec<BlogPost>, String>),
    /// A gift/trial code came back accepted (the new paid-until instant) or
    /// refused (the sentence to print).
    Redeem(Result<String, String>),
    /// A nickname submission came back accepted (the new effective label) or
    /// refused (the sentence to print).
    Nick(Result<String, String>),

    // ── Forum tab (paid-only) ───────────────────────────────────────────
    /// A thread-list page — the passive tab-open fetch (`append: false`) or
    /// `ForumRequest::LoadMoreThreads` (`append: true`).
    ForumThreads {
        append: bool,
        result: Result<(Vec<ForumThreadSummary>, Option<String>), String>,
    },
    /// `ForumRequest::LoadThread` — replaces the whole Forum sub-view with
    /// this thread (or the failure to load it).
    ForumThreadOpened(Result<ForumThreadResponse, String>),
    /// `ForumRequest::CreateThread` — success lands on the new thread;
    /// failure stays on the compose form with the sentence in
    /// `ForumSubView::Compose::error`.
    ForumThreadCreated(Result<ForumThreadResponse, String>),
    /// `ForumRequest::Reply` or `DeleteReply` — success refreshes the
    /// thread already open; failure sets `ForumSubView::Thread::
    /// reply_error` and leaves the thread as-is.
    ForumThreadRefreshed(Result<ForumThreadResponse, String>),
    /// `ForumRequest::DeleteThread` — success clears the thread and
    /// refreshes the list (back to `ForumSubView::List`); failure leaves
    /// the thread view up with the sentence in `reply_error`.
    ForumThreadDeleted(Result<(Vec<ForumThreadSummary>, Option<String>), String>),

    // ── Forum v2 tab (public boards, `CommunityTab::Forum`) ─────────────
    /// `ForumV2Request::LoadBoards` (passive first fetch, a background
    /// due-refresh, or the explicit "back to boards" reload).
    Forum2Boards(Result<Vec<ForumBoard>, String>),
    /// `ForumV2Request::LoadBoard` — `append: false` is the first page (or a
    /// background due-refresh); `append: true` is "Load more".
    Forum2BoardThreads {
        append: bool,
        result: Result<(Vec<ForumThreadV2Summary>, Option<String>), String>,
    },
    /// `ForumV2Request::OpenThread`, a background thread refresh, or the
    /// follow-up read after `CreateThread`/`Reply`/`DeleteReply`/`Moderate`
    /// succeeds — applied identically to `ForumV2View::Thread`, same
    /// "infer from the current view" idiom v1's `ForumThreadOpened` uses.
    Forum2Thread(Result<ForumThreadV2Response, String>),
    /// The follow-up read after `CreateThread`/`Reply`/`DeleteReply`/
    /// `Moderate` succeeds — unlike [`Self::Forum2Thread`], applies
    /// unconditionally (forces the view to the target thread, invalidates
    /// the board's thread list, clears the compose/reply fields) rather
    /// than only when the caller is still looking at that thread. Same
    /// three-way split v1 draws between `ForumThreadOpened` and
    /// `ForumThreadCreated`/`ForumThreadRefreshed`.
    Forum2ThreadAfterWrite(Result<ForumThreadV2Response, String>),
    /// `ForumV2Request::Search` — `q` rides alongside so the applied
    /// `ForumV2View::Search` always shows the query that was actually
    /// asked, never a guess reconstructed from whatever view happens to be
    /// showing when the answer lands.
    Forum2Search { q: String, result: Result<Vec<ForumSearchHit>, String> },
    /// `GET /api/forum/me` — passive, once per tab-open (and again after a
    /// sign-in/sign-out edge invalidates it) — see
    /// `ForumV2State::me_fetched_at`'s own doc.
    Forum2Me(Result<ForumMeResponse, String>),

    // ── Messages tab (paid, end-to-end encrypted) ───────────────────────
    /// The auto-publish attempt (`PUT /api/dm/identity`) — success carries
    /// the confirmed fingerprint. Best-effort: a failure just logs and
    /// leaves the next frame's check to retry, same tolerance the native
    /// shell's own auto-publish gives itself.
    DmIdentityPublished(Result<String, String>),
    /// `DmRequest::LoadConversations` (passive first fetch or due-refresh).
    DmConversations(Result<Vec<mlc_cloud::DmConversation>, String>),
    /// `DmRequest::Open` — resolves the peer's identity, decodes their
    /// public key, then opens (or resumes) the conversation, all in one
    /// round trip (no separate in-flight chain needed — unlike the native
    /// shell's two-command channel, this is one `spawn_local` body).
    DmConversationOpened(Result<(mlc_cloud::DmConversation, [u8; 32]), String>),
    /// Backfilling a LIST-sourced conversation's peer key/fingerprint
    /// (`GetIdentity` for a conversation reached via `ListConversations`,
    /// which does not carry either).
    DmBackfillPeerKey {
        conversation_id: i64,
        result: Result<mlc_cloud::DmIdentityResponse, String>,
    },
    /// `DmRequest::LoadMessages` — `page` carries the cursor direction the
    /// request was sent with, same split `Forum2BoardThreads` draws for its
    /// own pagination.
    DmMessages {
        conversation_id: i64,
        page: DmMessagePage,
        result: Result<mlc_cloud::DmMessagesResponse, String>,
    },
    /// `DmRequest::Send` — `plaintext` rides along so the settled answer can
    /// become an optimistic bubble without a second round trip.
    DmSent {
        plaintext: String,
        result: Result<(i64, String), String>,
    },
    /// `DmRequest::MarkRead`.
    DmRead {
        conversation_id: i64,
        result: Result<(), String>,
    },
    /// The recipient-search dropdown's debounced `GET /api/dm/lookup?q=`
    /// (community-modal-and-forum-ladder arc, piece 2 item b) — `q` rides
    /// alongside so the applied `DmState::lookup_query` always shows the
    /// query that was actually asked, never a guess reconstructed from
    /// whatever the field holds when the answer lands.
    DmLookup {
        q: String,
        result: Result<Vec<mlc_cloud::DmLookupHit>, String>,
    },
}

enum FriendApply {
    List(mlc_cloud::FriendRoster),
    Invite(String),
    Note(String),
}

fn friend_query_token() -> Option<String> {
    let window = web_sys::window()?;
    let search = window.location().search().ok()?;
    for part in search.trim_start_matches('?').split('&') {
        if let Some(token) = part.strip_prefix("friend=") {
            if !token.is_empty() {
                return Some(token.to_string());
            }
        }
    }
    None
}

struct Inner {
    transport: FetchTransport,
    token: Option<StoredToken>,
    /// An async step is in flight — don't stack requests.
    in_flight: bool,
    next_poll_ms: f64,
    /// Previous frame's overlay open flag (edge detection).
    was_open: bool,
    /// Finished async results, oldest first. A QUEUE, not a single slot: two
    /// flows finishing inside one frame (e.g. the v1 forum list and the v2
    /// boards fetch, both fired the moment the Community modal opens) used
    /// to overwrite each other, and the lost result's in-flight flag was
    /// never cleared — the forum stayed empty and the Messages tab never
    /// appeared until a reload (0.5.615 prod repro, 2026-09-24).
    results: std::collections::VecDeque<FlowResult>,
    /// Set by any authed call's `spawn_local` body the instant it sees a
    /// 401 (token invalid/expired/revoked) — `auth_me()`, the entitlements
    /// fetch, share create, forum requests, nick, and redeem all write this
    /// (via [`mark_unauthorized`]) before they turn the error into a
    /// display string, since none of them keep the typed `CloudError` past
    /// that point otherwise. Drained first thing in `drive()`, which drops
    /// to signed-out the same frame the answer lands — "signed in" must
    /// mean "the server still agrees", not just "a token is stored
    /// locally" (Arc B1, 2026-09-06). Never set for a 403 (entitlement
    /// refusal) or a network/transport failure — see
    /// [`mark_unauthorized`]'s own doc.
    unauthorized: bool,

    // ── Account label refresh (`GET /api/auth/me`) ─────────────────────
    // Fired once per token activation, never polled — a nick set from
    // another device or by the API is otherwise invisible until re-login
    // (owner, 2026-09-05). Own mailbox/in-flight pair, same reasoning as
    // `entitlements_result`/`share_result`: none of this driver's
    // concurrent fetches may block or overwrite another.
    /// A fetch is already in flight.
    me_in_flight: bool,
    /// Mailbox for the `auth_me()` fetch.
    me_result: Option<Result<MeResponse, String>>,
    /// Whether the boot-time check (fire if a stored token is already
    /// present) has run — true after the very first `drive()` call
    /// regardless of outcome, so it can never re-fire on a later frame.
    /// The post-login fetch is a separate, explicit call and does not use
    /// this flag.
    me_boot_checked: bool,

    // ── Entitlements ────────────────────────────────────────────────────
    // Runs on its own schedule, independent of the login/overlay phase
    // machine above: `multi_profile` gates the always-visible General tab,
    // not just the login overlay, so this keeps refreshing in the
    // background whether or not the overlay is even open.
    /// A fetch is already in flight — separate from `in_flight` above so an
    /// open login overlay never blocks (or gets blocked by) this refresh.
    entitlements_in_flight: bool,
    /// Wall-clock ms (same clock as `now_ms`) after which the next fetch is
    /// due. `0.0` (the `init()` default) means "ask on the first frame".
    next_entitlements_ms: f64,
    /// Mailbox for the entitlements fetch — separate from `result` above so
    /// a concurrent login/logout/overlay fetch can never overwrite it.
    entitlements_result: Option<Result<EntitlementsResponse, String>>,
    /// Last APPLIED entitlements answer `(paid, paid_until)` — compared
    /// against each new answer to detect a paid⇄free transition
    /// (system-event-bus arc, 2026-09-05). `None` means no answer has
    /// landed yet this session; the first real answer sets this without
    /// firing an event (a fresh boot reading "free" is not a demotion).
    prev_entitlements: Option<(bool, Option<String>)>,

    // ── Chart sharing ───────────────────────────────────────────────────
    /// Mailbox for the last `share_screenshot`/`share_live` upload's
    /// outcome — separate from `result` above for the same reason
    /// `entitlements_result` is: a concurrent login/logout/overlay fetch
    /// must not overwrite it. `(kind, Ok(absolute_url) | Err(message))`
    /// where `kind` is `"live"` or `"screenshot"`. Drained by `drive()`,
    /// which is the only place `&mut ChartApp` is reachable — the upload's
    /// own `spawn_local` body cannot log a system event directly.
    share_result: Option<(&'static str, Result<String, String>)>,
    /// Name for the chart a clone is about to create: `title — owner (clone)`.
    clone_name: Option<String>,
    share_rights_at_ms: f64,
    share_rights_busy: bool,
    /// `(token, clone_allowed)` from a rights poll. Applied on the next drive.
    /// Never carries a preset.
    share_rights: Option<(String, bool)>,
    share_admin_rows: Option<Result<Vec<mlc_cloud::ShareListEntry>, String>>,
    share_admin_busy: bool,
    share_ops_busy: bool,
    share_ops_in: Option<(String, mlc_cloud::ShareOpsResponse)>,
    share_ops_push_ok: Option<bool>,
    friend_busy: bool,
    friend_accept_tried: bool,
    friend_result: Option<FriendApply>,
    friend_lookup_q: String,
    friend_lookup_at_ms: f64,
    friend_lookup_busy: bool,
    friend_hits_in: Option<Vec<String>>,
    edge_socket: Option<web_sys::WebSocket>,
    edge_token: String,
    edge_in: Vec<String>,
    /// Ticket from `stream-open`. The socket's first frame is `hello` with
    /// this value. Empty until that GET returns.
    edge_ticket: String,
    edge_ticket_for: String,
    edge_ticket_busy: bool,
    edge_ticket_next_ms: f64,
    edge_hello_sent: bool,
    /// Live just turned off. Off-frames go out before the socket closes.
    edge_stop_sent: bool,
    /// `(chart_id, token)` from the last successful live publish.
    share_bound: Option<(String, String)>,
    share_lookup_q: String,
    share_lookup_at_ms: f64,
    share_lookup_busy: bool,
    share_lookup_hits: Option<Vec<(String, String)>>,
    share_settings_note: Option<String>,
    /// Generation for the in-flight screenshot-share card. A late blob from
    /// the previous click is dropped when this moves.
    preview_epoch: u64,
    /// Token of the screenshot share that should receive the card, tagged
    /// with the epoch that created it.
    preview_token: Option<(u64, String)>,
    /// Capped chart PNG waiting for that token.
    preview_card: Option<(u64, web_sys::Blob)>,

    // ── Chart-share clone (shared-viewer arc, 2026-09-06) ────────────────
    // `ChartApp::request_clone_share` is a one-shot mailbox a "Clone to my
    // charts" click arms; the round trip outlives one frame, so this needs
    // its own in-flight gate and mailbox, same shape as `share_result`
    // above but distinct from it (clone is its own endpoint, not a share
    // create).
    /// A `/api/share/{token}/clone` request is in flight.
    clone_in_flight: bool,
    /// Mailbox for the last clone outcome: the cloned preset's raw JSON plus
    /// the server's stripped-indicator list, or the failure sentence.
    /// Applied in `drive()`, the only place `&mut ChartApp` — and therefore
    /// `ChartApp::apply_cloned_preset` — is reachable.
    clone_result: Option<Result<(serde_json::Value, Vec<mlc_cloud::StrippedIndicator>), String>>,

    // ── Forum tab (paid-only) ───────────────────────────────────────────
    /// A forum request is in flight — separate from `in_flight` above so
    /// the Forum tab never blocks (or gets blocked by) the login/News/
    /// Download flow that field already gates.
    forum_in_flight: bool,

    // ── Forum v2 tab (public boards) ────────────────────────────────────
    /// A forum-v2 request is in flight — separate from `forum_in_flight`
    /// above (v1 and v2 forums are unrelated request streams) so neither
    /// tab's fetch can block or be blocked by the other's.
    forum2_in_flight: bool,

    // ── Direct messages (paid, end-to-end encrypted) ────────────────────
    // Owner ruling 2026-09-09: paid web users hold the vault too, so this
    // shell drives DMs exactly like the native shell (`mlc-app-shell-vello`'s
    // `main.rs` is the reference) — just without a separate background
    // thread/channel, since every crypto op here is cheap wasm-thread
    // computation and every network call already goes through `spawn_local`.
    /// This account's DM identity keypair, deterministically derived from
    /// the profile's already-unlocked `VaultKey`
    /// (`mlc_vault::dm::DmIdentity::from_vault_key`) — never a separate
    /// secret to generate or persist. Re-derived whenever the held vault
    /// key changes (see `dm_identity_key`); dropped when the vault seals.
    dm_identity: Option<mlc_vault::dm::DmIdentity>,
    /// The vault key `dm_identity` was last derived from — compared every
    /// frame against `chart.core.user().vault_key()` so a fresh unlock (or
    /// profile switch) re-derives the identity instead of keeping a stale
    /// one from before a seal.
    dm_identity_key: Option<mlc_vault::vault::VaultKey>,
    /// The public half of the identity this browser has already attempted
    /// to publish (`PUT /api/dm/identity`) since the last unlock or sign-in
    /// edge — `None` means no attempt yet this epoch. Mirrors the native
    /// shell's own `dm_identity_publish_attempted_for`.
    dm_identity_publish_attempted_for: Option<[u8; 32]>,
    /// Peer public keys cached per conversation id — populated by
    /// `DmRequest::Open`'s peer lookup and by the backfill queue below for
    /// a conversation reached via `GET /api/dm/conversations` instead,
    /// which does not carry the peer's key. Needed to derive
    /// `conversation_key` for both sending and decrypting.
    dm_peer_keys: std::collections::HashMap<i64, [u8; 32]>,
    /// Conversations loaded via `ListConversations` whose peer key is not
    /// yet cached — `(conversation_id, peer_label)`, drained one at a time
    /// whenever `dm_in_flight` is free. Mirrors the native shell's own
    /// `dm_backfill_queue`.
    dm_backfill_queue: std::collections::VecDeque<(i64, String)>,
    /// A DM request is in flight — separate from `forum2_in_flight` above
    /// (unrelated request streams) so neither tab's fetch can block or be
    /// blocked by the other's.
    dm_in_flight: bool,

    // ── Checkout modal (billing) ────────────────────────────────────────
    /// Answers to checkout requests, oldest first; applied to the modal by
    /// `drive_checkout` (`checkout.rs`), the only place `&mut ChartApp` and
    /// the modal's clock meet the mailbox.
    checkout_events: std::collections::VecDeque<mlc_app_layout::ui::checkout::CheckoutEvent>,
}

pub struct CloudSession {
    inner: Rc<RefCell<Inner>>,
}

impl Clone for CloudSession {
    /// Cheap handle clone — shares the same `Inner` (the share fire-and-forget
    /// spawns carry a copy into their `spawn_local` future).
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl CloudSession {
    /// Build the session from localStorage + `window.MLC_BUILD`.
    /// `server_url` is "" (same-origin) in production.
    pub fn init(server_url: &str) -> Self {
        // This shell drives the checkout, so its entry-point buttons may show.
        mlc_app_layout::ui::checkout::set_entry_points_enabled(true);
        let token = load_token();
        let device_id = load_or_create_device_id();
        let attestation = read_build_attestation();
        if attestation.is_none() {
            web_sys::console::warn_1(
                &"[cloud] window.MLC_BUILD missing — attested endpoints will 403 once enforced".into(),
            );
        }
        let transport = FetchTransport::new(CloudConfig {
            server_url: server_url.to_string(),
            token: token.as_ref().map(|t| t.token.clone()),
            profile_id: String::new(),
            device_id,
            attestation,
        });
        Self {
            inner: Rc::new(RefCell::new(Inner {
                transport,
                token,
                in_flight: false,
                next_poll_ms: 0.0,
                was_open: false,
                results: std::collections::VecDeque::new(),
                unauthorized: false,
                me_in_flight: false,
                me_result: None,
                me_boot_checked: false,
                entitlements_in_flight: false,
                next_entitlements_ms: 0.0,
                entitlements_result: None,
                prev_entitlements: None,
                share_result: None,
                clone_name: None,
                share_rights_at_ms: 0.0,
                share_rights_busy: false,
                share_rights: None,
                share_admin_rows: None,
                share_admin_busy: false,
                share_ops_busy: false,
                share_ops_in: None,
                share_ops_push_ok: None,
                friend_busy: false,
                friend_accept_tried: false,
                friend_result: None,
                friend_lookup_q: String::new(),
                friend_lookup_at_ms: 0.0,
                friend_lookup_busy: false,
                friend_hits_in: None,
                edge_socket: None,
                edge_token: String::new(),
                edge_in: Vec::new(),
                edge_ticket: String::new(),
                edge_ticket_for: String::new(),
                edge_ticket_busy: false,
                edge_ticket_next_ms: 0.0,
                edge_hello_sent: false,
                edge_stop_sent: false,
                share_bound: None,
                share_lookup_q: String::new(),
                share_lookup_at_ms: 0.0,
                share_lookup_busy: false,
                share_lookup_hits: None,
                share_settings_note: None,
                preview_epoch: 0,
                preview_token: None,
                preview_card: None,
                clone_in_flight: false,
                clone_result: None,
                forum_in_flight: false,
                forum2_in_flight: false,
                dm_identity: None,
                dm_identity_key: None,
                dm_identity_publish_attempted_for: None,
                dm_peer_keys: std::collections::HashMap::new(),
                dm_backfill_queue: std::collections::VecDeque::new(),
                dm_in_flight: false,
                checkout_events: std::collections::VecDeque::new(),
            })),
        }
    }

    /// Whether this install holds a stored token — the `account_signed_in`
    /// source of truth for this shell (`UserSettingsState::
    /// account_signed_in`'s own doc: never inferred from the entitlements
    /// answer). Read at boot and right after a login/logout flips it.
    pub fn is_signed_in(&self) -> bool {
        self.inner.borrow().token.is_some()
    }

    /// Per-frame driver. Reads the overlay's phase + one-shot flags, advances
    /// the flow, applies finished async results.
    /// The signed-in account's display name, or `None` when signed out.
    ///
    /// The chrome's account button reads this. Without it that button said
    /// "Sign in" forever — the web shell held the token all along and simply
    /// never told the strip, so a signed-in visitor was still being invited to
    /// sign in (owner, 2026-09-03).
    pub fn account_label(&self) -> Option<String> {
        self.inner.borrow().token.as_ref().map(|t| t.display_name.clone())
    }

    /// The provider that account signed in with, or `None` when signed out.
    pub fn account_provider(&self) -> Option<String> {
        self.inner.borrow().token.as_ref().map(|t| t.provider.clone())
    }

    /// Fire the single `GET /api/auth/me` fetch for the currently active
    /// token — called once at boot (if a token is already stored) and
    /// again right after a login resolves, never polled. Guarded by
    /// `me_in_flight` so a second activation before the first fetch lands
    /// cannot stack requests.
    fn fire_me_fetch(&self) {
        if self.inner.borrow().me_in_flight {
            return;
        }
        self.inner.borrow_mut().me_in_flight = true;
        let inner = self.inner.clone();
        let transport = self.inner.borrow().transport.snapshot();
        spawn_local(async move {
            let raw = transport.auth_me().await;
            if let Err(e) = &raw {
                mark_unauthorized(&inner, e);
            }
            inner.borrow_mut().me_result = Some(raw.map_err(|e| e.to_string()));
        });
    }

    /// Drop to signed-out — local state only, regardless of why. Factored
    /// out of the original `FlowResult::Logout` arm (Arc B1, 2026-09-06) so
    /// a server-rejected token (401 from `auth_me()`, the entitlements
    /// fetch, share create, a forum request, nick, or redeem — see
    /// [`mark_unauthorized`]) demotes through the exact same path an
    /// explicit "Sign out" click does. `reason` is the toast body — the two
    /// callers differ only in wording ("You have been logged out." vs the
    /// server-rejected sentence).
    fn apply_signed_out(&self, chart: &mut ChartApp, reason: &str) {
        clear_token();
        clear_tier();
        {
            let mut inner = self.inner.borrow_mut();
            inner.transport.set_token(None);
            inner.token = None;
            // Drop straight to the free default rather than leaving the
            // last identity's (possibly paid) answer on screen until the
            // next scheduled re-ask — and re-ask right away so it reflects
            // "no credential" rather than staying stuck on a stale cached
            // TTL.
            inner.next_entitlements_ms = 0.0;
        }
        chart.panel_app.user_settings_state.multi_profile = false;
        chart.panel_app.user_settings_state.account_label = String::new();
        chart.panel_app.user_settings_state.account_provider = String::new();
        chart.panel_app.user_settings_state.account_classes = Vec::new();
        chart.panel_app.user_settings_state.account_roles = Vec::new();
        chart.panel_app.user_settings_state.nick_edit_mode = false;
        chart.panel_app.user_settings_state.nick_edit_focused = false;
        chart.panel_app.user_settings_state.nick_status =
            mlc_app_layout::ui::modal_settings::NickStatus::Idle;
        // The identity that submitted a code just left, so its outcome is
        // not this session's news either — same reset the native shell's
        // own sign-out already gives `redeem_status` (shell-parity).
        chart.panel_app.user_settings_state.redeem_requested = false;
        chart.panel_app.user_settings_state.redeem_status =
            mlc_app_layout::ui::modal_settings::RedeemStatus::Idle;
        // `Account` is never in the rail while signed out
        // (`UserSettingsTab::visible`) — snap off it here too, the same
        // defensive move the native shell's per-frame sync makes for
        // `Server`'s own sign-out gate.
        if chart.panel_app.user_settings_state.active_tab
            == mlc_app_layout::ui::modal_settings::UserSettingsTab::Account
        {
            chart.panel_app.user_settings_state.active_tab =
                mlc_app_layout::ui::modal_settings::UserSettingsTab::General;
        }
        // Same fields the entitlements Err path degrades to on a failed
        // fetch (`drive()`'s entitlements block) — set here too so this
        // does not wait out that fetch's own round trip to stop showing a
        // paid/forum-entitled account that no longer exists.
        chart.panel_app.cloud_overlay_state.forum_entitled = false;
        chart.panel_app.cloud_login_state.is_paid = false;
        chart.panel_app.cloud_login_state.paid_until = None;
        // The signed-out answer to `GET /api/forum/me` is not the same as
        // whatever identity just left — clear it so the passive fetch
        // re-asks under the new (anonymous) caller.
        chart.panel_app.community_state.forum2.caller_signed_in = false;
        chart.panel_app.community_state.forum2.invalidate_me();
        // The board LIST itself is per-caller too (owner defect 2026-09-09:
        // an admin signing in saw zero boards until a hard reload) —
        // `invalidate_me()` alone only re-asks classes/roles, never the
        // boards a DIFFERENT account can read. Force a re-fetch on the next
        // tab-open/frame instead of keeping the previous account's cached
        // list around.
        chart.panel_app.community_state.forum2.invalidate_boards();
        // Same reasoning for DMs: the server-confirmed fingerprint,
        // conversation list, and recipient-search dropdown all belong to
        // whoever just signed out — clear them so a later sign-in gets its
        // own auto-publish attempt (`dm_identity_publish_attempted_for`)
        // instead of reading a stale "already confirmed" from a different
        // account, and never shows a lookup answer meant for someone else.
        chart.panel_app.community_state.dm.identity_fingerprint = None;
        chart.panel_app.community_state.dm.conversations = Vec::new();
        chart.panel_app.community_state.dm.invalidate_conversations();
        chart.panel_app.community_state.dm.reset_to_list();
        self.inner.borrow_mut().dm_identity_publish_attempted_for = None;
        {
            let mut user = chart.core.user_mut();
            let p = user.profile_mut();
            p.domain.linked_account = None;
            p.domain.cloud_enabled = false;
            p.domain.sync_level = "local".to_string();
        }
        chart.profile_dirty = true;
        // Same-frame gating (owner ruling 2026-09-06): a ShareLink pick
        // made while signed in must not survive sign-out and keep firing
        // uploads afterward.
        if chart.panel_app.toolbar_state.screenshot_destination
            == mlc_core::state::toolbar_state::ScreenshotDestination::ShareLink
        {
            chart.panel_app.toolbar_state.screenshot_destination =
                mlc_core::state::toolbar_state::ScreenshotDestination::Clipboard;
        }
        chart.panel_app.cloud_login_state.phase = CloudLoginPhase::RequestCode;
        chart.push_system_event(mlc_core::SystemEventKind::Account, "Signed out", reason, true);
    }

    // ── Chart sharing (fire-and-forget) ──────────────────────────────────
    //
    // Both share creates are one-shot uploads with the same result contract:
    // on success the ABSOLUTE share URL (origin + `url_path`) goes to the OS
    // clipboard and the console; a 403 is the per-kind active-share ceiling
    // and is logged as "share limit reached"; anything else logs the
    // transport's own description. No mailbox — nothing in the UI waits on
    // the answer, so these bypass the `FlowResult`/`in_flight` machine.

    /// `POST /api/share/screenshot` — upload the cropped chart-region PNG.
    /// Anonymous sessions are refused up front (the camera dropdown already
    /// hides its ShareLink row for them; this is the belt to its braces).
    pub fn share_screenshot(&self, png: web_sys::Blob, title: String, epoch: u64) {
        if !self.is_signed_in() {
            web_sys::console::warn_1(&"[share] screenshot share refused: not signed in".into());
            return;
        }
        let transport = self.inner.borrow().transport.snapshot();
        let inner = self.inner.clone();
        let session = self.clone();
        spawn_local(async move {
            match transport.share_create_screenshot(&title, png).await {
                Ok(resp) => {
                    let url = absolute_share_url(&resp.url_path);
                    crate::clipboard_write_text(&url);
                    web_sys::console::log_1(&format!("[share] screenshot shared: {}", url).into());
                    inner.borrow_mut().share_result = Some(("screenshot", Ok(url)));
                    session.offer_preview_token(epoch, resp.token);
                }
                Err(e) => {
                    mark_unauthorized(&inner, &e);
                    let message = share_error_message("screenshot", &e);
                    log_share_error("screenshot", &e);
                    inner.borrow_mut().share_result = Some(("screenshot", Err(message)));
                }
            }
        });
    }

    /// `POST /api/share/live` — freeze the active preset into a public link.
    /// Same refusal/result contract as [`Self::share_screenshot`].
    fn fire_share_admin(&self, cmd: mlc_app::ShareAdminCmd) {
        if !self.is_signed_in() {
            return;
        }
        if self.inner.borrow().share_admin_busy {
            return;
        }
        self.inner.borrow_mut().share_admin_busy = true;
        let transport = self.inner.borrow().transport.snapshot();
        let inner = self.inner.clone();
        spawn_local(async move {
            let outcome = match cmd {
                mlc_app::ShareAdminCmd::Refresh => transport.share_list().await.map_err(|e| e.to_string()),
                mlc_app::ShareAdminCmd::Revoke { token } => transport
                    .share_revoke(&token)
                    .await
                    .map_err(|e| e.to_string())
                    .and(transport.share_list().await.map_err(|e| e.to_string())),
                mlc_app::ShareAdminCmd::Grant { token, nick, tiers } => {
                    let mut note = format!("Added {nick}");
                    for tier in &tiers {
                        if let Err(e) = transport.share_grant(&token, &nick, tier).await {
                            note = e.to_string();
                            break;
                        }
                        if tier == "edit" {
                            note = format!("{nick} can edit");
                        }
                    }
                    let mut guard = inner.borrow_mut();
                    guard.share_admin_busy = false;
                    guard.share_settings_note = Some(note);
                    return;
                }
                mlc_app::ShareAdminCmd::Patch { token, body } => {
                    let value = match serde_json::from_str::<serde_json::Value>(&body) {
                        Ok(v) => v,
                        Err(e) => {
                            inner.borrow_mut().share_admin_busy = false;
                            inner.borrow_mut().share_admin_rows = Some(Err(e.to_string()));
                            return;
                        }
                    };
                    match transport.share_patch(&token, value).await {
                        Ok(()) => transport.share_list().await.map_err(|e| e.to_string()),
                        Err(e) => Err(e.to_string()),
                    }
                }
            };
            let mut inner = inner.borrow_mut();
            inner.share_admin_busy = false;
            inner.share_admin_rows = Some(outcome);
        });
    }

    /// Bump the screenshot-card generation and drop anything still queued
    /// from the previous click.
    pub fn begin_preview(&self) -> u64 {
        let mut inner = self.inner.borrow_mut();
        inner.preview_epoch = inner.preview_epoch.wrapping_add(1);
        inner.preview_token = None;
        inner.preview_card = None;
        inner.preview_epoch
    }

    /// Chart PNG for the screenshot share that `begin_preview` just armed.
    /// A blob from an older click is ignored.
    pub fn offer_preview_card(&self, epoch: u64, card: web_sys::Blob) {
        {
            let mut inner = self.inner.borrow_mut();
            if inner.preview_epoch != epoch {
                return;
            }
            inner.preview_card = Some((epoch, card));
        }
        self.flush_preview();
    }

    fn offer_preview_token(&self, epoch: u64, token: String) {
        {
            let mut inner = self.inner.borrow_mut();
            if inner.preview_epoch != epoch {
                return;
            }
            inner.preview_token = Some((epoch, token));
        }
        self.flush_preview();
    }

    /// When both halves of one epoch are present, POST the card. Either
    /// half may arrive first.
    fn flush_preview(&self) {
        let ready = {
            let mut inner = self.inner.borrow_mut();
            match (inner.preview_token.take(), inner.preview_card.take()) {
                (Some((epoch_t, token)), Some((epoch_c, card))) if epoch_t == epoch_c => {
                    Some((token, card))
                }
                (token, card) => {
                    inner.preview_token = token;
                    inner.preview_card = card;
                    None
                }
            }
        };
        let Some((token, card)) = ready else {
            return;
        };
        let transport = self.inner.borrow().transport.snapshot();
        spawn_local(async move {
            match transport.share_upload_preview(&token, card).await {
                Ok(()) => {
                    web_sys::console::log_1(&"[share] preview card uploaded".into());
                }
                Err(e) => {
                    web_sys::console::warn_1(
                        &format!("[share] preview card upload failed: {e}").into(),
                    );
                }
            }
        });
    }

    pub fn share_live(
        &self,
        title: String,
        preset: serde_json::Value,
        chart_id: String,
        preview: Option<web_sys::Blob>,
        clone_allowed: bool,
        interaction: String,
        visibility: String,
    ) {
        if !self.is_signed_in() {
            web_sys::console::warn_1(&"[share] live share refused: not signed in".into());
            return;
        }
        let transport = self.inner.borrow().transport.snapshot();
        let inner = self.inner.clone();
        spawn_local(async move {
            match transport
                .share_create_live(&title, preset, &chart_id, clone_allowed, &interaction, &visibility)
                .await
            {
                Ok(resp) => {
                    let url = absolute_share_url(&resp.url_path);
                    crate::clipboard_write_text(&url);
                    web_sys::console::log_1(&format!("[share] live chart shared: {}", url).into());
                    if !chart_id.is_empty() {
                        inner.borrow_mut().share_bound = Some((chart_id.clone(), resp.token.clone()));
                    }
                    if let Some(png) = preview {
                        match transport.share_upload_preview(&resp.token, png).await {
                            Ok(()) => {
                                web_sys::console::log_1(&"[share] preview card uploaded".into());
                            }
                            Err(e) => {
                                web_sys::console::warn_1(
                                    &format!("[share] preview card upload failed: {e}").into(),
                                );
                            }
                        }
                    }
                    inner.borrow_mut().share_result = Some(("live", Ok(url)));
                }
                Err(e) => {
                    mark_unauthorized(&inner, &e);
                    let message = share_error_message("live", &e);
                    log_share_error("live", &e);
                    inner.borrow_mut().share_result = Some(("live", Err(message)));
                }
            }
        });
    }

    /// Nick search for the share-settings overlay. Account nicks
    /// (`GET /api/share/lookup`), not the messenger catalog. A grant needs
    /// a nick, so nickless accounts are not hits.
    fn poll_share_nick_search(&self, chart: &ChartApp, now_ms: f64) {
        let Some(q) = chart.share_settings_nick_query() else {
            return;
        };
        if q.len() < 2 {
            let mut inner = self.inner.borrow_mut();
            if !inner.share_lookup_q.is_empty() {
                inner.share_lookup_q.clear();
                inner.share_lookup_hits = Some(Vec::new());
            }
            return;
        }
        {
            let mut inner = self.inner.borrow_mut();
            if inner.share_lookup_busy || inner.share_lookup_q == q || now_ms - inner.share_lookup_at_ms < 300.0 {
                return;
            }
            inner.share_lookup_q = q.clone();
            inner.share_lookup_at_ms = now_ms;
            inner.share_lookup_busy = true;
        }
        let transport = self.inner.borrow().transport.snapshot();
        let inner = self.inner.clone();
        spawn_local(async move {
            let (hits, note) = match transport.share_nick_lookup(&q).await {
                Ok(users) => {
                    let hits = users
                        .into_iter()
                        .filter_map(|u| {
                            let nick = u.nick.trim().to_string();
                            if nick.is_empty() {
                                return None;
                            }
                            let label = if u.label == nick { String::new() } else { u.label };
                            Some((nick, label))
                        })
                        .collect::<Vec<_>>();
                    (hits, None)
                }
                Err(e) => (Vec::new(), Some(e.to_string())),
            };
            let mut guard = inner.borrow_mut();
            guard.share_lookup_busy = false;
            guard.share_lookup_hits = Some(hits);
            if let Some(note) = note {
                guard.share_settings_note = Some(note);
            }
        });
    }

    /// Re-read only the rights of the active shared tab. The preset stays.
    /// A policy toggle must show or hide Clone without reloading drawings.
    fn poll_share_rights(&self, chart: &ChartApp, now_ms: f64) {
        const INTERVAL_MS: f64 = 8000.0;
        let due = {
            let mut inner = self.inner.borrow_mut();
            if inner.share_rights_busy || now_ms - inner.share_rights_at_ms < INTERVAL_MS {
                return;
            }
            let active = chart.panel_app.active_preset_id.clone();
            let Some(meta) = chart.shared_chart_meta(&active) else {
                return;
            };
            if meta.kind != "live" || meta.token.is_empty() {
                return;
            }
            inner.share_rights_at_ms = now_ms;
            inner.share_rights_busy = true;
            meta.token.clone()
        };
        let token = due;
        let transport = self.inner.borrow().transport.snapshot();
        let inner = self.inner.clone();
        spawn_local(async move {
            let allowed = match transport.share_get_public(&token).await {
                Ok(mlc_cloud::SharePublicResponse::Live { clone_allowed, .. }) => {
                    Some(clone_allowed)
                }
                _ => None,
            };
            let mut guard = inner.borrow_mut();
            guard.share_rights_busy = false;
            if let Some(allowed) = allowed {
                guard.share_rights = Some((token, allowed));
            }
        });
    }

    fn poll_friend_lookup(&self, chart: &ChartApp, now_ms: f64) {
        let Some(q) = chart.friends_nick_query() else {
            return;
        };
        if q.len() < 2 {
            let mut inner = self.inner.borrow_mut();
            if !inner.friend_lookup_q.is_empty() {
                inner.friend_lookup_q.clear();
                inner.friend_hits_in = Some(Vec::new());
            }
            return;
        }
        {
            let mut inner = self.inner.borrow_mut();
            if inner.friend_lookup_busy || inner.friend_lookup_q == q || now_ms - inner.friend_lookup_at_ms < 300.0 {
                return;
            }
            inner.friend_lookup_q = q.clone();
            inner.friend_lookup_at_ms = now_ms;
            inner.friend_lookup_busy = true;
        }
        let transport = self.inner.borrow().transport.snapshot();
        let inner = self.inner.clone();
        spawn_local(async move {
            let hits = match transport.share_nick_lookup(&q).await {
                Ok(users) => users
                    .into_iter()
                    .filter_map(|u| {
                        let nick = u.nick.trim().to_string();
                        if nick.is_empty() { None } else { Some(nick) }
                    })
                    .collect::<Vec<_>>(),
                Err(_) => Vec::new(),
            };
            let mut guard = inner.borrow_mut();
            guard.friend_lookup_busy = false;
            guard.friend_hits_in = Some(hits);
        });
    }

    fn poll_friends(&self, chart: &mut ChartApp) {
        if let Some(hits) = self.inner.borrow_mut().friend_hits_in.take() {
            chart.set_friend_hits(hits);
        }
        if let Some(apply) = self.inner.borrow_mut().friend_result.take() {
            match apply {
                FriendApply::List(roster) => {
                    let uss = &mut chart.panel_app.user_settings_state;
                    uss.discoverable = roster.discoverable;
                    uss.friend_nicks = roster.friends;
                    uss.friend_incoming = roster.incoming;
                    uss.friend_outgoing = roster.outgoing;
                    uss.friend_subscriptions = roster.subscriptions;
                    uss.friend_blocked = roster.blocked;
                    if uss.friend_note != "Copied" {
                        uss.friend_note.clear();
                    }
                }
                FriendApply::Invite(url) => {
                    let full = format!("https://chart.example{url}");
                    chart.panel_app.user_settings_state.friend_invite = full.clone();
                    chart.clipboard_text = Some(full);
                    chart.panel_app.user_settings_state.friend_note = "Copied".to_string();
                }
                FriendApply::Note(note) => {
                    chart.panel_app.user_settings_state.friend_note = note;
                }
            }
        }
        if self.inner.borrow().friend_busy || !self.is_signed_in() {
            return;
        }
        if !self.inner.borrow().friend_accept_tried {
            self.inner.borrow_mut().friend_accept_tried = true;
            if let Some(token) = friend_query_token() {
                self.inner.borrow_mut().friend_busy = true;
                let transport = self.inner.borrow().transport.snapshot();
                let inner = self.inner.clone();
                spawn_local(async move {
                    let _ = transport.account_accept_friend(&token).await;
                    if let Ok(roster) = transport.account_friends().await {
                        inner.borrow_mut().friend_result = Some(FriendApply::List(roster));
                    }
                    inner.borrow_mut().friend_busy = false;
                });
                return;
            }
        }
        let cmd = chart.panel_app.user_settings_state.friend_cmd.take();
        let Some(cmd) = cmd else {
            return;
        };
        self.inner.borrow_mut().friend_busy = true;
        let transport = self.inner.borrow().transport.snapshot();
        let inner = self.inner.clone();
        let discoverable = chart.panel_app.user_settings_state.discoverable;
        spawn_local(async move {
            let act = ["request", "approve", "remove", "block", "unblock", "subscribe", "unsubscribe"]
                .iter()
                .find_map(|action| cmd.strip_prefix(&format!("{action}:")).map(|nick| (*action, nick.to_string())));
            let result = if cmd == "discoverable" {
                let _ = transport.account_set_discoverable(discoverable).await;
                transport.account_friends().await.ok().map(FriendApply::List)
            } else if cmd == "invite" {
                transport.account_friend_invite().await.ok().map(FriendApply::Invite)
            } else if let Some((action, nick)) = act {
                match transport.account_friend_act(&nick, action).await {
                    Ok(()) => transport.account_friends().await.ok().map(FriendApply::List),
                    Err(e) => Some(FriendApply::Note(e.to_string())),
                }
            } else {
                transport.account_friends().await.ok().map(FriendApply::List)
            };
            let mut guard = inner.borrow_mut();
            guard.friend_result = result;
            guard.friend_busy = false;
        });
    }

    fn poll_edge(&self, chart: &mut ChartApp, now_ms: f64) {
        let incoming = std::mem::take(&mut self.inner.borrow_mut().edge_in);
        for frame in incoming {
            chart.apply_edge_frame(&frame);
        }
        let token = chart.edge_live_token();
        if token.is_none() {
            let open = self.inner.borrow().edge_socket.as_ref().map(|ws| ws.ready_state()) == Some(1);
            let stop_sent = self.inner.borrow().edge_stop_sent;
            if open && !stop_sent {
                if let Some((_, frames)) = chart.take_live_stop_frames() {
                    if let Some(ws) = self.inner.borrow().edge_socket.as_ref() {
                        for frame in &frames {
                            let _ = ws.send_with_str(frame);
                        }
                    }
                }
                self.inner.borrow_mut().edge_stop_sent = true;
                return;
            }
            self.close_edge();
            self.inner.borrow_mut().edge_stop_sent = false;
            return;
        }
        self.inner.borrow_mut().edge_stop_sent = false;
        let token = token.unwrap();
        let (have_ticket, busy, ticket_for, next_ms) = {
            let guard = self.inner.borrow();
            (
                !guard.edge_ticket.is_empty() && guard.edge_ticket_for == token,
                guard.edge_ticket_busy,
                guard.edge_ticket_for.clone(),
                guard.edge_ticket_next_ms,
            )
        };
        if !have_ticket {
            if busy && ticket_for == token {
                return;
            }
            if now_ms < next_ms && ticket_for == token {
                return;
            }
            {
                let mut guard = self.inner.borrow_mut();
                guard.edge_ticket_busy = true;
                guard.edge_ticket.clear();
                guard.edge_ticket_for = token.clone();
                guard.edge_hello_sent = false;
            }
            self.close_edge_socket();
            let transport = self.inner.borrow().transport.snapshot();
            let inner = self.inner.clone();
            let token_fetch = token.clone();
            spawn_local(async move {
                let result = transport.share_stream_open(&token_fetch).await;
                let mut guard = inner.borrow_mut();
                guard.edge_ticket_busy = false;
                match result {
                    Ok(body) if !body.ticket.is_empty() && guard.edge_ticket_for == token_fetch => {
                        guard.edge_ticket = body.ticket;
                    }
                    _ => {
                        if guard.edge_ticket_for == token_fetch {
                            guard.edge_ticket_for.clear();
                            guard.edge_ticket_next_ms = now_ms + 1000.0;
                        }
                    }
                }
            });
            return;
        }
        self.ensure_edge(&token);
        let open = self.inner.borrow().edge_socket.as_ref().map(|ws| ws.ready_state()) == Some(1);
        if !open || !self.inner.borrow().edge_hello_sent {
            return;
        }
        let Some((_, frames)) = chart.take_edge_frames() else {
            return;
        };
        let guard = self.inner.borrow();
        let Some(ws) = guard.edge_socket.as_ref() else {
            return;
        };
        for frame in frames {
            let _ = ws.send_with_str(&frame);
        }
    }

    fn close_edge_socket(&self) {
        let mut guard = self.inner.borrow_mut();
        if let Some(ws) = guard.edge_socket.take() {
            let _ = ws.close();
        }
        guard.edge_token.clear();
        guard.edge_hello_sent = false;
    }

    fn close_edge(&self) {
        self.close_edge_socket();
        let mut guard = self.inner.borrow_mut();
        guard.edge_ticket.clear();
        guard.edge_ticket_for.clear();
        guard.edge_ticket_busy = false;
    }

    fn ensure_edge(&self, token: &str) {
        let mut dead = false;
        {
            let guard = self.inner.borrow();
            if let Some(ws) = guard.edge_socket.as_ref() {
                if guard.edge_token == token {
                    let state = ws.ready_state();
                    if state == 0 || state == 1 {
                        return;
                    }
                }
                dead = true;
            }
        }
        if dead {
            self.close_edge_socket();
            let mut guard = self.inner.borrow_mut();
            guard.edge_ticket.clear();
            guard.edge_ticket_for.clear();
            return;
        }
        let ticket = self.inner.borrow().edge_ticket.clone();
        let Some(win) = web_sys::window() else { return };
        let loc = win.location();
        let proto = loc.protocol().ok().map(|p| if p.starts_with("https") { "wss" } else { "ws" }).unwrap_or("wss");
        let Ok(host) = loc.host() else { return };
        let url = format!("{proto}://{host}/api/share-live/{token}");
        let Ok(ws) = web_sys::WebSocket::new(&url) else { return };
        let inner = self.inner.clone();
        let on_msg = wasm_bindgen::closure::Closure::wrap(Box::new(move |ev: web_sys::MessageEvent| {
            if let Some(text) = ev.data().as_string() {
                inner.borrow_mut().edge_in.push(text);
            }
        }) as Box<dyn FnMut(web_sys::MessageEvent)>);
        ws.set_onmessage(Some(on_msg.as_ref().unchecked_ref()));
        on_msg.forget();
        let inner_open = self.inner.clone();
        let on_open = wasm_bindgen::closure::Closure::wrap(Box::new(move || {
            let hello = format!("{{\"t\":\"hello\",\"k\":\"{ticket}\"}}");
            let guard = inner_open.borrow();
            if let Some(sock) = guard.edge_socket.as_ref() {
                let _ = sock.send_with_str(&hello);
            }
            drop(guard);
            inner_open.borrow_mut().edge_hello_sent = true;
        }) as Box<dyn FnMut()>);
        ws.set_onopen(Some(on_open.as_ref().unchecked_ref()));
        on_open.forget();
        let mut guard = self.inner.borrow_mut();
        guard.edge_socket = Some(ws);
        guard.edge_token = token.to_string();
    }

    fn poll_share_ops(&self, chart: &mut ChartApp, now_ms: f64) {
        if let Some(ok) = self.inner.borrow_mut().share_ops_push_ok.take() {
            chart.ack_share_push(ok);
        }
        if let Some((token, resp)) = self.inner.borrow_mut().share_ops_in.take() {
            let ops: Vec<mlc_app::share_stream::ShareInboundOp> = resp
                .ops
                .into_iter()
                .map(|op| mlc_app::share_stream::ShareInboundOp {
                    seq: op.seq,
                    kind: op.kind,
                    object_id: op.object_id,
                    body: op.body,
                })
                .collect();
            chart.apply_share_ops(&token, resp.reset, resp.can_send, resp.live, &ops);
        }
        if self.inner.borrow().share_ops_busy {
            return;
        }
        if let Some((token, ops)) = chart.share_ops_due(now_ms) {
            self.inner.borrow_mut().share_ops_busy = true;
            let transport = self.inner.borrow().transport.snapshot();
            let inner = self.inner.clone();
            let wire: Vec<mlc_cloud::ShareChartOp> = ops
                .into_iter()
                .map(|op| mlc_cloud::ShareChartOp {
                    kind: op.kind,
                    object_id: op.object_id,
                    body: op.body,
                    seq: 0,
                })
                .collect();
            spawn_local(async move {
                let ok = match transport.share_post_ops(&token, &wire).await {
                    Ok(()) => true,
                    Err(e) => {
                        web_sys::console::warn_1(&format!("[share] op post failed: {e}").into());
                        false
                    }
                };
                inner.borrow_mut().share_ops_push_ok = Some(ok);
                inner.borrow_mut().share_ops_busy = false;
            });
            return;
        }
        if let Some((token, after)) = chart.share_pull_due(now_ms) {
            self.inner.borrow_mut().share_ops_busy = true;
            let transport = self.inner.borrow().transport.snapshot();
            let inner = self.inner.clone();
            spawn_local(async move {
                match transport.share_get_ops(&token, after).await {
                    Ok(resp) => {
                        inner.borrow_mut().share_ops_in = Some((token, resp));
                    }
                    Err(e) => {
                        web_sys::console::warn_1(&format!("[share] op pull failed: {e}").into());
                    }
                }
                inner.borrow_mut().share_ops_busy = false;
            });
        }
    }

    pub fn drive(&self, chart: &mut ChartApp, now_ms: f64) {
        if let Some((token, allowed)) = self.inner.borrow_mut().share_rights.take() {
            chart.set_share_clone_allowed(&token, allowed);
        }
        self.poll_share_rights(chart, now_ms);
        self.poll_share_nick_search(chart, now_ms);
        self.poll_friend_lookup(chart, now_ms);
        if let Some((chart_id, token)) = self.inner.borrow_mut().share_bound.take() {
            let arm = chart.take_arm_live();
            chart.note_chart_share_token(&chart_id, &token);
            if arm {
                chart.set_share_live(true);
            }
        }
        if let Some(hits) = self.inner.borrow_mut().share_lookup_hits.take() {
            chart.set_share_lookup_hits(hits);
        }
        if let Some(note) = self.inner.borrow_mut().share_settings_note.take() {
            chart.set_share_settings_note(note);
        }
        // ── Server-rejected token → signed-out (Arc B1, 2026-09-06) ───────
        // Drained first thing, every frame: any authed call in this driver
        // (boot/post-login `auth_me()`, the entitlements refresh, share
        // create, forum requests, nick, redeem) flags `unauthorized` the
        // instant its `spawn_local` body sees a 401 — see
        // `mark_unauthorized`'s own doc. Runs ahead of the
        // `account_signed_in`/`account_label` mirror just below so that
        // mirror reads the already-cleared token this same frame.
        let unauthorized = std::mem::take(&mut self.inner.borrow_mut().unauthorized);
        if unauthorized {
            self.apply_signed_out(
                chart,
                "The server no longer accepts this session — sign in again.",
            );
        }

        // Keep the profile header in sync with the active profile.
        let profile_id = chart.core.user().profile().domain.profile_id.clone();
        {
            let mut inner = self.inner.borrow_mut();
            let cfg = &mut inner.transport.config_mut().profile_id;
            if *cfg != profile_id {
                *cfg = profile_id;
            }
        }

        // `account_signed_in` tracks the stored token directly — covers the
        // boot frame and the frame right after a login/logout resolves
        // below without needing a separate write at each call site. Cheap
        // (a bool copy), unlike the entitlements fetch this is not an
        // endpoint call, so there is no reason to gate it to "moments".
        chart.panel_app.user_settings_state.account_signed_in = self.is_signed_in();
        // `ForumV2State::caller_signed_in` mirrors the same source — see
        // that field's own doc for why it is not derived from
        // `caller_classes` (which stays empty until the `Me` fetch
        // answers).
        chart.panel_app.community_state.forum2.caller_signed_in = self.is_signed_in();
        // Mirrors `account_label()` — the same source the chrome's account
        // button reads — so the nick edit box can prefill with what the
        // person already sees there, not the unrelated local profile name.
        chart.panel_app.user_settings_state.account_label =
            self.account_label().unwrap_or_default();
        chart.panel_app.user_settings_state.account_provider =
            self.account_provider().unwrap_or_default();
        self.drive_checkout(chart, now_ms);

        // ── Entitlements: boot + periodic background re-ask ────────────────
        // Deliberately its own block, ahead of and independent from the
        // login/overlay phase machine below — `UserSettingsState::
        // multi_profile` gates the always-visible General tab, so this must
        // keep refreshing whether or not the login overlay is even open.
        {
            let finished = self.inner.borrow_mut().entitlements_result.take();
            if let Some(r) = finished {
                let mut inner = self.inner.borrow_mut();
                inner.entitlements_in_flight = false;
                match r {
                    Ok(resp) => {
                        chart.panel_app.user_settings_state.multi_profile = resp.has_multi_profile();
                        // The Forum tab's sole visibility switch — written
                        // every entitlements cycle, same cadence
                        // `multi_profile` already gets (see
                        // `CloudOverlayState::forum_entitled`'s own doc).
                        chart.panel_app.cloud_overlay_state.forum_entitled = resp.has_forum();
                        // Tier transition → system event (owner ask
                        // 2026-09-05, scope §8): compare against the last
                        // APPLIED answer, not the live `cloud_login_state`
                        // this same block is about to overwrite below.
                        // `None` means this is the first entitlements answer
                        // of THIS session (boot or a reload) — fall back to
                        // the last answer this browser persisted to
                        // `TIER_KEY` for the SAME identity, so a tier change
                        // applied while the page was closed (a gift code
                        // redeemed via the API, then a reload) still
                        // surfaces once instead of silently becoming the new
                        // baseline. No stored record, or one for a
                        // different user, keeps the old behaviour: no event.
                        let current_user_id = inner.token.as_ref().map(|t| t.user_id);
                        let prev = inner.prev_entitlements.replace((resp.paid, resp.paid_until.clone()));
                        let baseline = prev.or_else(|| {
                            let uid = current_user_id?;
                            let stored = load_tier()?;
                            (stored.user_id == uid).then_some((stored.paid, stored.paid_until))
                        });
                        if let Some((prev_paid, prev_until)) = baseline {
                            if prev_paid != resp.paid || prev_until != resp.paid_until {
                                let (title, body) = if resp.paid {
                                    (
                                        "Paid tier active",
                                        match &resp.paid_until {
                                            Some(u) => format!("Active until {u}."),
                                            None => "Your paid tier is active.".to_string(),
                                        },
                                    )
                                } else {
                                    ("Paid tier expired", "Your paid tier has expired.".to_string())
                                };
                                chart.push_system_event(mlc_core::SystemEventKind::Tier, title, body, true);
                            }
                        }
                        // Persist this APPLIED answer as the new baseline —
                        // only when signed in; an anonymous entitlements
                        // answer has no identity to key the record on.
                        if let Some(uid) = current_user_id {
                            save_tier(&StoredTier {
                                user_id: uid,
                                paid: resp.paid,
                                paid_until: resp.paid_until.clone(),
                            });
                        }
                        // The account view states the tier outright; this is
                        // where that fact arrives. Read from the server's own
                        // `paid` field, never inferred from which features
                        // happen to be in the list.
                        chart.panel_app.cloud_login_state.is_paid = resp.paid;
                        chart.panel_app.cloud_login_state.paid_until = resp.paid_until.clone();
                        // `expires_at` is unix-ms, the same clock as
                        // `now_ms` here — floor it so a server answer that
                        // is already stale (or asks to be re-checked
                        // immediately) cannot turn into a fetch-every-frame
                        // loop.
                        inner.next_entitlements_ms =
                            (resp.expires_at as f64).max(now_ms + ENTITLEMENTS_MIN_REASK_MS);
                    }
                    Err(e) => {
                        // Deliberately does NOT touch `inner.prev_entitlements`
                        // (and fires no Tier event): a transient network
                        // failure degrading the DISPLAYED tier to free is not
                        // the same fact as the server actually reporting a
                        // paid→free transition, and firing "Paid tier
                        // expired" on every blip would be a false report.
                        // A failed or unanswered fetch degrades to the free
                        // set — never to the paid set, and never leaves the
                        // General tab locked on a stale answer either.
                        chart.panel_app.user_settings_state.multi_profile = false;
                        chart.panel_app.cloud_login_state.is_paid = false;
                        chart.panel_app.cloud_login_state.paid_until = None;
                        chart.panel_app.cloud_overlay_state.forum_entitled = false;
                        web_sys::console::warn_1(
                            &format!("[cloud] entitlements fetch failed: {}", e).into(),
                        );
                        inner.next_entitlements_ms = now_ms + ENTITLEMENTS_RETRY_MS;
                    }
                }
            }

            let due = {
                let inner = self.inner.borrow();
                !inner.entitlements_in_flight && now_ms >= inner.next_entitlements_ms
            };
            if due {
                self.inner.borrow_mut().entitlements_in_flight = true;
                let inner = self.inner.clone();
                let transport = self.inner.borrow().transport.snapshot();
                spawn_local(async move {
                    let raw = transport.entitlements().await;
                    if let Err(e) = &raw {
                        mark_unauthorized(&inner, e);
                    }
                    inner.borrow_mut().entitlements_result = Some(raw.map_err(|e| e.to_string()));
                });
            }
        }

        // ── Account label refresh (`GET /api/auth/me`) ────────────────────
        // Own block, independent of the login/overlay phase machine below —
        // a nick set on another device or by the API must reach the
        // chrome's account button without the person reopening the login
        // overlay. Fired once per token activation: here at boot, and again
        // right after a login resolves further down where the token is
        // saved. Never polled — see `Inner::me_result`'s own doc.
        {
            let finished = self.inner.borrow_mut().me_result.take();
            if let Some(r) = finished {
                let mut inner = self.inner.borrow_mut();
                inner.me_in_flight = false;
                match r {
                    Ok(resp) => {
                        // The stored token's `display_name` is what the
                        // account button paints, and it is the LABEL, not
                        // the provider name: `effective_label` (nick →
                        // display_name → public_id, server-computed) may
                        // differ from it if a nick was set on another
                        // device or via the API. Silent refresh: no system
                        // event, no toast.
                        let label = resp.effective_label.unwrap_or(resp.display_name);
                        let stale = inner
                            .token
                            .as_ref()
                            .map(|t| t.display_name != label)
                            .unwrap_or(false);
                        if stale {
                            if let Some(t) = inner.token.as_mut() {
                                t.display_name = label;
                            }
                            if let Some(t) = inner.token.clone() {
                                save_token(&t);
                            }
                        }
                    }
                    Err(e) => {
                        web_sys::console::debug_1(
                            &format!("[cloud] me() fetch failed: {}", e).into(),
                        );
                    }
                }
            }

            let due = {
                let inner = self.inner.borrow();
                !inner.me_boot_checked && !inner.me_in_flight && inner.token.is_some()
            };
            if due {
                self.fire_me_fetch();
            }
            self.inner.borrow_mut().me_boot_checked = true;
        }

        // ── Chart-share result → system event (owner ask 2026-09-05) ──────
        // `share_screenshot`/`share_live` are fire-and-forget (no `FlowResult`
        // mailbox — see their own doc comment); this drains their own tiny
        // mailbox, since `&mut ChartApp` is only reachable here.
        if let Some((kind, result)) = self.inner.borrow_mut().share_result.take() {
            match result {
                Ok(url) => {
                    let title = if kind == "live" { "Live chart shared" } else { "Screenshot shared" };
                    chart.push_system_event(
                        mlc_core::SystemEventKind::Share,
                        title,
                        format!("Link copied to clipboard: {url}"),
                        true,
                    );
                }
                Err(message) => {
                    chart.clear_arm_live();
                    chart.push_system_event(mlc_core::SystemEventKind::Share, "Share failed", message, true);
                }
            }
        }

        if let Some(rows) = self.inner.borrow_mut().share_admin_rows.take() {
            match rows {
                Ok(rows) => {
                    chart.panel_app.user_settings_state.share_rows = rows
                        .into_iter()
                        .map(|r| mlc_app_layout::ui::modal_settings::ShareAdminRow {
                            token: r.token,
                            title: r.title,
                            kind: r.kind,
                            pinned: r.pinned,
                            clone_allowed: r.clone_allowed,
                            interaction: r.interaction,
                            visibility: if r.visibility.is_empty() {
                                "public".to_string()
                            } else {
                                r.visibility
                            },
                            allow_forward: r.allow_forward,
                            expires_at: r.expires_at,
                            chart_id: r.chart_id,
                        })
                        .collect();
                    let bound: Vec<(String, String)> = chart
                        .panel_app
                        .user_settings_state
                        .share_rows
                        .iter()
                        .filter(|r| !r.chart_id.is_empty())
                        .map(|r| (r.chart_id.clone(), r.token.clone()))
                        .collect();
                    for (chart_id, token) in bound {
                        chart.note_chart_share_token(&chart_id, &token);
                    }
                    chart.panel_app.user_settings_state.share_status.clear();
                    let flags: Vec<(String, bool)> = chart
                        .panel_app
                        .user_settings_state
                        .share_rows
                        .iter()
                        .map(|r| (r.token.clone(), r.clone_allowed))
                        .collect();
                    for (token, allowed) in flags {
                        chart.set_share_clone_allowed(&token, allowed);
                    }
                }
                Err(message) => {
                    chart.panel_app.user_settings_state.share_status = message.clone();
                    chart.panel_app.share_settings_state.status = message;
                }
            }
        }
        if !self.inner.borrow().share_admin_busy {
            if let Some(cmd) = chart.take_share_admin() {
                self.fire_share_admin(cmd);
            }
        }
        chart.drain_share_catchup();
        self.poll_share_ops(chart, now_ms);
        self.poll_edge(chart, now_ms);
        self.poll_friends(chart);

        // ── Chart-share clone (shared-viewer arc, 2026-09-06) ──────────────
        // `ChartApp::request_clone_share` is the one-shot mailbox a "Clone to
        // my charts" click on a read-only viewer tab arms
        // (`ChartApp::dispatch_panel_click`'s `share:clone:` arm); drained
        // here because `&mut ChartApp` — and `apply_cloned_preset`, which
        // needs it — is only reachable inside `drive()`.
        if let Some(token) = chart.take_pending_clone_share() {
            if !chart.clone_open_share_locally(&token) {
                chart.push_system_event(
                    mlc_core::SystemEventKind::Share,
                    "Clone failed",
                    "This shared chart is not open, or clone is not allowed.".to_string(),
                    true,
                );
            }
        }

        if let Some(result) = self.inner.borrow_mut().clone_result.take() {
            self.inner.borrow_mut().clone_in_flight = false;
            match result {
                Ok((preset_value, stripped)) => {
                    match serde_json::from_value::<mlc_core::preset::preset::ChartPreset>(preset_value) {
                        Ok(mut preset) => {
                            if let Some(name) = self.inner.borrow_mut().clone_name.take() {
                                preset.name = name;
                            }
                            let stripped = stripped
                                .into_iter()
                                .map(|s| mlc_app::StrippedIndicator { type_id: s.type_id, name: s.name })
                                .collect();
                            chart.apply_cloned_preset(preset, stripped);
                        }
                        Err(e) => {
                            self.inner.borrow_mut().clone_name = None;
                            chart.push_system_event(
                                mlc_core::SystemEventKind::Share,
                                "Clone failed",
                                format!("The server's cloned chart did not parse: {e}"),
                                true,
                            );
                        }
                    }
                }
                Err(message) => {
                    self.inner.borrow_mut().clone_name = None;
                    chart.push_system_event(mlc_core::SystemEventKind::Share, "Clone failed", message, true);
                }
            }
        }

        // ── Apply finished async results ─────────────────────────────────
        let results: Vec<FlowResult> = self.inner.borrow_mut().results.drain(..).collect();
        for result in results {
            self.inner.borrow_mut().in_flight = false;
            let st = &mut chart.panel_app.cloud_login_state;
            match result {
                FlowResult::Code(Ok(code)) => {
                    st.phase = CloudLoginPhase::AwaitingConfirm { code };
                    self.inner.borrow_mut().next_poll_ms = now_ms + POLL_INTERVAL_MS;
                }
                FlowResult::Code(Err(e)) => {
                    st.phase = CloudLoginPhase::Error { message: e };
                }
                FlowResult::Poll(Ok(DeviceAuthPoll::Pending)) => {
                    self.inner.borrow_mut().next_poll_ms = now_ms + POLL_INTERVAL_MS;
                }
                FlowResult::Poll(Ok(DeviceAuthPoll::Expired)) => {
                    st.phase = CloudLoginPhase::Error {
                        message: "Code expired — try again".to_string(),
                    };
                }
                FlowResult::Poll(Ok(DeviceAuthPoll::Confirmed {
                    token,
                    display_name,
                    provider,
                    user_id,
                    public_id,
                })) => {
                    let stored = StoredToken {
                        token,
                        provider: provider.clone(),
                        display_name: display_name.clone(),
                        saved_at: (now_ms / 1000.0) as u64,
                        user_id,
                        public_id: Some(public_id.clone()),
                    };
                    save_token(&stored);
                    let mut inner = self.inner.borrow_mut();
                    inner.transport.set_token(Some(stored.token.clone()));
                    inner.token = Some(stored);
                    // A different identity now answers the entitlements
                    // question — re-ask right away rather than waiting out
                    // the previous (usually anonymous/free) answer's TTL.
                    inner.next_entitlements_ms = 0.0;
                    drop(inner);
                    // A fresh identity may already carry a nick set
                    // elsewhere (another device, the API) — ask once now
                    // rather than showing this frame's login-time
                    // `display_name` until the next re-login.
                    self.fire_me_fetch();
                    // The forum-v2 `caller_roles`/`caller_classes` answer, if
                    // any was already fetched, belongs to whoever was signed
                    // in (or nobody) BEFORE this login — clear it so the
                    // passive fetch re-asks for the new identity.
                    chart.panel_app.community_state.forum2.invalidate_me();
                    // The board LIST and DM conversation list are per-caller
                    // too (owner defect 2026-09-09: an admin signing in saw
                    // zero boards until a hard reload) — `invalidate_me()`
                    // alone only re-asks classes/roles, never the boards a
                    // DIFFERENT account can read, and this account's own
                    // conversations are not the ones whoever was signed in
                    // before had cached. Also drop any recipient-search
                    // dropdown state so a stale answer from before this
                    // login can never show under the new caller.
                    chart.panel_app.community_state.forum2.invalidate_boards();
                    chart.panel_app.community_state.dm.invalidate_conversations();
                    chart.panel_app.community_state.dm.close_lookup_dropdown();
                    // Sign-in edge (owner ruling 2026-09-09): this session
                    // gets its own DM auto-publish attempt, even if an
                    // earlier attempt this same unlock already failed while
                    // signed out — mirrors the native shell's own reset at
                    // this exact edge.
                    self.inner.borrow_mut().dm_identity_publish_attempted_for = None;
                    // Link the account on the profile (persisted by the
                    // shell's per-frame dirty drain).
                    {
                        let mut user = chart.core.user_mut();
                        let p = user.profile_mut();
                        p.domain.linked_account =
                            Some(mlc_core::user_profile::profile::LinkedAccount {
                                provider: provider.clone(),
                                public_id,
                                display_name: display_name.clone(),
                                linked_at: (now_ms / 1000.0) as u64,
                            });
                        p.domain.cloud_enabled = true;
                        p.domain.sync_level = "cloud".to_string();
                    }
                    chart.profile_dirty = true;
                    // `st`'s borrow (`&mut chart.panel_app.cloud_login_state`)
                    // must end before `push_system_event` below takes `&mut
                    // chart` — this is `st`'s last use on this arm's path.
                    st.phase = CloudLoginPhase::Success { display_name: display_name.clone() };
                    chart.push_system_event(
                        mlc_core::SystemEventKind::Account,
                        "Signed in",
                        format!("Signed in as {display_name}."),
                        true,
                    );
                }
                FlowResult::Poll(Err(e)) => {
                    // Transient poll failure — keep waiting, but surface
                    // persistent auth errors.
                    if e == "unauthorized" {
                        st.phase = CloudLoginPhase::Error { message: e };
                    } else {
                        self.inner.borrow_mut().next_poll_ms = now_ms + POLL_INTERVAL_MS;
                    }
                }
                FlowResult::Updates(Ok((platform, m, releases))) => {
                    chart.panel_app.cloud_overlay_state.data = CloudOverlayData::Ready {
                        platform: platform.to_string(),
                        version: m.version,
                        notes: m.release_notes,
                        download_url: m.download_url,
                        releases: releases
                            .into_iter()
                            .map(|r| CloudReleaseEntry {
                                platform: r.platform,
                                version: r.version,
                                file_size: r.file_size,
                                download_url: r.download_url,
                                created_at: r.created_at,
                            })
                            .collect(),
                    };
                }
                FlowResult::Updates(Err(e)) => {
                    chart.panel_app.cloud_overlay_state.data = CloudOverlayData::Error { message: e };
                }
                FlowResult::News(Ok(posts)) => {
                    let ov = &mut chart.panel_app.cloud_overlay_state;
                    ov.data = CloudOverlayData::News {
                        posts: posts
                            .into_iter()
                            .map(|p| CloudNewsPost {
                                id: p.id,
                                kind: p.kind,
                                version: p.version,
                                date: p.date,
                                title: p.title,
                                summary: p.summary,
                                body: p.body,
                                url: p.url,
                            })
                            .collect(),
                    };
                    ov.mark_news_fetched();
                }
                FlowResult::News(Err(e)) => {
                    let ov = &mut chart.panel_app.cloud_overlay_state;
                    // A due-refresh failure must not blank an already-shown
                    // page — only an initial (still-`Loading`) fetch's error
                    // replaces `data`; a background refresh just clears its
                    // own in-flight flag and quietly retries next cadence,
                    // same "leave the current page on screen" contract the
                    // Forum tab's own background refresh gives.
                    if matches!(ov.data, CloudOverlayData::Loading) {
                        ov.data = CloudOverlayData::Error { message: e };
                    } else {
                        web_sys::console::warn_1(&format!("[news] refresh failed: {}", e).into());
                    }
                    ov.news_refreshing = false;
                }
                FlowResult::Redeem(Ok(paid_until)) => {
                    // Lives on `UserSettingsState` now (Account tab, owner
                    // ruling 2026-09-23) — `st` above stays untouched on this
                    // arm's path, same reasoning the `Nick`/`Logout` arms
                    // already give for reaching `chart` directly.
                    chart.panel_app.user_settings_state.redeem_status = RedeemStatus::Redeemed {
                        paid_until: paid_until.clone(),
                    };
                    // The tier just changed; the cached entitlements answer is
                    // now wrong. Ask again on the next frame rather than
                    // waiting out a TTL the user did not agree to.
                    self.inner.borrow_mut().next_entitlements_ms = 0.0;
                    // The ladder's `paid` class changed too — same
                    // `invalidate_me` edge a saved nick already triggers
                    // (owner ask: "refresh the tier/role line ...
                    // immediately"), so `account_classes`/`account_roles`
                    // re-fetch instead of waiting out the passive cadence.
                    chart.panel_app.community_state.forum2.invalidate_me();
                }
                FlowResult::Redeem(Err(message)) => {
                    chart.panel_app.user_settings_state.redeem_status = RedeemStatus::Failed { message };
                }
                FlowResult::Nick(Ok(effective_label)) => {
                    // Persist the new label into the stored token so the
                    // chrome's `account_label()` — and every future
                    // session's boot read — reflects it, not just this
                    // frame's UI state.
                    if let Some(t) = self.inner.borrow_mut().token.as_mut() {
                        t.display_name = effective_label.clone();
                    }
                    if let Some(t) = self.inner.borrow().token.clone() {
                        save_token(&t);
                    }
                    // The account overlay's own "SIGNED IN AS" line is a
                    // second reader of the same fact — keep it from showing
                    // a name the person just changed elsewhere.
                    if let CloudLoginPhase::SignedIn { provider, .. } = st.phase.clone() {
                        st.phase = CloudLoginPhase::SignedIn {
                            display_name: effective_label.clone(),
                            provider,
                        };
                    }
                    let uss = &mut chart.panel_app.user_settings_state;
                    uss.nick_status = mlc_app_layout::ui::modal_settings::NickStatus::Saved {
                        effective_label: effective_label.clone(),
                    };
                    uss.nick_edit_mode = false;
                    uss.nick_edit_focused = false;
                    uss.account_label = effective_label.clone();
                    // `st`/`uss` are both done being used above — safe to
                    // take `&mut chart` again here.
                    // A saved nick invalidates the `Me` cache the forum/DM
                    // "choose a nick" gate reads (owner mandate
                    // 2026-09-09) — without this, `caller_nick` would keep
                    // answering "none" until the next passive refresh.
                    chart.panel_app.community_state.forum2.invalidate_me();
                    chart.push_system_event(
                        mlc_core::SystemEventKind::Nick,
                        "Nickname saved",
                        format!("Now shown as {effective_label}."),
                        false,
                    );
                }
                FlowResult::Nick(Err(message)) => {
                    chart.panel_app.user_settings_state.nick_status =
                        mlc_app_layout::ui::modal_settings::NickStatus::Failed { message: message.clone() };
                    chart.push_system_event(mlc_core::SystemEventKind::Nick, "Nickname not saved", message, false);
                }
                FlowResult::ForumThreads { append, result } => {
                    let ov = &mut chart.panel_app.cloud_overlay_state;
                    ov.forum_list_refreshing = false;
                    match result {
                        Ok((threads, next_before)) => {
                            let mapped: Vec<CloudForumThreadSummary> = threads.into_iter().map(map_forum_summary).collect();
                            if append {
                                if let ForumListState::Loaded { threads: existing, next_before: nb, loading_more } =
                                    &mut ov.forum_list
                                {
                                    existing.extend(mapped);
                                    *nb = next_before;
                                    *loading_more = false;
                                } else {
                                    ov.forum_list = ForumListState::Loaded { threads: mapped, next_before, loading_more: false };
                                }
                            } else {
                                ov.forum_list = ForumListState::Loaded { threads: mapped, next_before, loading_more: false };
                                ov.mark_forum_list_fetched();
                            }
                        }
                        Err(e) => {
                            if append {
                                if let ForumListState::Loaded { loading_more, .. } = &mut ov.forum_list {
                                    *loading_more = false;
                                }
                                // Load-more failure: the existing page stays
                                // up and usable — no dedicated UI slot for
                                // this, so it just goes to the console.
                                web_sys::console::warn_1(&format!("[forum] load more failed: {}", e).into());
                            } else if matches!(ov.forum_list, ForumListState::Loaded { .. }) {
                                // A background due-refresh failed (the
                                // initial-load path always flips `Loading`
                                // before firing, so landing here with
                                // `Loaded` means this was the quiet one) —
                                // the existing page stays up, same tolerance
                                // load-more failures get above.
                                web_sys::console::warn_1(&format!("[forum] list refresh failed: {}", e).into());
                            } else {
                                ov.forum_list = ForumListState::Error { message: e };
                            }
                        }
                    }
                    self.inner.borrow_mut().forum_in_flight = false;
                }
                FlowResult::ForumThreadOpened(result) => {
                    let ov = &mut chart.panel_app.cloud_overlay_state;
                    ov.forum_thread_refreshing = false;
                    match result {
                        Ok(resp) => {
                            ov.forum_view = ForumSubView::Thread {
                                thread: Some(map_forum_thread(resp)),
                                loading: false,
                                error: None,
                                posting_reply: false,
                                reply_error: None,
                            };
                            ov.mark_forum_thread_fetched();
                        }
                        Err(e) => if matches!(&ov.forum_view, ForumSubView::Thread { thread: Some(_), loading: false, .. }) {
                            // A background due-refresh failed — the
                            // initial-load path always sets `thread: None,
                            // loading: true` before firing, so landing here
                            // with a thread already showing means this was
                            // the quiet one; it stays on screen as-is.
                            web_sys::console::warn_1(&format!("[forum] thread refresh failed: {}", e).into());
                        } else {
                            ov.forum_view = ForumSubView::Thread {
                                thread: None,
                                loading: false,
                                error: Some(e),
                                posting_reply: false,
                                reply_error: None,
                            };
                        },
                    }
                    self.inner.borrow_mut().forum_in_flight = false;
                }
                FlowResult::ForumThreadCreated(result) => {
                    let ov = &mut chart.panel_app.cloud_overlay_state;
                    match result {
                        Ok(resp) => {
                            ov.forum_view = ForumSubView::Thread {
                                thread: Some(map_forum_thread(resp)),
                                loading: false,
                                error: None,
                                posting_reply: false,
                                reply_error: None,
                            };
                            ov.mark_forum_thread_fetched();
                            // A new thread now exists — invalidate the
                            // cached list page so Back re-fetches rather
                            // than showing a stale page missing it.
                            ov.invalidate_forum_list();
                            // Compose form is gone — clear its fields so a
                            // later "New thread" does not resurrect this
                            // post's text.
                            let mut coord = chart.input_coordinator.borrow_mut();
                            let tf = coord.text_fields_mut();
                            let title_wid = uzor::WidgetId::from(mlc_app::text_input::FORUM_TITLE);
                            let body_wid = uzor::WidgetId::from(mlc_app::text_input::FORUM_BODY);
                            if !tf.text(&title_wid).is_empty() {
                                tf.clear(&title_wid);
                            }
                            if !tf.text(&body_wid).is_empty() {
                                tf.clear(&body_wid);
                            }
                        }
                        Err(e) => {
                            if let ForumSubView::Compose { posting, error } = &mut ov.forum_view {
                                *posting = false;
                                *error = Some(e);
                            }
                        }
                    }
                    self.inner.borrow_mut().forum_in_flight = false;
                }
                FlowResult::ForumThreadRefreshed(result) => {
                    let ov = &mut chart.panel_app.cloud_overlay_state;
                    let was_reply = std::mem::take(&mut ov.forum_reply_pending);
                    match result {
                        Ok(resp) => {
                            ov.forum_view = ForumSubView::Thread {
                                thread: Some(map_forum_thread(resp)),
                                loading: false,
                                error: None,
                                posting_reply: false,
                                reply_error: None,
                            };
                            ov.mark_forum_thread_fetched();
                            // The reply (or delete) landed — the list's
                            // `reply_count`/`last_reply_at` for this thread
                            // are now stale.
                            ov.invalidate_forum_list();
                            if was_reply {
                                let mut coord = chart.input_coordinator.borrow_mut();
                                let tf = coord.text_fields_mut();
                                let wid = uzor::WidgetId::from(mlc_app::text_input::FORUM_REPLY);
                                if !tf.text(&wid).is_empty() {
                                    tf.clear(&wid);
                                }
                            }
                        }
                        Err(e) => {
                            if let ForumSubView::Thread { posting_reply, reply_error, .. } = &mut ov.forum_view {
                                *posting_reply = false;
                                *reply_error = Some(e);
                            }
                        }
                    }
                    self.inner.borrow_mut().forum_in_flight = false;
                }
                FlowResult::ForumThreadDeleted(result) => {
                    let ov = &mut chart.panel_app.cloud_overlay_state;
                    ov.forum_reply_pending = false;
                    ov.forum_thread_fetched_at = None;
                    ov.forum_thread_refreshing = false;
                    match result {
                        Ok((threads, next_before)) => {
                            ov.forum_view = ForumSubView::List;
                            ov.forum_list = ForumListState::Loaded {
                                threads: threads.into_iter().map(map_forum_summary).collect(),
                                next_before,
                                loading_more: false,
                            };
                            ov.mark_forum_list_fetched();
                        }
                        Err(e) => {
                            if let ForumSubView::Thread { reply_error, .. } = &mut ov.forum_view {
                                *reply_error = Some(format!("Delete failed: {}", e));
                            }
                        }
                    }
                    self.inner.borrow_mut().forum_in_flight = false;
                }
                FlowResult::Forum2Boards(result) => {
                    let ov = &mut chart.panel_app.community_state;
                    ov.forum2.boards_refreshing = false;
                    match result {
                        Ok(boards) => {
                            let caller_classes = ov.forum2.caller_classes.clone();
                            ov.forum2.boards = boards
                                .into_iter()
                                .map(|b| map_forum_board(b, &caller_classes))
                                .collect();
                            ov.forum2.mark_boards_fetched();
                        }
                        Err(e) => {
                            if ov.forum2.boards.is_empty() {
                                ov.forum2.last_error = Some(e);
                            } else {
                                web_sys::console::warn_1(&format!("[forum2] boards refresh failed: {}", e).into());
                            }
                        }
                    }
                    self.inner.borrow_mut().forum2_in_flight = false;
                }
                FlowResult::Forum2BoardThreads { append, result } => {
                    let ov = &mut chart.panel_app.community_state;
                    ov.forum2.board_threads_refreshing = false;
                    match result {
                        Ok((threads, next_before)) => {
                            let mapped: Vec<ForumThreadV2Ui> =
                                threads.into_iter().map(map_forum_thread_v2_summary).collect();
                            if let ForumV2View::BoardThreads { threads: existing, next_before: nb, .. } =
                                &mut ov.forum2.view
                            {
                                if append {
                                    existing.extend(mapped);
                                } else {
                                    *existing = mapped;
                                }
                                *nb = next_before;
                                ov.forum2.mark_board_threads_fetched();
                            }
                            // Else: the view moved on before this landed
                            // (back to boards, opened a thread, ...) — the
                            // page is dropped, same tolerance v1 gives an
                            // interrupted fetch.
                        }
                        Err(e) => {
                            web_sys::console::warn_1(&format!("[forum2] board threads failed: {}", e).into());
                            ov.forum2.last_error = Some(e);
                        }
                    }
                    self.inner.borrow_mut().forum2_in_flight = false;
                }
                FlowResult::Forum2Thread(result) => {
                    let ov = &mut chart.panel_app.community_state;
                    ov.forum2.thread_refreshing = false;
                    match result {
                        Ok(resp) => {
                            let detail = map_forum_thread_v2_detail(&resp);
                            let id = detail.id;
                            let replies_tree: Vec<ForumReplyV2Ui> =
                                resp.replies.into_iter().map(map_forum_reply_node).collect();
                            if let ForumV2View::Thread { id: existing_id, thread, replies_tree: rt, .. } =
                                &mut ov.forum2.view
                            {
                                if *existing_id == id {
                                    *thread = Some(detail);
                                    *rt = replies_tree;
                                    ov.forum2.mark_thread_fetched();
                                }
                                // Else: landed for a thread the caller
                                // already navigated away from — dropped.
                            }
                        }
                        Err(e) => {
                            ov.forum2.last_error = Some(e);
                        }
                    }
                    self.inner.borrow_mut().forum2_in_flight = false;
                }
                FlowResult::Forum2ThreadAfterWrite(result) => {
                    let ov = &mut chart.panel_app.community_state;
                    match result {
                        Ok(resp) => {
                            let detail = map_forum_thread_v2_detail(&resp);
                            let id = detail.id;
                            let replies_tree: Vec<ForumReplyV2Ui> =
                                resp.replies.into_iter().map(map_forum_reply_node).collect();
                            let reply_to = match &ov.forum2.view {
                                ForumV2View::Thread { id: existing_id, reply_to, .. } if *existing_id == id => {
                                    *reply_to
                                }
                                _ => None,
                            };
                            ov.forum2.view = ForumV2View::Thread {
                                id,
                                thread: Some(detail),
                                replies_tree,
                                reply_to,
                            };
                            ov.forum2.mark_thread_fetched();
                            // A write landed — the board's own thread list
                            // (`reply_count`/`last_reply_at`, or a brand new
                            // thread) is now stale.
                            ov.forum2.invalidate_board_threads();
                            // Compose/reply forms are gone — clear the
                            // shared fields the same way v1's own
                            // `ForumThreadCreated`/`ForumThreadRefreshed`
                            // arms do (an empty field clears to empty, so
                            // this is harmless when the write was a
                            // `Moderate`/`DeleteReply` rather than a text
                            // submission).
                            let mut coord = chart.input_coordinator.borrow_mut();
                            let tf = coord.text_fields_mut();
                            for field in [mlc_app::text_input::FORUM_TITLE, mlc_app::text_input::FORUM_BODY, mlc_app::text_input::FORUM_REPLY] {
                                let wid = uzor::WidgetId::from(field);
                                if !tf.text(&wid).is_empty() {
                                    tf.clear(&wid);
                                }
                            }
                        }
                        Err(e) if e == mlc_cloud::NICK_REQUIRED_SENTINEL => {
                            // Caller lost the nick between the proactive
                            // `nick_required()` gate and this write landing
                            // (owner mandate 2026-09-09) — re-ask `Me`
                            // rather than show the sentinel; the
                            // composer's own gate picks the prompt back up
                            // from `caller_nick` on the next frame.
                            ov.forum2.invalidate_me();
                        }
                        Err(e) => {
                            ov.forum2.last_error = Some(e.clone());
                            chart.push_system_event(mlc_core::SystemEventKind::Info, "Forum post failed", e, true);
                        }
                    }
                    self.inner.borrow_mut().forum2_in_flight = false;
                }
                FlowResult::Forum2Search { q, result } => {
                    let ov = &mut chart.panel_app.community_state;
                    // The inline dropdown (owner review 2026-09-09, item d)
                    // and the full-page "More results" navigation share this
                    // one request/result wire — `quick_search_pending` says
                    // which landing this particular answer gets.
                    let quick = std::mem::take(&mut ov.forum2.quick_search_pending);
                    match result {
                        Ok(hits) => {
                            let mapped: Vec<ForumSearchHitUi> = hits.into_iter().map(map_forum_search_hit).collect();
                            if quick {
                                ov.forum2.quick_search_query = Some(q);
                                ov.forum2.quick_search_results = mapped;
                            } else {
                                ov.forum2.view = ForumV2View::Search { q, hits: mapped };
                            }
                        }
                        Err(e) => {
                            ov.forum2.last_error = Some(e);
                        }
                    }
                    self.inner.borrow_mut().forum2_in_flight = false;
                }
                FlowResult::Forum2Me(result) => {
                    match result {
                        Ok(me) => {
                            // Account tab's own mirrors of the same fetch —
                            // see `UserSettingsState::account_classes`'s
                            // own doc for why this crate carries a second
                            // copy instead of reading `CommunityState`
                            // directly. Cloned before the `ov` block below
                            // moves `me.classes`/`me.roles` into `forum2`.
                            chart.panel_app.user_settings_state.account_classes = me.classes.clone();
                            chart.panel_app.user_settings_state.account_roles = me.roles.clone();

                            let ov = &mut chart.panel_app.community_state;
                            ov.forum2.caller_roles = me.roles;
                            // Recompute `can_write` on whatever boards are
                            // already loaded — a `Me` answer arriving after
                            // `Forum2Boards` must not wait for a second
                            // boards fetch to reflect it.
                            for b in ov.forum2.boards.iter_mut() {
                                b.can_write = forum2_can_write(&b.write_class, &me.classes);
                            }
                            ov.forum2.caller_classes = me.classes;
                            ov.forum2.caller_nick = me.nick;
                            ov.forum2.mark_me_fetched();
                        }
                        Err(e) => {
                            web_sys::console::warn_1(&format!("[forum2] me() failed: {}", e).into());
                            chart.panel_app.community_state.forum2.last_error = Some(e);
                        }
                    }
                    chart.panel_app.community_state.forum2.me_refreshing = false;
                    self.inner.borrow_mut().forum2_in_flight = false;
                }
                FlowResult::DmIdentityPublished(result) => {
                    match result {
                        Ok(fingerprint) => {
                            chart.panel_app.community_state.dm.identity_fingerprint = Some(fingerprint);
                        }
                        Err(e) => {
                            // Best-effort background attempt — the next
                            // frame's check retries; nothing user-facing to
                            // show for a publish nobody asked for directly.
                            web_sys::console::warn_1(&format!("[dm] identity publish failed: {}", e).into());
                        }
                    }
                    self.inner.borrow_mut().dm_in_flight = false;
                }
                FlowResult::DmConversations(result) => {
                    let ov = &mut chart.panel_app.community_state.dm;
                    ov.conversations_refreshing = false;
                    match result {
                        Ok(conversations) => {
                            let mut backfill_targets = Vec::with_capacity(conversations.len());
                            let mapped: Vec<DmConversationUi> = conversations
                                .into_iter()
                                .map(|c| {
                                    backfill_targets.push((c.id, c.peer.label.clone()));
                                    DmConversationUi {
                                        id: c.id,
                                        peer_label: c.peer.label,
                                        peer_fingerprint: c.peer.fingerprint,
                                        last_message_at: c.last_message_at,
                                        unread_count: c.unread,
                                    }
                                })
                                .collect();
                            ov.conversations = mapped;
                            ov.mark_conversations_fetched();
                            let mut inner = self.inner.borrow_mut();
                            for (id, label) in backfill_targets {
                                if !inner.dm_peer_keys.contains_key(&id) {
                                    inner.dm_backfill_queue.push_back((id, label));
                                }
                            }
                        }
                        Err(e) => {
                            web_sys::console::warn_1(&format!("[dm] conversations fetch failed: {}", e).into());
                            ov.last_error = Some(e);
                        }
                    }
                    self.inner.borrow_mut().dm_in_flight = false;
                }
                FlowResult::DmConversationOpened(result) => {
                    let ov = &mut chart.panel_app.community_state.dm;
                    match result {
                        Ok((conversation, peer_public_key)) => {
                            self.inner.borrow_mut().dm_peer_keys.insert(conversation.id, peer_public_key);
                            let row = DmConversationUi {
                                id: conversation.id,
                                peer_label: conversation.peer.label,
                                peer_fingerprint: conversation.peer.fingerprint,
                                last_message_at: conversation.last_message_at,
                                unread_count: conversation.unread,
                            };
                            if let Some(existing) = ov.conversations.iter_mut().find(|c| c.id == row.id) {
                                *existing = row;
                            } else {
                                ov.conversations.push(row);
                            }
                            ov.open_conversation(conversation.id);
                        }
                        Err(e) if e == mlc_cloud::NICK_REQUIRED_SENTINEL => {
                            // Same "nick_required" gate as the forum
                            // composer (owner mandate 2026-09-09) —
                            // `caller_nick` is a shared account-level fact,
                            // so a Me refetch there re-drives the DM
                            // composer's own gate too.
                            chart.panel_app.community_state.forum2.invalidate_me();
                        }
                        Err(e) => {
                            ov.last_error = Some(e);
                        }
                    }
                    self.inner.borrow_mut().dm_in_flight = false;
                }
                FlowResult::DmBackfillPeerKey { conversation_id, result } => {
                    match result {
                        Ok(identity) => {
                            if let Some(key) = dm_decode_public_key(&identity.public_key_b64) {
                                self.inner.borrow_mut().dm_peer_keys.insert(conversation_id, key);
                                let dm = &mut chart.panel_app.community_state.dm;
                                // Owner review 2026-09-23, `forum-messenger-
                                // realtime.md` item 2: this cache used to
                                // overwrite silently — a peer's key rotating
                                // made every message sealed under the old
                                // key permanently `Undecryptable` with no
                                // explanation. `Some(old) != Some(new)` only
                                // (never fires on the FIRST ever
                                // fingerprint for this row); read before the
                                // mutable row lookup so the two lookups
                                // never overlap borrows.
                                let old_fingerprint = dm
                                    .conversations
                                    .iter()
                                    .find(|c| c.id == conversation_id)
                                    .and_then(|c| c.peer_fingerprint.clone());
                                if old_fingerprint.is_some()
                                    && old_fingerprint.as_deref() != Some(identity.fingerprint.as_str())
                                {
                                    dm.mark_peer_key_changed(conversation_id);
                                }
                                if let Some(row) = dm.conversations.iter_mut().find(|c| c.id == conversation_id) {
                                    row.peer_fingerprint = Some(identity.fingerprint);
                                }
                            }
                            // A malformed key just leaves this row without a
                            // fingerprint — Send/decrypt refuse locally until
                            // a later attempt succeeds.
                        }
                        Err(e) => {
                            web_sys::console::warn_1(&format!("[dm] peer-key backfill failed: {}", e).into());
                        }
                    }
                    self.inner.borrow_mut().dm_in_flight = false;
                }
                FlowResult::DmMessages { conversation_id, page, result } => {
                    let comm = &mut chart.panel_app.community_state;
                    comm.dm.messages_refreshing = false;
                    match result {
                        Ok(resp) => {
                            let crypto_ctx = {
                                let inner = self.inner.borrow();
                                match (&inner.dm_identity, inner.dm_peer_keys.get(&conversation_id)) {
                                    (Some(identity), Some(peer_public)) => {
                                        let key: [u8; 32] = *mlc_vault::dm::conversation_key(
                                            &identity.secret,
                                            peer_public,
                                            &conversation_id.to_string(),
                                        );
                                        Some((key, identity.public, *peer_public))
                                    }
                                    _ => None,
                                }
                            };
                            let mapped: Vec<DmMessageUi> = resp
                                .messages
                                .into_iter()
                                .map(|m| {
                                    let body = decrypt_dm_message_web(
                                        crypto_ctx.as_ref(),
                                        conversation_id,
                                        m.sender_is_me,
                                        &m.nonce_b64,
                                        &m.ciphertext_b64,
                                    );
                                    DmMessageUi {
                                        id: m.id,
                                        mine: m.sender_is_me,
                                        body,
                                        created_at: m.created_at,
                                        read_at: m.read_at,
                                    }
                                })
                                .collect();
                            let page_len = mapped.len();
                            let available = comm.dm.available;
                            let has_error = comm.dm.last_error.is_some();
                            let key_changed = comm.dm.has_peer_key_changed(conversation_id);
                            if let DmView::Conversation { id: existing_id, messages, has_older, .. } = &mut comm.dm.view
                            {
                                if *existing_id == conversation_id {
                                    // "Load older" must keep the viewport
                                    // anchored on whatever message was at
                                    // the top before the page landed — read
                                    // the exact height the page added off
                                    // `dm_overlay`'s own sizing function
                                    // before AND after the merge (owner
                                    // review 2026-09-23, `forum-messenger-
                                    // realtime.md` item 1).
                                    let is_before = matches!(page, DmMessagePage::Before(_));
                                    let old_h = is_before.then(|| {
                                        mlc_app_layout::layout::modals::dm_overlay::conversation_scroll_h(
                                            available, has_error, key_changed, *has_older, messages,
                                        )
                                    });
                                    merge_dm_messages(messages, mapped, page);
                                    *has_older = dm_has_older(page, page_len, *has_older);
                                    if let Some(old_h) = old_h {
                                        let new_h = mlc_app_layout::layout::modals::dm_overlay::conversation_scroll_h(
                                            available, has_error, key_changed, *has_older, messages,
                                        );
                                        comm.scroll.nudge((new_h - old_h).max(0.0));
                                    }
                                    comm.dm.mark_messages_fetched();
                                }
                                // Else: the caller navigated away before this
                                // landed — dropped, same tolerance an
                                // interrupted v1/forum2 fetch gets.
                            }
                        }
                        Err(e) => {
                            web_sys::console::warn_1(&format!("[dm] messages fetch failed: {}", e).into());
                            comm.dm.last_error = Some(e);
                        }
                    }
                    self.inner.borrow_mut().dm_in_flight = false;
                }
                FlowResult::DmSent { plaintext, result } => {
                    let ov = &mut chart.panel_app.community_state.dm;
                    match result {
                        Ok((id, created_at)) => {
                            if let DmView::Conversation { messages, .. } = &mut ov.view {
                                messages.push(DmMessageUi {
                                    id,
                                    mine: true,
                                    body: DmMessageBody::Plain(plaintext),
                                    created_at,
                                    read_at: None,
                                });
                            }
                            // Right after a send (owner ruling 2026-09-10,
                            // item 1): the list is stale the instant this
                            // conversation's `last_message_at` moves — drop
                            // the freshness stamp so the very next frame's
                            // passive fetch (paid-gated, not tab-gated —
                            // see the `Passive: conversations list` block
                            // below) picks it up immediately instead of
                            // waiting out `FORUM_REFRESH_MS`.
                            ov.invalidate_conversations();
                        }
                        Err(message) if message == mlc_cloud::NICK_REQUIRED_SENTINEL => {
                            // Same "nick_required" gate as the forum
                            // composer (owner mandate 2026-09-09).
                            chart.panel_app.community_state.forum2.invalidate_me();
                        }
                        Err(message) => {
                            ov.last_error = Some(message.clone());
                            chart.push_system_event(mlc_core::SystemEventKind::Info, "Message not sent", message, true);
                        }
                    }
                    self.inner.borrow_mut().dm_in_flight = false;
                }
                FlowResult::DmRead { conversation_id, result } => {
                    match result {
                        Ok(()) => {
                            if let Some(row) = chart
                                .panel_app
                                .community_state
                                .dm
                                .conversations
                                .iter_mut()
                                .find(|c| c.id == conversation_id)
                            {
                                row.unread_count = 0;
                            }
                        }
                        Err(e) => {
                            chart.panel_app.community_state.dm.last_error = Some(e);
                        }
                    }
                    self.inner.borrow_mut().dm_in_flight = false;
                }
                FlowResult::DmLookup { q, result } => {
                    let dm = &mut chart.panel_app.community_state.dm;
                    match result {
                        Ok(hits) => {
                            dm.lookup_query = Some(q);
                            dm.lookup_results = hits
                                .into_iter()
                                .map(|h| DmLookupHitUi { public_id: h.public_id, nick: h.nick, label: h.label })
                                .collect();
                        }
                        Err(e) => {
                            dm.last_error = Some(e);
                        }
                    }
                    self.inner.borrow_mut().dm_in_flight = false;
                }
                FlowResult::Logout => {
                    // Local sign-out regardless of the server answer. `st`
                    // (borrowed above) is not touched on this arm's path —
                    // `apply_signed_out` reaches `cloud_login_state` through
                    // `chart` directly, same reordering the sign-in arm
                    // already relies on.
                    self.apply_signed_out(chart, "You have been logged out.");
                }
            }
        }

        // ── Account nickname (User Settings modal, NOT the login overlay) ──
        // Deliberately independent of `cloud_login_state.is_open` (checked
        // further down, before the phase machine) — this row lives in the
        // always-available Settings modal, not this driver's login/account
        // overlay, so it must not share that gate.
        {
            let submitted = std::mem::take(&mut chart.panel_app.user_settings_state.nick_save_requested);
            if submitted {
                let nick = {
                    let coord = chart.input_coordinator.borrow();
                    coord
                        .text_fields()
                        .text(&uzor::WidgetId::from(mlc_app::text_input::ACCOUNT_NICK))
                        .trim()
                        .to_string()
                };
                if self.inner.borrow().token.is_none() {
                    chart.panel_app.user_settings_state.nick_status =
                        mlc_app_layout::ui::modal_settings::NickStatus::Failed {
                            message: "Sign in first".to_string(),
                        };
                    chart.push_system_event(mlc_core::SystemEventKind::Nick, "Nickname not saved", "Sign in first.", false);
                } else {
                    let inner = self.inner.clone();
                    let transport = self.inner.borrow().transport.snapshot();
                    spawn_local(async move {
                        let raw = transport.set_nick(&nick).await;
                        if let Err(e) = &raw {
                            mark_unauthorized(&inner, e);
                        }
                        let r = raw
                            .map(|resp| resp.effective_label)
                            .map_err(|e| mlc_cloud::nick_error_message(&e));
                        inner.borrow_mut().results.push_back(FlowResult::Nick(r));
                    });
                }
            }
        }

        // ── Gift/trial code redeem (User Settings modal, NOT the login
        // overlay) ─────────────────────────────────────────────────────
        // Owner ruling 2026-09-23: moved off `CloudLoginState` onto
        // `UserSettingsState`, same "deliberately independent of
        // `cloud_login_state.is_open`" reasoning the nickname block above
        // gives for its own field — this row lives in the always-available
        // Settings → Account tab, not this driver's login/account overlay.
        // The field keeps its text on a refusal — a refused code is usually
        // one wrong character, not a reason to retype ten.
        {
            let submitted = std::mem::take(&mut chart.panel_app.user_settings_state.redeem_requested);
            if submitted {
                let code = {
                    let coord = chart.input_coordinator.borrow();
                    coord
                        .text_fields()
                        .text(&uzor::WidgetId::from(mlc_app::text_input::REDEEM_CODE))
                        .trim()
                        .to_string()
                };
                if code.is_empty() {
                    chart.panel_app.user_settings_state.redeem_status = RedeemStatus::Failed {
                        message: "Enter a code first".to_string(),
                    };
                } else if self.inner.borrow().token.is_none() {
                    chart.panel_app.user_settings_state.redeem_status = RedeemStatus::Failed {
                        message: "Sign in first".to_string(),
                    };
                } else {
                    chart.panel_app.user_settings_state.redeem_status = RedeemStatus::Working;
                    let inner = self.inner.clone();
                    let transport = self.inner.borrow().transport.snapshot();
                    self.inner.borrow_mut().in_flight = true;
                    spawn_local(async move {
                        let raw = transport.redeem(&code).await;
                        if let Err(e) = &raw {
                            mark_unauthorized(&inner, e);
                        }
                        let r = raw
                            .map(|resp| resp.paid_until)
                            .map_err(|e| redeem_error_message(&e));
                        inner.borrow_mut().results.push_back(FlowResult::Redeem(r));
                    });
                }
            }
        }

        // ── Cloud overlay (News/Download): url mailbox + data fetch ──
        {
            let ov = &mut chart.panel_app.cloud_overlay_state;
            if ov.is_open {
                if let Some(url) = ov.open_url_requested.take() {
                    open_new_tab(&url);
                }
                let needs_data = matches!(ov.kind, CloudOverlayKind::News | CloudOverlayKind::Download)
                    && matches!(ov.data, CloudOverlayData::Empty);
                // News-only quiet re-fetch (recon item F1): `open()` already
                // struck `news_fetched_at` stale on every open, and the
                // passive `FORUM_REFRESH_MS` cadence keeps it stale while the
                // panel stays open — same idiom the Forum tab's own
                // `forum_list_due_for_refresh()` uses. Deliberately NOT
                // extended to `Download`: that overlay reads the release
                // manifest, which the desktop build's own in-process updater
                // already polls on its own cadence.
                let news_due_refresh = ov.kind == CloudOverlayKind::News
                    && matches!(ov.data, CloudOverlayData::News { .. })
                    && ov.news_due_for_refresh();
                if (needs_data || news_due_refresh) && !self.inner.borrow().in_flight {
                    if news_due_refresh {
                        ov.news_refreshing = true;
                    } else {
                        ov.data = CloudOverlayData::Loading;
                    }
                    let kind = ov.kind;
                    let inner = self.inner.clone();
                    let transport = self.inner.borrow().transport.snapshot();
                    self.inner.borrow_mut().in_flight = true;
                    spawn_local(async move {
                        // News reads the static blog feed; Download reads the
                        // release manifest.
                        match kind {
                            CloudOverlayKind::News => {
                                let r = transport
                                    .blog_posts()
                                    .await
                                    .map_err(|e| e.to_string());
                                inner.borrow_mut().results.push_back(FlowResult::News(r));
                            }
                            _ => {
                                let platform = detect_platform().await;
                                let latest = transport
                                    .updates_latest(platform)
                                    .await
                                    .map_err(|e| e.to_string());
                                let list = transport
                                    .updates_list("stable")
                                    .await
                                    .map_err(|e| e.to_string());
                                let r = match (latest, list) {
                                    (Ok(m), Ok(releases)) => Ok((platform, m, releases)),
                                    (Err(e), _) | (_, Err(e)) => Err(e),
                                };
                                inner.borrow_mut().results.push_back(FlowResult::Updates(r));
                            }
                        }
                    });
                }
            }
        }

        // ── Forum tab (paid-only): passive list fetch + explicit actions ──
        {
            let forum_active = chart.panel_app.community_state.is_open
                && chart.panel_app.community_state.tab == CommunityTab::Forum;
            let ov = &mut chart.panel_app.cloud_overlay_state;

            // Passive: fetch page one the moment the tab is looked at with
            // nothing loaded yet, OR reissue it quietly once
            // `forum_list_due_for_refresh()` says the cadence has elapsed on
            // an already-loaded page — same "Empty -> fire" idiom
            // News/Download use above, on its own `forum_in_flight` gate so
            // it can never block (or be blocked by) that unrelated flow. The
            // due-refresh case leaves the current page and scroll on screen
            // (no `Loading` flip) — only the `FlowResult::ForumThreads`
            // apply below swaps in the fresh page once it lands.
            let needs_initial_fetch = matches!(ov.forum_list, ForumListState::Empty);
            let needs_refresh =
                matches!(ov.forum_list, ForumListState::Loaded { .. }) && ov.forum_list_due_for_refresh();
            if forum_active && (needs_initial_fetch || needs_refresh) && !self.inner.borrow().forum_in_flight {
                if needs_initial_fetch {
                    ov.forum_list = ForumListState::Loading;
                } else {
                    ov.forum_list_refreshing = true;
                }
                self.inner.borrow_mut().forum_in_flight = true;
                let inner = self.inner.clone();
                let transport = self.inner.borrow().transport.snapshot();
                spawn_local(async move {
                    let raw = transport.forum_threads(None).await;
                    if let Err(e) = &raw {
                        mark_unauthorized(&inner, e);
                    }
                    let r = raw
                        .map(|resp| (resp.threads, resp.next_before))
                        .map_err(|e| forum_error_message(&e));
                    inner.borrow_mut().results.push_back(FlowResult::ForumThreads { append: false, result: r });
                });
            }

            // Passive: while a thread is open (fully loaded, not itself
            // loading/posting/erroring), reissue `LoadThread` every
            // `FORUM_REFRESH_MS` so another participant's reply shows up
            // without the caller leaving and reopening the thread. Fires
            // WITHOUT touching `forum_view` — unlike the click-triggered
            // load, a background refresh must not blank the thread the
            // caller is reading or reset its scroll; the reply box the
            // caller may be mid-typing lives in the text-field store,
            // untouched by any of this.
            if forum_active {
                if let ForumSubView::Thread { thread: Some(t), loading: false, posting_reply: false, .. } =
                    &ov.forum_view
                {
                    if ov.forum_thread_due_for_refresh() && !self.inner.borrow().forum_in_flight {
                        let id = t.id;
                        ov.forum_thread_refreshing = true;
                        self.inner.borrow_mut().forum_in_flight = true;
                        let inner = self.inner.clone();
                        let transport = self.inner.borrow().transport.snapshot();
                        spawn_local(async move {
                            let raw = transport.forum_thread(id).await;
                            if let Err(e) = &raw {
                                mark_unauthorized(&inner, e);
                            }
                            let r = raw.map_err(|e| forum_error_message(&e));
                            inner.borrow_mut().results.push_back(FlowResult::ForumThreadOpened(r));
                        });
                    }
                }
            }

            // Explicit user action — one-shot mailbox, same idiom as
            // `redeem_requested`/`update_install_requested`. Dropped (not
            // queued) if a forum request is already in flight; a doubled
            // click on Load-more/Reply/Delete while one is running is not
            // worth a request queue for.
            if let Some(req) = ov.forum_request.take() {
                if !self.inner.borrow().forum_in_flight {
                    self.inner.borrow_mut().forum_in_flight = true;
                    let inner = self.inner.clone();
                    let transport = self.inner.borrow().transport.snapshot();
                    spawn_local(async move {
                        match req {
                            ForumRequest::LoadMoreThreads { before } => {
                                let raw = transport.forum_threads(Some(&before)).await;
                                if let Err(e) = &raw {
                                    mark_unauthorized(&inner, e);
                                }
                                let r = raw
                                    .map(|resp| (resp.threads, resp.next_before))
                                    .map_err(|e| forum_error_message(&e));
                                inner.borrow_mut().results.push_back(FlowResult::ForumThreads { append: true, result: r });
                            }
                            ForumRequest::LoadThread { id } => {
                                let raw = transport.forum_thread(id).await;
                                if let Err(e) = &raw {
                                    mark_unauthorized(&inner, e);
                                }
                                let r = raw.map_err(|e| forum_error_message(&e));
                                inner.borrow_mut().results.push_back(FlowResult::ForumThreadOpened(r));
                            }
                            ForumRequest::CreateThread { title, body } => {
                                let r = match transport.forum_create_thread(&title, &body).await {
                                    Ok(id) => {
                                        let raw = transport.forum_thread(id).await;
                                        if let Err(e) = &raw {
                                            mark_unauthorized(&inner, e);
                                        }
                                        raw.map_err(|e| forum_error_message(&e))
                                    }
                                    Err(e) => {
                                        mark_unauthorized(&inner, &e);
                                        Err(forum_error_message(&e))
                                    }
                                };
                                inner.borrow_mut().results.push_back(FlowResult::ForumThreadCreated(r));
                            }
                            ForumRequest::Reply { thread_id, body } => {
                                let r = match transport.forum_reply(thread_id, &body).await {
                                    Ok(_) => {
                                        let raw = transport.forum_thread(thread_id).await;
                                        if let Err(e) = &raw {
                                            mark_unauthorized(&inner, e);
                                        }
                                        raw.map_err(|e| forum_error_message(&e))
                                    }
                                    Err(e) => {
                                        mark_unauthorized(&inner, &e);
                                        Err(forum_error_message(&e))
                                    }
                                };
                                inner.borrow_mut().results.push_back(FlowResult::ForumThreadRefreshed(r));
                            }
                            ForumRequest::DeleteThread { id } => {
                                let r = match transport.forum_delete_thread(id).await {
                                    Ok(()) => {
                                        let raw = transport.forum_threads(None).await;
                                        if let Err(e) = &raw {
                                            mark_unauthorized(&inner, e);
                                        }
                                        raw.map(|resp| (resp.threads, resp.next_before))
                                            .map_err(|e| forum_error_message(&e))
                                    }
                                    Err(e) => {
                                        mark_unauthorized(&inner, &e);
                                        Err(forum_error_message(&e))
                                    }
                                };
                                inner.borrow_mut().results.push_back(FlowResult::ForumThreadDeleted(r));
                            }
                            ForumRequest::DeleteReply { id, thread_id } => {
                                let r = match transport.forum_delete_reply(id).await {
                                    Ok(()) => {
                                        let raw = transport.forum_thread(thread_id).await;
                                        if let Err(e) = &raw {
                                            mark_unauthorized(&inner, e);
                                        }
                                        raw.map_err(|e| forum_error_message(&e))
                                    }
                                    Err(e) => {
                                        mark_unauthorized(&inner, &e);
                                        Err(forum_error_message(&e))
                                    }
                                };
                                inner.borrow_mut().results.push_back(FlowResult::ForumThreadRefreshed(r));
                            }
                        }
                    });
                }
            }
        }

        // ── Forum v2 tab (public boards): passive loads + explicit actions ─
        {
            // User Settings' Account tab shows the same tier/role line off
            // this fetch (`UserSettingsState::account_classes`'s own doc) —
            // it must fire even when the Community modal itself is closed,
            // so the tab's line fills the moment it's looked at rather than
            // only after the Forum tab happens to have been opened once.
            let account_tab_visible = chart.panel_app.user_settings_state.is_open
                && chart.panel_app.user_settings_state.active_tab == UserSettingsTab::Account;

            let ov = &mut chart.panel_app.community_state;
            let forum2_active = ov.is_open && ov.tab == CommunityTab::Forum;

            // Passive: boards, the moment the tab is looked at with nothing
            // loaded, or once `boards_due_for_refresh()` says the cadence
            // elapsed — same "Empty -> fire" idiom the v1 Forum tab uses.
            let boards_needs_fetch = ov.forum2.boards.is_empty() && ov.forum2.boards_fetched_at.is_none();
            let boards_due = !ov.forum2.boards.is_empty() && ov.forum2.boards_due_for_refresh();
            if forum2_active && (boards_needs_fetch || boards_due) && !self.inner.borrow().forum2_in_flight {
                ov.forum2.boards_refreshing = true;
                self.inner.borrow_mut().forum2_in_flight = true;
                let inner = self.inner.clone();
                let transport = self.inner.borrow().transport.snapshot();
                spawn_local(async move {
                    let raw = transport.forum_boards().await;
                    if let Err(e) = &raw {
                        mark_unauthorized(&inner, e);
                    }
                    let r = raw.map_err(|e| forum_v2_error_message(&e));
                    inner.borrow_mut().results.push_back(FlowResult::Forum2Boards(r));
                });
            }

            // Passive: the caller's forum roles/classes, once per tab-open —
            // no refresh cadence (unlike boards/threads below): this is an
            // account-level fact that only changes on a sign-in/sign-out
            // edge, which already calls `forum2.invalidate_me()` explicitly
            // (`apply_signed_out`, the login-confirm arm further down). Gated
            // on the Community modal's Forum tab OR User Settings' Account
            // tab — either surface reading this fact is enough to fetch it.
            //
            // `self.is_signed_in()` gate (owner repro 2026-09-24): `/forum/me`
            // is the CALLER's own identity — an anonymous visitor has none to
            // ask for. Without this gate a never-signed-in visitor who opens
            // the Forum & Messages modal fires this authenticated-only
            // request anyway; the server's 401 (no token ever held) used to
            // land on `mark_unauthorized` and produce a spurious "Signed
            // out" toast on a session that was never signed in — and, since
            // a failed fetch never sets `me_fetched_at`, it refired every
            // frame the tab stayed open.
            if self.is_signed_in()
                && (forum2_active || account_tab_visible)
                && ov.forum2.me_fetched_at.is_none()
                && !self.inner.borrow().forum2_in_flight
            {
                ov.forum2.me_refreshing = true;
                self.inner.borrow_mut().forum2_in_flight = true;
                let inner = self.inner.clone();
                let transport = self.inner.borrow().transport.snapshot();
                spawn_local(async move {
                    let raw = transport.forum_me().await;
                    if let Err(e) = &raw {
                        mark_unauthorized(&inner, e);
                    }
                    let r = raw.map_err(|e| forum_v2_error_message(&e));
                    inner.borrow_mut().results.push_back(FlowResult::Forum2Me(r));
                });
            }

            // Passive: a board's threads, once its view is entered with
            // nothing loaded yet (a click on a board row already arms its
            // own `LoadBoard` request below, so this only fires for a view
            // that landed on `BoardThreads` some other way — e.g. restored
            // — with nothing loaded).
            if forum2_active && !self.inner.borrow().forum2_in_flight {
                let board_threads_stale = ov.forum2.board_threads_fetched_at.is_none();
                let board_threads_due = ov.forum2.board_threads_due_for_refresh();
                let due = match &ov.forum2.view {
                    ForumV2View::BoardThreads { slug, threads, .. }
                        if (threads.is_empty() && board_threads_stale) || board_threads_due =>
                    {
                        Some(slug.clone())
                    }
                    _ => None,
                };
                if let Some(slug) = due {
                    ov.forum2.board_threads_refreshing = true;
                    self.inner.borrow_mut().forum2_in_flight = true;
                    let inner = self.inner.clone();
                    let transport = self.inner.borrow().transport.snapshot();
                    spawn_local(async move {
                        let raw = transport.forum_board_threads(&slug, None).await;
                        if let Err(e) = &raw {
                            mark_unauthorized(&inner, e);
                        }
                        let r = raw
                            .map(|resp| (resp.threads, resp.next_before))
                            .map_err(|e| forum_v2_error_message(&e));
                        inner.borrow_mut().results.push_back(FlowResult::Forum2BoardThreads { append: false, result: r });
                    });
                }
            }

            // Passive: the open thread, once `thread_due_for_refresh()`
            // says the cadence elapsed — same idiom as v1's own thread
            // refresh, silent (no view flip) so it never disturbs scroll or
            // an in-progress reply draft.
            if forum2_active && !self.inner.borrow().forum2_in_flight {
                if let ForumV2View::Thread { id, thread: Some(_), .. } = &ov.forum2.view {
                    if ov.forum2.thread_due_for_refresh() {
                        let id = *id;
                        ov.forum2.thread_refreshing = true;
                        self.inner.borrow_mut().forum2_in_flight = true;
                        let inner = self.inner.clone();
                        let transport = self.inner.borrow().transport.snapshot();
                        spawn_local(async move {
                            let raw = transport.forum_thread_v2(id).await;
                            if let Err(e) = &raw {
                                mark_unauthorized(&inner, e);
                            }
                            let r = raw.map_err(|e| forum_v2_error_message(&e));
                            inner.borrow_mut().results.push_back(FlowResult::Forum2Thread(r));
                        });
                    }
                }
            }

            // Explicit user action — one-shot mailbox, dropped (not queued)
            // if a forum-v2 request is already in flight, same tolerance
            // the v1 Forum tab gives a doubled click.
            if let Some(req) = ov.forum2.request.take() {
                if !self.inner.borrow().forum2_in_flight {
                    // Optimistic view transitions before firing — mirrors
                    // v1's own `Loading`/`thread: None` flips, and gives
                    // `Forum2Thread`/`Forum2BoardThreads` above a view to
                    // match against the instant the answer lands.
                    match &req {
                        ForumV2Request::LoadBoard { slug, before } if before.is_none() => {
                            ov.forum2.view = ForumV2View::BoardThreads {
                                slug: slug.clone(),
                                threads: Vec::new(),
                                next_before: None,
                            };
                        }
                        ForumV2Request::OpenThread { id } => {
                            ov.forum2.view = ForumV2View::Thread {
                                id: *id,
                                thread: None,
                                replies_tree: Vec::new(),
                                reply_to: None,
                            };
                        }
                        _ => {}
                    }
                    // `DeleteReply` carries no thread_id of its own — read
                    // it from the view it was clicked from.
                    let delete_reply_thread_id = if matches!(&req, ForumV2Request::DeleteReply { .. }) {
                        match &ov.forum2.view {
                            ForumV2View::Thread { id, .. } => Some(*id),
                            _ => None,
                        }
                    } else {
                        None
                    };
                    self.inner.borrow_mut().forum2_in_flight = true;
                    let inner = self.inner.clone();
                    let transport = self.inner.borrow().transport.snapshot();
                    spawn_local(async move {
                        match req {
                            ForumV2Request::LoadBoards => {
                                let raw = transport.forum_boards().await;
                                if let Err(e) = &raw {
                                    mark_unauthorized(&inner, e);
                                }
                                let r = raw.map_err(|e| forum_v2_error_message(&e));
                                inner.borrow_mut().results.push_back(FlowResult::Forum2Boards(r));
                            }
                            ForumV2Request::LoadBoard { slug, before } => {
                                let raw = transport.forum_board_threads(&slug, before.as_deref()).await;
                                if let Err(e) = &raw {
                                    mark_unauthorized(&inner, e);
                                }
                                let r = raw
                                    .map(|resp| (resp.threads, resp.next_before))
                                    .map_err(|e| forum_v2_error_message(&e));
                                inner.borrow_mut().results.push_back(
                                    FlowResult::Forum2BoardThreads { append: before.is_some(), result: r });
                            }
                            ForumV2Request::OpenThread { id } => {
                                let raw = transport.forum_thread_v2(id).await;
                                if let Err(e) = &raw {
                                    mark_unauthorized(&inner, e);
                                }
                                let r = raw.map_err(|e| forum_v2_error_message(&e));
                                inner.borrow_mut().results.push_back(FlowResult::Forum2Thread(r));
                            }
                            ForumV2Request::CreateThread { slug, title, body } => {
                                let r = match transport.forum_create_thread_v2(&slug, &title, &body).await {
                                    Ok(id) => {
                                        let raw = transport.forum_thread_v2(id).await;
                                        if let Err(e) = &raw {
                                            mark_unauthorized(&inner, e);
                                        }
                                        raw.map_err(|e| forum_v2_error_message(&e))
                                    }
                                    Err(e) => {
                                        mark_unauthorized(&inner, &e);
                                        Err(forum_v2_error_message(&e))
                                    }
                                };
                                inner.borrow_mut().results.push_back(FlowResult::Forum2ThreadAfterWrite(r));
                            }
                            ForumV2Request::Reply { thread_id, body, parent_reply_id } => {
                                let r = match transport.forum_reply_v2(thread_id, &body, parent_reply_id).await {
                                    Ok(_) => {
                                        let raw = transport.forum_thread_v2(thread_id).await;
                                        if let Err(e) = &raw {
                                            mark_unauthorized(&inner, e);
                                        }
                                        raw.map_err(|e| forum_v2_error_message(&e))
                                    }
                                    Err(e) => {
                                        mark_unauthorized(&inner, &e);
                                        Err(forum_v2_error_message(&e))
                                    }
                                };
                                inner.borrow_mut().results.push_back(FlowResult::Forum2ThreadAfterWrite(r));
                            }
                            ForumV2Request::DeleteReply { id } => {
                                let Some(thread_id) = delete_reply_thread_id else {
                                    inner.borrow_mut().results.push_back(FlowResult::Forum2ThreadAfterWrite(Err(
                                        "No thread is open to delete this reply from".to_string(),
                                    )));
                                    return;
                                };
                                let r = match transport.forum_delete_reply_v2(id).await {
                                    Ok(()) => {
                                        let raw = transport.forum_thread_v2(thread_id).await;
                                        if let Err(e) = &raw {
                                            mark_unauthorized(&inner, e);
                                        }
                                        raw.map_err(|e| forum_v2_error_message(&e))
                                    }
                                    Err(e) => {
                                        mark_unauthorized(&inner, &e);
                                        Err(forum_v2_error_message(&e))
                                    }
                                };
                                inner.borrow_mut().results.push_back(FlowResult::Forum2ThreadAfterWrite(r));
                            }
                            ForumV2Request::Search { q, board } => {
                                let raw = transport.forum_search(&q, board.as_deref()).await;
                                if let Err(e) = &raw {
                                    mark_unauthorized(&inner, e);
                                }
                                let r = raw.map_err(|e| forum_v2_error_message(&e));
                                inner.borrow_mut().results.push_back(FlowResult::Forum2Search { q, result: r });
                            }
                            ForumV2Request::Moderate { thread_id, action, board } => {
                                let r = match transport.forum_moderate(thread_id, &action, board.as_deref()).await {
                                    Ok(()) => {
                                        let raw = transport.forum_thread_v2(thread_id).await;
                                        if let Err(e) = &raw {
                                            mark_unauthorized(&inner, e);
                                        }
                                        raw.map_err(|e| forum_v2_error_message(&e))
                                    }
                                    Err(e) => {
                                        mark_unauthorized(&inner, &e);
                                        Err(forum_v2_error_message(&e))
                                    }
                                };
                                inner.borrow_mut().results.push_back(FlowResult::Forum2ThreadAfterWrite(r));
                            }
                        }
                    });
                }
            }
        }

        // ── Forum v2 inline quick-search (owner review 2026-09-09, item d
        // of the community-modal-and-forum-ladder arc) ──────────────────
        // Debounced (or Enter-fired, `text_action_forum_search` in
        // mlc-app) refetch for the header's compact search box — lands in
        // `ForumV2State::quick_search_*` (the dropdown), never `view`.
        // Shares `forum2_in_flight` with the boards/threads passive
        // fetches above: not worth a second flag for a box idle almost
        // all the time.
        {
            let due = chart.panel_app.community_state.forum2.quick_search_due();
            if due && !self.inner.borrow().forum2_in_flight {
                let q = {
                    let coord = chart.input_coordinator.borrow();
                    coord
                        .text_fields()
                        .text(&uzor::WidgetId::from(mlc_app::text_input::FORUM_SEARCH))
                        .trim()
                        .to_string()
                };
                let ov = &mut chart.panel_app.community_state.forum2;
                ov.quick_search_due_at = None;
                if q.is_empty() {
                    ov.quick_search_query = None;
                    ov.quick_search_results = Vec::new();
                } else {
                    ov.quick_search_pending = true;
                    ov.last_error = None;
                    self.inner.borrow_mut().forum2_in_flight = true;
                    let inner = self.inner.clone();
                    let transport = self.inner.borrow().transport.snapshot();
                    spawn_local(async move {
                        let raw = transport.forum_search(&q, None).await;
                        if let Err(e) = &raw {
                            mark_unauthorized(&inner, e);
                        }
                        let r = raw.map_err(|e| forum_v2_error_message(&e));
                        inner.borrow_mut().results.push_back(FlowResult::Forum2Search { q, result: r });
                    });
                }
            }
        }

        // The single ladder-class predicate the whole messenger surface
        // shares (owner ruling E-14, 2026-09-23: class and role, not
        // payment — `ForumV2State::messenger_available`'s own doc).
        // Deliberately NOT `cloud_overlay_state.forum_entitled`
        // (`entitlements_rx`/`Feature::Forum`), a payment-only signal that
        // used to gate this whole surface and silently no-op'd it for a
        // non-paid moderator/admin.
        let messenger_ok = chart.panel_app.community_state.forum2.messenger_available();

        // ── DM recipient-search dropdown (owner ruling 2026-09-09,
        // community-modal-and-forum-ladder arc, piece 2 item b) ──────────
        // Debounced `GET /api/dm/lookup?q=` for the "start a conversation"
        // field (`text_action_dm_peer` in mlc-app arms `DmState::
        // lookup_due_at` on every keystroke) — fired directly here, never
        // through `DmState::request`, same "debounce fires inline" idiom
        // the Forum v2 quick-search block above uses. Shares `dm_in_flight`
        // with every other DM action below: not worth a second flag for a
        // box idle almost all the time. Gated on `messenger_ok`, same as
        // every other fetch on this path.
        {
            let due = chart.panel_app.community_state.dm.lookup_due();
            if due && messenger_ok && !self.inner.borrow().dm_in_flight {
                let q = {
                    let coord = chart.input_coordinator.borrow();
                    coord
                        .text_fields()
                        .text(&uzor::WidgetId::from(mlc_app::text_input::DM_PEER))
                        .trim()
                        .to_string()
                };
                let dm = &mut chart.panel_app.community_state.dm;
                dm.lookup_due_at = None;
                if q.is_empty() {
                    dm.lookup_query = None;
                    dm.lookup_results = Vec::new();
                } else {
                    dm.last_error = None;
                    self.inner.borrow_mut().dm_in_flight = true;
                    let inner = self.inner.clone();
                    let transport = self.inner.borrow().transport.snapshot();
                    spawn_local(async move {
                        let raw = transport.dm_lookup(&q).await;
                        if let Err(e) = &raw {
                            mark_unauthorized(&inner, e);
                        }
                        let r = raw.map_err(|e| dm_error_message(&e));
                        inner.borrow_mut().results.push_back(FlowResult::DmLookup { q, result: r });
                    });
                }
            }
        }

        // ── Messages tab (paid, end-to-end encrypted) ────────────────────
        // Owner ruling 2026-09-09: paid web users hold the vault too — this
        // mirrors the native shell's own `drive_dm` (`mlc-app-shell-vello`'s
        // `main.rs`), just as one shell-owned driver instead of a background
        // updater thread + channel (every crypto op below is cheap
        // synchronous wasm-thread computation; every network call already
        // goes through this file's own `spawn_local` idiom).
        {
            let messages_active = chart.panel_app.community_state.is_open
                && chart.panel_app.community_state.tab == CommunityTab::Messages;

            let vault_key = chart.core.user().vault_key();

            match vault_key {
                None => {
                    chart.panel_app.community_state.dm.available = false;
                    chart.panel_app.community_state.dm.unavailable_reason =
                        if messenger_ok { DmUnavailableReason::VaultLocked } else { DmUnavailableReason::NotPaid };
                    // No `last_error` for a locked vault: the messenger's dock
                    // already says it once, with the Unlock button (owner
                    // 0.5.615: three notices for one fact).
                    let mut inner = self.inner.borrow_mut();
                    inner.dm_identity = None;
                    inner.dm_identity_key = None;
                    inner.dm_identity_publish_attempted_for = None;
                }
                Some(key) => {
                    if !messenger_ok {
                        chart.panel_app.community_state.dm.available = false;
                        chart.panel_app.community_state.dm.unavailable_reason = DmUnavailableReason::NotPaid;
                        if messages_active {
                            chart.panel_app.community_state.dm.last_error = Some(DM_UNPAID_MESSAGE.to_string());
                        }
                    } else {
                        chart.panel_app.community_state.dm.available = true;
                        chart.panel_app.community_state.dm.unavailable_reason = DmUnavailableReason::None;
                    }
                    let mut inner = self.inner.borrow_mut();
                    let needs_derive = inner.dm_identity.is_none() || inner.dm_identity_key != Some(key);
                    if needs_derive {
                        inner.dm_identity = Some(mlc_vault::dm::DmIdentity::from_vault_key(&key));
                        inner.dm_identity_key = Some(key);
                        inner.dm_identity_publish_attempted_for = None;
                    }
                }
            }

            // Keep already-fetched ciphertext decrypting in place as new key
            // material lands — see `redecrypt_open_conversation`'s own doc.
            // Runs regardless of `messages_active`/tab (cheap no-op once
            // nothing is left to retry), same "not gated on which tab is
            // open" principle the passive list fetch below applies.
            {
                let inner = self.inner.borrow();
                redecrypt_open_conversation(
                    &mut chart.panel_app.community_state.dm,
                    inner.dm_identity.as_ref(),
                    &inner.dm_peer_keys,
                );
            }

            // Passive: conversations list — independent of the vault and of
            // which tab is open (owner report 2026-09-10: a recipient never
            // saw an incoming DM until they searched the sender by nick,
            // because this fetch used to live inside `if messages_active`
            // below, gated on the Messages tab being the one on screen).
            // Runs whenever the account is paid, no matter which tab or
            // whether the community modal is even open — on sign-in (the
            // first frame `paid` goes true), on every community-modal-open
            // frame, and on the `FORUM_REFRESH_MS` (30s) background cadence
            // `conversations_due_for_refresh` already carries; a send also
            // drops the freshness stamp (`DmState::invalidate_conversations`,
            // `FlowResult::DmSent`'s own handling) so this fires again right
            // away. This is also what lets the envelope badge
            // (`stamp_active_right_toolbar_id`, gated only on
            // `messages_tab_visible`) light with the modal closed.
            let conv_needs = {
                let ov = &chart.panel_app.community_state.dm;
                messenger_ok && (ov.conversations_fetched_at.is_none() || ov.conversations_due_for_refresh())
            };
            if conv_needs && !self.inner.borrow().dm_in_flight {
                chart.panel_app.community_state.dm.conversations_refreshing = true;
                self.inner.borrow_mut().dm_in_flight = true;
                let inner = self.inner.clone();
                let transport = self.inner.borrow().transport.snapshot();
                spawn_local(async move {
                    let raw = transport.dm_conversations().await;
                    if let Err(e) = &raw {
                        mark_unauthorized(&inner, e);
                    }
                    let r = raw.map_err(|e| dm_error_message(&e));
                    inner.borrow_mut().results.push_back(FlowResult::DmConversations(r));
                });
            }

            if messages_active {
                // Auto-publish this profile's key once signed in + paid —
                // the ONLY publish path (owner ruling 2026-09-09: no
                // user-facing button). Fires at most once per unlock and
                // again after a sign-in edge (both clear
                // `dm_identity_publish_attempted_for` — see the vault-key
                // branch above and the login-confirm arm below).
                if !self.inner.borrow().dm_in_flight {
                    let should_publish = {
                        let inner = self.inner.borrow();
                        match &inner.dm_identity {
                            Some(identity) if messenger_ok => {
                                let my_fingerprint = mlc_vault::dm::fingerprint(&identity.public);
                                let already_confirmed = chart
                                    .panel_app
                                    .community_state
                                    .dm
                                    .identity_fingerprint
                                    .as_deref()
                                    == Some(my_fingerprint.as_str());
                                let already_attempted =
                                    inner.dm_identity_publish_attempted_for == Some(identity.public);
                                (!already_confirmed && !already_attempted).then_some(identity.public)
                            }
                            _ => None,
                        }
                    };
                    if let Some(public) = should_publish {
                        self.inner.borrow_mut().dm_identity_publish_attempted_for = Some(public);
                        self.inner.borrow_mut().dm_in_flight = true;
                        let public_key_b64 = dm_encode(&public);
                        let inner = self.inner.clone();
                        let transport = self.inner.borrow().transport.snapshot();
                        spawn_local(async move {
                            let raw = transport.dm_put_identity(&public_key_b64).await;
                            if let Err(e) = &raw {
                                mark_unauthorized(&inner, e);
                            }
                            let r = raw.map_err(|e| dm_error_message(&e));
                            inner.borrow_mut().results.push_back(FlowResult::DmIdentityPublished(r));
                        });
                    }
                }

                // Passive: the open conversation's messages, on open or
                // due-refresh — silent (no view flip), same tolerance v1's
                // own thread refresh gives an in-progress draft.
                // `after = highest id held` once anything has landed
                // (`DmState::refresh_page`) — owner report F4 fix,
                // 2026-09-23: this used to always send `after: None`, which
                // pinned any conversation past one page to its oldest page
                // forever.
                if !self.inner.borrow().dm_in_flight {
                    let ov = &chart.panel_app.community_state.dm;
                    if let DmView::Conversation { id, .. } = &ov.view {
                        let id = *id;
                        let due = ov.messages_fetched_at.is_none() || ov.messages_due_for_refresh();
                        if due {
                            let page = ov.refresh_page().unwrap_or(DmMessagePage::Newest);
                            chart.panel_app.community_state.dm.messages_refreshing = true;
                            self.inner.borrow_mut().dm_in_flight = true;
                            let inner = self.inner.clone();
                            let transport = self.inner.borrow().transport.snapshot();
                            let (after, before) = match page {
                                DmMessagePage::Newest => (None, None),
                                DmMessagePage::After(a) => (Some(a), None),
                                DmMessagePage::Before(b) => (None, Some(b)),
                            };
                            spawn_local(async move {
                                let raw = transport.dm_messages(id, after, before, DM_MESSAGE_PAGE_LIMIT).await;
                                if let Err(e) = &raw {
                                    mark_unauthorized(&inner, e);
                                }
                                let r = raw.map_err(|e| dm_error_message(&e));
                                inner.borrow_mut().results.push_back(
                                    FlowResult::DmMessages { conversation_id: id, page, result: r });
                            });
                        }
                    }
                }

                // Explicit user action — dropped (not queued) if a DM
                // request is already in flight, same tolerance every other
                // tab in this driver gives a doubled click.
                if let Some(req) = chart.panel_app.community_state.dm.request.take() {
                    if !self.inner.borrow().dm_in_flight {
                        match req {
                            UiDmRequest::LoadConversations => {
                                chart.panel_app.community_state.dm.conversations_refreshing = true;
                                self.inner.borrow_mut().dm_in_flight = true;
                                let inner = self.inner.clone();
                                let transport = self.inner.borrow().transport.snapshot();
                                spawn_local(async move {
                                    let raw = transport.dm_conversations().await;
                                    if let Err(e) = &raw {
                                        mark_unauthorized(&inner, e);
                                    }
                                    let r = raw.map_err(|e| dm_error_message(&e));
                                    inner.borrow_mut().results.push_back(FlowResult::DmConversations(r));
                                });
                            }
                            UiDmRequest::Open { with_label } => {
                                self.inner.borrow_mut().dm_in_flight = true;
                                let inner = self.inner.clone();
                                let transport = self.inner.borrow().transport.snapshot();
                                spawn_local(async move {
                                    let result = match transport.dm_get_identity(&with_label).await {
                                        Err(e) => {
                                            mark_unauthorized(&inner, &e);
                                            Err(dm_error_message(&e))
                                        }
                                        Ok(identity) => match dm_decode_public_key(&identity.public_key_b64) {
                                            None => Err("That account's messaging key is malformed".to_string()),
                                            Some(peer_public_key) => {
                                                match transport.dm_open_conversation(&identity.public_id).await {
                                                    Err(e) => {
                                                        mark_unauthorized(&inner, &e);
                                                        Err(dm_error_message(&e))
                                                    }
                                                    Ok(conversation) => Ok((conversation, peer_public_key)),
                                                }
                                            }
                                        },
                                    };
                                    inner.borrow_mut().results.push_back(FlowResult::DmConversationOpened(result));
                                });
                            }
                            UiDmRequest::LoadMessages { id, page } => {
                                chart.panel_app.community_state.dm.messages_refreshing = true;
                                self.inner.borrow_mut().dm_in_flight = true;
                                let inner = self.inner.clone();
                                let transport = self.inner.borrow().transport.snapshot();
                                let (after, before) = match page {
                                    DmMessagePage::Newest => (None, None),
                                    DmMessagePage::After(a) => (Some(a), None),
                                    DmMessagePage::Before(b) => (None, Some(b)),
                                };
                                spawn_local(async move {
                                    let raw = transport.dm_messages(id, after, before, DM_MESSAGE_PAGE_LIMIT).await;
                                    if let Err(e) = &raw {
                                        mark_unauthorized(&inner, e);
                                    }
                                    let r = raw.map_err(|e| dm_error_message(&e));
                                    inner.borrow_mut().results.push_back(
                                        FlowResult::DmMessages { conversation_id: id, page, result: r });
                                });
                            }
                            UiDmRequest::Send { id, plaintext } => {
                                let sealed_payload: Result<(String, String), &'static str> = {
                                    let inner_borrow = self.inner.borrow();
                                    match (&inner_borrow.dm_identity, inner_borrow.dm_peer_keys.get(&id)) {
                                        (None, _) => Err("Unlock the vault to send messages"),
                                        (_, None) => Err("Open this conversation again to send"),
                                        (Some(identity), Some(peer_public)) => {
                                            let key: [u8; 32] = *mlc_vault::dm::conversation_key(
                                                &identity.secret,
                                                peer_public,
                                                &id.to_string(),
                                            );
                                            let my_fingerprint = mlc_vault::dm::fingerprint(&identity.public);
                                            let aad = dm_aad(id, &my_fingerprint);
                                            let sealed = mlc_vault::dm::seal_message(&key, plaintext.as_bytes(), &aad);
                                            Ok((dm_encode(&sealed.nonce), dm_encode(&sealed.ciphertext)))
                                        }
                                    }
                                };
                                match sealed_payload {
                                    Err(message) => {
                                        chart.panel_app.community_state.dm.last_error = Some(message.to_string());
                                    }
                                    Ok((nonce_b64, ciphertext_b64)) => {
                                        self.inner.borrow_mut().dm_in_flight = true;
                                        let inner = self.inner.clone();
                                        let transport = self.inner.borrow().transport.snapshot();
                                        spawn_local(async move {
                                            let raw = transport.dm_send(id, &nonce_b64, &ciphertext_b64).await;
                                            if let Err(e) = &raw {
                                                mark_unauthorized(&inner, e);
                                            }
                                            let r = raw
                                                .map(|resp| (resp.id, resp.created_at))
                                                .map_err(|e| dm_error_message(&e));
                                            inner.borrow_mut().results.push_back(FlowResult::DmSent { plaintext, result: r });
                                        });
                                    }
                                }
                            }
                            UiDmRequest::MarkRead { id, up_to } => {
                                self.inner.borrow_mut().dm_in_flight = true;
                                let inner = self.inner.clone();
                                let transport = self.inner.borrow().transport.snapshot();
                                spawn_local(async move {
                                    let raw = transport.dm_mark_read(id, up_to).await;
                                    if let Err(e) = &raw {
                                        mark_unauthorized(&inner, e);
                                    }
                                    let r = raw.map_err(|e| dm_error_message(&e));
                                    inner.borrow_mut().results.push_back(FlowResult::DmRead { conversation_id: id, result: r });
                                });
                            }
                        }
                    }
                }

                // Backfill: one peer-key lookup at a time for a LIST-sourced
                // conversation the `Open` flow never touched.
                if !self.inner.borrow().dm_in_flight {
                    let next = self.inner.borrow_mut().dm_backfill_queue.pop_front();
                    if let Some((conversation_id, label)) = next {
                        if self.inner.borrow().dm_peer_keys.contains_key(&conversation_id) {
                            // Already resolved by an earlier `Open` — drop.
                        } else {
                            self.inner.borrow_mut().dm_in_flight = true;
                            let inner = self.inner.clone();
                            let transport = self.inner.borrow().transport.snapshot();
                            spawn_local(async move {
                                let raw = transport.dm_get_identity(&label).await;
                                if let Err(e) = &raw {
                                    mark_unauthorized(&inner, e);
                                }
                                let r = raw.map_err(|e| dm_error_message(&e));
                                inner.borrow_mut().results.push_back(
                                    FlowResult::DmBackfillPeerKey { conversation_id, result: r });
                            });
                        }
                    }
                }
            }
        }

        let st = &mut chart.panel_app.cloud_login_state;

        // ── Open-edge: pick the initial phase from auth state ────────────
        {
            let mut inner = self.inner.borrow_mut();
            if st.is_open && !inner.was_open {
                if let Some(t) = inner.token.as_ref() {
                    st.phase = CloudLoginPhase::SignedIn {
                        display_name: t.display_name.clone(),
                        provider: t.provider.clone(),
                    };
                } else {
                    st.phase = CloudLoginPhase::RequestCode;
                }
            }
            inner.was_open = st.is_open;
        }

        if !st.is_open {
            return;
        }

        // ── One-shot UI flags ────────────────────────────────────────────
        // The sign-in trip goes to a popup, not a tab: this is the login
        // modal's own mailbox, and the user should come back to a modal that
        // is still open rather than to a tab telling them to close it.
        if let Some(url) = st.open_url_requested.take() {
            open_auth_popup(&url);
        }
        if st.sign_out_requested {
            st.sign_out_requested = false;
            let token = self.inner.borrow().token.as_ref().map(|t| t.token.clone());
            if let Some(_token) = token {
                let inner = self.inner.clone();
                let transport = self.inner.borrow().transport.snapshot();
                self.inner.borrow_mut().in_flight = true;
                spawn_local(async move {
                    // The server answer is deliberately ignored — local
                    // sign-out happens regardless.
                    let _ = transport.auth_logout().await;
                    inner.borrow_mut().results.push_back(FlowResult::Logout);
                });
            } else {
                // No token — nothing to revoke, just reset the view.
                st.phase = CloudLoginPhase::RequestCode;
            }
            return;
        }

        // ── Phase machine ────────────────────────────────────────────────
        let in_flight = self.inner.borrow().in_flight;
        if in_flight {
            return;
        }
        match &st.phase {
            CloudLoginPhase::RequestCode => {
                st.phase = CloudLoginPhase::CreatingCode;
                let inner = self.inner.clone();
                let transport = self.inner.borrow().transport.snapshot();
                self.inner.borrow_mut().in_flight = true;
                spawn_local(async move {
                    let r = transport
                        .device_code_create()
                        .await
                        .map(|resp| resp.code)
                        .map_err(|e| e.to_string());
                    inner.borrow_mut().results.push_back(FlowResult::Code(r));
                });
            }
            CloudLoginPhase::AwaitingConfirm { code } if now_ms >= self.inner.borrow().next_poll_ms => {
                let code = code.clone();
                let inner = self.inner.clone();
                let transport = self.inner.borrow().transport.snapshot();
                self.inner.borrow_mut().in_flight = true;
                spawn_local(async move {
                    let r = transport
                        .device_code_poll(&code)
                        .await
                        .map_err(|e| e.to_string());
                    inner.borrow_mut().results.push_back(FlowResult::Poll(r));
                });
            }
            _ => {}
        }
    }
}

// ── Forum wire → UI mapping ────────────────────────────────────────────
// `mlc-app-layout` cannot depend on `mlc-cloud` (the layout crate must not
// depend on the transport crate — same split `CloudNewsPost`/`BlogPost`
// already has), so the shell is where a wire row becomes the flat mirror
// the overlay renders.

fn map_forum_summary(s: ForumThreadSummary) -> CloudForumThreadSummary {
    CloudForumThreadSummary {
        id: s.id,
        title: s.title,
        author: s.author,
        author_is_me: s.author_is_me,
        created_at: s.created_at,
        last_reply_at: s.last_reply_at,
        reply_count: s.reply_count,
        excerpt: s.excerpt,
    }
}

fn map_forum_thread(resp: ForumThreadResponse) -> CloudForumThread {
    CloudForumThread {
        id: resp.thread.id,
        title: resp.thread.title,
        author: resp.thread.author,
        author_is_me: resp.thread.author_is_me,
        body: resp.thread.body,
        created_at: resp.thread.created_at,
        replies: resp
            .replies
            .into_iter()
            .map(|r| CloudForumReply {
                id: r.id,
                author: r.author,
                author_is_me: r.author_is_me,
                body: r.body,
                created_at: r.created_at,
            })
            .collect(),
    }
}

// ── Forum v2 wire → UI mapping ─────────────────────────────────────────
// Same split as the v1 mapping above: `mlc-app-layout` cannot depend on
// `mlc-cloud`, so the shell turns a wire row into the UI mirror.

/// Full client-side ladder for [`forum2_can_write`] — `public < user < paid
/// < trusted < moderator < admin`, matching `social_db::Class`'s own
/// ordering (`forum_v2_overlay`'s doc names it, this crate does not depend
/// on the server crate to read it directly). `trusted` (rank 3 — "paid with
/// standing", owner ruling 2026-09-08) sits between `paid` and `moderator`;
/// mirrored exactly in `mlc-app-shell-vello`'s own
/// `forum2_class_rank_native` — the two must never disagree on a rank.
fn forum2_class_rank(class: &str) -> u8 {
    match class {
        "public" => 0,
        "user" => 1,
        "paid" => 2,
        "trusted" => 3,
        "moderator" => 4,
        "admin" => 5,
        _ => 0,
    }
}

/// Whether the calling account may write to a board with `write_class`,
/// resolved from the server's own `GET /api/forum/me` answer
/// (`ForumV2State::caller_classes`) rather than a client-side `signed_in`/
/// `paid` guess — `caller_classes` is empty until that fetch lands (or
/// after a sign-in/sign-out edge invalidates it), which reads as rank 0
/// here, the same safe default the old guess used before the first
/// answer: it hides a button the server would refuse anyway rather than
/// showing one that always 403s. The server remains the real enforcement
/// point regardless of what this resolves to.
fn forum2_can_write(write_class: &str, caller_classes: &[String]) -> bool {
    let caller_rank = caller_classes.iter().map(|c| forum2_class_rank(c)).max().unwrap_or(0);
    caller_rank >= forum2_class_rank(write_class)
}

fn map_forum_board(b: ForumBoard, caller_classes: &[String]) -> BoardUi {
    let can_write = forum2_can_write(&b.write_class, caller_classes);
    BoardUi {
        slug: b.slug,
        title: b.title,
        description: b.description,
        read_class: b.read_class,
        write_class: b.write_class,
        can_write,
        thread_count: b.thread_count,
        reply_count: b.reply_count,
        last_post_at: b.last_post_at,
        last_thread_title: b.last_thread_title,
    }
}

fn map_forum_thread_v2_summary(s: ForumThreadV2Summary) -> ForumThreadV2Ui {
    ForumThreadV2Ui {
        id: s.id,
        title: s.title,
        author: s.author,
        author_is_me: s.author_is_me,
        author_rank: s.author_rank,
        created_at: s.created_at,
        updated_at: s.updated_at,
        last_reply_at: s.last_reply_at,
        reply_count: s.reply_count,
        pinned: s.pinned,
        locked: s.locked,
    }
}

fn map_forum_reply_node(r: ForumReplyNode) -> ForumReplyV2Ui {
    ForumReplyV2Ui {
        id: r.id,
        parent_reply_id: r.parent_reply_id,
        depth: r.depth,
        author: r.author,
        author_is_me: r.author_is_me,
        author_rank: r.author_rank,
        body: r.body,
        created_at: r.created_at,
        hidden: r.hidden,
        children: r.children.into_iter().map(map_forum_reply_node).collect(),
    }
}

fn map_forum_thread_v2_detail(resp: &ForumThreadV2Response) -> ForumThreadV2Detail {
    let t = &resp.thread;
    ForumThreadV2Detail {
        id: t.id,
        board_id: t.board_id,
        title: t.title.clone(),
        author: t.author.clone(),
        author_is_me: t.author_is_me,
        author_rank: t.author_rank.clone(),
        body: t.body.clone(),
        created_at: t.created_at.clone(),
        updated_at: t.updated_at.clone(),
        pinned: t.pinned,
        locked: t.locked,
        hidden: t.hidden,
    }
}

fn map_forum_search_hit(h: ForumSearchHit) -> ForumSearchHitUi {
    ForumSearchHitUi {
        kind: h.kind,
        board_id: h.board_id,
        thread_id: h.thread_id,
        reply_id: h.reply_id,
        title: h.title,
        snippet: h.snippet,
        author: h.author,
        created_at: h.created_at,
    }
}

// ── localStorage persistence ─────────────────────────────────────────────

/// `origin + url_path` → the absolute share URL handed to the clipboard.
/// `url_path` is root-relative (`/?share=<token>`) exactly so the client
/// pins the origin the share was created on.
pub(crate) fn absolute_share_url(url_path: &str) -> String {
    let origin = web_sys::window()
        .and_then(|w| w.location().origin().ok())
        .unwrap_or_default();
    format!("{}{}", origin, url_path)
}

/// Word a failed share create for the console AND the system-event log
/// (system-event-bus arc, 2026-09-05) — one wording, two destinations. 403
/// from either share-create endpoint is the per-kind active-share ceiling
/// (free 1 live / 10 screenshots, paid 10/100) — say so in words; anything
/// else is the transport's own description.
fn share_error_message(what: &str, e: &mlc_cloud::CloudError) -> String {
    match e {
        mlc_cloud::CloudError::Forbidden(_) => {
            format!("{what} share limit reached — revoke an existing share or upgrade the account")
        }
        other => format!("{what} share failed: {other}"),
    }
}

fn log_share_error(what: &str, e: &mlc_cloud::CloudError) {
    let message = share_error_message(what, e);
    match e {
        mlc_cloud::CloudError::Forbidden(_) => {
            web_sys::console::warn_1(&format!("[share] {message}").into());
        }
        _ => {
            web_sys::console::error_1(&format!("[share] {message}").into());
        }
    }
}

/// Flags [`Inner::unauthorized`] when `e` is a 401 AND a token was actually
/// held at call time — called from inside a `spawn_local` body, before the
/// typed `CloudError` is turned into a display string for its own mailbox,
/// so `drive()` can drop to signed-out the same frame the answer lands (Arc
/// B1, 2026-09-06: "signed in" means "the server agrees"). Deliberately does
/// nothing for `CloudError::Forbidden` (403 is an entitlement refusal, not a
/// rejected credential), a network/transport failure, or a 401 on a client
/// holding no credential at all — see [`mlc_cloud::is_session_revoked_401`]
/// for that predicate.
///
/// Owner repro 2026-09-24: the Forum & Messages modal fired the
/// caller-identity fetch (`/api/forum/me`) unconditionally on open, even for
/// a never-signed-in visitor; the resulting 401 used to flip straight to
/// signed-out and produce a "Signed out — The server no longer accepts this
/// session — sign in again." toast on a session that was never signed in.
/// The stray request is fixed at its call site (gated on `is_signed_in()`
/// below); this is the second, independent guard so any FUTURE stray
/// authenticated-only fetch fails silently (console only) instead of
/// misfiring the toast again.
fn mark_unauthorized(inner: &Rc<RefCell<Inner>>, e: &mlc_cloud::CloudError) {
    let mut inner = inner.borrow_mut();
    if mlc_cloud::is_session_revoked_401(inner.token.is_some(), e.is_unauthorized()) {
        inner.unauthorized = true;
    }
}

// ── Direct-message crypto helpers ────────────────────────────────────────
// The shell owns the crypto end to end (`mlc_vault::dm`'s own module doc) —
// mirrors `mlc-app-shell-vello`'s own copy of these exactly (`main.rs`'s
// `dm_aad`/`dm_decode_public_key`/`dm_decode_nonce`/`dm_encode`/
// `decrypt_dm_message_native`), so the two shells derive the identical AAD
// and reach the identical plaintext for the same wire message.
// `mlc-app-layout`'s `DmMessageUi::body` (`DmMessageBody`) never carries a
// key — only plaintext, raw wire ciphertext (pre-unlock, or before a peer
// key backfill lands), or the permanent "could not decrypt" marker.

/// AAD binding for one DM message's seal/open — conversation id plus the
/// SENDING identity's own key fingerprint. See `mlc-app-shell-vello`'s own
/// copy for the full reasoning; both shells must derive the same value for
/// the same message.
fn dm_aad(conversation_id: i64, sender_fingerprint: &str) -> Vec<u8> {
    format!("{}|{}", conversation_id, sender_fingerprint).into_bytes()
}

fn dm_decode_public_key(public_key_b64: &str) -> Option<[u8; 32]> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD.decode(public_key_b64).ok()?;
    bytes.try_into().ok()
}

fn dm_decode_nonce(nonce_b64: &str) -> Option<[u8; 12]> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD.decode(nonce_b64).ok()?;
    bytes.try_into().ok()
}

fn dm_encode(bytes: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn dm_base64_decode(s: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(s).ok()
}

/// Decrypt one wire message into a [`DmMessageBody`]. `ctx =
/// (conversation_key, my_public, peer_public)`, `None` when either this
/// profile's identity or the conversation's cached peer key is missing
/// (sealed vault, or the peer key has not been resolved/backfilled yet) —
/// that case returns `Ciphertext` (retryable: [`redecrypt_open_conversation`]
/// re-runs this exact function once new key material lands), never the
/// permanent `Undecryptable`.
fn decrypt_dm_message_web(
    ctx: Option<&([u8; 32], [u8; 32], [u8; 32])>,
    conversation_id: i64,
    sender_is_me: bool,
    nonce_b64: &str,
    ciphertext_b64: &str,
) -> DmMessageBody {
    let Some((key, my_public, peer_public)) = ctx else {
        return DmMessageBody::Ciphertext {
            nonce_b64: nonce_b64.to_string(),
            ciphertext_b64: ciphertext_b64.to_string(),
        };
    };
    let Some(nonce) = dm_decode_nonce(nonce_b64) else {
        return DmMessageBody::Undecryptable;
    };
    let Some(ciphertext) = dm_base64_decode(ciphertext_b64) else {
        return DmMessageBody::Undecryptable;
    };
    let sender_public = if sender_is_me { my_public } else { peer_public };
    let aad = dm_aad(conversation_id, &mlc_vault::dm::fingerprint(sender_public));
    let sealed = mlc_vault::dm::DmCiphertext { nonce, ciphertext };
    match mlc_vault::dm::open_message(key, &sealed, &aad) {
        Ok(plaintext) => DmMessageBody::Plain(String::from_utf8_lossy(&plaintext).into_owned()),
        Err(_) => DmMessageBody::Undecryptable,
    }
}

/// Re-derive `body` for every still-[`DmMessageBody::Ciphertext`] message in
/// the currently open conversation, from each message's own stored
/// `nonce_b64`/`ciphertext_b64` — no refetch. Idempotent (a no-op once
/// nothing is left to retry), so `drive()` calls this unconditionally every
/// frame rather than edge-detecting the two things that can unblock a
/// message: the vault unlocking (`DmState::available` flipping true) and a
/// peer-key backfill landing for the open conversation (owner ruling
/// 2026-09-10: ciphertext shows before unlock, decrypts in place after —
/// the same principle covers a backfill resolving mid-view).
fn redecrypt_open_conversation(
    dm: &mut mlc_app_layout::ui::modal_settings::DmState,
    identity: Option<&mlc_vault::dm::DmIdentity>,
    peer_keys: &std::collections::HashMap<i64, [u8; 32]>,
) {
    let DmView::Conversation { id, messages, .. } = &mut dm.view else { return };
    let conversation_id = *id;
    let ctx = match (identity, peer_keys.get(&conversation_id)) {
        (Some(identity), Some(peer_public)) => {
            let key: [u8; 32] =
                *mlc_vault::dm::conversation_key(&identity.secret, peer_public, &conversation_id.to_string());
            Some((key, identity.public, *peer_public))
        }
        _ => None,
    };
    let Some(ctx) = ctx else { return };
    for m in messages.iter_mut() {
        if let DmMessageBody::Ciphertext { nonce_b64, ciphertext_b64 } = &m.body {
            m.body = decrypt_dm_message_web(Some(&ctx), conversation_id, m.mine, nonce_b64, ciphertext_b64);
        }
    }
}

fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window().and_then(|w| w.local_storage().ok().flatten())
}

fn load_token() -> Option<StoredToken> {
    let raw = local_storage()?.get_item(TOKEN_KEY).ok().flatten()?;
    serde_json::from_str(&raw).ok()
}

fn save_token(token: &StoredToken) {
    if let (Some(ls), Ok(json)) = (local_storage(), serde_json::to_string(token)) {
        let _ = ls.set_item(TOKEN_KEY, &json);
    }
}

fn clear_token() {
    if let Some(ls) = local_storage() {
        let _ = ls.remove_item(TOKEN_KEY);
    }
}

/// Last APPLIED entitlements answer this browser persisted, keyed by the
/// identity it was answered for — see `TIER_KEY`'s own doc and the
/// entitlements block in `drive()`.
///
/// Built/read through `serde_json::Value` rather than derived (de)serialize
/// impls: this crate depends on `serde_json` but not `serde` directly, and
/// `StoredToken` (`mlc-cloud`) already carries the derive because it owns
/// that dependency — this small local record does not need to add one.
#[derive(Debug, Clone)]
struct StoredTier {
    user_id: i64,
    paid: bool,
    paid_until: Option<String>,
}

fn load_tier() -> Option<StoredTier> {
    let raw = local_storage()?.get_item(TIER_KEY).ok().flatten()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    Some(StoredTier {
        user_id: v.get("user_id")?.as_i64()?,
        paid: v.get("paid")?.as_bool()?,
        paid_until: v
            .get("paid_until")
            .and_then(|pu| pu.as_str())
            .map(|s| s.to_string()),
    })
}

fn save_tier(tier: &StoredTier) {
    if let Some(ls) = local_storage() {
        let json = serde_json::json!({
            "user_id": tier.user_id,
            "paid": tier.paid,
            "paid_until": tier.paid_until,
        });
        let _ = ls.set_item(TIER_KEY, &json.to_string());
    }
}

fn clear_tier() {
    if let Some(ls) = local_storage() {
        let _ = ls.remove_item(TIER_KEY);
    }
}

fn load_or_create_device_id() -> String {
    if let Some(ls) = local_storage() {
        if let Ok(Some(existing)) = ls.get_item(DEVICE_KEY) {
            if !existing.is_empty() {
                return existing;
            }
        }
        // Browser crypto RNG — UUID v4 from the platform.
        let id = web_sys::window()
            .and_then(|w| w.crypto().ok())
            .map(|c| c.random_uuid())
            .unwrap_or_else(|| format!("web-{}", js_sys::Date::now() as u64));
        let _ = ls.set_item(DEVICE_KEY, &id);
        id
    } else {
        format!("web-{}", js_sys::Date::now() as u64)
    }
}

/// Which build id (`win64` / `macos-arm64` / `macos-x64` / `linux64`,
/// `mlc_updater::platform`'s own ids) the Download overlay should offer this
/// visitor first. Mirrors the landing page's own `detectPlatform()`
/// (`mylittlechart-landing/web/index.html`) exactly, including the reasoning:
///
/// The OS is readable and reliable from `navigator.userAgent`. The CPU
/// architecture is not: only Chromium exposes it
/// (`navigator.userAgentData.getHighEntropyValues(['architecture'])`), and
/// Safari on an Apple Silicon Mac still reports an Intel-looking user agent
/// — so trusting the UA there would hand an M-series visitor the x86_64
/// build. Hence: read the OS from the UA, ask for the architecture where it
/// can actually be asked, and default macOS to arm64 otherwise, since Apple
/// stopped selling Intel Macs in 2023. Whoever this guesses wrong for finds
/// every other build in the expanded list, which is the only reason
/// guessing is acceptable at all.
///
/// `userAgentData` has no typed `web-sys` binding (it is Chromium-only and
/// not part of the stable web-sys feature set here), hence the raw
/// `js_sys::Reflect` calls rather than a struct field.
async fn detect_platform() -> &'static str {
    let Some(window) = web_sys::window() else {
        return "win64";
    };
    let navigator = window.navigator();
    let ua = navigator.user_agent().unwrap_or_default();
    if ua.contains("Windows") {
        return "win64";
    }
    if ua.contains("Linux") || ua.contains("X11") {
        return "linux64";
    }
    if !(ua.contains("Mac OS X") || ua.contains("Macintosh")) {
        // Unrecognised UA (a phone, something exotic): still offer a build
        // rather than nothing, same fallback the landing page uses.
        return "win64";
    }

    let uad = js_sys::Reflect::get(&navigator, &"userAgentData".into())
        .ok()
        .filter(|v| !v.is_undefined() && !v.is_null());
    let Some(uad) = uad else {
        return "macos-arm64";
    };
    let get_high_entropy_values = js_sys::Reflect::get(&uad, &"getHighEntropyValues".into())
        .ok()
        .and_then(|f| f.dyn_into::<js_sys::Function>().ok());
    let Some(func) = get_high_entropy_values else {
        return "macos-arm64";
    };
    let hints = js_sys::Array::of1(&"architecture".into());
    let promise = func
        .call1(&uad, &hints)
        .ok()
        .and_then(|p| p.dyn_into::<js_sys::Promise>().ok());
    let Some(promise) = promise else {
        return "macos-arm64";
    };
    match JsFuture::from(promise).await {
        Ok(v) => {
            let arch = js_sys::Reflect::get(&v, &"architecture".into())
                .ok()
                .and_then(|a| a.as_string())
                .unwrap_or_default();
            if arch == "x86" {
                "macos-x64"
            } else {
                "macos-arm64"
            }
        }
        Err(_) => "macos-arm64",
    }
}

/// Read `window.MLC_BUILD = { sig, version, timestamp, platform }`
/// (injected by package-app.sh at bundle-signing time).
fn read_build_attestation() -> Option<BuildAttestation> {
    let window = web_sys::window()?;
    let v = js_sys::Reflect::get(&window, &"MLC_BUILD".into()).ok()?;
    if v.is_undefined() || v.is_null() {
        return None;
    }
    let get_str = |k: &str| -> Option<String> {
        js_sys::Reflect::get(&v, &k.into()).ok()?.as_string()
    };
    let sig = get_str("sig")?;
    let version = get_str("version")?;
    let platform = get_str("platform").unwrap_or_else(|| "web".to_string());
    let timestamp = js_sys::Reflect::get(&v, &"timestamp".into())
        .ok()?
        .as_f64()? as u64;
    Some(BuildAttestation {
        signature_b64: sig,
        version,
        timestamp,
        platform,
    })
}

/// Open a URL in a new browser tab (best-effort; popup blockers may
/// interfere, but these calls all originate from click-driven frames).
fn open_new_tab(url: &str) {
    if let Some(w) = web_sys::window() {
        let _ = w.open_with_url_and_target(url, "_blank");
    }
}

/// Open the provider's sign-in page in a small popup rather than a tab.
///
/// The provider's page cannot live inside our own modal: it is their domain,
/// their cookies, and they forbid being framed. So a real browsing context is
/// unavoidable — but it does not have to be a full tab the user is then told
/// to close by hand. A sized popup keeps the app on screen behind it, and the
/// callback page closes itself, so the modal simply flips to signed-in without
/// the user doing anything (owner, 2026-09-03: "не надо спавнить другое окно
/// для оауса").
///
/// Falls back to a tab if the popup is blocked — refusing to sign in because a
/// popup blocker exists would be worse than the tab it replaces.
fn open_auth_popup(url: &str) {
    let Some(w) = web_sys::window() else { return };
    let features = "popup=yes,width=520,height=680,menubar=no,toolbar=no,location=yes";
    match w.open_with_url_and_target_and_features(url, "mlc_oauth", features) {
        Ok(Some(_)) => {}
        _ => {
            let _ = w.open_with_url_and_target(url, "_blank");
        }
    }
}
