# mylittlechart

Desktop trading client with GPU-accelerated charting, 480+ indicators, and order flow panels for 18 crypto exchanges.

[chart.example](https://chart.example)

<!-- Screenshots placeholder block:
![Main chart](docs/screenshots/main-chart.png)
![DOM panel](docs/screenshots/dom-panel.png)
-->

> **Status**: Alpha. APIs and features change between releases.

## Features

- **Charts** — Vello-based GPU rendering, multi-window, multi-chart per window, pipelined render with double-buffered scenes
- **Indicators** — 480+ across 23 categories: trend, momentum, volatility, volume, order flow, statistics, signal processing, adaptive, Kalman filters, chaos theory, entropy, regression, and more
- **Order flow panels** — DOM (depth of market), Footprint, Volume Profile, Liquidity Heatmap, L2 Tape, Trade Tape, Big Trades
- **Trading panels** — Order Entry, Position Manager, Risk Calculator, Trading Container
- **18 exchanges** — Binance, Bybit, OKX, Kraken, Coinbase, KuCoin, Bitget, GateIO, HTX, Bitfinex, Bitstamp, BingX, MEXC, CryptoCom, HyperLiquid, Dydx, Lighter, Deribit
- **Agent API** — local HTTP server (`127.0.0.1:17420`) for automation: read bars/indicators, control viewport, switch symbols, take chart screenshots, manage drawing primitives
- **Alerts** — in-app toast, Telegram (multi-subscriber, optional chart screenshots), HTTP webhook
- **Render backends** — Vello GPU (default), Vello CPU, Vello hybrid, TinySkia (CPU), wgpu instanced (experimental)

## Quick start

```bash
git clone https://github.com/ZENG3LD/mylittlechart
cd mylittlechart
cargo run --release -p chart-app-vello
```

Requires Rust stable (2021 edition).

Primary development target is Windows 11. macOS and Linux are supported by the underlying stack and our build environment can produce binaries for them, but builds for those targets are not yet part of the regular release pipeline.

## Architecture

Three components ship together:

| Component | What |
|---|---|
| **`mylittlechart`** (this repo) | Desktop client. Rust + Vello GPU. |
| **`zengeld-server`** (hosted) | Cloud API: user accounts (separate repo, not yet public). |
| **`mylittlechart-landing`** | Marketing site at chart.example (separate repo, not yet public). |

Inside the client:

| Crate | Role |
|---|---|
| `chart-app-vello` | Entry binary, multi-window event loop |
| `chart-app` | App logic, action queue, input handling, tick() |
| `chart` | Chart rendering, panel system, user profile |
| `indicators` | 480+ technical indicators |
| `panels` | Order flow + trading panels |
| `live-data` | WebSocket connector pool, bar cache, DataBridge |
| `alerts` + `alert-delivery` | Alert engine + Toast/Telegram/Webhook delivery |
| `zengeld-server` (in-process) | Local Agent API on `127.0.0.1:17420` |
| `trading-manager` | OrderManager, PositionTracker, paper trading engine |
| `bar-store` / `bar-service` | OHLCV series storage and access |
| `trade-store` / `trade-service` | Trade history storage and access |
| `orderbook-store` / `orderbook-service` | Order book storage and access |
| `vello-context` | Vello GPU renderer |
| `vello-cpu-context` | Vello CPU renderer |
| `vello-hybrid-context` | Vello hybrid renderer |
| `tiny-skia-context` | TinySkia software renderer |
| `instanced-context` | wgpu instanced renderer (experimental) |
| `diagnostics` | Runtime diagnostics |

## Documentation

- [Agent API reference](docs/agent-api.md) — full endpoint list with curl examples
- [Supported exchanges](docs/exchanges.md) — what data is available, planned credential vault
- [Alerts setup](docs/alerts.md) — Telegram bot + Webhook configuration
- [Trading panels](docs/panels.md) — what each panel does
- [Architecture](docs/architecture.md) — deeper dive into the client internals

## Tech stack

Rust 2021 · Vello · winit · wgpu · Tokio · axum · rayon · digdigdig3 (exchange connectors) · uzor (UI framework)

## License

[Business Source License 1.1](LICENSE).

**Free for:**
- Personal trading on your own account
- Non-commercial use
- Internal business use within a single organization

**Requires a commercial license for:** providing a "Commercial Trading Service" — any product or service made available to third parties (whether free or paid) that gives users the ability to trade financial instruments.

**Change Date: 2030-05-24** — at that point this code converts to Apache 2.0.

For commercial licensing inquiries, open an issue tagged `commercial-license`.

## Contributing

This project does not accept external pull requests at this time. For bug reports and feature requests, please open an issue.
