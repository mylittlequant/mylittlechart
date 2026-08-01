//! Crypto screener v1 — WATCHLIST mode (owner spec 2026-08-01, plan F,
//! `docs/mlc/plans/orderflow-panels-arc2-2026-08-01.md` §F).
//!
//! Pure compute + a cache, deliberately NOT a full manager+facade domain
//! (unlike watchlist/tag/preset/...): there is no user-editable state here,
//! only a read-through cache of derived metrics keyed the same way as
//! [`crate::MiniTickerData`]'s `mini_ticker` map (`"{symbol}:{exchange}:{at}"`).
//! Same rung-4 shape as `mini_ticker`/`trade_store` (doctrine §6) — the
//! `Arc<DashMap<...>>` lives directly on `BackendCore` and is written from a
//! detached background task rather than from inside `tick_writers`, which is
//! why it needs its own `Arc` (unlike `mini_ticker`, which is always written
//! synchronously from `&self`).
//!
//! RSI(14)/ATR(14) are computed here in plain Rust (Wilder smoothing) — no
//! `mli` dependency, per owner instruction. Distance-to-24h-high/low is
//! derived from the SAME fetched 1h series (last 24 bars) rather than from
//! the exchange ticker's `high_24h`/`low_24h`, so all four screener metrics
//! stay mutually consistent and refresh together on every re-Scan.

use mlc_core::Bar;

/// Cached per-instrument compute metrics for the screener.
///
/// Computed once per manual "Scan" (background history fetch of ~120 1h
/// bars) and cached until the next Scan — never recomputed per-frame.
#[derive(Clone, Copy, Debug)]
pub struct ScreenerMetrics {
    /// RSI(14), Wilder smoothing, over 1h closes. `None` when fewer than 15
    /// closes were available.
    pub rsi14: Option<f64>,
    /// ATR(14) / last_close * 100 — average true range as a percentage of
    /// the series' last close.
    pub atr_pct14: Option<f64>,
    /// Distance from the last close UP to the highest high of the last 24
    /// bars (hourly-bar approximation of "24h high"), as a percentage.
    pub dist_high_pct: Option<f64>,
    /// Distance from the last close DOWN to the lowest low of the last 24
    /// bars, as a percentage.
    pub dist_low_pct: Option<f64>,
    /// Wall-clock ms (`web_time::SystemTime::now()`) when this entry was
    /// computed — rendered next to the panel header so the user can see how
    /// stale the current Scan is.
    pub computed_at_ms: i64,
}

/// Wilder-smoothed RSI over `closes` (oldest→newest).
///
/// Returns `None` when fewer than `period + 1` closes are available (not
/// enough deltas to seed the first average gain/loss).
pub fn compute_rsi(closes: &[f64], period: usize) -> Option<f64> {
    if period == 0 || closes.len() < period + 1 {
        return None;
    }
    let mut avg_gain = 0.0;
    let mut avg_loss = 0.0;
    for i in 1..=period {
        let delta = closes[i] - closes[i - 1];
        if delta >= 0.0 {
            avg_gain += delta;
        } else {
            avg_loss -= delta;
        }
    }
    avg_gain /= period as f64;
    avg_loss /= period as f64;

    let period_f = period as f64;
    for i in (period + 1)..closes.len() {
        let delta = closes[i] - closes[i - 1];
        let (gain, loss) = if delta >= 0.0 { (delta, 0.0) } else { (0.0, -delta) };
        avg_gain = (avg_gain * (period_f - 1.0) + gain) / period_f;
        avg_loss = (avg_loss * (period_f - 1.0) + loss) / period_f;
    }

    if avg_loss <= f64::EPSILON {
        return Some(100.0);
    }
    let rs = avg_gain / avg_loss;
    Some(100.0 - (100.0 / (1.0 + rs)))
}

/// Wilder-smoothed ATR over `bars` (oldest→newest).
///
/// Returns `None` when fewer than `period + 1` bars are available (the
/// first true-range value needs a previous close).
pub fn compute_atr(bars: &[Bar], period: usize) -> Option<f64> {
    if period == 0 || bars.len() < period + 1 {
        return None;
    }
    let true_range = |i: usize| -> f64 {
        let high = bars[i].high;
        let low = bars[i].low;
        let prev_close = bars[i - 1].close;
        (high - low).max((high - prev_close).abs()).max((low - prev_close).abs())
    };

    let period_f = period as f64;
    let mut atr: f64 = (1..=period).map(true_range).sum::<f64>() / period_f;
    for i in (period + 1)..bars.len() {
        let tr = true_range(i);
        atr = (atr * (period_f - 1.0) + tr) / period_f;
    }
    Some(atr)
}

/// Build a full [`ScreenerMetrics`] snapshot from a page of 1h bars
/// (oldest→newest, as returned by the read-only history fetch).
///
/// Returns `None` when there are not enough bars to seed RSI/ATR(14)
/// (needs at least 15).
pub fn compute_screener_metrics(bars: &[Bar]) -> Option<ScreenerMetrics> {
    if bars.len() < 15 {
        return None;
    }
    let closes: Vec<f64> = bars.iter().map(|b| b.close).collect();
    let last_close = *closes.last()?;

    let rsi14 = compute_rsi(&closes, 14);
    let atr_pct14 = if last_close.abs() > f64::EPSILON {
        compute_atr(bars, 14).map(|atr| atr / last_close * 100.0)
    } else {
        None
    };

    let window_start = bars.len().saturating_sub(24);
    let window_24 = &bars[window_start..];
    let high_24 = window_24.iter().map(|b| b.high).fold(f64::MIN, f64::max);
    let low_24 = window_24.iter().map(|b| b.low).fold(f64::MAX, f64::min);
    let (dist_high_pct, dist_low_pct) = if last_close.abs() > f64::EPSILON {
        (
            Some((high_24 - last_close) / last_close * 100.0),
            Some((last_close - low_24) / last_close * 100.0),
        )
    } else {
        (None, None)
    };

    let computed_at_ms = web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);

    Some(ScreenerMetrics {
        rsi14,
        atr_pct14,
        dist_high_pct,
        dist_low_pct,
        computed_at_ms,
    })
}

/// Build the screener cache key — SAME grammar as `BackendCore::mini_ticker`
/// (`"{symbol}:{exchange}:{account_type}"`) so both maps can be looked up
/// from the same identity triple without reformatting.
pub fn screener_key(symbol: &str, exchange: &str, account_type_label: &str) -> String {
    format!("{}:{}:{}", symbol, exchange, account_type_label)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(close: f64, high: f64, low: f64) -> Bar {
        Bar {
            open: close,
            high,
            low,
            close,
            ..Bar::default()
        }
    }

    #[test]
    fn rsi_all_gains_is_100() {
        let closes: Vec<f64> = (0..20).map(|i| 100.0 + i as f64).collect();
        let rsi = compute_rsi(&closes, 14).expect("enough closes");
        assert!((rsi - 100.0).abs() < 1e-9, "all-gains RSI must be 100, got {rsi}");
    }

    #[test]
    fn rsi_all_losses_is_0() {
        let closes: Vec<f64> = (0..20).map(|i| 200.0 - i as f64).collect();
        let rsi = compute_rsi(&closes, 14).expect("enough closes");
        assert!(rsi.abs() < 1e-9, "all-losses RSI must be 0, got {rsi}");
    }

    #[test]
    fn rsi_none_when_too_short() {
        let closes = vec![1.0, 2.0, 3.0];
        assert_eq!(compute_rsi(&closes, 14), None);
    }

    #[test]
    fn atr_flat_bars_is_zero() {
        let bars: Vec<Bar> = (0..20).map(|_| bar(100.0, 100.0, 100.0)).collect();
        let atr = compute_atr(&bars, 14).expect("enough bars");
        assert!(atr.abs() < 1e-9, "flat bars ATR must be 0, got {atr}");
    }

    #[test]
    fn atr_none_when_too_short() {
        let bars: Vec<Bar> = (0..5).map(|_| bar(100.0, 101.0, 99.0)).collect();
        assert_eq!(compute_atr(&bars, 14), None);
    }

    #[test]
    fn metrics_none_when_too_few_bars() {
        let bars: Vec<Bar> = (0..10).map(|_| bar(100.0, 101.0, 99.0)).collect();
        assert!(compute_screener_metrics(&bars).is_none());
    }

    #[test]
    fn metrics_dist_high_low_from_last_24_bars() {
        // 30 bars: first 6 have a spike to 200/50 (outside the 24-bar
        // window), last 24 stay within 100..=110 / 95..=100 — dist must
        // reflect the last-24 window only, not the whole 30.
        let mut bars: Vec<Bar> = Vec::new();
        for _ in 0..6 {
            bars.push(bar(150.0, 200.0, 50.0));
        }
        for i in 0..24 {
            let c = 100.0 + (i % 5) as f64;
            bars.push(bar(c, c + 2.0, c - 2.0));
        }
        let metrics = compute_screener_metrics(&bars).expect("enough bars");
        let last_close = bars.last().expect("non-empty").close;
        let expected_high = (100.0 + 4.0 + 2.0 - last_close) / last_close * 100.0;
        assert!(metrics.dist_high_pct.expect("some") < 50.0, "must not be pulled by the 200 spike outside the window");
        assert!((metrics.dist_high_pct.unwrap() - expected_high).abs() < 1.0);
    }
}
