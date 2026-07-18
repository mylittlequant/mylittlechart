//! Time Scale - Calendar-aware tick mark system with intraday support
//!
//! Combines:
//! - Calendar-aware month/day boundaries (accurate 1st of month alignment)
//! - Smooth intraday transitions (hours, minutes) at high zoom levels
//!
//! Grid levels:
//! - Zoom out: Year → Month → Day (with 2d, 3d, 5d grouping from 1st)
//! - Zoom in: Day → Hour4 → Hour → Minute30 → Minute5 → Minute1

use crate::chart::types::Viewport;
use crate::Bar;
use crate::i18n::{month_names_short, current_language};
use crate::{TimeFormatSettings, DateFormat};

// =============================================================================
// Time Constants
// =============================================================================

/// One second (base unit for sub-minute / derived-bar zoom levels)
pub const SECOND: i64 = 1;
/// Seconds in a minute
pub const MINUTE: i64 = 60;
/// Seconds in an hour
pub const HOUR: i64 = 3600;
/// Seconds in a day
pub const DAY: i64 = 86400;

// =============================================================================
// Calendar Utilities (no external dependencies)
// =============================================================================

/// Days in each month (non-leap year)
const DAYS_IN_MONTH: [i32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

/// Check if year is a leap year
fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

/// Get days in a specific month (1-indexed, handles leap years)
fn days_in_month(year: i32, month: i32) -> i32 {
    if month == 2 && is_leap_year(year) {
        29
    } else {
        DAYS_IN_MONTH[(month - 1) as usize]
    }
}

/// Convert Unix timestamp (seconds) to date components
/// Returns: (year, month 1-12, day 1-31, hour 0-23, minute 0-59, second 0-59)
pub fn timestamp_to_date(ts: i64) -> (i32, i32, i32, i32, i32, i32) {
    // Handle time of day
    let time_of_day = ts.rem_euclid(DAY);
    let hour = (time_of_day / HOUR) as i32;
    let minute = ((time_of_day % HOUR) / MINUTE) as i32;
    let second = (time_of_day % MINUTE) as i32;

    // Calculate days since epoch (can be negative for dates before 1970)
    let mut days = ts.div_euclid(DAY);

    // Start from 1970-01-01
    let mut year = 1970_i32;

    // Handle years
    if days >= 0 {
        loop {
            let days_in_year = if is_leap_year(year) { 366 } else { 365 };
            if days < days_in_year as i64 {
                break;
            }
            days -= days_in_year as i64;
            year += 1;
        }
    } else {
        // Go backwards for negative days
        loop {
            year -= 1;
            let days_in_year = if is_leap_year(year) { 366 } else { 365 };
            days += days_in_year as i64;
            if days >= 0 {
                break;
            }
        }
    }

    // Handle months
    let mut month = 1_i32;
    loop {
        let dim = days_in_month(year, month) as i64;
        if days < dim {
            break;
        }
        days -= dim;
        month += 1;
    }

    let day = days as i32 + 1; // Days are 1-indexed

    (year, month, day, hour, minute, second)
}

/// Convert date components to Unix timestamp (seconds)
/// month: 1-12, day: 1-31
fn date_to_timestamp(year: i32, month: i32, day: i32, hour: i32, minute: i32, second: i32) -> i64 {
    // Calculate days from epoch to start of year
    let mut days: i64 = 0;

    if year >= 1970 {
        for y in 1970..year {
            days += if is_leap_year(y) { 366 } else { 365 };
        }
    } else {
        for y in year..1970 {
            days -= if is_leap_year(y) { 366 } else { 365 };
        }
    }

    // Add days for months
    for m in 1..month {
        days += days_in_month(year, m) as i64;
    }

    // Add days (1-indexed, so subtract 1)
    days += (day - 1) as i64;

    // Convert to seconds and add time
    days * DAY + hour as i64 * HOUR + minute as i64 * MINUTE + second as i64
}

// =============================================================================
// Tick Mark Weight
// =============================================================================

/// Hierarchical weights: Year=70, Month=60, Day=50, Hour=30, etc.
/// Higher weight = more important = larger font/brighter color.
///
/// Variants mirror TradingView Lightweight Charts'
/// `TickMarkWeight` so the LWC weight-by-time algorithm ports verbatim
/// (see `weight_by_time` and `fill_weights`).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
#[repr(u8)]
pub enum TickMarkWeight {
    /// Sub-second resolution (range/tick bars firing inside a single second).
    LessThanSecond = 1,
    /// Sub-minute granularity.
    #[default]
    Second = 5,
    /// 1-minute boundaries
    Minute1 = 10,
    /// 5-minute boundaries
    Minute5 = 15,
    /// 30-minute boundaries
    Minute30 = 20,
    /// Hour boundaries
    Hour = 30,
    /// 3-hour boundaries
    Hour3 = 31,
    /// 4-hour boundaries
    Hour4 = 35,
    /// 6-hour boundaries
    Hour6 = 36,
    /// 12-hour boundaries
    Hour12 = 37,
    /// Day boundaries
    Day = 50,
    /// Month boundaries
    Month = 60,
    /// Year boundaries
    Year = 70,
}

impl TickMarkWeight {
    /// Check if this weight is major (Year/Month) for brighter styling
    pub fn is_major(&self) -> bool {
        matches!(self, TickMarkWeight::Year | TickMarkWeight::Month)
    }

    /// Check if this weight is medium (Day) for medium styling
    pub fn is_medium(&self) -> bool {
        matches!(self, TickMarkWeight::Day)
    }
}

/// LWC intraday divisor table (ms-per-boundary, ascending) — verbatim from
/// `lightweight-charts/src/model/horz-scale-behavior-time/time-scale-point-weight-generator.ts`.
const INTRADAY_DIVISORS: &[(i64, TickMarkWeight)] = &[
    (1_000,       TickMarkWeight::Second),
    (60_000,      TickMarkWeight::Minute1),
    (300_000,     TickMarkWeight::Minute5),
    (1_800_000,   TickMarkWeight::Minute30),
    (3_600_000,   TickMarkWeight::Hour),
    (10_800_000,  TickMarkWeight::Hour3),
    (21_600_000,  TickMarkWeight::Hour6),
    (43_200_000,  TickMarkWeight::Hour12),
];

/// Weight for a bar given its and the previous bar's timestamps (UTC ms).
///
/// Returns the highest calendar boundary that was actually crossed between
/// `prev_ms` and `cur_ms`. This is the LWC `weightByTime` function in Rust.
pub fn weight_by_time(cur_ms: i64, prev_ms: i64) -> TickMarkWeight {
    let (cy, cm, cd, _, _, _) = timestamp_to_date(cur_ms / 1000);
    let (py, pm, pd, _, _, _) = timestamp_to_date(prev_ms / 1000);
    if cy != py { return TickMarkWeight::Year; }
    if cm != pm { return TickMarkWeight::Month; }
    if cd != pd { return TickMarkWeight::Day; }
    // Largest matching intraday boundary first.
    for (div, w) in INTRADAY_DIVISORS.iter().rev() {
        if cur_ms / div != prev_ms / div {
            return *w;
        }
    }
    TickMarkWeight::LessThanSecond
}

/// Detect non-uniform-time bars. `bars` is considered non-uniform if the
/// maximum gap between adjacent timestamps is > 2× the median gap. This
/// flags range/volume/tick charts (where adjacent bars can span wildly
/// different time deltas) and routes them through the LWC per-bar weight
/// path instead of the uniform `day_width_px` interval dispatch.
fn is_non_uniform(bars: &[Bar]) -> bool {
    let n = bars.len();
    if n < 3 { return false; }
    // Sample up to the last 100 gaps for stability on long histories.
    let sample = n.min(100);
    let start = n - sample;
    let mut gaps: Vec<i64> = Vec::with_capacity(sample - 1);
    for i in (start + 1)..n {
        let g = (bars[i].timestamp - bars[i - 1].timestamp).abs();
        if g > 0 { gaps.push(g); }
    }
    if gaps.len() < 2 { return false; }
    gaps.sort_unstable();
    let median = gaps[gaps.len() / 2];
    let max = *gaps.last().unwrap();
    median > 0 && max > median.saturating_mul(2)
}

/// Fill per-bar weights given a sorted slice of `(timestamp_secs, weight)`
/// tuples. `start` is the first index to (re-)compute (0 = full bulk pass,
/// `N` = incremental tail re-weight after appending bars). Weights are
/// written into `points[i].1`.
///
/// Mirrors LWC `fillWeightsForPoints` — first point gets a weight estimated
/// from the average interval, and downstream points compare against their
/// real predecessor.
pub fn fill_weights(points: &mut [(i64, TickMarkWeight)], start: usize) {
    if points.is_empty() { return; }
    let mut prev_ms: Option<i64> = if start > 0 {
        Some(points[start - 1].0 * 1000)
    } else {
        None
    };
    let mut total_diff: i64 = 0;
    let mut diff_count: i64 = 0;

    for i in start..points.len() {
        let cur_ms = points[i].0 * 1000;
        if let Some(p) = prev_ms {
            points[i].1 = weight_by_time(cur_ms, p);
            total_diff += cur_ms - p;
            diff_count += 1;
        }
        prev_ms = Some(cur_ms);
    }

    // First point of a full pass has no real predecessor — synthesize from
    // the average inter-bar interval. Only meaningful for >= 2 bars.
    if start == 0 && points.len() > 1 && diff_count > 0 {
        let avg = (total_diff + diff_count - 1) / diff_count;
        let approx_prev_ms = points[0].0 * 1000 - avg;
        points[0].1 = weight_by_time(points[0].0 * 1000, approx_prev_ms);
    }
}

// =============================================================================
// Time Tick
// =============================================================================

/// A tick mark on the time scale with position, weight, and label
#[derive(Clone, Debug)]
pub struct TimeTick {
    /// Bar index this tick corresponds to (can be negative for extrapolated past)
    pub bar_idx: i64,
    /// X pixel coordinate
    pub x: f64,
    /// Tick weight for styling
    pub weight: TickMarkWeight,
    /// Formatted label text
    pub label: String,
}

// =============================================================================
// Time Scale
// =============================================================================

/// Time scale configuration and tick generation
#[derive(Clone, Debug, Default)]
pub struct TimeScale {
    /// Minimum spacing in bars between tick marks
    min_tick_spacing_bars: Option<usize>,
}

impl TimeScale {
    /// Create a new time scale with default settings
    pub fn new() -> Self {
        Self::default()
    }

    /// Set minimum tick spacing in bars
    pub fn with_min_spacing(mut self, bars: usize) -> Self {
        self.min_tick_spacing_bars = Some(bars);
        self
    }

    /// Convert bar index to timestamp using real bar data where available.
    ///
    /// For indices within `[0..bars.len())` returns the actual bar timestamp.
    /// For indices outside that range extrapolates from the nearest edge using `bar_interval`.
    fn bar_idx_to_timestamp(idx: i64, bars: &[Bar], first_ts: i64, bar_interval: i64) -> i64 {
        if bars.is_empty() {
            return first_ts + idx * bar_interval;
        }
        if idx >= 0 && (idx as usize) < bars.len() {
            return bars[idx as usize].timestamp;
        }
        if idx < 0 {
            // Extrapolate backwards from the first bar
            first_ts + idx * bar_interval
        } else {
            // Extrapolate forwards from the last bar
            let last_ts = bars[bars.len() - 1].timestamp;
            let beyond = idx - (bars.len() as i64 - 1);
            last_ts + beyond * bar_interval
        }
    }

    /// Convert timestamp to bar index using real bar data where available.
    ///
    /// For timestamps within the data range performs a binary search.
    /// For timestamps outside the range extrapolates from the nearest edge.
    fn timestamp_to_bar_idx(ts: i64, bars: &[Bar], first_ts: i64, bar_interval: i64) -> i64 {
        if bars.is_empty() || bar_interval <= 0 {
            return (ts - first_ts) / bar_interval.max(1);
        }
        let last_ts = bars[bars.len() - 1].timestamp;
        if ts < first_ts {
            // Before data: extrapolate backwards from first bar
            (ts - first_ts) / bar_interval
        } else if ts > last_ts {
            // After data: extrapolate forwards from last bar
            let beyond_secs = ts - last_ts;
            (bars.len() as i64 - 1) + beyond_secs / bar_interval
        } else {
            // Within data: binary search for closest bar
            match bars.binary_search_by_key(&ts, |b| b.timestamp) {
                Ok(idx) => idx as i64,
                Err(idx) => idx as i64,
            }
        }
    }

    /// Convert timestamp to the NEAREST real bar index, clamped at the ends
    /// of `bars` — never extrapolates. Used by
    /// [`Self::generate_calendar_grid_ticks`]: a calendar boundary outside
    /// `[bars[0].timestamp, bars[last].timestamp]` has no honest bar to
    /// attach to, so it maps to whichever real end bar is closest instead of
    /// fabricating a synthetic index past the data (unlike
    /// [`Self::timestamp_to_bar_idx`], which extrapolates for the uniform
    /// kline path's off-screen padding).
    fn timestamp_to_bar_idx_nearest_clamped(ts: i64, bars: &[Bar]) -> usize {
        if bars.is_empty() {
            return 0;
        }
        let last = bars.len() - 1;
        if ts <= bars[0].timestamp {
            return 0;
        }
        if ts >= bars[last].timestamp {
            return last;
        }
        match bars.binary_search_by_key(&ts, |b| b.timestamp) {
            Ok(idx) => idx,
            Err(idx) => {
                // idx is the insertion point (0 < idx <= last since ts is
                // strictly between bars[0] and bars[last] here). Compare the
                // neighbor on each side and keep the closer one.
                let before = idx - 1;
                let after = idx;
                let d_before = (ts - bars[before].timestamp).abs();
                let d_after = (bars[after].timestamp - ts).abs();
                if d_before <= d_after { before } else { after }
            }
        }
    }

    /// Grid cell constraints (in pixels)
    const MIN_CELL_WIDTH: f64 = 50.0;
    const MAX_CELL_WIDTH: f64 = 250.0;

    /// Allowed day steps for grid
    const DAY_STEPS: [i32; 5] = [1, 2, 3, 4, 5];

    /// Tick days for each (month_days, step) combination
    /// Format: array of day numbers to show ticks on
    /// The gap from last tick to 1st of next month is the "cheat" cell
    const TICKS_31_STEP1: [i32; 31] = [1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31];
    const TICKS_31_STEP2: [i32; 15] = [1,3,5,7,9,11,13,15,17,19,21,23,25,27,29];
    const TICKS_31_STEP3: [i32; 10] = [1,4,7,10,13,16,19,22,25,28];
    const TICKS_31_STEP4: [i32; 8] = [1,5,9,13,17,21,25,29];
    const TICKS_31_STEP5: [i32; 6] = [1,6,11,16,21,26];

    const TICKS_30_STEP1: [i32; 30] = [1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30];
    const TICKS_30_STEP2: [i32; 15] = [1,3,5,7,9,11,13,15,17,19,21,23,25,27,29];
    const TICKS_30_STEP3: [i32; 10] = [1,4,7,10,13,16,19,22,25,28];
    const TICKS_30_STEP4: [i32; 8] = [1,5,9,13,17,21,25,29];
    const TICKS_30_STEP5: [i32; 6] = [1,6,11,16,21,26];

    const TICKS_29_STEP1: [i32; 29] = [1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29];
    const TICKS_29_STEP2: [i32; 15] = [1,3,5,7,9,11,13,15,17,19,21,23,25,27,29];
    const TICKS_29_STEP3: [i32; 10] = [1,4,7,10,13,16,19,22,25,28];
    const TICKS_29_STEP4: [i32; 8] = [1,5,9,13,17,21,25,29];
    const TICKS_29_STEP5: [i32; 6] = [1,6,11,16,21,26];

    const TICKS_28_STEP1: [i32; 28] = [1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28];
    const TICKS_28_STEP2: [i32; 14] = [1,3,5,7,9,11,13,15,17,19,21,23,25,27];
    const TICKS_28_STEP3: [i32; 10] = [1,4,7,10,13,16,19,22,25,28];
    const TICKS_28_STEP4: [i32; 7] = [1,5,9,13,17,21,25];
    const TICKS_28_STEP5: [i32; 6] = [1,6,11,16,21,26];

    /// Get tick days for a given month length and step
    fn get_tick_days(month_days: i32, step: i32) -> &'static [i32] {
        match (month_days, step) {
            (31, 1) => &Self::TICKS_31_STEP1,
            (31, 2) => &Self::TICKS_31_STEP2,
            (31, 3) => &Self::TICKS_31_STEP3,
            (31, 4) => &Self::TICKS_31_STEP4,
            (31, 5) => &Self::TICKS_31_STEP5,
            (30, 1) => &Self::TICKS_30_STEP1,
            (30, 2) => &Self::TICKS_30_STEP2,
            (30, 3) => &Self::TICKS_30_STEP3,
            (30, 4) => &Self::TICKS_30_STEP4,
            (30, 5) => &Self::TICKS_30_STEP5,
            (29, 1) => &Self::TICKS_29_STEP1,
            (29, 2) => &Self::TICKS_29_STEP2,
            (29, 3) => &Self::TICKS_29_STEP3,
            (29, 4) => &Self::TICKS_29_STEP4,
            (29, 5) => &Self::TICKS_29_STEP5,
            (28, 1) => &Self::TICKS_28_STEP1,
            (28, 2) => &Self::TICKS_28_STEP2,
            (28, 3) => &Self::TICKS_28_STEP3,
            (28, 4) => &Self::TICKS_28_STEP4,
            (28, 5) => &Self::TICKS_28_STEP5,
            // Fallback: use 31-day pattern
            (_, 1) => &Self::TICKS_31_STEP1,
            (_, 2) => &Self::TICKS_31_STEP2,
            (_, 3) => &Self::TICKS_31_STEP3,
            (_, 4) => &Self::TICKS_31_STEP4,
            _ => &Self::TICKS_31_STEP5,
        }
    }

    /// Generate time ticks for the visible range.
    ///
    /// Algorithm:
    /// 1. Calculate day_width_px (how many pixels one day spans)
    /// 2. If day_width_px > threshold: use intraday intervals (hours, minutes)
    /// 3. Otherwise: use calendar-aware day intervals anchored to 1st of month
    pub fn generate_ticks<F>(
        &self,
        viewport: &Viewport,
        bars: &[Bar],
        measure_text: F,
        format_settings: Option<&TimeFormatSettings>,
        is_non_temporal: bool,
    ) -> Vec<TimeTick>
    where
        F: Fn(&str) -> f64,
    {
        let mut ticks = Vec::new();

        if bars.is_empty() {
            return ticks;
        }

        // Non-temporal chart types (renko / pnf / kagi / three_line_break)
        // render on a derived series backed by the ts-shadow built from
        // each brick/column/segment/line's real open timestamp
        // (`active_series::to_bar_shadow` — OHLC-zeroed `Bar`s, but the
        // `timestamp` field is genuine emission-order data, not synthetic).
        // That means a real auto calendar grid IS meaningful for them: walk
        // round calendar boundaries and snap each to its nearest real
        // element, same as the counted-bar family below. Only fall back to
        // the pure per-bar picker (no cadence, candidates = the elements'
        // own timestamps) when the calendar generator can't find a usable
        // step — degenerate shadows (e.g. a 2-element renko, or every
        // element sharing one timestamp) — so the strip is never left blank.
        if is_non_temporal {
            self.generate_calendar_grid_ticks(
                &mut ticks,
                bars,
                viewport,
                &measure_text,
                format_settings,
            );
            if ticks.is_empty() {
                self.generate_per_bar_ticks(
                    &mut ticks,
                    bars,
                    viewport,
                    &measure_text,
                    format_settings,
                );
            }
            ticks.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));
            return ticks;
        }

        // Detect non-uniform bar spacing (range/volume/tick bars). When the
        // max gap between adjacent bars is > 2× the median gap, the existing
        // `day_width_px` interval dispatch picks labels at wrong timestamps
        // (the boundary cells skip over real bars). These bars carry real
        // per-bar close timestamps — route them through the same auto
        // calendar grid as uniform kline charts (round boundaries snapped to
        // the nearest real bar), falling back to the LWC per-bar weight path
        // only if the calendar generator yields nothing (e.g. too few bars).
        let non_uniform = bars.len() >= 3 && is_non_uniform(bars);
        if non_uniform {
            self.generate_calendar_grid_ticks(
                &mut ticks,
                bars,
                viewport,
                &measure_text,
                format_settings,
            );
            if ticks.is_empty() {
                self.generate_per_bar_ticks(
                    &mut ticks,
                    bars,
                    viewport,
                    &measure_text,
                    format_settings,
                );
            }
            ticks.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));
            return ticks;
        }

        // Calculate bar interval from data
        let bar_interval = if bars.len() >= 2 {
            let sample_count = bars.len().min(10);
            let sample_start = bars.len() - sample_count;
            let total_time = bars[bars.len() - 1].timestamp - bars[sample_start].timestamp;
            (total_time / (sample_count - 1) as i64).max(1)
        } else {
            HOUR
        };

        let first_ts = bars[0].timestamp;

        // Visible range
        let visible_bars = viewport.visible_bars();
        let start_bar = viewport.view_start.floor() as i64;
        let end_bar = (viewport.view_start + visible_bars as f64).ceil() as i64;

        let start_ts = Self::bar_idx_to_timestamp(start_bar - 50, bars, first_ts, bar_interval);
        let end_ts = Self::bar_idx_to_timestamp(end_bar + 50, bars, first_ts, bar_interval);

        // How many pixels is one day?
        let bars_per_day = DAY / bar_interval;
        let day_width_px = bars_per_day as f64 * viewport.bar_spacing;

        // Decision point: intraday vs daily
        // If 1 day > 4 * MIN_CELL_WIDTH (200px), use intraday intervals
        if day_width_px > 4.0 * Self::MIN_CELL_WIDTH {
            // INTRADAY MODE: use hour/minute intervals
            self.generate_intraday_ticks(
                &mut ticks,
                start_ts, end_ts,
                bars, first_ts, bar_interval,
                viewport,
                day_width_px,
                &measure_text,
                format_settings,
            );
        } else {
            // DAILY MODE: use calendar-aware day intervals
            self.generate_daily_ticks(
                &mut ticks,
                start_ts, end_ts,
                bars, first_ts, bar_interval,
                viewport,
                day_width_px,
                &measure_text,
                format_settings,
            );
        }

        // Sort by x position
        ticks.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));
        ticks
    }

    /// LWC-style per-bar tick generation for non-uniform-time bar charts
    /// (range / volume / tick). For each visible bar emits one candidate with
    /// `weight_by_time(bar[i], bar[i-1])`. The picker then selects on weight
    /// desc + bilateral index gap.
    fn generate_per_bar_ticks<F>(
        &self,
        ticks: &mut Vec<TimeTick>,
        bars: &[Bar],
        viewport: &Viewport,
        measure_text: &F,
        format_settings: Option<&TimeFormatSettings>,
    ) where
        F: Fn(&str) -> f64,
    {
        // Build weights array sized to the visible window plus a small
        // off-screen buffer (matches the legacy ±50-bar pad).
        let visible_bars = viewport.visible_bars() as i64;
        let start_bar = ((viewport.view_start.floor() as i64) - 50).max(0);
        let end_bar = ((viewport.view_start as i64) + visible_bars + 50).min(bars.len() as i64);

        if end_bar <= start_bar {
            return;
        }

        // Build (timestamp, weight) tuples for the visible slice and one
        // extra leading bar (so weight[0] has a real predecessor).
        let lead = (start_bar - 1).max(0) as usize;
        let from = lead;
        let to = end_bar as usize;
        let mut pts: Vec<(i64, TickMarkWeight)> = bars[from..to]
            .iter()
            .map(|b| (b.timestamp, TickMarkWeight::Second))
            .collect();
        // First entry of the slice gets a real predecessor (lead) so we
        // start filling from index 1.
        fill_weights(&mut pts, if from == 0 { 0 } else { 1 });

        // Convert to picker candidates: (bar_idx, x, weight, ts).
        let mut candidates: Vec<(i64, f64, TickMarkWeight, i64)> =
            Vec::with_capacity(pts.len());
        for (i_local, (ts, weight)) in pts.iter().enumerate() {
            let bar_idx = (from + i_local) as i64;
            // Use the axis-aware `x_for_bar` helper so the labels land
            // correctly when the time-axis path is active. In pure
            // index-mode `x_for_bar` resolves to `bar_to_x` so there is
            // no regression.
            let x = viewport.x_for_bar(bar_idx as usize, *ts);
            if x < -100.0 || x > viewport.chart_width + 100.0 {
                continue;
            }
            candidates.push((bar_idx, x, *weight, *ts));
        }

        // Reuse the bilateral-gap picker.
        self.select_ticks_with_spacing(ticks, candidates, viewport, measure_text, format_settings);
    }

    /// Auto calendar grid for non-uniform-time series (counted-bar family:
    /// range/volume/dollar/tick/imbalance/run bars; also renko/pnf/kagi/
    /// three_line_break when their ts-shadow carries real per-element
    /// timestamps). Unlike [`Self::generate_per_bar_ticks`] (candidates =
    /// the bars' own irregular timestamps, no cadence), this walks ROUND
    /// calendar boundaries — the same step ladder klines use — across the
    /// visible time range and maps each boundary to the nearest real bar by
    /// timestamp. Pixel spacing between resulting gridlines is therefore
    /// non-uniform (bars are index-positioned); that is the intended model.
    ///
    /// Density is derived from the MEDIAN inter-bar duration over the
    /// visible slice (not last-10 mean, which the uniform path uses) —
    /// robust to bursts where several bars land in the same second and a
    /// mean would understate the true cadence.
    ///
    /// Extends into the FUTURE region (right of the last real bar) when the
    /// viewport looks past it, mirroring the uniform kline path's off-screen
    /// pad (`generate_ticks`'s `end_bar + 50` walk via the extrapolating
    /// `timestamp_to_bar_idx`): the boundary walk's `end_ts` is extended by
    /// forward extrapolation off `median_interval`, and boundaries beyond
    /// the last real bar's timestamp resolve to a forward-extrapolated bar
    /// index instead of clamping to the last real bar. Boundaries within
    /// `[start_bar, end_bar]` still resolve via the clamped nearest-bar
    /// search, unchanged from before this extension.
    fn generate_calendar_grid_ticks<F>(
        &self,
        ticks: &mut Vec<TimeTick>,
        bars: &[Bar],
        viewport: &Viewport,
        measure_text: &F,
        format_settings: Option<&TimeFormatSettings>,
    ) where
        F: Fn(&str) -> f64,
    {
        if bars.len() < 2 {
            return;
        }

        // Visible bar range (same ±50 pad discipline as the other pickers).
        let visible_bars = viewport.visible_bars() as i64;
        let start_bar = ((viewport.view_start.floor() as i64) - 50)
            .max(0)
            .min(bars.len() as i64 - 1) as usize;
        let end_bar = (((viewport.view_start as i64) + visible_bars + 50) as usize)
            .min(bars.len() - 1);
        if end_bar <= start_bar {
            return;
        }

        // Median inter-bar duration over the visible slice.
        let mut gaps: Vec<i64> = Vec::with_capacity(end_bar - start_bar);
        for i in (start_bar + 1)..=end_bar {
            let g = bars[i].timestamp - bars[i - 1].timestamp;
            if g > 0 { gaps.push(g); }
        }
        if gaps.is_empty() {
            return;
        }
        gaps.sort_unstable();
        let median_interval = gaps[gaps.len() / 2].max(1);

        let start_ts = bars[start_bar].timestamp;
        let mut end_ts = bars[end_bar].timestamp;
        if end_ts <= start_ts {
            return;
        }

        // Viewport-driven right bound, mirroring the uniform kline path's
        // `end_bar = (view_start + visible_bars).ceil() + 50` off-screen pad
        // (`generate_ticks`, ~line 565/568). When the viewport looks past
        // the last real bar, extend the boundary walk's `end_ts` by forward
        // extrapolation from the last real bar using the median interval
        // computed above, so the calendar walk covers the future region
        // just like the uniform path's `bar_idx_to_timestamp` extrapolation.
        let last_real_idx = bars.len() as i64 - 1;
        let end_bar_vp = (viewport.view_start + visible_bars as f64).ceil() as i64 + 50;
        if end_bar_vp > last_real_idx {
            let beyond = end_bar_vp - last_real_idx;
            let extrapolated_end_ts = bars[last_real_idx as usize].timestamp + beyond * median_interval;
            if extrapolated_end_ts > end_ts {
                end_ts = extrapolated_end_ts;
            }
        }

        // Same day_width_px density measure the uniform path uses, driven
        // off the median interval instead of the last-10-bar mean.
        let bars_per_day = (DAY / median_interval).max(1);
        let day_width_px = bars_per_day as f64 * viewport.bar_spacing;

        // Raw boundary candidates: (timestamp, weight), round calendar
        // boundaries only — no per-bar timestamps here.
        let mut boundaries: Vec<(i64, TickMarkWeight)> = Vec::new();

        if day_width_px > 4.0 * Self::MIN_CELL_WIDTH {
            // Intraday regime: same divisor ladder as generate_intraday_ticks.
            let (time_interval, weight) = if day_width_px > 86400.0 * Self::MIN_CELL_WIDTH {
                (SECOND, TickMarkWeight::Second)
            } else if day_width_px > 17280.0 * Self::MIN_CELL_WIDTH {
                (5 * SECOND, TickMarkWeight::Second)
            } else if day_width_px > 8640.0 * Self::MIN_CELL_WIDTH {
                (10 * SECOND, TickMarkWeight::Second)
            } else if day_width_px > 2880.0 * Self::MIN_CELL_WIDTH {
                (30 * SECOND, TickMarkWeight::Second)
            } else if day_width_px > 1440.0 * Self::MIN_CELL_WIDTH {
                (MINUTE, TickMarkWeight::Minute1)
            } else if day_width_px > 288.0 * Self::MIN_CELL_WIDTH {
                (5 * MINUTE, TickMarkWeight::Minute5)
            } else if day_width_px > 48.0 * Self::MIN_CELL_WIDTH {
                (30 * MINUTE, TickMarkWeight::Minute30)
            } else if day_width_px > 24.0 * Self::MIN_CELL_WIDTH {
                (HOUR, TickMarkWeight::Hour)
            } else if day_width_px > 6.0 * Self::MIN_CELL_WIDTH {
                (4 * HOUR, TickMarkWeight::Hour4)
            } else {
                (12 * HOUR, TickMarkWeight::Hour4)
            };

            // Day boundaries for context (midnight UTC), same as the
            // uniform intraday path's `add_day_boundaries`, but without a
            // `bars`/`bar_interval` extrapolation dependency — walked
            // directly off the calendar.
            let (mut year, mut month, mut day, _, _, _) = timestamp_to_date(start_ts);
            let mut day_ts = date_to_timestamp(year, month, day, 0, 0, 0);
            if day_ts > start_ts {
                day -= 1;
                if day < 1 {
                    month -= 1;
                    if month < 1 { year -= 1; month = 12; }
                    day = days_in_month(year, month);
                }
                day_ts = date_to_timestamp(year, month, day, 0, 0, 0);
            }
            while day_ts <= end_ts {
                let w = if day == 1 { TickMarkWeight::Month } else { TickMarkWeight::Day };
                boundaries.push((day_ts, w));
                day += 1;
                if day > days_in_month(year, month) {
                    day = 1;
                    month += 1;
                    if month > 12 { month = 1; year += 1; }
                }
                day_ts = date_to_timestamp(year, month, day, 0, 0, 0);
            }

            // Intraday boundaries within the calendar day walk.
            // Skip ONLY the literal midnight second (already emitted as the
            // day boundary above). The previous `hour != 0` guard was too
            // broad and dropped every 00:05, 00:10 … 00:55 too, leaving the
            // first hour of every new day with no gridlines (visible on live
            // 15s series just after crossing midnight).
            const DAY_SEC: i64 = 86_400;
            let first_boundary = (start_ts / time_interval) * time_interval;
            let mut boundary_ts = first_boundary;
            while boundary_ts <= end_ts {
                if boundary_ts.rem_euclid(DAY_SEC) != 0 {
                    boundaries.push((boundary_ts, weight));
                }
                boundary_ts += time_interval;
            }
        } else {
            // Daily/monthly regime: same DAY_STEPS ladder as generate_daily_ticks.
            let mut step_days: i32 = 0;
            for &step in Self::DAY_STEPS.iter().rev() {
                let cell_width = day_width_px * step as f64;
                if (Self::MIN_CELL_WIDTH..=Self::MAX_CELL_WIDTH).contains(&cell_width) {
                    step_days = step;
                    break;
                }
            }
            if step_days == 0 {
                let max_step_width = day_width_px * 5.0;
                step_days = if max_step_width < Self::MIN_CELL_WIDTH { 0 } else { 5 };
            }

            if step_days == 0 {
                // Too zoomed out even for the 5-day step: months only.
                let (mut year, mut month, _, _, _, _) = timestamp_to_date(start_ts);
                loop {
                    let ts = date_to_timestamp(year, month, 1, 0, 0, 0);
                    if ts > end_ts { break; }
                    if ts >= start_ts - 31 * DAY {
                        boundaries.push((ts, TickMarkWeight::Month));
                    }
                    month += 1;
                    if month > 12 { month = 1; year += 1; }
                }
            } else {
                let (mut year, mut month, _, _, _, _) = timestamp_to_date(start_ts);
                month -= 1;
                if month < 1 { year -= 1; month = 12; }
                loop {
                    let month_start = date_to_timestamp(year, month, 1, 0, 0, 0);
                    if month_start > end_ts + 31 * DAY { break; }
                    if month_start >= start_ts - 31 * DAY {
                        let month_days = days_in_month(year, month);
                        let tick_days = Self::get_tick_days(month_days, step_days);
                        for &day in tick_days {
                            let ts = date_to_timestamp(year, month, day, 0, 0, 0);
                            if ts < start_ts - DAY || ts > end_ts + DAY { continue; }
                            let w = if day == 1 { TickMarkWeight::Month } else { TickMarkWeight::Day };
                            boundaries.push((ts, w));
                        }
                    }
                    month += 1;
                    if month > 12 { month = 1; year += 1; }
                }
            }
        }

        if boundaries.is_empty() {
            return;
        }

        // Resolve each boundary to a bar index — real bars in
        // `[start_bar, end_bar]` resolve via the clamped nearest-bar search
        // (byte-identical to the pre-fix behavior, pinned by existing
        // tests); boundaries beyond the last real bar's timestamp resolve
        // via forward extrapolation off `median_interval` instead of
        // clamping to `end_bar` (the future-region fix) — same fractional
        // step + rounding style (`floor`-toward-zero integer division) the
        // uniform kline path's `timestamp_to_bar_idx` uses for its own
        // forward-extrapolation branch. Then dedup consecutive boundaries
        // landing on the same bar index — keep the higher-weight tick. This
        // applies across the real/extrapolated boundary too: a real-region
        // boundary and an extrapolated one that round to the same index
        // still collapse to one tick.
        boundaries.sort_by_key(|&(ts, _)| ts);
        let last_ts = bars[end_bar].timestamp;
        let mut by_bar: Vec<(i64, i64, TickMarkWeight)> = Vec::with_capacity(boundaries.len());
        for (ts, weight) in boundaries {
            let idx: i64 = if ts > last_ts {
                end_bar as i64 + (ts - last_ts) / median_interval
            } else {
                Self::timestamp_to_bar_idx_nearest_clamped(ts, &bars[..=end_bar]) as i64
            };
            if idx < start_bar as i64 {
                continue;
            }
            match by_bar.last_mut() {
                Some(last) if last.0 == idx => {
                    // Same bar as the previous boundary — keep whichever
                    // carries the higher calendar weight (day > hour > minute).
                    if weight > last.2 {
                        last.1 = ts;
                        last.2 = weight;
                    }
                }
                _ => by_bar.push((idx, ts, weight)),
            }
        }

        let candidates: Vec<(i64, f64, TickMarkWeight, i64)> = by_bar
            .into_iter()
            .map(|(idx, ts, weight)| {
                let x = viewport.bar_to_x_f64(idx as f64);
                (idx, x, weight, ts)
            })
            .collect();

        self.select_ticks_with_spacing(ticks, candidates, viewport, measure_text, format_settings);
    }

    /// Generate intraday ticks (hours, minutes) for high zoom levels
    fn generate_intraday_ticks<F>(
        &self,
        ticks: &mut Vec<TimeTick>,
        start_ts: i64,
        end_ts: i64,
        bars: &[Bar],
        first_ts: i64,
        bar_interval: i64,
        viewport: &Viewport,
        day_width_px: f64,
        measure_text: &F,
        format_settings: Option<&TimeFormatSettings>,
    ) where
        F: Fn(&str) -> f64,
    {
        // Choose interval based on day width
        // day_width_px > 200 means we're zoomed in quite a bit
        // Sub-minute ladder first: sub-second / second-derived bars (1s/3s/5s/…)
        // zoom in far past 1-minute spacing. Without these, consecutive bars in
        // the same minute all collapse to one repeated "HH:MM" label.
        // Cell width = day_width_px * interval / DAY; thresholds keep it ≥ MIN_CELL_WIDTH.
        let (time_interval, weight) = if day_width_px > 86400.0 * Self::MIN_CELL_WIDTH {
            // Extreme zoom: 1-second ticks
            (SECOND, TickMarkWeight::Second)
        } else if day_width_px > 17280.0 * Self::MIN_CELL_WIDTH {
            // 5-second ticks (86400 / 5)
            (5 * SECOND, TickMarkWeight::Second)
        } else if day_width_px > 8640.0 * Self::MIN_CELL_WIDTH {
            // 10-second ticks (86400 / 10)
            (10 * SECOND, TickMarkWeight::Second)
        } else if day_width_px > 2880.0 * Self::MIN_CELL_WIDTH {
            // 30-second ticks (86400 / 30)
            (30 * SECOND, TickMarkWeight::Second)
        } else if day_width_px > 1440.0 * Self::MIN_CELL_WIDTH {
            // Show 1-minute (72000px+ per day)
            (MINUTE, TickMarkWeight::Minute1)
        } else if day_width_px > 288.0 * Self::MIN_CELL_WIDTH {
            // Very deep zoom: show 5-minute (14400px+ per day)
            (5 * MINUTE, TickMarkWeight::Minute5)
        } else if day_width_px > 48.0 * Self::MIN_CELL_WIDTH {
            // Deep zoom: show 30-minute (2400px+ per day)
            (30 * MINUTE, TickMarkWeight::Minute30)
        } else if day_width_px > 24.0 * Self::MIN_CELL_WIDTH {
            // Zoomed in: show hours (1200px+ per day)
            (HOUR, TickMarkWeight::Hour)
        } else if day_width_px > 6.0 * Self::MIN_CELL_WIDTH {
            // Moderately zoomed: show 4-hour (300px+ per day)
            (4 * HOUR, TickMarkWeight::Hour4)
        } else {
            // Less zoomed: show 12-hour
            (12 * HOUR, TickMarkWeight::Hour4)
        };

        // Generate boundaries
        let mut candidates: Vec<(i64, f64, TickMarkWeight, i64)> = Vec::new();

        // Also add day boundaries for context
        self.add_day_boundaries(&mut candidates, start_ts, end_ts, bars, first_ts, bar_interval, viewport);

        // Add intraday boundaries
        let first_boundary = (start_ts / time_interval) * time_interval;
        let mut boundary_ts = first_boundary;
        while boundary_ts <= end_ts {
            let bar_idx = Self::timestamp_to_bar_idx(boundary_ts, bars, first_ts, bar_interval);
            let x = viewport.bar_to_x_f64(bar_idx as f64);

            if x >= -100.0 && x <= viewport.chart_width + 100.0 {
                // Skip ONLY the literal midnight second (day boundary is
                // added separately by add_day_boundaries). The prior
                // `hour != 0` guard was too broad and dropped every
                // 00:05, 00:10 … 00:55 too — see calendar-grid path.
                const DAY_SEC_I: i64 = 86_400;
                if boundary_ts.rem_euclid(DAY_SEC_I) != 0 {
                    candidates.push((bar_idx, x, weight, boundary_ts));
                }
            }
            boundary_ts += time_interval;
        }

        // Select ticks with spacing
        self.select_ticks_with_spacing(ticks, candidates, viewport, measure_text, format_settings);
    }

    /// Generate daily ticks with uniform grid cells using lookup tables
    ///
    /// Algorithm:
    /// 1. Find all months in/near viewport
    /// 2. For each month, use the pre-computed tick table for (month_days, step)
    /// 3. Each month has uniform spacing with "cheat" cell at end (last_tick → 1st of next month)
    fn generate_daily_ticks<F>(
        &self,
        ticks: &mut Vec<TimeTick>,
        start_ts: i64,
        end_ts: i64,
        bars: &[Bar],
        first_ts: i64,
        bar_interval: i64,
        viewport: &Viewport,
        day_width_px: f64,
        measure_text: &F,
        format_settings: Option<&TimeFormatSettings>,
    ) where
        F: Fn(&str) -> f64,
    {
        let mut candidates: Vec<(i64, f64, TickMarkWeight, i64)> = Vec::new();

        // Find months in visible range (with buffer)
        let (mut year, mut month, _, _, _, _) = timestamp_to_date(start_ts);
        month -= 1;
        if month < 1 { year -= 1; month = 12; }

        let mut months_in_range: Vec<(i32, i32)> = Vec::new(); // (year, month)
        loop {
            let ts = date_to_timestamp(year, month, 1, 0, 0, 0);
            if ts > end_ts + 31 * DAY { break; }
            if ts >= start_ts - 31 * DAY {
                months_in_range.push((year, month));
            }
            month += 1;
            if month > 12 { month = 1; year += 1; }
        }

        // Count visible 1st-of-months (only within actual data range)
        let visible_month_count = months_in_range.iter().filter(|&&(y, m)| {
            let ts = date_to_timestamp(y, m, 1, 0, 0, 0);
            let bar_idx = Self::timestamp_to_bar_idx(ts, bars, first_ts, bar_interval);
            // Skip months before data starts
            if bar_idx < 0 { return false; }
            let x = viewport.bar_to_x_f64(bar_idx as f64);
            x >= 0.0 && x <= viewport.chart_width
        }).count();

        // If 3+ months visible, show only month labels
        if visible_month_count >= 3 {
            for &(y, m) in &months_in_range {
                let ts = date_to_timestamp(y, m, 1, 0, 0, 0);
                let bar_idx = Self::timestamp_to_bar_idx(ts, bars, first_ts, bar_interval);
                let x = viewport.bar_to_x_f64(bar_idx as f64);
                if x >= -50.0 && x <= viewport.chart_width + 50.0 {
                    candidates.push((bar_idx, x, TickMarkWeight::Month, ts));
                }
            }
            self.select_ticks_with_spacing(ticks, candidates, viewport, measure_text, format_settings);
            return;
        }

        // Choose optimal grid step (largest that fits)
        let mut step_days: i32 = 0;
        for &step in Self::DAY_STEPS.iter().rev() {
            let cell_width = day_width_px * step as f64;
            if (Self::MIN_CELL_WIDTH..=Self::MAX_CELL_WIDTH).contains(&cell_width) {
                step_days = step;
                break;
            }
        }

        // Fallback if no step fits perfectly
        if step_days == 0 {
            // If even step=5 is too small, show only months
            let max_step_width = day_width_px * 5.0;
            if max_step_width < Self::MIN_CELL_WIDTH {
                for &(y, m) in &months_in_range {
                    let ts = date_to_timestamp(y, m, 1, 0, 0, 0);
                    let bar_idx = Self::timestamp_to_bar_idx(ts, bars, first_ts, bar_interval);
                    let x = viewport.bar_to_x_f64(bar_idx as f64);
                    if x >= -50.0 && x <= viewport.chart_width + 50.0 {
                        candidates.push((bar_idx, x, TickMarkWeight::Month, ts));
                    }
                }
                self.select_ticks_with_spacing(ticks, candidates, viewport, measure_text, format_settings);
                return;
            }
            // Otherwise use largest step
            step_days = 5;
        }

        // Generate ticks for each month using lookup table
        // Add directly to ticks (no collision filtering - table already ensures proper spacing)
        for &(y, m) in &months_in_range {
            let month_days = days_in_month(y, m);
            let tick_days = Self::get_tick_days(month_days, step_days);

            for &day in tick_days {
                let ts = date_to_timestamp(y, m, day, 0, 0, 0);
                let bar_idx = Self::timestamp_to_bar_idx(ts, bars, first_ts, bar_interval);
                let x = viewport.bar_to_x_f64(bar_idx as f64);

                // Skip if far outside visible area
                if x < -50.0 || x > viewport.chart_width + 50.0 { continue; }

                // 1st of month gets Month weight, others get Day weight
                let weight = if day == 1 {
                    TickMarkWeight::Month
                } else {
                    TickMarkWeight::Day
                };

                let label = if let Some(settings) = format_settings {
                    format_time_by_weight_with_settings(ts, weight, settings)
                } else {
                    format_time_by_weight(ts, weight)
                };
                ticks.push(TimeTick {
                    bar_idx,
                    x,
                    weight,
                    label,
                });
            }
        }
    }

    /// Add day boundaries (midnight UTC)
    fn add_day_boundaries(
        &self,
        candidates: &mut Vec<(i64, f64, TickMarkWeight, i64)>,
        start_ts: i64,
        end_ts: i64,
        bars: &[Bar],
        first_ts: i64,
        bar_interval: i64,
        viewport: &Viewport,
    ) {
        let (mut year, mut month, mut day, _, _, _) = timestamp_to_date(start_ts);
        let mut ts = date_to_timestamp(year, month, day, 0, 0, 0);

        if ts > start_ts {
            day -= 1;
            if day < 1 {
                month -= 1;
                if month < 1 { year -= 1; month = 12; }
                day = days_in_month(year, month);
            }
            ts = date_to_timestamp(year, month, day, 0, 0, 0);
        }

        while ts <= end_ts {
            let bar_idx = Self::timestamp_to_bar_idx(ts, bars, first_ts, bar_interval);
            let x = viewport.bar_to_x_f64(bar_idx as f64);

            if x >= -100.0 && x <= viewport.chart_width + 100.0 {
                let weight = if day == 1 { TickMarkWeight::Month } else { TickMarkWeight::Day };
                candidates.push((bar_idx, x, weight, ts));
            }

            // Next day
            day += 1;
            if day > days_in_month(year, month) {
                day = 1;
                month += 1;
                if month > 12 { month = 1; year += 1; }
            }
            ts = date_to_timestamp(year, month, day, 0, 0, 0);
        }
    }

    /// Select ticks with proper spacing.
    ///
    /// Per LWC `_buildMarksImpl`: both the *left* and *right* index gap to the
    /// nearest already-accepted candidate must be `>= max_indexes_per_mark`
    /// where `max_indexes_per_mark = ceil(label_px / bar_spacing)`. Without
    /// the right-side check labels can crowd ahead of the cursor on
    /// non-uniform spacing.
    fn select_ticks_with_spacing<F>(
        &self,
        ticks: &mut Vec<TimeTick>,
        mut candidates: Vec<(i64, f64, TickMarkWeight, i64)>,
        viewport: &Viewport,
        measure_text: &F,
        format_settings: Option<&TimeFormatSettings>,
    ) where
        F: Fn(&str) -> f64,
    {
        if candidates.is_empty() {
            return;
        }

        // Sort by weight descending, then by position. Highest weight
        // candidates pre-empt the layout.
        candidates.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)));

        // LWC uses an estimate of the widest label as the index-gap basis.
        // We measure a representative full label and convert that to a
        // bar-count using bar_spacing. Falls back to the pixel-collision
        // check when bar_spacing is zero (defensive — should never happen
        // because viewport already guards on this).
        //
        // The sample MUST be sized from the widest weight tier actually in
        // play, not a hardcoded "00:00" (5 chars) — Second/LessThanSecond
        // tiers render "HH:MM:SS" (8 chars, see `format_time_by_weight`).
        // A hardcoded short sample undersized the index-gap for those tiers,
        // so adjacent range/tick/volume-bar ticks landed closer than their
        // rendered label was wide, producing visually-identical repeated
        // labels (2026-07-08 x-label-spacing-gate bug). Build one
        // representative label per distinct weight present and take the
        // widest — cheap, `candidates` is a handful of tick weights, not one
        // per candidate.
        let mut seen_weights: Vec<TickMarkWeight> = Vec::new();
        for c in &candidates {
            if !seen_weights.contains(&c.2) { seen_weights.push(c.2); }
        }
        let label_px = seen_weights.iter()
            .map(|&w| {
                let sample = if let Some(settings) = format_settings {
                    format_time_by_weight_with_settings(0, w, settings)
                } else {
                    format_time_by_weight(0, w)
                };
                measure_text(&sample)
            })
            .fold(0.0_f64, f64::max)
            .max(20.0);
        let bar_spacing = viewport.bar_spacing.max(1e-6);
        let max_indexes_per_mark = ((label_px / bar_spacing).ceil() as i64).max(1);

        // Track accepted bar indices sorted for bilateral lookup, alongside
        // each accepted tick's rendered label (same index into both Vecs) so
        // the adjacent-dedup check below can read a true visual neighbor's
        // label rather than whatever was accepted last in weight-major
        // iteration order.
        let mut accepted: Vec<i64> = Vec::new();
        let mut accepted_labels: Vec<String> = Vec::new();
        let mut used_positions: Vec<(f64, f64)> = Vec::new(); // (x, half_width)

        for (bar_idx, x, weight, ts) in candidates {
            // Skip if far outside visible area.
            if x < -50.0 || x > viewport.chart_width + 50.0 {
                continue;
            }

            // Bilateral index-gap check.
            let left_idx = accepted.iter().rposition(|&i| i < bar_idx);
            let right_idx = accepted.iter().position(|&i| i > bar_idx);
            let left_gap = left_idx.map(|i| bar_idx - accepted[i]);
            let right_gap = right_idx.map(|i| accepted[i] - bar_idx);

            if let Some(g) = left_gap {
                if g < max_indexes_per_mark { continue; }
            }
            if let Some(g) = right_gap {
                if g < max_indexes_per_mark { continue; }
            }

            // Format label.
            let label = if let Some(settings) = format_settings {
                format_time_by_weight_with_settings(ts, weight, settings)
            } else {
                format_time_by_weight(ts, weight)
            };

            // Adjacent-dedup safety net: the index-gap + pixel-collision
            // checks above are geometric (position-based) and can still
            // accept two candidates whose FORMATTED label is identical
            // (e.g. two Second-weight ticks that round to the same
            // "HH:MM:SS" string but sit far enough apart in bar_idx/x) —
            // skip a tick whose rendered label equals its nearest already-
            // accepted NEIGHBOR's label (left or right, whichever exists).
            let dup_of_neighbor = left_idx.map_or(false, |i| accepted_labels[i] == label)
                || right_idx.map_or(false, |i| accepted_labels[i] == label);
            if dup_of_neighbor {
                continue;
            }

            let measured_width = measure_text(&label);
            let half_width = measured_width / 2.0 + 5.0;

            // Pixel-based safety net (handles unequal label widths).
            let conflicts = used_positions
                .iter()
                .any(|(ox, ohw)| (x - ox).abs() < half_width + ohw + 10.0);
            if conflicts {
                continue;
            }

            used_positions.push((x, half_width));

            // Maintain accepted/accepted_labels sorted in lockstep by bar_idx.
            let pos = accepted.binary_search(&bar_idx).unwrap_or_else(|p| p);
            accepted.insert(pos, bar_idx);
            accepted_labels.insert(pos, label.clone());

            ticks.push(TimeTick {
                bar_idx,
                x,
                weight,
                label,
            });
        }
    }
}

// =============================================================================
// Time Formatting Functions
// =============================================================================

/// Format time label based on weight (uses current language)
pub fn format_time_by_weight(ts: i64, weight: TickMarkWeight) -> String {
    let (year, month, day, hour, minute, second) = timestamp_to_date(ts);
    let month_names = month_names_short(current_language());

    match weight {
        TickMarkWeight::Year => format!("{}", year),
        TickMarkWeight::Month => {
            month_names[(month - 1) as usize].to_string()
        }
        TickMarkWeight::Day => {
            format!("{} {}", day, month_names[(month - 1) as usize])
        }
        TickMarkWeight::Hour12
        | TickMarkWeight::Hour6
        | TickMarkWeight::Hour4
        | TickMarkWeight::Hour3
        | TickMarkWeight::Hour => {
            format!("{:02}:{:02}", hour, minute)
        }
        TickMarkWeight::Minute30
        | TickMarkWeight::Minute5
        | TickMarkWeight::Minute1 => {
            format!("{:02}:{:02}", hour, minute)
        }
        // Sub-minute zoom (sub-second / second-derived bars): include seconds so
        // consecutive bars within the same minute get distinct labels.
        TickMarkWeight::Second => format!("{:02}:{:02}:{:02}", hour, minute, second),
        TickMarkWeight::LessThanSecond => {
            let ms = (ts.rem_euclid(1)).abs(); // ts is seconds, ms is decorative
            let _ = ms;
            format!("{:02}:{:02}:{:02}", hour, minute, second)
        }
    }
}

/// Format full timestamp for crosshair display
pub fn format_time_full(ts: i64) -> String {
    let (_, month, day, hour, minute, _) = timestamp_to_date(ts);
    format!("{:02}.{:02} {:02}:{:02}", day, month, hour, minute)
}

/// Format time label with custom settings
pub fn format_time_by_weight_with_settings(
    ts: i64,
    weight: TickMarkWeight,
    settings: &TimeFormatSettings
) -> String {
    let (year, month, day, hour, minute, second) = timestamp_to_date(ts);
    let month_names = month_names_short(current_language());

    // 12h hour + meridiem, shared by the minute- and second-resolution formats.
    let (h12, ampm) = if hour == 0 {
        (12, "AM")
    } else if hour < 12 {
        (hour, "AM")
    } else if hour == 12 {
        (12, "PM")
    } else {
        (hour - 12, "PM")
    };

    // Format time part based on use_24h setting
    let time_str = if settings.use_24h {
        format!("{:02}:{:02}", hour, minute)
    } else {
        format!("{}:{:02} {}", h12, minute, ampm)
    };

    // Second-resolution variant (sub-minute zoom).
    let time_str_secs = if settings.use_24h {
        format!("{:02}:{:02}:{:02}", hour, minute, second)
    } else {
        format!("{}:{:02}:{:02} {}", h12, minute, second, ampm)
    };

    // Day of week prefix (if enabled)
    let dow_prefix = if settings.show_day_of_week {
        // Calculate day of week from timestamp
        let dow = day_of_week_from_timestamp(ts);
        let dow_names = ["вс", "пн", "вт", "ср", "чт", "пт", "сб"];
        format!("{} ", dow_names[dow as usize])
    } else {
        String::new()
    };

    match weight {
        TickMarkWeight::Year => format!("{}", year),
        TickMarkWeight::Month => {
            month_names[(month - 1) as usize].to_string()
        }
        TickMarkWeight::Day => {
            // Format date based on date_format setting
            match settings.date_format {
                DateFormat::DayMonthYear => format!("{}{}.{:02}", dow_prefix, day, month),
                DateFormat::MonthDayYear => format!("{}{:02}/{}", dow_prefix, month, day),
                DateFormat::YearMonthDay => format!("{}{}-{:02}-{:02}", dow_prefix, year, month, day),
                DateFormat::DayMonthShort => format!("{}{} {}", dow_prefix, day, month_names[(month - 1) as usize]),
            }
        }
        TickMarkWeight::Hour12 | TickMarkWeight::Hour6 |
        TickMarkWeight::Hour4 | TickMarkWeight::Hour3 |
        TickMarkWeight::Hour | TickMarkWeight::Minute30 |
        TickMarkWeight::Minute5 | TickMarkWeight::Minute1 => {
            time_str
        }
        TickMarkWeight::Second | TickMarkWeight::LessThanSecond => time_str_secs,
    }
}

/// Helper: calculate day of week from unix timestamp (0=Sunday, 6=Saturday)
fn day_of_week_from_timestamp(ts: i64) -> u8 {
    // Unix epoch (1970-01-01) was Thursday (4)
    let days = ts / 86400;
    ((days + 4) % 7) as u8
}

/// Format full timestamp for crosshair with custom settings
pub fn format_time_full_with_settings(ts: i64, settings: &TimeFormatSettings) -> String {
    let (year, month, day, hour, minute, _) = timestamp_to_date(ts);

    let time_str = if settings.use_24h {
        format!("{:02}:{:02}", hour, minute)
    } else {
        let (h12, ampm) = if hour == 0 {
            (12, "AM")
        } else if hour < 12 {
            (hour, "AM")
        } else if hour == 12 {
            (12, "PM")
        } else {
            (hour - 12, "PM")
        };
        format!("{}:{:02} {}", h12, minute, ampm)
    };

    let date_str = match settings.date_format {
        DateFormat::DayMonthYear => format!("{:02}.{:02}", day, month),
        DateFormat::MonthDayYear => format!("{:02}/{:02}", month, day),
        DateFormat::YearMonthDay => format!("{}-{:02}-{:02}", year, month, day),
        DateFormat::DayMonthShort => {
            let month_names = month_names_short(current_language());
            format!("{} {}", day, month_names[(month - 1) as usize])
        }
    };

    format!("{} {}", date_str, time_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tick_weight_ordering() {
        assert!(TickMarkWeight::Year > TickMarkWeight::Month);
        assert!(TickMarkWeight::Month > TickMarkWeight::Day);
        assert!(TickMarkWeight::Day > TickMarkWeight::Hour);
        assert!(TickMarkWeight::Hour > TickMarkWeight::Minute1);
    }

    #[test]
    fn test_calendar_accuracy() {
        // Test Feb 28 -> Mar 1 in a non-leap year
        let feb_28_2023 = date_to_timestamp(2023, 2, 28, 12, 0, 0);
        let mar_1_2023 = date_to_timestamp(2023, 3, 1, 12, 0, 0);

        let (y1, m1, d1, _, _, _) = timestamp_to_date(feb_28_2023);
        assert_eq!((y1, m1, d1), (2023, 2, 28));

        let (y2, m2, d2, _, _, _) = timestamp_to_date(mar_1_2023);
        assert_eq!((y2, m2, d2), (2023, 3, 1));
    }

    #[test]
    fn test_leap_year() {
        // Feb 29, 2024 exists (2024 is a leap year)
        let feb_29_2024 = date_to_timestamp(2024, 2, 29, 0, 0, 0);
        let (y, m, d, _, _, _) = timestamp_to_date(feb_29_2024);
        assert_eq!((y, m, d), (2024, 2, 29));
    }

    #[test]
    fn test_format_time_by_weight() {
        let ts = 1699920000_i64; // Some timestamp
        let label = format_time_by_weight(ts, TickMarkWeight::Hour);
        assert!(label.contains(':'));
    }

    #[test]
    fn test_format_second_weight_includes_seconds() {
        // 2023-01-02 03:04:05 UTC — Second weight must surface HH:MM:SS so
        // consecutive sub-minute bars get distinct labels; Minute weight stays HH:MM.
        let ts = date_to_timestamp(2023, 1, 2, 3, 4, 5);
        assert_eq!(format_time_by_weight(ts, TickMarkWeight::Second), "03:04:05");
        assert_eq!(format_time_by_weight(ts, TickMarkWeight::Minute1), "03:04");

        let settings = TimeFormatSettings { use_24h: true, ..Default::default() };
        assert_eq!(
            format_time_by_weight_with_settings(ts, TickMarkWeight::Second, &settings),
            "03:04:05"
        );
        assert_eq!(
            format_time_by_weight_with_settings(ts, TickMarkWeight::Minute1, &settings),
            "03:04"
        );
    }

    #[test]
    fn test_weight_classification() {
        assert!(TickMarkWeight::Year.is_major());
        assert!(TickMarkWeight::Month.is_major());
        assert!(!TickMarkWeight::Day.is_major());

        assert!(TickMarkWeight::Day.is_medium());
        assert!(!TickMarkWeight::Hour.is_medium());
    }

    // =========================================================================
    // Calendar auto-grid tests (owner two-axis model, step 1)
    // =========================================================================

    /// Wide fixed-width label measurer — keeps the bilateral-gap picker from
    /// rejecting candidates purely because a test's synthetic zoom level
    /// makes `bar_spacing` tiny relative to a real font metric. Individual
    /// tests still assert on real spacing/dedup behavior, not on how many
    /// ticks survive the picker's pixel-collision check.
    fn narrow_measure(_s: &str) -> f64 {
        20.0
    }

    fn test_viewport(bar_count: usize, bar_spacing: f64, chart_width: f64) -> Viewport {
        Viewport {
            view_start: 0.0,
            bar_spacing,
            bar_width_ratio: 0.8,
            chart_width,
            chart_height: 400.0,
            bar_count,
        }
    }

    /// Build a synthetic non-uniform series: a 5-minute base cadence with
    /// periodic bursts (3 bars 10s apart every 10th bar) and one large gap
    /// (2 hours) near the tail — triggers `is_non_uniform` (max gap > 2x
    /// median over the last-100-gap sample window) and exercises both the
    /// burst-density and gap-collapse paths of the calendar generator in one
    /// series. Kept under 100 total bars so every feature (bursts + the
    /// gap) falls inside `is_non_uniform`'s last-100-gap sample window
    /// regardless of how the burst injection shifts total bar count.
    fn synthetic_non_uniform_bars() -> Vec<Bar> {
        let mut bars = Vec::new();
        let mut ts = date_to_timestamp(2024, 3, 1, 0, 0, 0);
        for i in 0..60 {
            bars.push(Bar::new(ts, 0.0, 0.0, 0.0, 0.0));
            if i == 45 {
                // One large gap: 2 hours instead of 5 minutes.
                ts += 2 * HOUR;
            } else if i % 10 == 0 && i > 0 {
                // Burst: two extra bars 10s apart right after this one.
                ts += 10;
                bars.push(Bar::new(ts, 0.0, 0.0, 0.0, 0.0));
                ts += 10;
                bars.push(Bar::new(ts, 0.0, 0.0, 0.0, 0.0));
                ts += 5 * MINUTE;
            } else {
                ts += 5 * MINUTE;
            }
        }
        bars
    }

    #[test]
    fn test_calendar_grid_non_uniform_ticks_on_round_boundaries() {
        let bars = synthetic_non_uniform_bars();
        assert!(is_non_uniform(&bars), "fixture must actually be non-uniform");

        let viewport = test_viewport(bars.len(), 8.0, 1200.0);
        let scale = TimeScale::new();
        let ticks = scale.generate_ticks(&viewport, &bars, narrow_measure, None, false);

        assert!(!ticks.is_empty(), "calendar generator must produce ticks for a non-uniform series");

        for tick in &ticks {
            // A tick can now reference a forward-extrapolated index beyond
            // the last real bar (future-region extension, 2026-07-09) — only
            // reject indices before the first real bar. Every tick must
            // still land exactly on its bar index's x position — calendar
            // boundaries are resolved to the nearest (or extrapolated) bar,
            // never a synthetic fractional position.
            assert!(tick.bar_idx >= 0, "tick.bar_idx must not be negative for this fixture");
            assert_eq!(tick.x, viewport.bar_to_x_f64(tick.bar_idx as f64));
        }

        // No two ticks may share a bar_idx (dedup invariant).
        let mut seen: Vec<i64> = ticks.iter().map(|t| t.bar_idx).collect();
        let before = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), before, "duplicate bar_idx across ticks — dedup failed");
    }

    #[test]
    fn test_calendar_grid_dedup_keeps_higher_weight_on_gap_collapse() {
        // Two bars separated by a gap wide enough that several calendar
        // day-boundaries fall strictly between them with nothing to attach
        // to except these two bars (plus, since the future-region extension
        // 2026-07-09, the extrapolated tail past bar 1) — every boundary
        // must collapse onto a real or forward-extrapolated bar index,
        // deduped to one tick per index, and the survivor at each index
        // must carry the highest-weight boundary that collapsed onto it
        // (Day/Month over lower intraday weights).
        let start = date_to_timestamp(2024, 1, 1, 0, 0, 0);
        let end = date_to_timestamp(2024, 1, 10, 0, 0, 0); // 9-day gap
        let bars = vec![
            Bar::new(start, 0.0, 0.0, 0.0, 0.0),
            Bar::new(end, 0.0, 0.0, 0.0, 0.0),
        ];

        let viewport = test_viewport(bars.len(), 40.0, 1200.0);
        let scale = TimeScale::new();
        let mut ticks = Vec::new();
        scale.generate_calendar_grid_ticks(&mut ticks, &bars, &viewport, &narrow_measure, None);

        // Every emitted tick must reference bar 0, bar 1, or a
        // forward-extrapolated index beyond bar 1 (never before bar 0).
        for t in &ticks {
            assert!(t.bar_idx >= 0, "boundary must not resolve before the first real bar, got {}", t.bar_idx);
        }
        // At most one tick per bar index (dedup) — check the two real bars
        // specifically, same as before this fix.
        let idx0_count = ticks.iter().filter(|t| t.bar_idx == 0).count();
        let idx1_count = ticks.iter().filter(|t| t.bar_idx == 1).count();
        assert!(idx0_count <= 1);
        assert!(idx1_count <= 1);
        // Dedup invariant holds globally too — no bar_idx repeats.
        let mut seen: Vec<i64> = ticks.iter().map(|t| t.bar_idx).collect();
        let before = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), before, "duplicate bar_idx across ticks — dedup failed");
    }

    #[test]
    fn test_calendar_grid_backward_still_clamped_forward_extrapolates() {
        // A short series where the visible viewport pad (±50 bars) sits
        // mostly BEFORE the first real bar (view_start = 0.0, so the
        // backward pad clamps to `start_bar = 0`) and, since the
        // future-region extension (2026-07-09), legitimately extends past
        // the last real bar on the forward side (`end_bar_vp = ceil(0 +
        // visible_bars) + 50` comfortably exceeds this 5-bar fixture).
        // Backward indices must still never go negative; forward indices
        // are now allowed — and expected — to exceed `bars.len() - 1`.
        let mut bars = Vec::new();
        let mut ts = date_to_timestamp(2024, 6, 15, 8, 0, 0);
        for _ in 0..5 {
            bars.push(Bar::new(ts, 0.0, 0.0, 0.0, 0.0));
            ts += MINUTE;
        }

        let viewport = test_viewport(bars.len(), 8.0, 1200.0);
        let scale = TimeScale::new();
        let mut ticks = Vec::new();
        scale.generate_calendar_grid_ticks(&mut ticks, &bars, &viewport, &narrow_measure, None);

        for t in &ticks {
            assert!(t.bar_idx >= 0, "backward indices must still clamp at 0, got {}", t.bar_idx);
        }
        assert!(
            ticks.iter().any(|t| (t.bar_idx as usize) >= bars.len()),
            "viewport pad exceeds this fixture's length — expected at least one \
             forward-extrapolated tick beyond the last real bar, got {:?}",
            ticks.iter().map(|t| t.bar_idx).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_calendar_grid_uniform_kline_path_unchanged() {
        // Uniform 1-hour kline series must still dispatch through
        // `generate_intraday_ticks`/`generate_daily_ticks` (the
        // `is_non_uniform` gate must NOT fire for a regular cadence), and
        // must still emit Hour-weight ticks with "HH:MM"-shaped labels —
        // pinning the pre-existing uniform-path behavior byte-identical.
        let mut bars = Vec::new();
        let mut ts = date_to_timestamp(2024, 5, 1, 0, 0, 0);
        for _ in 0..300 {
            bars.push(Bar::new(ts, 0.0, 0.0, 0.0, 0.0));
            ts += HOUR;
        }
        assert!(!is_non_uniform(&bars), "uniform hourly series must not be flagged non-uniform");

        // Zoom level chosen so day_width_px > 4*MIN_CELL_WIDTH (intraday
        // mode) — bars_per_day = 24, so bar_spacing=10 -> day_width_px=240 > 200.
        let viewport = test_viewport(bars.len(), 10.0, 1200.0);
        let scale = TimeScale::new();
        let ticks = scale.generate_ticks(&viewport, &bars, narrow_measure, None, false);

        assert!(!ticks.is_empty(), "uniform kline path must still produce ticks");
        assert!(
            ticks.iter().any(|t| t.weight == TickMarkWeight::Hour || t.weight == TickMarkWeight::Day || t.weight == TickMarkWeight::Month),
            "expected calendar-weighted ticks from the uniform intraday/daily dispatch"
        );
        for t in &ticks {
            if matches!(t.weight, TickMarkWeight::Hour) {
                assert!(t.label.contains(':'), "Hour-weight label must be HH:MM shaped, got {:?}", t.label);
            }
        }
    }

    #[test]
    fn test_calendar_grid_uniform_daily_step_pins_month_and_day_weights() {
        // Zoomed far out on a uniform daily-kline series — must still route
        // through `generate_daily_ticks` (day_width_px small) and produce
        // Month/Day-weighted ticks, same as before this change.
        let mut bars = Vec::new();
        let mut ts = date_to_timestamp(2023, 1, 1, 0, 0, 0);
        for _ in 0..400 {
            bars.push(Bar::new(ts, 0.0, 0.0, 0.0, 0.0));
            ts += DAY;
        }
        let viewport = test_viewport(bars.len(), 1.0, 800.0);
        let scale = TimeScale::new();
        let ticks = scale.generate_ticks(&viewport, &bars, narrow_measure, None, false);

        assert!(!ticks.is_empty());
        assert!(
            ticks.iter().all(|t| matches!(t.weight, TickMarkWeight::Month | TickMarkWeight::Day | TickMarkWeight::Year)),
            "zoomed-out daily kline series must only emit calendar-scale weights, got {:?}",
            ticks.iter().map(|t| t.weight).collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_calendar_grid_renko_shadow_gets_real_calendar_ticks() {
        // Mirrors `active_series::to_bar_shadow()`'s renko/pnf/kagi/3lb
        // output: OHLC-zeroed `Bar`s whose `timestamp` is each brick's real
        // open time, at irregular (price-driven) intervals. Confirms the
        // renko-family finding: since the shadow carries genuine
        // timestamps, `is_non_temporal=true` now routes through the same
        // calendar generator instead of the old blank-strip suppression.
        let mut bars = Vec::new();
        let mut ts = date_to_timestamp(2024, 4, 10, 6, 0, 0);
        // Irregular brick formation times: price action speeds up and slows
        // down, so gaps between bricks vary widely (30s to 40min).
        let gaps = [30, 45, 300, 1800, 60, 2400, 90, 120, 3000, 15, 600, 2100];
        for (i, g) in gaps.iter().enumerate() {
            bars.push(Bar::new(ts, 0.0, 0.0, 0.0, 0.0));
            ts += *g;
            let _ = i;
        }
        bars.push(Bar::new(ts, 0.0, 0.0, 0.0, 0.0));

        let viewport = test_viewport(bars.len(), 12.0, 1200.0);
        let scale = TimeScale::new();
        let ticks = scale.generate_ticks(&viewport, &bars, narrow_measure, None, true);

        assert!(!ticks.is_empty(), "renko-shadow series with real timestamps must get calendar ticks, not a blank strip");
        for t in &ticks {
            // Indices beyond `bars.len() - 1` are legitimate
            // forward-extrapolated future-region ticks (2026-07-09) — only
            // reject a negative index. `x` must still match the same index's
            // linear position regardless of real-vs-extrapolated.
            assert!(t.bar_idx >= 0);
            assert_eq!(t.x, viewport.bar_to_x_f64(t.bar_idx as f64));
        }
    }

    #[test]
    fn test_calendar_grid_degenerate_shadow_falls_back_to_per_bar() {
        // A 2-element renko shadow has no usable calendar step (median gap
        // needs >=2 samples in the visible slice) — the calendar generator
        // must yield nothing and `generate_ticks` must fall back to the
        // per-bar picker so the strip is never left blank.
        let bars = vec![
            Bar::new(1_700_000_000, 0.0, 0.0, 0.0, 0.0),
            Bar::new(1_700_000_600, 0.0, 0.0, 0.0, 0.0),
        ];
        let viewport = test_viewport(bars.len(), 12.0, 1200.0);
        let scale = TimeScale::new();
        let ticks = scale.generate_ticks(&viewport, &bars, narrow_measure, None, true);
        assert!(!ticks.is_empty(), "degenerate shadow must still fall back to per-bar ticks, not blank");
    }

    // =========================================================================
    // Future-region extension + ts-shadow-units regression (2026-07-09)
    // =========================================================================

    #[test]
    fn test_calendar_grid_extends_past_last_bar_when_viewport_looks_forward() {
        // Viewport panned so `view_start` sits near the end of the data —
        // `end_bar_vp = ceil(view_start + visible_bars) + 50` reaches well
        // past the last real bar. The generator must produce calendar
        // ticks with `bar_idx > last real index`, at median-spaced calendar
        // boundaries extrapolated forward from the last real bar — mirrors
        // the uniform kline path's off-screen future padding.
        let mut bars = Vec::new();
        let mut ts = date_to_timestamp(2024, 3, 1, 0, 0, 0);
        for _ in 0..80 {
            bars.push(Bar::new(ts, 0.0, 0.0, 0.0, 0.0));
            ts += HOUR;
        }
        let last_real_idx = bars.len() - 1;

        // Pan the viewport so view_start is near the tail: visible_bars ≈
        // chart_width/bar_spacing = 1200/8 = 150, so view_start = 60 pushes
        // the ±50 pad well past `last_real_idx` (79).
        let mut viewport = test_viewport(bars.len(), 8.0, 1200.0);
        viewport.view_start = 60.0;

        let scale = TimeScale::new();
        let mut ticks = Vec::new();
        scale.generate_calendar_grid_ticks(&mut ticks, &bars, &viewport, &narrow_measure, None);

        assert!(!ticks.is_empty(), "calendar generator must still produce ticks when panned near the tail");
        assert!(
            ticks.iter().any(|t| (t.bar_idx as usize) > last_real_idx),
            "expected at least one tick past the last real bar (idx > {}), got {:?}",
            last_real_idx,
            ticks.iter().map(|t| t.bar_idx).collect::<Vec<_>>()
        );
        // Every future-region tick's x must still be the plain linear
        // projection from its (possibly beyond-length) index — no clamping.
        for t in &ticks {
            assert_eq!(t.x, viewport.bar_to_x_f64(t.bar_idx as f64));
        }
        // Dedup invariant still holds in the extended region.
        let mut seen: Vec<i64> = ticks.iter().map(|t| t.bar_idx).collect();
        let before = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), before, "duplicate bar_idx across ticks — dedup failed in future region");
    }

    #[test]
    fn test_calendar_grid_seconds_scale_shadow_gets_intraday_step() {
        // Regression for the ts-shadow unit-corruption bug (FIX A,
        // 2026-07-09): before the fix, `to_bar_shadow()` wrote a
        // millisecond `open_time` verbatim into `Bar.timestamp` (a SECONDS
        // field), inflating a 1-2 minute real cadence into an apparent
        // ~1970-01-18 gap of ~1000x, which `generate_calendar_grid_ticks`'s
        // `median_interval`-driven `day_width_px` measure would then
        // misclassify as a multi-day/month cadence (DAY_STEPS regime)
        // instead of intraday. Pin the CORRECT contract here: a bars array
        // with genuinely seconds-scale timestamps at a 60-120s cadence must
        // route through the intraday regime (Minute-family weights), never
        // the DAY_STEPS regime (Month/Day weights only), when the viewport
        // is zoomed in tight enough to resolve minute-level ticks.
        // Start well away from midnight and span several hours so the
        // single Day/Month day-boundary tick (highest weight, picked first
        // by `select_ticks_with_spacing`) doesn't index-collide with every
        // Minute-weight candidate — a short few-bar fixture starting at
        // midnight puts the lone Day boundary at bar_idx 0, which swallows
        // every nearby Minute tick via the bilateral index-gap check.
        let mut bars = Vec::new();
        let mut ts = date_to_timestamp(2024, 3, 1, 6, 0, 0);
        let gap_pattern = [60, 120, 90, 60, 60, 120, 90, 60, 120, 60];
        for i in 0..200 {
            bars.push(Bar::new(ts, 0.0, 0.0, 0.0, 0.0));
            ts += gap_pattern[i % gap_pattern.len()];
        }

        // Tight zoom (large bar_spacing) so day_width_px clears the
        // intraday threshold for a ~90s median cadence:
        // bars_per_day = 86400/90 = 960, day_width_px = 960 * 40 = 38400,
        // well past 4*MIN_CELL_WIDTH (200).
        let viewport = test_viewport(bars.len(), 40.0, 1200.0);
        let scale = TimeScale::new();
        let mut ticks = Vec::new();
        scale.generate_calendar_grid_ticks(&mut ticks, &bars, &viewport, &narrow_measure, None);

        assert!(!ticks.is_empty(), "seconds-scale shadow must produce ticks");
        assert!(
            ticks.iter().any(|t| matches!(
                t.weight,
                TickMarkWeight::Second | TickMarkWeight::Minute1 | TickMarkWeight::Minute5 | TickMarkWeight::Minute30 | TickMarkWeight::Hour | TickMarkWeight::Hour4
            )),
            "seconds-scale 1-2min-gap bars must get an INTRADAY step, got weights {:?}",
            ticks.iter().map(|t| t.weight).collect::<Vec<_>>()
        );
        assert!(
            !ticks.iter().all(|t| matches!(t.weight, TickMarkWeight::Day | TickMarkWeight::Month | TickMarkWeight::Year)),
            "seconds-scale shadow must NOT be misclassified into the DAY_STEPS regime, got weights {:?}",
            ticks.iter().map(|t| t.weight).collect::<Vec<_>>()
        );
    }

    // =========================================================================
    // DEBUG REPRO — live 15s trade-built series crossing midnight
    // (owner-reported: gridlines/labels stop dead at the "18.07" day-boundary
    // tick, ~10 min of real bars render to its right with zero ticks).
    // =========================================================================

    #[test]
    fn regress_intraday_ticks_survive_midnight_uniform() {
        // ~40 min at uniform 15s cadence, 23:20 -> 00:10 UTC, crossing
        // midnight July 17 -> 18. No gap yet — pins whether the UNIFORM
        // intraday path alone reproduces the symptom.
        let mut bars = Vec::new();
        let mut ts = date_to_timestamp(2026, 7, 17, 23, 20, 0);
        let end_ts = date_to_timestamp(2026, 7, 18, 0, 10, 0);
        while ts <= end_ts {
            bars.push(Bar::new(ts, 0.0, 0.0, 0.0, 0.0));
            ts += 15;
        }
        eprintln!("bars: {} first={} last={}", bars.len(), bars[0].timestamp, bars[bars.len() - 1].timestamp);
        assert!(!is_non_uniform(&bars), "uniform 15s series must not trip the non-uniform gate");

        // bar_spacing chosen so the 5-min tick step is selected:
        // day_width_px = (86400/15)*bar_spacing = 5760*bar_spacing.
        // Need 288*MIN_CELL_WIDTH(50) < day_width_px <= 1440*MIN_CELL_WIDTH
        // i.e. 14400 < 5760*bs <= 72000 => 2.5 < bs <= 12.5. Use bs=8.0.
        // Wide viewport so all ~200 bars sit inside the clip window
        // (matches the owner's live-follow chart state where 00:05-00:10
        // bars are visibly rendered to the right of the day-boundary
        // label). At bar_spacing=8 and bar_count=200, chart_width=1800px
        // clips to bar_idx ≤ 225 comfortably.
        let mut viewport = test_viewport(bars.len(), 8.0, 1800.0);
        viewport.view_start = 0.0;

        let scale = TimeScale::new();
        let ticks = scale.generate_ticks(&viewport, &bars, narrow_measure, None, false);

        let midnight_ts = date_to_timestamp(2026, 7, 18, 0, 0, 0);
        let post_midnight: Vec<&TimeTick> = ticks.iter().filter(|t| {
            // Recover ts from bar_idx via the same bar_idx_to_timestamp the
            // generator used — bars array is uniform so timestamp = first + idx*15.
            let approx_ts = bars[0].timestamp + t.bar_idx * 15;
            approx_ts > midnight_ts
        }).collect();

        eprintln!(
            "total ticks: {}, post-midnight ticks: {}",
            ticks.len(),
            post_midnight.len()
        );
        for t in &ticks {
            let approx_ts = bars[0].timestamp + t.bar_idx * 15;
            eprintln!(
                "  tick bar_idx={} x={:.1} weight={:?} label={:?} approx_ts={}",
                t.bar_idx, t.x, t.weight, t.label, approx_ts
            );
        }

        assert!(
            !post_midnight.is_empty(),
            "expected at least one tick with ts > midnight (e.g. 00:05), got NONE — reproduces the bug. \
             All ticks: {:?}",
            ticks.iter().map(|t| (t.bar_idx, t.weight, t.label.clone())).collect::<Vec<_>>()
        );
    }

    #[test]
    fn regress_intraday_ticks_survive_midnight_non_uniform() {
        // Same as above but with a realistic trade-built session gap
        // (e.g. venue reconnect / quiet period) shortly before midnight —
        // triggers `is_non_uniform` and routes through
        // `generate_calendar_grid_ticks` instead of the uniform intraday path.
        let mut bars = Vec::new();
        let mut ts = date_to_timestamp(2026, 7, 17, 23, 20, 0);
        let gap_before = date_to_timestamp(2026, 7, 17, 23, 55, 0);
        let end_ts = date_to_timestamp(2026, 7, 18, 0, 10, 0);
        while ts <= end_ts {
            bars.push(Bar::new(ts, 0.0, 0.0, 0.0, 0.0));
            if ts == gap_before {
                // 3-minute session gap (12x the 15s median) right before midnight.
                ts += 3 * MINUTE;
            } else {
                ts += 15;
            }
        }
        eprintln!("bars: {} first={} last={}", bars.len(), bars[0].timestamp, bars[bars.len() - 1].timestamp);
        assert!(is_non_uniform(&bars), "session-gap fixture must trip the non-uniform gate");

        // Wide viewport (same rationale as the uniform test above).
        let mut viewport = test_viewport(bars.len(), 8.0, 1800.0);
        viewport.view_start = 0.0;

        let scale = TimeScale::new();
        let ticks = scale.generate_ticks(&viewport, &bars, narrow_measure, None, false);

        let midnight_ts = date_to_timestamp(2026, 7, 18, 0, 0, 0);
        let post_midnight: Vec<&TimeTick> = ticks.iter().filter(|t| {
            let idx = t.bar_idx as usize;
            let approx_ts = if idx < bars.len() {
                bars[idx].timestamp
            } else {
                bars[bars.len() - 1].timestamp + (t.bar_idx - (bars.len() as i64 - 1)) * 15
            };
            approx_ts > midnight_ts
        }).collect();

        eprintln!(
            "total ticks: {}, post-midnight ticks: {}",
            ticks.len(),
            post_midnight.len()
        );
        for t in &ticks {
            eprintln!(
                "  tick bar_idx={} x={:.1} weight={:?} label={:?}",
                t.bar_idx, t.x, t.weight, t.label
            );
        }

        assert!(
            !post_midnight.is_empty(),
            "non-uniform (gap-before-midnight) path: expected at least one tick with ts > midnight, got NONE. \
             All ticks: {:?}",
            ticks.iter().map(|t| (t.bar_idx, t.weight, t.label.clone())).collect::<Vec<_>>()
        );
    }

    #[test]
    fn regress_intraday_ticks_survive_midnight_panned() {
        // View panned so the last visible bars sit near the right edge
        // (typical live-follow chart state) instead of view_start=0 showing
        // the whole 40min history in one screen.
        let mut bars = Vec::new();
        let mut ts = date_to_timestamp(2026, 7, 17, 23, 20, 0);
        let end_ts = date_to_timestamp(2026, 7, 18, 0, 10, 0);
        while ts <= end_ts {
            bars.push(Bar::new(ts, 0.0, 0.0, 0.0, 0.0));
            ts += 15;
        }
        let last_idx = bars.len() - 1;

        let mut viewport = test_viewport(bars.len(), 8.0, 1200.0);
        // visible_bars = 1200/8 = 150; pan so the tail sits near the right edge.
        viewport.view_start = (last_idx as f64) - 100.0;

        let scale = TimeScale::new();
        let ticks = scale.generate_ticks(&viewport, &bars, narrow_measure, None, false);

        let midnight_ts = date_to_timestamp(2026, 7, 18, 0, 0, 0);
        let post_midnight: Vec<&TimeTick> = ticks.iter().filter(|t| {
            let idx = t.bar_idx as usize;
            let approx_ts = if t.bar_idx >= 0 && idx < bars.len() {
                bars[idx].timestamp
            } else {
                bars[bars.len() - 1].timestamp + (t.bar_idx - (bars.len() as i64 - 1)) * 15
            };
            approx_ts > midnight_ts
        }).collect();

        eprintln!(
            "panned-to-tail: total ticks: {}, post-midnight ticks: {}",
            ticks.len(),
            post_midnight.len()
        );
        for t in &ticks {
            eprintln!(
                "  tick bar_idx={} x={:.1} weight={:?} label={:?}",
                t.bar_idx, t.x, t.weight, t.label
            );
        }

        assert!(
            !post_midnight.is_empty(),
            "panned-to-tail: expected at least one tick with ts > midnight, got NONE. All ticks: {:?}",
            ticks.iter().map(|t| (t.bar_idx, t.weight, t.label.clone())).collect::<Vec<_>>()
        );
    }
}
