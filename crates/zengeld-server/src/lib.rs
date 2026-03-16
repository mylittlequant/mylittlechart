//! `zengeld-server` — internal axum HTTP server providing the Agent API.
//!
//! Runs on `localhost:<port>` inside the terminal process. Trading agents can
//! query bar data, indicator snapshots, window layout, and issue viewport/symbol
//! commands via REST without touching exchange APIs directly.
//!
//! Authentication is controlled by the key registry in [`AgentState::keys`]:
//! when the registry is non-empty a Bearer token (or `?local_agent_key=` query param)
//! is required on all protected routes.  An empty registry disables auth
//! (open access) for local dev/single-user use.
//!
//! Key management (`/api/v1/keys`) requires the `admin` permission tier.  When
//! the registry is empty the endpoint is unrestricted so the first key can be
//! bootstrapped without existing credentials.
//!
//! The `/health` route is always public.

pub mod auth;
pub mod routes;
pub mod state;

use std::sync::Arc;
use axum::{Router, middleware};

pub use state::AgentState;

/// Start the internal Agent API server on the given tokio runtime.
///
/// Routes are split into two groups:
/// - **public** (`/health`) — always accessible, no auth required.
/// - **protected** (`/bars`, `/indicators`) — require the API key when
///   [`AgentState::local_keys`] is non-empty.
///
/// Returns a [`tokio::task::JoinHandle`] so the caller can abort or await it
/// if needed.
pub fn start_server(
    state: Arc<AgentState>,
    runtime: &tokio::runtime::Runtime,
    port: u16,
) -> tokio::task::JoinHandle<()> {
    // Health endpoint — no auth required.
    let public = Router::new().merge(routes::health::routes());

    // Build the auth middleware by capturing state in a closure.
    // This avoids `from_fn_with_state` which requires extra trait bounds.
    let auth_state = state.clone();
    let auth_middleware = middleware::from_fn(move |req, next| {
        let s = auth_state.clone();
        async move { auth::check_api_key(s, req, next).await }
    });

    // Protected endpoints — require API key when one is configured.
    let protected = Router::new()
        .merge(routes::bars::routes())
        .merge(routes::indicators::routes())
        .merge(routes::windows::routes())
        .merge(routes::viewport::routes())
        .merge(routes::indicators_crud::routes())
        .merge(routes::primitives::routes())
        .merge(routes::screenshot::routes())
        .merge(routes::catalog::routes())
        .merge(routes::watchlists::routes())
        .merge(routes::connectors_status::routes())
        .merge(routes::keys::routes())
        .route_layer(auth_middleware);

    let app: Router = Router::new()
        .merge(public)
        .merge(protected)
        .with_state(state);

    runtime.spawn(async move {
        let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));

        // Retry binding — during OTA restart the old process may still hold the port.
        let mut listener = None;
        for attempt in 1..=10 {
            match tokio::net::TcpListener::bind(addr).await {
                Ok(l) => {
                    listener = Some(l);
                    break;
                }
                Err(e) => {
                    eprintln!(
                        "[zengeld-server] port {} busy, retry {}/10: {}",
                        port, attempt, e
                    );
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            }
        }

        let Some(listener) = listener else {
            eprintln!("[zengeld-server] failed to bind port {} after 10 retries — Agent API disabled", port);
            return;
        };

        eprintln!("[zengeld-server] Agent API listening on http://{addr}");
        if let Err(e) = axum::serve(listener, app).await {
            eprintln!("[zengeld-server] server error: {}", e);
        }
    })
}
