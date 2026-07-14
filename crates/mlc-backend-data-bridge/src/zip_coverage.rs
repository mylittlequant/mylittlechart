//! Zip-vs-delta coverage map for the trade backfill planner (owner doctrine
//! 2026-07-14): for any instrument `(exchange, symbol, account_type)` whose
//! venue publishes a daily-archive-zip, the zip is ALWAYS the default
//! history source. REST trade pagination closes ONLY the regions the zip
//! does not (cannot) cover:
//!
//! 1. **head** — the current open UTC day (not yet a finished, publishable
//!    day file);
//! 2. **publication lag** — freshly-closed days the venue has not yet
//!    uploaded (`newest_published_day` trails "yesterday UTC" by some
//!    delay);
//! 3. **interior holes** — specific days inside `[floor..newest_published]`
//!    that 404'd (a genuine gap in the venue's archive host, not a
//!    boundary).
//!
//! There is no depth boundary switch anymore ("REST above X days, archive
//! below") — [`plan_fetch`] replaces it with an interval-algebra coverage
//! map, at UTC-day granularity.
//!
//! This module is pure data + pure functions — no I/O, no network, no
//! locking. [`ZipCoverage`] is the fact base (owned per-instrument by
//! [`crate::backfill_engine::BackfillEngine`]); [`plan_fetch`] is the
//! planner that turns a requested `[start_ms, end_ms)` window plus a
//! coverage snapshot into a [`FetchPlan`] the trade ramp executes.

use std::collections::BTreeSet;

use chrono::{Datelike, Duration, NaiveDate};

/// Milliseconds in one day — the day-boundary arithmetic unit throughout
/// this module.
const DAY_MS: i64 = 86_400_000;

/// What is known about a venue's daily-archive-zip coverage for one
/// `(exchange, symbol, account_type)` instrument. Built lazily (cold) and
/// refreshed lazily (its newest edge only) — see
/// `BackfillEngine`'s coverage-probe helper in `backfill_engine.rs`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ZipCoverage {
    /// Newest UTC day the archive host has actually published a file for.
    /// `None` = no published day is known yet (cold / never successfully
    /// probed / venue has zero archive coverage for this instrument) — a
    /// coverage this shallow makes the ENTIRE requested range fall back to
    /// delta (REST decides; see [`plan_fetch`]'s case (e)).
    pub newest_published_day: Option<NaiveDate>,
    /// UTC days strictly BETWEEN `floor` and `newest_published_day`
    /// (inclusive range) that a real fetch attempt 404'd on — i.e. genuine
    /// interior gaps, never a boundary. Populated ONLY from
    /// `ArchiveDayOutcome::Gap` (see `backfill_engine::classify_archive_day_
    /// result`) — a transient fetch error must NEVER be recorded here (the
    /// caller doesn't know yet whether that day is published or not).
    pub holes: BTreeSet<NaiveDate>,
    /// Inclusive lower bound of the archive host's retention — the UTC day
    /// containing the venue's declared `ArchiveSpec::since_ms`. Any
    /// requested day strictly below this floor is never zip-coverable and
    /// always falls back to delta (REST decides whatever it can there).
    pub floor: NaiveDate,
    /// Wall-clock ms of the last time this coverage's `newest_published_day`
    /// edge was (re)probed — drives the "lazy refresh: only re-probe the
    /// newest edge after it goes stale" rule in `backfill_engine.rs`.
    pub last_probe_ms: i64,
}

impl ZipCoverage {
    /// Fresh coverage for an instrument with a known archive floor and
    /// nothing probed yet.
    pub fn new(floor: NaiveDate) -> Self {
        Self { newest_published_day: None, holes: BTreeSet::new(), floor, last_probe_ms: 0 }
    }
}

/// One resolved fetch plan: the caller executes `zip_ranges` (via
/// `run_archive_group`, day file by day file) and `delta_ranges` (via the
/// REST walk) to cover exactly the originally `requested_ms` window, with
/// no double-coverage and no gaps.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FetchPlan {
    /// Inclusive `(start_day, end_day)` UTC-day pairs to fetch from the
    /// daily-archive-zip. `start_day <= end_day`; ranges are already
    /// hole-split (a hole inside a would-be contiguous run splits it into
    /// two ranges around the hole) and sorted newest-first (matching the
    /// archive walk's own backward-in-time direction).
    pub zip_ranges: Vec<(NaiveDate, NaiveDate)>,
    /// `[start_ms, end_ms)` sub-windows of the ORIGINAL requested range the
    /// zip does not (or cannot yet) cover — head, publication lag, holes,
    /// and anything below the archive floor. Sorted newest-first (largest
    /// `start_ms` first), matching the REST walk's own backward-in-time
    /// direction.
    pub delta_ranges: Vec<(i64, i64)>,
}

/// UTC day containing `ms` (floor division — matches
/// `digdigdig3_station::archive::url`'s day-from-ms convention: a
/// timestamp belongs to the day whose UTC midnight is `<= ms`).
fn day_from_ms(ms: i64) -> NaiveDate {
    let days = ms.div_euclid(DAY_MS);
    NaiveDate::from_num_days_from_ce_opt(719_163 + days as i32)
        .unwrap_or(NaiveDate::MIN)
}

/// UTC midnight of `date`, in epoch ms.
fn ms_from_day(date: NaiveDate) -> i64 {
    let days = date.num_days_from_ce() - 719_163;
    (days as i64) * DAY_MS
}

/// Pure interval algebra: split `requested_ms` into the zip-coverable
/// portion (day ranges, hole-aware) and everything else (delta ms ranges),
/// given a coverage snapshot and the current wall-clock `now_ms` (used only
/// to bound the head region — the zip can never cover "today UTC", it isn't
/// finished yet).
///
/// `requested_ms = (start_ms, end_ms)`: `start_ms` is the OLDEST edge
/// requested (further back in time, smaller), `end_ms` the NEWEST edge
/// (closer to now, larger) — same "oldest..newest" convention the ramp's
/// `target_ts..now` window already uses. `end_ms` is clamped to `now_ms`.
///
/// Day-granularity contract: the zip contributes WHOLE UTC day files
/// (`floor..=newest_published_day`, minus `holes`) for every published day
/// the request TOUCHES — including days only partially inside
/// `requested_ms` (a scrolled viewport almost always lands mid-day). The
/// day file is the atomic unit; rows overlapping what the store already
/// holds are dropped by the emit-time trim, so over-fetch inside a touched
/// day is bounded and harmless. Only genuinely unpublishable spans (today
/// UTC, publication lag above `newest_published_day`, holes, below-floor)
/// go to `delta_ranges`.
pub fn plan_fetch(requested_ms: (i64, i64), coverage: &ZipCoverage, now_ms: i64) -> FetchPlan {
    let (req_start, req_end_raw) = requested_ms;
    let req_end = req_end_raw.min(now_ms);
    if req_end <= req_start {
        return FetchPlan::default();
    }

    // "Today UTC" never has a finished archive file — the zip's newest
    // reachable day is at most yesterday UTC relative to `now_ms`, clamped
    // by whatever the coverage snapshot actually knows.
    let today = day_from_ms(now_ms);
    let yesterday = today - Duration::days(1);

    let zip_ceiling_day = coverage.newest_published_day.filter(|d| *d <= yesterday);

    // The zip claims ANY published day the request TOUCHES (intersection,
    // not containment): the zip is the DEFAULT source for published days
    // (owner doctrine) and a day file is atomic anyway — the overlap trim
    // at emit time drops rows the store already holds. Containment-only
    // routing sent the partially-requested day (every scroll lands
    // mid-day) to delta, which RecentOnly/FromId venues cannot fulfill —
    // the second day of history never loaded (live incident 2026-07-15).
    let touch_day_lo = day_from_ms(req_start.max(0));
    let touch_day_hi = day_from_ms(req_end - 1);

    let mut zip_days: Vec<NaiveDate> = Vec::new();
    if let Some(ceiling) = zip_ceiling_day {
        if touch_day_lo <= touch_day_hi {
            let lo = touch_day_lo.max(coverage.floor);
            let hi = touch_day_hi.min(ceiling);
            if lo <= hi {
                let mut d = lo;
                while d <= hi {
                    if !coverage.holes.contains(&d) {
                        zip_days.push(d);
                    }
                    d += Duration::days(1);
                }
            }
        }
    }

    // Hole-split zip_days (sorted ascending already) into contiguous
    // inclusive ranges, then report newest-first (reverse).
    let mut zip_ranges: Vec<(NaiveDate, NaiveDate)> = Vec::new();
    for d in zip_days {
        match zip_ranges.last_mut() {
            Some((_, hi)) if *hi + Duration::days(1) == d => {
                *hi = d;
            }
            _ => zip_ranges.push((d, d)),
        }
    }
    zip_ranges.reverse();

    // Delta = requested range MINUS the whole days the zip just claimed.
    // Build the covered-ms union from the (pre-reversal) zip day list by
    // re-deriving it from zip_ranges (cheaper than keeping both), then
    // subtract from [req_start, req_end).
    let mut covered: Vec<(i64, i64)> = zip_ranges
        .iter()
        .map(|(lo, hi)| (ms_from_day(*lo), ms_from_day(*hi) + DAY_MS))
        .collect();
    covered.sort_by_key(|(lo, _)| *lo);

    let mut delta_ranges: Vec<(i64, i64)> = Vec::new();
    let mut cursor = req_start;
    for (c_lo, c_hi) in covered {
        if c_lo > cursor {
            delta_ranges.push((cursor, c_lo.min(req_end)));
        }
        cursor = cursor.max(c_hi);
        if cursor >= req_end {
            break;
        }
    }
    if cursor < req_end {
        delta_ranges.push((cursor, req_end));
    }
    // Drop any degenerate zero-length slivers produced by exact-boundary
    // adjacency.
    delta_ranges.retain(|(lo, hi)| hi > lo);
    // Newest-first, matching the ramp's backward-in-time walk direction.
    delta_ranges.reverse();

    FetchPlan { zip_ranges, delta_ranges }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap_or_default()
    }

    fn day_ms(y: i32, m: u32, d: u32) -> i64 {
        ms_from_day(day(y, m, d))
    }

    // (a) requested entirely within "head" (today UTC, no finished archive
    // day at all) -> single delta range, no zip.
    #[test]
    fn requested_entirely_in_head_is_all_delta() {
        let now = day_ms(2026, 7, 14) + 6 * 3_600_000; // 06:00 UTC today
        let coverage = ZipCoverage {
            newest_published_day: Some(day(2026, 7, 12)), // published up through yesterday-1
            holes: BTreeSet::new(),
            floor: day(2026, 1, 1),
            last_probe_ms: now,
        };
        // requested = last 3 hours of "today" only.
        let requested = (now - 3 * 3_600_000, now);
        let plan = plan_fetch(requested, &coverage, now);
        assert!(plan.zip_ranges.is_empty(), "no whole day inside a 3h same-day window");
        assert_eq!(plan.delta_ranges, vec![requested]);
    }

    // (b) requested range crosses the newest-published edge -> zip covers
    // the fully-published whole days, delta covers the head remainder.
    #[test]
    fn requested_crossing_newest_edge_splits_zip_and_delta() {
        let now = day_ms(2026, 7, 14) + 6 * 3_600_000; // 06:00 UTC on the 14th
        let coverage = ZipCoverage {
            newest_published_day: Some(day(2026, 7, 12)),
            holes: BTreeSet::new(),
            floor: day(2026, 1, 1),
            last_probe_ms: now,
        };
        // requested = from start of 2026-07-10 through "now".
        let requested = (day_ms(2026, 7, 10), now);
        let plan = plan_fetch(requested, &coverage, now);
        assert_eq!(plan.zip_ranges, vec![(day(2026, 7, 10), day(2026, 7, 12))]);
        // Delta = [start of 07-13 .. now) — everything the zip could not
        // cover (07-13 unpublished-lag day + 07-14 head slice).
        assert_eq!(plan.delta_ranges, vec![(day_ms(2026, 7, 13), now)]);
    }

    // (c) a hole inside the range -> zip range split around it, hole falls
    // into delta.
    #[test]
    fn interior_hole_splits_zip_range_and_becomes_delta() {
        let now = day_ms(2026, 7, 14) + 1_000;
        let mut holes = BTreeSet::new();
        holes.insert(day(2026, 7, 11));
        let coverage = ZipCoverage {
            newest_published_day: Some(day(2026, 7, 12)),
            holes,
            floor: day(2026, 1, 1),
            last_probe_ms: now,
        };
        let requested = (day_ms(2026, 7, 9), day_ms(2026, 7, 13));
        let plan = plan_fetch(requested, &coverage, now);
        // Newest-first: (07-12,07-12) then (07-09,07-10) around the 07-11 hole.
        assert_eq!(
            plan.zip_ranges,
            vec![(day(2026, 7, 12), day(2026, 7, 12)), (day(2026, 7, 9), day(2026, 7, 10))]
        );
        assert_eq!(plan.delta_ranges, vec![(day_ms(2026, 7, 11), day_ms(2026, 7, 12))]);
    }

    // (d) requested range entirely below the archive floor -> pure delta,
    // REST decides (zip cannot possibly help, no matter how deep).
    #[test]
    fn requested_below_floor_is_all_delta() {
        let now = day_ms(2026, 7, 14);
        let coverage = ZipCoverage {
            newest_published_day: Some(day(2026, 7, 12)),
            holes: BTreeSet::new(),
            floor: day(2026, 6, 1),
            last_probe_ms: now,
        };
        let requested = (day_ms(2026, 5, 1), day_ms(2026, 5, 5));
        let plan = plan_fetch(requested, &coverage, now);
        assert!(plan.zip_ranges.is_empty(), "entire request is below the archive retention floor");
        assert_eq!(plan.delta_ranges, vec![requested]);
    }

    // (e) coverage is empty (newest_published_day = None, never probed
    // successfully / no archive at all) -> everything is delta.
    #[test]
    fn empty_coverage_is_all_delta() {
        let now = day_ms(2026, 7, 14);
        let coverage = ZipCoverage::new(day(2026, 1, 1));
        let requested = (day_ms(2026, 7, 1), day_ms(2026, 7, 10));
        let plan = plan_fetch(requested, &coverage, now);
        assert!(plan.zip_ranges.is_empty());
        assert_eq!(plan.delta_ranges, vec![requested]);
    }

    // Extra: zero/negative-length requested window yields an empty plan
    // (defensive — callers should never construct this, but the algebra
    // must not panic or emit degenerate ranges).
    #[test]
    fn degenerate_requested_window_yields_empty_plan() {
        let now = day_ms(2026, 7, 14);
        let coverage = ZipCoverage::new(day(2026, 1, 1));
        let plan = plan_fetch((now, now), &coverage, now);
        assert!(plan.zip_ranges.is_empty());
        assert!(plan.delta_ranges.is_empty());
    }

    // Regression (live incident 2026-07-15): a scrolled viewport lands
    // mid-day — the touched published day MUST be claimed by the zip
    // (intersection, not containment), otherwise it falls to delta which
    // RecentOnly/FromId venues cannot fulfill and the second day of
    // history never loads.
    #[test]
    fn partially_touched_published_day_is_claimed_by_zip() {
        let now = day_ms(2026, 7, 14) + 20 * 3_600_000; // 20:00 UTC on the 14th
        let coverage = ZipCoverage {
            newest_published_day: Some(day(2026, 7, 13)),
            holes: BTreeSet::new(),
            floor: day(2026, 1, 1),
            last_probe_ms: now,
        };
        // Store already holds [2026-07-13 00:00 .. now); the scrolled
        // viewport asks down to 22:00 on the 12th — a partial touch of
        // day 07-12 only.
        let requested = (day_ms(2026, 7, 12) + 22 * 3_600_000, day_ms(2026, 7, 13));
        let plan = plan_fetch(requested, &coverage, now);
        assert_eq!(plan.zip_ranges, vec![(day(2026, 7, 12), day(2026, 7, 12))]);
        assert!(plan.delta_ranges.is_empty(), "zip day covers the partial slice; no delta remains");
    }

    // Extra: requested end beyond "now" clamps to now.
    #[test]
    fn requested_end_beyond_now_clamps() {
        let now = day_ms(2026, 7, 14) + 3_600_000;
        let coverage = ZipCoverage::new(day(2026, 1, 1));
        let requested = (now - 1_000, now + 999_999_999);
        let plan = plan_fetch(requested, &coverage, now);
        assert_eq!(plan.delta_ranges, vec![(now - 1_000, now)]);
    }
}
