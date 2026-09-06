//! Agent API auth — Bearer key against a sealed, TTL-bounded key store.
//!
//! Two layers, wired in `lib.rs`:
//! - [`auth_middleware`] (outermost): resolves a `Authorization: Bearer`
//!   token against the profile's minted [`AgentKeyRecord`]s and injects
//!   [`CallerTier`] + [`CallerRole`] extensions on success.
//! - [`tier_gate`] (per route module, innermost): reads [`CallerTier`] and
//!   refuses a caller whose key tier is below the route's requirement.
//!
//! Mutation permission ([`AgentKeyRole`]) is checked by HTTP method inside
//! [`auth_middleware`] itself rather than per-route: every `POST`/`PUT`/
//! `PATCH`/`DELETE` needs [`AgentKeyRole::Control`], every `GET`/`HEAD`
//! passes either role.
//!
//! # Where the git history was reused from
//!
//! An earlier `zengeld-server::auth`/`routes::keys` pair (deleted by the
//! `oss-strip 2/N` commit, `d81c4dbb`) shaped this design: Bearer-header
//! parsing, SHA-256 key fingerprinting instead of storing the raw key, and
//! injecting request extensions on a match. Not reused: its "empty key
//! registry disables auth" bootstrap mode (a sealed/empty store here means
//! `423`/`401`, never open access), its query-param fallback, and its
//! string-typed `tier`/`admin` permission table — `AgentKeyRole` (`Control`/
//! `ReadOnly`) plus a route-level [`Tier`] replace it.

use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::{header, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Extension, Json, Router,
};
use serde::{Deserialize, Serialize};

use mlc_vault::crypto::{constant_time_eq, fingerprint_sha256};
use mlc_vault::{SecretStore, Sealed};

// ===========================================================================
// Tier / role / key-record types
// ===========================================================================

/// Minimum access level a route requires, checked by [`tier_gate`]. All
/// routes use [`Tier::SignedIn`] today; [`Tier::Paid`] is reserved for a
/// future paid-only route — flip that route's own `tier_gate` argument, no
/// other change needed. Ordered so `caller_tier < required_tier` is exactly
/// "refuse".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Tier {
    SignedIn,
    Paid,
}

/// Mutation permission attached to a minted key. [`Control`](Self::Control)
/// may call a mutating route; [`ReadOnly`](Self::ReadOnly) may only `GET`/
/// `HEAD`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentKeyRole {
    Control,
    ReadOnly,
}

/// One minted agent API key's public record — persisted as a
/// `Vec<AgentKeyRecord>` JSON array inside the `agent_keys.enc` sidecar
/// (encrypted with the profile's `VaultKey`, held in RAM by
/// [`crate::AgentState::agent_key_store`]). Never carries the raw key —
/// only its SHA-256 fingerprint, hex-encoded so it round-trips through JSON
/// and compares byte-for-byte against a freshly fingerprinted Bearer token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentKeyRecord {
    pub fingerprint_hex: String,
    pub label: String,
    pub tier: Tier,
    pub role: AgentKeyRole,
    pub created_at_ms: u64,
}

/// Request extension inserted by [`auth_middleware`] on a successful match.
#[derive(Debug, Clone, Copy)]
pub struct CallerTier(pub Tier);

/// Request extension inserted by [`auth_middleware`] on a successful match.
#[derive(Debug, Clone, Copy)]
pub struct CallerRole(pub AgentKeyRole);

// ===========================================================================
// Error envelope
// ===========================================================================

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

fn error_response(status: StatusCode, message: &str) -> Response {
    (status, Json(ErrorBody { error: message.to_string() })).into_response()
}

/// Parse the `agent_keys.enc` sidecar payload into its key records.
///
/// A fresh, never-written store decrypts to the literal bytes `b"{}"` (see
/// [`SecretStore::unlock`]'s doc — a missing vault file is not a failed
/// unlock) which is a JSON object, not an array, so any parse failure here
/// is read as "no keys minted yet" rather than propagated as an error.
pub(crate) fn parse_key_records(bytes: &[u8]) -> Vec<AgentKeyRecord> {
    serde_json::from_slice(bytes).unwrap_or_default()
}

// ===========================================================================
// Sidecar paths
// ===========================================================================

/// Compute the `agent_keys.enc` sidecar's `(salt_path, vault_path)` pair
/// from a profile's data directory.
///
/// The salt file is the SAME `salt.hex` the profile's own `vault.enc`
/// already uses — the sidecar is encrypted under the identical `VaultKey`
/// (see this module's doc, "Storage"), so there is no separate salt to
/// derive, only a different payload path. Kept as the single source of
/// truth so the render-thread mint/revoke path
/// (`mlc-app-agent-api::commands`'s `CreateKey`/`DeleteKey` handling) and
/// the host's initial `SecretStore` construction
/// (`mlc-app-shell-vello::main`, wiring `AgentState::with_agent_key_store`)
/// can never drift onto different paths for the same store.
pub fn agent_keys_paths(profile_dir: &std::path::Path) -> (std::path::PathBuf, std::path::PathBuf) {
    (profile_dir.join("salt.hex"), profile_dir.join("agent_keys.enc"))
}

// ===========================================================================
// Outer auth middleware
// ===========================================================================

/// Outermost Agent API middleware (see `lib.rs`'s wiring order).
///
/// Takes `Arc<SecretStore>` directly as its captured middleware state —
/// deliberately not the full `Arc<AgentState>` — so this decision has no
/// dependency on the data bridge / backend core and can be exercised in a
/// unit test with nothing more than a `SecretStore` pointed at a temp
/// directory (see the `tests` module below).
///
/// - No `Authorization: Bearer` token at all (missing header, or a header
///   that is not the `Bearer <token>` shape) → `401`, checked BEFORE the
///   store is ever touched — an unauthenticated caller must not learn
///   whether the store is sealed or unlocked.
/// - Only once a well-formed bearer token is in hand: sealed store
///   (`agent_key_store.get()` → `Err(Sealed)`) → `423`.
/// - A well-formed bearer whose fingerprint matches no record → `401`.
/// - A mutating method (`POST`/`PUT`/`PATCH`/`DELETE`) presented by a
///   [`AgentKeyRole::ReadOnly`] key → `403`.
/// - Otherwise: [`CallerTier`] + [`CallerRole`] extensions are inserted and
///   the request proceeds to the per-module [`tier_gate`] check.
pub async fn auth_middleware(
    State(agent_key_store): State<Arc<SecretStore>>,
    mut req: Request,
    next: Next,
) -> Response {
    let bearer = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::to_string);

    let Some(bearer) = bearer else {
        return error_response(StatusCode::UNAUTHORIZED, "unauthorized");
    };

    let bytes = match agent_key_store.get() {
        Ok(bytes) => bytes,
        Err(Sealed) => return error_response(StatusCode::LOCKED, "vault sealed"),
    };

    let fingerprint_hex = hex::encode(fingerprint_sha256(bearer.as_bytes()));
    let records = parse_key_records(&bytes);
    let Some(record) = records
        .iter()
        .find(|r| constant_time_eq(r.fingerprint_hex.as_bytes(), fingerprint_hex.as_bytes()))
    else {
        return error_response(StatusCode::UNAUTHORIZED, "unauthorized");
    };

    let is_mutating = matches!(
        *req.method(),
        Method::POST | Method::PUT | Method::PATCH | Method::DELETE
    );
    if is_mutating && record.role != AgentKeyRole::Control {
        return error_response(
            StatusCode::FORBIDDEN,
            "read-only key cannot call a mutating route",
        );
    }

    req.extensions_mut().insert(CallerTier(record.tier));
    req.extensions_mut().insert(CallerRole(record.role));

    next.run(req).await
}

// ===========================================================================
// Per-module tier gate
// ===========================================================================

/// Wrap a module's route sub-router with a minimum [`Tier`] requirement.
///
/// Reads the [`CallerTier`] extension [`auth_middleware`] already inserted
/// — this layer runs INSIDE it (route-local layers are always inner to
/// whatever is `.layer()`ed on the router they get merged into; see
/// `lib.rs`'s wiring order), so the extension is always present by the time
/// this check runs. Generic over the router's state so it composes with
/// both the real `Router<Arc<AgentState>>` and a state-free test router.
pub fn tier_gate<S>(router: Router<S>, min_tier: Tier) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    router.route_layer(axum::middleware::from_fn(
        move |Extension(CallerTier(tier)): Extension<CallerTier>, req: Request, next: Next| async move {
            if tier < min_tier {
                return error_response(StatusCode::FORBIDDEN, "tier too low for this route");
            }
            next.run(req).await
        },
    ))
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    use axum::body::Body;
    use axum::routing::{get, post};
    use tower::ServiceExt;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap_or_else(|e| {
            panic!("test setup: create_dir_all({dir:?}) failed: {e}")
        });
        dir
    }

    /// Build a `SecretStore` bound to a fresh temp `salt.hex`/`agent_keys.enc`
    /// pair, unlocked and pre-loaded with `records`.
    fn unlocked_store_with(name: &str, records: &[AgentKeyRecord]) -> Arc<SecretStore> {
        let dir = temp_dir(name);
        let (salt_path, vault_path) = agent_keys_paths(&dir);

        let salt = mlc_vault::load_or_create_salt(&salt_path).expect("create salt");
        let key = mlc_vault::derive_key("test passphrase", &salt);
        mlc_vault::save_encrypted(&key, &vault_path, &records.to_vec()).expect("seed records");

        let store = Arc::new(SecretStore::with_ttls(
            salt_path,
            vault_path,
            None,
            std::time::Duration::from_secs(24 * 60 * 60),
        ));
        store.unlock("test passphrase").expect("unlock seeded store");
        store
    }

    fn sealed_store(name: &str) -> Arc<SecretStore> {
        let dir = temp_dir(name);
        let (salt_path, vault_path) = agent_keys_paths(&dir);
        Arc::new(SecretStore::with_ttls(
            salt_path,
            vault_path,
            None,
            std::time::Duration::from_secs(24 * 60 * 60),
        ))
    }

    fn control_record(fingerprint_hex: String) -> AgentKeyRecord {
        AgentKeyRecord {
            fingerprint_hex,
            label: "control key".to_string(),
            tier: Tier::SignedIn,
            role: AgentKeyRole::Control,
            created_at_ms: 0,
        }
    }

    fn read_only_record(fingerprint_hex: String) -> AgentKeyRecord {
        AgentKeyRecord {
            fingerprint_hex,
            label: "read-only key".to_string(),
            tier: Tier::SignedIn,
            role: AgentKeyRole::ReadOnly,
            created_at_ms: 0,
        }
    }

    fn test_router(key_store: Arc<SecretStore>, min_tier: Tier) -> Router<()> {
        let router: Router<()> = Router::new()
            .route("/probe", get(|| async { StatusCode::OK }))
            .route("/probe", post(|| async { StatusCode::OK }));
        tier_gate(router, min_tier)
            .layer(axum::middleware::from_fn_with_state(key_store, auth_middleware))
    }

    fn get_request(bearer: Option<&str>) -> Request<Body> {
        let mut builder = Request::builder().method("GET").uri("/probe");
        if let Some(token) = bearer {
            builder = builder.header("authorization", format!("Bearer {token}"));
        }
        builder.body(Body::empty()).expect("build GET request")
    }

    fn post_request(bearer: Option<&str>) -> Request<Body> {
        let mut builder = Request::builder().method("POST").uri("/probe");
        if let Some(token) = bearer {
            builder = builder.header("authorization", format!("Bearer {token}"));
        }
        builder.body(Body::empty()).expect("build POST request")
    }

    #[tokio::test]
    async fn sealed_store_answers_423() {
        let store = sealed_store("auth_mw_sealed");
        let router = test_router(store, Tier::SignedIn);
        let res = router.oneshot(get_request(Some("whatever"))).await.expect("dispatch");
        assert_eq!(res.status(), StatusCode::LOCKED);
    }

    #[tokio::test]
    async fn missing_bearer_answers_401() {
        let store = unlocked_store_with("auth_mw_missing_bearer", &[]);
        let router = test_router(store, Tier::SignedIn);
        let res = router.oneshot(get_request(None)).await.expect("dispatch");
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn missing_bearer_on_sealed_store_answers_401_not_423() {
        // The header check must run BEFORE the store is ever touched — an
        // unauthenticated caller must not be able to distinguish a sealed
        // store from an unlocked one just by omitting the header.
        let store = sealed_store("auth_mw_missing_bearer_sealed");
        let router = test_router(store, Tier::SignedIn);
        let res = router.oneshot(get_request(None)).await.expect("dispatch");
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn unknown_bearer_answers_401() {
        let known = control_record(hex::encode(fingerprint_sha256(b"known-raw-key")));
        let store = unlocked_store_with("auth_mw_unknown_bearer", &[known]);
        let router = test_router(store, Tier::SignedIn);
        let res = router.oneshot(get_request(Some("some-other-key"))).await.expect("dispatch");
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn matching_control_key_answers_200_on_get() {
        let raw = "control-raw-key";
        let record = control_record(hex::encode(fingerprint_sha256(raw.as_bytes())));
        let store = unlocked_store_with("auth_mw_control_get", &[record]);
        let router = test_router(store, Tier::SignedIn);
        let res = router.oneshot(get_request(Some(raw))).await.expect("dispatch");
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn matching_control_key_answers_200_on_post() {
        let raw = "control-raw-key-2";
        let record = control_record(hex::encode(fingerprint_sha256(raw.as_bytes())));
        let store = unlocked_store_with("auth_mw_control_post", &[record]);
        let router = test_router(store, Tier::SignedIn);
        let res = router.oneshot(post_request(Some(raw))).await.expect("dispatch");
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn read_only_key_answers_403_on_post() {
        let raw = "read-only-raw-key";
        let record = read_only_record(hex::encode(fingerprint_sha256(raw.as_bytes())));
        let store = unlocked_store_with("auth_mw_readonly_post", &[record]);
        let router = test_router(store, Tier::SignedIn);
        let res = router.oneshot(post_request(Some(raw))).await.expect("dispatch");
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn read_only_key_answers_200_on_get() {
        let raw = "read-only-raw-key-2";
        let record = read_only_record(hex::encode(fingerprint_sha256(raw.as_bytes())));
        let store = unlocked_store_with("auth_mw_readonly_get", &[record]);
        let router = test_router(store, Tier::SignedIn);
        let res = router.oneshot(get_request(Some(raw))).await.expect("dispatch");
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn signed_in_key_below_paid_gate_answers_403() {
        let raw = "signed-in-raw-key";
        let record = control_record(hex::encode(fingerprint_sha256(raw.as_bytes())));
        let store = unlocked_store_with("auth_mw_tier_gate", &[record]);
        // Gate the route at Tier::Paid — a SignedIn-tier key must be refused.
        let router = test_router(store, Tier::Paid);
        let res = router.oneshot(get_request(Some(raw))).await.expect("dispatch");
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }
}
