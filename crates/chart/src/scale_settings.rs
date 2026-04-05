//! Scale settings configuration types.
//!
//! Controls positioning, dimensions, and visibility of chart scales.

use serde::{Deserialize, Serialize};

/// Date format presets
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DateFormat {
    /// Day.Month.Year (21.01.2026) - European style
    #[default]
    DayMonthYear,
    /// Month/Day/Year (01/21/2026) - US style
    MonthDayYear,
    /// Year-Month-Day (2026-01-21) - ISO style
    YearMonthDay,
    /// Short: Day Month (21 Jan)
    DayMonthShort,
}

impl DateFormat {
    pub fn next(&self) -> Self {
        match self {
            Self::DayMonthYear => Self::MonthDayYear,
            Self::MonthDayYear => Self::YearMonthDay,
            Self::YearMonthDay => Self::DayMonthShort,
            Self::DayMonthShort => Self::DayMonthYear,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::DayMonthYear => "21.01.2026",
            Self::MonthDayYear => "01/21/2026",
            Self::YearMonthDay => "2026-01-21",
            Self::DayMonthShort => "21 Jan",
        }
    }
}

/// Time format settings
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TimeFormatSettings {
    /// Date format preset
    pub date_format: DateFormat,
    /// Use 24-hour time format (vs 12-hour AM/PM)
    pub use_24h: bool,
    /// Show day of week on labels (пн, вт, ср...)
    pub show_day_of_week: bool,
    /// Timezone UTC offset in hours (e.g., 3 for Moscow UTC+3, -5 for EST)
    pub timezone_offset_hours: i32,
}

impl TimeFormatSettings {
    pub fn new() -> Self {
        Self {
            date_format: DateFormat::DayMonthYear,
            use_24h: true,
            show_day_of_week: false,
            timezone_offset_hours: 3, // Moscow default
        }
    }

    /// Cycle through common timezone offsets (-12 to +12)
    pub fn cycle_timezone(&mut self) {
        self.timezone_offset_hours = if self.timezone_offset_hours >= 12 {
            -12
        } else {
            self.timezone_offset_hours + 1
        };
    }

    /// Get timezone display label with city name
    pub fn timezone_label(&self) -> String {
        let offset = self.timezone_offset_hours;
        let city = match offset {
            -12 => "Бейкер",
            -11 => "Паго-Паго",
            -10 => "Гонолулу",
            -9 => "Аляска",
            -8 => "Лос-Анджелес",
            -7 => "Денвер",
            -6 => "Чикаго",
            -5 => "Нью-Йорк",
            -4 => "Галифакс",
            -3 => "Буэнос-Айрес",
            -2 => "Среднеатлант.",
            -1 => "Азорские о-ва",
            0 => "Лондон",
            1 => "Берлин",
            2 => "Киев",
            3 => "Москва",
            4 => "Дубай",
            5 => "Ташкент",
            6 => "Алматы",
            7 => "Бангкок",
            8 => "Сингапур",
            9 => "Токио",
            10 => "Сидней",
            11 => "Магадан",
            12 => "Окленд",
            _ => "",
        };
        if offset >= 0 {
            format!("(UTC+{}) {}", offset, city)
        } else {
            format!("(UTC{}) {}", offset, city)
        }
    }

    /// Generate clock time string from a UTC timestamp using this timezone offset
    pub fn format_clock_time(&self, utc_timestamp_secs: i64) -> String {
        let offset_secs = self.timezone_offset_hours as i64 * 3600;
        let local_ts = utc_timestamp_secs + offset_secs;

        let secs_in_day = ((local_ts % 86400) + 86400) % 86400;
        let hours = secs_in_day / 3600;
        let minutes = (secs_in_day % 3600) / 60;
        let seconds = secs_in_day % 60;

        if self.use_24h {
            format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
        } else {
            let (h12, ampm) = if hours == 0 {
                (12, "AM")
            } else if hours < 12 {
                (hours, "AM")
            } else if hours == 12 {
                (12, "PM")
            } else {
                (hours - 12, "PM")
            };
            format!("{}:{:02}:{:02} {}", h12, minutes, seconds, ampm)
        }
    }
}

/// Position configuration for price scale (Y-axis)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PriceScalePosition {
    /// Price scale on left side
    Left,
    /// Price scale on right side (default)
    #[default]
    Right,
    /// Price scale hidden
    Hidden,
}

impl PriceScalePosition {
    /// Cycle to next position: Right -> Left -> Hidden -> Right
    pub fn next(&self) -> Self {
        match self {
            Self::Right => Self::Left,
            Self::Left => Self::Hidden,
            Self::Hidden => Self::Right,
        }
    }

    /// Get display label for UI
    pub fn label(&self) -> &'static str {
        match self {
            Self::Left => "Слева",
            Self::Right => "Справа",
            Self::Hidden => "Скрыта",
        }
    }

    /// Check if scale is visible
    pub fn is_visible(&self) -> bool {
        !matches!(self, Self::Hidden)
    }
}

/// Position configuration for time scale (X-axis)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimeScalePosition {
    /// Time scale on top
    Top,
    /// Time scale on bottom (default)
    #[default]
    Bottom,
    /// Time scale hidden
    Hidden,
}

impl TimeScalePosition {
    /// Cycle to next position: Bottom -> Top -> Hidden -> Bottom
    pub fn next(&self) -> Self {
        match self {
            Self::Bottom => Self::Top,
            Self::Top => Self::Hidden,
            Self::Hidden => Self::Bottom,
        }
    }

    /// Get display label for UI
    pub fn label(&self) -> &'static str {
        match self {
            Self::Top => "Сверху",
            Self::Bottom => "Снизу",
            Self::Hidden => "Скрыта",
        }
    }

    /// Check if scale is visible
    pub fn is_visible(&self) -> bool {
        !matches!(self, Self::Hidden)
    }
}

/// Visibility mode for scale corner (A/M and mode buttons area)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScaleCornerVisibility {
    /// Always visible (default)
    #[default]
    Always,
    /// Visible only when mouse hovers over corner area
    OnHover,
    /// Never visible
    Never,
}

impl ScaleCornerVisibility {
    /// Cycle to next mode: Always -> OnHover -> Never -> Always
    pub fn next(&self) -> Self {
        match self {
            Self::Always => Self::OnHover,
            Self::OnHover => Self::Never,
            Self::Never => Self::Always,
        }
    }

    /// Get display label for UI
    pub fn label(&self) -> &'static str {
        match self {
            Self::Always => "Всегда",
            Self::OnHover => "При наведении",
            Self::Never => "Никогда",
        }
    }

    /// Check if corner should be visible given hover state
    pub fn should_show(&self, is_hovered: bool) -> bool {
        match self {
            Self::Always => true,
            Self::OnHover => is_hovered,
            Self::Never => false,
        }
    }
}

/// Default price scale width in pixels
pub const DEFAULT_PRICE_SCALE_WIDTH: f64 = 70.0;

/// Default time scale height in pixels
pub const DEFAULT_TIME_SCALE_HEIGHT: f64 = 30.0;

/// Minimum price scale width
pub const MIN_PRICE_SCALE_WIDTH: f64 = 50.0;

/// Maximum price scale width
pub const MAX_PRICE_SCALE_WIDTH: f64 = 150.0;

/// Minimum time scale height
pub const MIN_TIME_SCALE_HEIGHT: f64 = 20.0;

/// Maximum time scale height
pub const MAX_TIME_SCALE_HEIGHT: f64 = 60.0;

/// Runtime-configurable scale settings
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScaleSettings {
    /// Price scale position (left/right/hidden)
    pub price_scale_position: PriceScalePosition,
    /// Time scale position (top/bottom/hidden)
    pub time_scale_position: TimeScalePosition,
    /// Scale corner visibility mode
    pub corner_visibility: ScaleCornerVisibility,
    /// Price scale width in pixels
    pub price_scale_width: f64,
    /// Time scale height in pixels
    pub time_scale_height: f64,
    /// Time format settings (date format, 24h/12h, day of week)
    pub time_format: TimeFormatSettings,
    /// Show countdown to bar close on price scale
    #[serde(default = "default_true")]
    pub show_bar_countdown: bool,
    /// Show previous close price line
    pub show_prev_close_line: bool,
    /// Previous close line color
    pub prev_close_color: String,
    /// User-configured price precision (None = automatic)
    pub user_precision: Option<usize>,
    /// Price tick line style: "dotted", "dashed", "solid" (default: "dotted")
    #[serde(default = "default_price_tick_style")]
    pub price_tick_style: String,
    /// Extend price tick line to the right (default: true)
    #[serde(default = "default_true")]
    pub price_tick_extend_right: bool,
    /// Extend price tick line to the left (default: true)
    #[serde(default = "default_true")]
    pub price_tick_extend_left: bool,
}

fn default_price_tick_style() -> String {
    "dotted".to_string()
}

fn default_true() -> bool {
    true
}

impl Default for ScaleSettings {
    fn default() -> Self {
        Self {
            price_scale_position: PriceScalePosition::Right,
            time_scale_position: TimeScalePosition::Bottom,
            corner_visibility: ScaleCornerVisibility::Always,
            price_scale_width: DEFAULT_PRICE_SCALE_WIDTH,
            time_scale_height: DEFAULT_TIME_SCALE_HEIGHT,
            time_format: TimeFormatSettings::default(),
            show_bar_countdown: true,  // on by default
            show_prev_close_line: false,  // off by default
            prev_close_color: "#787B86".to_string(),  // gray
            user_precision: None,  // automatic by default
            price_tick_style: "dotted".to_string(),
            price_tick_extend_right: true,
            price_tick_extend_left: true,
        }
    }
}

impl ScaleSettings {
    /// Create new settings with defaults
    pub fn new() -> Self {
        Self::default()
    }

    /// Get effective price scale width (0 if hidden)
    pub fn effective_price_scale_width(&self) -> f64 {
        if self.price_scale_position.is_visible() {
            self.price_scale_width
        } else {
            0.0
        }
    }

    /// Get effective time scale height (0 if hidden)
    pub fn effective_time_scale_height(&self) -> f64 {
        if self.time_scale_position.is_visible() {
            self.time_scale_height
        } else {
            0.0
        }
    }

    /// Set price scale width with clamping to valid range
    pub fn set_price_scale_width(&mut self, width: f64) {
        self.price_scale_width = width.clamp(MIN_PRICE_SCALE_WIDTH, MAX_PRICE_SCALE_WIDTH);
    }

    /// Set time scale height with clamping to valid range
    pub fn set_time_scale_height(&mut self, height: f64) {
        self.time_scale_height = height.clamp(MIN_TIME_SCALE_HEIGHT, MAX_TIME_SCALE_HEIGHT);
    }
}

/// Cycle through precision options: Авто -> 0 -> 1 -> 2 -> 3 -> 4 -> 5 -> 6 -> 8 -> Авто
pub fn cycle_precision(current: Option<usize>) -> Option<usize> {
    match current {
        None => Some(0),
        Some(0) => Some(1),
        Some(1) => Some(2),
        Some(2) => Some(3),
        Some(3) => Some(4),
        Some(4) => Some(5),
        Some(5) => Some(6),
        Some(6) => Some(8),
        Some(8) => None,
        Some(_) => None,
    }
}

/// Get display label for a precision value
pub fn precision_label(precision: Option<usize>) -> &'static str {
    match precision {
        None => "Авто",
        Some(0) => "0",
        Some(1) => "1 (0.0)",
        Some(2) => "2 (0.00)",
        Some(3) => "3 (0.000)",
        Some(4) => "4 (0.0000)",
        Some(5) => "5 (0.00000)",
        Some(6) => "6",
        Some(8) => "8",
        Some(_) => "Авто",
    }
}
