//! Wire-format models for the MLC server API (`/api/sync/*`, `/api/auth/*`).
//!
//! These mirror the server-side shapes in
//! `mylittlechart-server/crates/server/src/routes/sync.rs`,
//! `.../routes/device_auth.rs` and `.../models/types.rs`, and the pre-strip
//! client models recovered from `mylittlechart-private-stash/updater`
//! (source commit `deedb7f`). The server is private; these DTOs carry no
//! secrets and are safe to keep in the OSS tree.

use serde::{Deserialize, Serialize};

// =============================================================================
// Sync items
// =============================================================================

/// Tier that governs how an item is transported to the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncTier {
    /// Plaintext structured data — no client-side encryption.
    CloudSync,
    /// Already-encrypted binary blob — pushed as-is, no re-encryption.
    ZtBlob,
    /// Device-local only — never sent to the server.
    DeviceLocal,
}

/// Lightweight metadata for a single sync item — used in change-detection
/// lists to avoid transferring full content on every tick.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncItemMeta {
    pub sync_id: String,
    pub category: String,
    pub name: String,
    pub checksum: String,
    pub modified_at: i64,
    #[serde(default)]
    pub deleted: bool,
}

/// A sync item with its full serialized content.
///
/// `content` is an opaque blob from the server's point of view (plaintext
/// JSON for [`SyncTier::CloudSync`], base64 ciphertext for
/// [`SyncTier::ZtBlob`]). `checksum` must be `SHA-256(content)` as 64
/// lowercase hex chars — the server verifies it before writing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncItem {
    pub sync_id: String,
    pub category: String,
    pub name: String,
    pub content: String,
    pub checksum: String,
    pub modified_at: i64,
    #[serde(default)]
    pub deleted: bool,
}

// =============================================================================
// /api/sync/* request/response shapes
// =============================================================================

/// Request body for `POST /api/sync/push` (max 50 items per request).
#[derive(Debug, Clone, Serialize)]
pub struct PushRequest {
    pub items: Vec<SyncItem>,
}

/// Response body from `POST /api/sync/push`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushResponse {
    #[serde(alias = "synced")]
    pub accepted: usize,
    #[serde(default)]
    pub rejected: Vec<serde_json::Value>,
}

/// Response body from `GET /api/sync/changes?since=T`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangesResponse {
    pub items: Vec<SyncItemMeta>,
    pub server_timestamp: i64,
}

/// Response body from `GET /api/sync/pull` (full snapshot).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullResponse {
    pub items: Vec<SyncItem>,
}

/// Response body from `GET /api/sync/status`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncStatusResponse {
    pub has_cloud_data: bool,
    pub item_count: i64,
    pub last_modified: Option<i64>,
    pub quota_used_bytes: Option<i64>,
    #[serde(default)]
    pub quota_limit_bytes: Option<i64>,
}

/// Response body from `DELETE /api/sync/all`
/// (requires the `X-Confirm-Delete: yes` header).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteAllResponse {
    pub deleted: u64,
}

/// Response body from `GET /api/sync/e2e-params`.
///
/// The server returns `{ "enabled": false }` when the user has not set up
/// vault recovery; the remaining fields are absent in that case.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct E2EParamsResponse {
    pub enabled: bool,
    #[serde(default)]
    pub salt: Option<String>,
    #[serde(default)]
    pub iterations: Option<i32>,
    #[serde(default)]
    pub encrypted_master_key: Option<String>,
}

/// Vault parameters for an account with E2E recovery set up.
#[derive(Debug, Clone)]
pub struct VaultParams {
    /// Hex-encoded 16-byte random salt used for PBKDF2 key derivation.
    pub salt: String,
    /// Number of PBKDF2 iterations.
    pub iterations: i32,
    /// AES-256-GCM blob: master key encrypted with the user's recovery key
    /// (base64). The server stores it opaquely and never inspects it.
    pub encrypted_master_key: Option<String>,
}

/// Request body for `POST /api/sync/e2e-setup`.
///
/// `salt` must be exactly 32 lowercase hex chars (16 bytes);
/// `iterations` must be >= 100 000.
#[derive(Debug, Clone, Serialize)]
pub struct E2ESetupRequest {
    pub salt: String,
    pub iterations: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encrypted_master_key: Option<String>,
}

// =============================================================================
// /api/auth/* shapes (device-code login)
// =============================================================================

/// Request body for `POST /api/auth/device-code`.
#[derive(Debug, Clone, Serialize)]
pub struct DeviceCodeRequest {
    pub device_id: String,
}

/// Response body from `POST /api/auth/device-code`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceCodeResponse {
    pub code: String,
    pub expires_in: u64,
}

/// Outcome of `GET /api/auth/poll?code=…`.
///
/// `pending` returns HTTP 202, `expired` HTTP 410, `confirmed` HTTP 200 —
/// the transport maps all three onto this enum (status code + body).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum DeviceAuthPoll {
    Pending,
    Expired,
    Confirmed {
        token: String,
        display_name: String,
        provider: String,
        user_id: i64,
        public_id: String,
    },
}

/// A single linked OAuth provider in a user's profile.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderInfo {
    pub provider: String,
    pub display_name: String,
}

/// Response body from `GET /api/auth/me`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeResponse {
    pub user_id: i64,
    /// Non-sequential public identifier — safe to expose in APIs.
    pub public_id: String,
    /// Provider-supplied name (e.g. the Google account name) — NOT what
    /// other users see once a nick is set; that is `effective_label`.
    pub display_name: String,
    /// The user's chosen nick, if any (`POST /api/account/nick`).
    #[serde(default)]
    pub nick: Option<String>,
    /// Server-computed label shown to everyone: nick → display_name →
    /// public_id. `Option` + default so a server predating nicks still
    /// parses; callers fall back to `display_name`.
    #[serde(default)]
    pub effective_label: Option<String>,
    /// Primary provider (kept for backwards compatibility).
    pub provider: String,
    /// All providers linked to this account.
    pub providers: Vec<ProviderInfo>,
}

// =============================================================================
// /api/entitlements shape (feature gates)
// =============================================================================

/// Response body from `GET /api/entitlements`.
///
/// Answers for an anonymous caller too — the server treats "no credential"
/// as a supported caller (the free feature set), not a 401. Because of that,
/// `account_signed_in` (whether THIS install actually holds a credential) is
/// never derived from this response — each shell already knows it from its
/// own token store, and reads it separately
/// (`mlc_app_layout::ui::modal_settings::UserSettingsState::
/// account_signed_in`'s own doc has the full reasoning). `expires_at` is a
/// unix-ms timestamp: the caller re-asks once it passes rather than on every
/// frame or every modal open.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntitlementsResponse {
    pub features: Vec<String>,
    pub expires_at: i64,
    /// Whether this caller's paid access is live right now.
    ///
    /// Stated outright rather than inferred from the presence of a paid
    /// feature name: the feature set is what paid currently *buys*, and that
    /// list will move. Whether the account is paid is a separate fact and the
    /// account view shows it directly. `#[serde(default)]` because a server
    /// older than 2026-09-04 does not send it — a missing answer reads as not
    /// paid, never the other way round.
    #[serde(default)]
    pub paid: bool,
    /// When that paid access ends (RFC 3339), or `None` when it is not paid.
    #[serde(default)]
    pub paid_until: Option<String>,
}

impl EntitlementsResponse {
    /// Wire name of the multi-profile feature gate.
    pub const MULTI_PROFILE: &'static str = "multi_profile";
    /// Wire name of the paid-only forum feature gate — never in the free
    /// set by construction (unlike the other gates here, closed from day
    /// one, see `docs/mlc/plans/web-surface-cleanup-and-in-app-blog-
    /// 2026-09-05.md` phase 3).
    pub const FORUM: &'static str = "forum";

    /// Whether the resolved feature set contains [`Self::MULTI_PROFILE`].
    pub fn has_multi_profile(&self) -> bool {
        self.features.iter().any(|f| f == Self::MULTI_PROFILE)
    }

    /// Whether the resolved feature set contains [`Self::FORUM`] — the
    /// client's sole visibility switch for the Forum tab.
    pub fn has_forum(&self) -> bool {
        self.features.iter().any(|f| f == Self::FORUM)
    }
}

// =============================================================================
// /api/share/* shapes (chart sharing)
//
// Mirrors `mylittlechart-server/crates/server/src/routes/share.rs`. The public
// read (`GET /api/share/{token}`) is unauthenticated — the link token alone is
// the capability. Creation is capped per kind (free 1/10, paid 10/100 active
// live/screenshot shares); exceeding the ceiling answers 403.
// =============================================================================

/// Request body for `POST /api/share/live` — the active preset frozen into
/// a public link. `preset` is the serialized `ChartPreset` JSON; the server
/// caps it at 256 KiB (bars are `skip_serializing`, so a normal preset is
/// far below that).
#[derive(Debug, Clone, Serialize)]
pub struct ShareCreateLiveRequest {
    pub title: String,
    pub preset: serde_json::Value,
}

/// Response body from `POST /api/share/live` and `POST /api/share/screenshot`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareCreateResponse {
    pub token: String,
    /// Root-relative link target (`/?share=<token>`) — prefix with the
    /// origin before handing it to a clipboard.
    pub url_path: String,
    /// RFC 3339 instant after which the share stops resolving.
    pub expires_at: String,
}

/// Public view of one share (`GET /api/share/{token}`), tagged by `kind`.
/// Unknown/expired/revoked tokens answer a bare 404 instead.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum SharePublicResponse {
    Live {
        title: String,
        /// Owner display label (display_name with public_id fallback).
        owner: String,
        /// The frozen `ChartPreset` JSON.
        preset: serde_json::Value,
        created_at: String,
        expires_at: String,
        /// Effective rights for this caller (rights v1, plan §2 of
        /// `docs/mlc/plans/native-share-viewer-and-chart-rights-2026-09-06.md`):
        /// `["read"]` plus `"clone"` when the link's `clone_allowed` is set.
        /// `#[serde(default)]` for a server predating rights v1, which never
        /// sent this field at all.
        #[serde(default)]
        rights: Vec<String>,
    },
    Screenshot {
        title: String,
        owner: String,
        /// Root-relative URL of the PNG blob (`/api/share/{token}/png`).
        png_url: String,
        bytes: i64,
        expires_at: String,
    },
}

/// One row of `GET /api/share/mine` (the caller's own active shares).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareListEntry {
    pub token: String,
    pub kind: String,
    pub title: String,
    pub url_path: String,
    /// PNG byte count for screenshot shares; 0 for live shares.
    pub bytes: i64,
    pub created_at: String,
    pub expires_at: String,
}

/// Response body from `GET /api/share/mine`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareListResponse {
    pub shares: Vec<ShareListEntry>,
}

/// Metadata describing a share link — the shape `GET /api/share/{token}`
/// grows once rights v1 lands (plan §2, `docs/mlc/plans/
/// native-share-viewer-and-chart-rights-2026-09-06.md`): `rights` answers
/// `["read"]` plus `"clone"` when the link's `clone_allowed` is set. Not yet
/// emitted by `SharePublicResponse` above — added ahead of the server change
/// so `mlc_app::share_viewer::SharedChartMeta` (the app-side mirror the
/// native/web shells construct from this) and this wire shape share one
/// field contract from day one.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedChartMeta {
    pub token: String,
    pub title: String,
    pub owner_label: String,
    /// `"live"` or `"screenshot"` — mirrors `SharePublicResponse`'s `kind` tag.
    pub kind: String,
    #[serde(default)]
    pub rights: Vec<String>,
    pub expires_at: String,
}

/// One indicator instance `POST /api/share/{token}/clone` dropped from the
/// cloned preset because its `type_id` names a paid/private vendor
/// indicator the recipient holds no grant for (plan §3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrippedIndicator {
    pub type_id: String,
    pub name: String,
}

/// Response body from `POST /api/share/{token}/clone` (auth required).
/// `preset` is the cloned `ChartPreset` JSON, already stripped of any
/// indicator instance the caller has no license for.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClonePresetResponse {
    pub preset: serde_json::Value,
    #[serde(default)]
    pub stripped: Vec<StrippedIndicator>,
}

/// Response body from `POST /api/account/redeem` on success.
///
/// The failure side is not a shape at all — the server answers 404 for an
/// unknown code and 409 for one already spent, and the transport turns those
/// into the message the modal prints, so there is nothing here to model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RedeemResponse {
    /// The account's new paid-until instant (RFC 3339) after the grant.
    pub paid_until: String,
}

/// Request body for `POST /api/account/nick` — sets the account-level
/// nickname (distinct from any per-profile display name; see
/// `mlc_app_layout::ui::modal_settings::UserSettingsState::nick_edit_mode`'s
/// own doc for why the two never share state).
#[derive(Debug, Clone, Serialize)]
pub struct SetNickRequest {
    pub nick: String,
}

/// Response body from `POST /api/account/nick` on success.
///
/// The failure side is not a shape either — 400 is a locally-shaped nick that
/// still failed server validation, 409 is taken/reserved, 429 is the
/// per-account cooldown (its body additionally carries `next_allowed_at`,
/// read directly off the raw [`CloudError::Http`] body by
/// [`crate::nick_error_message`] rather than modeled here, the same split
/// `redeem_error_message` uses for [`RedeemResponse`]'s failure side).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetNickResponse {
    /// The nick as the server stored it (may differ from the submission by
    /// normalization, e.g. casing).
    pub nick: String,
    /// The label to show for this account from now on — the nick when one
    /// is set. Read this rather than re-deriving it locally.
    pub effective_label: String,
}

// =============================================================================
// /api/updates/* shapes (anonymous release manifest)
// =============================================================================

/// Response body from `GET /api/updates/latest?platform=…`.
/// Mirrors `VersionManifest` in the server's `models/types.rs`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionManifest {
    pub version: String,
    #[serde(default)]
    pub sha256: String,
    pub download_url: String,
    #[serde(default)]
    pub release_notes: String,
    #[serde(default)]
    pub file_size: u64,
    #[serde(default)]
    pub signature: String,
    #[serde(default)]
    pub channel: String,
}

/// One entry in the release catalogue (`GET /api/updates/list?channel=…`).
/// Mirrors one row of the server's `releases` table — every published
/// platform/version pair, not just the caller's own platform. Drives the
/// Download overlay's expanded "other platforms & versions" list; the
/// single-platform `VersionManifest` above stays what the collapsed view
/// (and the OTA updater) asks for.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseEntry {
    pub platform: String,
    pub version: String,
    pub channel: String,
    #[serde(default)]
    pub sha256: String,
    #[serde(default)]
    pub file_size: u64,
    #[serde(default)]
    pub release_notes: String,
    pub created_at: String,
    pub download_url: String,
}

/// Response body from `GET /api/updates/list?channel=…`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseListResponse {
    pub releases: Vec<ReleaseEntry>,
}

/// One entry in the blog feed (`GET /api/posts?limit=N`, same-origin,
/// anonymous). Drives the News overlay in cloud builds. The old static
/// `GET /blog/posts.json` document serves the same rows without `kind`/
/// `version` — the defaults below keep this type parsing either shape.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlogPost {
    pub id: String,
    /// `"post"` (editorial) or `"patch"` (release note); defaults to
    /// `"post"` for feeds served before this field existed.
    #[serde(default = "default_blog_post_kind")]
    pub kind: String,
    /// Release version the entry documents — set on `"patch"` rows, `None`
    /// otherwise (and for feeds served before this field existed).
    #[serde(default)]
    pub version: Option<String>,
    pub date: String,
    pub title: String,
    pub summary: String,
    /// Full post text; paragraphs separated by blank lines. Older feeds may
    /// lack it — then the overlay falls back to the summary.
    #[serde(default)]
    pub body: String,
    pub url: String,
}

fn default_blog_post_kind() -> String {
    "post".to_string()
}

/// The blog feed document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlogFeed {
    pub posts: Vec<BlogPost>,
}

// =============================================================================
// /api/forum/* shapes (paid-only communication board)
//
// Every route requires auth (cookie `session_id` or Bearer, same as sync)
// AND a paid account: 401 anonymous, 403 free — bodies `{"error": …}`.
// `GET /api/entitlements`'s `"forum"` feature is the client's visibility
// switch (`EntitlementsResponse::has_forum`); these endpoints are never
// called for a caller that switch says no to.
// =============================================================================

/// One row of `GET /api/forum/threads` (`ForumThreadsResponse::threads`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ForumThreadSummary {
    pub id: i64,
    pub title: String,
    pub author: String,
    pub author_is_me: bool,
    pub created_at: String,
    pub last_reply_at: Option<String>,
    pub reply_count: i64,
    pub excerpt: String,
}

/// Response body from `GET /api/forum/threads?limit=N&before=<activity>|<id>`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForumThreadsResponse {
    pub threads: Vec<ForumThreadSummary>,
    pub next_before: Option<String>,
}

/// The thread itself, as nested in `ForumThreadResponse::thread` — never
/// carries its replies (those are the sibling `replies` field).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ForumThread {
    pub id: i64,
    pub title: String,
    pub author: String,
    pub author_is_me: bool,
    pub body: String,
    pub created_at: String,
}

/// One reply, as returned in `ForumThreadResponse::replies`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ForumReply {
    pub id: i64,
    pub author: String,
    pub author_is_me: bool,
    pub body: String,
    pub created_at: String,
}

/// Response body from `GET /api/forum/threads/{id}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForumThreadResponse {
    pub thread: ForumThread,
    pub replies: Vec<ForumReply>,
}

/// Request body for `POST /api/forum/threads`.
/// (400 if `title` is not 3–120 bytes or `body` is not 1–8192 bytes;
/// 429 with `{"error", "retry_after_secs"}` at one thread per 60s.)
#[derive(Debug, Clone, Serialize)]
pub struct ForumCreateThreadRequest {
    pub title: String,
    pub body: String,
}

/// Request body for `POST /api/forum/threads/{id}/replies`.
/// (429 at ten replies per 60s.)
#[derive(Debug, Clone, Serialize)]
pub struct ForumReplyRequest {
    pub body: String,
}

/// Response body from both `POST /api/forum/threads` and
/// `POST /api/forum/threads/{id}/replies` — both answer just the new row's id.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForumIdResponse {
    pub id: i64,
}

// =============================================================================
// /api/forum/* v2 shapes (public boards, role-gated read/write, threaded
// replies, moderation, search) — mirrors `mylittlechart-server/crates/
// server/src/routes/forum_v2.rs` + `social_db.rs`. NOT every route here
// requires a paid account: a board's own `read_class`/`write_class` decide
// per board (wire names from `social_db::Class::as_str`: "public"|"user"|
// "paid"|"moderator"|"admin"), and a `public` board answers a signed-out
// caller. Distinct from the v1 forum shapes above, which stay untouched.
// =============================================================================

/// One row of `GET /api/forum/boards` (`ForumBoardsResponse::boards`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ForumBoard {
    pub id: i64,
    pub slug: String,
    pub title: String,
    pub description: String,
    /// Wire name of the read-access class gate — see this section's doc.
    pub read_class: String,
    /// Wire name of the write-access class gate — see this section's doc.
    pub write_class: String,
    pub position: i64,
    /// Total threads on this board. `#[serde(default)]` (community-modal
    /// arc W2, owner review 2026-09-09: "a forum, not a feed") — the server
    /// change landing this field is a separate in-flight deploy, so an
    /// older server's response (missing the key entirely) must still
    /// deserialize rather than reject the whole boards list.
    #[serde(default)]
    pub thread_count: i64,
    /// Total replies across every thread on this board. Same
    /// `#[serde(default)]` reasoning as `thread_count`.
    #[serde(default)]
    pub reply_count: i64,
    /// Timestamp of the board's most recent thread or reply, whichever is
    /// newer — `None` for a board with no activity yet, or an older server
    /// that does not send this field.
    #[serde(default)]
    pub last_post_at: Option<String>,
    /// Title of the thread that carries `last_post_at` — `None` under the
    /// same conditions as `last_post_at`.
    #[serde(default)]
    pub last_thread_title: Option<String>,
}

/// Response body from `GET /api/forum/boards`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForumBoardsResponse {
    pub boards: Vec<ForumBoard>,
}

/// One row of `GET /api/forum/boards/{slug}/threads`
/// (`ForumBoardThreadsResponse::threads`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ForumThreadV2Summary {
    pub id: i64,
    pub board_id: i64,
    pub title: String,
    pub author: String,
    pub author_is_me: bool,
    pub created_at: String,
    pub updated_at: String,
    pub last_reply_at: Option<String>,
    pub reply_count: i64,
    pub pinned: bool,
    pub locked: bool,
}

/// Response body from
/// `GET /api/forum/boards/{slug}/threads?limit=N&before=<activity>|<id>`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForumBoardThreadsResponse {
    pub threads: Vec<ForumThreadV2Summary>,
    pub next_before: Option<String>,
}

/// The thread itself, as nested in `ForumThreadV2Response::thread` — never
/// carries its replies (those are the sibling `replies` field).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ForumThreadV2 {
    pub id: i64,
    pub board_id: i64,
    pub title: String,
    pub author: String,
    pub author_is_me: bool,
    pub body: String,
    pub created_at: String,
    pub updated_at: String,
    pub pinned: bool,
    pub locked: bool,
    /// Always `false` for a caller this response was ever returned to — a
    /// hidden thread reads as 404 for anyone but a moderator/admin, and the
    /// server sends the true value only to them
    /// (`routes::forum_v2::get_thread`).
    pub hidden: bool,
}

/// One reply node in `ForumThreadV2Response::replies` — a tree, not a flat
/// list: `children` nests replies posted under this one
/// (`parent_reply_id`), the same shape `social_db::ReplyNode`/
/// `routes::forum_v2::render_reply_tree` build server-side.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ForumReplyNode {
    pub id: i64,
    pub parent_reply_id: Option<i64>,
    pub depth: i64,
    pub author: String,
    pub author_is_me: bool,
    pub body: String,
    pub created_at: String,
    /// `true` only reaches this caller when they are a moderator/admin — a
    /// hidden reply is omitted from the tree entirely for everyone else,
    /// same as a hidden thread.
    pub hidden: bool,
    pub children: Vec<ForumReplyNode>,
}

/// Response body from `GET /api/forum/v2/threads/{id}`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForumThreadV2Response {
    pub thread: ForumThreadV2,
    pub replies: Vec<ForumReplyNode>,
}

/// Request body for `POST /api/forum/boards/{slug}/threads`.
/// (400 if `title` is not 3-120 bytes or `body` is not 1-8192 bytes; 401
/// anonymous; 403 write class too low; 429 at one thread per 60s.)
#[derive(Debug, Clone, Serialize)]
pub struct ForumCreateThreadV2Request {
    pub title: String,
    pub body: String,
}

/// Request body for `POST /api/forum/v2/threads/{id}/replies`.
/// `parent_reply_id = None` posts a top-level reply; `Some(id)` nests under
/// an existing (visible, same-thread) reply. (429 at ten replies per 60s;
/// 403 if the thread is locked or write class too low.)
#[derive(Debug, Clone, Serialize)]
pub struct ForumReplyV2Request {
    pub body: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_reply_id: Option<i64>,
}

/// One hit in `GET /api/forum/search`'s `results` array — either a thread
/// (`kind == "thread"`, `title` set) or a reply (`kind == "reply"`,
/// `reply_id` set, `title` absent).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ForumSearchHit {
    pub kind: String,
    pub board_id: i64,
    pub thread_id: i64,
    pub reply_id: Option<i64>,
    pub title: Option<String>,
    pub snippet: String,
    pub author: String,
    pub created_at: String,
}

/// Response body from `GET /api/forum/search?q=&board=`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForumSearchResponse {
    pub results: Vec<ForumSearchHit>,
}

/// Request body for `POST /api/forum/v2/threads/{id}/moderate` — moderator
/// or admin only (403 otherwise). `action` is one of hide|unhide|lock|
/// unlock|pin|unpin|move; `board` names the target slug for `"move"` and is
/// ignored otherwise.
#[derive(Debug, Clone, Serialize)]
pub struct ForumModerateAction {
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub board: Option<String>,
}

/// Response body from `GET /api/forum/me` — the caller's resolved forum
/// classes and roles. Optional bearer: an anonymous caller gets the
/// signed-out answer (`classes` holding just `"public"`, `roles` empty),
/// never a 401 — same optionality every other v2 read in this section has.
/// `classes` is the caller's full cumulative class membership (wire names
/// from `social_db::Class::as_str`, e.g. `["public","user","paid"]`), the
/// server-side authority [`crate::forum_v2_error_message`]'s own 403 body
/// already enforces; the client compares a board's `write_class` against
/// this set instead of guessing from `signed_in`/`paid` locally. `roles`
/// carries moderator/admin role names (`social_db::Role::as_str`) and gates
/// the moderation row on the Thread view.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ForumMeResponse {
    pub classes: Vec<String>,
    pub roles: Vec<String>,
    /// `null` for an anonymous caller (the server answers `label: null,
    /// public_id: null` then) — hence `Option`, not `String`.
    pub label: Option<String>,
    pub public_id: Option<String>,
}

// =============================================================================
// /api/dm/* shapes (paid, end-to-end encrypted direct messages)
//
// Mirrors `mylittlechart-server/crates/server/src/routes/dm.rs` +
// `dm_db.rs`. Every route requires an authenticated AND paid caller
// (401 anonymous, 403 free); several also require the NAMED peer to be
// currently paid (403 with a different sentence — see `dm_error_message`
// in `lib.rs`). `nonce_b64`/`ciphertext_b64` are opaque base64 to this
// crate: the caller (vault layer) already produced/consumes them — nothing
// here encodes, decodes, or inspects the bytes inside.
// =============================================================================

/// Request body for `PUT /api/dm/identity`.
#[derive(Debug, Clone, Serialize)]
pub struct DmPublishIdentityRequest {
    pub public_key_b64: String,
}

/// Response body from `PUT /api/dm/identity`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DmPublishIdentityResponse {
    pub fingerprint: String,
}

/// Response body from `GET /api/dm/identity/{public_id_or_nick}`. 404 if
/// the label names no user, or the user exists but has never published a
/// DM identity key — the two cases are indistinguishable to the caller.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DmIdentityResponse {
    pub public_id: String,
    pub label: String,
    pub public_key_b64: String,
    pub fingerprint: String,
}

/// Request body for `POST /api/dm/conversations`.
#[derive(Debug, Clone, Serialize)]
pub struct DmCreateConversationRequest {
    pub with: String,
}

/// The other party to a [`DmConversation`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DmConversation {
    pub id: i64,
    pub peer: DmConversationPeer,
    /// Absent on `POST /api/dm/conversations`'s creation response (a brand
    /// new conversation has no messages yet); present on the list response.
    #[serde(default)]
    pub last_message_at: Option<String>,
    /// Absent on the creation response; wire name is `unread_count` on the
    /// list response.
    #[serde(default, rename = "unread_count")]
    pub unread: i64,
}

/// Response body from `GET /api/dm/conversations`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DmConversationsResponse {
    pub conversations: Vec<DmConversation>,
}

/// One row of `GET /api/dm/conversations/{id}/messages`
/// (`DmMessagesResponse::messages`). `nonce_b64`/`ciphertext_b64` are opaque
/// to this crate — see this section's doc.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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

/// Response body from
/// `GET /api/dm/conversations/{id}/messages?after=&limit=`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DmMessagesResponse {
    pub messages: Vec<DmMessage>,
    pub next_after: Option<i64>,
}

/// Request body for `POST /api/dm/conversations/{id}/messages`.
#[derive(Debug, Clone, Serialize)]
pub struct DmSendMessageRequest {
    pub nonce_b64: String,
    pub ciphertext_b64: String,
}

/// Response body from `POST /api/dm/conversations/{id}/messages`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DmSendMessageResponse {
    pub id: i64,
    pub created_at: String,
}

/// Request body for `POST /api/dm/conversations/{id}/read`.
#[derive(Debug, Clone, Serialize)]
pub struct DmMarkReadRequest {
    pub up_to_id: i64,
}

/// Locally persisted auth token (wasm: localStorage; native: config dir).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StoredToken {
    pub token: String,
    pub provider: String,
    pub display_name: String,
    pub saved_at: u64,
    /// User ID returned by the server. Defaults to 0 for tokens saved before
    /// this field was added.
    #[serde(default)]
    pub user_id: i64,
    /// Non-sequential public identifier (present for tokens issued after the
    /// server started returning it from the poll endpoint).
    #[serde(default)]
    pub public_id: Option<String>,
}
