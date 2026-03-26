use digdigdig3::{ExchangeId, AccountType};

/// Canonical key for a bar series.
/// Matches the existing bar_cache key exactly — no migration needed.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BarSeriesKey {
    pub exchange_id: ExchangeId,
    pub account_type: AccountType,
    pub symbol: String,
    pub timeframe: String,
}

impl BarSeriesKey {
    pub fn new(
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol: impl Into<String>,
        timeframe: impl Into<String>,
    ) -> Self {
        Self {
            exchange_id,
            account_type,
            symbol: symbol.into(),
            timeframe: timeframe.into(),
        }
    }

    /// Exchange name string (for disk store compatibility).
    pub fn exchange_str(&self) -> &'static str {
        self.exchange_id.as_str()
    }

    /// Account type short label ("S", "F", etc.) for disk compatibility.
    pub fn account_type_label(&self) -> &'static str {
        self.account_type.short_label()
    }
}
