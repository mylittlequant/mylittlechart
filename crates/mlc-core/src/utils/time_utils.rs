//! Unix time helpers — used as id suffixes by templates and as save-path
//! components by the preset system.

use web_time::SystemTime;

/// `(unix_seconds, nanos_suffix_mod_1_000_000)`.
pub fn unix_timestamp_parts() -> (u64, u32) {
    let now = SystemTime::now();
    let duration = now
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs();
    let nanos_suffix = duration.subsec_nanos() % 1_000_000;
    (secs, nanos_suffix)
}

/// Current Unix timestamp in seconds.
pub fn unix_now_secs() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
