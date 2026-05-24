# Supported Exchanges

## Exchange list

18 exchanges are supported for live market data.

| Exchange | ID | Notes |
|---|---|---|
| Binance | `binance` | |
| BingX | `bingx` | |
| Bitfinex | `bitfinex` | |
| Bitget | `bitget` | |
| Bitstamp | `bitstamp` | |
| Bybit | `bybit` | |
| Coinbase | `coinbase` | |
| CryptoCom | `cryptocom` | |
| Deribit | `deribit` | Options and perpetuals |
| Dydx | `dydx` | Decentralized perps |
| GateIO | `gateio` | |
| HTX | `htx` | |
| HyperLiquid | `hyperliquid` | Decentralized perps |
| Kraken | `kraken` | |
| KuCoin | `kucoin` | |
| Lighter | `lighter` | |
| MEXC | `mexc` | |
| OKX | `okx` | |

Additional exchanges with WebSocket support (not in the TRUSTED pool): Upbit, Moex, Gemini.

## Trading is not wired up yet

Right now mylittlechart works on **public market data only**. No API keys, no signing, no exchange account required.

What works out of the box, against any of the exchanges above, with zero configuration:

- OHLCV bar history
- Real-time price feed (WebSocket trades and book updates)
- Order book depth
- Trade tape
- Volume data
- All indicators and order flow panels

Order entry, position tracking, account balance, and trade history paths exist in the codebase (`trading-manager`, `order-entry` panel, etc.) but are not yet connected to live exchange APIs — they currently run against the in-process paper engine only.

## Planned: encrypted credential vault

When live trading is wired up, exchange API keys will be handled by an existing on-disk vault. The vault is already built and battle-tested separately:

- AES-256-GCM at rest
- Passphrase-derived key via PBKDF2-HMAC-SHA256 (600,000 iterations) + HKDF-SHA256 → 32-byte vault key
- Blob format: `[12-byte nonce][ciphertext][16-byte GCM tag]`
- Validated against extraction, key recovery, and passphrase round-trip flows

It just doesn't have anything to store yet, because nothing in the client currently consumes API keys.

Credentials, when added, will never be transmitted to any server in plaintext.

## Account types

The `account_type` field used in the Agent API accepts these short labels (forward-looking — currently only `S` is exercised by the data path):

| Label | Meaning |
|---|---|
| `S` | Spot (default) |
| `FC` | Futures Cross-margin |
| `M` | Margin |
