//! Where the bars come from. The app asks a [`Source`] for a timeframe and drains
//! what arrived each frame. [`DemoSource`] answers at once with demo candles;
//! `live::LiveSource` (feature `live`) fetches them from an exchange through
//! mlc's data bridge and falls back to demo when that fails.

use mlc_core::Bar;

/// What a source delivers.
#[derive(Clone, Debug)]
pub enum Feed {
    /// The whole series of the requested timeframe (replaces the chart's bars).
    Bars { tf: String, bars: Vec<Bar>, live: bool },
    /// The last bar changed or a new bar opened.
    Bar(Bar),
    /// The source gave up (no network): the chart shows demo data.
    Offline,
}

/// A bar source for one symbol.
pub trait Source: 'static {
    /// Venue shown in the top bar.
    fn venue(&self) -> &str;
    /// Symbol shown in the top bar.
    fn symbol(&self) -> &str;
    /// Ask for timeframe `code` (`1m` … `1d`).
    fn request(&mut self, code: &str);
    /// What arrived since the last call.
    fn poll(&mut self) -> Vec<Feed>;
    /// Called once with a way to wake the app when data arrives off-frame.
    fn set_waker(&mut self, _wake: Box<dyn Fn() + Send + Sync>) {}
}

/// Demo candles, no network.
pub struct DemoSource {
    pending: Vec<Feed>,
    /// End of the demo series (unix seconds); fixed for repeatable screenshots.
    pub end: i64,
    symbol: String,
    venue: String,
    level: (f64, u64),
}

impl Default for DemoSource {
    fn default() -> Self {
        Self::new(0)
    }
}

impl DemoSource {
    /// Binance BTC/USDT.
    pub fn new(end: i64) -> Self {
        Self::for_symbol("BTC/USDT", "Binance", end)
    }

    /// Demo candles for `symbol` on `venue`, at a price level of its own.
    pub fn for_symbol(symbol: &str, venue: &str, end: i64) -> Self {
        Self { pending: Vec::new(), end, symbol: symbol.to_string(), venue: venue.to_string(), level: crate::demo::level_for(symbol) }
    }
}

impl Source for DemoSource {
    fn venue(&self) -> &str {
        &self.venue
    }
    fn symbol(&self) -> &str {
        &self.symbol
    }
    fn request(&mut self, code: &str) {
        let tf = crate::demo::TIMEFRAMES.iter().position(|(c, _)| *c == code).unwrap_or(3);
        let (price, seed) = self.level;
        self.pending.push(Feed::Bars { tf: code.to_string(), bars: crate::demo::series_for(tf, self.end, price, seed), live: false });
    }
    fn poll(&mut self) -> Vec<Feed> {
        std::mem::take(&mut self.pending)
    }
}
