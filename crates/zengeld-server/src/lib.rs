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
use socket2::{Domain, Protocol, Socket, Type};

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

        // Prefer SO_REUSEADDR so a zombie socket left by process::exit(0) on
        // Windows does not block the new process from binding the same port.
        // Retry up to 3 times (15s total) then give up — avoids zombie loops.
        let listener = {
            let mut attempts = 0u32;
            loop {
                match bind_with_reuse(addr) {
                    Ok(std_listener) => {
                        std_listener
                            .set_nonblocking(true)
                            .expect("set_nonblocking");
                        break tokio::net::TcpListener::from_std(std_listener)
                            .expect("tokio TcpListener from_std");
                    }
                    Err(e) => {
                        attempts += 1;
                        if attempts >= 3 {
                            eprintln!(
                                "[zengeld-server] port {} unavailable after {} attempts — giving up: {}",
                                port, attempts, e
                            );
                            return;
                        }
                        eprintln!(
                            "[zengeld-server] port {} unavailable ({}), retry {}/3 in 5s",
                            port, e, attempts
                        );
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    }
                }
            }
        };

        eprintln!("[zengeld-server] Agent API listening on http://{addr}");
        if let Err(e) = axum::serve(listener, app).await {
            eprintln!("[zengeld-server] server error: {}", e);
        }
    })
}

/// Bind a TCP listener with `SO_REUSEADDR` set.
///
/// On Windows, `process::exit(0)` leaves a zombie TCP socket whose PID is
/// dead but whose kernel entry still shows `LISTENING`. A plain `bind()` will
/// fail with `EADDRINUSE` until the kernel cleans it up (can take tens of
/// seconds). Setting `SO_REUSEADDR` before `bind()` lets the new process claim
/// the port immediately.
fn bind_with_reuse(addr: std::net::SocketAddr) -> std::io::Result<std::net::TcpListener> {
    let socket = Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP))?;
    socket.set_reuse_address(true)?;
    socket.bind(&addr.into())?;
    socket.listen(128)?;
    Ok(socket.into())
}
