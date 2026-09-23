//! Direct-message client (`PUT/GET/POST /api/dm/*`) — paid, end-to-end
//! encrypted conversations. Mirrors `mylittlechart-server/crates/server/
//! src/routes/dm.rs` + `dm_db.rs`, and the native twin of
//! `mlc_cloud::wasm::FetchTransport`'s own `dm_put_identity`/`dm_send`/etc.
//!
//! Every route requires an authenticated AND paid caller (401 anonymous,
//! 403 free); several also require the NAMED peer to currently be paid
//! (same 403 status, different sentence — see [`error_message`]). Unlike
//! `forum_v2.rs`, there is no anonymous-ok path here at all: every function
//! below takes `auth_header: &str`, never `Option`.
//!
//! `nonce_b64`/`ciphertext_b64` are opaque base64 strings end to end — this
//! module never encodes, decodes, or inspects the bytes inside; the vault
//! layer above it does that.

use crate::UPDATE_SERVER;

/// Response body from `PUT /api/dm/identity`.
#[derive(serde::Deserialize)]
struct PublishIdentityWire {
    fingerprint: String,
}

/// Response body from `GET /api/dm/identity/{public_id_or_nick}`. 404 if
/// the label names no user, or the user exists but has never published a
/// DM identity key — the two cases are indistinguishable to the caller.
#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
pub struct DmIdentity {
    pub public_id: String,
    pub label: String,
    pub public_key_b64: String,
    pub fingerprint: String,
}

/// The other party to a [`DmConversation`].
#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
pub struct DmConversationPeer {
    pub label: String,
    pub public_id: String,
    /// `None` when the peer has never published a DM identity key. Also
    /// always absent on `GET /api/dm/conversations`'s list shape — only
    /// `POST /api/dm/conversations`'s creation response sends this field
    /// at all.
    #[serde(default)]
    pub fingerprint: Option<String>,
}

/// One conversation — the shape both `POST /api/dm/conversations`
/// (creation) and `GET /api/dm/conversations` (listing) answer with, though
/// the two omit different optional fields (see each field's own doc).
#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
pub struct DmConversation {
    pub id: i64,
    pub peer: DmConversationPeer,
    /// Absent on the creation response (a brand new conversation has no
    /// messages yet); present on the list response.
    #[serde(default)]
    pub last_message_at: Option<String>,
    /// Absent on the creation response; wire name is `unread_count` on the
    /// list response.
    #[serde(default, rename = "unread_count")]
    pub unread: i64,
}

#[derive(serde::Deserialize)]
struct ConversationsWire {
    conversations: Vec<DmConversation>,
}

/// One row of `GET /api/dm/lookup?q=`'s answer — a fuzzy nick/label match
/// for the recipient-search dropdown (community-modal-and-forum-ladder arc,
/// piece 2 item b). Mirrors `mlc_cloud::models::DmLookupHit`'s wasm twin;
/// the wire body's `rank` field is not declared here for the same reason
/// (unread, `serde` drops unknown fields by default).
#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
pub struct DmLookupHit {
    pub public_id: String,
    pub nick: Option<String>,
    pub label: String,
}

#[derive(serde::Deserialize)]
struct LookupWire {
    users: Vec<DmLookupHit>,
}

/// One row of `GET /api/dm/conversations/{id}/messages`. `nonce_b64`/
/// `ciphertext_b64` are opaque to this crate — see this module's doc.
#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
pub struct DmMessage {
    pub id: i64,
    pub sender_public_id: String,
    #[serde(default)]
    pub sender_is_me: bool,
    pub nonce_b64: String,
    pub ciphertext_b64: String,
    pub created_at: String,
    #[serde(default)]
    pub read_at: Option<String>,
}

#[derive(serde::Deserialize)]
struct MessagesWire {
    messages: Vec<DmMessage>,
    #[serde(default)]
    next_after: Option<i64>,
}

#[derive(serde::Deserialize)]
struct SendMessageWire {
    id: i64,
    created_at: String,
}

/// Server's own error body shape on a non-success response — a write
/// refusal additionally carries a stable `code` (`"nick_required"` today;
/// see [`crate::forum_v2::NICK_REQUIRED_SENTINEL`], reused here so both
/// gates read the same sentinel).
#[derive(serde::Deserialize)]
struct ErrorBody {
    error: String,
    #[serde(default)]
    code: Option<String>,
}

/// The 429 body additionally carries how long to wait.
#[derive(serde::Deserialize, Default)]
struct RateLimitBody {
    #[serde(default)]
    error: String,
    #[serde(default)]
    retry_after_secs: u64,
}

/// Turn a non-success response into the sentence the overlay prints. Mirrors
/// `mlc_cloud::dm_error_message`'s wasm twin exactly, so the two builds
/// never say a different thing for the same server answer.
async fn error_message(resp: reqwest::Response) -> String {
    let status = resp.status();
    if status.as_u16() == 429 {
        return match resp.json::<RateLimitBody>().await {
            Ok(b) if b.retry_after_secs > 0 => format!("Try again in {}s", b.retry_after_secs),
            Ok(b) if !b.error.is_empty() => b.error,
            _ => "Too many messages today — try again tomorrow".to_string(),
        };
    }
    if status.as_u16() == 401 {
        return "Sign in again, then retry".to_string();
    }
    if status.as_u16() == 403 {
        return match resp.json::<ErrorBody>().await {
            Ok(b) if b.code.as_deref() == Some("nick_required") => {
                crate::forum_v2::NICK_REQUIRED_SENTINEL.to_string()
            }
            Ok(b) => b.error,
            Err(_) => "Direct messages require a paid account".to_string(),
        };
    }
    match resp.json::<ErrorBody>().await {
        Ok(b) => b.error,
        Err(_) => format!("Server returned {}", status),
    }
}

/// `PUT /api/dm/identity` — publish/rotate the caller's DM public key.
/// Returns the fingerprint.
pub async fn publish_identity(client: &reqwest::Client, auth_header: &str, public_key_b64: &str) -> Result<String, String> {
    let url = format!("{}/api/dm/identity", UPDATE_SERVER);
    let resp = client
        .put(&url)
        .header("Authorization", auth_header)
        .json(&serde_json::json!({ "public_key_b64": public_key_b64 }))
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;
    if !resp.status().is_success() {
        return Err(error_message(resp).await);
    }
    let wire: PublishIdentityWire = resp.json().await.map_err(|e| format!("Parse error: {}", e))?;
    Ok(wire.fingerprint)
}

/// `GET /api/dm/identity/{public_id_or_nick}` — 404 if `label` names no
/// user, or the user has never published a DM key.
pub async fn get_identity(client: &reqwest::Client, auth_header: &str, label: &str) -> Result<DmIdentity, String> {
    let url = format!("{}/api/dm/identity/{}", UPDATE_SERVER, urlencode(label));
    let resp = client
        .get(&url)
        .header("Authorization", auth_header)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;
    if !resp.status().is_success() {
        return Err(error_message(resp).await);
    }
    resp.json().await.map_err(|e| format!("Parse error: {}", e))
}

/// `GET /api/dm/lookup?q=` — fuzzy nick/label search for the recipient
/// dropdown; server-sorted and -capped, distinct from the exact-label
/// [`get_identity`] above.
pub async fn lookup(client: &reqwest::Client, auth_header: &str, q: &str) -> Result<Vec<DmLookupHit>, String> {
    let url = format!("{}/api/dm/lookup?q={}", UPDATE_SERVER, urlencode(q));
    let resp = client
        .get(&url)
        .header("Authorization", auth_header)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;
    if !resp.status().is_success() {
        return Err(error_message(resp).await);
    }
    let wire: LookupWire = resp.json().await.map_err(|e| format!("Parse error: {}", e))?;
    Ok(wire.users)
}

/// `GET /api/dm/conversations` — the caller's own, newest activity first.
pub async fn list_conversations(client: &reqwest::Client, auth_header: &str) -> Result<Vec<DmConversation>, String> {
    let url = format!("{}/api/dm/conversations", UPDATE_SERVER);
    let resp = client
        .get(&url)
        .header("Authorization", auth_header)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;
    if !resp.status().is_success() {
        return Err(error_message(resp).await);
    }
    let wire: ConversationsWire = resp.json().await.map_err(|e| format!("Parse error: {}", e))?;
    Ok(wire.conversations)
}

/// `POST /api/dm/conversations` — `{"with": <public_id_or_nick>}` ->
/// existing or new conversation. 404 unknown peer, 403 the peer is not
/// currently paid.
pub async fn open_conversation(client: &reqwest::Client, auth_header: &str, with: &str) -> Result<DmConversation, String> {
    let url = format!("{}/api/dm/conversations", UPDATE_SERVER);
    let resp = client
        .post(&url)
        .header("Authorization", auth_header)
        .json(&serde_json::json!({ "with": with }))
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;
    if !resp.status().is_success() {
        return Err(error_message(resp).await);
    }
    resp.json().await.map_err(|e| format!("Parse error: {}", e))
}

/// `GET /api/dm/conversations/{id}/messages?after=&before=&limit=` — 404 if
/// the conversation does not exist or the caller is not a party. No cursor
/// -> the newest `limit` messages; `after` -> unchanged "what's new"
/// polling; `before` -> "load older" — mirrors `routes::dm::list_messages`'s
/// own doc (server F4 fix, 2026-09-23). `after`/`before` are mutually
/// exclusive; every caller in this tree keeps them exclusive by
/// construction (see [`crate::state::DmRequest::ListMessages`]'s own doc).
pub async fn list_messages(
    client: &reqwest::Client,
    auth_header: &str,
    id: i64,
    after: Option<i64>,
    before: Option<i64>,
    limit: i64,
) -> Result<(Vec<DmMessage>, Option<i64>), String> {
    let url = match (after, before) {
        (Some(a), _) => format!("{}/api/dm/conversations/{}/messages?after={}&limit={}", UPDATE_SERVER, id, a, limit),
        (None, Some(b)) => format!("{}/api/dm/conversations/{}/messages?before={}&limit={}", UPDATE_SERVER, id, b, limit),
        (None, None) => format!("{}/api/dm/conversations/{}/messages?limit={}", UPDATE_SERVER, id, limit),
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
    let wire: MessagesWire = resp.json().await.map_err(|e| format!("Parse error: {}", e))?;
    Ok((wire.messages, wire.next_after))
}

/// `POST /api/dm/conversations/{id}/messages` — sealed message, opaque to
/// the server. 403 if the other party is not currently paid, 413 if the
/// ciphertext exceeds the server's cap, 429 at the per-sender daily send
/// limit.
pub async fn send_message(
    client: &reqwest::Client,
    auth_header: &str,
    id: i64,
    nonce_b64: &str,
    ciphertext_b64: &str,
) -> Result<(i64, String), String> {
    let url = format!("{}/api/dm/conversations/{}/messages", UPDATE_SERVER, id);
    let resp = client
        .post(&url)
        .header("Authorization", auth_header)
        .json(&serde_json::json!({ "nonce_b64": nonce_b64, "ciphertext_b64": ciphertext_b64 }))
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;
    if !resp.status().is_success() {
        return Err(error_message(resp).await);
    }
    let wire: SendMessageWire = resp.json().await.map_err(|e| format!("Parse error: {}", e))?;
    Ok((wire.id, wire.created_at))
}

/// `POST /api/dm/conversations/{id}/read` — marks every message from the
/// other party with `id <= up_to` as read.
pub async fn mark_read(client: &reqwest::Client, auth_header: &str, id: i64, up_to: i64) -> Result<(), String> {
    let url = format!("{}/api/dm/conversations/{}/read", UPDATE_SERVER, id);
    let resp = client
        .post(&url)
        .header("Authorization", auth_header)
        .json(&serde_json::json!({ "up_to_id": up_to }))
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;
    if !resp.status().is_success() {
        return Err(error_message(resp).await);
    }
    Ok(())
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
