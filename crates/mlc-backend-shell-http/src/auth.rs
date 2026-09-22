//! Agent API auth — Bearer key against a sealed, TTL-bounded key store.
//!
//! Two layers, wired in `lib.rs`:
//! - [`auth_middleware`] (outermost): resolves a `Authorization: Bearer`
//!   token against the profile's minted [`AgentKeyRecord`]s and injects
//!   [`CallerTier`] + [`CallerRole`] extensions on success.
//! - [`tier_gate`] (per route module, innermost): reads [`CallerTier`] and
//!   refuses a caller whose key tier is below the route's requirement.
//!
//! [`auth_middleware`] also carries its own sliding-window
//! [`BearerFailureTracker`] over the Bearer-key comparison itself — insurance
//! against unbounded `401` guessing, not the primary defense (the primary
//! defense is the loopback-only bind plus the 32-byte key's entropy; see
//! that struct's doc). Deliberately its own counter, not a reuse of
//! [`crate::state::AgentState`]'s vault-unlock counter — the two guard
//! different credentials at different, independently-justified thresholds.
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

use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

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

/// Decode a Bearer token into the 32 raw key bytes it must be, or `None` for
/// anything else.
///
/// A minted key is always presented as `hex::encode(raw_key)` — exactly 64
/// lowercase-or-uppercase hex characters encoding 32 bytes (see
/// `mlc-app-agent-api::commands`'s `CreateKey` handling). This is checked
/// BEFORE the key is ever fingerprinted: fingerprinting the bearer STRING
/// instead of these decoded BYTES (the defect this function replaces) meant
/// every minted key answered `401` forever, because `commands.rs` fingerprints
/// the bytes it hands out, never the hex string. A bearer that is not exactly
/// 64 hex characters is never fingerprinted at all — there is nothing for it
/// to legitimately match.
fn decode_bearer_key(bearer: &str) -> Option<[u8; 32]> {
    if bearer.len() != 64 {
        return None;
    }
    let bytes = hex::decode(bearer).ok()?;
    bytes.try_into().ok()
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
// Bearer-check failure tracking
// ===========================================================================

/// [`BearerFailureTracker`]'s window: at most [`BEARER_FAILURE_LIMIT`]
/// failed Bearer-key comparisons inside this many seconds before
/// [`auth_middleware`] answers `429` without even attempting the next
/// comparison.
///
/// Deliberately loose. The Agent API binds `127.0.0.1` only (see this
/// crate's doc) and a minted key is 32 random bytes
/// ([`decode_bearer_key`]) — guessing one is not a realistic threat on this
/// surface today, so this counter is insurance for the day the
/// loopback-only assumption stops holding, not an active brute-force
/// defense (the audit finding this closes:
/// `nemo/docs/mlc/audits/api-surface-hardening-2026-09-10.md` §5). It is
/// also deliberately NOT the vault-unlock window
/// (`crate::state::VAULT_UNLOCK_FAILURE_WINDOW`, 5 failures / 60s) — that
/// one throttles a PBKDF2-bound passphrase (600,000 rounds per attempt,
/// genuinely expensive to spam), while this one throttles a cheap in-memory
/// hash compare against a caller that may legitimately retry a
/// just-expired/just-revoked key several times in a row while its owner
/// notices and re-mints. The threshold stays wide (20) and the window short
/// (10s) so a tripped lock ages itself out within 10 seconds of the last
/// failure even if the caller never succeeds, and a caller presenting a
/// freshly minted correct key clears it immediately on the very next match
/// (see [`BearerFailureTracker::clear`]) rather than staying locked out by
/// attempts that happened before it.
const BEARER_FAILURE_WINDOW: Duration = Duration::from_secs(10);

/// See [`BEARER_FAILURE_WINDOW`].
const BEARER_FAILURE_LIMIT: usize = 20;

/// Sliding-window failure counter over the Bearer-key comparison
/// [`auth_middleware`] performs against the unlocked key store. See
/// [`BEARER_FAILURE_WINDOW`]'s doc for the limit/window rationale.
///
/// Checked only AFTER the store is confirmed unlocked (a sealed store
/// already answers `423` before this is ever reached — see
/// [`auth_middleware`]'s doc) and BEFORE the fingerprint comparison for the
/// current request, so a rate-limited caller never even reaches the
/// compare step.
pub struct BearerFailureTracker {
    limit: usize,
    window: Duration,
    failures: Mutex<Vec<web_time::Instant>>,
}

impl BearerFailureTracker {
    /// Production tracker: [`BEARER_FAILURE_LIMIT`] failures inside
    /// [`BEARER_FAILURE_WINDOW`].
    pub fn new() -> Self {
        Self::with_limit(BEARER_FAILURE_LIMIT, BEARER_FAILURE_WINDOW)
    }

    /// Build a tracker with a caller-chosen limit/window. Used by this
    /// module's own unit tests to exercise the sliding window without a
    /// real 10-second sleep; [`Self::new`] is the only production caller.
    fn with_limit(limit: usize, window: Duration) -> Self {
        Self {
            limit,
            window,
            failures: Mutex::new(Vec::new()),
        }
    }

    fn prune(&self, failures: &mut Vec<web_time::Instant>) {
        let now = web_time::Instant::now();
        let window = self.window;
        failures.retain(|t| now.duration_since(*t) < window);
    }

    /// Whether [`Self::limit`] failures have already landed inside the
    /// window — checked by [`auth_middleware`] before it attempts the next
    /// comparison.
    pub fn is_rate_limited(&self) -> bool {
        let mut failures = self
            .failures
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.prune(&mut failures);
        failures.len() >= self.limit
    }

    /// Record one failed Bearer-key comparison (fingerprint matched no
    /// minted key).
    pub fn record_failure(&self) {
        let mut failures = self
            .failures
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.prune(&mut failures);
        failures.push(web_time::Instant::now());
    }

    /// Clear the counter — called on a successful Bearer match, so a
    /// legitimate caller is never throttled by attempts that happened
    /// before it (same reasoning as
    /// `crate::state::AgentState::clear_vault_unlock_failures`).
    pub fn clear(&self) {
        if let Ok(mut failures) = self.failures.lock() {
            failures.clear();
        }
    }
}

impl Default for BearerFailureTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// [`auth_middleware`]'s captured state: the key store plus its sibling
/// [`BearerFailureTracker`]. Kept as a small bespoke struct — not the full
/// `Arc<AgentState>` — for the same testability reason [`auth_middleware`]'s
/// own doc gives for the store alone: this middleware can be exercised with
/// nothing more than a `SecretStore` and a counter, no data bridge or
/// backend core required.
#[derive(Clone)]
pub struct AuthState {
    pub agent_key_store: Arc<RwLock<Arc<SecretStore>>>,
    pub bearer_failures: Arc<BearerFailureTracker>,
}

// ===========================================================================
// Outer auth middleware
// ===========================================================================

/// Outermost Agent API middleware (see `lib.rs`'s wiring order).
///
/// Takes [`AuthState`] directly as its captured middleware state —
/// deliberately not the full `Arc<AgentState>` — so this decision has no
/// dependency on the data bridge / backend core and can be exercised in a
/// unit test with nothing more than a `SecretStore` pointed at a temp
/// directory and a fresh [`BearerFailureTracker`] (see the `tests` module
/// below). The `RwLock` layer around the store is what lets a profile
/// switch RE-POINT the store this middleware checks against
/// ([`crate::AgentState::set_agent_key_store`]) without rebuilding the
/// router: `lib.rs` clones the SAME `Arc<RwLock<..>>` out of
/// `AgentState::agent_key_store` (an `Arc` pointer copy) into this
/// middleware's state, so the two always agree — a read here takes the
/// lock fresh on every request rather than trusting a snapshot captured
/// once at router-build time.
///
/// - No `Authorization: Bearer` token at all (missing header, or a header
///   that is not the `Bearer <token>` shape) → `401`, checked BEFORE the
///   store is ever touched — an unauthenticated caller must not learn
///   whether the store is sealed or unlocked.
/// - A bearer that is not exactly 64 hex characters (i.e. not a
///   `hex::encode`d 32-byte raw key — see [`decode_bearer_key`]) → `401`,
///   checked BEFORE the store is touched for the same reason as the missing
///   header above, and BEFORE any fingerprinting: an arbitrary string is
///   never hashed and compared, only the decoded key bytes are.
/// - Only once a well-formed (64-hex-char) bearer is in hand: sealed store
///   (`agent_key_store.get()` → `Err(Sealed)`) → `423`. This still runs
///   BEFORE the failure-throttle check below — the documented
///   401-before-423 ordering above (a fix in its own right, covered by
///   `missing_bearer_on_sealed_store_answers_401_not_423` and
///   `malformed_bearer_on_sealed_store_answers_401_not_423`) is untouched
///   by the throttle, which only ever runs for a well-formed bearer against
///   an already-unlocked store.
/// - [`BearerFailureTracker::is_rate_limited`] already at the limit → `429`,
///   without attempting the fingerprint comparison for this request.
/// - A well-formed bearer whose fingerprint matches no record → `401`, and
///   the failure is recorded ([`BearerFailureTracker::record_failure`]).
/// - A mutating method (`POST`/`PUT`/`PATCH`/`DELETE`) presented by a
///   [`AgentKeyRole::ReadOnly`] key → `403`.
/// - Otherwise: the failure counter is cleared
///   ([`BearerFailureTracker::clear`]), [`CallerTier`] + [`CallerRole`]
///   extensions are inserted, and the request proceeds to the per-module
///   [`tier_gate`] check.
pub async fn auth_middleware(
    State(AuthState { agent_key_store, bearer_failures }): State<AuthState>,
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

    // Reject anything that is not a plausible 32-byte hex-encoded key BEFORE
    // the store is ever touched — same "an unauthenticated caller must not
    // learn whether the store is sealed" reasoning as the missing-header
    // check above, and the fix for the bearer-fingerprint defect: a raw
    // string bearer must never be fingerprinted directly (see
    // `decode_bearer_key`'s own doc for why that broke every minted key).
    let Some(raw_key) = decode_bearer_key(&bearer) else {
        return error_response(StatusCode::UNAUTHORIZED, "unauthorized");
    };

    // Read the CURRENT store on every request — never a snapshot captured
    // once at router-build time, so a re-point via `set_agent_key_store`
    // is visible to the very next request.
    let store = agent_key_store
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    let bytes = match store.get() {
        Ok(bytes) => bytes,
        Err(Sealed) => return error_response(StatusCode::LOCKED, "vault sealed"),
    };

    // Throttle checked only once the store is confirmed unlocked (the
    // sealed-store 423 above already returned otherwise) and BEFORE the
    // fingerprint comparison — see `BearerFailureTracker`'s doc.
    if bearer_failures.is_rate_limited() {
        return error_response(
            StatusCode::TOO_MANY_REQUESTS,
            "too many failed bearer attempts, try again later",
        );
    }

    let fingerprint_hex = hex::encode(fingerprint_sha256(&raw_key));
    let records = parse_key_records(&bytes);
    let Some(record) = records
        .iter()
        .find(|r| constant_time_eq(r.fingerprint_hex.as_bytes(), fingerprint_hex.as_bytes()))
    else {
        bearer_failures.record_failure();
        return error_response(StatusCode::UNAUTHORIZED, "unauthorized");
    };

    // A matching fingerprint is a successful Bearer authentication even if
    // the role check below still refuses this particular method — clear
    // now so a legitimate caller is never throttled by attempts that
    // happened before it.
    bearer_failures.clear();

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

    /// Mint a raw 32-byte agent key filled with `fill` and return
    /// `(raw_hex, fingerprint_hex)` — the exact `(bearer presented,
    /// fingerprint stored)` pair `AgentCommand::CreateKey` produces
    /// (`hex::encode(raw_key)` handed to the user, `fingerprint_sha256
    /// (&raw_key)` stored in the record). Every test below presents a
    /// bearer shaped exactly like a real minted key rather than an
    /// arbitrary string, now that the middleware requires that shape.
    fn mint_raw_hex(fill: u8) -> (String, String) {
        let raw = [fill; 32];
        (hex::encode(raw), hex::encode(fingerprint_sha256(&raw)))
    }

    fn build_router(auth_state: AuthState, min_tier: Tier) -> Router<()> {
        let router: Router<()> = Router::new()
            .route("/probe", get(|| async { StatusCode::OK }))
            .route("/probe", post(|| async { StatusCode::OK }));
        tier_gate(router, min_tier)
            .layer(axum::middleware::from_fn_with_state(auth_state, auth_middleware))
    }

    fn test_router(key_store: Arc<SecretStore>, min_tier: Tier) -> Router<()> {
        test_router_shared(Arc::new(RwLock::new(key_store)), min_tier)
    }

    /// Same as [`test_router`] but takes the shared lock directly — used by
    /// the re-point test below, which needs to swap the store out from under
    /// an already-built router.
    fn test_router_shared(key_store: Arc<RwLock<Arc<SecretStore>>>, min_tier: Tier) -> Router<()> {
        build_router(
            AuthState {
                agent_key_store: key_store,
                bearer_failures: Arc::new(BearerFailureTracker::new()),
            },
            min_tier,
        )
    }

    /// Same as [`test_router`] but with a caller-supplied
    /// [`BearerFailureTracker`] (typically built via a small test-only
    /// limit/window) — used by the failure-throttle tests below, which
    /// need to observe the SAME counter across a sequence of requests
    /// against a cloned router (`Router` is a cheap `Arc`-backed clone, see
    /// `repointed_store_is_read_on_next_request`).
    fn test_router_with_tracker(
        key_store: Arc<SecretStore>,
        min_tier: Tier,
        bearer_failures: Arc<BearerFailureTracker>,
    ) -> Router<()> {
        build_router(
            AuthState {
                agent_key_store: Arc::new(RwLock::new(key_store)),
                bearer_failures,
            },
            min_tier,
        )
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
        let (raw, _fingerprint) = mint_raw_hex(0x01);
        let res = router.oneshot(get_request(Some(&raw))).await.expect("dispatch");
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
    async fn non_hex_bearer_answers_401() {
        // Not a hex-shaped bearer at all — must never reach the fingerprint
        // step, only the up-front shape check.
        let store = unlocked_store_with("auth_mw_non_hex_bearer", &[]);
        let router = test_router(store, Tier::SignedIn);
        let res = router.oneshot(get_request(Some("not-a-valid-key"))).await.expect("dispatch");
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn malformed_bearer_on_sealed_store_answers_401_not_423() {
        // Same reasoning as `missing_bearer_on_sealed_store_answers_401_not_423`
        // — a bearer that fails the hex-shape check must not learn whether
        // the store behind it is sealed either.
        let store = sealed_store("auth_mw_malformed_bearer_sealed");
        let router = test_router(store, Tier::SignedIn);
        // 64 characters, but not valid hex — length alone must not pass.
        let not_hex = "z".repeat(64);
        let res = router.oneshot(get_request(Some(&not_hex))).await.expect("dispatch");
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn unknown_bearer_answers_401() {
        let (_known_raw, known_fingerprint) = mint_raw_hex(0xAA);
        let (unknown_raw, _unknown_fingerprint) = mint_raw_hex(0xBB);
        let known = control_record(known_fingerprint);
        let store = unlocked_store_with("auth_mw_unknown_bearer", &[known]);
        let router = test_router(store, Tier::SignedIn);
        let res = router.oneshot(get_request(Some(&unknown_raw))).await.expect("dispatch");
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    /// Reproduces the exact mint→present round trip `AgentCommand::CreateKey`
    /// performs: 32 raw bytes, `hex::encode` of THOSE bytes handed to the
    /// user as the bearer, `fingerprint_sha256` of the SAME bytes stored in
    /// the record. Regression test for the bearer-fingerprint defect (owner
    /// 2026-09-09): the middleware used to fingerprint the bearer STRING
    /// instead of decoding it back to bytes first, so this exact round trip
    /// answered `401` for every key ever minted.
    #[tokio::test]
    async fn mint_then_present_round_trip_answers_200() {
        let raw_bytes = [0x42u8; 32];
        let raw_hex = hex::encode(raw_bytes);
        let fingerprint_hex = hex::encode(fingerprint_sha256(&raw_bytes));
        let record = control_record(fingerprint_hex);
        let store = unlocked_store_with("auth_mw_mint_round_trip", &[record]);
        let router = test_router(store, Tier::SignedIn);
        let res = router.oneshot(get_request(Some(&raw_hex))).await.expect("dispatch");
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn matching_control_key_answers_200_on_get() {
        let (raw, fingerprint) = mint_raw_hex(0x02);
        let record = control_record(fingerprint);
        let store = unlocked_store_with("auth_mw_control_get", &[record]);
        let router = test_router(store, Tier::SignedIn);
        let res = router.oneshot(get_request(Some(&raw))).await.expect("dispatch");
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn matching_control_key_answers_200_on_post() {
        let (raw, fingerprint) = mint_raw_hex(0x03);
        let record = control_record(fingerprint);
        let store = unlocked_store_with("auth_mw_control_post", &[record]);
        let router = test_router(store, Tier::SignedIn);
        let res = router.oneshot(post_request(Some(&raw))).await.expect("dispatch");
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn read_only_key_answers_403_on_post() {
        let (raw, fingerprint) = mint_raw_hex(0x04);
        let record = read_only_record(fingerprint);
        let store = unlocked_store_with("auth_mw_readonly_post", &[record]);
        let router = test_router(store, Tier::SignedIn);
        let res = router.oneshot(post_request(Some(&raw))).await.expect("dispatch");
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn read_only_key_answers_200_on_get() {
        let (raw, fingerprint) = mint_raw_hex(0x05);
        let record = read_only_record(fingerprint);
        let store = unlocked_store_with("auth_mw_readonly_get", &[record]);
        let router = test_router(store, Tier::SignedIn);
        let res = router.oneshot(get_request(Some(&raw))).await.expect("dispatch");
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn signed_in_key_below_paid_gate_answers_403() {
        let (raw, fingerprint) = mint_raw_hex(0x06);
        let record = control_record(fingerprint);
        let store = unlocked_store_with("auth_mw_tier_gate", &[record]);
        // Gate the route at Tier::Paid — a SignedIn-tier key must be refused.
        let router = test_router(store, Tier::Paid);
        let res = router.oneshot(get_request(Some(&raw))).await.expect("dispatch");
        assert_eq!(res.status(), StatusCode::FORBIDDEN);
    }

    /// Proves `auth_middleware` reads the CURRENT store on every request,
    /// not a snapshot captured once at router-build time: swap the shared
    /// `RwLock`'s inner `Arc` to a DIFFERENT store between two requests
    /// against the SAME already-built router, and the second request must
    /// answer against the new store — this is exactly the re-point
    /// `AgentState::set_agent_key_store` performs on a profile switch.
    #[tokio::test]
    async fn repointed_store_is_read_on_next_request() {
        let (raw, fingerprint) = mint_raw_hex(0x07);
        let record = control_record(fingerprint);
        let store_a = unlocked_store_with("auth_mw_repoint_a", &[record]);
        let shared = Arc::new(RwLock::new(store_a));
        let router = test_router_shared(shared.clone(), Tier::SignedIn);

        // Profile A's key works.
        let res = router.clone().oneshot(get_request(Some(&raw))).await.expect("dispatch");
        assert_eq!(res.status(), StatusCode::OK);

        // Re-point to profile B's store, which has no such key.
        let store_b = unlocked_store_with("auth_mw_repoint_b", &[]);
        *shared.write().expect("lock not poisoned") = store_b;

        // The SAME router, SAME raw key — now answers 401 because it is
        // checked against profile B's (different) store.
        let res = router.oneshot(get_request(Some(&raw))).await.expect("dispatch");
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    // -- BearerFailureTracker ------------------------------------------------
    //
    // `matching_control_key_answers_200_on_get` and
    // `mint_then_present_round_trip_answers_200` above already cover "a
    // correct key still passes with an empty (freshly built) failure
    // counter" — every `test_router` call builds a fresh
    // `BearerFailureTracker`, so those two are also throttle regression
    // tests. The three below exercise the counter itself.

    /// [`BearerFailureTracker::is_rate_limited`] trips once its limit is
    /// reached inside the window — even a WELL-FORMED bearer whose
    /// fingerprint would otherwise be checked gets `429` without a
    /// comparison once the limit is hit.
    #[tokio::test]
    async fn bearer_failure_limit_trips_429_after_threshold_failures_in_window() {
        let (known_raw, known_fingerprint) = mint_raw_hex(0x10);
        let (unknown_raw, _unknown_fingerprint) = mint_raw_hex(0x11);
        let known = control_record(known_fingerprint);
        let store = unlocked_store_with("auth_mw_throttle_trip", &[known]);
        let tracker = Arc::new(BearerFailureTracker::with_limit(3, Duration::from_secs(60)));
        let router = test_router_with_tracker(store, Tier::SignedIn, tracker);

        // Three failures with an unknown (but well-formed) bearer land the
        // counter exactly at the limit.
        for _ in 0..3 {
            let res = router
                .clone()
                .oneshot(get_request(Some(&unknown_raw)))
                .await
                .expect("dispatch");
            assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        }

        // The fourth request — even with the CORRECT key — is throttled
        // before the comparison ever runs.
        let res = router
            .oneshot(get_request(Some(&known_raw)))
            .await
            .expect("dispatch");
        assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    /// A successful Bearer match clears [`BearerFailureTracker`] — attempts
    /// that happened before a legitimate caller's request must never carry
    /// over and throttle it later.
    #[tokio::test]
    async fn successful_bearer_auth_clears_failure_counter() {
        let (known_raw, known_fingerprint) = mint_raw_hex(0x12);
        let (unknown_raw, _unknown_fingerprint) = mint_raw_hex(0x13);
        let known = control_record(known_fingerprint);
        let store = unlocked_store_with("auth_mw_throttle_clear", &[known]);
        let tracker = Arc::new(BearerFailureTracker::with_limit(3, Duration::from_secs(60)));
        let router = test_router_with_tracker(store, Tier::SignedIn, tracker);

        // Two failures — one short of limit=3, so the counter is not yet
        // tripped.
        for _ in 0..2 {
            let res = router
                .clone()
                .oneshot(get_request(Some(&unknown_raw)))
                .await
                .expect("dispatch");
            assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        }

        // The correct key succeeds and clears the counter.
        let res = router
            .clone()
            .oneshot(get_request(Some(&known_raw)))
            .await
            .expect("dispatch");
        assert_eq!(res.status(), StatusCode::OK);

        // Two MORE failures still fall short of the limit — if the earlier
        // two had survived the clear, this pair would already total 4 >= 3
        // and trip the throttle here instead.
        for _ in 0..2 {
            let res = router
                .clone()
                .oneshot(get_request(Some(&unknown_raw)))
                .await
                .expect("dispatch");
            assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        }
    }

    /// Failures older than [`BearerFailureTracker`]'s window are pruned — a
    /// tripped throttle ages itself out on its own once the offending burst
    /// is far enough in the past, without needing a success to clear it.
    #[tokio::test]
    async fn bearer_failures_older_than_window_are_forgotten() {
        let (known_raw, known_fingerprint) = mint_raw_hex(0x14);
        let (unknown_raw, _unknown_fingerprint) = mint_raw_hex(0x15);
        let known = control_record(known_fingerprint);
        let store = unlocked_store_with("auth_mw_throttle_window", &[known]);
        let tracker = Arc::new(BearerFailureTracker::with_limit(2, Duration::from_millis(50)));
        let router = test_router_with_tracker(store, Tier::SignedIn, tracker);

        // Two failures trip the limit=2 throttle immediately.
        for _ in 0..2 {
            let res = router
                .clone()
                .oneshot(get_request(Some(&unknown_raw)))
                .await
                .expect("dispatch");
            assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        }
        let res = router
            .clone()
            .oneshot(get_request(Some(&known_raw)))
            .await
            .expect("dispatch");
        assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);

        // Once the window has fully elapsed, the old failures are pruned
        // and the correct key passes without needing an explicit clear.
        tokio::time::sleep(Duration::from_millis(80)).await;
        let res = router
            .oneshot(get_request(Some(&known_raw)))
            .await
            .expect("dispatch");
        assert_eq!(res.status(), StatusCode::OK);
    }
}
