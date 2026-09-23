//! Forum engine v2 client (`GET/POST/DELETE /api/forum/*`,
//! `/api/forum/v2/*`) — public boards, role-gated read/write, threaded
//! replies, moderation, and search. Mirrors `mylittlechart-server/crates/
//! server/src/routes/forum_v2.rs` + `social_db.rs`, and the native twin of
//! `mlc_cloud::wasm::FetchTransport`'s own `forum_boards`/`forum_thread_v2`/
//! etc.
//!
//! Unlike v1's `forum.rs` (always paid, always a bearer), NOT every route
//! here requires auth: a board's own `read_class` decides per board, and a
//! `public` board answers a signed-out caller. Every function below takes
//! `auth_header: Option<&str>` for that reason — `None` sends the request
//! with no `Authorization` header at all, exactly as an anonymous browser
//! caller would (same optionality `share.rs::get_share` already uses for
//! its own anonymous-ok read). `updater_loop` decides whether a `None` is
//! actually allowed for a given action — write/moderate commands refuse
//! locally with "Sign in first" before ever calling in here.

use crate::UPDATE_SERVER;

/// One row of `GET /api/forum/boards`.
#[derive(serde::Deserialize, Clone, Debug, PartialEq)]
pub struct ForumBoard {
    pub id: i64,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub read_class: String,
    pub write_class: String,
    pub position: i64,
    /// Total threads on this board. `#[serde(default)]` — same reasoning
    /// as `mlc_cloud::models::ForumBoard`'s own copy of these four fields
    /// (community-modal arc W2): the server change landing them is a
    /// separate in-flight deploy.
    #[serde(default)]
    pub thread_count: i64,
    /// Total replies across every thread on this board.
    #[serde(default)]
    pub reply_count: i64,
    /// Timestamp of the board's most recent thread or reply, whichever is
    /// newer — `None` for a board with no activity yet, or an older server.
    #[serde(default)]
    pub last_post_at: Option<String>,
    /// Title of the thread that carries `last_post_at`.
    #[serde(default)]
    pub last_thread_title: Option<String>,
}

/// One row of `GET /api/forum/boards/{slug}/threads`.
#[derive(serde::Deserialize, Clone, Debug, PartialEq)]
pub struct ForumThreadV2Summary {
    pub id: i64,
    pub board_id: i64,
    pub title: String,
    pub author: String,
    #[serde(default)]
    pub author_is_me: bool,
    /// The thread starter's ladder rank (`"user"|"paid"|"trusted"|
    /// "moderator"|"admin"`), or `None` for an unresolved/anonymous author.
    /// `#[serde(default)]` — same soft-rollout idiom as `ForumBoard::
    /// thread_count`'s own copy: the server change landing this field is a
    /// separate in-flight deploy.
    #[serde(default)]
    pub author_rank: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default)]
    pub last_reply_at: Option<String>,
    #[serde(default)]
    pub reply_count: i64,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub locked: bool,
}

/// One reply node in a thread's reply tree — mirrors
/// `mlc_cloud::ForumReplyNode` field-for-field (this crate does not depend
/// on `mlc-cloud`; its native transport does not exist there — see that
/// crate's own module doc).
#[derive(serde::Deserialize, Clone, Debug, PartialEq)]
pub struct ForumReplyNode {
    pub id: i64,
    #[serde(default)]
    pub parent_reply_id: Option<i64>,
    #[serde(default)]
    pub depth: i64,
    pub author: String,
    #[serde(default)]
    pub author_is_me: bool,
    /// See `ForumThreadV2Summary::author_rank`'s own doc.
    #[serde(default)]
    pub author_rank: Option<String>,
    pub body: String,
    pub created_at: String,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub children: Vec<ForumReplyNode>,
}

/// A thread plus its reply tree — the flattened shape
/// `GET /api/forum/v2/threads/{id}` answers (`thread`/`replies` siblings,
/// merged here the same way v1's `forum::get_thread` merges its own).
#[derive(Clone, Debug, PartialEq)]
pub struct ForumThreadV2 {
    pub id: i64,
    pub board_id: i64,
    pub title: String,
    pub author: String,
    pub author_is_me: bool,
    /// See `ForumThreadV2Summary::author_rank`'s own doc.
    pub author_rank: Option<String>,
    pub body: String,
    pub created_at: String,
    pub updated_at: String,
    pub pinned: bool,
    pub locked: bool,
    pub hidden: bool,
    pub replies: Vec<ForumReplyNode>,
}

/// One hit in `GET /api/forum/search`'s `results` array.
#[derive(serde::Deserialize, Clone, Debug, PartialEq)]
pub struct ForumSearchHit {
    pub kind: String,
    pub board_id: i64,
    pub thread_id: i64,
    #[serde(default)]
    pub reply_id: Option<i64>,
    #[serde(default)]
    pub title: Option<String>,
    pub snippet: String,
    pub author: String,
    pub created_at: String,
}

#[derive(serde::Deserialize)]
struct BoardsWire {
    boards: Vec<ForumBoard>,
}

#[derive(serde::Deserialize)]
struct BoardThreadsWire {
    threads: Vec<ForumThreadV2Summary>,
    #[serde(default)]
    next_before: Option<String>,
}

#[derive(serde::Deserialize)]
struct ThreadDetailWire {
    id: i64,
    board_id: i64,
    title: String,
    author: String,
    #[serde(default)]
    author_is_me: bool,
    #[serde(default)]
    author_rank: Option<String>,
    body: String,
    created_at: String,
    updated_at: String,
    #[serde(default)]
    pinned: bool,
    #[serde(default)]
    locked: bool,
    #[serde(default)]
    hidden: bool,
}

#[derive(serde::Deserialize)]
struct ThreadWire {
    thread: ThreadDetailWire,
    replies: Vec<ForumReplyNode>,
}

#[derive(serde::Deserialize)]
struct IdWire {
    id: i64,
}

#[derive(serde::Deserialize)]
struct SearchWire {
    results: Vec<ForumSearchHit>,
}

/// `GET /api/forum/me`'s response — mirrors `mlc_cloud::ForumMeResponse`
/// field-for-field (this crate does not depend on `mlc-cloud`; its native
/// transport does not exist there — see that crate's own module doc).
#[derive(serde::Deserialize, Clone, Debug, PartialEq, Default)]
pub struct ForumMe {
    pub classes: Vec<String>,
    pub roles: Vec<String>,
    /// `null` for an anonymous caller (the server answers `label: null,
    /// public_id: null` then) — hence `Option`, not `String`.
    pub label: Option<String>,
    pub public_id: Option<String>,
    /// The caller's chosen nickname, or `null` for a signed-in account that
    /// has never set one — `#[serde(default)]` since the server change
    /// landing this field is a separate in-flight deploy (same reasoning as
    /// `ForumBoard::thread_count`'s own copy of this idiom). A signed-in,
    /// nick-less account is gated off forum/DM writing (owner mandate
    /// 2026-09-09, `nick_required` 403s) — the client reads this to show
    /// that gate as a prompt instead of the raw refusal.
    #[serde(default)]
    pub nick: Option<String>,
}

/// Server's own error body shape on a non-success response, e.g.
/// `{"error": "That title is too short"}` — a write refusal additionally
/// carries a stable `code` for the caller to branch on (`"nick_required"`
/// today; see [`NICK_REQUIRED_SENTINEL`]).
#[derive(serde::Deserialize)]
struct ErrorBody {
    error: String,
    #[serde(default)]
    code: Option<String>,
}

/// The one refusal `updater_loop`'s forum2/DM write handling special-cases
/// rather than surfacing verbatim: a signed-in caller with no nickname yet
/// (owner mandate 2026-09-09 — `code: "nick_required"` on `POST /api/
/// forum/v2/*` thread/reply creation and `POST /dm/conversations`/`POST
/// /dm/conversations/{id}/messages`). [`error_message`] returns this exact
/// string instead of the server's own sentence so the caller can tell it
/// apart with a plain `==` and re-drive the "choose a nick" prompt
/// (`ForumV2State::nick_required`) instead of showing raw error text —
/// same shape `mlc_cloud`'s wasm twin uses for its own copy of this
/// constant (the two builds never share a dependency for it, so the
/// VALUE, not the item, is what has to match).
pub const NICK_REQUIRED_SENTINEL: &str = "__mlc_nick_required__";

/// The 429 body additionally carries how long to wait.
#[derive(serde::Deserialize, Default)]
struct RateLimitBody {
    #[serde(default)]
    error: String,
    #[serde(default)]
    retry_after_secs: u64,
}

/// Turn a non-success response into the sentence the overlay prints. Mirrors
/// `mlc_cloud::forum_v2_error_message`'s wasm twin exactly, so the two
/// builds never say a different thing for the same server answer. Unlike
/// v1's `forum::error_message`, 403 reads the server's own body when
/// present — v2's board-write/moderate refusals carry different sentences,
/// not one fixed "requires a paid account".
async fn error_message(resp: reqwest::Response) -> String {
    let status = resp.status();
    if status.as_u16() == 429 {
        return match resp.json::<RateLimitBody>().await {
            Ok(b) if b.retry_after_secs > 0 => format!("Try again in {}s", b.retry_after_secs),
            Ok(b) if !b.error.is_empty() => b.error,
            _ => "Too many requests — try again shortly".to_string(),
        };
    }
    if status.as_u16() == 401 {
        return "Sign in again, then retry".to_string();
    }
    match resp.json::<ErrorBody>().await {
        Ok(b) if b.code.as_deref() == Some("nick_required") => NICK_REQUIRED_SENTINEL.to_string(),
        Ok(b) => b.error,
        Err(_) => format!("Server returned {}", status),
    }
}

/// `GET /api/forum/boards` — boards the caller can read.
pub async fn list_boards(client: &reqwest::Client, auth_header: Option<&str>) -> Result<Vec<ForumBoard>, String> {
    let url = format!("{}/api/forum/boards", UPDATE_SERVER);
    let mut req = client.get(&url);
    if let Some(auth) = auth_header {
        req = req.header("Authorization", auth);
    }
    let resp = req.send().await.map_err(|e| format!("Request failed: {}", e))?;
    if !resp.status().is_success() {
        return Err(error_message(resp).await);
    }
    let wire: BoardsWire = resp.json().await.map_err(|e| format!("Parse error: {}", e))?;
    Ok(wire.boards)
}

/// `GET /api/forum/boards/{slug}/threads?limit=50&before=…` — 404 unknown
/// slug, or a board the caller cannot read.
pub async fn list_board_threads(
    client: &reqwest::Client,
    auth_header: Option<&str>,
    slug: &str,
    before: Option<&str>,
) -> Result<(Vec<ForumThreadV2Summary>, Option<String>), String> {
    let url = match before {
        Some(b) => format!(
            "{}/api/forum/boards/{}/threads?limit=50&before={}",
            UPDATE_SERVER,
            slug,
            urlencode(b)
        ),
        None => format!("{}/api/forum/boards/{}/threads?limit=50", UPDATE_SERVER, slug),
    };
    let mut req = client.get(&url);
    if let Some(auth) = auth_header {
        req = req.header("Authorization", auth);
    }
    let resp = req.send().await.map_err(|e| format!("Request failed: {}", e))?;
    if !resp.status().is_success() {
        return Err(error_message(resp).await);
    }
    let wire: BoardThreadsWire = resp.json().await.map_err(|e| format!("Parse error: {}", e))?;
    Ok((wire.threads, wire.next_before))
}

/// `POST /api/forum/boards/{slug}/threads` — requires a stored token; 403
/// if the board's write class is not met, 429 at one thread per 60s.
pub async fn create_thread(
    client: &reqwest::Client,
    auth_header: &str,
    slug: &str,
    title: &str,
    body: &str,
) -> Result<i64, String> {
    let url = format!("{}/api/forum/boards/{}/threads", UPDATE_SERVER, slug);
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

/// `GET /api/forum/v2/threads/{id}` — the thread plus its reply tree.
pub async fn get_thread(client: &reqwest::Client, auth_header: Option<&str>, id: i64) -> Result<ForumThreadV2, String> {
    let url = format!("{}/api/forum/v2/threads/{}", UPDATE_SERVER, id);
    let mut req = client.get(&url);
    if let Some(auth) = auth_header {
        req = req.header("Authorization", auth);
    }
    let resp = req.send().await.map_err(|e| format!("Request failed: {}", e))?;
    if !resp.status().is_success() {
        return Err(error_message(resp).await);
    }
    let wire: ThreadWire = resp.json().await.map_err(|e| format!("Parse error: {}", e))?;
    Ok(ForumThreadV2 {
        id: wire.thread.id,
        board_id: wire.thread.board_id,
        title: wire.thread.title,
        author: wire.thread.author,
        author_is_me: wire.thread.author_is_me,
        author_rank: wire.thread.author_rank,
        body: wire.thread.body,
        created_at: wire.thread.created_at,
        updated_at: wire.thread.updated_at,
        pinned: wire.thread.pinned,
        locked: wire.thread.locked,
        hidden: wire.thread.hidden,
        replies: wire.replies,
    })
}

/// `POST /api/forum/v2/threads/{id}/replies` — requires a stored token;
/// 429 at ten replies per 60s, 403 if the thread is locked or the write
/// class is not met. `parent_reply_id = None` posts a top-level reply.
pub async fn create_reply(
    client: &reqwest::Client,
    auth_header: &str,
    thread_id: i64,
    body: &str,
    parent_reply_id: Option<i64>,
) -> Result<i64, String> {
    let url = format!("{}/api/forum/v2/threads/{}/replies", UPDATE_SERVER, thread_id);
    let resp = client
        .post(&url)
        .header("Authorization", auth_header)
        .json(&serde_json::json!({ "body": body, "parent_reply_id": parent_reply_id }))
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;
    if !resp.status().is_success() {
        return Err(error_message(resp).await);
    }
    resp.json::<IdWire>().await.map(|w| w.id).map_err(|e| format!("Parse error: {}", e))
}

/// `DELETE /api/forum/v2/replies/{id}` — requires a stored token; 204 for
/// the author or a moderator/admin, 403 otherwise.
pub async fn delete_reply(client: &reqwest::Client, auth_header: &str, id: i64) -> Result<(), String> {
    let url = format!("{}/api/forum/v2/replies/{}", UPDATE_SERVER, id);
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

/// `GET /api/forum/search?q=&board=` — anonymous-ok, restricted to boards
/// the caller can read; `board` filters to one slug when set.
pub async fn search(
    client: &reqwest::Client,
    auth_header: Option<&str>,
    q: &str,
    board: Option<&str>,
) -> Result<Vec<ForumSearchHit>, String> {
    let mut url = format!("{}/api/forum/search?q={}", UPDATE_SERVER, urlencode(q));
    if let Some(b) = board {
        url.push_str(&format!("&board={}", urlencode(b)));
    }
    let mut req = client.get(&url);
    if let Some(auth) = auth_header {
        req = req.header("Authorization", auth);
    }
    let resp = req.send().await.map_err(|e| format!("Request failed: {}", e))?;
    if !resp.status().is_success() {
        return Err(error_message(resp).await);
    }
    let wire: SearchWire = resp.json().await.map_err(|e| format!("Parse error: {}", e))?;
    Ok(wire.results)
}

/// `POST /api/forum/v2/threads/{id}/moderate` — requires a stored token and
/// moderator/admin role (403 otherwise); `board` names the target slug for
/// `"move"`, ignored otherwise.
pub async fn moderate(
    client: &reqwest::Client,
    auth_header: &str,
    thread_id: i64,
    action: &str,
    board: Option<&str>,
) -> Result<(), String> {
    let url = format!("{}/api/forum/v2/threads/{}/moderate", UPDATE_SERVER, thread_id);
    let resp = client
        .post(&url)
        .header("Authorization", auth_header)
        .json(&serde_json::json!({ "action": action, "board": board }))
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;
    if !resp.status().is_success() {
        return Err(error_message(resp).await);
    }
    Ok(())
}

/// `GET /api/forum/me` — the caller's resolved forum classes/roles.
/// Anonymous-ok: goes out with whatever bearer is configured (possibly
/// none — see [`ForumMe`]'s own doc).
pub async fn me(client: &reqwest::Client, auth_header: Option<&str>) -> Result<ForumMe, String> {
    let url = format!("{}/api/forum/me", UPDATE_SERVER);
    let mut req = client.get(&url);
    if let Some(auth) = auth_header {
        req = req.header("Authorization", auth);
    }
    let resp = req.send().await.map_err(|e| format!("Request failed: {}", e))?;
    if !resp.status().is_success() {
        return Err(error_message(resp).await);
    }
    resp.json::<ForumMe>().await.map_err(|e| format!("Parse error: {}", e))
}

/// Percent-encode a query-string component — mirrors
/// `mlc_cloud::wasm::encode_query_component`'s wasm twin exactly.
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
