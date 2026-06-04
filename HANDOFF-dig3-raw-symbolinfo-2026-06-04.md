# Handoff → mylittlechart team: dig3 SymbolInfo is now RAW (2026-06-04)

dig3 (`digdigdig3` `=0.3.15`, local — not yet bumped/published) changed the
`get_exchange_info` / `SymbolInfo` contract to be **raw + complete**. This
affects mylittlechart's symbol search directly. Two items for you: a behavior
adaptation (REQUIRED, or search fills with junk) and the long-standing
incomplete-HL-tickers bug (the original symptom).

## What changed in dig3 (context)

- **`get_exchange_info` no longer filters.** Every connector now returns ALL
  symbols — including non-trading ones: `PreLaunch`, `delisting`, `expired`,
  `settled`, `halted`, `offline`, `inactive`, etc. Previously dig3 dropped
  everything except status==TRADING. **Your search universe will grow and will
  now contain delisted/pre-launch symbols unless you filter.**
- **`SymbolInfo.status` is the venue-NATIVE string** verbatim now (`"online"`,
  `"Trading"`, `"live"`, `"tradable"`, `"active"`, `"1"`, `"delisted"`, ...) —
  no longer a synthetic `"TRADING"`.
- **`SymbolInfo` gained two fields:**
  - `instrument_type: Option<String>` — native instrument/asset-class token
    (OKX `SWAP/FUTURES/OPTION/SPOT`, Deribit `future/option`, Bybit
    `LinearPerpetual`, Alpaca `us_equity/crypto`, Tinkoff `share/bond/future/
    currency/etf`, India brokers segment, ...). This is the per-ticker asset
    info — use it for grouping/labels.
  - `extra: serde_json::Value` — the full raw native symbol record (nothing the
    exchange returned is lost; e.g. contractType, expiry, strike, leverage,
    permissions, `isDelisted`, broker `class`, ...).
- **dig3 stays RAW; normalization is opt-in in `digdigdig3-station`:**
  - `digdigdig3_station::active_only(Vec<SymbolInfo>) -> Vec<SymbolInfo>` — keep
    only currently-tradable symbols (the filter dig3 used to apply inline).
  - `digdigdig3_station::canonical_status(&SymbolInfo) -> SymbolStatus`
    (`Trading/Halted/PreLaunch/Closed/Unknown`) — venue-agnostic status.
  - `digdigdig3_station::is_active(&SymbolInfo) -> bool`.

## Item 1 (REQUIRED) — apply station normalization in search

`build_symbol_search_results` (`chart-app/src/lib.rs:~5777`) currently has NO
asset-class filter and only labels `moex → "Stock"`, everything else `"Crypto"`.
With raw dig3 it will now also surface delisted/pre-launch symbols. Adopt:

- Filter the universe with `digdigdig3_station::active_only(...)` (or
  `is_active`) when building the search index — OR surface status to the user
  and let them toggle. Don't show `Closed`/`PreLaunch` as if tradable.
- Replace the `moex→Stock / else→Crypto` hack with `SymbolInfo.instrument_type`
  (per-ticker, native) + `canonical_status`. The connector folder (cex/dex) and
  exchange identity must NOT decide asset class — `instrument_type` does. A
  tokenized-stock or real equity on any venue is classed by its ticker.
- Where you need a coarse class for tabs (Crypto vs Stock vs FX vs Bond), derive
  it from `instrument_type` + the venue's `ConnectorMetadata.category` as a
  fallback — NOT from a hardcoded exchange→class map.

## Item 2 (the original bug) — silent partial-fetch drops half a venue

`live-data/src/bridge.rs:~995-1023` fetches a venue's symbols with two
independent calls (spot + perp/futures) and swallows either failure:

```rust
if let Ok(spot)    = connector.get_exchange_info(AccountType::Spot).await { ... }
if let Ok(futures) = connector.get_exchange_info(AccountType::FuturesCross).await { ... }
```

If the spot call fails (network / geo / rate-limit) only the perp leg lands —
e.g. HyperLiquid then shows ~200 perps but loses its ~300+ spot pairs, with no
retry and no UI feedback. **This is the "not all HL tickers in search" symptom.**

Fix:
- Fetch each market leg independently; on a leg error, retry once, then log +
  surface a partial-load warning — never drop the whole venue because one leg
  failed.
- Cache partial results with a `partial: bool` flag so a later success can fill
  the gap; consider re-requesting the missing leg on next warmup tick.
- Applies to every dual-market venue (Binance spot+futures, OKX, Bybit, ...),
  not just HL.

## Notes

- mylittlechart is already on `digdigdig3 = "=0.3.15"` (`live-data/Cargo.toml`,
  `chart-app/Cargo.toml`) — same workspace pin, no version bump needed; the
  "MLC pinned on 0.1.32" note elsewhere is stale.
- HyperLiquid is now under `dig3 .../crypto/dex/hyperliquid/` (moved from `cex/`)
  and classed `ExchangeType::Dex` — no API change for you, just FYI.
- dig3 changes are LOCAL (not pushed / not bumped). Coordinate the bump before
  relying on these in a published build.

Reference: dig3 `examples/symbolinfo_raw_e2e.rs` (live proof, 7 venues) and
`digdigdig3/CLAUDE.md` → "RAW core / normalized station boundary".
