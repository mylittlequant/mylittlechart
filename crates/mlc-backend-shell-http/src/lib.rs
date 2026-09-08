//! `zengeld-server` — internal axum HTTP server providing the Agent API.
//!
//! Runs on `localhost:<port>` inside the terminal process. Trading agents can
//! query bar data, indicator snapshots, window layout, and issue viewport/symbol
//! commands via REST without touching exchange APIs directly.
//!
//! Every route requires a `Bearer` agent key minted from inside the app
//! (`AgentState::agent_key_store`, see `auth.rs`): a sealed key store answers
//! `423`, a missing or unrecognised key answers `401`, and a mutating method
//! (`POST`/`PUT`/`PATCH`/`DELETE`) presented by a `ReadOnly` key answers
//! `403`. The server still only binds `127.0.0.1` — the auth layer bounds
//! WHO on this machine may act, not WHERE the traffic can come from.
//!
//! One exception: `routes::vault`'s `POST /api/v1/vault/unlock` and
//! `GET /api/v1/vault/status` sit OUTSIDE `auth_middleware` entirely — an
//! agent key can only ever be minted from an unlocked vault, so a sealed
//! vault after a restart would have no bearer able to reopen it if that
//! route required one. The passphrase itself is the credential there; see
//! that module's own doc.

pub mod auth;
pub mod routes;
pub mod state;

use std::sync::Arc;
use axum::{Router, extract::State, middleware::Next, response::Response};
use axum::http::Request;
use socket2::{Domain, Protocol, Socket, Type};

use auth::{auth_middleware, tier_gate, Tier};
pub use state::AgentState;

/// Middleware: record the time of every Agent API request.
///
/// The render thread reads `AgentState::accessed_within` to decide whether to
/// build the expensive per-second snapshots. When no agent is talking to the
/// terminal (the common case), the snapshots — which clone every indicator's
/// full output series — are skipped, eliminating a 20-30ms periodic frame stall.
async fn track_access(
    State(state): State<Arc<AgentState>>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    state.bump_access();
    next.run(req).await
}

/// Start the internal Agent API server on the given tokio runtime handle.
///
/// All routes are merged into a single Router, each module gated at
/// [`Tier::SignedIn`] via [`tier_gate`]. [`auth_middleware`] wraps the whole
/// thing as the outermost layer (added last — see the doc on
/// [`axum::middleware::from_fn_with_state`]'s ordering) and `track_access`
/// stays innermost so it only ever sees a request that already passed auth.
/// [`routes::vault`] is merged in AFTER those layers are applied (see this
/// crate's doc for why it must carry neither) — order matters here: a
/// `.layer()` call only wraps the router it is called on, so building
/// `routes::vault`'s router separately and merging its ALREADY-`with_state`d
/// form is what keeps it layer-free rather than accidentally inheriting
/// `auth_middleware` from a shared builder chain.
///
/// The listener is still bound to `127.0.0.1` only, so the server is not
/// reachable from the network.
///
/// Returns a [`tokio::task::JoinHandle`] so the caller can abort or await it
/// if needed.
pub fn start_server(
    state: Arc<AgentState>,
    handle: &tokio::runtime::Handle,
    port: u16,
) -> tokio::task::JoinHandle<()> {
    let agent_key_store = state.agent_key_store_handle();

    // Built and `.with_state()`'d on its own, before either layer below is
    // applied — merging this in further down picks up no auth/tier layer.
    let vault_router: Router = routes::vault::routes().with_state(state.clone());

    let app: Router = Router::new()
        .merge(tier_gate(routes::health::routes(), Tier::SignedIn))
        .merge(tier_gate(routes::apps::routes(), Tier::SignedIn))
        .merge(tier_gate(routes::bars::routes(), Tier::SignedIn))
        .merge(tier_gate(routes::indicators::routes(), Tier::SignedIn))
        .merge(tier_gate(routes::windows::routes(), Tier::SignedIn))
        .merge(tier_gate(routes::viewport::routes(), Tier::SignedIn))
        .merge(tier_gate(routes::controls::routes(), Tier::SignedIn))
        .merge(tier_gate(routes::indicators_crud::routes(), Tier::SignedIn))
        .merge(tier_gate(routes::primitives::routes(), Tier::SignedIn))
        .merge(tier_gate(routes::screenshot::routes(), Tier::SignedIn))
        .merge(tier_gate(routes::catalog::routes(), Tier::SignedIn))
        .merge(tier_gate(routes::watchlists::routes(), Tier::SignedIn))
        .merge(tier_gate(routes::connectors_status::routes(), Tier::SignedIn))
        .merge(tier_gate(routes::ui::routes(), Tier::SignedIn))
        .merge(tier_gate(routes::share::routes(), Tier::SignedIn))
        .merge(tier_gate(routes::alerts::routes(), Tier::SignedIn))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            track_access,
        ))
        .layer(axum::middleware::from_fn_with_state(
            agent_key_store,
            auth_middleware,
        ))
        .with_state(state)
        .merge(vault_router);

    handle.spawn(async move {
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
