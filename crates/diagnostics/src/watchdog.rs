use sysinfo::System;
use tokio::sync::mpsc;

/// Memory warning levels.
#[derive(Debug, Clone)]
pub enum MemoryWarning {
    /// System RAM getting low (< 1 GB available).
    Low { available_mb: u64, total_mb: u64 },
    /// System RAM critical (< 500 MB available).
    Critical { available_mb: u64, total_mb: u64 },
    /// System RAM dangerously low (< 300 MB available).
    Emergency { available_mb: u64, total_mb: u64 },
}

/// Thresholds in MB for system available RAM.
const THRESHOLD_LOW_MB: u64 = 1024;
const THRESHOLD_CRITICAL_MB: u64 = 512;
const THRESHOLD_EMERGENCY_MB: u64 = 300;

/// Interval between checks in seconds.
const POLL_INTERVAL_SECS: u64 = 30;

/// Cooldown: don't repeat same warning level for N polls (10 * 30s = 5 minutes).
const WARNING_COOLDOWN_POLLS: u32 = 10;

/// Start the memory watchdog as a tokio task.
///
/// Returns a receiver for memory warnings. The caller should consume this and
/// react to warnings (show toast, graceful shutdown, etc.). The watchdog logs
/// warnings via tracing regardless of whether the receiver is consumed.
pub fn start() -> mpsc::Receiver<MemoryWarning> {
    let (tx, rx) = mpsc::channel(16);

    tokio::spawn(async move {
        watchdog_loop(tx).await;
    });

    rx
}

async fn watchdog_loop(tx: mpsc::Sender<MemoryWarning>) {
    let mut sys = System::new();
    let mut low_cooldown: u32 = 0;
    let mut critical_cooldown: u32 = 0;
    let mut emergency_cooldown: u32 = 0;

    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(POLL_INTERVAL_SECS));

    loop {
        interval.tick().await;

        sys.refresh_memory();
        let total_mb = sys.total_memory() / 1024 / 1024;
        let available_mb = sys.available_memory() / 1024 / 1024;

        // Decrement cooldowns
        low_cooldown = low_cooldown.saturating_sub(1);
        critical_cooldown = critical_cooldown.saturating_sub(1);
        emergency_cooldown = emergency_cooldown.saturating_sub(1);

        if available_mb < THRESHOLD_EMERGENCY_MB && emergency_cooldown == 0 {
            tracing::error!(available_mb, total_mb, "EMERGENCY: system memory critically low");
            let _ = tx.send(MemoryWarning::Emergency { available_mb, total_mb }).await;
            emergency_cooldown = WARNING_COOLDOWN_POLLS;
        } else if available_mb < THRESHOLD_CRITICAL_MB && critical_cooldown == 0 {
            tracing::warn!(available_mb, total_mb, "system memory critical");
            let _ = tx.send(MemoryWarning::Critical { available_mb, total_mb }).await;
            critical_cooldown = WARNING_COOLDOWN_POLLS;
        } else if available_mb < THRESHOLD_LOW_MB && low_cooldown == 0 {
            tracing::warn!(available_mb, total_mb, "system memory low");
            let _ = tx.send(MemoryWarning::Low { available_mb, total_mb }).await;
            low_cooldown = WARNING_COOLDOWN_POLLS;
        }
    }
}

/// Get current memory status (one-shot, for crash reports).
///
/// Returns `(available_mb, total_mb)`.
pub fn current_memory_status() -> (u64, u64) {
    let mut sys = System::new();
    sys.refresh_memory();
    let total_mb = sys.total_memory() / 1024 / 1024;
    let available_mb = sys.available_memory() / 1024 / 1024;
    (available_mb, total_mb)
}
