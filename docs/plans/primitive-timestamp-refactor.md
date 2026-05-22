# Implementation Plan: Primitive Timestamp Refactor

**Goal:** Primitives store `(timestamp_s: i64, price: f64)` as source of truth.
Bar index is a viewport/render concern computed per-frame. No cache sync hooks after the refactor.

---

## A. Current State Inventory

### Storage model today

Each concrete primitive type (e.g. `TrendLine`) owns named fields:

```
// crates/chart/src/drawing/primitives_v2/lines/trend_line.rs:19-38
pub bar1: f64, pub price1: f64,
pub bar2: f64, pub price2: f64,
```

The `Primitive` trait exposes them as a `Vec<(f64, f64)>` where `.0 = bar_index, .1 = price`:

```
// traits.rs:397 — "Get all coordinate points as (bar, price) pairs"
fn points(&self) -> Vec<(f64, f64)>;
fn set_points(&mut self, points: &[(f64, f64)]);
```

`PrimitiveData` carries a parallel array as a hack:

```
// traits.rs:128
pub point_timestamps: Vec<i64>,   // Unix seconds, parallel to points()
```

This field is `#[serde(default)]` — empty on load for old presets.

**`Bar.timestamp` is Unix SECONDS** (`i64`), confirmed at `types.rs:200`.
The plan uses seconds throughout; "timestamp" always means Unix seconds unless noted.

### Migration helper functions — all live in `manager.rs`

| Function | Location | Called from |
|----------|----------|-------------|
| `recalculate_all_bar_caches` | `manager.rs:1192` | `chart_window.rs:894` (on TF change) |
| `ensure_timestamps_populated` | `manager.rs:1230` | nowhere called today (dead) |
| `update_all_timestamps_from_bars` | `manager.rs:1242` | `panel_grid.rs:2139` (after freehand complete) |
| `sync_primitive_timestamps` | `manager.rs:1249` | called by the two above |
| `bar_idx_to_timestamp` (private) | `manager.rs:1259` | called by `sync_primitive_timestamps` |

All five go away after the refactor.

### Call sites of `points()` — classified

Every primitive's `render()` calls `ctx.bar_to_x(self.bar1)` / `ctx.price_to_y(self.price1)` directly against its own fields, **not** through the trait's `points()`. The default trait `render()` at `traits.rs:456-549` does call `points()` but concrete types always override it.

The default `render()` at `traits.rs:468`:
```rust
let screen_points: Vec<(f64, f64)> = points
    .iter()
    .map(|(bar, price)| (ctx.bar_to_x(*bar), ctx.price_to_y(*price)))
    .collect();
```
**Reads bar_index AND price.**

Hit-test in each primitive (e.g. `trend_line.rs:130-145`):
```rust
let x1 = viewport.bar_to_x_f64(self.bar1);   // reads bar_index
let y1 = viewport.price_to_y(self.price1, …); // reads price
```
**Reads bar_index AND price.**

`move_control_point` / `translate` — write bar_index and price directly.

`get_points_at` / `set_points_at` in `manager.rs:2181-2190` — read/write both.
Used in undo/redo snapshots via `panel_grid.rs:2145`.

`DrawingState::Creating { points }` in `manager.rs:97-143` — accumulates `(bar, price)` during multi-click creation. Written by `on_click` / `add_freehand_point` / `create_preview`.

`sync_primitive_timestamps` at `manager.rs:1249-1256` — reads `bar` from `points()`, converts to timestamps, writes `point_timestamps`. This is the reverse of what we want.

### Call sites of `set_points()` — what they pass

| Site | File:Line | What it passes |
|------|-----------|----------------|
| `on_click` 2-point | `manager.rs:906` | `(bar, price)` from `viewport.x_to_bar_f64` |
| `on_click` 3/4-point | `manager.rs:950, 986` | same |
| `complete_freehand` | `manager.rs:696` | same |
| `recalculate_all_bar_caches` | `manager.rs:1220` | remapped bar from ts lookup |
| `set_points_at` (undo/redo) | `manager.rs:2186` | whatever was in snapshot |
| Per-primitive `set_points` impl | all primitive files | destructures into named fields |

---

## B. Target Design

### Storage after refactor

`PrimitiveData.point_timestamps` is dropped. Each primitive's named coordinate fields change meaning:

```rust
// TrendLine example — same field names, new semantics
pub ts1: i64,     // Unix seconds (renamed from bar1)
pub price1: f64,
pub ts2: i64,     // renamed from bar2
pub price2: f64,
```

OR: keep field names `bar1/bar2` to minimize diff surface but document them as timestamps. Better to rename to `ts1/ts2` for clarity — prevents confusion if someone reads the raw JSON.

`Primitive` trait signature changes:

```rust
/// Get coordinate points as (timestamp_s, price) pairs.
/// timestamp_s is Unix seconds matching Bar::timestamp.
fn points(&self) -> Vec<(i64, f64)>;

/// Set coordinate points. points[n].0 is Unix timestamp in seconds.
fn set_points(&mut self, points: &[(i64, f64)]);

/// Translate: ts_delta in seconds, price_delta in price units.
fn translate(&mut self, ts_delta: i64, price_delta: f64);

/// Move a control point to (timestamp_s, price).
fn move_control_point(&mut self, point_type: ControlPointType, ts: i64, price: f64);
```

`PrimitiveData`:
```rust
// Remove:
pub point_timestamps: Vec<i64>,   // gone

// Add nothing — coordinates live in the primitive's own fields
```

Serialization: `TrendLine` JSON gains `ts1`/`ts2` instead of `bar1`/`bar2`.
The `point_timestamps` field stays in `PrimitiveData` as `#[serde(default, skip_serializing)]`
for one release cycle so old JSON that still has the field doesn't error on load.

### Per-frame coordinate conversion helpers

Add to `Viewport`:

```rust
/// Convert timestamp (Unix seconds) to bar index (f64) for current bar array.
/// Returns None if bars is empty. Extrapolates beyond last bar.
pub fn timestamp_to_bar_f64(bars: &[Bar], ts: i64) -> f64;
```

This is a thin wrapper around the existing `find_bar_for_timestamp` + extrapolation.
It returns `f64` because `find_bar_for_timestamp` returns `usize` but sub-bar precision is needed.
Implement as:
```rust
pub fn timestamp_to_bar_f64(bars: &[Bar], ts: i64) -> f64 {
    match find_bar_for_timestamp(bars, ts) {
        Some(idx) => idx as f64,
        None => 0.0,
    }
}
```

Add inverse: screen-click → timestamp:

```rust
impl Viewport {
    /// Convert X pixel to timestamp using bars array.
    /// For positions right of last bar, extrapolates using bar interval.
    pub fn x_to_timestamp(bars: &[Bar], x: f64) -> i64;
}
```

Implementation:
```rust
pub fn x_to_timestamp(&self, bars: &[Bar], x: f64) -> i64 {
    let bar_f = self.x_to_bar_f64(x);
    let bar_idx = bar_f.floor() as usize;
    let frac = bar_f - bar_f.floor();
    if bars.is_empty() { return 0; }
    let interval = bar_interval(bars); // last_ts - prev_ts or 3600
    if bar_idx < bars.len() {
        bars[bar_idx].timestamp + (frac * interval as f64) as i64
    } else {
        let last = bars[bars.len() - 1].timestamp;
        let beyond = bar_idx - (bars.len() - 1);
        last + (beyond as i64 + frac as i64) * interval
    }
}
```

### Render path (per-frame, no cache)

Each primitive's `render()` receives `bars: &[Bar]` (or the `RenderContext` carries it). Two options:

**Option A (preferred):** Add `bars: &[Bar]` to `RenderContext` alongside the coordinate functions. `bar_to_x` stays but is used only by indicator series rendering. Primitives call `ctx.ts_to_x(ts)` which does `Viewport::timestamp_to_bar_f64(ctx.bars(), ts)` → `Viewport::bar_to_x_f64(bar)`.

```rust
// engine/render/context.rs — extend RenderContext
fn ts_to_x(&self, ts: i64) -> f64;   // timestamp → screen X
fn price_to_y(&self, price: f64) -> f64; // unchanged
```

**Option B:** Keep `RenderContext` unchanged; add a `bars` slice parameter to `render()`. This changes every `render()` signature across ~100 primitive files — high churn.

**Decision: Option A.** `RenderContext` already owns viewport state; adding `bars` reference there is natural and avoids signature churn.

### Hit-test path

Before:
```rust
let x1 = viewport.bar_to_x_f64(self.bar1);
```
After:
```rust
let x1 = viewport.bar_to_x_f64(Viewport::timestamp_to_bar_f64(bars, self.ts1));
```

Or more cleanly, hit-test receives `(bars, viewport, price_scale)`. The `bars` is already accessible via `ChartWindow` in all callers. The `Primitive::hit_test` signature gains `bars: &[Bar]`:

```rust
fn hit_test(
    &self,
    screen_x: f64,
    screen_y: f64,
    bars: &[Bar],
    viewport: &Viewport,
    price_scale: &PriceScale,
) -> HitTestResult;
```

`DrawingManager::hit_test` passes `bars` down. All callers of `hit_test` already have `bars` because they live in `ChartWindow` or `PanelGrid`.

### Drag path

`on_click` and `start_drag` / `update_drag` currently receive `(bar: f64, price: f64)`.
After refactor they receive `(ts: i64, price: f64)`. The conversion from mouse X → timestamp happens in the caller (panel_grid / input handler) via `viewport.x_to_timestamp(bars, screen_x)`.

`DrawingState::Creating { points: Vec<(i64, f64)> }` — points accumulate as `(ts, price)`.

`update_drag` calls `prim.translate(ts_delta, price_delta)`. `ts_delta` is seconds. Caller computes `current_ts - prev_ts`.

`move_control_point` receives `(ts, price)` in seconds.

### DrawingManager — what changes

- `on_click(ts: i64, price: f64)` — signature change
- `start_drag(index, ts, price)` — signature change
- `update_drag(current_ts: i64, current_price: f64)` — signature change
- `start_freehand(ts, price)` / `add_freehand_point(ts, price)` — signature change
- `create_preview(cursor_ts: i64, cursor_price: f64)` — signature change
- Remove: `recalculate_all_bar_caches`, `ensure_timestamps_populated`, `update_all_timestamps_from_bars`, `sync_primitive_timestamps`, `bar_idx_to_timestamp`
- `get_points_at` / `set_points_at` return `Vec<(i64, f64)>` — undo/redo snapshots update accordingly

---

## C. Backwards Compatibility (Legacy Presets)

### Detection

Old preset JSON for `TrendLine`:
```json
{ "bar1": 42.0, "price1": 50000.0, "bar2": 48.0, "price2": 51000.0,
  "data": { "point_timestamps": [1700000000, 1700003600] } }
```

New preset JSON:
```json
{ "ts1": 1700000000, "ts2": 1700003600, "price1": 50000.0, "price2": 51000.0,
  "data": {} }
```

Migration runs at preset load, before any render. In `DrawingSnapshot::restore()` or equivalent:

```rust
fn migrate_primitive(type_id: &str, json: &str, bars: &[Bar]) -> String {
    // Deserialize as raw Value
    let mut v: serde_json::Value = serde_json::from_str(json)?;
    let timestamps = v["data"]["point_timestamps"].as_array()
        .cloned()
        .unwrap_or_default();

    if !timestamps.is_empty() {
        // Has timestamps → replace bar fields with ts fields
        let ts_vals: Vec<i64> = timestamps.iter()
            .filter_map(|t| t.as_i64())
            .collect();
        // Map per primitive type (bar1→ts1, bar2→ts2, etc.)
        replace_bar_fields_with_timestamps(&mut v, type_id, &ts_vals);
        // Clear the parallel array
        v["data"]["point_timestamps"] = serde_json::Value::Array(vec![]);
    } else if has_bar_fields(&v, type_id) && !bars.is_empty() {
        // Very old: has bar1/bar2 but NO timestamps.
        // Best-effort: read bar indices, look up timestamp in current bars.
        // If bar index is in-range, use bar's timestamp.
        // If out-of-range or bars mismatch → leave as-is. User must redraw.
        migrate_from_bar_indices(&mut v, type_id, bars);
    }
    // else: already new format (ts1/ts2 present)
    serde_json::to_string(&v)?
}
```

**Decision for very-old primitives (no timestamps, bar indices only):** Attempt best-effort migration using current bars. If bar count differs significantly from what was saved (no way to know), the primitive may appear at the wrong position. User-visible result: primitive appears shifted. They can redraw. Document this in release notes. This is strictly better than the current bug (flash then disappear).

After migration and save, presets are in new format. `point_timestamps` field survives in `PrimitiveData` struct with `#[serde(default, skip_serializing)]` for one release to avoid deserialization errors on any remaining old JSON that has the field.

---

## D. Call Site Change List

### D1 — Render (per primitive, ~100 files)

Every `fn render(&self, ctx: &mut dyn RenderContext, is_selected: bool)`:

- Replace `ctx.bar_to_x(self.bar1)` with `ctx.ts_to_x(self.ts1)`
- Replace `ctx.bar_to_x(self.bar2)` with `ctx.ts_to_x(self.ts2)`
- Pattern applies to all N-point primitives; for brushes/freehand, iterate `self.points` as `(ts, price)` pairs

Default trait `render()` at `traits.rs:468` changes its inner map:
```rust
.map(|(ts, price)| (ctx.ts_to_x(*ts), ctx.price_to_y(*price)))
```

### D2 — Hit-test (per primitive, ~100 files)

Every `fn hit_test(&self, screen_x, screen_y, viewport, price_scale)` gains `bars: &[Bar]`:
```rust
fn hit_test(&self, screen_x, screen_y, bars: &[Bar], viewport, price_scale) -> HitTestResult {
    let x1 = viewport.bar_to_x_f64(Viewport::timestamp_to_bar_f64(bars, self.ts1));
    let y1 = viewport.price_to_y(self.price1, …);
    …
}
```

`DrawingManager::hit_test` at `manager.rs:1451` passes `bars` down.
Callers in `panel_grid.rs` and `panel_app.rs` already hold `window.bars` — pass as `&window.bars`.

### D3 — Control points (per primitive, ~100 files)

`fn control_points(&self, viewport, price_scale)` gains `bars: &[Bar]`.
Same `timestamp_to_bar_f64` call to convert before `bar_to_x_f64`.

### D4 — Drag

`manager.rs:update_drag` at line `1624`:
```rust
pub fn update_drag(&mut self, current_ts: i64, current_price: f64) {
    …
    let ts_delta = current_ts - start_ts;
    let price_delta = current_price - start_price;
    prim.translate(ts_delta, price_delta);
    self.drag_start = Some((current_ts, current_price));
}
```

Callers: `panel_app.rs:433`, `panel_grid.rs` input handler — compute `current_ts` via `viewport.x_to_timestamp(bars, screen_x)`.

`move_control_point` at `manager.rs:1638`:
```rust
self.primitives[idx].move_control_point(*point_type, current_ts, current_price);
```

`Primitive::translate` signature:
```rust
fn translate(&mut self, ts_delta: i64, price_delta: f64);
// TrendLine impl:
self.ts1 += ts_delta;
self.ts2 += ts_delta;
self.price1 += price_delta;
self.price2 += price_delta;
```

### D5 — Creation (on_click / freehand)

`manager.rs:on_click` signature: `pub fn on_click(&mut self, ts: i64, price: f64)`.
`DrawingState::Creating { points: Vec<(i64, f64)> }`.
All factory functions: `fn create_trend_line(points: &[(i64, f64)], color: &str)`.

Input handler converts screen X → timestamp BEFORE calling `on_click`.

### D6 — Serialization (each primitive struct)

`TrendLine` before: fields `bar1: f64, bar2: f64`.
`TrendLine` after: fields `ts1: i64, ts2: i64`.

JSON key changes: `"bar1"` → `"ts1"` etc. Migration code (Section C) handles old keys.

Every concrete primitive file needs field renames + type changes.

### D7 — Undo/redo snapshots

`get_points_at` at `manager.rs:2181` returns `Vec<(i64, f64)>`.
`set_points_at` at `manager.rs:2186` takes `&[(i64, f64)]`.
`set_data_at` at `manager.rs:2198` no longer copies `point_timestamps` (field gone from `PrimitiveData`).

`FreehandCompleteResult.points` in `panel_grid.rs:2147` type changes to `Vec<(i64, f64)>`.

### D8 — Sync (sync_from_group_primitives)

`manager.rs:1120` — unchanged. It clones `Box<dyn Primitive>` by value; coordinates travel as `(ts, price)` already. No bar recalculation needed.

### D9 — move_control_point_screen (default trait impl)

`traits.rs:411-422` — default implementation currently calls `viewport.x_to_bar_f64`. After refactor:
```rust
fn move_control_point_screen(&mut self, point_type, screen_x, screen_y, bars, viewport, price_scale) {
    let ts = viewport.x_to_timestamp(bars, screen_x);
    let price = viewport.y_to_price(screen_y, price_scale.price_min, price_scale.price_max);
    self.move_control_point(point_type, ts, price);
}
```

---

## E. Phasing

### Phase 1 — Trait + Viewport API, zero-compile-break

1. Add `Viewport::timestamp_to_bar_f64(bars, ts) -> f64` at `chart/types/viewport.rs`.
2. Add `Viewport::x_to_timestamp(bars, x) -> i64`.
3. Add `bar_interval(bars: &[Bar]) -> i64` helper (private to viewport module).
4. Extend `RenderContext` (`engine/render/context.rs`) with `fn ts_to_x(&self, ts: i64) -> f64`.
   Implement in the concrete render impl via `bar_to_x(timestamp_to_bar_f64(…))`.
5. Change `Primitive::points()` return type to `Vec<(i64, f64)>` and `set_points`, `translate`, `move_control_point` accordingly.
6. Change `DrawingState::Creating.points` to `Vec<(i64, f64)>`.
7. Change `DrawingManager::on_click`, `start_drag`, `update_drag`, `start_freehand`, `add_freehand_point`, `create_preview` to use `i64` timestamp as first coordinate.
8. Change `hit_test` and `control_points` signatures to include `bars: &[Bar]`.
9. All callers in `panel_grid.rs` and `panel_app.rs` convert screen X → timestamp before calling manager.

After step 9: `cargo check` clean. No concrete primitive implementations changed yet — they will fail to compile because field types mismatch trait. This is expected.

### Phase 2 — Migrate all concrete primitives (~100 files)

For each primitive file:
- Rename `bar1` → `ts1`, `bar2` → `ts2`, etc. (type `f64` → `i64`)
- Update `points()` to return `vec![(self.ts1, self.price1), …]`
- Update `set_points()` to read `ts` from first element
- Update `translate()` to add `i64` delta
- Update `move_control_point()` for `i64` ts param
- Update `render()` to call `ctx.ts_to_x(self.ts1)` instead of `ctx.bar_to_x(self.bar1)`
- Update `hit_test()` to use `Viewport::timestamp_to_bar_f64(bars, self.ts1)` before `bar_to_x_f64`
- Update `control_points()` same way
- Update factory function: `fn create_x(points: &[(i64, f64)], color)` → unpack `ts1 = points[0].0`

After: `cargo check` clean. Primitives work correctly. `recalculate_all_bar_caches` still compiles but is now dead logic.

### Phase 3 — Remove old infrastructure + add migration

1. Remove from `PrimitiveData`: `point_timestamps` field (keep `#[serde(default, skip_serializing)]` stub for one version if needed, then fully delete).
2. Remove `DrawingManager`: `recalculate_all_bar_caches`, `ensure_timestamps_populated`, `update_all_timestamps_from_bars`, `sync_primitive_timestamps`, `bar_idx_to_timestamp`.
3. Remove call at `chart_window.rs:894`.
4. Remove call at `panel_grid.rs:2139` (also remove `update_all_timestamps_from_bars` from the `complete_freehand` body).
5. Add `migrate_primitive()` function in `preset/snapshots.rs` and call it from `DrawingSnapshot` restore path.
6. `cargo check` clean. Full refactor complete.

---

## F. Risk Inventory

### F1 — Drag math correctness

`translate(ts_delta: i64, price_delta: f64)` — ts_delta is seconds. At 1m timeframe, bar_spacing ~8px → 1 bar = 60s. Moving 1 bar left → ts_delta = -60. This is exact; no float precision issues.

Risk: caller must compute `current_ts - start_ts` using i64 arithmetic. If caller accidentally uses f64 intermediary (`viewport.x_to_bar_f64` → scale to seconds), rounding errors accumulate. Mitigation: `x_to_timestamp` must return `i64` directly; never pass through f64.

### F2 — Freehand stroke accumulation

`add_freehand_point(ts: i64, price: f64)` called on every mousemove. The existing bar-distance filter at `manager.rs:601-615`:
```rust
const MIN_BAR_DIST: f64 = 0.15;
```
Must convert to timestamp units. At the current TF, `MIN_BAR_DIST` bars = `0.15 * interval_seconds`. After refactor, filter becomes:
```rust
const MIN_TS_DIST_FRAC: f64 = 0.15;
let min_ts_dist = (interval as f64 * MIN_TS_DIST_FRAC) as i64;
if (ts - last_ts).abs() < min_ts_dist && price_dist < price_threshold { return false; }
```
The `interval` needs to be passed in or computed from `bars`. Preferred: pass `interval: i64` to `add_freehand_point` from the caller which already holds `bars`.

### F3 — Future bar positions (right of last bar)

When user clicks in empty space right of the last bar, `x_to_timestamp` must extrapolate:
```rust
let bar_f = self.x_to_bar_f64(x);  // may exceed bars.len()-1
```
`find_bar_for_timestamp` already handles this case at `types.rs:310-323`.
Reverse: `x_to_timestamp` extrapolates using `bar_interval(bars)`. This is correct behavior — a primitive drawn "2 bars into the future" will appear at the right position when new bars arrive, because `timestamp_to_bar_f64` will find the correct new bar index when those bars exist.

**This is a genuine benefit of the refactor.**

### F4 — Snap-to-bar

Snap aligns primitive to bar center. Current implementation (if any) would round `bar_f` to `round(bar_f)`. After refactor: snap rounds the bar index first, then converts back to timestamp:
```rust
let bar_idx = (viewport.x_to_bar_f64(screen_x)).round() as usize;
let ts = bars.get(bar_idx).map(|b| b.timestamp).unwrap_or_else(|| extrapolate(bars, bar_idx));
```
Not more complex than before.

### F5 — Cross-timeframe primitive persistence

Previously: on TF change, `recalculate_all_bar_caches` mapped old bar_index → new bar_index via timestamp lookup. The bug was that this ran AFTER backfill prepended bars, meaning the primitive flashed at the wrong position for 1 frame.

After refactor: TF change triggers no bar-cache update. Each primitive's timestamp is canonical. The render loop computes `ts_to_x` per-frame using the new TF's bar array. Cross-TF jump is instant and correct on first frame.

**The entire class of the reported bug is eliminated.**

### F6 — `move_control_point_screen` for emoji/image (screen-space sizing)

`traits.rs:411` — default impl converts to data coords. Emoji/image override this to keep screen-space size. After refactor the override still works — it bypasses `move_control_point` and directly mutates pixel-size fields, not timestamps. No change required in the override itself; only the default impl's first two lines change.

### F7 — Undo/redo correctness

Undo snapshots contain `Vec<(i64, f64)>` after the refactor. Restoring a snapshot calls `set_points(&[(i64, f64)])` — fully correct. Old undo stacks in memory at migration time will be invalid (they hold bar indices), but undo history is ephemeral (not persisted), so users simply lose undo history at app restart. Acceptable.

### F8 — `DrawingState` serialization (if any)

`DrawingState` is not persisted to disk. Only used for in-progress drawing. No migration needed.

---

## G. Estimated LOC + File Count

| Category | Files | Estimated changed lines |
|----------|-------|------------------------|
| Viewport (new helpers) | 1 | ~40 |
| RenderContext (new ts_to_x) | 1 | ~15 |
| `PrimitiveData` (remove field) | 1 | ~5 |
| `Primitive` trait signatures | 1 | ~25 |
| `DrawingManager` (remove helpers, change sigs) | 1 | ~120 |
| `DrawingState` type | 1 | ~10 |
| `panel_grid.rs` callers | 1 | ~30 |
| `panel_app.rs` callers | 1 | ~20 |
| `chart_window.rs` (remove recalc call) | 1 | ~5 |
| Concrete primitives (rename fields, update points/hit/render) | ~100 | ~8 each = ~800 |
| Preset migration code | 1 | ~80 |
| Undo/redo snapshot types | 2 | ~15 |
| **Total** | **~112 files** | **~1165 lines** |

All changes are mechanical and non-algorithmic except:
- `x_to_timestamp` (new logic, ~20 lines)
- `add_freehand_point` filter conversion (5 lines)
- Migration function (80 lines)

The ~100 primitive files are high volume but low risk — each file's change is identical in structure to the `TrendLine` example.

---

## Key File References

- `crates/chart/src/drawing/primitives_v2/traits.rs` — `Primitive` trait + `PrimitiveData`
- `crates/chart/src/drawing/primitives_v2/lines/trend_line.rs` — canonical example for primitive migration
- `crates/chart/src/drawing/manager.rs:1192-1279` — all migration helpers to delete
- `crates/chart/src/drawing/manager.rs:853` — `on_click` signature change
- `crates/chart/src/drawing/manager.rs:1624` — `update_drag` signature change
- `crates/chart/src/chart/types/viewport.rs:149-152` — `x_to_bar_f64` (basis for new `x_to_timestamp`)
- `crates/chart/src/types.rs:302` — `find_bar_for_timestamp` (keep as-is, used by new helpers)
- `crates/chart/src/engine/render/context.rs:35` — `bar_to_x` → add `ts_to_x` alongside
- `crates/chart/src/state/chart_window.rs:894` — remove `recalculate_all_bar_caches` call
- `crates/chart/src/state/panel_grid.rs:2139` — remove `update_all_timestamps_from_bars` call
- `crates/chart/src/preset/snapshots.rs:65` — `DrawingSnapshot::from_manager`, add restore/migrate path

**Estimated Complexity:** High (volume), Medium (algorithmic). Each individual change is simple; the risk is the volume of ~100 primitive files which must all be migrated consistently.
