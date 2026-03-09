use serde::{Deserialize, Serialize};

// ============================================================================
// Heartbeat
// ============================================================================

#[derive(Debug, Clone, Serialize)]
pub struct HeartbeatRequest {
    pub device_id: String,
    pub app_version: String,
    pub uptime_seconds: u64,
    pub os: String,
    pub device_name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HeartbeatResponse {
    pub status: String,
    pub update_available: bool,
    pub latest_version: String,
    pub message: Option<String>,
}

// ============================================================================
// Updates
// ============================================================================

#[derive(Debug, Clone, Deserialize)]
pub struct UpdateCheckResponse {
    pub update_available: bool,
    pub latest_version: String,
    pub download_url: Option<String>,
    pub release_notes: Option<String>,
}

// ============================================================================
// Telemetry
// ============================================================================

#[derive(Debug, Clone, Serialize)]
pub struct TelemetryRequest {
    pub device_id: String,
    pub app_version: String,
    pub session_duration_seconds: u64,
    pub charts_opened: u64,
    pub indicators_added: u64,
    pub drawings_created: u64,
    pub presets_saved: u64,
    pub templates_saved: u64,
    pub total_interactions: u64,
    pub symbols_viewed: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TelemetryResponse {
    pub status: String,
}
