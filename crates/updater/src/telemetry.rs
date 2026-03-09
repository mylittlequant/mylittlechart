//! Fixed-schema telemetry metrics collection and sending.

use serde::{Serialize, Deserialize};
use std::path::PathBuf;
use crate::UPDATE_SERVER;

/// Fixed-schema telemetry payload. No arbitrary user strings allowed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryPayload {
    pub device_id: String,
    pub app_version: String,
    pub os: String,
    pub arch: String,
    pub gpu_name: String,
    pub screen_width: u32,
    pub screen_height: u32,
    pub connector_count: u32,
    pub window_count: u32,
    pub avg_fps: f32,
    pub uptime_secs: u64,
    pub total_bars: u64,
    pub ws_connections: u32,
}

/// Path to the persisted device ID file.
fn device_id_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("zengeld").join("device_id"))
}

/// Return the persisted device ID, generating and saving a new one if absent.
///
/// Never panics — if writing to disk fails the new ID is returned in-memory
/// so the session still works; it will just generate a fresh ID next launch.
pub fn get_or_create_device_id() -> String {
    // Try to read an existing ID.
    if let Some(path) = device_id_path() {
        if let Ok(contents) = std::fs::read_to_string(&path) {
            let id = contents.trim().to_string();
            if !id.is_empty() {
                return id;
            }
        }
    }

    // Generate a new random UUID v4.
    let new_id = uuid::Uuid::new_v4().to_string();

    // Attempt to persist it for future launches.
    if let Some(path) = device_id_path() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&path, &new_id);
    }

    new_id
}

/// Sanitize GPU name: only printable ASCII, max 128 chars.
pub fn sanitize_gpu_name(raw: &str) -> String {
    raw.chars()
        .filter(|c| c.is_ascii_graphic() || *c == ' ')
        .take(128)
        .collect()
}

/// Send telemetry to the server (fire-and-forget, errors are logged).
pub async fn send_telemetry(payload: &TelemetryPayload, auth_header: Option<&str>) -> Result<(), String> {
    let url = format!("{}/api/telemetry", UPDATE_SERVER);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    let mut req = client.post(&url).json(payload);
    if let Some(auth) = auth_header {
        req = req.header("Authorization", auth);
    }

    let resp = req.send().await.map_err(|e| format!("Telemetry send failed: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Telemetry server returned {}", resp.status()));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_gpu_name() {
        assert_eq!(sanitize_gpu_name("NVIDIA GeForce RTX 4060 Ti"), "NVIDIA GeForce RTX 4060 Ti");
        assert_eq!(sanitize_gpu_name("GPU\x00with\x01nulls"), "GPUwithnulls");
        let long = "A".repeat(200);
        assert_eq!(sanitize_gpu_name(&long).len(), 128);
    }
}
