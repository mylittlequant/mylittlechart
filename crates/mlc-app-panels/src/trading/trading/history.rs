//! History-depth setting for the trading tables (owner spec 2026-07-28).
//!
//! Every bottom-strip table (Positions / Orders / Trades) carries a
//! [`HistoryDepth`]: how much history to show — off (active only), a
//! day-window, or everything. Default 30 days. When the panels live inside
//! the trading container the knobs surface in the container's common gear
//! modal (per-panel mini-gears are suppressed there); a future standalone
//! control pane would read the same field.

/// How much history a trading table shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HistoryDepth {
    /// No history section — active rows only.
    Off,
    /// History within the last N days.
    Days(u32),
    /// Unbounded history (up to the engine's own cap).
    All,
}

impl Default for HistoryDepth {
    fn default() -> Self {
        Self::Days(30)
    }
}

impl HistoryDepth {
    /// The selectable options, in control display order.
    pub const OPTIONS: [HistoryDepth; 4] = [
        HistoryDepth::Off,
        HistoryDepth::Days(7),
        HistoryDepth::Days(30),
        HistoryDepth::All,
    ];

    pub fn label(&self) -> String {
        match self {
            Self::Off => "Off".to_string(),
            Self::Days(n) => format!("{}d", n),
            Self::All => "All".to_string(),
        }
    }

    /// Index into [`Self::OPTIONS`] (nearest match for foreign `Days`
    /// values — snaps to 30d).
    pub fn option_index(&self) -> usize {
        Self::OPTIONS.iter().position(|o| o == self).unwrap_or(2)
    }

    /// Whether a record with timestamp `ts_ms` passes this depth filter at
    /// wall-clock `now_ms`.
    pub fn passes(&self, ts_ms: i64, now_ms: i64) -> bool {
        match self {
            Self::Off => false,
            Self::Days(n) => ts_ms >= now_ms - (*n as i64) * 86_400_000,
            Self::All => true,
        }
    }
}
