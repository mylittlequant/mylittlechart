use std::sync::{Arc, RwLock};
use crate::BarSeries;

/// Window-side handle: holds the shared series and the last version this
/// window rendered against. Used for cheap change detection.
pub struct TrackedSeriesHandle {
    pub series: Arc<RwLock<BarSeries>>,
    /// Version number at the last render that consumed this series.
    pub last_seen_version: u64,
}

impl TrackedSeriesHandle {
    pub fn new(series: Arc<RwLock<BarSeries>>) -> Self {
        Self { series, last_seen_version: 0 }
    }

    /// Returns `true` if the series was mutated since `last_seen_version`.
    pub fn is_stale(&self) -> bool {
        self.series
            .read()
            .map(|s| s.version > self.last_seen_version)
            .unwrap_or(false)
    }

    /// Mark as consumed — call after rendering / recalculating.
    pub fn mark_seen(&mut self) {
        if let Ok(s) = self.series.read() {
            self.last_seen_version = s.version;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::series::DEFAULT_CAPACITY;

    fn make_series(period_secs: i64) -> Arc<RwLock<BarSeries>> {
        Arc::new(RwLock::new(BarSeries::new(DEFAULT_CAPACITY, period_secs)))
    }

    #[test]
    fn test_tracked_handle_is_stale() {
        let arc = make_series(60);
        let mut handle = TrackedSeriesHandle::new(arc.clone());

        // Initially not stale (version 0, last_seen 0).
        assert!(!handle.is_stale());

        // Bump version as BarService would.
        arc.write().unwrap().version += 1;
        assert!(handle.is_stale());

        handle.mark_seen();
        assert!(!handle.is_stale());
    }

    #[test]
    fn test_two_handles_share_series() {
        let arc = make_series(60);
        let mut h1 = TrackedSeriesHandle::new(arc.clone());
        let mut h2 = TrackedSeriesHandle::new(arc.clone());

        arc.write().unwrap().version += 1;

        assert!(h1.is_stale());
        assert!(h2.is_stale());

        h1.mark_seen();
        assert!(!h1.is_stale());
        // h2 still stale — independent tracking.
        assert!(h2.is_stale());

        h2.mark_seen();
        assert!(!h2.is_stale());
    }
}
