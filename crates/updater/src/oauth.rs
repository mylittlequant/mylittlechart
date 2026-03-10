//! OAuth authentication via Telegram, GitHub, or Discord.
//!
//! Flow: open browser → user authenticates → callback to localhost → exchange code → store token.

use crate::token_store::StoredToken;
use crate::UPDATE_SERVER;
use std::time::{SystemTime, UNIX_EPOCH};

/// Port range for the OAuth callback server.
const CALLBACK_PORT_START: u16 = 17421;
const CALLBACK_PORT_END: u16 = 17424;

/// Start an OAuth flow for the given provider ("github", "discord", "telegram").
/// Opens the browser, waits for the callback, exchanges the code, returns a token.
pub async fn start_oauth_flow(provider: &str) -> Result<StoredToken, String> {
    // Find an available port for the callback
    let (listener, port) = bind_callback_listener().await?;

    // Build the authorization URL
    let auth_url = format!(
        "{}/api/oauth/{}/authorize?redirect_port={}",
        UPDATE_SERVER, provider, port
    );

    // Open browser
    open_url(&auth_url)?;

    log::info!("Waiting for OAuth callback on port {}...", port);

    // Wait for the callback (with timeout)
    let (code, state) = wait_for_callback(listener).await?;

    // Exchange code for token
    let token = exchange_code(provider, &code, port, state).await?;

    Ok(token)
}

/// Bind a TCP listener on one of the callback ports.
async fn bind_callback_listener() -> Result<(tokio::net::TcpListener, u16), String> {
    for port in CALLBACK_PORT_START..=CALLBACK_PORT_END {
        match tokio::net::TcpListener::bind(format!("127.0.0.1:{}", port)).await {
            Ok(listener) => return Ok((listener, port)),
            Err(_) => continue,
        }
    }
    Err("All OAuth callback ports are busy".to_string())
}

/// Open a URL in the default browser.
fn open_url(url: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(&["/c", "start", "", url])
            .spawn()
            .map_err(|e| format!("Failed to open browser: {}", e))?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map_err(|e| format!("Failed to open browser: {}", e))?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map_err(|e| format!("Failed to open browser: {}", e))?;
    }
    Ok(())
}

/// Wait for the OAuth callback GET request. Extracts `code` and `state` query parameters.
/// Times out after 120 seconds.
async fn wait_for_callback(listener: tokio::net::TcpListener) -> Result<(String, Option<String>), String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let accept = tokio::time::timeout(
        std::time::Duration::from_secs(120),
        listener.accept(),
    ).await
        .map_err(|_| "OAuth callback timed out (120s)".to_string())?
        .map_err(|e| format!("Accept error: {}", e))?;

    let (mut stream, _addr) = accept;

    // Read the HTTP request
    let mut buf = vec![0u8; 4096];
    let n = stream.read(&mut buf).await.map_err(|e| format!("Read error: {}", e))?;
    let request = String::from_utf8_lossy(&buf[..n]);

    // Parse query params from GET /callback?code=XXX&state=YYY HTTP/1.1
    let (code, state) = request.lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .map(|path| {
            let query = path.split('?').nth(1).unwrap_or("");
            let code = query.split('&')
                .find(|p| p.starts_with("code="))
                .map(|p| p[5..].to_string());
            let state = query.split('&')
                .find(|p| p.starts_with("state="))
                .map(|p| p[6..].to_string());
            (code, state)
        })
        .unwrap_or((None, None));
    let code = code.ok_or_else(|| "No 'code' parameter in callback".to_string())?;

    // Send a response to the browser
    let response = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n\
        <html><body><h2>Authentication successful!</h2><p>You can close this tab.</p>\
        <script>window.close()</script></body></html>";
    let _ = stream.write_all(response.as_bytes()).await;

    Ok((code, state))
}

/// Exchange an OAuth code for a stored token.
async fn exchange_code(provider: &str, code: &str, redirect_port: u16, state: Option<String>) -> Result<StoredToken, String> {
    let url = format!("{}/api/oauth/{}/callback", UPDATE_SERVER, provider);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    #[derive(serde::Serialize)]
    struct ExchangeRequest {
        code: String,
        redirect_port: u16,
        #[serde(skip_serializing_if = "Option::is_none")]
        state: Option<String>,
    }

    #[derive(serde::Deserialize)]
    struct ExchangeResponse {
        token: String,
        display_name: String,
    }

    let resp = client.post(&url)
        .json(&ExchangeRequest {
            code: code.to_string(),
            redirect_port,
            state,
        })
        .send()
        .await
        .map_err(|e| format!("Token exchange failed: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Token exchange returned {}: {}", status, body));
    }

    let exchange: ExchangeResponse = resp.json().await.map_err(|e| format!("Parse error: {}", e))?;

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    Ok(StoredToken {
        token: exchange.token,
        provider: provider.to_string(),
        display_name: exchange.display_name,
        saved_at: now,
    })
}
