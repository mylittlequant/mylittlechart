//! Leak-intern registry for footprint bucket-interval strings — per-family
//! scoping arc 2026-07-16.
//!
//! `bubble.chart_type_interval` is `&'static str` end-to-end (see
//! `mlc-backend::bubble::store::BubbleStore::set_chart_type_interval` /
//! `facade::set_chart_type_interval`), which is why `footprint_interval_static`
//! (`mlc-app::input::chart_out_events`) and the hydration re-clamp
//! (`mlc-app::persistence::hydration_impl`) used to be closed matches over a
//! fixed literal table — any user-typed custom bucket interval (e.g. `"7m"`,
//! `"90s"`) had nowhere `'static` to live and silently fell back to a
//! hardcoded default.
//!
//! [`intern_interval`] is the fix: known literals resolve through a small
//! `match` (zero lock, zero allocation — the common path, hit on every
//! restart/apply of a builtin interval), and any OTHER validated interval
//! string is leaked exactly once via [`Box::leak`] and cached in a
//! process-lifetime `HashSet<&'static str>` so repeat lookups of the SAME
//! custom string return the SAME `'static` reference instead of leaking
//! again. The leak is bounded by how many distinct custom bucket-interval
//! strings a user ever types in one process lifetime (a handful, typically
//! — this is a UI custom-value list, not a hot data path) and is the same
//! trade-off `Timeframe::parse`'s callers already accept implicitly for
//! chart_type ids elsewhere in the registry.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

/// Fixed table mirroring the pre-arc closed match
/// (`footprint_interval_static`'s 10 literals) — checked FIRST so the
/// common path (a builtin interval, which is the overwhelming majority of
/// lookups) never touches the lock or allocates.
const KNOWN_LITERALS: &[&str] = &[
    "5s", "15s", "30s",
    "1m", "3m", "5m", "15m", "30m", "1h", "4h",
];

fn leaked_intervals() -> &'static Mutex<HashSet<&'static str>> {
    static REGISTRY: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Resolve `name` to a `'static` string usable as a `chart_type_interval`
/// value.
///
/// Checks [`KNOWN_LITERALS`] first (no lock, no allocation). For any other
/// string, looks it up in the process-lifetime leaked-interval registry;
/// on first sight, [`Box::leak`]s an owned copy and caches it so later
/// lookups of the identical string return the SAME `'static` reference
/// rather than leaking again.
///
/// This function does not validate `name` — callers (`footprint_interval_
/// static`, the hydration re-clamp) are responsible for running
/// `Timeframe::parse` + the 1s..=1h clamp BEFORE interning, so garbage
/// input never reaches (and never bloats) the leak registry.
pub fn intern_interval(name: &str) -> &'static str {
    if let Some(known) = KNOWN_LITERALS.iter().find(|&&lit| lit == name) {
        return known;
    }
    let mut registry = leaked_intervals()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(existing) = registry.get(name) {
        return existing;
    }
    let leaked: &'static str = Box::leak(name.to_string().into_boxed_str());
    registry.insert(leaked);
    leaked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_literal_resolves_without_leaking() {
        assert_eq!(intern_interval("1m"), "1m");
        assert_eq!(intern_interval("4h"), "4h");
    }

    #[test]
    fn custom_interval_interns_and_is_stable() {
        let first = intern_interval("7m");
        let second = intern_interval("7m");
        assert_eq!(first, "7m");
        // Same underlying `'static` allocation on repeat lookup — pointer
        // equality proves no re-leak happened.
        assert_eq!(first.as_ptr(), second.as_ptr());
    }

    #[test]
    fn distinct_custom_intervals_intern_independently() {
        let a = intern_interval("11m");
        let b = intern_interval("13m");
        assert_eq!(a, "11m");
        assert_eq!(b, "13m");
        assert_ne!(a.as_ptr(), b.as_ptr());
    }
}
