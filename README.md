# mylittlechart

GPU-accelerated desktop charting and order-flow terminal for crypto traders. 18 exchanges, 480+ indicators, multi-window, scriptable through a local HTTP agent API.

**Site:** [chart.example](https://chart.example)
**Telegram channel:** [@mylittlechart](https://t.me/mylittlechart) · **X:** [@mylittlechart](https://x.com/mylittlechart)

> **Status:** Alpha. Trading is not yet wired to live exchange APIs — market data only. Paper trading engine works locally.

## What it is

mylittlechart is a desktop application that connects to 18 crypto exchanges and renders live OHLCV charts, order book data, and trade flow panels using GPU-accelerated vector graphics. It runs fully locally — no cloud account required. An embedded HTTP server exposes bar data, indicator values, and chart controls to local automation scripts and trading agents.

## Features

- **Charting** — Vello GPU rendering (default), multi-window, multi-chart per window, pipelined double-buffered scenes. Fallback backends: Vello CPU, Vello hybrid, TinySkia, wgpu instanced.
- **Indicators** — 480+ across 23 categories: trend, trend_stop, momentum, volatility, volume, channels, accumulation, regression, statistics, signal_processing, adaptive, kalman, entropy, chaos, clusters, divergence, book, ratio, candles, zigzag, average, levels, position. Computation runs in parallel on a rayon thread pool.
- **Order flow panels** — DOM (depth of market), Footprint, Volume Profile, Liquidity Heatmap, L2 Tape, Trade Tape, Big Trades.
- **Trading panels** — Order Entry, Position Manager, Risk Calculator, Trading Container. Currently backed by the in-process paper engine; not yet connected to live exchange APIs.
- **Exchanges** — 18 exchanges for live market data: Binance, Bybit, OKX, Kraken, Coinbase, KuCoin, Bitget, GateIO, HTX, Bitfinex, Bitstamp, BingX, MEXC, CryptoCom, HyperLiquid, Dydx, Lighter, Deribit.
- **Agent API** — local HTTP server on `127.0.0.1:17420`, 22 endpoints. Read bars and indicator snapshots, control viewport and symbol, take chart screenshots, manage drawing primitives. No auth — binds loopback only.
- **Alerts** — in-app toast, Telegram (multi-subscriber, optional PNG screenshot attachment), HTTP webhook. All channels run concurrently; a failed channel does not block the others.
- **Profile encryption** — user profile encrypted at rest with AES-256-GCM, passphrase → PBKDF2-HMAC-SHA256 (600K iterations) + HKDF-SHA256.

## Install

```sh
git clone https://github.com/ZENG3LD/mylittlechart
cd mylittlechart
cargo build --release
```

After the build finishes the binary is at `target/release/mylittlechart.exe` (Windows) or `target/release/mylittlechart` (macOS/Linux). Copy it anywhere and run it directly.

Or install it on your PATH:

```sh
cargo install --path crates/chart-app-vello
# then just:
mylittlechart
```

Requires Rust stable. Primary development target is Windows 11; macOS and Linux are supported by the underlying stack but not currently part of the release pipeline.

## Documentation

- [Agent API reference](docs/agent-api.md) — full endpoint list with curl examples
- [Supported exchanges](docs/exchanges.md) — what data is available, planned credential vault
- [Alerts setup](docs/alerts.md) — Telegram bot and webhook configuration
- [Trading panels](docs/panels.md) — what each panel does
- [Architecture](docs/architecture.md) — render pipeline, data flow, state ownership

## License

[Business Source License 1.1](LICENSE).

- Free for personal trading, non-commercial use, and internal business use within a single organization.
- A commercial license is required to provide a "Commercial Trading Service" — any product or service made available to third parties (free or paid) that gives users the ability to trade financial instruments.
- Change Date **2030-05-24** — at that point the code converts to Apache 2.0.

For commercial licensing inquiries: open an issue tagged `commercial-license` or reach out via [Telegram @zeng3ld](https://t.me/zeng3ld).

## Contributing

This project does not accept external pull requests at this time. For bug reports and feature requests, open an issue.

## Author

Bergman Konstantin Igorevich — [zeng3ld.com](https://zeng3ld.com) · [github.com/ZENG3LD](https://github.com/ZENG3LD) · [Telegram @zeng3ld](https://t.me/zeng3ld)
