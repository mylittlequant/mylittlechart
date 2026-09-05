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
/// Shared by both share-create endpoints — 403 is the per-kind active-share
/// ceiling on both, worded the same way `mlc-app-shell-web::cloud_session::
/// share_error_message` words it for the wasm shell.
async fn error_message(resp: reqwest::Response) -> String {
    let status = resp.status();
    if status.as_u16() == 403 {
        return "share limit reached — revoke an existing share or upgrade".to_string();
    }
    if status.as_u16() == 401 {
        return "Sign in again to share".to_string();
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
