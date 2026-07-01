pub mod crash;
pub mod logging;
pub mod reporter;
pub mod watchdog;

use std::path::{Path, PathBuf};

/// Handles returned from init — must be kept alive for the lifetime of the app.
pub struct DiagnosticsGuard {
    pub _log_guard: tracing_appender::non_blocking::WorkerGuard,
    pub _root_span: tracing::span::EnteredSpan,
    pub log_buffer: reporter::LogBuffer,
}

/// Initialize all diagnostics systems. Call at the very start of main().
/// Returns a guard that must be held alive.
pub fn init(log_dir: &Path, version: &str) -> DiagnosticsGuard {
    // 1. Logging
    let (log_guard, log_buffer) = logging::init(log_dir);

    // 2. Root span with PID + version
    let root_span = tracing::info_span!(
        "app",
        pid = std::process::id(),
        version = %version,
    )
    .entered();

    tracing::info!("diagnostics initialized");

    // 3. Panic hook
    crash::install_panic_hook(log_dir, log_buffer.clone(), version);

    DiagnosticsGuard {
        _log_guard: log_guard,
        _root_span: root_span,
        log_buffer,
    }
}

/// Default log directory: %APPDATA%/mylittlechart/logs/
pub fn default_log_dir() -> PathBuf {
    let base = std::env::var("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."));
    base.join("mylittlechart").join("logs")
}
