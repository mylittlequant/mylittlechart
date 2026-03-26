# Plan: `set_bars()` Split Refactor

## Problem Statement

`set_bars()` in `chart_window.rs:736` is called from the `BarsLoaded` handler for two fundamentally different scenarios:

1. **New window / symbol-change** — `chart_width` is 0 (layout not yet computed), `bar_spacing` is the default (8.0), no bars exist. Needs deferred snap-to-end in `prepare_frame` once `chart_width` becomes valid.
2. **Restored preset** — `bar_spacing` and `chart_width` are already valid (saved and restored from snapshot). `view_start` is intentionally zeroed in the snapshot (`from_window` line 201). Can snap immediately when bars arrive, no need for a deferred path.

Both scenarios hit the same `is_backfill = false` branch in `BarsLoaded` (~line 1933) and both set `needs_auto_scale_after_bars = true`, deferring the snap to `prepare_frame`. This causes a subtle but real problem: for restored presets the deferred snap fires after layout settles, which is fine — but the snap recalculates `view_start` from scratch using only `bar_spacing` and `chart_width`, which are now valid. The deferred snap works correctly for _both_ cases today, which is why this has not caused visible bugs. However it is architectural noise: the flag `needs_auto_scale_after_bars` conflates "chart_width not yet known" with "snap needed", and the `snap_cooldown` mechanism exists purely to guard against `sync_viewport_from_layout` undoing the snap before sub-pane heights settle.

The refactor goal: **make the intent explicit**, remove the false conflation, and set the stage for a future where restored presets snap immediately when bars arrive (no deferred path needed).

---

## Codebase Survey Results

### `set_bars()` — `chart_window.rs:736`
- Stores bars, recalculates MAs and `prev_close`
- Sets `viewport.bar_count`
- Sets `needs_auto_scale_after_bars = true` (defers snap to `prepare_frame`)
- Does NOT touch `view_start`, `bar_spacing`, or `chart_width`

### `update_bars()` — `chart_window.rs:757`
- Same data work (bars, MAs, `prev_close`, `bar_count`)
- Does NOT set `needs_auto_scale_after_bars`
- Calls `calc_auto_scale()` immediately if `scale_mode.is_auto_y()`
- Used by: backfill path in `BarsLoaded` (line 1922), `BackfillComplete` (line 2030)

### `BarsLoaded` handler — `lib.rs:1889`

The `is_backfill` discriminator (line 1913):
```
is_backfill = if window.pending_symbol_load { false }
              else { !window.bars.is_empty() }
```

Non-backfill branch (line 1933):
1. `window.price_scale.scale_mode = self.default_scale_mode;`  ← applies global default
2. `window.set_bars(bars.clone());`
3. `window.pending_symbol_load = false;`
4. Schedules background backfill

**The `default_scale_mode` override (line 1936) is the most important signal**: it applies to ALL non-backfill loads regardless of whether the window is new or restored. For restored presets this OVERWRITES the scale_mode that was just restored from the snapshot (line 16121 in input.rs: `window.price_scale.scale_mode = snap.price_scale.scale_mode`). This is a bug if the user had saved with Manual scale mode — it gets silently overridden back to Auto on every reload.

### Deferred snap in `prepare_frame` — two copies

**Non-split** (`lib.rs:2937`): runs after `sync_viewport_from_layout()` has set `chart_width`
**Split** (`lib.rs:3584`): runs after per-leaf layout computation sets `chart_width`

Both do identical work:
- Guard: `needs_auto_scale_after_bars && !bars.is_empty() && chart_width > 0.0`
- Compute `view_start` from `(bar_count + margin - visible_bars)`
- Set `snap_cooldown = 3`
- Force `ScaleMode::Auto` temporarily, call `calc_auto_scale()`, restore saved mode
- Propagate `view_start` + `bar_spacing` to sync-group peers

**The deferred snap is duplicated in two places.** This is maintenance debt.

### `sync_viewport_from_layout` — `lib.rs:1734`

Called on every resize and at the top of `prepare_frame` (non-split only). Applies `bar_shift` when `chart_width` changes, but suppresses it when:
- `snap_cooldown > 0` (post-snap frames)
- `needs_auto_scale_after_bars` is true (waiting for initial data)
- `old_width == 0.0` (first frame, no shift possible)

### `needs_initial_viewport_fit` — `lib.rs:293`

A separate, app-level flag set in three places:
- `lib.rs:874` — startup with saved preset (preset-restore path, `new()`)
- `lib.rs:1100` — `new_window()` constructor
- `lib.rs:1366` — a second `new_window` variant

Fires once in `resize()` (line 1706): repositions ALL windows to snap-to-end using `visible_bars()`. This is the startup snap for windows that have bars loaded synchronously (not from `BarsLoaded`). It currently does a fixed 5-bar right margin, different from the dynamic margin in the `needs_auto_scale_after_bars` snap.

**Important**: `needs_initial_viewport_fit` is fired from both `new()` AND `new_window()`. The `new()` path is the initial-load restore case at startup. The `new_window()` path is for a multi-window app startup with a skeleton window. In practice, windows get bars async via `BarsLoaded` so `needs_initial_viewport_fit` fires when bars may not yet be present — making it a no-op most of the time (`count + right_margin > visible` condition is false if count = 0).

### `pending_symbol_load`

Set in these places:
- `input.rs:7230` — watchlist sidebar click (symbol change on active window)
- `input.rs:13936` — watchlist modal item click (symbol change)
- `input.rs:14341` — symbol search select (symbol change)
- `input.rs:16126` — LoadPreset new-window build (layout restore path)
- `input.rs:16262` — LoadPreset patch path (old preset fallback)
- `input.rs:18161` — `propagate_symbol_to_sync_group` (symbol propagation to peer windows)

Cleared in:
- `lib.rs:1938` — `BarsLoaded` non-backfill branch, after `set_bars()`

Used as a gate in:
- `lib.rs:2135` — `TradeUpdate` handler: skip synthetic bar insertion while waiting
- `lib.rs:2514` — scroll-left prefetch guard: don't prefetch while loading

**`pending_symbol_load` serves two duties**: (1) forcing the initial-load path in `BarsLoaded` even if a stray TradeUpdate pre-inserted a bar, and (2) marking the window as "not ready" for trade/scroll operations. These two duties are conceptually coupled and the flag should stay as-is.

### `from_window()` snapshot — `snapshots.rs:194`

What is saved:
- Full `viewport` clone **except** `view_start` is zeroed (line 201): `saved_viewport.view_start = 0.0`
- `bar_spacing` IS saved (it is part of the viewport struct)
- `chart_width` IS saved (though it will be stale/wrong after window resize — it reflects the chart width at save time, not at restore time)
- `bar_count` IS saved (will be stale — bars are re-fetched on restore)
- `price_scale` clone with `price_min`/`price_max` zeroed

What is restored (LoadPreset, input.rs:16039):
- `window.viewport = snap.viewport.clone()` — full viewport, so `chart_width`, `bar_spacing`, `bar_count` all come from snapshot
- Then immediately after (line 16118): overrides only `bar_spacing` if `> 0.0`
- `window.price_scale.scale_mode = snap.price_scale.scale_mode`

So after LoadPreset: `window.viewport.bar_spacing` = saved value, `window.viewport.chart_width` = saved (stale) value, `window.viewport.view_start` = 0.0, `window.viewport.bar_count` = saved (stale) value, `window.bars` = empty.

Then `BarsLoaded` arrives. `is_backfill = false` (because `pending_symbol_load = true`). The non-backfill branch:
1. **OVERWRITES** `price_scale.scale_mode` with `default_scale_mode` — bug for users who saved with Manual mode
2. Calls `set_bars()` → sets `needs_auto_scale_after_bars = true`
3. Deferred snap fires in `prepare_frame` → recomputes `view_start` correctly (bar_spacing is valid from snap)

---

## All Callers of `set_bars()`

Only ONE caller exists: `lib.rs:1937` in the `BarsLoaded` non-backfill branch. There is no other call site in the codebase.

The `is_backfill = false` path applies to ALL of these triggering scenarios:
- **Chain A**: Brand new window (fresh profile, no preset)
- **Chain B**: Preset restore on startup
- **Chain C**: Tab switch (LoadPreset for existing preset)
- **Chain C2**: Symbol change (user clicks symbol in search/watchlist)

All four enter the same `set_bars()` → `needs_auto_scale_after_bars = true` → deferred-snap path.

---

## The Four Chains — What Happens vs What Should Happen

### Chain A: Brand New Window (fresh profile)

**Current flow:**
1. `new()` constructor: fresh `ChartWindow` with `chart_width=800.0` (default Viewport), `bar_spacing=8.0`, no bars, `pending_symbol_load=false`
2. `bridge.request_bars()` called
3. `needs_initial_viewport_fit = true` set
4. `resize()` fires (first layout): `sync_viewport_from_layout()` sets `chart_width` to real value. `needs_initial_viewport_fit` fires → tries to snap, but `bars.len() = 0`, so snap is skipped
5. `BarsLoaded` arrives: `is_backfill = false` (bars were empty). `price_scale.scale_mode = default_scale_mode`. `set_bars()` → `needs_auto_scale_after_bars = true`. `pending_symbol_load = false`
6. `prepare_frame`: `needs_auto_scale_after_bars = true` AND `chart_width > 0` (from step 4). Snap fires: computes `view_start`, sets `snap_cooldown = 3`

**What should happen:** Same as current, but the "new window" snap should NOT use the same `bar_spacing` path as restore — a new window has no meaningful saved `bar_spacing`, so `8.0` (Viewport::default) is used. This is correct.

**Problem:** In step 5, `default_scale_mode` is applied even here. For a brand new window this is correct behavior (the global default should apply). So the override is appropriate for Chain A but wrong for Chain B/C.

### Chain B: Preset Restore (startup or tab switch)

**Current flow:**
1. LoadPreset: `window.viewport = snap.viewport` (full clone with `view_start=0`, stale `chart_width`, saved `bar_spacing`)
2. `window.price_scale.scale_mode = snap.price_scale.scale_mode` ← user's saved mode
3. `window.pending_symbol_load = true`
4. `bridge.request_bars()` called
5. `BarsLoaded` arrives: `is_backfill = false` (forced by `pending_symbol_load`)
6. **`price_scale.scale_mode = default_scale_mode`** ← OVERWRITES the restored scale_mode (BUG)
7. `set_bars()` → `needs_auto_scale_after_bars = true`
8. `sync_viewport_from_layout()` sets `chart_width` to real current value (discarding the stale saved one)
9. Deferred snap fires: `view_start = (bar_count + margin - visible_bars).max(0.0)` using the REAL `chart_width` and RESTORED `bar_spacing`

**What should happen:**
- Step 6 should NOT apply `default_scale_mode` — the user's saved scale_mode should be preserved
- The snap in step 9 is correct (uses real chart_width, saved bar_spacing)
- Ideally the snap could fire immediately when bars arrive IF `chart_width` is already valid, but deferred is also fine

### Chain C: Symbol Change on Existing Window

**Current flow:**
1. `window.bars.clear()`, `window.viewport.bar_count = 0`, `window.viewport.view_start = 0.0`
2. `window.pending_symbol_load = true`
3. `bridge.request_bars()` called
4. `BarsLoaded` arrives: `is_backfill = false` (forced by `pending_symbol_load`)
5. `price_scale.scale_mode = default_scale_mode` ← resets to Auto even if user had set Manual
6. `set_bars()` → `needs_auto_scale_after_bars = true`
7. Deferred snap fires with current `chart_width` (valid, window is open) and current `bar_spacing` (user's zoom level preserved — window.bars were cleared but `bar_spacing` was not touched)

**What should happen:** Same as current EXCEPT step 5 should preserve the user's existing `scale_mode` rather than resetting to global default. A symbol change should not reset the price scale mode.

### Chain D: Backfill (update_bars path)

Not affected by this refactor. `update_bars()` is called directly, no deferred snap. `calc_auto_scale()` is called immediately if `scale_mode.is_auto_y()`. Viewport `view_start` and `bar_spacing` are unchanged.

---

## Proposed Split

### Two Methods

```rust
// chart_window.rs

/// Initial bar load — brand new window or symbol change where the user
/// has no saved viewport position to recover. Defers snap-to-end to
/// `prepare_frame` where `chart_width` is guaranteed valid.
///
/// - Does NOT reset `bar_spacing` (preserves user zoom on symbol change)
/// - Does NOT apply `scale_mode` (caller must set it before calling if needed)
/// - Sets `needs_auto_scale_after_bars = true` → deferred snap in prepare_frame
///
/// Use this when: fresh profile window, symbol change via search/watchlist
pub fn set_bars_new(&mut self, bars: Vec<Bar>) {
    self.bars = bars;
    self.calc_moving_averages();
    if !self.bars.is_empty() {
        self.prev_close_price = Some(self.bars[0].open);
    } else {
        self.prev_close_price = None;
    }
    self.update_prev_close_line();
    self.viewport.bar_count = self.bars.len();
    self.needs_auto_scale_after_bars = true;
}

/// Bar load for a restored preset — `bar_spacing` and `chart_width` are
/// already valid from the snapshot. Snaps to end immediately if
/// `chart_width > 0`; falls back to deferred snap otherwise.
///
/// - Preserves `bar_spacing` from restored snapshot (already set by LoadPreset)
/// - Preserves `price_scale.scale_mode` from restored snapshot (caller must NOT override it)
/// - If `chart_width > 0`: snaps `view_start` immediately and calls `calc_auto_scale()`
/// - If `chart_width == 0`: falls back to `needs_auto_scale_after_bars = true`
///
/// Use this when: LoadPreset restoring from a saved snapshot
pub fn set_bars_restore(&mut self, bars: Vec<Bar>) {
    self.bars = bars;
    self.calc_moving_averages();
    if !self.bars.is_empty() {
        self.prev_close_price = Some(self.bars[0].open);
    } else {
        self.prev_close_price = None;
    }
    self.update_prev_close_line();
    self.viewport.bar_count = self.bars.len();

    if self.viewport.chart_width > 0.0 && !self.bars.is_empty() {
        // chart_width is known — snap immediately
        let count = self.bars.len();
        let visible_f = self.viewport.chart_width / self.viewport.bar_spacing;
        let dynamic_margin = compute_snap_margin(visible_f);
        self.viewport.view_start = (count as f64 + dynamic_margin - visible_f).max(0.0);
        self.calc_auto_scale();
        self.snap_cooldown = 3;
        // needs_auto_scale_after_bars stays false — snap already done
    } else {
        // chart_width not yet known — fall back to deferred snap
        self.needs_auto_scale_after_bars = true;
    }
}

/// Shared margin computation — same formula as the deferred snap in prepare_frame.
fn compute_snap_margin(visible_bars: f64) -> f64 {
    let v = visible_bars as usize;
    if v <= 10 { 1.0 }
    else if v <= 20 { 2.0 }
    else if v <= 50 { 3.0 }
    else if v <= 100 { 4.0 }
    else { 5.0 }
}
```

Keep `set_bars()` as a **deprecated alias** for `set_bars_new()` during transition — or remove it immediately since there is only one call site.

---

## BarsLoaded Handler Split

The `is_backfill = false` branch in `lib.rs:1933` must distinguish the sub-scenarios:

```rust
// In BarsLoaded, replace the non-backfill block:

if is_backfill {
    window.update_bars(bars.clone());
    // ... backfill scheduling ...
} else {
    // Determine if this is a restore or a fresh load.
    // A restore is indicated by: bars arrived from a window that was
    // just reconstructed by LoadPreset (pending_symbol_load=true AND
    // bar_spacing was restored from snapshot, i.e. != 8.0 default).
    //
    // Simpler signal: track it explicitly via a new flag.
    if window.pending_preset_restore {
        // Chain B: restored preset — preserve scale_mode, snap immediately
        window.pending_preset_restore = false;
        window.set_bars_restore(bars.clone());
    } else {
        // Chain A/C: brand new window or symbol change
        // Apply global default scale mode (fresh context, no saved preference to preserve)
        window.price_scale.scale_mode = self.default_scale_mode;
        window.set_bars_new(bars.clone());
    }
    window.pending_symbol_load = false;
    // ... backfill scheduling ...
}
```

### New Field: `pending_preset_restore: bool`

Add to `ChartWindow` (alongside `pending_symbol_load`):

```rust
/// Set to `true` when this window was just reconstructed from a saved preset
/// snapshot (LoadPreset). Signals the BarsLoaded handler to use set_bars_restore()
/// instead of set_bars_new(), preserving scale_mode and bar_spacing from the snapshot
/// rather than applying global defaults.
/// Cleared after set_bars_restore() runs.
pub pending_preset_restore: bool,
```

Set in:
- `input.rs:16126` (LoadPreset new-window build path) — SET `pending_preset_restore = true`, keep `pending_symbol_load = true`
- `input.rs:16262` (LoadPreset patch fallback path) — same

Do NOT set it for:
- `input.rs:7230` (watchlist sidebar click — symbol change, Chain C)
- `input.rs:13936` (watchlist modal — symbol change, Chain C)
- `input.rs:14341` (symbol search select — symbol change, Chain C)
- `input.rs:18161` (sync-group symbol propagation — symbol change, Chain C)

Default: `false` in both `new_with_provider()` and `with_id()` constructors and `for_split()`.

---

## `default_scale_mode` Override — Scoping

Currently `lib.rs:1936` unconditionally applies `default_scale_mode` to every non-backfill load. After the refactor:

- **Chain A (new window)**: apply `default_scale_mode` before `set_bars_new()` — correct, the window has no saved preference
- **Chain B (preset restore)**: do NOT apply `default_scale_mode` — the `scale_mode` was restored from snapshot at LoadPreset time and must be preserved
- **Chain C (symbol change)**: Do NOT apply `default_scale_mode`. A symbol change should preserve the user's current price scale mode — if they were in Manual mode with a custom Y range, a symbol switch should reset to Auto (since the Y range is no longer meaningful) but should NOT permanently override their saved preference. This is a design question — the current behavior (always set to `default_scale_mode`) is arguably correct for Chain C because Manual scale on the old symbol is meaningless for the new symbol. Keep it for Chain C.

So the scoping becomes: only Chain B (preset restore) does NOT override `scale_mode`. Chains A and C continue to apply `default_scale_mode`.

---

## Deferred Snap Duplication

`prepare_frame` contains two identical snap implementations (non-split line 2940, split line 3588). Extract to a method:

```rust
// On ChartApp or in a helper:
fn apply_deferred_viewport_snap(window: &mut ChartWindow) -> Option<(f64, f64)> {
    if !window.needs_auto_scale_after_bars { return None; }
    if window.bars.is_empty() { return None; }
    if window.viewport.chart_width <= 0.0 { return None; }

    window.needs_auto_scale_after_bars = false;
    let count = window.bars.len();
    let visible_f = window.viewport.chart_width / window.viewport.bar_spacing;
    let margin = compute_snap_margin(visible_f);
    window.viewport.view_start = (count as f64 + margin - visible_f).max(0.0);
    window.snap_cooldown = 3;
    let saved_mode = window.price_scale.scale_mode;
    window.price_scale.scale_mode = ScaleMode::Auto;
    window.calc_auto_scale();
    window.price_scale.scale_mode = saved_mode;
    Some((window.viewport.view_start, window.viewport.bar_spacing))
}
```

Replace both deferred snap blocks in `prepare_frame` with calls to this helper.

---

## `needs_initial_viewport_fit` — Is It Still Needed?

After the refactor: `needs_initial_viewport_fit` fires in `resize()` and tries to snap windows. If bars are not yet loaded (count=0) the snap is a no-op. If bars ARE loaded (they arrived before the first resize), it snaps to end.

For Chain A/C: bars arrive via `BarsLoaded` which sets `needs_auto_scale_after_bars = true`. If `resize()` fires BEFORE `BarsLoaded`, `needs_initial_viewport_fit` fires with `count=0` — no-op. Then `BarsLoaded` sets the flag, and `prepare_frame` snaps. If `resize()` fires AFTER `BarsLoaded`, both `needs_initial_viewport_fit` AND the deferred snap attempt to fire on the same frame. The deferred snap wins (it runs in `prepare_frame` after `sync_viewport_from_layout` which is called from `resize`). `needs_initial_viewport_fit` may have already set `view_start` but the deferred snap overwrites it with the correct value.

**Conclusion:** `needs_initial_viewport_fit` is vestigial. In the pre-`needs_auto_scale_after_bars` era it was the only snap mechanism. After the refactor it should be removed. The `set_bars_new()` → `needs_auto_scale_after_bars` path handles ALL non-restore snapping.

However, removing it is a follow-up cleanup task and not required for the correctness of this refactor. Flag it as dead code.

---

## Module Layout

**Files to Modify:**

- `c:\Users\VA PC\CODING\ML_TRADING\nemo\mylittlechart\crates\chart\src\state\chart_window.rs`
  - line 290: add `pub pending_preset_restore: bool` field after `pending_symbol_load`
  - line 407: initialize `pending_preset_restore: false` in `new_with_provider()`
  - line 603: initialize `pending_preset_restore: false` in `for_split()`
  - line 736: rename `set_bars()` to `set_bars_new()` (or deprecate by calling through)
  - after line 755: add `set_bars_restore()` method
  - extract `compute_snap_margin(visible_f: f64) -> f64` as a `pub(crate) fn` or `const fn`

- `c:\Users\VA PC\CODING\ML_TRADING\nemo\mylittlechart\crates\chart-app\src\lib.rs`
  - line 1933: split non-backfill branch — check `window.pending_preset_restore`
  - line 1936: apply `default_scale_mode` only when NOT a preset restore
  - line 1937: call `set_bars_new()` or `set_bars_restore()` based on flag
  - line 2940: replace deferred snap block with `apply_deferred_viewport_snap()` call
  - line 3588: same
  - extract `apply_deferred_viewport_snap()` as a free function or method (near the snap code)

- `c:\Users\VA PC\CODING\ML_TRADING\nemo\mylittlechart\crates\chart-app\src\input.rs`
  - line 16126: set `window.pending_preset_restore = true` (new-window build path)
  - line 16262: set `window.pending_preset_restore = true` (patch fallback path)

**No new files needed.** All changes are in-place edits to existing files.

---

## Chain Diagrams (Post-Refactor)

### Chain A: Brand New Window

```
new() constructor
  → ChartWindow::new_with_provider()
      bars=[], bar_spacing=8.0, chart_width=800.0(default), pending_symbol_load=false,
      pending_preset_restore=false, needs_auto_scale_after_bars=false

  → bridge.request_bars()
  → needs_initial_viewport_fit=true (set on app)

resize() fires (first real dimensions)
  → sync_viewport_from_layout() → sets chart_width to real value (e.g. 1200px)
  → needs_initial_viewport_fit fires → bars.len()=0 → no-op

BarsLoaded arrives
  → is_backfill=false (bars were empty, pending_symbol_load=false)
  → pending_preset_restore=false → "new/change" path
  → window.price_scale.scale_mode = default_scale_mode (Auto)
  → window.set_bars_new(bars) → needs_auto_scale_after_bars=true, bar_count=N

prepare_frame
  → apply_deferred_viewport_snap(): chart_width>0 AND bars.len()>0
      → view_start = (N + margin - visible).max(0)
      → snap_cooldown = 3
      → calc_auto_scale()
  → propagate view_start to sync-group peers
```

### Chain B: Preset Restore (startup or tab switch)

```
LoadPreset handler (input.rs:15972)
  → ChartWindow::with_id()
      bars=[], bar_spacing=8.0(default), chart_width=800.0(default)
  → window.viewport = snap.viewport  (bar_spacing=saved, chart_width=stale, view_start=0)
  → window.price_scale.scale_mode = snap.price_scale.scale_mode  (user's saved mode)
  → window.pending_symbol_load = true
  → window.pending_preset_restore = true  ← NEW
  → bridge.request_bars()

sync_viewport_from_layout() (called each frame, non-split)
  → chart_width updated to real value; bar_shift suppressed (needs_auto_scale_after_bars=false
     but old_width was stale from snap, not 0.0 — potential bar_shift on first frame)

  NOTE: After LoadPreset, window.viewport.chart_width = stale saved value (say 950px from
  last session). sync_viewport_from_layout sees old_width=950 → new_width=1200 → applies
  bar_shift. This is wrong! Resolved by setting chart_width=0.0 explicitly in LoadPreset
  before bars arrive (see implementation note below).

BarsLoaded arrives
  → is_backfill=false (pending_symbol_load=true forces it)
  → pending_preset_restore=true → "restore" path
  → DO NOT apply default_scale_mode (preserve user's scale_mode)
  → window.set_bars_restore(bars)
      → if chart_width > 0: snap immediately (view_start computed, snap_cooldown=3)
      → if chart_width == 0: needs_auto_scale_after_bars=true (deferred)
  → pending_symbol_load = false
  → pending_preset_restore = false

prepare_frame (if deferred)
  → apply_deferred_viewport_snap() fires if chart_width became valid
```

### Chain C: Symbol Change on Existing Window

```
User clicks symbol in search (input.rs:14330)
  → window.bars.clear(), bar_count=0, view_start=0.0
  → window.pending_symbol_load = true
  → pending_preset_restore stays false (NOT a preset restore)
  → bridge.request_bars()

BarsLoaded arrives
  → is_backfill=false (pending_symbol_load=true)
  → pending_preset_restore=false → "new/change" path
  → window.price_scale.scale_mode = default_scale_mode  (reset to Auto — correct for symbol change)
  → window.set_bars_new(bars) → needs_auto_scale_after_bars=true
  → pending_symbol_load = false

prepare_frame
  → apply_deferred_viewport_snap(): chart_width>0 (window was open)
      → view_start snapped, snap_cooldown=3
```

### Chain D: Backfill

```
BackfillComplete / BarsLoaded(is_backfill=true)
  → window.update_bars(bars)  [unchanged]
  → bar_count updated, view_start preserved
  → calc_auto_scale() if auto_y
```

---

## Implementation Note: Stale `chart_width` in Snapshot

When LoadPreset restores `window.viewport = snap.viewport`, the `chart_width` in the snapshot is the value from the previous session (could be different resolution/layout). `sync_viewport_from_layout` will then compute `bar_shift = (old_width - new_width) / bar_spacing` and apply it to `view_start`. Since `view_start` was zeroed in the snapshot, this shifts it in the wrong direction.

**Fix**: In LoadPreset, after restoring `window.viewport`, zero out `chart_width`:

```rust
// After: window.viewport = snap.viewport.clone();
window.viewport.chart_width = 0.0;  // force sync_viewport_from_layout to skip bar_shift
```

This ensures `sync_viewport_from_layout` sees `old_width = 0.0` → skips bar_shift (guarded by `old_w > 0.0` check at line 1763). The real `chart_width` is set by the layout system on the next frame, and the snap fires correctly.

Add this line at:
- `input.rs:16039` (layout restore path, after `window.viewport = snap.viewport.clone()`)
- `input.rs:16198` (patch fallback path, after `window.viewport = snap.viewport.clone()`)

---

## What Becomes Dead Code / Can Be Removed

| Item | Location | Status after refactor |
|------|----------|-----------------------|
| `needs_initial_viewport_fit` | `lib.rs:293,738,874,1000,1100,1167,1366,1706` | Dead — deferred snap handles all cases. Remove in follow-up. |
| Duplicate deferred snap (split path) | `lib.rs:3584` | Replaced by shared `apply_deferred_viewport_snap()` |
| Duplicate deferred snap (non-split) | `lib.rs:2937` | Replaced by shared `apply_deferred_viewport_snap()` |
| `set_bars()` original | `chart_window.rs:736` | Rename to `set_bars_new()` or keep as deprecated alias |

`snap_cooldown` and `needs_auto_scale_after_bars` are still needed — they guard against bar_shift undoing the snap in the first 3 frames after layout settles. Not removable.

`pending_symbol_load` remains needed — it gates trade updates and scroll prefetch while bars are in flight.

---

## Error Handling

No new failure modes introduced. All methods are infallible (same as current `set_bars()`).

---

## Testing Plan

### Unit Tests (in `chart_window.rs` `#[cfg(test)] mod tests`)

1. `set_bars_new_sets_deferred_flag` — after calling `set_bars_new()`, assert `needs_auto_scale_after_bars == true` and `view_start == 0.0`
2. `set_bars_restore_snaps_immediately_when_width_known` — create window with `chart_width=1000.0`, `bar_spacing=10.0`, call `set_bars_restore(300_bars)`, assert `view_start > 0.0` and `needs_auto_scale_after_bars == false`
3. `set_bars_restore_defers_when_width_zero` — `chart_width=0.0`, call `set_bars_restore()`, assert `needs_auto_scale_after_bars == true`
4. `set_bars_new_preserves_bar_spacing` — call `set_bars_new()` with custom `bar_spacing=20.0`, assert spacing unchanged after call
5. `compute_snap_margin_boundaries` — test each tier (10, 20, 50, 100+ visible bars)

### Integration Scenarios (manual verification)

1. **Fresh profile**: launch with no saved preset → BTCUSDT loads → snaps to end with Auto scale
2. **Preset restore**: save with Manual scale on ETH → restart → ETH loads → Manual scale preserved, view snaps to end
3. **Symbol change**: manually set Manual scale → click different symbol → new symbol snaps to end with Auto scale (reset is intentional on symbol change)
4. **Tab switch**: switch between two saved presets → each loads with its own scale_mode and bar_spacing preserved
5. **Backfill**: scroll left → background fetch completes → position preserved, no snap fires

---

## Estimated Complexity

**Medium.**

The data-flow change is well-contained (one call site, two new methods). The main risk is the `chart_width=0.0` zeroing in LoadPreset — it must be done in both the layout-restore path AND the patch-fallback path to avoid bar_shift corruption. The deferred snap extraction is pure refactoring (no logic change). The `default_scale_mode` scoping fix for Chain B is a one-line guard. The `needs_initial_viewport_fit` removal is a clean-up follow-up, not required for correctness.

**Implementation order:**
1. Add `pending_preset_restore: bool` field to `ChartWindow` + defaults
2. Add `compute_snap_margin()` helper
3. Add `set_bars_new()` (same as current `set_bars()`, just renamed)
4. Add `set_bars_restore()` with immediate-snap path
5. Set `chart_width=0.0` in LoadPreset (both paths)
6. Set `pending_preset_restore=true` in LoadPreset (both paths)
7. Split `BarsLoaded` non-backfill branch on `pending_preset_restore`
8. Extract `apply_deferred_viewport_snap()` and replace both deferred-snap blocks
9. (Follow-up) Remove `needs_initial_viewport_fit`
