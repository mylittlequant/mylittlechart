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
    CloudForumReply, CloudForumThread, CloudForumThreadSummary, CloudLoginPhase, CloudNewsPost,
    CloudOverlayData, CloudOverlayKind, CloudReleaseEntry, ForumListState, ForumRequest,
    ForumSubView, NewsTab, RedeemStatus,
};
use mlc_cloud::{
    forum_error_message, redeem_error_message, BuildAttestation, BlogPost, CloudConfig,
    DeviceAuthPoll, EntitlementsResponse, FetchTransport, ForumThreadResponse, ForumThreadSummary,
    MeResponse, ReleaseEntry, StoredToken, VersionManifest,
};

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
}

struct Inner {
    transport: FetchTransport,
    token: Option<StoredToken>,
    /// An async step is in flight — don't stack requests.
    in_flight: bool,
    next_poll_ms: f64,
    /// Previous frame's overlay open flag (edge detection).
    was_open: bool,
    result: Option<FlowResult>,

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

    // ── Forum tab (paid-only) ───────────────────────────────────────────
    /// A forum request is in flight — separate from `in_flight` above so
    /// the Forum tab never blocks (or gets blocked by) the login/News/
    /// Download flow that field already gates.
    forum_in_flight: bool,
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
                result: None,
                me_in_flight: false,
                me_result: None,
                me_boot_checked: false,
                entitlements_in_flight: false,
                next_entitlements_ms: 0.0,
                entitlements_result: None,
                prev_entitlements: None,
                share_result: None,
                forum_in_flight: false,
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
            let r = transport.auth_me().await.map_err(|e| e.to_string());
            inner.borrow_mut().me_result = Some(r);
        });
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
    pub fn share_screenshot(&self, png: web_sys::Blob, title: String) {
        if !self.is_signed_in() {
            web_sys::console::warn_1(&"[share] screenshot share refused: not signed in".into());
            return;
        }
        let transport = self.inner.borrow().transport.snapshot();
        let inner = self.inner.clone();
        spawn_local(async move {
            match transport.share_create_screenshot(&title, png).await {
                Ok(resp) => {
                    let url = absolute_share_url(&resp.url_path);
                    crate::clipboard_write_text(&url);
                    web_sys::console::log_1(&format!("[share] screenshot shared: {}", url).into());
                    inner.borrow_mut().share_result = Some(("screenshot", Ok(url)));
                }
                Err(e) => {
                    let message = share_error_message("screenshot", &e);
                    log_share_error("screenshot", &e);
                    inner.borrow_mut().share_result = Some(("screenshot", Err(message)));
                }
            }
        });
    }

    /// `POST /api/share/live` — freeze the active preset into a public link.
    /// Same refusal/result contract as [`Self::share_screenshot`].
    pub fn share_live(&self, title: String, preset: serde_json::Value) {
        if !self.is_signed_in() {
            web_sys::console::warn_1(&"[share] live share refused: not signed in".into());
            return;
        }
        let transport = self.inner.borrow().transport.snapshot();
        let inner = self.inner.clone();
        spawn_local(async move {
            match transport.share_create_live(&title, preset).await {
                Ok(resp) => {
                    let url = absolute_share_url(&resp.url_path);
                    crate::clipboard_write_text(&url);
                    web_sys::console::log_1(&format!("[share] live chart shared: {}", url).into());
                    inner.borrow_mut().share_result = Some(("live", Ok(url)));
                }
                Err(e) => {
                    let message = share_error_message("live", &e);
                    log_share_error("live", &e);
                    inner.borrow_mut().share_result = Some(("live", Err(message)));
                }
            }
        });
    }

    pub fn drive(&self, chart: &mut ChartApp, now_ms: f64) {
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
        // Mirrors `account_label()` — the same source the chrome's account
        // button reads — so the nick edit box can prefill with what the
        // person already sees there, not the unrelated local profile name.
        chart.panel_app.user_settings_state.account_label =
            self.account_label().unwrap_or_default();

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
                    let r = transport.entitlements().await.map_err(|e| e.to_string());
                    inner.borrow_mut().entitlements_result = Some(r);
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
                    chart.push_system_event(mlc_core::SystemEventKind::Share, "Share failed", message, true);
                }
            }
        }

        // ── Apply finished async results ─────────────────────────────────
        let result = self.inner.borrow_mut().result.take();
        if let Some(result) = result {
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
                    chart.panel_app.cloud_overlay_state.data = CloudOverlayData::News {
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
                }
                FlowResult::News(Err(e)) => {
                    chart.panel_app.cloud_overlay_state.data = CloudOverlayData::Error { message: e };
                }
                FlowResult::Redeem(Ok(paid_until)) => {
                    st.redeem_status = RedeemStatus::Redeemed {
                        paid_until: paid_until.clone(),
                    };
                    // The tier just changed; the cached entitlements answer is
                    // now wrong. Ask again on the next frame rather than
                    // waiting out a TTL the user did not agree to.
                    self.inner.borrow_mut().next_entitlements_ms = 0.0;
                }
                FlowResult::Redeem(Err(message)) => {
                    st.redeem_status = RedeemStatus::Failed { message };
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
                FlowResult::Logout => {
                    // Local sign-out regardless of the server answer.
                    clear_token();
                    clear_tier();
                    let mut inner = self.inner.borrow_mut();
                    inner.transport.set_token(None);
                    inner.token = None;
                    // Drop straight to the free default rather than leaving
                    // last identity's (possibly paid) answer on screen until
                    // the next scheduled re-ask — and re-ask right away so
                    // it reflects "no credential" rather than staying stuck
                    // on a stale cached TTL.
                    inner.next_entitlements_ms = 0.0;
                    drop(inner);
                    chart.panel_app.user_settings_state.multi_profile = false;
                    chart.panel_app.user_settings_state.account_label = String::new();
                    chart.panel_app.user_settings_state.nick_edit_mode = false;
                    chart.panel_app.user_settings_state.nick_edit_focused = false;
                    chart.panel_app.user_settings_state.nick_status =
                        mlc_app_layout::ui::modal_settings::NickStatus::Idle;
                    {
                        let mut user = chart.core.user_mut();
                        let p = user.profile_mut();
                        p.domain.linked_account = None;
                        p.domain.cloud_enabled = false;
                        p.domain.sync_level = "local".to_string();
                    }
                    chart.profile_dirty = true;
                    // Same-frame gating (owner ruling 2026-09-06): a
                    // ShareLink pick made while signed in must not survive
                    // sign-out and keep firing uploads afterward.
                    if chart.panel_app.toolbar_state.screenshot_destination
                        == mlc_core::state::toolbar_state::ScreenshotDestination::ShareLink
                    {
                        chart.panel_app.toolbar_state.screenshot_destination =
                            mlc_core::state::toolbar_state::ScreenshotDestination::Clipboard;
                    }
                    st.phase = CloudLoginPhase::RequestCode;
                    // `st`'s last use above ends its borrow before this
                    // method call, same reordering as the sign-in arm.
                    chart.push_system_event(
                        mlc_core::SystemEventKind::Account,
                        "Signed out",
                        "You have been logged out.",
                        true,
                    );
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
                        let r = transport
                            .set_nick(&nick)
                            .await
                            .map(|resp| resp.effective_label)
                            .map_err(|e| mlc_cloud::nick_error_message(&e));
                        inner.borrow_mut().result = Some(FlowResult::Nick(r));
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
                if needs_data && !self.inner.borrow().in_flight {
                    ov.data = CloudOverlayData::Loading;
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
                                inner.borrow_mut().result = Some(FlowResult::News(r));
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
                                inner.borrow_mut().result = Some(FlowResult::Updates(r));
                            }
                        }
                    });
                }
            }
        }

        // ── Forum tab (paid-only): passive list fetch + explicit actions ──
        {
            let ov = &mut chart.panel_app.cloud_overlay_state;
            let forum_active = ov.is_open && ov.kind == CloudOverlayKind::News && ov.news_tab == NewsTab::Forum;

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
                    let r = transport
                        .forum_threads(None)
                        .await
                        .map(|resp| (resp.threads, resp.next_before))
                        .map_err(|e| forum_error_message(&e));
                    inner.borrow_mut().result = Some(FlowResult::ForumThreads { append: false, result: r });
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
                            let r = transport.forum_thread(id).await.map_err(|e| forum_error_message(&e));
                            inner.borrow_mut().result = Some(FlowResult::ForumThreadOpened(r));
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
                                let r = transport
                                    .forum_threads(Some(&before))
                                    .await
                                    .map(|resp| (resp.threads, resp.next_before))
                                    .map_err(|e| forum_error_message(&e));
                                inner.borrow_mut().result = Some(FlowResult::ForumThreads { append: true, result: r });
                            }
                            ForumRequest::LoadThread { id } => {
                                let r = transport.forum_thread(id).await.map_err(|e| forum_error_message(&e));
                                inner.borrow_mut().result = Some(FlowResult::ForumThreadOpened(r));
                            }
                            ForumRequest::CreateThread { title, body } => {
                                let r = match transport.forum_create_thread(&title, &body).await {
                                    Ok(id) => transport.forum_thread(id).await.map_err(|e| forum_error_message(&e)),
                                    Err(e) => Err(forum_error_message(&e)),
                                };
                                inner.borrow_mut().result = Some(FlowResult::ForumThreadCreated(r));
                            }
                            ForumRequest::Reply { thread_id, body } => {
                                let r = match transport.forum_reply(thread_id, &body).await {
                                    Ok(_) => transport.forum_thread(thread_id).await.map_err(|e| forum_error_message(&e)),
                                    Err(e) => Err(forum_error_message(&e)),
                                };
                                inner.borrow_mut().result = Some(FlowResult::ForumThreadRefreshed(r));
                            }
                            ForumRequest::DeleteThread { id } => {
                                let r = match transport.forum_delete_thread(id).await {
                                    Ok(()) => transport
                                        .forum_threads(None)
                                        .await
                                        .map(|resp| (resp.threads, resp.next_before))
                                        .map_err(|e| forum_error_message(&e)),
                                    Err(e) => Err(forum_error_message(&e)),
                                };
                                inner.borrow_mut().result = Some(FlowResult::ForumThreadDeleted(r));
                            }
                            ForumRequest::DeleteReply { id, thread_id } => {
                                let r = match transport.forum_delete_reply(id).await {
                                    Ok(()) => transport.forum_thread(thread_id).await.map_err(|e| forum_error_message(&e)),
                                    Err(e) => Err(forum_error_message(&e)),
                                };
                                inner.borrow_mut().result = Some(FlowResult::ForumThreadRefreshed(r));
                            }
                        }
                    });
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
        // A submitted code. The field keeps its text — a refused code is
        // usually one wrong character, not a reason to retype ten.
        if st.redeem_requested {
            st.redeem_requested = false;
            let code = {
                let coord = chart.input_coordinator.borrow();
                coord
                    .text_fields()
                    .text(&uzor::WidgetId::from(mlc_app::text_input::REDEEM_CODE))
                    .trim()
                    .to_string()
            };
            if code.is_empty() {
                st.redeem_status = RedeemStatus::Failed {
                    message: "Enter a code first".to_string(),
                };
            } else if self.inner.borrow().token.is_none() {
                st.redeem_status = RedeemStatus::Failed {
                    message: "Sign in first".to_string(),
                };
            } else {
                st.redeem_status = RedeemStatus::Working;
                let inner = self.inner.clone();
                let transport = self.inner.borrow().transport.snapshot();
                self.inner.borrow_mut().in_flight = true;
                spawn_local(async move {
                    let r = transport
                        .redeem(&code)
                        .await
                        .map(|resp| resp.paid_until)
                        .map_err(|e| redeem_error_message(&e));
                    inner.borrow_mut().result = Some(FlowResult::Redeem(r));
                });
            }
            return;
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
                    inner.borrow_mut().result = Some(FlowResult::Logout);
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
                    inner.borrow_mut().result = Some(FlowResult::Code(r));
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
                    inner.borrow_mut().result = Some(FlowResult::Poll(r));
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

// ── localStorage persistence ─────────────────────────────────────────────

/// `origin + url_path` → the absolute share URL handed to the clipboard.
/// `url_path` is root-relative (`/?share=<token>`) exactly so the client
/// pins the origin the share was created on.
fn absolute_share_url(url_path: &str) -> String {
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
