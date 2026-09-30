//! `/api/staff/*` — wire shapes and error classification for the in-app Admin
//! tab of the settings modal.
//!
//! Mirrors `mylittlechart-server/crates/server/src/routes/staff.rs`. Every
//! route needs the signed-in session (the Bearer token) plus the `admin` role;
//! anything else answers `403 {"error":"forbidden"}`. Errors are
//! `{"error": <sentence>}`; a 429 `rate_limited` carries `Retry-After`
//! (30 writes per minute per actor), which the transport folds into the body
//! as `retry_after_secs` ([`with_retry_after`]) so this module reads it the
//! same way the forum's 429 is read.
//!
//! Gift codes are secrets: [`StaffMintResponse`] prints only their count in
//! `Debug`, so a stray `{:?}` can never put a code into a log line.

use serde::{Deserialize, Serialize};

use crate::CloudError;

// =============================================================================
// Requests
// =============================================================================

/// Body of `POST /api/staff/gift-codes`.
#[derive(Debug, Clone, Serialize)]
pub struct StaffMintRequest {
    /// Days each code grants once redeemed, 1..=365.
    pub days: i64,
    /// Codes to mint, 1..=200.
    pub count: i64,
    /// Why (optional, at most 200 printable characters).
    pub note: String,
}

/// Body of `POST /api/staff/grants/paid-days`.
#[derive(Debug, Clone, Serialize)]
pub struct StaffPaidDaysRequest {
    pub nick: String,
    /// Days to extend by, 1..=3650.
    pub days: i64,
}

/// Body of `POST /api/staff/grants/lifetime`.
#[derive(Debug, Clone, Serialize)]
pub struct StaffLifetimeRequest {
    pub nick: String,
    /// `lifetime_paid` or `lifetime_anon`.
    pub grant: String,
    /// Why, 1..=200 printable characters.
    pub note: String,
}

/// Body of `POST /api/staff/grants/lifetime/revoke`.
#[derive(Debug, Clone, Serialize)]
pub struct StaffLifetimeRevokeRequest {
    pub nick: String,
    /// `lifetime_paid` or `lifetime_anon`.
    pub grant: String,
}

// =============================================================================
// Responses
// =============================================================================

/// `201` reply of `POST /api/staff/gift-codes`. The codes come back ONCE —
/// the server stores only their digests.
#[derive(Clone, Deserialize, PartialEq, Eq)]
pub struct StaffMintResponse {
    pub batch_id: String,
    pub days: i64,
    pub count: i64,
    #[serde(default)]
    pub note: String,
    pub codes: Vec<String>,
}

impl std::fmt::Debug for StaffMintResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StaffMintResponse")
            .field("batch_id", &self.batch_id)
            .field("days", &self.days)
            .field("count", &self.count)
            .field("note", &self.note)
            .field("codes", &format_args!("<{} redacted>", self.codes.len()))
            .finish()
    }
}

/// One row of `GET /api/staff/gift-batches` (newest first, no code values).
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct StaffGiftBatch {
    pub batch_id: String,
    #[serde(default)]
    pub note: String,
    pub days: i64,
    pub count: i64,
    /// Codes redeemed so far.
    pub redeemed: i64,
    /// Unredeemed codes that were revoked.
    pub revoked: i64,
    pub created_at: String,
    /// `null` for a batch minted from the ops path or by an account with no
    /// nick.
    #[serde(default)]
    pub created_by_nick: Option<String>,
}

/// Reply of `POST /api/staff/gift-batches/{id}/revoke`.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct StaffBatchRevoked {
    pub batch_id: String,
    /// Unredeemed codes that stopped working just now (`0` on a repeat).
    pub revoked: i64,
}

/// Reply of `POST /api/staff/grants/paid-days`.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct StaffPaidDays {
    pub nick: String,
    pub paid_until: String,
}

/// Reply of `POST /api/staff/grants/lifetime` — `201` when the grant was
/// created, `200` (`created: false`) when the identical grant was already in
/// force.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct StaffLifetimeGrant {
    pub nick: String,
    pub grant: String,
    pub grant_id: i64,
    /// Always `"admin"` for this route.
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub source_ref: String,
    #[serde(default)]
    pub created: bool,
}

/// Reply of `POST /api/staff/grants/lifetime/revoke`.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct StaffLifetimeRevoked {
    pub nick: String,
    pub grant: String,
    /// Admin-source grants that were in force and are now revoked.
    pub revoked: i64,
}

/// One lifetime-grant ledger row inside [`StaffUser`].
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct StaffLedgerRow {
    pub grant_id: i64,
    pub grant: String,
    /// `"admin"` or `"invoice"`.
    pub source: String,
    #[serde(default)]
    pub source_ref: String,
    pub created_at: String,
    /// `null` while the grant is in force.
    #[serde(default)]
    pub revoked_at: Option<String>,
}

/// Reply of `GET /api/staff/user?nick=` — one account's grants and roles.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct StaffUser {
    pub nick: String,
    pub user_id: i64,
    /// The time-bound grant while it is still running.
    #[serde(default)]
    pub paid_until: Option<String>,
    #[serde(default)]
    pub lifetime_paid: bool,
    #[serde(default)]
    pub lifetime_anon: bool,
    #[serde(default)]
    pub lifetime_grants: Vec<StaffLedgerRow>,
    #[serde(default)]
    pub quant_tier: Option<String>,
    #[serde(default)]
    pub quant_active_until: Option<String>,
    /// The account's forum role names (0 or 1).
    #[serde(default)]
    pub roles: Vec<String>,
}

// =============================================================================
// Errors
// =============================================================================

/// What a refused or failed `/api/staff/*` call means to the person using the
/// Admin tab. Wording is the caller's job (it is localized there); this only
/// classifies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaffError {
    /// 403 — not an admin (the server answers every non-admin, signed in or
    /// not, with the same body).
    Forbidden,
    /// 404 — no such nick or batch.
    NotFound,
    /// 429 — the per-actor write limit; `retry_after_secs` is the
    /// `Retry-After` header when the server sent one.
    RateLimited { retry_after_secs: Option<u64> },
    /// 400 — the server's own sentence about the input.
    BadRequest { detail: String },
    /// 401 — the session is no longer valid.
    SignedOut,
    /// The request never completed (offline, CORS, JS exception).
    Network { detail: String },
    /// Any other status, or a reply that did not decode.
    Server { detail: String },
}

#[derive(Deserialize, Default)]
struct ErrorBody {
    #[serde(default)]
    error: String,
    #[serde(default)]
    retry_after_secs: u64,
}

fn parse_error_body(body: &str) -> ErrorBody {
    serde_json::from_str(body).unwrap_or_default()
}

/// Classify a failed `/api/staff/*` call. See [`StaffError`].
pub fn staff_error(e: &CloudError) -> StaffError {
    match e {
        CloudError::Unauthorized => StaffError::SignedOut,
        CloudError::Forbidden(_) => StaffError::Forbidden,
        CloudError::Http { status: 404, .. } => StaffError::NotFound,
        CloudError::Http { status: 429, body } => {
            let parsed = parse_error_body(body);
            StaffError::RateLimited {
                retry_after_secs: (parsed.retry_after_secs > 0).then_some(parsed.retry_after_secs),
            }
        }
        CloudError::Http { status: 400, body } => {
            let parsed = parse_error_body(body);
            StaffError::BadRequest {
                detail: if parsed.error.is_empty() { body.trim().to_string() } else { parsed.error },
            }
        }
        CloudError::Http { status, body } => {
            let parsed = parse_error_body(body);
            let text = if parsed.error.is_empty() { body.trim().to_string() } else { parsed.error };
            StaffError::Server { detail: format!("HTTP {status}: {text}") }
        }
        CloudError::Transport(detail) => StaffError::Network { detail: detail.clone() },
        CloudError::Decode(detail) => StaffError::Server { detail: format!("unexpected reply: {detail}") },
    }
}

/// Fold a `Retry-After` header into a 429 body as `retry_after_secs`, the
/// field [`staff_error`] reads. The server's body is `{"error":"rate_limited"}`
/// and carries the wait only in the header, which the transport's
/// `(status, body)` pair would otherwise drop. Any other status, or a 429
/// without a usable header, passes through untouched.
pub fn with_retry_after(status: u16, body: String, retry_after_secs: Option<u64>) -> String {
    let Some(secs) = retry_after_secs.filter(|_| status == 429) else {
        return body;
    };
    let mut value = match serde_json::from_str::<serde_json::Value>(&body) {
        Ok(serde_json::Value::Object(map)) => map,
        _ => serde_json::Map::new(),
    };
    value.insert("retry_after_secs".to_string(), serde_json::Value::from(secs));
    serde_json::Value::Object(value).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mint_reply_decodes_and_debug_hides_the_codes() {
        let body = r#"{"batch_id":"gb_1a2b","days":30,"count":2,"note":"beta wave","codes":["K7Q2M9XPTA","B4N8R5WCDE"]}"#;
        let minted: StaffMintResponse = serde_json::from_str(body).expect("mint reply");
        assert_eq!(minted.batch_id, "gb_1a2b");
        assert_eq!((minted.days, minted.count), (30, 2));
        assert_eq!(minted.note, "beta wave");
        assert_eq!(minted.codes, vec!["K7Q2M9XPTA", "B4N8R5WCDE"]);
        let printed = format!("{minted:?}");
        assert!(!printed.contains("K7Q2M9XPTA"), "a code reached Debug output: {printed}");
        assert!(printed.contains("2 redacted"));
    }

    #[test]
    fn mint_request_serializes_the_three_fields() {
        let req = StaffMintRequest { days: 7, count: 3, note: "x".into() };
        let v = serde_json::to_value(&req).expect("serialize");
        assert_eq!(v, serde_json::json!({"days": 7, "count": 3, "note": "x"}));
    }

    #[test]
    fn batches_decode_with_and_without_a_creator() {
        let body = r#"[
            {"batch_id":"gb_2","note":"beta wave","days":30,"count":3,"redeemed":1,"revoked":2,
             "created_at":"2026-09-29T10:00:00+00:00","created_by_nick":"alice"},
            {"batch_id":"gb_1","note":"","days":7,"count":10,"redeemed":0,"revoked":0,
             "created_at":"2026-09-01T10:00:00+00:00","created_by_nick":null}
        ]"#;
        let rows: Vec<StaffGiftBatch> = serde_json::from_str(body).expect("batches");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].created_by_nick.as_deref(), Some("alice"));
        assert_eq!((rows[0].redeemed, rows[0].revoked, rows[0].count), (1, 2, 3));
        assert_eq!(rows[1].created_by_nick, None);
        assert_eq!(rows[1].note, "");
    }

    #[test]
    fn revoke_and_paid_days_replies_decode() {
        let revoked: StaffBatchRevoked = serde_json::from_str(r#"{"batch_id":"gb_2","revoked":2}"#).expect("revoke");
        assert_eq!((revoked.batch_id.as_str(), revoked.revoked), ("gb_2", 2));
        let paid: StaffPaidDays =
            serde_json::from_str(r#"{"nick":"alice","paid_until":"2026-11-01T00:00:00+00:00"}"#).expect("paid");
        assert_eq!(paid.paid_until, "2026-11-01T00:00:00+00:00");
    }

    #[test]
    fn lifetime_replies_decode_created_and_existing() {
        let created = r#"{"nick":"alice","grant":"lifetime_paid","grant_id":12,"source":"admin","source_ref":"7:early supporter","created":true}"#;
        let g: StaffLifetimeGrant = serde_json::from_str(created).expect("created");
        assert!(g.created);
        assert_eq!((g.grant_id, g.grant.as_str(), g.source.as_str()), (12, "lifetime_paid", "admin"));
        let existing = r#"{"nick":"alice","grant":"lifetime_paid","grant_id":12,"source":"admin","source_ref":"7:early supporter","created":false}"#;
        let g: StaffLifetimeGrant = serde_json::from_str(existing).expect("existing");
        assert!(!g.created);
        let r: StaffLifetimeRevoked =
            serde_json::from_str(r#"{"nick":"alice","grant":"lifetime_anon","revoked":1}"#).expect("revoke");
        assert_eq!(r.revoked, 1);
    }

    #[test]
    fn user_card_decodes_the_documented_sample() {
        let body = r#"{"nick":"alice","user_id":42,"paid_until":"2026-11-01T00:00:00+00:00",
            "lifetime_paid":true,"lifetime_anon":false,
            "lifetime_grants":[
              {"grant_id":12,"grant":"lifetime_paid","source":"admin","source_ref":"7:early supporter",
               "created_at":"2026-09-01T00:00:00+00:00","revoked_at":null},
              {"grant_id":9,"grant":"lifetime_anon","source":"invoice","source_ref":"inv_9",
               "created_at":"2026-08-01T00:00:00+00:00","revoked_at":"2026-08-20T00:00:00+00:00"}],
            "quant_tier":null,"quant_active_until":null,"roles":["moderator"]}"#;
        let u: StaffUser = serde_json::from_str(body).expect("user");
        assert_eq!((u.nick.as_str(), u.user_id), ("alice", 42));
        assert_eq!(u.paid_until.as_deref(), Some("2026-11-01T00:00:00+00:00"));
        assert!(u.lifetime_paid && !u.lifetime_anon);
        assert_eq!(u.lifetime_grants.len(), 2);
        assert_eq!(u.lifetime_grants[0].revoked_at, None);
        assert_eq!(u.lifetime_grants[1].revoked_at.as_deref(), Some("2026-08-20T00:00:00+00:00"));
        assert_eq!(u.quant_tier, None);
        assert_eq!(u.roles, vec!["moderator"]);
    }

    #[test]
    fn user_card_tolerates_a_bare_account() {
        let u: StaffUser = serde_json::from_str(r#"{"nick":"bob","user_id":7}"#).expect("bare user");
        assert!(u.lifetime_grants.is_empty() && u.roles.is_empty());
        assert_eq!(u.paid_until, None);
    }

    #[test]
    fn forbidden_maps_to_forbidden() {
        let e = CloudError::Forbidden(r#"{"error":"forbidden"}"#.to_string());
        assert_eq!(staff_error(&e), StaffError::Forbidden);
    }

    #[test]
    fn not_found_maps_to_not_found() {
        let e = CloudError::Http { status: 404, body: r#"{"error":"no such user"}"#.to_string() };
        assert_eq!(staff_error(&e), StaffError::NotFound);
    }

    #[test]
    fn bad_request_keeps_the_server_sentence() {
        let e = CloudError::Http {
            status: 400,
            body: r#"{"error":"days must be between 1 and 365"}"#.to_string(),
        };
        assert_eq!(
            staff_error(&e),
            StaffError::BadRequest { detail: "days must be between 1 and 365".to_string() }
        );
        let bare = CloudError::Http { status: 400, body: "  oops ".to_string() };
        assert_eq!(staff_error(&bare), StaffError::BadRequest { detail: "oops".to_string() });
    }

    #[test]
    fn rate_limit_reads_the_folded_retry_after() {
        let raw = r#"{"error":"rate_limited"}"#.to_string();
        let folded = with_retry_after(429, raw.clone(), Some(60));
        let e = CloudError::Http { status: 429, body: folded };
        assert_eq!(staff_error(&e), StaffError::RateLimited { retry_after_secs: Some(60) });
        let unfolded = CloudError::Http { status: 429, body: raw };
        assert_eq!(staff_error(&unfolded), StaffError::RateLimited { retry_after_secs: None });
    }

    #[test]
    fn retry_after_is_only_folded_into_a_429() {
        let body = r#"{"error":"nope"}"#.to_string();
        assert_eq!(with_retry_after(400, body.clone(), Some(30)), body);
        assert_eq!(with_retry_after(429, body.clone(), None), body);
        let folded = with_retry_after(429, "not json".to_string(), Some(5));
        let v: serde_json::Value = serde_json::from_str(&folded).expect("folded body is json");
        assert_eq!(v["retry_after_secs"], 5);
    }

    #[test]
    fn session_and_transport_failures_are_classified() {
        assert_eq!(staff_error(&CloudError::Unauthorized), StaffError::SignedOut);
        assert_eq!(
            staff_error(&CloudError::Transport("offline".to_string())),
            StaffError::Network { detail: "offline".to_string() }
        );
        let e = CloudError::Http { status: 500, body: r#"{"error":"Database error"}"#.to_string() };
        assert_eq!(
            staff_error(&e),
            StaffError::Server { detail: "HTTP 500: Database error".to_string() }
        );
        let d = staff_error(&CloudError::Decode("bad".to_string()));
        assert!(matches!(d, StaffError::Server { .. }));
    }
}
