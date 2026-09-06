//! Chart sharing (`POST /api/share/live`, `POST /api/share/screenshot`) —
//! the native twin of `mlc-cloud`'s wasm transport
//! (`crates/mlc-cloud/src/wasm.rs`'s own `share_create_live`/
//! `share_create_screenshot`). Mirrors `nick.rs`/`forum.rs`'s shape: thin
//! request/parse functions, with the publish-and-log decision left to the
//! caller in `lib.rs`.
//!
//! Both endpoints answer the same response shape
//! (`{"token","url_path","expires_at"}`) and the same per-kind active-share
//! ceiling on 403 (free 1 live / 10 screenshot, paid 10 / 100) — one
//! [`ShareCreated`] type and one [`error_message`] cover both.

use crate::UPDATE_SERVER;

/// Response body from `POST /api/share/live` and `POST /api/share/screenshot`.
///
/// `token`/`expires_at` are not modeled — same call `nick.rs`'s
/// `SetNickResponse` makes for `nick`: the caller only ever wants
/// `url_path` (to build the absolute link), and serde ignores the wire's
/// other fields on deserialize.
#[derive(serde::Deserialize)]
pub struct ShareCreated {
    /// Root-relative link target (`/?share=<token>`) — the caller prefixes
    /// `UPDATE_SERVER` before handing it to a clipboard, mirroring
    /// `mlc-app-shell-web`'s own `absolute_share_url`.
    pub url_path: String,
}

/// Server's own error body shape on a non-success response, e.g.
/// `{"error": "That title is too long"}`.
#[derive(serde::Deserialize)]
struct ErrorBody {
    error: String,
}

/// Turn a non-success response into the sentence the caller publishes.
/// Shared by every share endpoint in this module — 403 is the per-kind
/// active-share ceiling on create and the missing-`clone`-right refusal on
/// clone, worded the same way `mlc-app-shell-web::cloud_session::
/// share_error_message` words it for the wasm shell; 404 is the one answer
/// `GET`/`POST .../clone` give an unknown, expired, or revoked token alike
/// (`routes/share.rs`'s `share_not_found`) — none of the three is
/// distinguishable from here, by design.
async fn error_message(resp: reqwest::Response) -> String {
    let status = resp.status();
    if status.as_u16() == 403 {
        return "share limit reached — revoke an existing share or upgrade".to_string();
    }
    if status.as_u16() == 401 {
        return "Sign in again to share".to_string();
    }
    if status.as_u16() == 404 {
        return "share not found or expired".to_string();
    }
    match resp.json::<ErrorBody>().await {
        Ok(b) => b.error,
        Err(_) => format!("Server returned {}", status),
    }
}

/// `POST /api/share/live` — freeze `preset_json` (a serialized `ChartPreset`)
/// into a public link.
pub async fn share_live(
    client: &reqwest::Client,
    auth_header: &str,
    title: &str,
    preset_json: serde_json::Value,
) -> Result<ShareCreated, String> {
    let url = format!("{}/api/share/live", UPDATE_SERVER);
    let resp = client
        .post(&url)
        .header("Authorization", auth_header)
        .json(&serde_json::json!({ "title": title, "preset": preset_json }))
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    if !resp.status().is_success() {
        return Err(error_message(resp).await);
    }
    resp.json::<ShareCreated>().await.map_err(|e| format!("Parse error: {}", e))
}

/// `POST /api/share/screenshot` — multipart upload, one `png` field plus an
/// optional `title`.
pub async fn share_screenshot(
    client: &reqwest::Client,
    auth_header: &str,
    title: &str,
    png_bytes: Vec<u8>,
) -> Result<ShareCreated, String> {
    let url = format!("{}/api/share/screenshot", UPDATE_SERVER);
    let part = reqwest::multipart::Part::bytes(png_bytes)
        .file_name("chart.png")
        .mime_str("image/png")
        .map_err(|e| format!("multipart part error: {}", e))?;
    let mut form = reqwest::multipart::Form::new().part("png", part);
    if !title.is_empty() {
        form = form.text("title", title.to_string());
    }

    let resp = client
        .post(&url)
        .header("Authorization", auth_header)
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    if !resp.status().is_success() {
        return Err(error_message(resp).await);
    }
    resp.json::<ShareCreated>().await.map_err(|e| format!("Parse error: {}", e))
}

// ============================================================================
// GET /api/share/{token} and POST /api/share/{token}/clone — native open path
// ============================================================================
//
// Native twin of `mlc-app::share_viewer`'s job (native-share-viewer arc,
// `docs/mlc/plans/native-share-viewer-and-chart-rights-2026-09-06.md`
// §1/§6 step 4): this module fetches, `share_viewer` restores. Kept as
// serde-only mirrors of `routes/share.rs`'s response shapes rather than
// `mlc_core::preset::preset::ChartPreset` itself — the caller (the shell)
// owns the version check and the `SharedChartMeta` construction, same
// division of labor `ShareCreated` already draws with `share_live`/
// `share_screenshot` above.

/// `GET /api/share/{token}` response — the server's live-share body
/// (`routes/share.rs::get_share`'s `"live"` arm). A `"screenshot"` share
/// answers the same envelope with `preset: None` (that kind carries no
/// preset to open) — the caller checks `kind` before treating a `None`
/// preset as an error.
#[derive(serde::Deserialize)]
pub struct SharedChartResponse {
    /// `"live"` or `"screenshot"`.
    pub kind: String,
    #[serde(default)]
    pub title: String,
    /// Owner's display label — empty when the server has none on file.
    #[serde(default)]
    pub owner: String,
    /// The frozen `ChartPreset`, still-serialized JSON — only present for a
    /// `"live"` share. The caller deserializes it into `ChartPreset` itself
    /// (this crate does not depend on `mlc-app`, so it cannot name that
    /// type here — see this module's own header doc).
    #[serde(default)]
    pub preset: Option<serde_json::Value>,
    /// RFC 3339 instant after which the share stops resolving.
    #[serde(default)]
    pub expires_at: String,
    /// Effective rights for the resolved caller (anonymous or bearer) —
    /// `["read"]`, `["read","clone"]`, or `["read","clone","edit"]` for the
    /// owner. Empty only on a malformed/old server response.
    #[serde(default)]
    pub rights: Vec<String>,
}

/// Non-preset half of [`SharedChartResponse`] — what
/// `state::ShareOpenState::Opened` carries as `meta`, with the preset itself
/// split out into that variant's own `preset_json` field (mirrors
/// `mlc_app::share_viewer::SharedChartMeta`'s own shape, which likewise has
/// no preset field — the preset is what THAT struct describes, not part of
/// it). Built by the caller from a fetched [`SharedChartResponse`], not
/// deserialized on its own.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SharedChartMetaWire {
    /// `"live"` or `"screenshot"` — only `"live"` carries a preset to open.
    pub kind: String,
    pub title: String,
    pub owner_label: String,
    /// `["read"]`, `["read","clone"]`, or `["read","clone","edit"]`.
    pub rights: Vec<String>,
    /// RFC 3339 instant after which the share stops resolving.
    pub expires_at: String,
}

/// One indicator instance `POST /api/share/{token}/clone` dropped because
/// its `type_id` names a paid/private vendor indicator the caller holds no
/// grant for. Mirrors `routes/share.rs`'s `{"type_id","name"}` element —
/// wire-shape twin of `mlc_app::share_viewer::StrippedIndicator`, which this
/// crate cannot name directly (no `mlc-app` dependency).
#[derive(serde::Deserialize, Clone, Debug, PartialEq, Default)]
pub struct StrippedIndicatorWire {
    pub type_id: String,
    #[serde(default)]
    pub name: String,
}

/// `POST /api/share/{token}/clone` response.
#[derive(serde::Deserialize)]
pub struct ClonePresetResponse {
    /// The cloned preset, already stripped of any ungranted indicator —
    /// still-serialized JSON, same reasoning as
    /// [`SharedChartResponse::preset`].
    pub preset: serde_json::Value,
    #[serde(default)]
    pub stripped: Vec<StrippedIndicatorWire>,
}

/// `GET /api/share/{token}` — public read of a share link's metadata plus
/// its frozen preset (for a `"live"` share). `auth_header` is OPTIONAL: an
/// anonymous caller still gets the full response (the token is the
/// capability), just without `"clone"`/`"edit"` in `rights` unless the link
/// itself opted in. Unknown, expired, and revoked tokens all answer the
/// same [`error_message`] wording (bare 404 upstream).
pub async fn get_share(
    client: &reqwest::Client,
    auth_header: Option<&str>,
    token: &str,
) -> Result<SharedChartResponse, String> {
    let url = format!("{}/api/share/{}", UPDATE_SERVER, token);
    let mut req = client.get(&url);
    if let Some(auth) = auth_header {
        req = req.header("Authorization", auth);
    }
    let resp = req.send().await.map_err(|e| format!("Request failed: {}", e))?;

    if !resp.status().is_success() {
        return Err(error_message(resp).await);
    }
    resp.json::<SharedChartResponse>().await.map_err(|e| format!("Parse error: {}", e))
}

/// `POST /api/share/{token}/clone` — auth required. 403 when the caller's
/// effective rights on this token do not include `"clone"`; 400 when the
/// token names a `"screenshot"` share (nothing to clone); same bare-404
/// discipline as [`get_share`] for an unknown/expired/revoked token.
pub async fn clone_share(
    client: &reqwest::Client,
    auth_header: &str,
    token: &str,
) -> Result<ClonePresetResponse, String> {
    let url = format!("{}/api/share/{}/clone", UPDATE_SERVER, token);
    let resp = client
        .post(&url)
        .header("Authorization", auth_header)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    if !resp.status().is_success() {
        return Err(error_message(resp).await);
    }
    resp.json::<ClonePresetResponse>().await.map_err(|e| format!("Parse error: {}", e))
}
