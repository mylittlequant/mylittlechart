//! Watchlist read endpoints.
//!
//! - `GET /v1/watchlists` — list all watchlists with their symbol items
//!
//! Response shape mirrors `mlc-backend-agent-server`'s `/api/v1/watchlists` so
//! consumers can parse one shape from both binaries.

use axum::{extract::State, response::IntoResponse, routing::get, Json, Router};
use serde::Serialize;

use crate::AppState;

#[derive(Serialize)]
struct WatchlistItemEntry {
    symbol: String,
    exchange: String,
    account_type: String,
}

#[derive(Serialize)]
struct WatchlistEntry {
    id: u64,
    name: String,
    active: bool,
    items: Vec<WatchlistItemEntry>,
}

#[derive(Serialize)]
struct WatchlistsOut {
    watchlists: Vec<WatchlistEntry>,
}

async fn list_watchlists(State(s): State<AppState>) -> impl IntoResponse {
    let guard = s.core.watchlists();
    let active_id = guard.active_id();
    let watchlists: Vec<WatchlistEntry> = guard
        .lists()
        .iter()
        .map(|wl| {
            let mut items: Vec<WatchlistItemEntry> = Vec::new();
            for g in &wl.groups {
                for sym in &g.symbols {
                    items.push(WatchlistItemEntry {
                        symbol: sym.symbol.clone(),
                        exchange: sym.exchange.clone(),
                        account_type: sym.account_type.as_label().to_string(),
                    });
                }
            }
            for sym in &wl.ungrouped {
                items.push(WatchlistItemEntry {
                    symbol: sym.symbol.clone(),
                    exchange: sym.exchange.clone(),
                    account_type: sym.account_type.as_label().to_string(),
                });
            }
            WatchlistEntry {
                id: wl.id,
                name: wl.name.clone(),
                active: wl.id == active_id,
                items,
            }
        })
        .collect();
    Json(WatchlistsOut { watchlists })
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/v1/watchlists", get(list_watchlists))
}
