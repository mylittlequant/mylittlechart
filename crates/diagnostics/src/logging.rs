use crate::reporter::LogBuffer;
use std::path::Path;
use tracing_appender::rolling;
use tracing_subscriber::{filter::LevelFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};

/// Initialize tracing with two outputs:
/// - stderr: WARN+ (colored, compact)
/// - file: INFO+ (JSON, daily rotation, keep 7 days)
///
/// Returns the file guard (must keep alive) and a log buffer for crash reports.
pub fn init(log_dir: &Path) -> (tracing_appender::non_blocking::WorkerGuard, LogBuffer) {
    std::fs::create_dir_all(log_dir).ok();

    // File appender: daily rotation, keep 7 days
    let file_appender = rolling::Builder::new()
        .rotation(rolling::Rotation::DAILY)
        .filename_prefix("mylittlechart")
        .filename_suffix("log")
        .max_log_files(7)
        .build(log_dir)
        .expect("log directory must be writable — check APPDATA/zengeld/logs/ permissions");

    let (non_blocking_file, file_guard) = tracing_appender::non_blocking(file_appender);

    // Log buffer for crash reports (last 200 WARN+ entries)
    let log_buffer = LogBuffer::new(200);
    let buffer_layer = log_buffer.clone();

    // Stderr: WARN+ by default, respects RUST_LOG env var
    let stderr_filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::WARN.into())
        .from_env_lossy();

    let stderr_layer = fmt::layer()
        .with_writer(std::io::stderr)
        .with_ansi(true)
        .with_thread_ids(true)
        .with_file(true)
        .with_line_number(true)
        .compact()
        .with_filter(stderr_filter);

    // File: INFO+ JSON
    let file_filter = EnvFilter::new("info");

    let file_layer = fmt::layer()
        .with_writer(non_blocking_file)
        .with_ansi(false)
        .with_thread_ids(true)
        .with_file(true)
        .with_line_number(true)
        .json()
        .with_filter(file_filter);

    tracing_subscriber::registry()
        .with(stderr_layer)
        .with(file_layer)
        .with(buffer_layer)
        .init();

    // Bridge log crate → tracing
    tracing_log::LogTracer::init().ok();

    (file_guard, log_buffer)
}
