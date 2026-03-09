//! API key management endpoints.
//!
//! All endpoints require `admin` permission.  When the key registry is empty
//! (auth disabled) all requests pass through, enabling the first key to be
//! created via a plain unauthenticated POST.
//!
//! Routes:
//! - `GET    /api/v1/keys`         — list all keys (label, tier, created_at, agent_id)
//! - `POST   /api/v1/keys`         — create a new key; raw key returned once
//! - `DELETE /api/v1/keys/:label`  — revoke a key by label

use std::sync::Arc;
use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::auth::check_permission;
use crate::state::{hash_key, AgentState, ApiKeyEntry, Permissions};

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

/// Public view of an API key — never exposes the hash.
#[derive(Serialize)]
struct KeyInfo {
    label: String,
    tier: String,
    created_at: u64,
    agent_id: Option<String>,
}

#[derive(Serialize)]
struct KeyListResponse {
    keys: Vec<KeyInfo>,
}

#[derive(Deserialize)]
struct CreateKeyRequest {
    label: String,
    /// `"read_only"`, `"read_write"`, or `"admin"`.
    tier: String,
    agent_id: Option<String>,
}

#[derive(Serialize)]
struct CreateKeyResponse {
    /// Raw key — returned ONCE, never stored.
    key: String,
    label: String,
    tier: String,
}

#[derive(Serialize)]
struct DeleteKeyResponse {
    deleted: bool,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

// ---------------------------------------------------------------------------
// Admin permission guard
// ---------------------------------------------------------------------------

/// Returns `Err(403)` when the key registry is non-empty and the caller does
/// not have admin rights.  When the registry is empty (auth disabled) this
/// always passes so the very first key can be created.
fn require_admin(
    state: &AgentState,
    perms: Option<&Permissions>,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    let keys_empty = state
        .keys
        .read()
        .map(|g| g.is_empty())
        .unwrap_or(false);

    if keys_empty {
        // Auth disabled — bootstrap mode, anyone may create first key.
        return Ok(());
    }

    check_permission(perms, "admin")
}

// ---------------------------------------------------------------------------
// GET /api/v1/keys
// ---------------------------------------------------------------------------

async fn list_keys(
    State(state): State<Arc<AgentState>>,
    perms: Option<Extension<Permissions>>,
) -> Result<Json<KeyListResponse>, (StatusCode, Json<serde_json::Value>)> {
    require_admin(&state, perms.as_ref().map(|Extension(p)| p))?;

    let entries = state.list_keys();
    let keys = entries
        .into_iter()
        .map(|e| KeyInfo {
            label: e.label,
            tier: e.tier,
            created_at: e.created_at,
            agent_id: e.agent_id,
        })
        .collect();

    Ok(Json(KeyListResponse { keys }))
}

// ---------------------------------------------------------------------------
// POST /api/v1/keys
// ---------------------------------------------------------------------------

async fn create_key(
    State(state): State<Arc<AgentState>>,
    perms: Option<Extension<Permissions>>,
    Json(body): Json<CreateKeyRequest>,
) -> Result<(StatusCode, Json<CreateKeyResponse>), (StatusCode, Json<serde_json::Value>)> {
    let caller_perms = perms.as_ref().map(|Extension(p)| p);
    require_admin(&state, caller_perms)?;

    // Validate tier.
    let valid_tiers = ["read_only", "read_write", "admin"];
    if !valid_tiers.contains(&body.tier.as_str()) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "invalid tier",
                "valid_tiers": valid_tiers
            })),
        ));
    }

    // Tier escalation restriction.
    //
    // Bootstrap mode (empty key registry): allow creating the first `admin`
    // key — this is the initial setup path triggered from the terminal UI.
    // Once at least one key exists, only the terminal UI may create admin
    // keys; any API caller (even an existing admin) is blocked from doing so
    // to prevent privilege escalation via the API surface.
    let keys_empty = state
        .keys
        .read()
        .map(|g| g.is_empty())
        .unwrap_or(false);

    if !keys_empty {
        // We have keys — apply the escalation policy.
        let is_admin = caller_perms.map(|p| p.admin).unwrap_or(false);
        let is_read_write = caller_perms
            .map(|p| p.write_primitives && !p.admin)
            .unwrap_or(false);

        let allowed = match body.tier.as_str() {
            "admin" => {
                // Nobody may create an admin key via the API after bootstrap.
                false
            }
            "read_write" => {
                // Only admins may create read_write keys.
                is_admin
            }
            "read_only" => {
                // Admins and read_write callers may create read_only keys.
                is_admin || is_read_write
            }
            _ => false,
        };

        if !allowed {
            return Err((
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({
                    "error": "cannot create keys with equal or higher tier"
                })),
            ));
        }
    }

    // Generate 32 random bytes, hex-encode them → 64-char raw key.
    let raw_bytes: [u8; 32] = rand::random();
    let raw_key: String = raw_bytes.iter().map(|b| format!("{:02x}", b)).collect();

    let created_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let entry = ApiKeyEntry {
        key_hash: hash_key(&raw_key),
        label: body.label.clone(),
        tier: body.tier.clone(),
        permissions: Permissions::from_tier(&body.tier),
        created_at,
        agent_id: body.agent_id,
    };

    state.add_key(entry);

    Ok((
        StatusCode::CREATED,
        Json(CreateKeyResponse {
            key: raw_key,
            label: body.label,
            tier: body.tier,
        }),
    ))
}

// ---------------------------------------------------------------------------
// DELETE /api/v1/keys/:label
// ---------------------------------------------------------------------------

async fn revoke_key(
    State(state): State<Arc<AgentState>>,
    perms: Option<Extension<Permissions>>,
    Path(label): Path<String>,
) -> Result<Json<DeleteKeyResponse>, (StatusCode, Json<serde_json::Value>)> {
    require_admin(&state, perms.as_ref().map(|Extension(p)| p))?;

    let deleted = state.remove_key(&label);
    Ok(Json(DeleteKeyResponse { deleted }))
}

// ---------------------------------------------------------------------------
// Route builder
// ---------------------------------------------------------------------------

/// Build the keys management sub-router.
pub fn routes() -> Router<Arc<AgentState>> {
    Router::new()
        .route("/api/v1/keys", get(list_keys).post(create_key))
        .route("/api/v1/keys/:label", delete(revoke_key))
}
