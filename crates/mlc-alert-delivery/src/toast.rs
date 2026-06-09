//! Toast notification types.
//!
//! The actual rendering is done by the chart UI layer.
//! This module defines the data structures and helper methods only.

pub use crate::ToastNotification;

impl ToastNotification {
    /// Returns `true` if this toast has expired based on the current time.
    pub fn is_expired(&self, now_millis: u64) -> bool {
        now_millis > self.timestamp + self.duration_ms
    }

    /// Remaining display fraction: `1.0` = just appeared, `0.0` = fully expired.
    pub fn remaining_fraction(&self, now_millis: u64) -> f64 {
        if now_millis >= self.timestamp + self.duration_ms {
            return 0.0;
        }
        if now_millis <= self.timestamp {
            return 1.0;
        }
        let elapsed = now_millis - self.timestamp;
        1.0 - (elapsed as f64 / self.duration_ms as f64)
    }
}
