//! Alert data types.

use serde::{Deserialize, Serialize};

// =============================================================================
// DrawingExtendMode
// =============================================================================

/// Extension mode for drawing primitives — controls alert boundary detection.
///
/// Mirrors `zengeld_chart::drawing::primitives_v2::types::ExtendMode` but lives
/// in the alerts crate so we avoid a circular dependency.  Convert from
/// `ExtendMode` at the chart-app call site via `from_u8`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DrawingExtendMode {
    /// Segment only — no extension beyond the two endpoints.
    #[default]
    None,
    /// Extends to the right (ray).
    Right,
    /// Extends to the left.
    Left,
    /// Extends in both directions (infinite line).
    Both,
}

impl DrawingExtendMode {
    /// Convert from a raw `u8` discriminant produced by `Primitive::extend_mode_raw()`.
    ///
    /// | value | meaning |
    /// |-------|---------|
    /// | 0     | None    |
    /// | 1     | Right   |
    /// | 2     | Left    |
    /// | 3     | Both    |
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Self::Right,
            2 => Self::Left,
            3 => Self::Both,
            _ => Self::None,
        }
    }
}

// =============================================================================
// AlertStatus
// =============================================================================

/// Alert status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlertStatus {
    Active,
    Triggered,
    Paused,
    Expired,
}

// =============================================================================
// AlertCondition
// =============================================================================

/// Alert trigger condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlertCondition {
    CrossingUp,
    CrossingDown,
    Crossing,
    GreaterThan,
    LessThan,
    EnteringRange,
    ExitingRange,
    InsideRange,
    OutsideRange,
    MovingUp,
    MovingDown,
    MovingUpOrDown,
}

impl AlertCondition {
    pub fn display_name(&self) -> &'static str {
        match self {
            AlertCondition::CrossingUp => "Crossing Up",
            AlertCondition::CrossingDown => "Crossing Down",
            AlertCondition::Crossing => "Crossing",
            AlertCondition::GreaterThan => "Greater Than",
            AlertCondition::LessThan => "Less Than",
            AlertCondition::EnteringRange => "Entering Channel",
            AlertCondition::ExitingRange => "Exiting Channel",
            AlertCondition::InsideRange => "Inside Channel",
            AlertCondition::OutsideRange => "Outside Channel",
            AlertCondition::MovingUp => "Moving Up %",
            AlertCondition::MovingDown => "Moving Down %",
            AlertCondition::MovingUpOrDown => "Moving Up or Down %",
        }
    }

    pub fn all() -> &'static [AlertCondition] {
        &[
            AlertCondition::CrossingUp,
            AlertCondition::CrossingDown,
            AlertCondition::Crossing,
            AlertCondition::GreaterThan,
            AlertCondition::LessThan,
            AlertCondition::EnteringRange,
            AlertCondition::ExitingRange,
            AlertCondition::InsideRange,
            AlertCondition::OutsideRange,
            AlertCondition::MovingUp,
            AlertCondition::MovingDown,
            AlertCondition::MovingUpOrDown,
        ]
    }

    /// Whether this condition requires a second price (range conditions).
    pub fn requires_second_price(&self) -> bool {
        matches!(
            self,
            AlertCondition::EnteringRange
                | AlertCondition::ExitingRange
                | AlertCondition::InsideRange
                | AlertCondition::OutsideRange
        )
    }

    /// Whether this condition requires a percentage value.
    pub fn requires_percentage(&self) -> bool {
        matches!(
            self,
            AlertCondition::MovingUp | AlertCondition::MovingDown | AlertCondition::MovingUpOrDown
        )
    }
}

// =============================================================================
// SignalBarState / SignalDirection
// =============================================================================

/// Whether the signal alert fires on a forming (unclosed) bar or only on a formed (closed) bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignalBarState {
    /// Alert fires as soon as the signal appears on the current (unclosed) bar.
    Forming,
    /// Alert fires only after the bar closes and the signal is confirmed.
    Formed,
}

/// Filter for which signal directions to alert on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignalDirection {
    /// Any direction (bullish, bearish, neutral).
    Any,
    /// Only bullish signals (direction == +1).
    Bullish,
    /// Only bearish signals (direction == -1).
    Bearish,
}

// =============================================================================
// AlertSource
// =============================================================================

/// What the alert is attached to / monitors.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AlertSource {
    /// Horizontal price alert on a symbol.
    Price { symbol: String },
    /// Attached to a drawing primitive (trendline, channel, etc.).
    Drawing { primitive_id: u64, label: String },
    /// Attached to an indicator output line.
    Indicator {
        indicator_id: u64,
        output_index: usize,
        label: String,
    },
    /// Crossing between two sources (indicator crosses indicator, price crosses indicator, etc.).
    CrossingPair {
        source_a: Box<AlertSource>,
        source_b: Box<AlertSource>,
    },
    /// Alert on indicator signals (crossovers, divergences, etc.).
    Signal {
        indicator_id: u64,
        /// Human-readable label (e.g. "RSI(14)").
        label: String,
        /// Filter by signal direction (Any/Bullish/Bearish).
        direction_filter: SignalDirection,
        /// Whether to fire on forming (unclosed) or formed (closed) bar signals.
        bar_state: SignalBarState,
    },
}

impl AlertSource {
    /// Human-readable description of this source.
    pub fn display_name(&self) -> String {
        match self {
            AlertSource::Price { symbol } => format!("{} Price", symbol),
            AlertSource::Drawing { primitive_id, label } => {
                if label.is_empty() {
                    format!("Drawing #{}", primitive_id)
                } else {
                    format!("{} #{}", label, primitive_id)
                }
            }
            AlertSource::Indicator {
                label,
                output_index,
                ..
            } => {
                if label.is_empty() {
                    format!("Indicator line {}", output_index)
                } else {
                    format!("{} line {}", label, output_index)
                }
            }
            AlertSource::CrossingPair { source_a, source_b } => {
                format!("{} \u{00d7} {}", source_a.display_name(), source_b.display_name())
            }
            AlertSource::Signal { label, direction_filter, bar_state, .. } => {
                let dir = match direction_filter {
                    SignalDirection::Any => "",
                    SignalDirection::Bullish => " Bullish",
                    SignalDirection::Bearish => " Bearish",
                };
                let state = match bar_state {
                    SignalBarState::Forming => "Forming",
                    SignalBarState::Formed => "Formed",
                };
                if label.is_empty() {
                    format!("Signal{} ({})", dir, state)
                } else {
                    format!("{}{} Signal ({})", label, dir, state)
                }
            }
        }
    }
}

// =============================================================================
// AlertTriggerMode
// =============================================================================

/// How many times the alert fires.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlertTriggerMode {
    /// Fire once, then set status to Triggered.
    OneShot,
    /// Fire every time the condition is met.
    EveryTime,
    /// Fire once per bar/candle.
    OncePerBar,
    /// Fire N times then stop.
    TimesN(u32),
}

impl AlertTriggerMode {
    /// Human-readable label. For `TimesN` use `display_name_owned()` to include the count.
    pub fn display_name(&self) -> &'static str {
        match self {
            AlertTriggerMode::OneShot => "Once",
            AlertTriggerMode::EveryTime => "Every Time",
            AlertTriggerMode::OncePerBar => "Once Per Bar",
            AlertTriggerMode::TimesN(_) => "N Times",
        }
    }

    /// Owned display name that includes the N for `TimesN`.
    pub fn display_name_owned(&self) -> String {
        match self {
            AlertTriggerMode::TimesN(n) => format!("{} Times", n),
            other => other.display_name().to_string(),
        }
    }
}

// =============================================================================
// AlertTransport
// =============================================================================

/// Notification delivery method.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum AlertTransport {
    /// Show popup notification in terminal.
    Popup,
    /// Play sound.
    Sound,
    /// HTTP POST to URL.
    Webhook { url: String },
    /// Send a Telegram message via the bot configured in `NotificationSettings`.
    Telegram,
}

impl AlertTransport {
    pub fn display_name(&self) -> &'static str {
        match self {
            AlertTransport::Popup => "Popup",
            AlertTransport::Sound => "Sound",
            AlertTransport::Webhook { .. } => "Webhook",
            AlertTransport::Telegram => "Telegram",
        }
    }

    /// Default transport set for new alerts.
    pub fn all_default() -> Vec<AlertTransport> {
        vec![AlertTransport::Popup]
    }
}

// =============================================================================
// Serde default helpers
// =============================================================================

fn default_alert_source() -> AlertSource {
    AlertSource::Price {
        symbol: String::new(),
    }
}

fn default_trigger_mode() -> AlertTriggerMode {
    AlertTriggerMode::OneShot
}

fn default_transports() -> Vec<AlertTransport> {
    AlertTransport::all_default()
}

// =============================================================================
// AlertItem
// =============================================================================

/// A single alert.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AlertItem {
    pub id: u64,
    pub name: String,
    /// What this alert monitors.
    #[serde(default = "default_alert_source")]
    pub source: AlertSource,
    pub condition: AlertCondition,
    pub price: f64,
    /// Second price for range conditions.
    #[serde(default)]
    pub price2: f64,
    /// Percentage for MovingUp/Down conditions.
    #[serde(default)]
    pub percentage: f64,
    /// How many times to trigger.
    #[serde(default = "default_trigger_mode")]
    pub trigger_mode: AlertTriggerMode,
    /// Max triggers for TimesN mode (0 = unlimited for EveryTime).
    #[serde(default)]
    pub max_triggers: u32,
    pub trigger_count: u32,
    /// Notification channels.
    #[serde(default = "default_transports")]
    pub transports: Vec<AlertTransport>,
    pub status: AlertStatus,
    /// Unix timestamp millis.
    #[serde(default)]
    pub created_at: u64,
    /// Unix timestamp millis.
    #[serde(default)]
    pub last_triggered_at: Option<u64>,
    /// Optional expiry (unix timestamp millis).
    #[serde(default)]
    pub expires_at: Option<u64>,
    // Kept for backward-compat deserialization of old snapshots.
    // Use `symbol()` accessor instead of reading this field directly.
    #[serde(default)]
    symbol: String,
    // Kept for backward-compat deserialization of old snapshots.
    #[serde(default)]
    pub last_triggered: Option<String>,
    /// Previous tick's dynamic level (for crossing detection on Drawing/Indicator alerts).
    /// Not persisted — recalculated on tick.
    #[serde(skip)]
    pub prev_dynamic_price: f64,

    /// Sync group that owns this alert. `None` only in pre-migration presets.
    /// Populated at alert-create time from the active window's `group_id`.
    #[serde(default)]
    pub group_id: Option<u64>,

    /// Exchange this alert is bound to (e.g. `"Binance"`).
    /// Populated at create time from the active window's `exchange`.
    #[serde(default)]
    pub exchange: String,

    /// Window that created this alert. Used as a display hint only —
    /// alerts are NOT exclusive to one window, they show on all windows
    /// in the same group that match `exchange:symbol`.
    #[serde(default)]
    pub window_id_hint: Option<u64>,
}

impl AlertItem {
    /// Create a new alert from an `AlertSource`.
    pub fn new(
        id: u64,
        source: AlertSource,
        name: &str,
        price: f64,
        condition: AlertCondition,
        status: AlertStatus,
    ) -> Self {
        Self {
            id,
            name: name.to_string(),
            source,
            condition,
            price,
            price2: 0.0,
            percentage: 0.0,
            trigger_mode: AlertTriggerMode::OneShot,
            max_triggers: 0,
            trigger_count: 0,
            transports: AlertTransport::all_default(),
            status,
            created_at: 0,
            last_triggered_at: None,
            expires_at: None,
            symbol: String::new(),
            last_triggered: None,
            prev_dynamic_price: 0.0,
            group_id: None,
            exchange: String::new(),
            window_id_hint: None,
        }
    }

    // -------------------------------------------------------------------------
    // Builder methods
    // -------------------------------------------------------------------------

    pub fn with_trigger_mode(mut self, mode: AlertTriggerMode) -> Self {
        self.trigger_mode = mode;
        self
    }

    pub fn with_transports(mut self, transports: Vec<AlertTransport>) -> Self {
        self.transports = transports;
        self
    }

    pub fn with_price2(mut self, price2: f64) -> Self {
        self.price2 = price2;
        self
    }

    pub fn with_percentage(mut self, pct: f64) -> Self {
        self.percentage = pct;
        self
    }

    pub fn with_created_at(mut self, ts_millis: u64) -> Self {
        self.created_at = ts_millis;
        self
    }

    pub fn with_expires_at(mut self, ts_millis: u64) -> Self {
        self.expires_at = Some(ts_millis);
        self
    }

    pub fn with_trigger_count(mut self, count: u32) -> Self {
        self.trigger_count = count;
        self
    }

    pub fn with_last_triggered(mut self, time: &str) -> Self {
        self.last_triggered = Some(time.to_string());
        self
    }

    // -------------------------------------------------------------------------
    // Accessors
    // -------------------------------------------------------------------------

    /// Extract the symbol string from the source, with a fallback to the legacy field.
    pub fn symbol(&self) -> &str {
        match &self.source {
            AlertSource::Price { symbol } => symbol,
            _ => &self.symbol,
        }
    }

    /// Human-readable description of what this alert monitors.
    pub fn source_display(&self) -> String {
        self.source.display_name()
    }

    /// Returns `true` when this alert has `AlertStatus::Active`.
    pub fn is_active(&self) -> bool {
        self.status == AlertStatus::Active
    }

    /// Returns `true` when this alert has `AlertStatus::Triggered`.
    pub fn is_triggered(&self) -> bool {
        self.status == AlertStatus::Triggered
    }

    /// Returns `"exchange:symbol"` routing key for per-symbol crossing detection.
    /// Falls back to just `symbol()` when exchange is empty (old presets).
    pub fn routing_key(&self) -> String {
        let sym = self.symbol();
        if self.exchange.is_empty() {
            sym.to_string()
        } else {
            format!("{}:{}", self.exchange, sym)
        }
    }

    /// Returns true when this alert should be shown on a window with
    /// the given `symbol` and `exchange`. Exchange match is skipped for
    /// old presets that have no exchange stored (empty string).
    pub fn matches_window(&self, symbol: &str, exchange: &str) -> bool {
        let sym_match = self.symbol() == symbol;
        let exch_match = self.exchange.is_empty() || self.exchange == exchange;
        sym_match && exch_match
    }
}
