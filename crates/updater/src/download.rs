//! Binary download with SHA256 verification and progress reporting.

use sha2::{Sha256, Digest};
use futures_util::StreamExt;

/// Download binary from URL, verify SHA256, report progress via callback.
/// Returns the verified binary data.
pub async fn download_and_verify(
    url: &str,
    expected_sha256: &str,
    on_progress: impl Fn(u8),
) -> Result<Vec<u8>, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(600)) // 10 min for large binaries
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    let resp = client.get(url).send().await.map_err(|e| format!("Download request failed: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Download server returned {}", resp.status()));
    }

    let total_size = resp.content_length().unwrap_or(0);
    let mut data = Vec::with_capacity(total_size as usize);
    let mut downloaded: u64 = 0;
    let mut last_pct: u8 = 0;

    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Download stream error: {}", e))?;
        data.extend_from_slice(&chunk);
        downloaded += chunk.len() as u64;

        if total_size > 0 {
            let pct = ((downloaded * 100) / total_size).min(100) as u8;
            if pct != last_pct {
                last_pct = pct;
                on_progress(pct);
            }
        }
    }

    // Verify SHA256
    on_progress(100);
    let mut hasher = Sha256::new();
    hasher.update(&data);
    let hash = format!("{:x}", hasher.finalize());

    if hash != expected_sha256 {
        return Err(format!(
            "SHA256 mismatch: expected {}, got {}",
            expected_sha256, hash
        ));
    }

    log::info!("Download complete: {} bytes, SHA256 verified", data.len());
    Ok(data)
}
