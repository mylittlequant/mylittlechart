//! Conversion utilities between V5 connector types and chart types.

use digdigdig3::core::types::Kline;
use mlc_core::instrument::{AccountType, InstrumentAddress};
use mlc_core::Bar;
use mlc_core::state::Timeframe;

// =============================================================================
// InstrumentAddress <-> digdigdig3 (exchange, account_type)
// =============================================================================

/// Error returned by [`resolve_exchange`] / [`to_dig3`] when the address's
/// `exchange` field doesn't match any known `digdigdig3::ExchangeId`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownExchange(pub String);

impl std::fmt::Display for UnknownExchange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown exchange: {:?}", self.0)
    }
}

impl std::error::Error for UnknownExchange {}

/// Convert [`mlc_core::instrument::AccountType`] to [`digdigdig3::AccountType`].
///
/// Infallible — the two enums are variant-for-variant identical (see
/// `docs/mlc/plans/doctrine-cutover-2026-06-27.md` §1C).
pub fn account_type_to_dig3(at: AccountType) -> digdigdig3::AccountType {
    match at {
        AccountType::Spot => digdigdig3::AccountType::Spot,
        AccountType::Margin => digdigdig3::AccountType::Margin,
        AccountType::FuturesCross => digdigdig3::AccountType::FuturesCross,
        AccountType::FuturesIsolated => digdigdig3::AccountType::FuturesIsolated,
        AccountType::Earn => digdigdig3::AccountType::Earn,
        AccountType::Lending => digdigdig3::AccountType::Lending,
        AccountType::Options => digdigdig3::AccountType::Options,
        AccountType::Convert => digdigdig3::AccountType::Convert,
    }
}

/// Convert [`digdigdig3::AccountType`] to [`mlc_core::instrument::AccountType`].
///
/// Infallible — the two enums are variant-for-variant identical.
pub fn account_type_from_dig3(at: digdigdig3::AccountType) -> AccountType {
    match at {
        digdigdig3::AccountType::Spot => AccountType::Spot,
        digdigdig3::AccountType::Margin => AccountType::Margin,
        digdigdig3::AccountType::FuturesCross => AccountType::FuturesCross,
        digdigdig3::AccountType::FuturesIsolated => AccountType::FuturesIsolated,
        digdigdig3::AccountType::Earn => AccountType::Earn,
        digdigdig3::AccountType::Lending => AccountType::Lending,
        digdigdig3::AccountType::Options => AccountType::Options,
        digdigdig3::AccountType::Convert => AccountType::Convert,
    }
}

/// Resolve an [`InstrumentAddress`]'s `exchange` field to a
/// [`digdigdig3::ExchangeId`]. The single fallible entry point for the
/// mlc-core -> dig3 boundary — `mlc-core` never imports `digdigdig3`, so
/// every exchange-name lookup funnels through here.
pub fn resolve_exchange(address: &InstrumentAddress) -> Result<digdigdig3::ExchangeId, UnknownExchange> {
    digdigdig3::ExchangeId::from_str(&address.exchange).ok_or_else(|| UnknownExchange(address.exchange.clone()))
}

/// Convert an [`InstrumentAddress`] to the `(ExchangeId, AccountType)` pair
/// `digdigdig3` connectors expect. Fails only when `address.exchange`
/// doesn't resolve to a known exchange.
pub fn to_dig3(address: &InstrumentAddress) -> Result<(digdigdig3::ExchangeId, digdigdig3::AccountType), UnknownExchange> {
    Ok((resolve_exchange(address)?, account_type_to_dig3(address.account_type)))
}

/// Convert a V5 `Kline` to a chart `Bar`.
///
/// `Kline.open_time` is in milliseconds; `Bar.timestamp` is in seconds.
pub fn kline_to_bar(kline: &Kline) -> Bar {
    let ts = kline.open_time / 1000;
    if ts < 1_000_000_000 || ts > 9_999_999_999 {
        crate::dlog!(
            "[convert] WARNING: suspicious bar timestamp {} (from open_time={})",
            ts, kline.open_time
        );
    }
    Bar {
        timestamp: ts,
        open: kline.open,
        high: kline.high,
        low: kline.low,
        close: kline.close,
        volume: kline.volume,
        derived_ts_ms: kline.open_time,
    }
}

/// Convert a chart timeframe NAME to the exchange kline interval label.
///
/// Chart uses uppercase for hours+ (`"1H"`, `"4H"`, `"1D"`, `"1W"`, `"1M"`);
/// exchanges use lowercase for all of those EXCEPT month — `"1M"` (month)
/// vs `"1m"` (minute) is the one case-SENSITIVE pair on the wire (Binance
/// et al). The old blanket `to_lowercase()` collapsed the pair: picking the
/// 1-month TF subscribed a 1-minute feed (owner report 2026-07-05).
pub fn interval_label(tf_name: &str) -> String {
    if tf_name.ends_with('M') {
        tf_name.to_string()
    } else {
        tf_name.to_lowercase()
    }
}

/// Convert a chart `Timeframe` to a V5 kline interval string. See
/// [`interval_label`] for the month case-sensitivity contract.
pub fn timeframe_to_interval(tf: &Timeframe) -> String {
    interval_label(&tf.name)
}

/// Duration in milliseconds for a canonical dig3 kline-interval string
/// (`"1s"`, `"5m"`, `"2h"`, `"1d"`, `"1w"`, `"1M"`, ...) — the numeric-suffix
/// convention every `KlineIntervalCapabilities::intervals_for` entry and
/// [`interval_label`]'s output both follow. Used by
/// `crate::kline_agg::aggregation_base`'s callers to turn a venue's native
/// interval-string set into `(label, duration_ms)` pairs.
///
/// Mirrors `mlc_app::active_series::dig3_interval_to_ms` (the dropdown-side
/// twin of this function — `mlc-app` sits above this crate in the layering
/// and cannot be depended on from here, so the same closed-form logic is
/// duplicated at this lower layer rather than shared).
///
/// Month (`M`) uses a fixed 30-day approximation for divisibility purposes
/// only (calendar months vary 28-31 days, so no exact millisecond count
/// exists) — this only affects whether some OTHER interval divides evenly
/// into "1 month", a comparison that in practice never fires for the
/// second/minute/hour/day/week intervals this module actually aggregates.
///
/// Returns `None` for a suffix outside the `s/m/h/d/w/M` set, an empty
/// string, a non-numeric prefix, or a non-positive count.
pub fn interval_to_ms(interval: &str) -> Option<i64> {
    let bytes = interval.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let unit = *bytes.last()?;
    let n_str = std::str::from_utf8(&bytes[..bytes.len().saturating_sub(1)]).ok()?;
    let n: i64 = n_str.parse().ok()?;
    if n <= 0 {
        return None;
    }
    const SEC: i64 = 1_000;
    const MIN: i64 = 60 * SEC;
    const HOUR: i64 = 60 * MIN;
    const DAY: i64 = 24 * HOUR;
    match unit {
        b's' => Some(n * SEC),
        b'm' => Some(n * MIN),
        b'h' => Some(n * HOUR),
        b'd' => Some(n * DAY),
        b'w' => Some(n * 7 * DAY),
        b'M' => Some(n * 30 * DAY),
        _ => None,
    }
}

#[cfg(test)]
mod interval_to_ms_tests {
    use super::*;

    #[test]
    fn parses_every_unit() {
        assert_eq!(interval_to_ms("1s"), Some(1_000));
        assert_eq!(interval_to_ms("5m"), Some(300_000));
        assert_eq!(interval_to_ms("2h"), Some(7_200_000));
        assert_eq!(interval_to_ms("1d"), Some(86_400_000));
        assert_eq!(interval_to_ms("1w"), Some(604_800_000));
        assert_eq!(interval_to_ms("1M"), Some(2_592_000_000));
    }

    #[test]
    fn rejects_malformed_input() {
        assert_eq!(interval_to_ms(""), None);
        assert_eq!(interval_to_ms("m"), None);
        assert_eq!(interval_to_ms("0m"), None);
        assert_eq!(interval_to_ms("-5m"), None);
        assert_eq!(interval_to_ms("5x"), None);
    }

    #[test]
    fn month_vs_minute_case_sensitivity_preserved() {
        // "1M" (month) and "1m" (minute) must resolve to different durations
        // — the one case-sensitive pair on the wire (see `interval_label`).
        assert_ne!(interval_to_ms("1M"), interval_to_ms("1m"));
    }
}
