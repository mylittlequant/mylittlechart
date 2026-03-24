# Implementation Plan: AccountType End-to-End Pipeline

## Architecture Decision

Thread `AccountType` as a first-class field through every struct that currently carries `(exchange, symbol)`, using serde `default` attributes on each new field so that all existing serialized data deserializes cleanly as `AccountType::Spot`. The WS actor key expands from `(ExchangeId, WsStreamType)` to `(ExchangeId, WsStreamType, AccountType)` so that spot and futures WS connections are fully independent multiplexed actors.

---

## Current State Summary (what was confirmed by code audit)

| Layer | File | Problem |
|---|---|---|
| `SymbolInfo` | `digdigdig3/.../trading.rs:1156` | No `account_type` field — exchange info endpoint returns one pool of symbols |
| `SubscriptionRequest` | `digdigdig3/.../websocket.rs:105` | Only `symbol + stream_type`, no account type |
| OKX `subscribe/unsubscribe` | `digdigdig3/.../okx/websocket.rs:454,492` | Hardcodes `AccountType::Spot` to build `inst_id` |
| `WsKey` | `live-data/src/ws_manager.rs:53` | `(ExchangeId, WsStreamType)` — no account type in actor key |
| `WsCmd` | `live-data/src/ws_manager.rs:66` | `AddSymbol { symbol: String }` — no account type |
| `bridge.subscribe_trades/ticker` | `live-data/src/bridge.rs:647,661` | `(ExchangeId, &str)` — no account type |
| `LiveUpdate` variants | `live-data/src/bridge.rs:25+` | `BarsLoaded/BarUpdate/TradeUpdate` carry `symbol: String` but no account type |
| `bar_cache` key | `live-data/src/bridge.rs:137` | `(ExchangeId, String, String)` — no account type (spot and futures bars would collide) |
| `WatchlistSymbol` | `sidebar-content/src/watchlist.rs:59` | Only `symbol + exchange` |
| `WatchlistItem` | `sidebar-content/src/types.rs:148` | Only `symbol + exchange` |
| `SearchResult` | `zengeld_chart` modal state | `symbol + exchange_id` composite key — no account type |
| `ChartWindow` | `chart/src/state/chart_window.rs:149,153` | `pub symbol: String`, `pub exchange: String` — no account type |
| Symbol switch handler | `chart-app/src/input.rs:13655-13671` | Sets `window.symbol`, `window.exchange`, calls `request_bars(exchange, symbol)` — no account type |
| Watchlist click handler | `chart-app/src/input.rs:6963-6968` | Reads `item.symbol` + `item.exchange` — no account type |
| `request_bars` | `live-data/src/bridge.rs:265` | Signature `(ExchangeId, &str, &Timeframe, ...)` — no account type |

---

## Types and Traits

### Phase 1 — digdigdig3 layer

```rust
// digdigdig3/src/core/types/trading.rs — add to SymbolInfo
pub struct SymbolInfo {
    pub symbol: String,
    pub base_asset: String,
    pub quote_asset: String,
    pub status: String,
    pub price_precision: u8,
    pub quantity_precision: u8,
    pub min_quantity: Option<f64>,
    pub max_quantity: Option<f64>,
    pub tick_size: Option<f64>,
    pub step_size: Option<f64>,
    pub min_notional: Option<f64>,
    // NEW — which market this symbol belongs to
    #[serde(default)]
    pub account_type: super::AccountType,
}

// digdigdig3/src/core/types/websocket.rs — add to SubscriptionRequest
pub struct SubscriptionRequest {
    pub symbol: Symbol,
    pub stream_type: StreamType,
    // NEW — which market to subscribe on
    #[serde(default)]
    pub account_type: AccountType,
}

impl SubscriptionRequest {
    pub fn trade(symbol: Symbol) -> Self { ... }  // keeps Spot default
    pub fn trade_for(symbol: Symbol, account_type: AccountType) -> Self { ... }
    pub fn ticker(symbol: Symbol) -> Self { ... }
    pub fn ticker_for(symbol: Symbol, account_type: AccountType) -> Self { ... }
}
```

### Phase 2 — live-data WS actor layer

```rust
// live-data/src/ws_manager.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct WsKey {
    pub exchange_id: ExchangeId,
    pub stream_type: WsStreamType,
    pub account_type: AccountType,  // NEW
}

pub(crate) enum WsCmd {
    AddSymbol { symbol: String },
    RemoveSymbol { symbol: String },
    Shutdown,
}
// WsCmd stays the same — account_type is embedded in WsKey (the actor itself
// always serves one fixed account_type).
```

### Phase 3 — live-data bridge layer

```rust
// live-data/src/bridge.rs
pub enum LiveUpdate {
    BarsLoaded {
        exchange_id: ExchangeId,
        symbol: String,
        timeframe: String,
        bars: Vec<Bar>,
        account_type: AccountType,  // NEW
    },
    BarUpdate {
        exchange_id: ExchangeId,
        symbol: String,
        bar: Bar,
        is_closed: bool,
        account_type: AccountType,  // NEW
    },
    TradeUpdate {
        exchange_id: ExchangeId,
        symbol: String,
        price: f64,
        quantity: f64,
        timestamp: i64,
        account_type: AccountType,  // NEW
    },
    MiniTickerUpdate {
        exchange_id: ExchangeId,
        symbol: String,
        last_price: f64,
        price_change_percent: Option<f64>,
        high_price: Option<f64>,
        low_price: Option<f64>,
        volume: Option<f64>,
        account_type: AccountType,  // NEW
    },
    // ... ConnectorReady, SymbolsLoaded, etc. unchanged
}

// bridge public API
impl DataBridge {
    pub fn request_bars(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        timeframe: &Timeframe,
        account_type: AccountType,  // NEW
        limit: Option<usize>,
        total_bars: Option<usize>,
    ) { ... }

    pub fn subscribe_trades(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,  // NEW
    ) { ... }

    pub fn subscribe_mini_ticker(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,  // NEW
    ) { ... }

    pub fn unsubscribe_trades(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,  // NEW
    ) { ... }

    pub fn unsubscribe_mini_ticker(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,  // NEW
    ) { ... }
}

// bar_cache key gains account_type
// OLD: HashMap<(ExchangeId, String, String), Vec<Bar>>
// NEW: HashMap<(ExchangeId, String, String, AccountType), Vec<Bar>>
```

### Phase 4 — sidebar-content layer

```rust
// sidebar-content/src/watchlist.rs
pub struct WatchlistSymbol {
    pub symbol: String,
    #[serde(default = "default_exchange")]
    pub exchange: String,
    #[serde(default)]
    pub account_type: AccountType,  // NEW
}

impl WatchlistSymbol {
    pub fn new(symbol: String, exchange: String) -> Self { ... }  // keeps Spot default
    pub fn new_with_type(symbol: String, exchange: String, account_type: AccountType) -> Self { ... }
}

// sidebar-content/src/types.rs
pub struct WatchlistItem {
    pub symbol: String,
    pub exchange: String,
    pub last_price: f64,
    pub change_percent: f64,
    pub high_24h: f64,
    pub low_24h: f64,
    pub volume_24h: f64,
    pub account_type: AccountType,  // NEW
}
```

### Phase 5 — chart layer

```rust
// zengeld_chart modal state — SearchResult (exact location TBD, referenced in search_overlay.rs)
pub struct SearchResult {
    pub symbol: String,
    pub name: String,
    pub exchange: String,
    pub exchange_id: String,
    pub asset_type: String,
    pub category_icon: String,
    pub in_watchlist: bool,
    pub account_type: AccountType,  // NEW
}

// chart/src/state/chart_window.rs
pub struct ChartWindow {
    // ... existing fields ...
    pub symbol: String,
    pub exchange: String,
    pub account_type: AccountType,  // NEW (serde default = Spot)
    // ...
}
```

---

## Module Layout

No new modules are created. All changes are additive field additions to existing structs.

**Files to Modify:**

- `C:/Users/VA PC/CODING/ML_TRADING/nemo/digdigdig3/src/core/types/trading.rs:1156` — add `account_type` to `SymbolInfo`
- `C:/Users/VA PC/CODING/ML_TRADING/nemo/digdigdig3/src/core/types/websocket.rs:105` — add `account_type` to `SubscriptionRequest`, add `trade_for`/`ticker_for` constructors
- `C:/Users/VA PC/CODING/ML_TRADING/nemo/digdigdig3/src/crypto/cex/okx/websocket.rs:454,492` — replace hardcoded `AccountType::Spot` with `request.account_type`
- `C:/Users/VA PC/CODING/ML_TRADING/nemo/mylittlechart/crates/live-data/src/ws_manager.rs:53,66` — add `account_type` to `WsKey`, update `get_or_spawn`, `make_sub_request`, `build_ws`
- `C:/Users/VA PC/CODING/ML_TRADING/nemo/mylittlechart/crates/live-data/src/bridge.rs:25+,137,265,647,661,676,705` — add `account_type` to all `LiveUpdate` variants, bar cache key, and all pub subscribe/unsubscribe/request methods
- `C:/Users/VA PC/CODING/ML_TRADING/nemo/mylittlechart/crates/sidebar-content/src/watchlist.rs:59` — add `account_type` to `WatchlistSymbol` with serde default
- `C:/Users/VA PC/CODING/ML_TRADING/nemo/mylittlechart/crates/sidebar-content/src/types.rs:148` — add `account_type` to `WatchlistItem`
- `C:/Users/VA PC/CODING/ML_TRADING/nemo/mylittlechart/crates/chart/src/state/chart_window.rs:149+` — add `pub account_type: AccountType` with serde default
- `C:/Users/VA PC/CODING/ML_TRADING/nemo/mylittlechart/crates/chart-app/src/lib.rs:5080+` — `build_symbol_search_results`: read `account_type` from `SymbolInfo`, propagate to `SearchResult`
- `C:/Users/VA PC/CODING/ML_TRADING/nemo/mylittlechart/crates/chart-app/src/input.rs:6963+,13651+` — watchlist item click and symbol search select: read `account_type`, pass to `request_bars`, `subscribe_trades`, set `window.account_type`
- `C:/Users/VA PC/CODING/ML_TRADING/nemo/mylittlechart/crates/chart/src/layout/modals/search_overlay.rs:631` — item key changes from `"symbol:exchange"` to `"symbol:exchange:account_type"` (or keep old key, see note below)

---

## Implementation Steps

### Step 1 — digdigdig3: SymbolInfo gets account_type

In `digdigdig3/src/core/types/trading.rs` at line 1156, add:

```rust
#[serde(default)]
pub account_type: super::AccountType,
```

`AccountType` already derives `Default`? Check: no `Default` derive found. Add `#[derive(Default)]` to `AccountType` with `#[default]` on `Spot`. This is the only breaking change at the enum level and is the prerequisite for all serde defaults.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum AccountType {
    #[default]
    Spot,
    Margin,
    FuturesCross,
    FuturesIsolated,
    Earn,
    Lending,
    Options,
    Convert,
}
```

### Step 2 — digdigdig3: SubscriptionRequest gets account_type

In `digdigdig3/src/core/types/websocket.rs` at line 105, add `account_type: AccountType` with `#[serde(default)]`. Add new convenience constructors `trade_for` and `ticker_for`. Existing `trade()` and `ticker()` keep working, defaulting to `Spot`.

The `WebSocketConnector::subscribe` trait receives the full `SubscriptionRequest` including `account_type`. Every connector that ignores it (because their symbols are the same regardless of account type, e.g. Binance uses different symbols like `BTCUSDT` vs `BTCUSDT_PERP`) already has the right symbol in `request.symbol.raw`. For OKX specifically, `request.account_type` now drives `format_symbol`.

### Step 3 — digdigdig3: OKX subscribe/unsubscribe fix

In `digdigdig3/src/crypto/cex/okx/websocket.rs` at lines 454 and 492, replace:

```rust
let account_type = AccountType::Spot;  // REMOVE
let inst_id = format_symbol(&request.symbol.base, &request.symbol.quote, account_type);
```

with:

```rust
let inst_id = format_symbol(&request.symbol.base, &request.symbol.quote, request.account_type);
```

This is the root fix for the original bug. The OKX `format_symbol` function already knows how to build `BTC-USDT` (Spot) vs `BTC-USDT-SWAP` (Futures) — it was just being given the wrong account type.

Other exchanges where `AccountType` changes the WS URL or channel format must be audited similarly (Bybit uses `BTCUSDT` for spot and `BTCUSDT` for linear futures but subscribes to different category channels; Binance uses `BTCUSDT` vs `BTCUSDT_PERP`). This audit is scoped to OKX in Phase 1.

### Step 4 — live-data: WsKey and build_ws

In `live-data/src/ws_manager.rs`:

The `WsKey` struct gains `account_type: AccountType`. Because `WsKey` implements `Hash` and `Eq`, and is the map key for `WsActorMap.actors`, spot and futures actors for the same exchange are automatically kept separate — no additional dispatch logic needed.

`build_ws(exchange_id, account_type, ...)` gains the account_type parameter. For OKX: `ws.connect(account_type)` already receives it. For all other exchanges with `standard!` macro: they already pass `AccountType::Spot` hardcoded in `connect()` — this is correct for their spot WS URLs. If futures WS requires a different URL, those exchanges would need individual handling (scoped out of Phase 1 — only OKX is known to need it at this time).

`make_sub_request(stream_type, exchange_id, symbol, account_type)` gains account_type and calls `SubscriptionRequest::trade_for(sym, account_type)` or `SubscriptionRequest::ticker_for(sym, account_type)`.

### Step 5 — live-data: bridge subscribe/unsubscribe/request APIs

All public methods on `DataBridge` gain `account_type: AccountType`. All `LiveUpdate` enum variants that carry exchange+symbol data also gain `account_type: AccountType`. The bar cache key becomes `(ExchangeId, String, String, AccountType)`.

The dispatch from `LiveUpdate` to chart state in `chart-app` must also match on `account_type` when routing bar updates to the correct `ChartWindow`. Since `ChartWindow` now carries `account_type`, the routing check becomes:

```rust
// dispatch logic (chart-app/src/lib.rs or similar)
if update.exchange_id == window.active_exchange
    && update.symbol == window.symbol
    && update.account_type == window.account_type {
    // apply bar
}
```

### Step 6 — sidebar-content: WatchlistSymbol and WatchlistItem

`WatchlistSymbol` gains `account_type: AccountType` with `#[serde(default)]`. All existing `add_symbol`, `remove_symbol`, `contains`, `toggle_symbol`, etc. methods that currently check `(symbol, exchange)` must be updated to check `(symbol, exchange, account_type)`.

The `color_flags` key format is currently `"symbol:exchange"`. Update to `"symbol:exchange:account_type"` for new entries. Old entries (without `:account_type` suffix) are treated as `Spot` on read — implement this by checking key suffix in `get_color_flag`.

`WatchlistItem` gains `account_type: AccountType` with `Default::default()`.

The `deserialize_symbols` helper that handles the migration from plain `String` format stays intact — it already defaults to `exchange: "binance"`, it will also default `account_type: AccountType::Spot` via serde.

### Step 7 — chart: ChartWindow.account_type

In `chart/src/state/chart_window.rs` at line 149+, add:

```rust
#[serde(default)]
pub account_type: AccountType,
```

`ChartWindow::new(symbol, timeframe)` and `new_with_provider(symbol, timeframe, provider)` keep their signatures — `account_type` defaults to `Spot`.

Add a new constructor or builder method for when account type is known at creation time:

```rust
pub fn new_for_account(symbol: &str, timeframe: Timeframe, account_type: AccountType) -> Self {
    let mut w = Self::new(symbol, timeframe);
    w.account_type = account_type;
    w
}
```

`ChartWindow::update_title()` should incorporate account type in the title string (see UI section below).

### Step 8 — chart-app: SearchResult and symbol switch

**SearchResult** (in `zengeld_chart::ui::modal_state`): add `account_type: AccountType` with `#[serde(default)]`.

**Item key format** in `search_overlay.rs` at line 631: The key `"symbol:exchange_id"` is used for hit-testing and watchlist-star toggling. Change to `"symbol:exchange_id:account_type_str"` where `account_type_str` is a short stable string (`"spot"`, `"futures_cross"`, etc.). Implement `AccountType::as_key_str() -> &'static str`.

**`build_symbol_search_results`** in `chart-app/src/lib.rs` at line 5081: The `SymbolInfo` now carries `account_type`. Propagate it directly to `SearchResult`. This is the entry point where all exchange symbols become searchable with their correct type tag.

**Symbol switch handler** (watchlist item click at `input.rs:6963`):

```rust
// BEFORE
let symbol = item.symbol.clone();
let item_exchange = item.exchange.clone();

// AFTER
let symbol = item.symbol.clone();
let item_exchange = item.exchange.clone();
let account_type = item.account_type;  // NEW

// Then when calling bridge:
self.bridge.request_bars(resolved_exchange, &symbol, &timeframe, account_type, ...);
self.bridge.subscribe_trades(resolved_exchange, &symbol, account_type);
self.bridge.subscribe_mini_ticker(resolved_exchange, &symbol, account_type);

// And setting window:
window.account_type = account_type;
```

Same pattern applies to the symbol search select handler at `input.rs:13651` (search result click) and watchlist modal item click at `input.rs:13666`.

**Timeframe change handler** at `input.rs:15273` currently reads the active window's symbol and uses `self.active_exchange`. It needs to also read `window.account_type` and pass it forward.

### Step 9 — UI: account type tags in search and watchlist

**Symbol search modal** (`search_overlay.rs`): The `category_icon` field on `SearchResult` is currently `"C"` (crypto). Extend this: when `account_type != Spot`, render an additional small badge (`S`/`F`/`M`) after the exchange name column. This does not require a new field — `account_type` is now available on `SearchResult` and can be read in the render function to decide the badge.

Render logic in `render_symbol_search_results_scrollable` at line 670:

```rust
// After rendering exchange name:
let type_badge = match item.account_type {
    AccountType::Spot => "",          // no badge for spot (it's the default)
    AccountType::FuturesCross | AccountType::FuturesIsolated => "F",
    AccountType::Margin => "M",
    _ => "",
};
if !type_badge.is_empty() {
    ctx.fill_text(type_badge, x + width - 90.0, current_y + item_height / 2.0);
}
```

**Watchlist sidebar**: The watchlist render (not in scope of read files, but follows the same pattern) — each `WatchlistItem` row that has `account_type != Spot` should show a small `F`/`M` badge next to the symbol name.

**Chart title bar**: `ChartWindow::update_title()` should include account type when not Spot:

```rust
pub fn update_title(&mut self) {
    self.title = if self.account_type == AccountType::Spot {
        format!("{} · {} · {}", self.symbol, self.timeframe.name, self.exchange)
    } else {
        format!("{} [{}] · {} · {}",
            self.symbol,
            self.account_type.short_label(),
            self.timeframe.name,
            self.exchange,
        )
    };
}
```

Add `AccountType::short_label() -> &'static str` returning `"S"`, `"F"`, `"M"`, `"FC"`, `"FI"` etc.

---

## Migration Strategy for Existing Serialized Data

### Presets / ChartWindow JSON

`ChartWindow` is serialized as part of the preset system. Adding `#[serde(default)]` to `account_type` means all old presets deserialize with `AccountType::Spot` — exactly correct. No migration script needed.

### WatchlistSymbol JSON

`WatchlistSymbol` already has a custom `deserialize_symbols` function that handles the v1 (plain string) → v2 (struct) migration. The existing v2 format `{"symbol": "...", "exchange": "..."}` will deserialize with `account_type = AccountType::Spot` via serde default. No changes to the migration function needed.

### Bar Cache

Bar cache is session-local (in-memory) and rebuilt on startup. The key change `(ExchangeId, String, String)` → `(ExchangeId, String, String, AccountType)` requires no migration — the cache is empty at startup.

### On-disk bar cache (if persisted)

The `seed_bar_cache` method at `bridge.rs:849` loads disk-persisted bars. If bars are stored with a key that does not include `AccountType`, they will all be treated as Spot on load, which is correct for existing data. New bars written to disk must include `AccountType` in the key. If the cache serialization format is a flat file, add a version field or use serde default on the key struct.

---

## WS Actor Key: Why AccountType Goes In WsKey (Not WsCmd)

**Problem**: For OKX, `BTC-USDT` (spot) and `BTC-USDT-SWAP` (futures) are literally different symbols at the WS level. A single actor handles one fixed account type (and therefore one fixed WS endpoint URL). Two symbols of the same ticker but different account types cannot share one actor.

**Solution**: `WsKey = (ExchangeId, WsStreamType, AccountType)`. Each unique combination gets its own actor. The actor is born for a specific account type and all symbols it manages are that same account type.

This is correct because:
- `build_ws(exchange_id, account_type)` would connect to the correct URL/channel for that exchange+account_type combination
- `make_sub_request(stream_type, exchange_id, symbol, account_type)` builds the correct subscription message
- The actor itself never needs to switch account types mid-life

**For exchanges where spot and futures share the same WS endpoint** (e.g. Binance public market data is on the same WS host, just different stream names): Two separate actors will maintain two separate connections to the same URL. This is slightly wasteful but architecturally simpler and correct. Optimization to share the connection can be done later if needed.

---

## build_ws Implications

`build_ws` currently takes `exchange_id` only. It needs `account_type` to potentially select different WS URLs. For each exchange:

- **OKX**: No URL change needed for public data (same WS host for spot/futures). The account type affects the `instType` in subscription messages only.
- **Binance**: Spot WS = `wss://stream.binance.com`, Futures WS = `wss://fstream.binance.com`. These are **different URLs**. `build_ws` must select the correct URL. The `BinanceWebSocket::new(creds, testnet, account_type)` constructor already receives `AccountType` — check if it selects the URL based on it.
- **Bybit**: Spot WS = `wss://stream.bybit.com/v5/public/spot`, Linear futures = `wss://stream.bybit.com/v5/public/linear`. Account type determines path.

This means `build_ws` must receive `account_type`. The `standard!` macro currently hardcodes `AccountType::Spot`. Update the macro to pass the account type from the `WsKey`.

---

## Phase Breakdown (Ship Independently)

### Phase 1 — Foundation (digdigdig3 changes, no UI impact)
1. Add `#[derive(Default)]` + `#[default]` to `AccountType::Spot` in `digdigdig3/src/core/types/common.rs:250`
2. Add `account_type` to `SymbolInfo` in `digdigdig3/src/core/types/trading.rs:1156`
3. Add `account_type` to `SubscriptionRequest` in `digdigdig3/src/core/types/websocket.rs:105`; add `trade_for`/`ticker_for` constructors
4. Fix OKX: use `request.account_type` in `okx/websocket.rs:454,492`

Ships as a digdigdig3 version bump. No user-visible changes.

### Phase 2 — Live Data Layer
5. Add `account_type` to `WsKey` in `live-data/src/ws_manager.rs:53`
6. Update `build_ws` to accept and pass `account_type`
7. Update `make_sub_request` to pass `account_type`
8. Add `account_type` to all `LiveUpdate` variants in `live-data/src/bridge.rs`
9. Update bar cache key to include `account_type`
10. Update all `DataBridge` public method signatures to include `account_type`

Ships with a live-data version bump. Still no user-visible changes — all callers temporarily pass `AccountType::Spot` explicitly.

### Phase 3 — Sidebar and Chart State
11. Add `account_type` to `WatchlistSymbol` in `sidebar-content/src/watchlist.rs:59`
12. Update `WatchlistList` methods (`contains`, `add_symbol`, `remove_symbol`, `toggle_symbol`) to include `account_type` in their `(symbol, exchange)` checks
13. Add `account_type` to `WatchlistItem` in `sidebar-content/src/types.rs:148`
14. Add `account_type` to `ChartWindow` in `chart/src/state/chart_window.rs`
15. Add `account_type` to `SearchResult` in modal_state

Ships as milestone — data structures are correct end-to-end.

### Phase 4 — Input Handlers
16. Symbol search select: extract `account_type` from item key, set on window, pass to bridge
17. Watchlist item click: pass `account_type` through
18. Watchlist modal item click: pass `account_type` through
19. Timeframe change handler: read `window.account_type`, pass to bridge
20. All `request_bars` call sites updated with `account_type`

Ships as the core behavioral fix — OKX futures works correctly.

### Phase 5 — UI Tags
21. Symbol search modal: render `F`/`M` badge for non-spot symbols
22. Watchlist sidebar: render type badge per row
23. Chart title bar: show account type in title when non-spot
24. `symbol_drawings` key: decide if it should include `account_type` (if user switches from BTCUSDT spot to BTCUSDT-SWAP, drawings should be separate — add `account_type` to the snapshot key)

Ships as visual polish.

---

## Questions Answered

**1. Where exactly does AccountType get added to each struct?**

Every struct that currently carries `symbol + exchange` gets `account_type: AccountType` with `#[serde(default)]`. See the Types section above for all 8 structs.

**2. How do exchanges with the SAME symbol for different types get distinguished?**

At the digdigdig3 layer: OKX uses raw symbol strings (`BTC-USDT` vs `BTC-USDT-SWAP`). These look different. No extra disambiguation needed at the raw symbol level. At the mylittlechart layer: `WatchlistSymbol` uniqueness is now `(symbol, exchange, account_type)` — two entries with the same symbol/exchange but different account types are distinct rows. In the symbol search modal, the composite hit-test key becomes `"symbol:exchange:account_type_str"`. The UI renders a badge to make the distinction visible.

**3. Migration path for existing presets/BarStore?**

Fully handled by `#[serde(default)]` on every new field. `AccountType` gets `#[derive(Default)]` with `Spot` as default. No migration scripts, no version bumps to serialization formats. Old data = Spot.

**4. Should build_ws() connect to different WS URLs per AccountType?**

Yes, for exchanges where the URLs differ (Binance spot vs Binance futures are confirmed to be different hosts). The `WsKey` now carries `account_type`, so `build_ws(key.exchange_id, key.account_type)` can branch on both. The `standard!` macro must be updated to pass `key.account_type` instead of hardcoding `AccountType::Spot`.

**5. UI changes needed?**

- Symbol search: `F`/`M` badge in the right column, after the exchange name
- Watchlist sidebar: small `F`/`M` tag on each row that is non-spot
- Chart title bar: `[F]` or `[M]` suffix when account type is not Spot
- `symbol_drawings` cache key in `ChartWindow`: extend to include `account_type` so spot and futures drawings are stored separately for the same ticker

---

## Error Handling

No new error types required. `AccountType` mismatch (user picks a futures symbol but the connector returns spot data) is handled at the WS level by verifying the `account_type` in the `LiveUpdate` matches the `ChartWindow.account_type` before applying the update. Mismatches are silently ignored (the update is for a different window).

---

## Risk Assessment

**High risk**: Step 12 — updating `WatchlistList` contains/add/remove to 3-tuple identity. Many callers pass only `(symbol, exchange)`. Must audit all call sites in `chart-app/src/input.rs` that call `watchlist_manager.contains/add_symbol/remove_symbol/toggle_symbol`. The old 2-tuple callers (star click in search modal, watchlist delete button) must be updated to pass `account_type`.

**Medium risk**: Step 5 — `LiveUpdate` enum variant changes. This enum is matched in `chart-app` and possibly other consumers. All match arms must be updated. Because it's a non-exhaustive change (adding a field to existing variants, not adding new variants), Rust will catch every missed destructuring pattern at compile time — this is safe.

**Medium risk**: Step 6 — `build_ws` URL selection for Binance futures. Need to verify that `BinanceWebSocket::new(creds, testnet, AccountType::FuturesCross)` already selects `fstream.binance.com`. If not, that constructor needs to be fixed in digdigdig3.

**Low risk**: All serde migration — covered by `#[serde(default)]`.

**Low risk**: Bar cache key expansion — session-local, always empty at startup.

**Estimated Complexity:** High (8 distinct layers, 20+ files, must remain backward compatible at every serialization boundary)

---

## Testing Plan

### Unit tests (in-file, `#[cfg(test)]`)

- `watchlist.rs`: Add test `test_same_symbol_different_account_types` — adds `BTC-USDT:okx:spot` and `BTC-USDT-SWAP:okx:futures_cross` as distinct entries, verifies both coexist and `contains` distinguishes them
- `watchlist.rs`: Add test `test_serde_migration_no_account_type` — deserialize old JSON without `account_type` field, verify it deserializes as `AccountType::Spot`
- `websocket.rs` (digdigdig3): Add test verifying `SubscriptionRequest::ticker(sym).account_type == AccountType::Spot` and `SubscriptionRequest::ticker_for(sym, AccountType::FuturesCross).account_type == AccountType::FuturesCross`

### Integration tests

- OKX WebSocket: integration test that subscribes to `BTC-USDT` with `AccountType::Spot` and verifies the subscription message contains `"instType": "SPOT"`, and `BTC-USDT-SWAP` with `AccountType::FuturesCross` verifies `"instType": "SWAP"`
- `WsActorMap`: verify that two subscribe calls with the same exchange+stream_type but different account_type spawn two separate actors

### Manual QA

- Open chart with OKX, search for `BTC-USDT` — spot version loads spot candles
- Open chart with OKX, search for `BTC-USDT-SWAP` — futures version loads futures candles
- Add both to watchlist — they appear as separate rows with `S`/`F` badge
- Save preset with futures symbol — reload — futures symbol is restored correctly (not downgraded to spot)
