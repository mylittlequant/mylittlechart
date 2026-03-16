//! API key authentication middleware and permission helpers.
//!
//! Checks for a Bearer token in the `Authorization` header or an `api_key`
//! query parameter.  When the key registry ([`AgentState::keys`]) is empty,
//! authentication is skipped (open access for local dev use).
//!
//! On successful authentication the middleware injects two axum extensions
//! into the request so handlers can access them:
//! - [`Permissions`] — what the authenticated key is allowed to do
//! - [`AgentId`] — optional agent identifier attached to the key

use axum::{
    extract::{Extension, Request},
    http::StatusCode,
    middleware::Next,
    response::Response,
    Json,
};
use std::sync::Arc;

use crate::state::{AgentState, Permissions};

// ===========================================================================
// Extension types injected into requests by the middleware
// ===========================================================================

/// Wrapper for the optional agent identifier carried by an API key.
#[derive(Clone, Debug)]
pub struct AgentId(pub Option<String>);

// ===========================================================================
// Auth middleware
// ===========================================================================

/// Core authentication middleware invoked from the closure in `lib.rs`.
///
/// Accepts the key via:
/// - `Authorization: Bearer <key>` header
/// - `?api_key=<key>` query parameter (fallback)
///
/// When the key registry is empty, all requests are allowed (auth disabled).
/// On a successful match the middleware injects [`Permissions`] and [`AgentId`]
/// as request extensions; handlers can extract them with `Extension<T>`.
pub async fn check_api_key(
    state: Arc<AgentState>,
    mut request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // Check if auth is enabled (non-empty key registry).
    // Clone the vec out immediately so the lock guard is dropped before any
    // `.await` point — required for the future to be `Send`.
    let keys_empty: bool = {
        let guard = state
            .local_keys
            .read()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        guard.is_empty()
    };

    if keys_empty {
        // Auth disabled — inject open permissions and no agent id.
        request.extensions_mut().insert(Permissions::admin());
        request.extensions_mut().insert(AgentId(None));
        return Ok(next.run(request).await);
    }

    // Try Authorization: Bearer <key> header first.
    let provided = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.to_string());

    // Fallback: ?api_key= query parameter.
    let provided = provided.or_else(|| {
        request.uri().query().and_then(|q| {
            q.split('&').find_map(|pair| {
                let (k, v) = pair.split_once('=')?;
                if k == "api_key" {
                    Some(v.to_string())
                } else {
                    None
                }
            })
        })
    });

    let raw_key = match provided {
        Some(k) => k,
        None => return Err(StatusCode::UNAUTHORIZED),
    };

    match state.resolve_key(&raw_key) {
        Some((permissions, agent_id)) => {
            request.extensions_mut().insert(permissions);
            request.extensions_mut().insert(AgentId(agent_id));
            Ok(next.run(request).await)
        }
        None => Err(StatusCode::UNAUTHORIZED),
    }
}

// ===========================================================================
// Permission check helper
// ===========================================================================

/// Check whether the injected permissions satisfy a named requirement.
///
/// - When `perms` is `None` (auth disabled, no extension injected), all
///   operations are permitted.
/// - Returns `Err((403, JSON body))` when the check fails.
///
/// `required` must be one of:
/// `"write_viewport"`, `"write_indicators"`, `"write_primitives"`, `"admin"`.
pub fn check_permission(
    perms: Option<&Permissions>,
    required: &str,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    let p = match perms {
        Some(p) => p,
        // No permissions injected means auth is disabled — allow everything.
        None => return Ok(()),
    };

    let allowed = match required {
        "write_viewport" => p.write_viewport,
        "write_indicators" => p.write_indicators,
        "write_primitives" => p.write_primitives,
        "admin" => p.admin,
        _ => false,
    };

    if allowed {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "error": "permission denied",
                "required": required
            })),
        ))
    }
}
