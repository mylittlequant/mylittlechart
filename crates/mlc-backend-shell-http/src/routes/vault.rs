//! Loopback-only vault routes — the ONE Agent API surface that sits OUTSIDE
//! [`crate::auth::auth_middleware`] (see `lib.rs`'s wiring: this sub-router
//! is merged without the auth/tier layers at all). The server only ever
//! binds `127.0.0.1` (see the crate doc), and here the passphrase itself is
//! the credential — there is no bearer to check, and checking one would only
//! add a second secret an agent must already hold before it could ever use
//! this route to recover from the first restart that sealed the vault.
//!
//! Routes:
//! - `POST /api/v1/vault/unlock` — `{"passphrase": "..."}`. `204` on success
//!   (also when the vault is already unlocked this session — no
//!   re-derivation), `401 {"error":"wrong passphrase"}` on a bad passphrase,
//!   `404` when the active profile has no vault at all, `429` after 5 failed
//!   attempts within 60 seconds ([`AgentState::vault_unlock_rate_limited`]).
//!   Never logs the passphrase (see `mlc-app-agent-api::commands`'s
//!   `UnlockVault` handling).
//! - `GET  /api/v1/vault/status` — `{"unlocked": bool, "has_vault": bool}`.
//!   Carries no secret, so it needs no rate limit either.

use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::State,
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::state::{AgentCommand, VaultUnlockError};
use crate::AgentState;

// ---------------------------------------------------------------------------
// POST /api/v1/vault/unlock
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct UnlockRequest {
    passphrase: String,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

/// `POST /api/v1/vault/unlock` — see the module doc for the full status
/// contract. The rate-limit check runs BEFORE anything is queued, so a
/// throttled caller never reaches PBKDF2 on the render thread.
async fn unlock_vault(
    State(state): State<Arc<AgentState>>,
    Json(body): Json<UnlockRequest>,
) -> Result<StatusCode, (StatusCode, Json<ErrorResponse>)> {
    if state.vault_unlock_rate_limited() {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            Json(ErrorResponse {
                error: "too many failed unlock attempts, try again later".to_string(),
            }),
        ));
    }

    let (tx, rx) = tokio::sync::oneshot::channel::<Result<(), VaultUnlockError>>();
    state.push_command(AgentCommand::UnlockVault {
        passphrase: body.passphrase,
        response_tx: tx,
    });

    // 10s, not the 5s other round trips use — passphrase derivation is
    // 600,000 rounds of PBKDF2 (`mlc_vault::crypto::PBKDF2_ITERATIONS`) run
    // synchronously on the render thread, and a slower machine deserves more
    // room before this reads as a timeout rather than a slow, honest answer.
    match tokio::time::timeout(Duration::from_secs(10), rx).await {
        Ok(Ok(Ok(()))) => {
            state.clear_vault_unlock_failures();
            Ok(StatusCode::NO_CONTENT)
        }
        Ok(Ok(Err(VaultUnlockError::WrongPassphrase))) => {
            state.record_vault_unlock_failure();
            Err((
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse { error: "wrong passphrase".to_string() }),
            ))
        }
        Ok(Ok(Err(VaultUnlockError::NoVault))) => Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse { error: "profile has no vault".to_string() }),
        )),
        Ok(Ok(Err(VaultUnlockError::NotReady))) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse { error: "no backend attached".to_string() }),
        )),
        Ok(Err(_recv_err)) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: "render thread dropped the response channel".to_string(),
            }),
        )),
        Err(_timeout) => Err((
            StatusCode::GATEWAY_TIMEOUT,
            Json(ErrorResponse {
                error: "vault unlock timed out after 10 seconds".to_string(),
            }),
        )),
    }
}

// ---------------------------------------------------------------------------
// GET /api/v1/vault/status
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct VaultStatusResponse {
    unlocked: bool,
    has_vault: bool,
}

/// `GET /api/v1/vault/status` — a direct read off the attached
/// `BackendCore`, no command round trip: neither field is a secret, and
/// `core.user().vault_key()`/`has_vault_file()` are already cheap
/// synchronous reads (see `mlc-backend::user::facade::User`'s methods).
async fn vault_status(State(state): State<Arc<AgentState>>) -> Json<VaultStatusResponse> {
    let (unlocked, has_vault) = match state.core() {
        Some(core) => (core.user().vault_key().is_some(), core.user().has_vault_file()),
        None => (false, false),
    };
    Json(VaultStatusResponse { unlocked, has_vault })
}

// ---------------------------------------------------------------------------
// Route builder
// ---------------------------------------------------------------------------

/// Build the vault routes sub-router. Deliberately never wrapped in
/// [`crate::auth::tier_gate`] or merged behind
/// [`crate::auth::auth_middleware`] — see the module doc.
pub fn routes() -> Router<Arc<AgentState>> {
    Router::new()
        .route("/api/v1/vault/unlock", post(unlock_vault))
        .route("/api/v1/vault/status", get(vault_status))
}
