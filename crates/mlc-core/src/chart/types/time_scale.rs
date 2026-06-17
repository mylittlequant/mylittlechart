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
    ) -> Vec<TimeTick>
    where
        F: Fn(&str) -> f64,
    {
        let mut ticks = Vec::new();

        // Time-axis charts (tick / derived bars) place ticks by real timestamp,
        // independent of bar index / bar_spacing.
        if viewport.is_time_axis() {
            self.generate_time_axis_ticks(&mut ticks, viewport, &measure_text, format_settings);
            ticks.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));
            return ticks;
        }

        if bars.is_empty() {
            return ticks;
        }

        // Detect non-uniform bar spacing (range/volume/tick bars). When the
        // max gap between adjacent bars is > 2× the median gap, the existing
        // `day_width_px` interval dispatch picks labels at wrong timestamps
        // (the boundary cells skip over real bars). Switch to the LWC
        // per-bar weight path: fill_weights → one candidate per bar with the
        // calendar-boundary weight, picker accepts in weight-desc order.
        let non_uniform = bars.len() >= 3 && is_non_uniform(bars);
        if non_uniform {
            self.generate_per_bar_ticks(
                &mut ticks,
                bars,
                viewport,
                &measure_text,
                format_settings,
            );
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

    /// Generate ticks for a time-domain X axis (tick chart / derived bars).
    ///
    /// Picks the smallest "nice" step (1s … 1d) whose pixel cell is at least
    /// `MIN_CELL_WIDTH`, walks aligned boundaries across the visible time range,
    /// and positions each via `viewport.ts_to_x`. Labels reuse
    /// `format_time_by_weight` (so sub-minute steps show HH:MM:SS).
    fn generate_time_axis_ticks<F>(
        &self,
        ticks: &mut Vec<TimeTick>,
        viewport: &Viewport,
        _measure_text: &F,
        format_settings: Option<&TimeFormatSettings>,
    ) where
        F: Fn(&str) -> f64,
    {
        let (ts_start, ts_end) = match viewport.visible_time_range() {
            Some(r) => r,
            None => return,
        };
        let axis = match viewport.time_axis {
            Some(a) => a,
            None => return,
        };

        // Candidate steps in ms, paired with the label weight.
        // (ms per step, weight)
        const STEPS: &[(i64, TickMarkWeight)] = &[
            (1_000, TickMarkWeight::Second),
            (5_000, TickMarkWeight::Second),
            (10_000, TickMarkWeight::Second),
            (30_000, TickMarkWeight::Second),
            (60_000, TickMarkWeight::Minute1),
            (5 * 60_000, TickMarkWeight::Minute5),
            (15 * 60_000, TickMarkWeight::Minute5),
            (30 * 60_000, TickMarkWeight::Minute30),
            (3_600_000, TickMarkWeight::Hour),
            (4 * 3_600_000, TickMarkWeight::Hour4),
            (12 * 3_600_000, TickMarkWeight::Hour4),
            (86_400_000, TickMarkWeight::Day),
        ];

        // Smallest step whose pixel cell >= MIN_CELL_WIDTH.
        let step = STEPS
            .iter()
            .find(|(ms, _)| (*ms as f64 / axis.ms_per_px) >= Self::MIN_CELL_WIDTH)
            .copied()
            .unwrap_or((86_400_000, TickMarkWeight::Day));
        let (step_ms, weight) = step;

        // Walk aligned boundaries across the visible span.
        let first = (ts_start / step_ms) * step_ms;
        let mut ts = first;
        // Cap iterations defensively (very wide span at tiny step).
        let mut guard = 0;
        while ts <= ts_end && guard < 10_000 {
            let x = axis.ts_to_x(ts);
            if x >= -50.0 && x <= viewport.chart_width + 50.0 {
                let label = if let Some(settings) = format_settings {
                    format_time_by_weight_with_settings(ts, weight, settings)
                } else {
                    format_time_by_weight(ts, weight)
                };
                ticks.push(TimeTick { bar_idx: 0, x, weight, label });
            }
            ts += step_ms;
            guard += 1;
        }
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
                // Check if this is a day boundary (midnight)
                let (_, _, _, hour, _, _) = timestamp_to_date(boundary_ts);
                if hour != 0 {
                    // Not midnight, add as intraday
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
        // We measure a representative full label ("01 Jan" / "01:00") and
        // convert that to a bar-count using bar_spacing. Falls back to the
        // pixel-collision check when bar_spacing is zero (defensive — should
        // never happen because viewport already guards on this).
        let sample_label = "00:00";
        let label_px = measure_text(sample_label).max(20.0);
        let bar_spacing = viewport.bar_spacing.max(1e-6);
        let max_indexes_per_mark = ((label_px / bar_spacing).ceil() as i64).max(1);

        // Track accepted bar indices sorted for bilateral lookup.
        let mut accepted: Vec<i64> = Vec::new();
        let mut used_positions: Vec<(f64, f64)> = Vec::new(); // (x, half_width)

        for (bar_idx, x, weight, ts) in candidates {
            // Skip if far outside visible area.
            if x < -50.0 || x > viewport.chart_width + 50.0 {
                continue;
            }

            // Bilateral index-gap check.
            let left_gap = accepted
                .iter()
                .rev()
                .find(|&&i| i < bar_idx)
                .map(|i| bar_idx - i);
            let right_gap = accepted
                .iter()
                .find(|&&i| i > bar_idx)
                .map(|i| i - bar_idx);

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
            let measured_width = measure_text(&label);
            let half_width = measured_width / 2.0 + 5.0;

            // Pixel-based safety net (handles unequal label widths).
            let conflicts = used_positions
                .iter()
                .any(|(ox, ohw)| (x - ox).abs() < half_width + ohw + 10.0);
            if conflicts {
                continue;
            }

            ticks.push(TimeTick {
                bar_idx,
                x,
                weight,
                label,
            });
            used_positions.push((x, half_width));

            // Maintain accepted as sorted by bar_idx.
            let pos = accepted.binary_search(&bar_idx).unwrap_or_else(|p| p);
            accepted.insert(pos, bar_idx);
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
    fn test_time_axis_ticks_use_ts_not_index() {
        use crate::chart::types::viewport::{TimeAxis, Viewport};
        let mut vp = Viewport { chart_width: 1000.0, ..Default::default() };
        // 100 ms/px -> 100_000 ms visible. Cell for 30s step = 30000/100 = 300px (>=50).
        vp.time_axis = Some(TimeAxis { t_start_ms: 0, ms_per_px: 100.0 });
        let ts = TimeScale::new();
        // bars empty on purpose: time-axis path must NOT depend on bars.
        let ticks = ts.generate_ticks(&vp, &[], |_s| 20.0, None);
        assert!(!ticks.is_empty(), "time-axis must emit ticks with no bars");
        // All tick X must land inside (with margin) the chart width.
        for t in &ticks {
            assert!(t.x >= -50.0 && t.x <= 1050.0, "tick x out of range: {}", t.x);
        }
        // Ticks must be strictly increasing in x (sorted, distinct boundaries).
        for w in ticks.windows(2) {
            assert!(w[1].x > w[0].x, "ticks not strictly increasing");
        }
    }

    #[test]
    fn test_weight_classification() {
        assert!(TickMarkWeight::Year.is_major());
        assert!(TickMarkWeight::Month.is_major());
        assert!(!TickMarkWeight::Day.is_major());

        assert!(TickMarkWeight::Day.is_medium());
        assert!(!TickMarkWeight::Hour.is_medium());
    }
}
