//! Screenshot endpoint — Phase 5 of the Agent API.
//!
//! Route:
//! - `POST /api/v1/windows/:window_id/charts/:chart_id/screenshot`
//!
//! The handler pushes [`AgentCommand::RequestScreenshot`] into the render
//! thread's command queue and awaits a [`ScreenshotData`] response via a
//! oneshot channel.  A 5-second timeout is applied; if the render thread
//! does not respond in time, 504 Gateway Timeout is returned.

use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::post,
    Json, Router,
};
use base64::Engine as _;
use serde::{Deserialize, Serialize};

use crate::state::AgentCommand;
use crate::AgentState;

// ---------------------------------------------------------------------------
// Path extractor
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ChartPath {
    window_id: String,
    chart_id: u64,
}

// ---------------------------------------------------------------------------
// Request / response types
// ---------------------------------------------------------------------------

/// Optional request body for the screenshot endpoint.
#[derive(Deserialize, Default)]
struct ScreenshotRequest {
    agent_id: Option<String>,
}

/// Successful screenshot response.
#[derive(Serialize)]
struct ScreenshotResponse {
    window_id: String,
    chart_id: u64,
    width: u32,
    height: u32,
    png_base64: String,
}

/// Error response envelope.
#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// `POST /api/v1/windows/:window_id/charts/:chart_id/screenshot`
///
/// Captures a PNG screenshot of the specified chart and returns it
/// base64-encoded.  The render thread performs the actual GPU readback;
/// this handler waits up to 5 seconds for the result.
async fn take_screenshot(
    State(state): State<Arc<AgentState>>,
    Path(ChartPath { window_id, chart_id }): Path<ChartPath>,
    body: Option<Json<ScreenshotRequest>>,
) -> Result<Json<ScreenshotResponse>, (StatusCode, Json<ErrorResponse>)> {
    let agent_id = body.and_then(|Json(b)| b.agent_id);

    let (tx, rx) = tokio::sync::oneshot::channel::<Result<crate::state::ScreenshotData, String>>();

    state.push_command(AgentCommand::RequestScreenshot {
        window_id: window_id.clone(),
        chart_id,
        agent_id,
        response_tx: tx,
    });

    // Wait up to 5 seconds for the render thread to respond.
    let screenshot_data = match tokio::time::timeout(Duration::from_secs(5), rx).await {
        Ok(Ok(Ok(data))) => data,
        Ok(Ok(Err(render_err))) => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("render thread error: {}", render_err),
                }),
            ));
        }
        Ok(Err(_recv_err)) => {
            // Sender was dropped — render thread panicked or window was closed.
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "render thread dropped the response channel".to_string(),
                }),
            ));
        }
        Err(_timeout) => {
            return Err((
                StatusCode::GATEWAY_TIMEOUT,
                Json(ErrorResponse {
                    error: "screenshot timed out after 5 seconds".to_string(),
                }),
            ));
        }
    };

    let png_base64 = base64::engine::general_purpose::STANDARD.encode(&screenshot_data.png_bytes);

    Ok(Json(ScreenshotResponse {
        window_id,
        chart_id,
        width: screenshot_data.width,
        height: screenshot_data.height,
        png_base64,
    }))
}

// ---------------------------------------------------------------------------
// Route builder
// ---------------------------------------------------------------------------

/// Build the screenshot routes sub-router.
pub fn routes() -> Router<Arc<AgentState>> {
    Router::new().route(
        "/api/v1/windows/:window_id/charts/:chart_id/screenshot",
        post(take_screenshot),
    )
}
