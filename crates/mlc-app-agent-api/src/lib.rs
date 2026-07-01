//! Render-agnostic Agent API plumbing.
//!
//! Hosts the snapshot builders and the AgentCommand processor as plain
//! functions over [`AgentHost`] — a trait that gives access to the host's
//! windows (each `(window_id, &mut ChartApp)`) without leaking renderer-side
//! types (winit, vello, screenshot ring buffers).
//!
//! Two consumers:
//! - `mlc-app-shell-vello` (the desktop GPU binary): implements `AgentHost` over
//!   its `App<'a>` struct, gets snapshots + command dispatch for free.
//! - `mlc-app-shell-headless` (the upcoming headless backend binary): implements
//!   `AgentHost` over a thin tick-loop owner of one `ChartApp` per window.
//!
//! Screenshot is the one capability that genuinely needs a renderer. The
//! `request_screenshot` hook returns `Result<(), String>` so headless hosts
//! can short-circuit with `Err("screenshot not supported")` without taking
//! the work into themselves.

pub mod commands;
pub mod host;
pub mod snapshots;

pub use host::AgentHost;
