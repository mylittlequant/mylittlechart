//! Forum board client (`GET/POST/DELETE /api/forum/*`) — paid-only
//! communication board, gated server-side by `Feature::Forum` plus
//! `user_is_paid` on top of the usual auth (401 anonymous, 403 free).
//!
//! Mirrors `nick.rs`/`redeem.rs`'s shape: thin request/parse functions, with
//! the publish-and-log decision left to the caller in `lib.rs`. Six
//! endpoints share this one file (rather than `nick.rs`/`redeem.rs`'s
//! one-file-one-endpoint split) because they all speak to the same board and
//! the error-body parsing below is identical across every one of them.

use crate::UPDATE_SERVER;

/// One row of `GET /api/forum/threads`.
#[derive(serde::Deserialize, Clone, Debug, PartialEq)]
pub struct ForumThreadSummary {
    pub id: i64,
    pub title: String,
    pub author: String,
    #[serde(default)]
    pub author_is_me: bool,
    pub created_at: String,
    #[serde(default)]
    pub last_reply_at: Option<String>,
    #[serde(default)]
    pub reply_count: i64,
    #[serde(default)]
    pub excerpt: String,
}

/// One reply in `GET /api/forum/threads/{id}`'s `replies` array.
#[derive(serde::Deserialize, Clone, Debug, PartialEq)]
pub struct ForumReply {
    pub id: i64,
    pub author: String,
    #[serde(default)]
    pub author_is_me: bool,
    pub body: String,
    pub created_at: String,
}

/// A thread with its replies — the flattened shape [`crate::state::ForumState::thread`]
/// carries. The server nests `thread`/`replies` as two sibling fields
/// ([`ThreadWire`] below); [`get_thread`] merges them into one value.
#[derive(Clone, Debug, PartialEq)]
pub struct ForumThread {
    pub id: i64,
    pub title: String,
    pub author: String,
    pub author_is_me: bool,
    pub body: String,
    pub created_at: String,
    pub replies: Vec<ForumReply>,
}

#[derive(serde::Deserialize)]
struct ThreadsWire {
    threads: Vec<ForumThreadSummary>,
    #[serde(default)]
    next_before: Option<String>,
}

#[derive(serde::Deserialize)]
struct ThreadDetailWire {
    id: i64,
    title: String,
    author: String,
    #[serde(default)]
    author_is_me: bool,
    body: String,
    created_at: String,
}

#[derive(serde::Deserialize)]
struct ThreadWire {
    thread: ThreadDetailWire,
    replies: Vec<ForumReply>,
}

#[derive(serde::Deserialize)]
struct IdWire {
    id: i64,
}

/// Server's own error body shape on a non-success response, e.g.
/// `{"error": "That title is too short"}`.
#[derive(serde::Deserialize)]
struct ErrorBody {
    error: String,
}

/// The 429 body additionally carries how long to wait.
#[derive(serde::Deserialize, Default)]
struct RateLimitBody {
    #[serde(default)]
    error: String,
    #[serde(default)]
    retry_after_secs: u64,
}

/// Turn a non-success response into the sentence the overlay prints. Shared
/// by every function below — all six forum endpoints answer
/// 400/401/403/429 the same way, unlike `nick.rs`/`redeem.rs`'s
/// per-endpoint bodies. Mirrors `mlc_cloud::forum_error_message`'s wasm
/// twin exactly, so the two builds never say a different thing for the
/// same server answer.
async fn error_message(resp: reqwest::Response) -> String {
    let status = resp.status();
    if status.as_u16() == 429 {
        return match resp.json::<RateLimitBody>().await {
            Ok(b) if b.retry_after_secs > 0 => format!("Try again in {}s", b.retry_after_secs),
            Ok(b) if !b.error.is_empty() => b.error,
            _ => "Too many requests — try again shortly".to_string(),
        };
    }
    if status.as_u16() == 403 {
        return "Forum access requires a paid account".to_string();
    }
    if status.as_u16() == 401 {
        return "Sign in again, then retry".to_string();
    }
    match resp.json::<ErrorBody>().await {
        Ok(b) => b.error,
        Err(_) => format!("Server returned {}", status),
    }
}

/// `GET /api/forum/threads?limit=20&before=…` — `before` is the previous
/// page's `next_before` cursor (`None` for the first page). Returns the
/// page's rows plus the next cursor (`None` once the list has run out).
pub async fn list_threads(
    client: &reqwest::Client,
    auth_header: &str,
    before: Option<&str>,
) -> Result<(Vec<ForumThreadSummary>, Option<String>), String> {
    let url = match before {
        Some(b) => format!("{}/api/forum/threads?limit=20&before={}", UPDATE_SERVER, urlencode(b)),
        None => format!("{}/api/forum/threads?limit=20", UPDATE_SERVER),
    };
    let resp = client
        .get(&url)
        .header("Authorization", auth_header)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;
    if !resp.status().is_success() {
        return Err(error_message(resp).await);
    }
    let wire: ThreadsWire = resp.json().await.map_err(|e| format!("Parse error: {}", e))?;
    Ok((wire.threads, wire.next_before))
}

/// `GET /api/forum/threads/{id}` — the thread plus its replies, in order.
pub async fn get_thread(client: &reqwest::Client, auth_header: &str, id: i64) -> Result<ForumThread, String> {
    let url = format!("{}/api/forum/threads/{}", UPDATE_SERVER, id);
    let resp = client
        .get(&url)
        .header("Authorization", auth_header)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;
    if !resp.status().is_success() {
        return Err(error_message(resp).await);
    }
    let wire: ThreadWire = resp.json().await.map_err(|e| format!("Parse error: {}", e))?;
    Ok(ForumThread {
        id: wire.thread.id,
        title: wire.thread.title,
        author: wire.thread.author,
        author_is_me: wire.thread.author_is_me,
        body: wire.thread.body,
        created_at: wire.thread.created_at,
        replies: wire.replies,
    })
}

/// `POST /api/forum/threads` — 400 title 3–120 / body 1–8192 bytes, 429 at
/// one thread per 60s. Returns the new thread's id.
pub async fn create_thread(client: &reqwest::Client, auth_header: &str, title: &str, body: &str) -> Result<i64, String> {
    let url = format!("{}/api/forum/threads", UPDATE_SERVER);
    let resp = client
        .post(&url)
        .header("Authorization", auth_header)
        .json(&serde_json::json!({ "title": title, "body": body }))
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;
    if !resp.status().is_success() {
        return Err(error_message(resp).await);
    }
    resp.json::<IdWire>().await.map(|w| w.id).map_err(|e| format!("Parse error: {}", e))
}

/// `POST /api/forum/threads/{id}/replies` — 429 at ten replies per 60s.
/// Returns the new reply's id.
pub async fn create_reply(client: &reqwest::Client, auth_header: &str, thread_id: i64, body: &str) -> Result<i64, String> {
    let url = format!("{}/api/forum/threads/{}/replies", UPDATE_SERVER, thread_id);
    let resp = client
        .post(&url)
        .header("Authorization", auth_header)
        .json(&serde_json::json!({ "body": body }))
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;
    if !resp.status().is_success() {
        return Err(error_message(resp).await);
    }
    resp.json::<IdWire>().await.map(|w| w.id).map_err(|e| format!("Parse error: {}", e))
}

/// `DELETE /api/forum/threads/{id}` — 204 for the author, 403 otherwise.
pub async fn delete_thread(client: &reqwest::Client, auth_header: &str, id: i64) -> Result<(), String> {
    let url = format!("{}/api/forum/threads/{}", UPDATE_SERVER, id);
    let resp = client
        .delete(&url)
        .header("Authorization", auth_header)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;
    if !resp.status().is_success() {
        return Err(error_message(resp).await);
    }
    Ok(())
}

/// `DELETE /api/forum/replies/{id}` — 204 for the author, 403 otherwise.
pub async fn delete_reply(client: &reqwest::Client, auth_header: &str, id: i64) -> Result<(), String> {
    let url = format!("{}/api/forum/replies/{}", UPDATE_SERVER, id);
    let resp = client
        .delete(&url)
        .header("Authorization", auth_header)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;
    if !resp.status().is_success() {
        return Err(error_message(resp).await);
    }
    Ok(())
}

/// Percent-encode a query-string component — used only for the `before`
/// pagination cursor, which may carry an RFC3339 timestamp (`:`, `+`) that
/// must not travel raw in a URL. No `url`/`percent-encoding` crate pulled in
/// for one field — mirrors `mlc_cloud::wasm::encode_query_component`'s wasm
/// twin exactly.
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}
