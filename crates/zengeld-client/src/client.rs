use crate::types::*;
use std::sync::mpsc;
use std::time::Duration;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// Blocking HTTP client for the zengeld API.
///
/// `Send + Sync` — safe to share across threads.
pub struct ZengeldClient {
    base_url: String,
    http: reqwest::blocking::Client,
}

impl ZengeldClient {
    /// Create a new client pointed at `base_url`.
    ///
    /// Trailing slashes on `base_url` are stripped automatically.
    pub fn new(base_url: &str) -> Self {
        let http = reqwest::blocking::Client::builder()
            .timeout(DEFAULT_TIMEOUT)
            .build()
            .expect("failed to build HTTP client");
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            http,
        }
    }

    /// Send a heartbeat to the server.
    pub fn heartbeat(&self, req: &HeartbeatRequest) -> Result<HeartbeatResponse, ClientError> {
        let url = format!("{}/api/heartbeat", self.base_url);
        let resp = self.http.post(&url).json(req).send()?;
        let body = resp.json::<HeartbeatResponse>()?;
        Ok(body)
    }

    /// Check for updates.
    pub fn check_updates(
        &self,
        current_version: &str,
        os: &str,
    ) -> Result<UpdateCheckResponse, ClientError> {
        let url = format!(
            "{}/api/updates?current_version={}&os={}",
            self.base_url, current_version, os
        );
        let resp = self.http.get(&url).send()?;
        let body = resp.json::<UpdateCheckResponse>()?;
        Ok(body)
    }

    /// Submit telemetry data.
    pub fn submit_telemetry(
        &self,
        req: &TelemetryRequest,
    ) -> Result<TelemetryResponse, ClientError> {
        let url = format!("{}/api/telemetry", self.base_url);
        let resp = self.http.post(&url).json(req).send()?;
        let body = resp.json::<TelemetryResponse>()?;
        Ok(body)
    }
}

// ============================================================================
// Error type
// ============================================================================

/// Errors produced by [`ZengeldClient`].
#[derive(Debug)]
pub enum ClientError {
    Http(reqwest::Error),
    Json(serde_json::Error),
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientError::Http(e) => write!(f, "HTTP error: {}", e),
            ClientError::Json(e) => write!(f, "JSON error: {}", e),
        }
    }
}

impl std::error::Error for ClientError {}

impl From<reqwest::Error> for ClientError {
    fn from(e: reqwest::Error) -> Self {
        ClientError::Http(e)
    }
}

impl From<serde_json::Error> for ClientError {
    fn from(e: serde_json::Error) -> Self {
        ClientError::Json(e)
    }
}

// ============================================================================
// Background client — fire-and-forget from UI thread
// ============================================================================

/// Commands that can be sent to the background worker.
pub enum ClientCommand {
    Heartbeat(HeartbeatRequest),
    SubmitTelemetry(TelemetryRequest),
    CheckUpdates {
        current_version: String,
        os: String,
    },
    Shutdown,
}

/// Responses that come back from the background worker.
#[derive(Debug)]
pub enum ClientEvent {
    HeartbeatResult(Result<HeartbeatResponse, String>),
    TelemetryResult(Result<TelemetryResponse, String>),
    UpdateCheckResult(Result<UpdateCheckResponse, String>),
}

/// A background worker that processes API calls on a dedicated thread.
///
/// The UI thread sends [`ClientCommand`]s via [`BackgroundClient::send`] and
/// can poll for [`ClientEvent`]s via [`BackgroundClient::try_recv`].
///
/// The worker thread shuts down automatically when [`BackgroundClient`] is
/// dropped, or when [`BackgroundClient::shutdown`] is called explicitly.
pub struct BackgroundClient {
    cmd_tx: mpsc::Sender<ClientCommand>,
    event_rx: mpsc::Receiver<ClientEvent>,
    _thread: Option<std::thread::JoinHandle<()>>,
}

impl BackgroundClient {
    /// Spawn a background thread that processes API calls.
    ///
    /// `base_url` is the server base URL, e.g. `"https://chart.example"`.
    pub fn spawn(base_url: &str) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel::<ClientCommand>();
        let (event_tx, event_rx) = mpsc::channel::<ClientEvent>();
        let url = base_url.to_string();

        let thread = std::thread::Builder::new()
            .name("zengeld-client".to_string())
            .spawn(move || {
                let client = ZengeldClient::new(&url);
                Self::worker_loop(client, cmd_rx, event_tx);
            })
            .expect("failed to spawn zengeld-client thread");

        Self {
            cmd_tx,
            event_rx,
            _thread: Some(thread),
        }
    }

    fn worker_loop(
        client: ZengeldClient,
        cmd_rx: mpsc::Receiver<ClientCommand>,
        event_tx: mpsc::Sender<ClientEvent>,
    ) {
        loop {
            match cmd_rx.recv() {
                Ok(ClientCommand::Heartbeat(req)) => {
                    let result = client.heartbeat(&req).map_err(|e| e.to_string());
                    let _ = event_tx.send(ClientEvent::HeartbeatResult(result));
                }
                Ok(ClientCommand::SubmitTelemetry(req)) => {
                    let result = client.submit_telemetry(&req).map_err(|e| e.to_string());
                    let _ = event_tx.send(ClientEvent::TelemetryResult(result));
                }
                Ok(ClientCommand::CheckUpdates { current_version, os }) => {
                    let result = client
                        .check_updates(&current_version, &os)
                        .map_err(|e| e.to_string());
                    let _ = event_tx.send(ClientEvent::UpdateCheckResult(result));
                }
                Ok(ClientCommand::Shutdown) | Err(_) => {
                    eprintln!("[zengeld-client] background worker shutting down");
                    break;
                }
            }
        }
    }

    /// Send a command to the background worker (non-blocking).
    pub fn send(&self, cmd: ClientCommand) {
        let _ = self.cmd_tx.send(cmd);
    }

    /// Try to receive a response event (non-blocking).
    ///
    /// Returns `None` if no events are ready.
    pub fn try_recv(&self) -> Option<ClientEvent> {
        self.event_rx.try_recv().ok()
    }

    /// Send a heartbeat (convenience method).
    pub fn heartbeat(&self, req: HeartbeatRequest) {
        self.send(ClientCommand::Heartbeat(req));
    }

    /// Submit telemetry (convenience method).
    pub fn submit_telemetry(&self, req: TelemetryRequest) {
        self.send(ClientCommand::SubmitTelemetry(req));
    }

    /// Check for updates (convenience method).
    pub fn check_updates(&self, current_version: &str, os: &str) {
        self.send(ClientCommand::CheckUpdates {
            current_version: current_version.to_string(),
            os: os.to_string(),
        });
    }

    /// Shut down the background worker gracefully.
    pub fn shutdown(&self) {
        self.send(ClientCommand::Shutdown);
    }
}

impl Drop for BackgroundClient {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(ClientCommand::Shutdown);
        if let Some(thread) = self._thread.take() {
            let _ = thread.join();
        }
    }
}
