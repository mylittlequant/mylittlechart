# Bar Index System — Complete Research

**Date**: 2026-03-24
**Scope**: `crates/chart/`, `crates/chart-app/`, `crates/indicators/`, all render context crates
**Goal**: Can `view_start` and primitive `bar` coords be replaced with timestamp-based "logical bar numbers"?

---

## 1. Central Conversion Functions

### File: `crates/chart/src/chart/types/viewport.rs`

All bar↔pixel conversions live on the `Viewport` struct.

#### `bar_to_x(bar_idx: usize) -> f64`  (line 113)
```rust
pub fn bar_to_x(&self, bar_idx: usize) -> f64 {
    let relative_idx = bar_idx as f64 - self.view_start;
    relative_idx * self.bar_spacing + self.bar_spacing / 2.0
}
```

#### `bar_to_x_f64(bar_idx: f64) -> f64`  (line 122)
```rust
pub fn bar_to_x_f64(&self, bar_idx: f64) -> f64 {
    let relative_idx = bar_idx - self.view_start;
    relative_idx * self.bar_spacing + self.bar_spacing / 2.0
}
```
**Key formula**: `x = (bar_idx - view_start) * bar_spacing + bar_spacing/2.0`
The `+ bar_spacing/2.0` centers the pixel on the bar cell.

#### `x_to_bar(x: f64) -> Option<usize>`  (line 132)
```rust
pub fn x_to_bar(&self, x: f64) -> Option<usize> {
    if x < 0.0 || x > self.chart_width { return None; }
    let relative_idx = x / self.bar_spacing;
    let bar_idx = (self.view_start + relative_idx) as i64;
    if bar_idx >= 0 && (bar_idx as usize) < self.bar_count {
        Some(bar_idx as usize)
    } else {
        None
    }
}
```

#### `x_to_bar_f64(x: f64) -> f64`  (line 149)
```rust
pub fn x_to_bar_f64(&self, x: f64) -> f64 {
    let relative_idx = x / self.bar_spacing;
    self.view_start + relative_idx
}
```
**Key formula**: `bar = view_start + x / bar_spacing`

**Note**: No other bar↔pixel conversion functions exist in the codebase. ALL coordinate conversion routes through these four methods on `Viewport`.

The `RenderContext` trait (`crates/chart/src/engine/render/context.rs`, line 35) adds:
```rust
fn bar_to_x(&self, bar: f64) -> f64;
```
All render context implementations (vello, tiny-skia, instanced, vello-hybrid, vello-cpu) implement this by delegating to the same formula:
```rust
// From instanced-context/src/context.rs line 194:
fn bar_to_x(&self, bar: f64) -> f64 {
    if let Some(ref ovr) = self.coord_override {
        let offset = bar - ovr.view_start;
        offset * ovr.bar_spacing + ovr.bar_spacing / 2.0
    } else ...
}
```

---

## 2. Viewport Fields

### File: `crates/chart/src/chart/types/viewport.rs` (lines 14-33)

```rust
pub struct Viewport {
    pub view_start: f64,        // Starting bar index (f64, allows sub-bar panning, can be negative)
    pub bar_spacing: f64,       // Pixels per bar (default: 8.0)
    pub bar_width_ratio: f64,   // Bar body width ratio (0.0-1.0, default: 0.8)
    pub chart_width: f64,       // Chart area width in pixels
    pub chart_height: f64,      // Chart area height in pixels
    pub bar_count: usize,       // Total bars in data (set from bars.len())
}
```

### What is `view_start`?

`view_start` is a **Vec index** — the fractional index into `bars[]` at the left edge of the visible chart area. When `view_start = 100.0`, bar `bars[100]` is at the left edge. When `view_start = -5.0`, we are scrolled 5 bars into future space (right of last bar).

It is set in several places:
- `chart-app/src/lib.rs:1643`: `window.viewport.view_start = (count + right_margin - visible) as f64` — based on `window.bars.len()`
- `chart-app/src/lib.rs:1645`: `window.viewport.view_start = 0.0`
- `chart-app/src/lib.rs:1674`: `window.viewport.view_start += bar_shift` — shift on resize
- `chart-app/src/lib.rs:1842`: restored from `ViewportSnapshot.view_start` (preset persistence)
- `chart-app/src/lib.rs:1968`: `window.viewport.view_start = (count as f64 + dynamic_margin - visible_f).max(0.0)` — follow mode on new bar
- `viewport.rs:217`: `scroll_to_end()` → `view_start = (bar_count - visible_bars()) as f64`

### What is `bar_spacing`?

Pixels per one bar slot. Default is `8.0`. Zoom in/out changes this. Min: `0.7`, Max: `chart_width * 0.5`.

### What is `bar_count`?

Mirrors `bars.len()`, updated at `chart-app/src/lib.rs:1948`:
```rust
window.viewport.bar_count = window.bars.len();
```
Used only for clamping in `visible_range()`, `x_to_bar()`, `view_start_idx()`, `scroll_to_end()`.

---

## 3. Core Formula Summary

```
x_pixel = (bar_vec_index - view_start) * bar_spacing + bar_spacing/2.0
bar_vec_index = view_start + x_pixel / bar_spacing
```

`view_start`, `bar_vec_index` are BOTH Vec indices (integers or fractional).

---

## 4. How Primitives Use Bar Indices

### File: `crates/chart/src/drawing/primitives_v2/traits.rs`

The `Primitive` trait has:
```rust
fn points(&self) -> Vec<(f64, f64)>;   // Returns (bar_vec_index, price) pairs
fn set_points(&mut self, points: &[(f64, f64)]);
fn translate(&mut self, bar_delta: f64, price_delta: f64);
fn move_control_point(&mut self, point_type: ControlPointType, bar: f64, price: f64);
```

The **first f64 in each point tuple is always a Vec index** (f64 for sub-bar precision). This is confirmed by `TrendLine`:
```rust
// trend_line.rs line 86-97
fn points(&self) -> Vec<(f64, f64)> {
    vec![(self.bar1, self.price1), (self.bar2, self.price2)]
}
fn set_points(&mut self, points: &[(f64, f64)]) {
    self.bar1 = points[0].0;  // stored as f64 bar index
    self.bar2 = points[1].0;
}
fn translate(&mut self, bar_delta: f64, price_delta: f64) {
    self.bar1 += bar_delta;  // delta is also a Vec index delta
    self.bar2 += bar_delta;
}
```

### Hit testing uses `bar_to_x_f64`:
```rust
// trend_line.rs line 130-133
let x1 = viewport.bar_to_x_f64(self.bar1);
let x2 = viewport.bar_to_x_f64(self.bar2);
```

### Rendering uses `ctx.bar_to_x(bar: f64)`:
```rust
// trend_line.rs line 178-181
let x1 = ctx.bar_to_x(self.bar1);
let x2 = ctx.bar_to_x(self.bar2);
```

### CRITICAL: Dual coordinate system for primitives

**In-memory**: primitives store Vec indices as `bar1`, `bar2`, etc.
**On-disk / for TF sync**: `PrimitiveData.point_timestamps: Vec<i64>` stores Unix timestamps.

When timeframe changes, `drawing_manager.recalculate_all_bar_caches()` rebuilds Vec indices from timestamps:
```rust
// manager.rs line 874-883
let new_points = timestamps.iter().zip(current_points.iter())
    .map(|(ts, (_old_bar, price))| {
        let bar = find_bar_for_timestamp(bars, *ts).unwrap_or(0) as f64;
        (bar, *price)
    }).collect();
prim.set_points(&new_points);
```

So primitives have a two-layer system:
1. **Runtime**: Vec index (`bar1: f64`) — used for rendering and hit testing
2. **Persistent**: timestamp (`point_timestamps`) — source of truth across TF changes

---

## 5. How Rendering Uses Bar Indices

### Candle rendering (`crates/chart/src/chart/render/candles.rs`)

```rust
// candles.rs line 44-49
for i in start..end {
    if i >= bars.len() { break; }
    let bar = &bars[i];          // ARRAY ACCESS by Vec index
    let cx = viewport.bar_to_x(i); // i is a Vec index
    ...
    bar.close >= bars[i - 1].close  // Vec index - 1 also accessed
}
```

### Series rendering (`crates/chart/src/chart/render/series.rs`)

```rust
// series.rs line 37-41
for i in start..end {
    let bar = &bars[i];          // ARRAY ACCESS by Vec index
    let x = rect.x + viewport.bar_to_x(i);
}
```

### Indicator overlay rendering (`crates/chart/src/layout/render_chart.rs`)

```rust
// render_chart.rs line 461-468
for i in start_bar..end_bar {
    let value = values[i];       // indicator values[i] — ARRAY ACCESS by Vec index
    let x = state.chart_rect.x + state.viewport.bar_to_x(i);
}
```

### Sub-pane rendering (`crates/chart/src/chart/render/panes.rs`)

```rust
// panes.rs line 161-171
for i in start..end {
    let v = values[i];           // ARRAY ACCESS by Vec index
    let x = rect.x + bar_to_x(i);
}
```

**Pattern is consistent everywhere**: iterate `i in start..end` (where start/end from `viewport.visible_range()`), use `i` as both the Vec array index AND pass it to `bar_to_x(i)`.

---

## 6. `find_bar_for_timestamp`

### File: `crates/chart/src/types.rs` (lines 302-336)

```rust
pub fn find_bar_for_timestamp(bars: &[Bar], timestamp: i64) -> Option<usize>
```

Algorithm:
1. If `timestamp > last_bar.timestamp`: extrapolates using `bars.len() - 1 + bars_beyond` (can return index beyond `bars.len()`)
2. Binary search via `partition_point` for exact / before-range lookup
3. Before first bar: returns `Some(0)`

Return type: `Option<usize>` — always a **Vec index**.

**Also exists**: `bar_to_timestamp(bars: &[Bar], bar_idx: usize) -> Option<i64>` — simply `bars.get(bar_idx).map(|b| b.timestamp)`.

**Also exists** (in `time_scale.rs`, private):
```rust
fn bar_idx_to_timestamp(idx: i64, first_ts: i64, bar_interval: i64) -> i64 {
    first_ts + idx * bar_interval
}
fn timestamp_to_bar_idx(ts: i64, first_ts: i64, bar_interval: i64) -> i64 {
    (ts - first_ts) / bar_interval
}
```
These are used ONLY internally by `TimeScale::generate_ticks()` for calendar-aware tick positioning. They also produce Vec indices (`first_ts` is `bars[0].timestamp`).

---

## 7. Where `bars[i]` Array Access Happens (Definitive List)

These are the places that **require a true Vec index**:

| File | Pattern | Context |
|------|---------|---------|
| `chart/render/candles.rs:48-52` | `&bars[i]`, `bars[i-1].close` | Candle rendering loop |
| `chart/render/series.rs:38,82` | `&bars[i]` | Line/area series rendering |
| `chart/render/panes.rs:162,228,242,265` | `values[i]` | Sub-pane indicator rendering |
| `layout/render_chart.rs:462` | `values[i]` | Overlay indicator rendering |
| `drawing/manager.rs:915` | `bars[bar_idx]` | `bar_idx_to_timestamp` lookup |
| `state/chart.rs` (various) | `bars[i]` | State management |
| `indicators/src/...` | `bars[i]` | All indicator calculation loops |
| `chart/types/time_scale.rs:313` | `bars[bars.len()-1]` | Bar interval calculation for ticks |
| `types.rs:307` | `&bars[bars.len()-1]` | `find_bar_for_timestamp` |

The array accesses in the **rendering path** always use `i` from `viewport.visible_range()` which returns `(usize, usize)` clamped to `0..bar_count`. So those are always valid Vec indices.

---

## 8. The Key Question: Can `view_start` Become `timestamp / period_secs`?

### Proposed approach

Instead of `view_start = 1500` (bar index), use `view_start = first_ts + 1500 * period_secs` (a timestamp). Define "logical bar number" = `(timestamp - first_ts) / period_secs`. Then:

```
x = (logical_bar_at_pixel - view_start_logical) * bar_spacing + bar_spacing/2
logical_bar = view_start_logical + x / bar_spacing
```

where `view_start_logical = (view_start_ts - first_ts) / period_secs`.

### Analysis: What Would Still Work

1. **`bar_to_x_f64` / `x_to_bar_f64`** — The math is identical if the input is a "logical bar number" instead of a Vec index. Primitives that store timestamps could convert via `(ts - first_ts) / period` and the formula is unchanged.

2. **`TimeScale::generate_ticks()`** — Already uses timestamps as the internal coordinate. It converts to Vec indices only to call `viewport.bar_to_x_f64(bar_idx as f64)`. If `bar_to_x_f64` accepted logical bar numbers (which equals Vec index when timestamps are regular), this would be transparent.

3. **Primitive `points()` / rendering** — Could store `(logical_bar, price)` where `logical_bar = (ts - first_ts) / period`. All `ctx.bar_to_x(logical_bar)` calls would still work because the formula is the same.

### Analysis: What Would Break

**All of these would break and require rework:**

1. **`bars[i]` array access** — everywhere candles, series, indicators, and panes iterate `i in start..end` and access `bars[i]` and `values[i]`. "Logical bar number" ≠ Vec index unless bars are perfectly contiguous without gaps.

2. **`viewport.visible_range() -> (usize, usize)`** — returns indices for array iteration. Converting to logical bar numbers would require a separate "logical to vec" translation step before every rendering loop.

3. **`bar_count`** — currently `bars.len()`. With logical coordinates, this would need to mean something different (last logical bar number), but array iteration still needs `bars.len()`.

4. **`viewport.x_to_bar() -> Option<usize>`** — returns `usize` for clamped Vec index. Would need to return logical bar and separately look up Vec index for any code that does `bars[result]`.

5. **`bar_to_x(bar_idx: usize)` (integer version)** — called from all rendering loops passing `i` (Vec index). Changing `i` to mean something else breaks everything.

6. **Gaps in data (trading halts, weekends)** — With real timestamp-based coords, bars[i] would no longer align with position `(ts[i] - first_ts) / period` for data with gaps (e.g. crypto has no gaps but stocks do). The current system works fine with gaps because it just places contiguous bars at positions 0,1,2,...

7. **`scroll_to_end()`, `visible_range()` clamping** — these use `bar_count` as max. With logical coords, the max would be a timestamp, but the rendering loops still need Vec indices.

8. **Indicator `values[]` arrays** — computed as `Vec<f64>` with one value per bar in `bars[]`. The index into `values[i]` is always a Vec index. There is no concept of "logical bar index" in indicator output.

9. **Crosshair bar lookup** — code like `viewport.x_to_bar(cursor_x)` returns a Vec index used to look up `bars[bar_idx]` for the tooltip/legend OHLC display.

10. **Persistence (ViewportSnapshot)** — `view_start` is serialized to JSON. Changing its meaning would break all saved presets and templates that store a viewport.

---

## 9. Root Problem and Why It's Subtle

The current design conflates two things into one number:

- **Display coordinate**: "how far from the left edge is this bar?" → `(bar_idx - view_start) * bar_spacing`
- **Array address**: "which element of `bars[]` and `values[]` is this?" → `bars[bar_idx]`

They happen to be the same number (`i`) because:
- Bars are stored in a contiguous `Vec<Bar>` in chronological order
- There are no "holes" in the Vec (the chart only holds loaded bars)
- So Vec index == display offset from first loaded bar

### The Actual Problem Being Solved

Primitives drawn at bar index 1500 (relative to current load) become invalid when you reload with different history depth or switch timeframe. The current solution is `point_timestamps: Vec<i64>` in `PrimitiveData` — timestamps are the stable truth, bar Vec indices are the ephemeral rendering cache. This is correct and complete.

---

## 10. Feasibility Assessment of Timestamp-Based `view_start`

### Option A: Pure timestamp-based `view_start`
**Status: Would require pervasive refactor, not worth it**

Every rendering loop would need to convert logical bar numbers to Vec indices for array access. This is O(log n) binary search per bar rendered — 100x slower rendering. Also breaks gaps handling.

### Option B: Keep `view_start` as Vec index, add `view_start_ts` for persistence
**Status: Already done in a different form**

`ViewportSnapshot` stores `view_start` as a Vec index. When bars reload, the snapshot is applied with the same Vec-index value. This works as long as bar counts are consistent.

### Option C: `view_start` = `(anchor_timestamp - first_ts) / period_secs` stored, converted to Vec index on bars load
**Status: Viable for persistence only**

For persistence (presets, TF switches), store a timestamp-anchored viewport. On bars load, compute: `view_start_vec = find_bar_for_timestamp(bars, anchor_ts)`. This is how `recalculate_all_bar_caches` already works for primitives.

This is exactly what should be done for viewport persistence too. The rendering path never changes — `view_start` at runtime is always a Vec index.

### Option D: "Logical bar number = timestamp / period_secs" as unified coord
**Status: Only works for evenly-spaced crypto data, not recommended**

For crypto without gaps, `logical_bar = (ts - first_ts) / period` equals the Vec index. So technically, `view_start = (view_start_ts - first_ts) / period` gives the same number as the Vec index. But this requires `first_ts` and `period` to be known by the viewport, adding coupling. Any gap in data breaks the identity.

---

## 11. What Should Actually Be Done

The codebase is architecturally correct. The dual-layer system is:

1. **Runtime**: Vec index for everything (rendering, hit testing, interaction)
2. **Persistent**: timestamps for primitives (`point_timestamps`), viewport anchor for preset persistence

**Remaining gap**: viewport `view_start` is serialized as a raw Vec index. If bar history depth changes between save and restore (different backfill, different start date), the restored `view_start` will point to the wrong bar.

**Recommended fix**: When saving viewport to a preset/template, also save `view_start_anchor_ts = bar_to_timestamp(bars, view_start as usize)`. On restore, after bars load: `viewport.view_start = find_bar_for_timestamp(bars, view_start_anchor_ts)`. The chart `ViewportSnapshot` struct (`chart_window.rs` line 103) should gain an `anchor_timestamp: Option<i64>` field.

This is the minimal, correct, non-breaking improvement — no change to the rendering math, no change to `view_start`'s runtime meaning.

---

## 12. File Reference Map

| File | Role |
|------|------|
| `crates/chart/src/chart/types/viewport.rs` | Viewport struct, ALL bar↔pixel math |
| `crates/chart/src/chart/types/time_scale.rs` | TimeScale, tick generation, bar↔timestamp (internal only) |
| `crates/chart/src/types.rs` | `find_bar_for_timestamp`, `bar_to_timestamp`, `Bar` struct |
| `crates/chart/src/drawing/primitives_v2/traits.rs` | `Primitive` trait, `points() -> Vec<(f64,f64)>` |
| `crates/chart/src/drawing/primitives_v2/lines/trend_line.rs` | Canonical primitive example |
| `crates/chart/src/drawing/manager.rs` | `recalculate_all_bar_caches`, `sync_primitive_timestamps` |
| `crates/chart/src/engine/render/context.rs` | `RenderContext` trait with `bar_to_x(bar: f64)` |
| `crates/vello-context/src/context.rs` | Vello render context implementation |
| `crates/instanced-context/src/context.rs` | Instanced render context implementation |
| `crates/chart/src/chart/render/candles.rs` | Candle rendering — canonical `bars[i]` + `bar_to_x(i)` |
| `crates/chart/src/chart/render/series.rs` | Series rendering — same pattern |
| `crates/chart/src/layout/render_chart.rs` | Indicator overlay rendering — `values[i]` + `bar_to_x(i)` |
| `crates/chart/src/chart/render/panes.rs` | Sub-pane rendering — same pattern |
| `crates/chart/src/state/chart_window.rs` | `ChartWindow`, `ViewportSnapshot`, `bars: Vec<Bar>` |
| `crates/chart-app/src/lib.rs` | `view_start` mutations (set_bars, follow mode, resize) |
