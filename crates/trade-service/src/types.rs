use digdigdig3::{ExchangeId, AccountType};

/// Canonical key for a trade series.
///
/// No `timeframe` field — trades are not keyed by timeframe.
/// Each panel that needs a specific timeframe view derives its own
/// aggregation from the same raw ring.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TradeKey {
    pub exchange_id: ExchangeId,
    pub account_type: AccountType,
    pub symbol: String,
}

impl TradeKey {
    pub fn new(
        exchange_id: ExchangeId,
        account_type: AccountType,
        symbol: impl Into<String>,
    ) -> Self {
        Self {
            exchange_id,
            account_type,
            symbol: symbol.into(),
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
