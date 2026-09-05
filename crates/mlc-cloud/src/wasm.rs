//! Fetch-based [`CloudTransport`] for wasm32 (browser).
//!
//! One `XMLHttpRequest`-free implementation on top of `window.fetch` via
//! `web-sys`. All endpoints are same-origin in production (`server_url = ""`),
//! so no CORS preflight is involved beyond the custom `X-*` headers — the
//! server must allow `Authorization`, `X-Profile-Id`, `X-Device-Id` and
//! `X-Build-*` in its CORS config for cross-origin (dev) use.

use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Headers, Request, RequestCache, RequestInit, RequestMode, Response};

use crate::models::*;
use crate::{BuildAttestation, CloudConfig, CloudError};

/// Wasm32 cloud transport: one `fetch` per call, no connection state.
pub struct FetchTransport {
    config: CloudConfig,
}

impl FetchTransport {
    pub fn new(config: CloudConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &CloudConfig {
        &self.config
    }

    pub fn config_mut(&mut self) -> &mut CloudConfig {
        &mut self.config
    }

    /// Cheap clone of the transport config for a single spawned request —
    /// the driver stays `Rc`-shared while each fetch owns its snapshot.
    pub fn snapshot(&self) -> Self {
        Self {
            config: self.config.clone(),
        }
    }

    /// Install (or clear) the bearer token after a completed login.
    pub fn set_token(&mut self, token: Option<String>) {
        self.config.token = token;
    }

    pub fn set_attestation(&mut self, attestation: Option<BuildAttestation>) {
        self.config.attestation = attestation;
    }

    fn url(&self, path: &str) -> String {
        format!(
            "{}{}",
            self.config.server_url.trim_end_matches('/'),
            path
        )
    }

    /// Build one `Request` with the shared header set attached.
    ///
    /// `json_body` controls Content-Type: `application/json` for structured
    /// bodies, absent for multipart (the browser must set the
    /// `multipart/form-data; boundary=…` header itself). `auth` gates the
    /// bearer token — the public share read must go out WITHOUT it even
    /// when one is configured: the link token alone is the capability,
    /// and sending the viewer's own credential along is pure leak surface.
    fn build_request(
        &self,
        method: &str,
        path: &str,
        body: Option<JsValue>,
        json_body: bool,
        auth: bool,
        extra_headers: &[(&str, &str)],
    ) -> Result<Request, CloudError> {
        let opts = RequestInit::new();
        opts.set_method(method);
        opts.set_mode(RequestMode::Cors);
        opts.set_cache(RequestCache::NoStore);
        if let Some(b) = body.as_ref() {
            opts.set_body(b);
        }

        let req = Request::new_with_str_and_init(&self.url(path), &opts)
            .map_err(|e| CloudError::Transport(format!("build request: {:?}", e)))?;

        let headers: Headers = req.headers();
        let set = |k: &str, v: &str| -> Result<(), CloudError> {
            headers
                .set(k, v)
                .map_err(|e| CloudError::Transport(format!("set header {}: {:?}", k, e)))
        };
        if json_body {
            set("Content-Type", "application/json")?;
        }
        if auth {
            if let Some(token) = self.config.token.as_ref() {
                set("Authorization", &format!("Bearer {}", token))?;
            }
        }
        set("X-Profile-Id", &self.config.profile_id)?;
        set("X-Device-Id", &self.config.device_id)?;
        if let Some(a) = self.config.attestation.as_ref() {
            set("X-Build-Attestation", &a.signature_b64)?;
            set("X-Build-Version", &a.version)?;
            set("X-Build-Timestamp", &a.timestamp.to_string())?;
            set("X-Build-Platform", &a.platform)?;
        }
        for (k, v) in extra_headers {
            set(k, v)?;
        }
        Ok(req)
    }

    /// Execute one built request. Returns the raw status + body for every
    /// completed request; only network/JS failures become `Err`.
    async fn perform(&self, req: &Request) -> Result<(u16, String), CloudError> {
        let window = web_sys::window().ok_or_else(|| CloudError::Transport("no window".into()))?;
        let resp_value = JsFuture::from(window.fetch_with_request(req))
            .await
            .map_err(|e| CloudError::Transport(format!("fetch: {:?}", e)))?;
        let resp: Response = resp_value
            .dyn_into()
            .map_err(|e| CloudError::Transport(format!("response cast: {:?}", e)))?;

        let status = resp.status();
        let text = JsFuture::from(
            resp.text()
                .map_err(|e| CloudError::Transport(format!("body(): {:?}", e)))?,
        )
        .await
        .map_err(|e| CloudError::Transport(format!("read body: {:?}", e)))?;
        let body = text
            .as_string()
            .ok_or_else(|| CloudError::Decode("body is not a string".into()))?;
        Ok((status, body))
    }

    /// One HTTP exchange (JSON body or none, bearer attached when
    /// configured). Callers map statuses (`poll` needs its own 202/410
    /// handling, everything else uses [`decode_success`]).
    async fn exchange(
        &self,
        method: &str,
        path: &str,
        body: Option<String>,
        extra_headers: &[(&str, &str)],
    ) -> Result<(u16, String), CloudError> {
        let json_body = body.is_some();
        let req = self.build_request(
            method,
            path,
            body.map(|b| JsValue::from_str(&b)),
            json_body,
            true,
            extra_headers,
        )?;
        self.perform(&req).await
    }

    /// One multipart POST (`POST /api/share/screenshot`). No Content-Type —
    /// the browser fills in `multipart/form-data; boundary=…` from the
    /// [`web_sys::FormData`] body.
    async fn exchange_multipart(
        &self,
        path: &str,
        form: web_sys::FormData,
    ) -> Result<(u16, String), CloudError> {
        let req = self.build_request("POST", path, Some(form.into()), false, true, &[])?;
        self.perform(&req).await
    }

    /// Map a completed exchange onto a typed body, or a semantic error.
    fn decode_success<T: serde::de::DeserializeOwned>(
        &self,
        status: u16,
        body: &str,
    ) -> Result<T, CloudError> {
        match status {
            200..=299 => serde_json::from_str(body)
                .map_err(|e| CloudError::Decode(format!("{}: {}", e, body))),
            401 => Err(CloudError::Unauthorized),
            403 => Err(CloudError::Forbidden(body.to_string())),
            _ => Err(CloudError::Http {
                status,
                body: body.to_string(),
            }),
        }
    }

    // ── /api/sync/* ─────────────────────────────────────────────────────

    /// `GET /api/sync/status`
    pub async fn sync_status(&self) -> Result<SyncStatusResponse, CloudError> {
        let (status, body) = self.exchange("GET", "/api/sync/status", None, &[]).await?;
        self.decode_success(status, &body)
    }

    /// `GET /api/sync/changes?since=T` — metadata only, no content.
    pub async fn sync_changes(&self, since: i64) -> Result<ChangesResponse, CloudError> {
        let (status, body) = self
            .exchange("GET", &format!("/api/sync/changes?since={}", since), None, &[])
            .await?;
        self.decode_success(status, &body)
    }

    /// `POST /api/sync/push` — caller must chunk to <= 50 items (server cap).
    pub async fn sync_push(&self, items: Vec<SyncItem>) -> Result<PushResponse, CloudError> {
        let payload = serde_json::to_string(&PushRequest { items })
            .map_err(|e| CloudError::Decode(e.to_string()))?;
        let (status, body) = self
            .exchange("POST", "/api/sync/push", Some(payload), &[])
            .await?;
        self.decode_success(status, &body)
    }

    /// `GET /api/sync/pull` — full snapshot of the profile namespace.
    pub async fn sync_pull(&self) -> Result<PullResponse, CloudError> {
        let (status, body) = self.exchange("GET", "/api/sync/pull", None, &[]).await?;
        self.decode_success(status, &body)
    }

    /// `DELETE /api/sync/item/:sync_id` — soft-delete one item.
    pub async fn sync_delete_item(&self, sync_id: &str) -> Result<(), CloudError> {
        let (status, body) = self
            .exchange("DELETE", &format!("/api/sync/item/{}", sync_id), None, &[])
            .await?;
        self.decode_success(status, &body).map(|_: serde_json::Value| ())
    }

    /// `DELETE /api/sync/all` — hard-delete the user's cloud data.
    /// Sends the server-required `X-Confirm-Delete: yes` header.
    pub async fn sync_delete_all(&self) -> Result<u64, CloudError> {
        let (status, body) = self
            .exchange(
                "DELETE",
                "/api/sync/all",
                None,
                &[("X-Confirm-Delete", "yes")],
            )
            .await?;
        let resp: DeleteAllResponse = self.decode_success(status, &body)?;
        Ok(resp.deleted)
    }

    /// `GET /api/sync/e2e-params` — `Ok(None)` when vault recovery is not
    /// set up for this account.
    pub async fn e2e_params(&self) -> Result<Option<VaultParams>, CloudError> {
        let (status, body) = self.exchange("GET", "/api/sync/e2e-params", None, &[]).await?;
        let resp: E2EParamsResponse = self.decode_success(status, &body)?;
        if !resp.enabled {
            return Ok(None);
        }
        match (resp.salt, resp.iterations) {
            (Some(salt), Some(iterations)) => Ok(Some(VaultParams {
                salt,
                iterations,
                encrypted_master_key: resp.encrypted_master_key,
            })),
            _ => Err(CloudError::Decode(format!(
                "enabled e2e-params without salt/iterations: {}",
                body
            ))),
        }
    }

    /// `POST /api/sync/e2e-setup` — initialise/replace E2E parameters.
    pub async fn e2e_setup(&self, params: &E2ESetupRequest) -> Result<(), CloudError> {
        let payload =
            serde_json::to_string(params).map_err(|e| CloudError::Decode(e.to_string()))?;
        let (status, body) = self
            .exchange("POST", "/api/sync/e2e-setup", Some(payload), &[])
            .await?;
        self.decode_success(status, &body).map(|_: serde_json::Value| ())
    }

    // ── /api/auth/* ─────────────────────────────────────────────────────

    /// `POST /api/auth/device-code` — start the device-code login flow.
    /// Anonymous (no token required).
    pub async fn device_code_create(&self) -> Result<DeviceCodeResponse, CloudError> {
        let payload = serde_json::to_string(&DeviceCodeRequest {
            device_id: self.config.device_id.clone(),
        })
        .map_err(|e| CloudError::Decode(e.to_string()))?;
        let (status, body) = self
            .exchange("POST", "/api/auth/device-code", Some(payload), &[])
            .await?;
        self.decode_success(status, &body)
    }

    /// `GET /api/auth/poll?code=…` — 202 → [`DeviceAuthPoll::Pending`],
    /// 410 → [`DeviceAuthPoll::Expired`], 200 → [`DeviceAuthPoll::Confirmed`].
    pub async fn device_code_poll(&self, code: &str) -> Result<DeviceAuthPoll, CloudError> {
        let (status, body) = self
            .exchange("GET", &format!("/api/auth/poll?code={}", code), None, &[])
            .await?;
        match status {
            200 | 202 | 410 => serde_json::from_str(&body)
                .map_err(|e| CloudError::Decode(format!("{}: {}", e, body))),
            _ => self.decode_success(status, &body),
        }
    }

    /// `GET /api/auth/me` — account info for the current token.
    pub async fn auth_me(&self) -> Result<MeResponse, CloudError> {
        let (status, body) = self.exchange("GET", "/api/auth/me", None, &[]).await?;
        self.decode_success(status, &body)
    }

    /// `POST /api/auth/logout` — revoke the current token server-side.
    pub async fn auth_logout(&self) -> Result<(), CloudError> {
        let (status, body) = self.exchange("POST", "/api/auth/logout", None, &[]).await?;
        self.decode_success(status, &body).map(|_: serde_json::Value| ())
    }

    /// `GET /api/entitlements` — the resolved feature set for the current
    /// caller. Answers for an anonymous caller too (no token attached =
    /// the free set, not a 401), so this never needs a login gate the way
    /// `auth_me` does.
    pub async fn entitlements(&self) -> Result<EntitlementsResponse, CloudError> {
        let (status, body) = self.exchange("GET", "/api/entitlements", None, &[]).await?;
        self.decode_success(status, &body)
    }

    // ── /api/account/* ────────────────────────────────────────────────────

    /// `POST /api/account/redeem` — spend a gift/trial code on the signed-in
    /// account, returning the new paid-until instant.
    ///
    /// Failures keep their status: 404 is "no such code", 409 is "already
    /// spent", 401 is "not signed in". Turning those into a sentence is
    /// [`redeem_error_message`]'s job, not the transport's — the wording
    /// belongs next to the screen that prints it.
    pub async fn redeem(&self, code: &str) -> Result<RedeemResponse, CloudError> {
        let payload = serde_json::to_string(&serde_json::json!({ "code": code }))
            .map_err(|e| CloudError::Decode(e.to_string()))?;
        let (status, body) = self
            .exchange("POST", "/api/account/redeem", Some(payload), &[])
            .await?;
        self.decode_success(status, &body)
    }

    /// `POST /api/account/nick` — set the account's nickname. Format is
    /// checked once on the client, in `mlc_app_layout::ui::modal_settings::
    /// validate_nick`, before the submission is ever armed; the server
    /// re-checks regardless and answers 400/409/429 its own way.
    /// [`crate::nick_error_message`] turns a refusal into the sentence the
    /// modal prints.
    pub async fn set_nick(&self, nick: &str) -> Result<SetNickResponse, CloudError> {
        let payload = serde_json::to_string(&SetNickRequest { nick: nick.to_string() })
            .map_err(|e| CloudError::Decode(e.to_string()))?;
        let (status, body) = self
            .exchange("POST", "/api/account/nick", Some(payload), &[])
            .await?;
        self.decode_success(status, &body)
    }

    // ── /api/share/* ────────────────────────────────────────────────────

    /// `POST /api/share/live` — freeze the active preset into a public
    /// link. 403 is the per-kind active-share ceiling (free 1 / paid 10
    /// live shares) — the caller words it for the user.
    pub async fn share_create_live(
        &self,
        title: &str,
        preset: serde_json::Value,
    ) -> Result<ShareCreateResponse, CloudError> {
        let payload = serde_json::to_string(&ShareCreateLiveRequest {
            title: title.to_string(),
            preset,
        })
        .map_err(|e| CloudError::Decode(e.to_string()))?;
        let (status, body) = self
            .exchange("POST", "/api/share/live", Some(payload), &[])
            .await?;
        self.decode_success(status, &body)
    }

    /// `POST /api/share/screenshot` — multipart upload, one `png` field
    /// plus an optional `title`. Same response shape as
    /// [`Self::share_create_live`]; ceiling free 10 / paid 100.
    pub async fn share_create_screenshot(
        &self,
        title: &str,
        png: web_sys::Blob,
    ) -> Result<ShareCreateResponse, CloudError> {
        let form = web_sys::FormData::new()
            .map_err(|e| CloudError::Transport(format!("FormData::new: {:?}", e)))?;
        form.append_with_blob("png", &png)
            .map_err(|e| CloudError::Transport(format!("FormData png: {:?}", e)))?;
        if !title.is_empty() {
            form.append_with_str("title", title)
                .map_err(|e| CloudError::Transport(format!("FormData title: {:?}", e)))?;
        }
        let (status, body) = self.exchange_multipart("/api/share/screenshot", form).await?;
        self.decode_success(status, &body)
    }

    /// `GET /api/share/mine` — the caller's own active shares.
    pub async fn share_list(&self) -> Result<Vec<ShareListEntry>, CloudError> {
        let (status, body) = self.exchange("GET", "/api/share/mine", None, &[]).await?;
        let resp: ShareListResponse = self.decode_success(status, &body)?;
        Ok(resp.shares)
    }

    /// `DELETE /api/share/{token}` — owner revoke.
    pub async fn share_revoke(&self, token: &str) -> Result<(), CloudError> {
        let (status, body) = self
            .exchange("DELETE", &format!("/api/share/{}", token), None, &[])
            .await?;
        self.decode_success(status, &body)
            .map(|_: serde_json::Value| ())
    }

    /// `GET /api/share/{token}` — PUBLIC: deliberately goes out without
    /// the configured bearer token (see [`Self::build_request`]'s `auth`
    /// gate). Unknown/expired/revoked tokens answer the same bare 404,
    /// which surfaces here as [`CloudError::Http`] with status 404.
    pub async fn share_get_public(&self, token: &str) -> Result<SharePublicResponse, CloudError> {
        let req = self.build_request("GET", &format!("/api/share/{}", token), None, false, false, &[])?;
        let (status, body) = self.perform(&req).await?;
        self.decode_success(status, &body)
    }

    // ── /api/updates/* (anonymous) ────────────────────────────────────────

    /// `GET /api/updates/latest?platform=…` — release manifest for the
    /// Download overlay. Anonymous, no attestation required.
    pub async fn updates_latest(&self, platform: &str) -> Result<VersionManifest, CloudError> {
        let (status, body) = self
            .exchange("GET", &format!("/api/updates/latest?platform={}", platform), None, &[])
            .await?;
        self.decode_success(status, &body)
    }

    /// `GET /api/updates/list?channel=…` — every published release, every
    /// platform, newest first. Drives the Download overlay's expanded list;
    /// `updates_latest` above stays what the collapsed single-platform view
    /// (and the OTA updater) asks for.
    pub async fn updates_list(&self, channel: &str) -> Result<Vec<ReleaseEntry>, CloudError> {
        let (status, body) = self
            .exchange("GET", &format!("/api/updates/list?channel={}", channel), None, &[])
            .await?;
        let resp: ReleaseListResponse = self.decode_success(status, &body)?;
        Ok(resp.releases)
    }

    /// `GET /api/posts?limit=100` — recent blog feed for the News overlay,
    /// newest first. Same-origin, anonymous. Replaces the old static
    /// `GET /blog/posts.json` fetch (still served by the server from the
    /// same store, so an unrebuilt client keeps working); `next_before` is
    /// ignored — the overlay has no "load more" yet.
    pub async fn blog_posts(&self) -> Result<Vec<BlogPost>, CloudError> {
        let (status, body) = self.exchange("GET", "/api/posts?limit=100", None, &[]).await?;
        let feed: BlogFeed = self.decode_success(status, &body)?;
        Ok(feed.posts)
    }
}
