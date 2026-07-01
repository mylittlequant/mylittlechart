//! Bridge data request endpoint.
//!
//! - `POST /v1/bridge/request-bars` — subscribe to live bars for a symbol

use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::post,
    Json, Router,
};
use serde::{Deserialize, Serialize};

use mlc_backend_data_bridge::{account_type_from_short_label, ExchangeId};
use mlc_core::state::Timeframe;

use crate::AppState;

fn timeframe_ms(name: &str) -> u64 {
    match name {
        "1m" => 60_000,
        "3m" => 180_000,
        "5m" => 300_000,
        "15m" => 900_000,
        "30m" => 1_800_000,
        "1h" | "1H" => 3_600_000,
        "2h" | "2H" => 7_200_000,
        "4h" | "4H" => 14_400_000,
        "6h" | "6H" => 21_600_000,
        "8h" | "8H" => 28_800_000,
        "12h" | "12H" => 43_200_000,
        "1d" | "1D" => 86_400_000,
        "3d" | "3D" => 259_200_000,
        "1w" | "1W" => 604_800_000,
        "1M" => 2_592_000_000,
        other => {
            if let Ok(n) = other.trim_end_matches(|c: char| !c.is_numeric()).parse::<u64>() {
                n * 1_000
            } else {
                60_000
            }
        }
    }
}

#[derive(Deserialize)]
struct RequestBarsIn {
    exchange: String,
    #[serde(default = "default_account_type")]
    account_type: String,
    symbol: String,
    timeframe: String,
    limit: Option<u32>,
    total_bars: Option<u32>,
    force: Option<bool>,
}

fn default_account_type() -> String {
    "S".to_string()
}

#[derive(Serialize)]
struct RequestBarsOut {
    requested: bool,
}

async fn request_bars(
    State(s): State<AppState>,
    Json(body): Json<RequestBarsIn>,
) -> impl IntoResponse {
    let exchange_id = match ExchangeId::from_str(&body.exchange) {
        Some(e) => e,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": format!("unknown exchange: {}", body.exchange)})),
            )
                .into_response()
        }
    };
    let acct = account_type_from_short_label(&body.account_type);
    let ms = timeframe_ms(&body.timeframe);
    let tf = Timeframe::new(&body.timeframe, ms as i64);
    let limit = body.limit.map(|v| v as u16);
    let total_bars = body.total_bars.map(|v| v as usize);
    s.core
        .bridge
        .request_bars(exchange_id, &body.symbol, &tf, acct, limit, total_bars, body.force.unwrap_or(false));
    Json(RequestBarsOut { requested: true }).into_response()
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/v1/bridge/request-bars", post(request_bars))
}
