# Implementation Plan: Unify Snap-to-End Logic

## Architecture Decision

One method on `ChartWindow` owns the formula. All callers pass through it. The method lives on `ChartWindow` (not `Viewport`) because it reads `self.bars.len()` for the bar count alongside `self.viewport.chart_width` and `self.viewport.bar_spacing`. A constant `DEFAULT_SNAP_MARGIN` is placed in `chart_window.rs` alongside the method. The dynamic margin lookup is eliminated — flat margin everywhere produces predictable positioning and enables future user-settable margin.

---

## Inventory of All Sites

| # | File | Line(s) | Formula | Action |
|---|------|---------|---------|--------|
| A | `crates/chart/src/state/chart_window.rs` | 761–767 | dynamic_margin in `set_bars()` eager path | Replace |
| B | `crates/chart-app/src/lib.rs` | 2939–2945 | dynamic_margin in `prepare_frame()` non-split deferred path | Replace |
| C | `crates/chart-app/src/lib.rs` | 3587–3593 | dynamic_margin in `prepare_frame()` split deferred path | Replace |
| D | `crates/chart-app/src/lib.rs` | 1706–1719 | `needs_initial_viewport_fit` integer-math in `resize()` | Replace |
| E | `crates/chart-app/src/lib.rs` | 2194–2202 | dynamic_margin in `TradeUpdate` follow-mode handler | Replace |
| F | `crates/chart-app/src/input.rs` | 162–164 | `right_margin = 2.0` in `ScaleCornerButton::AutoManual` (leaf path) | Replace |
| G | `crates/chart-app/src/input.rs` | 14536–14538 | `right_margin = 2.0` in `ScaleCornerButton::AutoManual` (active-window path) | Replace |
| H | `crates/chart/src/state/chart_window.rs` | 833 | `self.viewport.scroll_to_end()` after `set_bars()` in `change_symbol()` | Remove (double-snap) |
| I | `crates/chart/src/state/chart_window.rs` | 867 | `self.viewport.scroll_to_end()` after `set_bars()` in `change_timeframe()` | Remove (double-snap) |
| J | `crates/chart/src/state/chart_window.rs` | 1656 | `self.viewport.scroll_to_end()` in `fit_content()` | Replace |
| K | `crates/chart/src/state/chart_window.rs` | 1664 | `self.viewport.scroll_to_end()` in `reset_zoom()` | Replace |

---

## Types and Constants

```rust
// In: crates/chart/src/state/chart_window.rs
// Add near top of impl ChartWindow block

/// Default number of empty bars shown to the right of the last candle
/// when snapping to end. Will become a user setting in a future release.
pub const DEFAULT_SNAP_MARGIN: f64 = 5.0;

impl ChartWindow {
    /// Snap the viewport so the last bar is visible with `margin` empty bars
    /// of right-padding.
    ///
    /// Formula: `view_start = (bar_count + margin - visible_f).max(0.0)`
    ///
    /// Must only be called when `self.viewport.chart_width > 0.0` and
    /// `self.viewport.bar_spacing > 0.0`.
    pub fn snap_to_end(&mut self, margin: f64) {
        let visible_f = self.viewport.chart_width / self.viewport.bar_spacing;
        let count = self.bars.len();
        self.viewport.view_start = (count as f64 + margin - visible_f).max(0.0);
    }
}
```

No changes to `Viewport`. `scroll_to_end()` on `Viewport` is retained but deprecated-by-disuse — it stays compilable so any external consumer (e.g. integration tests) does not break, but it is never called from production paths after this refactor.

---

## Module Layout (no new files)

All changes are edits to existing files:

- `crates/chart/src/state/chart_window.rs` — add constant + method, fix 7 call sites
- `crates/chart-app/src/lib.rs` — fix 4 call sites
- `crates/chart-app/src/input.rs` — fix 2 call sites

---

## Implementation Steps

### Step 1 — Add constant and method to `ChartWindow`

**File:** `c:/Users/VA PC/CODING/ML_TRADING/nemo/mylittlechart/crates/chart/src/state/chart_window.rs`

After the existing `use` block and before or inside the first `impl ChartWindow` block (around line 30–50 range, near the top of the impl), insert:

```rust
pub const DEFAULT_SNAP_MARGIN: f64 = 5.0;
```

Then add the method `snap_to_end` as a new `pub fn` inside `impl ChartWindow`. A natural location is between `zoom_out()` (line 1643) and `fit_content()` (line 1650) since these are all view-position helpers:

```rust
/// Snap viewport to the most recent bar with `margin` bars of empty right space.
///
/// `margin` is a count of empty bars shown to the right of the last candle.
/// Use [`DEFAULT_SNAP_MARGIN`] for the standard value.
///
/// Preconditions: `self.viewport.chart_width > 0.0` and
/// `self.viewport.bar_spacing > 0.0` must both hold, or the result is
/// meaningless (those are the same preconditions as [`Viewport::visible_bars`]).
pub fn snap_to_end(&mut self, margin: f64) {
    let visible_f = self.viewport.chart_width / self.viewport.bar_spacing;
    let count = self.bars.len();
    self.viewport.view_start = (count as f64 + margin - visible_f).max(0.0);
}
```

---

### Step 2 — Fix Site A: `set_bars()` eager path

**File:** `c:/Users/VA PC/CODING/ML_TRADING/nemo/mylittlechart/crates/chart/src/state/chart_window.rs`
**Lines 758–767 (before edit):**
```rust
if self.viewport.chart_width > 0.0 && self.viewport.bar_spacing > 0.0 {
    // Eager snap: chart_width is valid, compute view_start immediately.
    let count = self.bars.len();
    let visible_f = self.viewport.chart_width / self.viewport.bar_spacing;
    let dynamic_margin = if (visible_f as usize) <= 10 { 1.0 }
        else if (visible_f as usize) <= 20 { 2.0 }
        else if (visible_f as usize) <= 50 { 3.0 }
        else if (visible_f as usize) <= 100 { 4.0 }
        else { 5.0 };
    self.viewport.view_start = (count as f64 + dynamic_margin - visible_f).max(0.0);
```

**Replace lines 761–767 with:**
```rust
if self.viewport.chart_width > 0.0 && self.viewport.bar_spacing > 0.0 {
    // Eager snap: chart_width is valid, snap immediately.
    self.snap_to_end(DEFAULT_SNAP_MARGIN);
```

The `count` local variable is no longer needed and is removed. The `self.calc_auto_scale()` call on the line immediately after (line 768) is untouched.

---

### Step 3 — Fix Sites H and I: double-snap in `change_symbol()` and `change_timeframe()`

**File:** `c:/Users/VA PC/CODING/ML_TRADING/nemo/mylittlechart/crates/chart/src/state/chart_window.rs`

**Site H — `change_symbol()`, line 833:**
```rust
self.set_bars(new_bars);
self.viewport.scroll_to_end();   // <-- REMOVE THIS LINE
```
Remove `self.viewport.scroll_to_end();` at line 833. `set_bars()` already calls `snap_to_end()` in the eager path; adding `scroll_to_end()` afterward overwrites the margin with a zero-margin position.

**Site I — `change_timeframe()`, line 867:**
```rust
self.set_bars(new_bars);
// Recalculate primitive bar caches for new timeframe
self.drawing_manager.recalculate_all_bar_caches(&self.bars);
self.viewport.scroll_to_end();   // <-- REMOVE THIS LINE
```
Remove `self.viewport.scroll_to_end();` at line 867.

---

### Step 4 — Fix Sites J and K: `fit_content()` and `reset_zoom()`

**File:** `c:/Users/VA PC/CODING/ML_TRADING/nemo/mylittlechart/crates/chart/src/state/chart_window.rs`

**Site J — `fit_content()`, lines 1651–1658 (before):**
```rust
pub fn fit_content(&mut self) {
    let bar_count = self.bars.len();
    if bar_count > 0 {
        self.viewport.bar_spacing = self.viewport.chart_width / bar_count as f64;
        self.viewport.bar_spacing = self.viewport.bar_spacing.clamp(1.0, 30.0);
        self.viewport.scroll_to_end();
        self.calc_auto_scale();
    }
}
```

**Replace `self.viewport.scroll_to_end()` with:**
```rust
        self.snap_to_end(DEFAULT_SNAP_MARGIN);
```

**Site K — `reset_zoom()`, lines 1661–1666 (before):**
```rust
pub fn reset_zoom(&mut self) {
    self.viewport.bar_spacing = 8.0;
    self.viewport.scroll_to_end();
    self.calc_auto_scale();
}
```

**Replace `self.viewport.scroll_to_end()` with:**
```rust
    self.snap_to_end(DEFAULT_SNAP_MARGIN);
```

---

### Step 5 — Fix Sites B and C: `prepare_frame()` deferred snap paths in `lib.rs`

**File:** `c:/Users/VA PC/CODING/ML_TRADING/nemo/mylittlechart/crates/chart-app/src/lib.rs`

Both sites follow the same pattern. The `window` variable has type `&mut ChartWindow`.

**Site B — non-split deferred snap, lines 2935–2945 (before):**
```rust
if window.needs_auto_scale_after_bars && !window.bars.is_empty() && window.viewport.chart_width > 0.0 {
    window.needs_auto_scale_after_bars = false;
    // Focus-style snap: position last bar with right margin
    let count = window.bars.len();
    let visible_f = window.viewport.chart_width / window.viewport.bar_spacing;
    let dynamic_margin = if (visible_f as usize) <= 10 { 1.0 }
        else if (visible_f as usize) <= 20 { 2.0 }
        else if (visible_f as usize) <= 50 { 3.0 }
        else if (visible_f as usize) <= 100 { 4.0 }
        else { 5.0 };
    window.viewport.view_start = (count as f64 + dynamic_margin - visible_f).max(0.0);
```

**Replace lines 2937–2945 with:**
```rust
    // Snap to end with standard margin.
    window.snap_to_end(zengeld_chart::state::chart_window::DEFAULT_SNAP_MARGIN);
```

Note: adjust the path to `DEFAULT_SNAP_MARGIN` to match whatever `use` imports are already in scope. If `ChartWindow` is already imported directly via `use zengeld_chart::state::ChartWindow` or similar, the constant path may be `ChartWindow::DEFAULT_SNAP_MARGIN` or simply the bare constant if re-exported. Confirm the exact import path by checking the existing `use` block at the top of `lib.rs`. If a re-export is added (Step 8 below), use that instead.

**Site C — split deferred snap, lines 3582–3593 (before):**
```rust
if window.needs_auto_scale_after_bars && !window.bars.is_empty() && window.viewport.chart_width > 0.0 {
    window.needs_auto_scale_after_bars = false;
    // Snap-to-end: position last bar with right margin,
    // using CURRENT bar_spacing (restored from preset).
    let count = window.bars.len();
    let visible_f = window.viewport.chart_width / window.viewport.bar_spacing;
    let dynamic_margin = if (visible_f as usize) <= 10 { 1.0 }
        else if (visible_f as usize) <= 20 { 2.0 }
        else if (visible_f as usize) <= 50 { 3.0 }
        else if (visible_f as usize) <= 100 { 4.0 }
        else { 5.0 };
    window.viewport.view_start = (count as f64 + dynamic_margin - visible_f).max(0.0);
```

**Replace lines 3584–3593 with:**
```rust
    // Snap to end with standard margin.
    window.snap_to_end(zengeld_chart::state::chart_window::DEFAULT_SNAP_MARGIN);
```

---

### Step 6 — Fix Site D: `resize()` `needs_initial_viewport_fit` path in `lib.rs`

**File:** `c:/Users/VA PC/CODING/ML_TRADING/nemo/mylittlechart/crates/chart-app/src/lib.rs`
**Lines 1706–1722 (before):**
```rust
if self.needs_initial_viewport_fit {
    self.needs_initial_viewport_fit = false;
    for window in self.panel_app.panel_grid.windows_mut().values_mut() {
        let count = window.bars.len();
        let visible = window.viewport.visible_bars();
        let right_margin: usize = 5;
        // Snap ALL windows to the most recent bar with a right margin.
        if count + right_margin > visible {
            window.viewport.view_start = (count + right_margin - visible) as f64;
        } else {
            window.viewport.view_start = 0.0;
        }
        window.calc_auto_scale();
    }
}
```

**Replace the inner loop body (lines 1709–1720) with:**
```rust
if self.needs_initial_viewport_fit {
    self.needs_initial_viewport_fit = false;
    for window in self.panel_app.panel_grid.windows_mut().values_mut() {
        // Only snap if chart dimensions are valid and bars are loaded.
        if window.viewport.chart_width > 0.0 && window.viewport.bar_spacing > 0.0 && !window.bars.is_empty() {
            window.snap_to_end(zengeld_chart::state::chart_window::DEFAULT_SNAP_MARGIN);
        }
        window.calc_auto_scale();
    }
}
```

Rationale: the old integer formula `(count + right_margin - visible)` could underflow or produce off-by-one results when `count < visible - right_margin`. The new formula uses `.max(0.0)` inside `snap_to_end()`, which handles that correctly. The empty-bars guard (`!window.bars.is_empty()`) avoids calling snap on zero-bar windows (the old code guarded with `count + right_margin > visible`, which was always false when count=0 so it fell through to `view_start = 0.0` — the new guard achieves the same semantic without the branch).

---

### Step 7 — Fix Site E: follow-mode snap in `TradeUpdate` handler in `lib.rs`

**File:** `c:/Users/VA PC/CODING/ML_TRADING/nemo/mylittlechart/crates/chart-app/src/lib.rs`
**Lines 2189–2202 (before):**
```rust
let count = window.bars.len();
let visible_f = window.viewport.chart_width / window.viewport.bar_spacing;
let visible_bars = visible_f as usize;

// Dynamic right margin based on zoom level.
let dynamic_margin = if visible_bars <= 10 { 1.0 }
    else if visible_bars <= 20 { 2.0 }
    else if visible_bars <= 50 { 3.0 }
    else if visible_bars <= 100 { 4.0 }
    else { 5.0 };

// Follow mode: keep last bar visible with dynamic margin.
if window.price_scale.scale_mode.is_follow() {
    window.viewport.view_start = (count as f64 + dynamic_margin - visible_f).max(0.0);
}
```

**Replace lines 2189–2202 with:**
```rust
// Follow mode: keep last bar visible with standard margin.
if window.price_scale.scale_mode.is_follow() {
    window.snap_to_end(zengeld_chart::state::chart_window::DEFAULT_SNAP_MARGIN);
}
```

Remove the `count`, `visible_f`, `visible_bars`, and `dynamic_margin` locals entirely — they are only consumed by the `is_follow()` branch and the old `is_new_bar`/`Auto` nudge below which references `visible_f`. Check whether `visible_f` is still needed in the `is_new_bar && ScaleMode::Auto` nudge block at lines 2210–2216:

```rust
// Auto mode guard: if a new bar appeared and it would
// be off-screen or at the very edge, nudge viewport by exactly 1 bar.
if is_new_bar && window.price_scale.scale_mode == ScaleMode::Auto {
    let right_edge_bar = window.viewport.view_start + visible_f;   // <-- uses visible_f
    let last_bar = count as f64;
    if last_bar >= right_edge_bar {
        window.viewport.view_start += 1.0;
    }
}
```

`visible_f` and `count` ARE still needed by this Auto nudge block. Keep those two locals only, remove `visible_bars` and `dynamic_margin`. The replacement becomes:

```rust
let count = window.bars.len();
let visible_f = window.viewport.chart_width / window.viewport.bar_spacing;

// Follow mode: keep last bar visible with standard margin.
if window.price_scale.scale_mode.is_follow() {
    window.snap_to_end(zengeld_chart::state::chart_window::DEFAULT_SNAP_MARGIN);
}

// Auto mode guard: nudge by 1 bar when new bar appears at right edge.
if is_new_bar && window.price_scale.scale_mode == ScaleMode::Auto {
    let right_edge_bar = window.viewport.view_start + visible_f;
    let last_bar = count as f64;
    if last_bar >= right_edge_bar {
        window.viewport.view_start += 1.0;
    }
}
```

---

### Step 8 — Fix Sites F and G: `ScaleCornerButton::AutoManual` follow-mode initial snap in `input.rs`

**File:** `c:/Users/VA PC/CODING/ML_TRADING/nemo/mylittlechart/crates/chart-app/src/input.rs`

**Site F — leaf path, lines 160–165 (before):**
```rust
if next_mode.is_follow() {
    let count = window.bars.len();
    let visible_f = window.viewport.chart_width / window.viewport.bar_spacing;
    let right_margin = 2.0_f64;
    window.viewport.view_start = (count as f64 + right_margin - visible_f).max(0.0);
}
```

**Replace with:**
```rust
if next_mode.is_follow() {
    window.snap_to_end(zengeld_chart::state::chart_window::DEFAULT_SNAP_MARGIN);
}
```

**Site G — active-window path, lines 14533–14539 (before):**
```rust
if next_mode.is_follow() {
    // Focus mode: position viewport to last bar + 2 bar right margin
    let count = window.bars.len();
    let visible_f = window.viewport.chart_width / window.viewport.bar_spacing;
    let right_margin = 2.0_f64;
    window.viewport.view_start = (count as f64 + right_margin - visible_f).max(0.0);
}
```

**Replace with:**
```rust
if next_mode.is_follow() {
    window.snap_to_end(zengeld_chart::state::chart_window::DEFAULT_SNAP_MARGIN);
}
```

---

### Step 9 — Expose `DEFAULT_SNAP_MARGIN` for use in `lib.rs` and `input.rs`

**Option A (preferred):** Use the full path `zengeld_chart::state::chart_window::DEFAULT_SNAP_MARGIN` at every call site. No changes to any `mod.rs` or `lib.rs` re-export.

**Option B:** Add a re-export in `crates/chart/src/state/mod.rs`:
```rust
pub use chart_window::DEFAULT_SNAP_MARGIN;
```
Then callers use `zengeld_chart::state::DEFAULT_SNAP_MARGIN`.

**Option C:** Re-export at the crate root in `crates/chart/src/lib.rs`:
```rust
pub use state::chart_window::DEFAULT_SNAP_MARGIN;
```
Then callers use `zengeld_chart::DEFAULT_SNAP_MARGIN`.

Choose Option C for minimal path verbosity. Check `crates/chart/src/lib.rs` for the existing re-export block for `ChartWindow` and add `DEFAULT_SNAP_MARGIN` next to it.

---

## Precise File and Line Summary

### `crates/chart/src/state/chart_window.rs`

| Lines | Change |
|-------|--------|
| ~1648 (between `zoom_out` and `fit_content`) | INSERT `pub const DEFAULT_SNAP_MARGIN: f64 = 5.0;` and `pub fn snap_to_end(&mut self, margin: f64)` method |
| 761–767 | REPLACE 7-line dynamic_margin block with `self.snap_to_end(DEFAULT_SNAP_MARGIN);` |
| 833 | REMOVE `self.viewport.scroll_to_end();` |
| 867 | REMOVE `self.viewport.scroll_to_end();` |
| 1656 | REPLACE `self.viewport.scroll_to_end();` with `self.snap_to_end(DEFAULT_SNAP_MARGIN);` |
| 1664 | REPLACE `self.viewport.scroll_to_end();` with `self.snap_to_end(DEFAULT_SNAP_MARGIN);` |

### `crates/chart-app/src/lib.rs`

| Lines | Change |
|-------|--------|
| 1709–1719 | REPLACE integer-math snap block with `snap_to_end` call + empty-bar guard |
| 2189–2202 | REPLACE dynamic_margin block with `snap_to_end` call; keep `count` and `visible_f` for the Auto nudge below |
| 2937–2945 | REPLACE 9-line dynamic_margin block with `window.snap_to_end(DEFAULT_SNAP_MARGIN);` |
| 3584–3593 | REPLACE 10-line dynamic_margin block with `window.snap_to_end(DEFAULT_SNAP_MARGIN);` |

### `crates/chart-app/src/input.rs`

| Lines | Change |
|-------|--------|
| 161–164 | REPLACE 4-line snap block with `window.snap_to_end(DEFAULT_SNAP_MARGIN);` |
| 14534–14538 | REPLACE 5-line snap block with `window.snap_to_end(DEFAULT_SNAP_MARGIN);` |

### `crates/chart/src/lib.rs` (or `crates/chart/src/state/mod.rs`)

| Change |
|--------|
| ADD re-export `pub use state::chart_window::DEFAULT_SNAP_MARGIN;` |

---

## What Happens to `Viewport::scroll_to_end()`

**Do not delete it.** It still compiles, it may be used in tests, and removing it is a separate concern. After this refactor it will have zero call sites in production code. Add a doc comment:

```rust
/// Scroll to show the most recent bars (right edge).
///
/// # Deprecated usage
/// This method positions the last bar flush against the right edge with no
/// margin. Prefer [`ChartWindow::snap_to_end`] for all UI-level snap
/// operations, which includes a configurable right margin.
pub fn scroll_to_end(&mut self) {
    self.view_start = (self.bar_count.saturating_sub(self.visible_bars())) as f64;
}
```

---

## Error Handling

No fallible operations are introduced. The only invariant is `chart_width > 0.0 && bar_spacing > 0.0` — both conditions are already checked by every call site before invoking `snap_to_end`. If they are not checked (future caller), the worst outcome is `view_start = 0.0` (`.max(0.0)` clamps) or a NaN/infinity which is no worse than the current code.

To be explicit, add the precondition note to the doc comment (already included in Step 1 above). Do not add a `debug_assert!` — it would fire during tests that set up partial viewport state, and the silent `.max(0.0)` clamp is safe.

---

## Implementation Order

Execute in this exact order to keep the code compilable at each step:

1. **Step 1** — Add `DEFAULT_SNAP_MARGIN` and `snap_to_end()` to `chart_window.rs`. Code compiles; method is unused.
2. **Step 9** — Add re-export to `crates/chart/src/lib.rs`. No breakage.
3. **Step 2** — Replace Site A in `set_bars()`. `cargo check` clean.
4. **Step 3** — Remove Sites H and I (double-snap deletions). `cargo check` clean.
5. **Step 4** — Replace Sites J and K in `fit_content()` / `reset_zoom()`. `cargo check` clean.
6. **Step 5** — Replace Sites B and C in `prepare_frame()`. `cargo check` clean.
7. **Step 6** — Replace Site D in `resize()`. `cargo check` clean.
8. **Step 7** — Replace Site E in `TradeUpdate` handler. `cargo check` clean.
9. **Step 8** — Replace Sites F and G in `input.rs`. `cargo check` clean.
10. **Final** — Add deprecation note to `Viewport::scroll_to_end()`. `cargo check` clean.

Run `cargo check --package chart` after steps 1–5, then `cargo check` (full workspace) after step 6 onwards.

---

## Testing Plan

### Unit tests in `chart_window.rs` — `#[cfg(test)] mod tests`

Add to `crates/chart/src/state/chart_window.rs`:

```rust
#[cfg(test)]
mod snap_to_end_tests {
    use super::*;

    fn make_window_with_bars(bar_count: usize, chart_width: f64, bar_spacing: f64) -> ChartWindow {
        // construct a minimal ChartWindow with controlled viewport
        // (use the existing test helpers or Default + manual field assignment)
        let mut w = ChartWindow::default(); // or test constructor
        w.bars = vec![Bar::default(); bar_count];
        w.viewport.chart_width = chart_width;
        w.viewport.bar_spacing = bar_spacing;
        w.viewport.bar_count = bar_count;
        w
    }

    #[test]
    fn snap_to_end_places_last_bar_with_margin() {
        // 200 bars, 800px wide, 8px/bar → 100 visible bars
        // Expected: view_start = 200 + 5 - 100 = 105
        let mut w = make_window_with_bars(200, 800.0, 8.0);
        w.snap_to_end(5.0);
        assert!((w.viewport.view_start - 105.0).abs() < 0.001);
    }

    #[test]
    fn snap_to_end_clamps_to_zero_when_few_bars() {
        // 10 bars, 800px wide, 8px/bar → 100 visible bars
        // 10 + 5 - 100 = -85 → clamped to 0.0
        let mut w = make_window_with_bars(10, 800.0, 8.0);
        w.snap_to_end(5.0);
        assert_eq!(w.viewport.view_start, 0.0);
    }

    #[test]
    fn snap_to_end_zero_margin_flush_right() {
        // 100 bars, 800px, 8px → 100 visible → view_start = 0.0
        let mut w = make_window_with_bars(100, 800.0, 8.0);
        w.snap_to_end(0.0);
        assert_eq!(w.viewport.view_start, 0.0);
    }

    #[test]
    fn default_snap_margin_is_five() {
        assert_eq!(DEFAULT_SNAP_MARGIN, 5.0);
    }
}
```

### Regression check — no behavioral change for typical case

With `visible_f > 100` the old dynamic formula returned margin `5.0`. The new formula always uses `5.0`. For `visible_f <= 100` the old formula returned a smaller margin (1–4). This is an intentional behavioral change: at all zoom levels the margin is now a uniform 5 bars. If this causes visual regression at extreme zoom-in (very few visible bars), the margin can be clamped to `margin.min(visible_f * 0.2)` — but this is a follow-up decision, not part of this refactor.

---

## Estimated Complexity: Low

The change is pure mechanical substitution with zero new abstractions. All types, lifetimes, and ownership are unchanged. The only non-trivial judgment call is Site E (TradeUpdate), where two locals (`count`, `visible_f`) must be kept for the downstream Auto nudge. Every other site is a straight deletion of boilerplate lines.
