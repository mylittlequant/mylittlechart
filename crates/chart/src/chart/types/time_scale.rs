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
/// Higher weight = more important = larger font/brighter color
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
#[repr(u8)]
pub enum TickMarkWeight {
    /// Sub-minute granularity
    #[default]
    Second = 0,
    /// 1-minute boundaries
    Minute1 = 10,
    /// 5-minute boundaries
    Minute5 = 15,
    /// 30-minute boundaries
    Minute30 = 20,
    /// Hour boundaries
    Hour = 30,
    /// 4-hour boundaries
    Hour4 = 35,
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

    /// Convert bar index to timestamp (handles extrapolation)
    fn bar_idx_to_timestamp(idx: i64, first_ts: i64, bar_interval: i64) -> i64 {
        first_ts + idx * bar_interval
    }

    /// Convert timestamp to bar index (handles extrapolation)
    fn timestamp_to_bar_idx(ts: i64, first_ts: i64, bar_interval: i64) -> i64 {
        (ts - first_ts) / bar_interval
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

        if bars.is_empty() {
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

        let start_ts = Self::bar_idx_to_timestamp(start_bar - 50, first_ts, bar_interval);
        let end_ts = Self::bar_idx_to_timestamp(end_bar + 50, first_ts, bar_interval);

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
                first_ts, bar_interval,
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
                first_ts, bar_interval,
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

    /// Generate intraday ticks (hours, minutes) for high zoom levels
    fn generate_intraday_ticks<F>(
        &self,
        ticks: &mut Vec<TimeTick>,
        start_ts: i64,
        end_ts: i64,
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
        let (time_interval, weight) = if day_width_px > 24.0 * Self::MIN_CELL_WIDTH {
            // Very zoomed in: show hours (1200px+ per day)
            (HOUR, TickMarkWeight::Hour)
        } else if day_width_px > 6.0 * Self::MIN_CELL_WIDTH {
            // Moderately zoomed: show 4-hour (300px+ per day)
            (4 * HOUR, TickMarkWeight::Hour4)
        } else {
            // Less zoomed: show 12-hour (but with Hour4 weight)
            (12 * HOUR, TickMarkWeight::Hour4)
        };

        // Generate boundaries
        let mut candidates: Vec<(i64, f64, TickMarkWeight, i64)> = Vec::new();

        // Also add day boundaries for context
        self.add_day_boundaries(&mut candidates, start_ts, end_ts, first_ts, bar_interval, viewport);

        // Add intraday boundaries
        let first_boundary = (start_ts / time_interval) * time_interval;
        let mut boundary_ts = first_boundary;
        while boundary_ts <= end_ts {
            let bar_idx = Self::timestamp_to_bar_idx(boundary_ts, first_ts, bar_interval);
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
            let bar_idx = Self::timestamp_to_bar_idx(ts, first_ts, bar_interval);
            // Skip months before data starts
            if bar_idx < 0 { return false; }
            let x = viewport.bar_to_x_f64(bar_idx as f64);
            x >= 0.0 && x <= viewport.chart_width
        }).count();

        // If 3+ months visible, show only month labels
        if visible_month_count >= 3 {
            for &(y, m) in &months_in_range {
                let ts = date_to_timestamp(y, m, 1, 0, 0, 0);
                let bar_idx = Self::timestamp_to_bar_idx(ts, first_ts, bar_interval);
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
            if cell_width >= Self::MIN_CELL_WIDTH && cell_width <= Self::MAX_CELL_WIDTH {
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
                    let bar_idx = Self::timestamp_to_bar_idx(ts, first_ts, bar_interval);
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
                let bar_idx = Self::timestamp_to_bar_idx(ts, first_ts, bar_interval);
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
            let bar_idx = Self::timestamp_to_bar_idx(ts, first_ts, bar_interval);
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

    /// Select ticks with proper spacing
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

        // Sort by weight descending, then by position
        candidates.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)));

        let mut used_positions: Vec<(f64, f64)> = Vec::new(); // (x, half_width)

        for (bar_idx, x, weight, ts) in candidates {
            // Skip if far outside visible area
            if x < -50.0 || x > viewport.chart_width + 50.0 {
                continue;
            }

            // Format label
            let label = if let Some(settings) = format_settings {
                format_time_by_weight_with_settings(ts, weight, settings)
            } else {
                format_time_by_weight(ts, weight)
            };
            let measured_width = measure_text(&label);
            let half_width = measured_width / 2.0 + 5.0;

            // Pixel-based collision check
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
        }
    }
}

// =============================================================================
// Time Formatting Functions
// =============================================================================

/// Format time label based on weight (uses current language)
pub fn format_time_by_weight(ts: i64, weight: TickMarkWeight) -> String {
    let (year, month, day, hour, minute, _) = timestamp_to_date(ts);
    let month_names = month_names_short(current_language());

    match weight {
        TickMarkWeight::Year => format!("{}", year),
        TickMarkWeight::Month => {
            month_names[(month - 1) as usize].to_string()
        }
        TickMarkWeight::Day => {
            format!("{} {}", day, month_names[(month - 1) as usize])
        }
        TickMarkWeight::Hour4 | TickMarkWeight::Hour => {
            format!("{:02}:{:02}", hour, minute)
        }
        TickMarkWeight::Minute30 | TickMarkWeight::Minute5 | TickMarkWeight::Minute1 => {
            format!("{:02}:{:02}", hour, minute)
        }
        TickMarkWeight::Second => format!("{:02}:{:02}", hour, minute),
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
    let (year, month, day, hour, minute, _) = timestamp_to_date(ts);
    let month_names = month_names_short(current_language());

    // Format time part based on use_24h setting
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
        TickMarkWeight::Hour4 | TickMarkWeight::Hour |
        TickMarkWeight::Minute30 | TickMarkWeight::Minute5 |
        TickMarkWeight::Minute1 | TickMarkWeight::Second => {
            time_str
        }
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
    fn test_weight_classification() {
        assert!(TickMarkWeight::Year.is_major());
        assert!(TickMarkWeight::Month.is_major());
        assert!(!TickMarkWeight::Day.is_major());

        assert!(TickMarkWeight::Day.is_medium());
        assert!(!TickMarkWeight::Hour.is_medium());
    }
}
