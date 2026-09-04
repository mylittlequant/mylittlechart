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
    CloudLoginPhase, CloudNewsPost, CloudOverlayData, CloudOverlayKind, CloudReleaseEntry,
    RedeemStatus,
};
use mlc_cloud::{
    redeem_error_message, BuildAttestation, BlogPost, CloudConfig, DeviceAuthPoll,
    EntitlementsResponse, FetchTransport, ReleaseEntry, StoredToken, VersionManifest,
};

const TOKEN_KEY: &str = "mlc.cloud.token.v1";
const DEVICE_KEY: &str = "mlc.cloud.device_id.v1";
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
                entitlements_in_flight: false,
                next_entitlements_ms: 0.0,
                entitlements_result: None,
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
        spawn_local(async move {
            match transport.share_create_screenshot(&title, png).await {
                Ok(resp) => {
                    let url = absolute_share_url(&resp.url_path);
                    crate::clipboard_write_text(&url);
                    web_sys::console::log_1(&format!("[share] screenshot shared: {}", url).into());
                }
                Err(e) => log_share_error("screenshot", &e),
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
        spawn_local(async move {
            match transport.share_create_live(&title, preset).await {
                Ok(resp) => {
                    let url = absolute_share_url(&resp.url_path);
                    crate::clipboard_write_text(&url);
                    web_sys::console::log_1(&format!("[share] live chart shared: {}", url).into());
                }
                Err(e) => log_share_error("live", &e),
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
                        // A failed or unanswered fetch degrades to the free
                        // set — never to the paid set, and never leaves the
                        // General tab locked on a stale answer either.
                        chart.panel_app.user_settings_state.multi_profile = false;
                        chart.panel_app.cloud_login_state.is_paid = false;
                        chart.panel_app.cloud_login_state.paid_until = None;
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
                    st.phase = CloudLoginPhase::Success { display_name };
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
                FlowResult::Logout => {
                    // Local sign-out regardless of the server answer.
                    clear_token();
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
                    {
                        let mut user = chart.core.user_mut();
                        let p = user.profile_mut();
                        p.domain.linked_account = None;
                        p.domain.cloud_enabled = false;
                        p.domain.sync_level = "local".to_string();
                    }
                    chart.profile_dirty = true;
                    st.phase = CloudLoginPhase::RequestCode;
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

/// Word a failed share create for the console. 403 from either share-create
/// endpoint is the per-kind active-share ceiling (free 1 live / 10
/// screenshots, paid 10/100) — say so in words; anything else is the
/// transport's own description.
fn log_share_error(what: &str, e: &mlc_cloud::CloudError) {
    match e {
        mlc_cloud::CloudError::Forbidden(_) => {
            web_sys::console::warn_1(
                &format!("[share] {} share limit reached — revoke an existing share or upgrade the account", what).into(),
            );
        }
        other => {
            web_sys::console::error_1(
                &format!("[share] {} share failed: {}", what, other).into(),
            );
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
