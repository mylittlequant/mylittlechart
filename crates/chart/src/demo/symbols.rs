//! Demo Symbol Definitions
//!
//! 10 demo trading symbols for testing multi-symbol functionality.
//! Each symbol has unique characteristics (volatility, price range, trend).

/// Demo symbol configuration with generation parameters
///
/// Simple struct for demo data generation - no category abstractions.
/// For real trading symbols with categories, see terminal's state module.
#[derive(Debug, Clone)]
pub struct DemoSymbol {
    /// Symbol ticker (e.g., "AAPL", "BTCUSD")
    pub symbol: String,
    /// Full name (e.g., "Apple Inc.")
    pub name: String,
    /// Base price for generation
    pub base_price: f64,
    /// Volatility multiplier (1.0 = normal, 2.0 = double volatility)
    pub volatility: f64,
    /// Trend bias (-1.0 to 1.0, 0.0 = neutral)
    pub trend_bias: f64,
    /// Unique seed offset for reproducible generation
    pub seed_offset: u64,
    /// Price precision (decimal places)
    pub precision: u8,
    /// Exchange or data source name (e.g. "BINANCE", "NASDAQ", "OANDA")
    pub exchange: String,
}

impl DemoSymbol {
    /// Create a new demo symbol
    pub fn new(
        symbol: &str,
        name: &str,
        base_price: f64,
        volatility: f64,
        trend_bias: f64,
        seed_offset: u64,
        precision: u8,
        exchange: &str,
    ) -> Self {
        Self {
            symbol: symbol.to_string(),
            name: name.to_string(),
            base_price,
            volatility,
            trend_bias,
            seed_offset,
            precision,
            exchange: exchange.to_string(),
        }
    }

    /// Get symbol ticker
    pub fn ticker(&self) -> &str {
        &self.symbol
    }

    /// Get full display name
    pub fn display(&self) -> String {
        format!("{} - {}", self.symbol, self.name)
    }
}

/// Get all demo symbols (10 symbols total)
pub fn demo_symbols() -> Vec<DemoSymbol> {
    vec![
        // Stocks (4)
        DemoSymbol::new("AAPL", "Apple Inc.", 175.0, 1.0, 0.1, 1001, 2, "NASDAQ"),
        DemoSymbol::new("TSLA", "Tesla Inc.", 250.0, 2.5, 0.0, 1002, 2, "NASDAQ"),
        DemoSymbol::new("NVDA", "NVIDIA Corporation", 480.0, 1.8, 0.2, 1003, 2, "NASDAQ"),
        DemoSymbol::new("MSFT", "Microsoft Corporation", 380.0, 0.9, 0.15, 1004, 2, "NASDAQ"),
        // Crypto (3)
        DemoSymbol::new("BTCUSD", "Bitcoin / US Dollar", 42000.0, 2.0, 0.05, 2001, 2, "BINANCE"),
        DemoSymbol::new("ETHUSD", "Ethereum / US Dollar", 2200.0, 2.2, 0.03, 2002, 2, "BINANCE"),
        DemoSymbol::new("SOLUSD", "Solana / US Dollar", 100.0, 3.0, 0.0, 2003, 2, "BINANCE"),
        // Forex (2)
        DemoSymbol::new("EURUSD", "Euro / US Dollar", 1.0850, 0.3, 0.0, 3001, 5, "OANDA"),
        DemoSymbol::new("GBPUSD", "British Pound / US Dollar", 1.2650, 0.4, -0.02, 3002, 5, "OANDA"),
        // Index (1)
        DemoSymbol::new("SPX", "S&P 500 Index", 4800.0, 0.8, 0.15, 4001, 2, "INDEX"),
    ]
}

/// Get demo symbol by ticker
pub fn get_demo_symbol(ticker: &str) -> Option<DemoSymbol> {
    demo_symbols().into_iter().find(|s| s.ticker() == ticker)
}

/// Get all demo symbol tickers
pub fn demo_symbol_tickers() -> Vec<&'static str> {
    vec!["AAPL", "TSLA", "NVDA", "MSFT", "BTCUSD", "ETHUSD", "SOLUSD", "EURUSD", "GBPUSD", "SPX"]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_demo_symbols_count() {
        assert_eq!(demo_symbols().len(), 10);
    }

    #[test]
    fn test_get_demo_symbol() {
        let btc = get_demo_symbol("BTCUSD").unwrap();
        assert_eq!(btc.base_price, 42000.0);
    }

    #[test]
    fn test_unique_seeds() {
        let symbols = demo_symbols();
        let seeds: Vec<u64> = symbols.iter().map(|s| s.seed_offset).collect();
        let mut unique_seeds = seeds.clone();
        unique_seeds.sort();
        unique_seeds.dedup();
        assert_eq!(seeds.len(), unique_seeds.len(), "All seeds should be unique");
    }
}
