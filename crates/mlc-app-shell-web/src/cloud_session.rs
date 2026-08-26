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

use wasm_bindgen_futures::spawn_local;

use mlc_app::ChartApp;
use mlc_app_layout::ui::modal_settings::CloudLoginPhase;
use mlc_cloud::{BuildAttestation, CloudConfig, DeviceAuthPoll, FetchTransport, StoredToken};

const TOKEN_KEY: &str = "mlc.cloud.token.v1";
const DEVICE_KEY: &str = "mlc.cloud.device_id.v1";
const POLL_INTERVAL_MS: f64 = 2500.0;
/// Where the user confirms the device code (cookie-session web flow).
const CONFIRM_PAGE: &str = "/dashboard";

/// Completed async step, written by spawn_local futures and applied to the
/// chart state by the next `drive()` call (mailbox pattern).
enum FlowResult {
    Code(Result<String, String>),
    Poll(Result<DeviceAuthPoll, String>),
    Logout,
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
}

pub struct CloudSession {
    inner: Rc<RefCell<Inner>>,
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
            })),
        }
    }

    /// Per-frame driver. Reads the overlay's phase + one-shot flags, advances
    /// the flow, applies finished async results.
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
                FlowResult::Logout => {
                    // Local sign-out regardless of the server answer.
                    clear_token();
                    let mut inner = self.inner.borrow_mut();
                    inner.transport.set_token(None);
                    inner.token = None;
                    drop(inner);
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
        if st.open_page_requested {
            st.open_page_requested = false;
            if let Some(w) = web_sys::window() {
                let _ = w.open_with_url_and_target(CONFIRM_PAGE, "_blank");
            }
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
