//! Conversion utilities between V5 connector types and chart types.

use digdigdig3::core::types::Kline;
use zengeld_chart::Bar;
use zengeld_chart::state::Timeframe;

/// Convert a V5 `Kline` to a chart `Bar`.
///
/// `Kline.open_time` is in milliseconds; `Bar.timestamp` is in seconds.
pub fn kline_to_bar(kline: &Kline) -> Bar {
    Bar {
        timestamp: kline.open_time / 1000,
        open: kline.open,
        high: kline.high,
        low: kline.low,
        close: kline.close,
        volume: kline.volume,
    }
}

/// Convert a chart `Timeframe` name to a V5 kline interval string.
///
/// Chart uses uppercase for hours+ (`"1H"`, `"4H"`, `"1D"`, `"1W"`, `"1M"`).
/// Most exchanges use lowercase (`"1h"`, `"4h"`, `"1d"`, `"1w"`, `"1M"`).
pub fn timeframe_to_interval(tf: &Timeframe) -> String {
    tf.name.to_lowercase()
}
