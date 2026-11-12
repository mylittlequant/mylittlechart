//! Demo candles: a made-up, repeatable price walk shown when no exchange data is
//! there (headless tests, screenshots, offline). The screen labels them as demo.

use mlc_core::Bar;

/// Seconds per bar of each timeframe the screen offers.
pub const TIMEFRAMES: [(&str, i64); 6] = [("1m", 60), ("5m", 300), ("15m", 900), ("1h", 3600), ("4h", 14_400), ("1d", 86_400)];

/// `n` demo bars of `secs` seconds ending at `end` (unix seconds), starting near
/// `start_price`. Same arguments, same bars.
pub fn bars(n: usize, secs: i64, end: i64, start_price: f64, seed: u64) -> Vec<Bar> {
    let mut s = seed ^ 0x9E37_79B9_7F4A_7C15;
    let mut next = move || {
        // xorshift64*: deterministic on every target, no rand dependency.
        s ^= s >> 12;
        s ^= s << 25;
        s ^= s >> 27;
        (s.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
    };
    let vol = 0.004 * (secs as f64 / 3600.0).sqrt().clamp(0.25, 5.0);
    let mut price = start_price;
    let first = end - (n as i64 - 1) * secs;
    let first = first - first.rem_euclid(secs);
    (0..n)
        .map(|i| {
            let open = price;
            // A slow swing plus noise, so the chart has trends to pan and zoom into.
            let drift = (i as f64 / 37.0).sin() * vol * 0.35;
            let close = (open * (1.0 + drift + (next() - 0.5) * 2.0 * vol)).max(1.0);
            let high = open.max(close) * (1.0 + next() * vol * 0.6);
            let low = open.min(close) * (1.0 - next() * vol * 0.6);
            price = close;
            Bar { timestamp: first + i as i64 * secs, open, high, low, close, volume: 10.0 + next() * 90.0, ..Default::default() }
        })
        .collect()
}

/// The demo series of a timeframe index ([`TIMEFRAMES`]): 500 bars of BTC/USDT-like
/// prices ending at `end`.
pub fn series(tf: usize, end: i64) -> Vec<Bar> {
    series_for(tf, end, 64_000.0, 0)
}

/// The demo series of a timeframe index for another instrument: 500 bars from
/// `start_price`, `seed` making its walk its own.
pub fn series_for(tf: usize, end: i64, start_price: f64, seed: u64) -> Vec<Bar> {
    let (_, secs) = TIMEFRAMES[tf.min(TIMEFRAMES.len() - 1)];
    bars(500, secs, end, start_price, (tf as u64 + 1) ^ seed.wrapping_mul(0x100_0000_01B3))
}

/// A demo price level and seed for `symbol`: BTC near 64k, ETH near 3.2k, the rest
/// somewhere between 0.1 and 500, the same every time.
pub fn level_for(symbol: &str) -> (f64, u64) {
    let base = symbol.split(['/', '-', ' ']).next().unwrap_or(symbol).to_ascii_uppercase();
    match base.as_str() {
        "BTC" => (64_000.0, 0),
        "ETH" => (3_200.0, 2),
        _ => {
            // FNV-1a: stable on every target.
            let h = base.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01B3));
            let price = 10f64.powf(-1.0 + (h % 3700) as f64 / 1000.0);
            (price, h)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_bars_are_repeatable_consistent_and_on_the_grid() {
        let a = series(3, 1_762_900_000);
        let b = series(3, 1_762_900_000);
        assert_eq!(a.len(), 500);
        assert!(a.iter().zip(&b).all(|(x, y)| x.close == y.close && x.timestamp == y.timestamp));
        for w in a.windows(2) {
            assert_eq!(w[1].timestamp - w[0].timestamp, 3600);
            assert_eq!(w[1].open, w[0].close, "no gaps");
        }
        assert!(a.iter().all(|b| b.low <= b.open.min(b.close) && b.high >= b.open.max(b.close) && b.low > 0.0));
        assert_eq!(a[0].timestamp % 3600, 0);
    }
}
