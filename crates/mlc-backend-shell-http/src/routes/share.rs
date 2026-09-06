//! Native share-viewer Agent API — open a share link, or clone one, in a
//! given window.
//!
//! Plan: `docs/mlc/plans/native-share-viewer-and-chart-rights-2026-09-06.md`
//! §5. "Role" is the effective right set resolved on the object server-side
//! (plan §2), not a separate enum here: `open` always succeeds (every link
//! grants `read`), while `clone` requires `right ⊇ {read, clone}` — enforced
//! by the server's `POST /api/share/{token}/clone`, not by this route. Both
//! push an [`AgentCommand`] and answer immediately, same fire-and-forget
//! shape as `ui.rs`'s `ui/dispatch`: the actual network fetch + preset
//! restore happens on the shell's own drain of `ChartApp::
//! take_pending_open_share`/`take_pending_clone_share`, off this thread
//! entirely.
//!
//! - `POST /api/v1/windows/:window_id/share/open`  `{"token"}`
//! - `POST /api/v1/windows/:window_id/share/clone` `{"token"}`
//!
//! `clone` mutates (creates a new preset), so `auth_middleware` already
//! answers `403` for a `ReadOnly` key on it — no bespoke gate needed here.

use std::sync::Arc;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::post,
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::state::AgentCommand;
use crate::AgentState;

#[derive(Deserialize)]
struct WindowPath {
    window_id: String,
}

#[derive(Deserialize)]
struct ShareTokenRequest {
    token: String,
}

/// Mirrors `ui.rs`'s `UiDispatchResponse` — the same `{"accepted": true}`
/// shape every other fire-and-forget Agent API write answers with.
#[derive(Serialize)]
struct AcceptedResponse {
    accepted: bool,
}

/// `POST /api/v1/windows/:window_id/share/open` — queue
/// [`AgentCommand::OpenShare`]; the shell resolves the token into a
/// read-only tab.
async fn open_share(
    State(state): State<Arc<AgentState>>,
    Path(WindowPath { window_id }): Path<WindowPath>,
    Json(body): Json<ShareTokenRequest>,
) -> (StatusCode, Json<AcceptedResponse>) {
    state.push_command(AgentCommand::OpenShare {
        window_id,
        token: body.token,
    });
    (StatusCode::ACCEPTED, Json(AcceptedResponse { accepted: true }))
}

/// `POST /api/v1/windows/:window_id/share/clone` — queue
/// [`AgentCommand::CloneShare`]; the shell resolves the token into a
/// writable clone. A `ReadOnly` agent key never reaches here —
/// `auth_middleware` refuses this mutating method with `403` first.
async fn clone_share(
    State(state): State<Arc<AgentState>>,
    Path(WindowPath { window_id }): Path<WindowPath>,
    Json(body): Json<ShareTokenRequest>,
) -> (StatusCode, Json<AcceptedResponse>) {
    state.push_command(AgentCommand::CloneShare {
        window_id,
        token: body.token,
    });
    (StatusCode::ACCEPTED, Json(AcceptedResponse { accepted: true }))
}

/// Build the share-viewer routes sub-router.
pub fn routes() -> Router<Arc<AgentState>> {
    Router::new()
        .route("/api/v1/windows/:window_id/share/open", post(open_share))
        .route("/api/v1/windows/:window_id/share/clone", post(clone_share))
}
