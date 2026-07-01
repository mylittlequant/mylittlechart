# zengeld-panels

Trimmed mirror of the panel collection from **zengeld-terminal**, scoped to
execution-heavy trading panels that belong inside the chart app.

## Source of truth

The full panel collection (trading + info + visual, ~99 panels) lives in:

```
zengeld-terminal/crates/
  trading-panels/
  info-panels/
  visual-panels/
```

This crate was seeded by copying those three crates wholesale in Phase 4-new
of the sidebar docking refactor. Nothing here is original — it is a local
working copy so the chart app can depend on panel state types without
reaching across into the terminal workspace.

## What we actually wire into the chart

Only panels with **real execution-input logic** (DOM-linked, click-to-trade,
text editing, filter pipelines, composite layouts) are wired into the
sidebar `FreeItem` enum. The current keep-list is:

| Panel | File | Why kept |
|-------|------|----------|
| DOM | `trading/order_flow/dom.rs` | Price ladder, user orders, click-to-trade |
| Footprint | `trading/order_flow/footprint.rs` | Imbalance detection, 5 display modes |
| Volume Profile | `trading/order_flow/volume_profile.rs` | POC/VAH/VAL, value area |
| Liquidity Heatmap | `trading/order_flow/liquidity_heatmap.rs` | Snapshot viewport, intensity gradient |
| Big Trades | `trading/order_flow/big_trades.rs` | Size/notional filters, flash animation |
| L2 Tape | `trading/order_flow/l2_tape.rs` | MBO events, spoof alerts |
| Order Entry | `trading/trading/order_entry.rs` | Full inline text editing, validation |
| Position Manager | `trading/trading/position_manager.rs` | Risk levels, edit modes |
| Trade Log | `trading/trading/trade_log.rs` | Time/symbol filters, column sort |
| Risk Calculator | `trading/trading/risk_calculator.rs` | Real `calculate()` with R:R, margin |
| Trading Container | `trading/trading/trading_container.rs` | Composite DOM + sub-panel layout |

Everything else in `src/trading/`, `src/info/`, `src/visual/` is present in
the source tree but **not exported** from `FreeItem` — it's retained as a
compile-checked reference so future panels can be wired without re-copying.

## Panels deliberately NOT brought here

Anything that is a pure **read-only dashboard** (news, calendar, utility,
analytics, scientific plots, hierarchy/network viz, etc.) stays in
zengeld-terminal as an external warehouse of unfinished panels. If we ever
need one, we copy it back — migration is trivial because:

- All panels use the same `RenderContext` trait (re-exported here as
  `crate::render::RenderContext` from `zengeld-chart`).
- State structs are pure data (serde + HashMap/Vec), no async, no I/O.
- Renderers are stateless functions taking `&mut RenderContext`.

No traits need rewriting, no API translation layer — just `cp` + fix a
handful of `use` paths.

## Skeletons encountered during audit

These were candidates that turned out to be `f64 → String` formatters with
no interaction logic. Left in place (physically present in source tree) but
not wired:

- `info/portfolio/account_summary.rs` — 5 metrics, no logic
- `info/options/greeks_panel.rs` — display of 9 Greeks, no calc
- `trading/market_data/watchlist.rs` — HashMap + format_cell
- `trading/market_data/time_sales.rs` — VecDeque + format_trade
- `info/options/options_chain.rs` — Vec + format_contract

If any of these get promoted to real panels later, they'll need to grow
actual input handling before being wired.
