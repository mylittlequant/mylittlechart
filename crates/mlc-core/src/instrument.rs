//! `InstrumentAddress` — the canonical instrument key.
//!
//! Two-story key, mirroring dig3-station's `SeriesKey = (exchange, account,
//! symbol) + kind(tf inside)`.  Timeframe is a series axis, NOT part of
//! instrument identity — consumers that need TF compose
//! `{ address: InstrumentAddress, timeframe: Timeframe }` (ex-`InstrumentKey`).
//!
//! `mlc-core` never imports `digdigdig3` — conversion to dig3 types lives at
//! the data-bridge boundary (`mlc-backend-data-bridge`).
//!
//! See `docs/mlc/plans/doctrine-cutover-2026-06-27.md` §1C.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

// =============================================================================
// AccountType
// =============================================================================

/// Type of trading account/market.
///
/// Serializes as the short label used throughout persisted state, bubble
/// metadata, and map keys (`"S"`, `"M"`, `"F"`, `"FI"`, `"E"`, `"L"`, `"O"`,
/// `"CV"`) — persistence-compatible with the pre-cutover `String` fields.
///
/// Variant names mirror `digdigdig3::AccountType` (Spot / Margin /
/// FuturesCross / FuturesIsolated / Earn / Lending / Options / Convert,
/// see `digdigdig3/crates/dig3-core/src/core/types/common.rs`). Conversion
/// to/from `digdigdig3::AccountType` lives at the data-bridge boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum AccountType {
    /// Spot trading.
    #[default]
    Spot,
    /// Margin trading.
    Margin,
    /// Futures with cross margin.
    FuturesCross,
    /// Futures with isolated margin.
    FuturesIsolated,
    /// Earn / savings / staking account.
    Earn,
    /// Lending / margin lending account.
    Lending,
    /// Options account.
    Options,
    /// Convert (instant swap) account.
    Convert,
}

impl AccountType {
    /// Short label used in persisted state, bubble metadata, and map keys.
    /// Inverse of [`AccountType::from_str`] (infallible direction).
    pub fn as_label(&self) -> &'static str {
        match self {
            AccountType::Spot => "S",
            AccountType::Margin => "M",
            AccountType::FuturesCross => "F",
            AccountType::FuturesIsolated => "FI",
            AccountType::Earn => "E",
            AccountType::Lending => "L",
            AccountType::Options => "O",
            AccountType::Convert => "CV",
        }
    }
}

impl fmt::Display for AccountType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_label())
    }
}

/// Parses the short label form. Unknown or empty labels fall back to
/// `AccountType::Spot` (backward compat with pre-account_type persisted
/// data) — this fn never fails, matching the legacy `account_type_from_label`
/// behaviour it replaces.
impl FromStr for AccountType {
    type Err = std::convert::Infallible;

    fn from_str(label: &str) -> Result<Self, Self::Err> {
        Ok(match label {
            "M" => AccountType::Margin,
            "F" | "FC" => AccountType::FuturesCross,
            "FI" => AccountType::FuturesIsolated,
            "E" => AccountType::Earn,
            "L" => AccountType::Lending,
            "O" => AccountType::Options,
            "C" | "CV" => AccountType::Convert,
            _ => AccountType::Spot, // "S" and unknown -> Spot (backward compat).
        })
    }
}

impl Serialize for AccountType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_label())
    }
}

impl<'de> Deserialize<'de> for AccountType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let label = String::deserialize(deserializer)?;
        // `FromStr::from_str` is infallible (unknown -> Spot fallback).
        Ok(label.parse().unwrap_or_default())
    }
}

// =============================================================================
// InstrumentAddress
// =============================================================================

/// Canonical `(exchange, symbol, account_type)` identity of a tradable
/// instrument. Timeframe is NOT part of the address — series consumers that
/// need TF compose `{ address, timeframe }` at their own layer.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct InstrumentAddress {
    /// Exchange name (e.g. `"binance"`).
    ///
    /// `#[serde(default)]` so structs that embed this via `#[serde(flatten)]`
    /// (e.g. `AlertItem`) keep loading pre-cutover snapshots that predate the
    /// `exchange` field.
    #[serde(default)]
    pub exchange: String,
    /// Symbol (e.g. `"BTCUSDT"`).
    #[serde(default)]
    pub symbol: String,
    /// Account type (Spot / Margin / Futures / ...).
    ///
    /// `#[serde(default)]` defaults to `AccountType::Spot` for pre-cutover
    /// snapshots that predate the `account_type` field.
    #[serde(default)]
    pub account_type: AccountType,
}

impl InstrumentAddress {
    /// Construct a new instrument address.
    pub fn new(exchange: impl Into<String>, symbol: impl Into<String>, account_type: AccountType) -> Self {
        Self {
            exchange: exchange.into(),
            symbol: symbol.into(),
            account_type,
        }
    }

    /// Returns `"exchange:symbol:account_type"` routing key for per-symbol
    /// crossing detection / map keys.
    ///
    /// MUST stay byte-for-byte identical to the pre-cutover
    /// `AlertItem::routing_key()` format: falls back to just the symbol
    /// when `exchange` is empty (old presets without an exchange).
    pub fn routing_key(&self) -> String {
        if self.exchange.is_empty() {
            self.symbol.clone()
        } else {
            format!("{}:{}:{}", self.exchange, self.symbol, self.account_type)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_type_label_roundtrip() {
        for at in [
            AccountType::Spot,
            AccountType::Margin,
            AccountType::FuturesCross,
            AccountType::FuturesIsolated,
            AccountType::Earn,
            AccountType::Lending,
            AccountType::Options,
            AccountType::Convert,
        ] {
            let label = at.as_label();
            let parsed: AccountType = label.parse().unwrap_or_default();
            assert_eq!(parsed, at, "round-trip failed for {label}");
        }
    }

    #[test]
    fn account_type_legacy_aliases() {
        assert_eq!("FC".parse::<AccountType>().unwrap_or_default(), AccountType::FuturesCross);
        assert_eq!("C".parse::<AccountType>().unwrap_or_default(), AccountType::Convert);
        assert_eq!("".parse::<AccountType>().unwrap_or_default(), AccountType::Spot);
        assert_eq!("bogus".parse::<AccountType>().unwrap_or_default(), AccountType::Spot);
    }

    #[test]
    fn account_type_serde_short_label() {
        let json = serde_json::to_string(&AccountType::FuturesIsolated).unwrap();
        assert_eq!(json, "\"FI\"");
        let back: AccountType = serde_json::from_str(&json).unwrap();
        assert_eq!(back, AccountType::FuturesIsolated);
    }

    #[test]
    fn routing_key_matches_legacy_format() {
        let addr = InstrumentAddress::new("Binance", "BTCUSDT", AccountType::FuturesCross);
        assert_eq!(addr.routing_key(), "Binance:BTCUSDT:F");

        let addr_no_exchange = InstrumentAddress::new("", "BTCUSDT", AccountType::Spot);
        assert_eq!(addr_no_exchange.routing_key(), "BTCUSDT");
    }
}
