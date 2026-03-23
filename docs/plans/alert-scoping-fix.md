# Implementation Plan: Alert Scoping Fix

## Architecture Decision

Add three scoping fields to `AlertItem` (`group_id`, `exchange`, `window_id_hint`), replace the single `last_price: f64` on `AlertManager` with a `HashMap<String, f64>` keyed by `"exchange:symbol"`, and thread the active window's context through every call site so rendering and crossing detection filter by the correct symbol/exchange. No trait redesign, no ownership restructure — surgical field additions and call-site patches only.

---

## Problem Summary

| Bug | Root Cause |
|-----|-----------|
| All alerts fire on every symbol | `check_crossings_dynamic` at `lib.rs:2088` uses `active_window()` price for ALL `AlertManager::items()` regardless of their symbol |
| One `last_price` for all symbols | `AlertManager.last_price: f64` is a single scalar; previous-price comparison is wrong when multiple symbols change |
| Price alert lines rendered on wrong windows | `alert_render_data` filter at `lib.rs:3294` only checks `AlertSource::Price` but does NOT filter by `window.symbol` or `window.exchange` |
| Bell icons shown on wrong windows | `draw_alert_bell_icons` at `lib.rs:2274` iterates all items without a symbol match guard |

---

## Types and Traits

### Phase 1 — New fields on `AlertItem` (`crates/alerts/src/types.rs`)

```rust
/// A single alert.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AlertItem {
    // --- existing fields unchanged ---
    pub id: u64,
    pub name: String,
    pub source: AlertSource,
    pub condition: AlertCondition,
    pub price: f64,
    pub price2: f64,
    pub percentage: f64,
    pub trigger_mode: AlertTriggerMode,
    pub max_triggers: u32,
    pub trigger_count: u32,
    pub transports: Vec<AlertTransport>,
    pub status: AlertStatus,
    pub created_at: u64,
    pub last_triggered_at: Option<u64>,
    pub expires_at: Option<u64>,
    pub last_triggered: Option<String>,
    pub prev_dynamic_price: f64,
    // legacy compat
    symbol: String,

    // --- NEW scoping fields (all #[serde(default)] for backward compat) ---

    /// Sync group that owns this alert. `None` only in pre-migration presets.
    /// Populated at alert-create time from the active window's `group_id`.
    #[serde(default)]
    pub group_id: Option<u64>,

    /// Exchange this alert is bound to (e.g. `"Binance"`).
    /// Populated at create time from the active window's `exchange`.
    #[serde(default)]
    pub exchange: String,

    /// Window that created this alert. Used as a display hint only —
    /// alerts are NOT exclusive to one window, they show on all windows
    /// in the same group that match `exchange:symbol`.
    #[serde(default)]
    pub window_id_hint: Option<u64>,
}
```

**No changes to `AlertSource`** — the `symbol` is still inside `AlertSource::Price { symbol }`. The `exchange` field on `AlertItem` supplements it.

**Key accessor addition:**

```rust
impl AlertItem {
    /// Returns `"exchange:symbol"` routing key for per-symbol crossing detection.
    /// Falls back to just `symbol()` when exchange is empty (old presets).
    pub fn routing_key(&self) -> String {
        let sym = self.symbol();
        if self.exchange.is_empty() {
            sym.to_string()
        } else {
            format!("{}:{}", self.exchange, sym)
        }
    }

    /// Returns true when this alert should be shown on a window with
    /// the given `symbol` and `exchange`. Exchange match is skipped for
    /// old presets that have no exchange stored (empty string).
    pub fn matches_window(&self, symbol: &str, exchange: &str) -> bool {
        let sym_match = self.symbol() == symbol;
        let exch_match = self.exchange.is_empty() || self.exchange == exchange;
        sym_match && exch_match
    }
}
```

### Phase 2 — Per-symbol `last_price` in `AlertManager` (`crates/alerts/src/manager.rs`)

```rust
pub struct AlertManager {
    items: Vec<AlertItem>,
    next_id: u64,
    /// Per-symbol previous price for crossing detection.
    /// Key: `"exchange:symbol"` (or just `"symbol"` for old alerts without exchange).
    /// Not serialized — rebuilt from live ticks.
    last_prices: HashMap<String, f64>,
}
```

**Updated method signatures:**

```rust
impl AlertManager {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            next_id: 1,
            last_prices: HashMap::new(),
        }
    }

    /// `check_crossings_dynamic` gains an `exchange` parameter so it can
    /// scope both the price lookup and the alert filter.
    pub fn check_crossings_dynamic(
        &mut self,
        current_price: f64,
        current_bar: f64,
        symbol: &str,
        exchange: &str,
        drawing_points: &[(u64, Vec<(f64, f64)>, DrawingExtendMode)],
        indicator_values: &[(u64, usize, Vec<f64>)],
    ) -> Vec<u64>;

    /// Clearing also resets the per-symbol price map.
    pub fn clear(&mut self) {
        self.items.clear();
        self.next_id = 1;
        self.last_prices.clear();
    }

    /// Restore resets the price map (rebuilt on next tick).
    pub fn restore(&mut self, alerts: Vec<AlertItem>) {
        self.next_id = alerts.iter().map(|a| a.id).max().unwrap_or(0) + 1;
        self.items = alerts;
        self.last_prices.clear();
    }
}
```

**Internal logic change in `check_crossings_dynamic`:**

```rust
// At the top of the loop body, replace:
//   let prev_price = self.last_price;
// with:

let routing_key = alert.routing_key();
// Only process alerts that belong to this symbol:exchange tick.
if !alert.matches_window(symbol, exchange) {
    continue;
}
let prev_price = self.last_prices.get(&routing_key).copied().unwrap_or(0.0);

// ... crossing logic unchanged ...

// At the end, replace:
//   self.last_price = current_price;
// with:
let own_key = format!("{}:{}", exchange, symbol);
self.last_prices.insert(own_key, current_price);
```

The old `check_crossings(&mut self, price: f64)` method can be kept as a deprecated shim that calls the new form with `symbol = ""` and `exchange = ""`, or removed — no call sites use it in the codebase (only `check_crossings_dynamic` is called from `lib.rs:2088`).

### Phase 3 — Call-site changes (`crates/chart-app/src/lib.rs`)

**Tick loop (`lib.rs` ~line 2062):**

The existing block already extracts `active_window()`. It must be extended to call `check_crossings_dynamic` per window, not once for the active window.

```rust
// NEW: iterate all visible windows, call check_crossings_dynamic per symbol:exchange
if had_trade_update {
    // Collect (window_id, symbol, exchange, price, bar, drawing_points) for each window
    // then call alert_manager.check_crossings_dynamic once per unique symbol:exchange.
    // Multiple windows on the same symbol:exchange share one check (deduplicated by routing_key).
}
```

Concretely, the loop collects a `HashSet<(symbol, exchange)>` from all live windows, then iterates that set and calls `check_crossings_dynamic` once per pair. Drawing points and indicator values must be gathered from the window that matches the symbol:exchange (first match is sufficient since they share the same bar data within a group).

**Render loop — `alert_render_data` filter (~line 3294):**

```rust
let alert_render_data: Vec<AlertRenderData> = self.alert_manager.items()
    .iter()
    .filter(|a| a.status == alerts::AlertStatus::Active)
    .filter(|a| matches!(a.source, alerts::AlertSource::Price { .. }))
    // ADD: filter by window context
    .filter(|a| a.matches_window(&window.symbol, &window.exchange))
    .filter_map(|alert| { ... })
    .collect();
```

**`draw_alert_bell_icons` (~line 2274):**

```rust
// ADD `exchange: &str` parameter to the function signature.
// At top of loop body add:
if !alert.matches_window(symbol, exchange) {
    continue;
}
```

Update all three call sites of `draw_alert_bell_icons` (~line 3372 and ~line 3789) to pass `&window.exchange`.

**`build_indicator_values_for_alerts` (~line 2192):**

This function already filters by `alert.status`. No symbol filter needed here because indicator value resolution is keyed by `indicator_id + output_index` — the per-symbol filtering happens in `check_crossings_dynamic` at the outer loop. No change required.

### Phase 4 — Alert creation: stamp scoping fields (`crates/chart-app/src/input.rs`)

All `alert_manager.create(...)` call sites (lines ~10489, ~6849-6876 area) must stamp the new fields immediately after creation:

```rust
let id = self.alert_manager.create(source, &name, price, condition);
if let Some(alert) = self.alert_manager.get_mut(id) {
    alert.price2 = price2;
    alert.percentage = percentage;
    alert.trigger_mode = trigger_mode;
    alert.transports = transports;
    // NEW scoping stamp:
    if let Some(window) = self.panel_app.panel_grid.active_window() {
        alert.exchange = window.exchange.clone();
        alert.window_id_hint = Some(window.id.0);
        alert.group_id = window.group_id.map(|g| g.0);
    }
}
```

The `create_price_alert` convenience method on `AlertManager` does not need to change — it doesn't know about windows. The caller (chart-app) stamps the fields.

---

## Module Layout

No new files. All changes are within existing files.

- `crates/alerts/src/types.rs` — add 3 fields + `routing_key()` + `matches_window()`
- `crates/alerts/src/manager.rs` — replace `last_price: f64` with `last_prices: HashMap<String, f64>`, update `check_crossings_dynamic` signature and internals, update `clear` and `restore`
- `crates/chart-app/src/lib.rs` — fix tick loop, fix render filter, fix bell icon filter + signature
- `crates/chart-app/src/input.rs` — stamp exchange/group/window on create

---

## Files to Modify

- `crates/alerts/src/types.rs` — add `group_id: Option<u64>`, `exchange: String`, `window_id_hint: Option<u64>` with `#[serde(default)]`; add `routing_key()` and `matches_window()` methods
- `crates/alerts/src/manager.rs` — replace `last_price: f64` with `last_prices: HashMap<String, f64>`; update `check_crossings_dynamic` to accept `symbol: &str, exchange: &str`, scope iteration to matching alerts, use per-key prev price
- `crates/chart-app/src/lib.rs:2062-2126` — tick loop: iterate all window symbol:exchange pairs and call `check_crossings_dynamic` per pair instead of once for active window
- `crates/chart-app/src/lib.rs:3294-3310` — render filter: add `.filter(|a| a.matches_window(&window.symbol, &window.exchange))`
- `crates/chart-app/src/lib.rs:2240-2380` — `draw_alert_bell_icons`: add `exchange: &str` param, add `matches_window` guard in loop
- `crates/chart-app/src/lib.rs:3372` and `lib.rs:3789` — call sites of `draw_alert_bell_icons`: pass `&window.exchange`
- `crates/chart-app/src/input.rs:10489` and nearby alert-create call sites — stamp `exchange`, `window_id_hint`, `group_id` after creation

---

## Implementation Phases

### Phase 1 — Types (alerts crate only, compiles standalone)

1. Open `crates/alerts/src/types.rs`.
2. Add `group_id: Option<u64>`, `exchange: String`, `window_id_hint: Option<u64>` to `AlertItem` — all with `#[serde(default)]` so old presets still deserialize.
3. Update `AlertItem::new(...)` constructor to set new fields to their defaults (`None`, `String::new()`, `None`).
4. Add `routing_key(&self) -> String` method.
5. Add `matches_window(&self, symbol: &str, exchange: &str) -> bool` method.
6. Run `cargo check --package alerts` — must be green.

### Phase 2 — Per-symbol last_price (alerts crate)

1. Open `crates/alerts/src/manager.rs`.
2. Replace `last_price: f64` field with `last_prices: HashMap<String, f64>`.
3. Add `use std::collections::HashMap;` import.
4. Update `new()`, `clear()`, `restore()` to use `last_prices`.
5. Update `check_crossings_dynamic` signature: add `symbol: &str, exchange: &str` parameters (before `drawing_points`).
6. Inside the loop:
   - Skip alerts where `!alert.matches_window(symbol, exchange)`.
   - Read `prev_price` from `self.last_prices.get(&alert.routing_key()).copied().unwrap_or(0.0)`.
7. After the loop, insert `format!("{}:{}", exchange, symbol)` → `current_price` into `last_prices`.
8. The old `check_crossings` method: update its internal `last_price` reference to use `last_prices` with a sentinel key, or mark it `#[deprecated]` and leave as dead shim. Since no call site uses it (`check_crossings_dynamic` is the only caller from chart-app), it can be simplified to call `check_crossings_dynamic` with empty strings.
9. Run `cargo check --package alerts` — must be green.

### Phase 3 — Call-site fix in lib.rs (chart-app crate)

1. Open `crates/chart-app/src/lib.rs`.
2. **Tick loop (~line 2062):** Replace the single `active_window()` approach with a deduplicated per-symbol iteration:
   - Collect `Vec<(String, String)>` of `(symbol, exchange)` tuples from all windows that have bars.
   - Deduplicate by `(symbol, exchange)` pair.
   - For each pair, find the first matching window, extract `current_price`, `current_bar`, `drawing_points`.
   - Call `self.alert_manager.check_crossings_dynamic(price, bar, symbol, exchange, &pts, &ind_vals)`.
   - Accumulate all triggered IDs, deduplicate, then build delivery events.
3. **Render filter (~line 3294):** Add `.filter(|a| a.matches_window(&window.symbol, &window.exchange))`.
4. **`draw_alert_bell_icons` signature (~line 2240):** Add `exchange: &str` after `symbol: &str`.
5. **Bell icon loop (~line 2274):** Add early-continue guard `if !alert.matches_window(symbol, exchange) { continue; }`.
6. **Call sites at ~line 3372 and ~line 3789:** Add `&window.exchange` argument.
7. Also update the second render path (~line 3670-3792) which is the single-leaf variant — same filter patches apply there.
8. Run `cargo check --package chart-app` — must be green.

### Phase 4 — Stamp scoping fields at creation (input.rs)

1. Open `crates/chart-app/src/input.rs`.
2. At every `alert_manager.create(...)` call site (main create ~line 10489, drawing bell ~line 6849 area, indicator bell ~line 6870 area):
   - After `create()` returns the new `id`, call `self.alert_manager.get_mut(id)` and set:
     - `alert.exchange = window.exchange.clone()` (from `active_window()`)
     - `alert.window_id_hint = Some(window.id.0)`
     - `alert.group_id = window.group_id.map(|g| g.0)`
3. Run `cargo check --package chart-app` — must be green.
4. Run full workspace check: `cargo check` from the workspace root.

---

## Error Handling

No new error types needed. All new operations are infallible (`HashMap::insert`, `Option::unwrap_or`, `String::clone`). The `matches_window` predicate returns `bool` — no `Result` needed.

---

## Backward Compatibility

- All three new `AlertItem` fields are `#[serde(default)]` — old preset files with no `exchange`/`group_id`/`window_id_hint` fields deserialize correctly: `exchange` defaults to `""`, which causes `matches_window` to skip the exchange check (only symbol is matched). This is correct — old alerts have no exchange scope, so they show on any exchange for that symbol.
- `last_prices.clear()` in `restore()` means the first tick after loading an old preset reseeds the per-symbol price baseline cleanly (same behavior as the old `last_price = 0.0` reset).

---

## Testing Plan

All tests live inside their source files under `#[cfg(test)] mod tests`.

**`crates/alerts/src/types.rs` — unit tests:**
- `test_routing_key_with_exchange` — verify `"Binance:BTCUSDT"` format
- `test_routing_key_no_exchange` — verify fallback to just `"BTCUSDT"`
- `test_matches_window_exact` — symbol + exchange both match
- `test_matches_window_legacy_no_exchange` — old alert (empty exchange) matches any exchange for same symbol
- `test_matches_window_wrong_symbol` — returns false
- `test_matches_window_wrong_exchange` — returns false when both are non-empty and differ
- `test_serde_roundtrip_old_preset` — deserialize JSON without new fields, verify defaults are sane

**`crates/alerts/src/manager.rs` — unit tests:**
- `test_per_symbol_last_price_isolation` — create BTC alert and ETH alert; feed BTC price that crosses BTC threshold; verify only BTC alert triggers, ETH alert does not
- `test_check_crossings_dynamic_scoped` — two alerts on same symbol but different exchange; feed price for one exchange; verify only the matching exchange's alert fires
- `test_clear_resets_prices` — call `clear()`, verify `last_prices` is empty
- `test_restore_resets_prices` — call `restore()` with items, verify `last_prices` is empty

**`crates/chart-app/src/lib.rs` — integration-level (manual smoke):**
- No automated integration test is planned in this phase. The tick loop change is tested by running the app with two chart windows on different symbols and verifying alert triggers are scoped correctly.

---

## Estimated Complexity

Low. The logic is straightforward HashMap keying. The largest risk is the tick loop refactor in `lib.rs` — it goes from one `active_window()` call to iterating all windows. That loop already exists in other parts of the tick handler (indicator recalc). The pattern is established and safe to follow.
