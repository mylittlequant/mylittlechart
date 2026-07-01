use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tracing::Subscriber;
use tracing_subscriber::layer::Context;
use tracing_subscriber::Layer;

/// Ring buffer that captures recent log entries for crash reports.
#[derive(Clone)]
pub struct LogBuffer {
    inner: Arc<Mutex<VecDeque<String>>>,
    max_size: usize,
}

impl LogBuffer {
    pub fn new(max_size: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(VecDeque::with_capacity(max_size))),
            max_size,
        }
    }

    pub fn push(&self, line: String) {
        if let Ok(mut buf) = self.inner.lock() {
            if buf.len() >= self.max_size {
                buf.pop_front();
            }
            buf.push_back(line);
        }
    }

    /// Get last N log lines (for crash reports).
    pub fn last_n(&self, n: usize) -> Vec<String> {
        if let Ok(buf) = self.inner.lock() {
            let collected: Vec<String> = buf.iter().rev().take(n).cloned().collect();
            collected.into_iter().rev().collect()
        } else {
            Vec::new()
        }
    }
}

/// Implement tracing Layer to capture WARN+ events into the ring buffer.
impl<S: Subscriber> Layer<S> for LogBuffer {
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        // Only buffer WARN and ERROR
        if *event.metadata().level() > tracing::Level::WARN {
            return;
        }

        // Extract message using a visitor
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);

        let line = format!(
            "{} [{}] {}: {}",
            chrono::Local::now().format("%H:%M:%S%.3f"),
            event.metadata().level(),
            event.metadata().target(),
            visitor.message.unwrap_or_default(),
        );
        self.push(line);
    }
}

#[derive(Default)]
struct MessageVisitor {
    message: Option<String>,
}

impl tracing::field::Visit for MessageVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = Some(format!("{value:?}"));
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_string());
        }
    }
}

/// Crash report structure for server upload.
#[derive(serde::Serialize)]
pub struct CrashReport {
    pub report_id: String,
    pub app_version: String,
    pub crash_type: String,
    pub message: String,
    pub location: Option<String>,
    pub backtrace: Option<String>,
    pub backtrace_hash: String,
    pub os_version: String,
    pub gpu_name: String,
    pub uptime_secs: u64,
    pub ram_used_mb: u64,
    pub system_ram_available_mb: u64,
    pub recent_logs: Vec<String>,
}

/// Compute a dedup hash from backtrace frames.
pub fn backtrace_hash(backtrace_text: &str) -> String {
    use sha2::{Digest, Sha256};
    let frames: Vec<&str> = backtrace_text
        .lines()
        .filter(|l| l.contains("::") && !l.contains("std::panicking") && !l.contains("core::"))
        .take(8)
        .collect();

    let mut hasher = Sha256::new();
    for frame in &frames {
        hasher.update(frame.trim().as_bytes());
        hasher.update(b"\n");
    }
    let result = hasher.finalize();
    format!("{:x}", result).chars().take(16).collect()
}

/// Upload crash report to server (stub — endpoint not yet implemented).
pub async fn upload_crash_report(
    _client: &reqwest::Client,
    _endpoint: &str,
    _report: &CrashReport,
) -> Result<(), Box<dyn std::error::Error>> {
    // Rate limit: max 3 uploads per hour, min 30s between uploads
    // Compress with gzip before sending
    // POST /api/crash-report
    tracing::debug!("crash report upload: stub (server endpoint not implemented)");
    Ok(())
}
