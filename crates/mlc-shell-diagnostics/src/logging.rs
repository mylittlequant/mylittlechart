use crate::reporter::LogBuffer;
use std::path::Path;
use tracing_appender::rolling;
use tracing_subscriber::{filter::LevelFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};

/// Env var that lets the exchange connectors log again.
///
/// Set to anything non-empty to restore them at WARN, for a debugging session
/// where their chatter is the subject.
const CONNECTOR_LOG_ENV: &str = "MLC_LOG_CONNECTORS";

/// Silence the exchange connectors' targets unless explicitly asked for.
///
/// Their logs are dig3's, not this application's, and this application has no
/// use for them: one connector parser can emit at frame rate — measured
/// 2026-09-03 at 132 lines/second sustained, 42 214 of one 42 934-line
/// session's lines being a single repeated message. Inheriting a dependency's
/// per-frame chatter is not a logging level to tune, it is a decision not to
/// take someone else's stream.
///
/// A directive rather than a target-list check so `RUST_LOG` still wins: it is
/// appended first, and anything the user names afterwards overrides it.
fn connector_muted(filter: EnvFilter) -> EnvFilter {
    if std::env::var_os(CONNECTOR_LOG_ENV).is_some_and(|v| !v.is_empty()) {
        return filter.add_directive("dig3=warn".parse().expect("static directive must parse"));
    }
    filter.add_directive("dig3=off".parse().expect("static directive must parse"))
}

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

    // Stderr: WARN+ by default, respects RUST_LOG env var, connectors muted.
    let stderr_filter = connector_muted(
        EnvFilter::builder()
            .with_default_directive(LevelFilter::WARN.into())
            .from_env_lossy(),
    );

    let stderr_layer = fmt::layer()
        .with_writer(std::io::stderr)
        .with_ansi(true)
        .with_thread_ids(true)
        .with_file(true)
        .with_line_number(true)
        .compact()
        .with_filter(stderr_filter);

    // File: INFO+ JSON, connectors muted on the same terms as stderr. This
    // sink is the expensive one: it had no target filter at all, so the
    // connector firehose below went to disk at INFO for as long as the app
    // ran, with seven days of retention behind it — 197 MB in a single day,
    // 363 MB standing.
    let file_filter = connector_muted(EnvFilter::new("info"));

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
