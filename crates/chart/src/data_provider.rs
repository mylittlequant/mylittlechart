//! Data Provider Trait
//!
//! Abstracts data loading for ChartWindow, allowing different data sources
//! (demo, live exchange, historical files) to be plugged in without coupling
//! the chart state to any specific data source.

use std::sync::Arc;
use crate::{Bar, state::Timeframe};

/// Trait for loading OHLC data for symbols and timeframes.
///
/// This trait allows `ChartWindow` to be decoupled from specific data sources.
/// Implementations can provide demo data, live exchange data, or historical files.
///
/// # Example
///
/// ```ignore
/// // Demo data provider
/// let provider = Arc::new(DemoDataProvider);
/// let mut window = ChartWindow::new_with_provider(provider);
/// window.change_symbol("BTCUSD"); // Uses provider to load data
///
/// // Live exchange provider
/// let provider = Arc::new(ExchangeDataProvider::new(api_client));
/// let mut window = ChartWindow::new_with_provider(provider);
/// ```
pub trait DataProvider: Send + Sync {
    /// Load bars for a given symbol and timeframe.
    ///
    /// Returns `None` if the symbol/timeframe combination is not available.
    fn get_bars(&self, symbol: &str, timeframe: &Timeframe) -> Option<Vec<Bar>>;

    /// Check if a symbol is available in this provider.
    ///
    /// Default implementation tries to load bars and checks if successful.
    fn has_symbol(&self, symbol: &str) -> bool {
        // Try with a common timeframe
        self.get_bars(symbol, &Timeframe::h1()).is_some()
    }

    /// Get available symbols from this provider.
    ///
    /// Default implementation returns empty list - override for enumerable providers.
    fn available_symbols(&self) -> Vec<String> {
        Vec::new()
    }

    /// Get available timeframes for a symbol.
    ///
    /// Default implementation returns common timeframes - override for provider-specific lists.
    fn available_timeframes(&self, _symbol: &str) -> Vec<Timeframe> {
        vec![
            Timeframe::m1(),
            Timeframe::m5(),
            Timeframe::m15(),
            Timeframe::m30(),
            Timeframe::h1(),
            Timeframe::h4(),
            Timeframe::d1(),
            Timeframe::w1(),
        ]
    }

    /// Get exchange/provider name for a symbol.
    ///
    /// Returns the exchange or data source name (e.g. "Binance", "NASDAQ", "Demo").
    /// Default returns empty string.
    fn exchange_name(&self, _symbol: &str) -> String {
        String::new()
    }
}

/// Shared reference to a data provider.
pub type SharedDataProvider = Arc<dyn DataProvider>;

/// A null data provider that returns no data.
///
/// Useful for testing or when no data source is configured.
pub struct NullDataProvider;

impl DataProvider for NullDataProvider {
    fn get_bars(&self, _symbol: &str, _timeframe: &Timeframe) -> Option<Vec<Bar>> {
        None
    }

    fn has_symbol(&self, _symbol: &str) -> bool {
        false
    }
}
