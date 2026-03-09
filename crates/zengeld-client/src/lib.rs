//! HTTP client library for communicating with the zengeld server API.
//!
//! Provides both a blocking [`ZengeldClient`] for direct use and a
//! thread-based [`BackgroundClient`] for fire-and-forget calls from a UI
//! thread. No async runtime is required.
//!
//! # Example
//!
//! ```ignore
//! use zengeld_client::{BackgroundClient, ClientEvent, HeartbeatRequest};
//!
//! let bg = BackgroundClient::spawn("https://chart.example");
//!
//! bg.heartbeat(HeartbeatRequest {
//!     device_id: "abc123".to_string(),
//!     app_version: "0.1.0".to_string(),
//!     uptime_seconds: 42,
//!     os: "windows".to_string(),
//!     device_name: "my-pc".to_string(),
//! });
//!
//! // Later, poll for responses on the UI tick:
//! if let Some(event) = bg.try_recv() {
//!     match event {
//!         ClientEvent::HeartbeatResult(Ok(resp)) => {
//!             println!("server says: {}", resp.status);
//!         }
//!         _ => {}
//!     }
//! }
//! ```

pub mod client;
pub mod types;

pub use client::{
    BackgroundClient,
    ClientCommand,
    ClientError,
    ClientEvent,
    ZengeldClient,
};
pub use types::*;

/// Default server URL.
pub const DEFAULT_SERVER_URL: &str = "https://chart.example";
