# Implementation Plan: Viewport Snap-to-End Bug Fix

## Problem Statement

After restart/preset load, the viewport does not snap correctly to the latest bar.
Root causes validated through discussion:

1. `set_bars()` always defers snap via `needs_auto_scale_after_bars = true` — even when `chart_width` is already valid (non-zero). This means for every normal `BarsLoaded` (not first-launch), the snap fires in `prepare_frame()` one frame later, during which the stale `snap_cooldown` path is traversed.
2. `snap_cooldown` was added to guard against `bar_shift` undoing the snap, but it is a brittle frame-count workaround that races with sub-pane layout settling.
3. `from_window` currently saves the FULL viewport including `chart_width` (see `snapshots.rs:200` — the comment says "chart_width needed" but the design decision is to NOT save it). The current code does: `let mut saved_viewport = window.viewport.clone(); saved_viewport.view_start = 0.0;` — this saves `chart_width`, which is stale after restart with a different window size.
4. In `LoadPreset`, `scale_mode` is restored from the snapshot before bars arrive. Then in `BarsLoaded`, `window.price_scale.scale_mode = self.default_scale_mode` overwrites it for initial loads — losing the user's Manual scale preference.

## Architecture Decision

Fix by moving snap logic INTO `set_bars()` (eager snap when `chart_width > 0`), keeping deferred snap only for the true edge case of `chart_width == 0` at call time. Remove `snap_cooldown` entirely — it is no longer needed because snap happens BEFORE any layout code runs in that frame. Add `restore_scale_mode: Option<ScaleMode>` field to `ChartWindow` to carry the user's scale preference across the async `BarsLoaded` boundary without `default_scale_mode` clobbering it.

---

## Types and Signatures

```rust
// In ChartWindow struct — NEW field:
/// Scale mode to restore after the next set_bars() completes.
/// Set by LoadPreset before bars arrive; consumed in set_bars().
/// None = use whatever mode was set by the caller (default_scale_mode / Auto).
pub restore_scale_mode: Option<ScaleMode>,

// set_bars() — new contract:
pub fn set_bars(&mut self, bars: Vec<Bar>) {
    // 1. Store bars, calc MAs, prev_close, update bar_count.
    // 2. if chart_width > 0.0 && bar_spacing > 0.0 {
    //       snap view_start immediately
    //       calc_auto_scale()
    //       if let Some(mode) = self.restore_scale_mode.take() {
    //           self.price_scale.scale_mode = mode;
    //       }
    //       needs_auto_scale_after_bars = false
    //    } else {
    //       needs_auto_scale_after_bars = true  // only for first-launch (chart_width==0)
    //    }
}
```

---

## Files to Modify

### 1. `mylittlechart/crates/chart/src/state/chart_window.rs`

**A. Struct definition — remove `snap_cooldown`, add `restore_scale_mode`**

Lines 312–319 (current `snap_cooldown` doc + field):
- REMOVE `snap_cooldown: u8` field and its doc comment.

After line 296 (`pub needs_auto_scale_after_bars: bool`), ADD:

```rust
/// Scale mode to restore after the next `set_bars()` call completes.
///
/// Set by `LoadPreset` before bars arrive asynchronously. Consumed and
/// cleared inside `set_bars()` — applied after snap-to-end and auto-scale
/// so the user's Manual/Auto preference survives the async boundary.
/// `None` = no restoration needed; `set_bars()` leaves the mode as-is.
/// Runtime-only, not persisted.
pub restore_scale_mode: Option<ScaleMode>,
```

**B. `new_with_provider()` — line 408**

Current:
```rust
needs_auto_scale_after_bars: false,
scroll_fetch_in_flight: false,
scroll_fetch_started: None,
snap_cooldown: 0,
```

Replace with:
```rust
needs_auto_scale_after_bars: false,
restore_scale_mode: None,
scroll_fetch_in_flight: false,
scroll_fetch_started: None,
```

**C. `clone_for_split()` — lines 602–610**

Current:
```rust
pending_symbol_load: false,
needs_auto_scale_after_bars: false,
scroll_fetch_in_flight: false,
scroll_fetch_started: None,
snap_cooldown: 0,
```

Replace with:
```rust
pending_symbol_load: false,
needs_auto_scale_after_bars: false,
restore_scale_mode: None,
scroll_fetch_in_flight: false,
scroll_fetch_started: None,
```

**D. `set_bars()` — lines 736–755 (full rewrite)**

Current implementation defers unconditionally. Replace body with:

```rust
pub fn set_bars(&mut self, bars: Vec<Bar>) {
    self.bars = bars;
    self.calc_moving_averages();

    if !self.bars.is_empty() {
        self.prev_close_price = Some(self.bars[0].open);
    } else {
        self.prev_close_price = None;
    }
    self.update_prev_close_line();

    self.viewport.bar_count = self.bars.len();

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
        self.calc_auto_scale();
        // Restore user's scale mode preference if LoadPreset set one.
        if let Some(mode) = self.restore_scale_mode.take() {
            self.price_scale.scale_mode = mode;
        }
        self.needs_auto_scale_after_bars = false;
    } else {
        // Deferred: chart_width not yet set (first-launch, brand-new window).
        // prepare_frame() will run the snap once layout sets real dimensions.
        self.needs_auto_scale_after_bars = true;
    }
}
```

---

### 2. `mylittlechart/crates/chart/src/preset/snapshots.rs`

**`from_window()` — lines 197–215**

Current code saves the full viewport including `chart_width`, only zeroing `view_start`.

Replace the viewport-capture block (lines 197–215) with:

```rust
// Save ONLY bar_spacing and bar_count from the viewport.
// chart_width depends on the current window size and would be stale
// after restart. view_start is always recomputed by snap-to-end.
// Restore starts from Viewport::default() so no stale values leak.
let mut saved_viewport = Viewport::default();
saved_viewport.bar_spacing = window.viewport.bar_spacing;
saved_viewport.bar_count   = window.viewport.bar_count;
// view_start = 0.0 (default) — always snap to latest on restore.
// chart_width = 0.0 (default) — set_bars() deferred path handles first frame.
```

This replaces the current `let mut saved_viewport = window.viewport.clone(); saved_viewport.view_start = 0.0;` approach.

The `saved_viewport` variable is then used in the `Self { viewport: saved_viewport, ... }` struct construction at line 215 — no other change needed there.

---

### 3. `mylittlechart/crates/chart-app/src/lib.rs`

**A. `sync_viewport_from_layout()` — lines 1755–1770**

Remove `snap_cooldown` decrement branch. The full `if/else if` block currently reads:

```rust
if window.snap_cooldown > 0 {
    window.snap_cooldown -= 1;
} else if !window.needs_auto_scale_after_bars
    && (old_width - new_width).abs() > 0.5
    && window.viewport.bar_spacing > 0.0
    && old_width > 0.0
{
    let bar_shift = (old_width - new_width) / window.viewport.bar_spacing;
    window.viewport.view_start += bar_shift;
}
```

Replace with (remove the outer `if snap_cooldown` arm, keep only the `else if` condition as the sole condition):

```rust
if !window.needs_auto_scale_after_bars
    && (old_width - new_width).abs() > 0.5
    && window.viewport.bar_spacing > 0.0
    && old_width > 0.0
{
    let bar_shift = (old_width - new_width) / window.viewport.bar_spacing;
    window.viewport.view_start += bar_shift;
}
```

Also remove the comment block at lines 1755–1757 that mentions `snap_cooldown`.

**B. Non-split deferred snap block — lines 2937–2968**

The deferred snap block (inside `if !self.panel_app.panel_grid.is_split()`) currently sets `snap_cooldown = 3` at line 2952 and uses `saved_mode` round-trip for scale_mode.

Replace lines 2950–2957 (inside the `if window.needs_auto_scale_after_bars` body, after `view_start` assignment):

Current:
```rust
window.viewport.view_start = (count as f64 + dynamic_margin - visible_f).max(0.0);
// Arm cooldown so the next few frames don't let bar_shift undo this snap.
window.snap_cooldown = 3;
// Force auto-scale regardless of current scale_mode
let saved_mode = window.price_scale.scale_mode;
window.price_scale.scale_mode = ScaleMode::Auto;
window.calc_auto_scale();
window.price_scale.scale_mode = saved_mode;
```

Replace with:
```rust
window.viewport.view_start = (count as f64 + dynamic_margin - visible_f).max(0.0);
// No snap_cooldown needed: snap fires in prepare_frame AFTER
// sync_viewport_from_layout, so bar_shift cannot undo it this frame.
// Next frame old_width == new_width → bar_shift = 0.
window.calc_auto_scale();
// restore_scale_mode is already consumed inside set_bars() for the eager
// path; for the deferred path (chart_width was 0 at set_bars time),
// consume it here.
if let Some(mode) = window.restore_scale_mode.take() {
    window.price_scale.scale_mode = mode;
}
```

**C. Split-mode deferred snap block — lines 3600–3605**

Same pattern as B. Remove `snap_cooldown = 3` assignment and the `saved_mode` round-trip:

Current (lines 3600–3605):
```rust
// Arm cooldown so the next few frames don't let bar_shift undo this snap.
window.snap_cooldown = 3;
let saved_mode = window.price_scale.scale_mode;
window.price_scale.scale_mode = ScaleMode::Auto;
window.calc_auto_scale();
window.price_scale.scale_mode = saved_mode;
```

Replace with:
```rust
window.calc_auto_scale();
if let Some(mode) = window.restore_scale_mode.take() {
    window.price_scale.scale_mode = mode;
}
```

Also remove the comment at line 3566–3568 that mentions `snap_cooldown`, leaving only the `needs_auto_scale_after_bars` comment.

Parallel cooldown removal in split bar_shift block (lines 3569–3578): same as fix A — remove the outer `if snap_cooldown > 0` arm, keep only the `else if` condition bare.

**D. `BarsLoaded` handler — lines 1933–1937**

Current (initial-load path):
```rust
// Apply scale mode BEFORE set_bars so calc_auto_scale()
// runs with the correct mode (not stale Manual from previous symbol).
window.price_scale.scale_mode = self.default_scale_mode;
window.set_bars(bars.clone());
window.pending_symbol_load = false;
```

Replace with:
```rust
// Set scale_mode to Auto so calc_auto_scale() inside set_bars() runs
// correctly. If LoadPreset stored a restore_scale_mode, set_bars() will
// apply it AFTER auto-scale, restoring the user's preference.
// For new-symbol / symbol-switch paths, restore_scale_mode is None
// so the window stays on default_scale_mode (Auto).
window.price_scale.scale_mode = self.default_scale_mode;
// consume restore_scale_mode into the window before calling set_bars
// (it was placed there by LoadPreset; for symbol-switch it is None)
window.set_bars(bars.clone());
window.pending_symbol_load = false;
```

Note: No structural change here — the existing `self.default_scale_mode` assignment stays. The `restore_scale_mode` mechanism in `set_bars()` will override it IF LoadPreset set it. The comment is updated for clarity.

---

### 4. `mylittlechart/crates/chart-app/src/input.rs`

**A. LoadPreset — full layout restore path — lines 16115–16126**

Current (the section that restores bar_spacing and scale_mode):
```rust
// Restore persisted bar_spacing (zoom) and scale_mode.
// Viewport position (view_start) is NOT persisted — always
// snap-to-end after bars arrive.
if snap.viewport.bar_spacing > 0.0 {
    window.viewport.bar_spacing = snap.viewport.bar_spacing;
}
window.price_scale.scale_mode = snap.price_scale.scale_mode.clone();

// Bars arrive asynchronously via BarsLoaded.
// set_bars() will set needs_auto_scale_after_bars = true,
// and the deferred snap in prepare_frame will position view_start.
window.pending_symbol_load = true;
```

Replace with:
```rust
// Restore bar_spacing (user's zoom level). chart_width is NOT restored —
// it comes from layout on the first frame; view_start is NOT restored —
// snap-to-end always positions to the latest bar.
if snap.viewport.bar_spacing > 0.0 {
    window.viewport.bar_spacing = snap.viewport.bar_spacing;
}
// Do NOT set scale_mode here. Instead, stash it into restore_scale_mode
// so set_bars() can apply it AFTER auto-scale completes. This prevents
// the BarsLoaded handler's `window.price_scale.scale_mode = default_scale_mode`
// from clobbering a Manual scale preference.
window.restore_scale_mode = Some(snap.price_scale.scale_mode.clone());

// Bars arrive asynchronously via BarsLoaded.
window.pending_symbol_load = true;
```

**B. LoadPreset — fallback patch path — lines 16255–16262**

Current:
```rust
// Restore persisted bar_spacing (zoom) and scale_mode.
if snap.viewport.bar_spacing > 0.0 {
    window.viewport.bar_spacing = snap.viewport.bar_spacing;
}
window.price_scale.scale_mode = snap.price_scale.scale_mode.clone();

// Bars arrive asynchronously via BarsLoaded.
window.pending_symbol_load = true;
```

Replace with the same pattern as A:
```rust
if snap.viewport.bar_spacing > 0.0 {
    window.viewport.bar_spacing = snap.viewport.bar_spacing;
}
window.restore_scale_mode = Some(snap.price_scale.scale_mode.clone());

window.pending_symbol_load = true;
```

Also update the viewport assignment at line 16198 (`window.viewport = snap.viewport.clone()`) — since `from_window` now only saves `bar_spacing` and `bar_count` in the viewport, this clone is safe (chart_width will be 0.0 from default, which is correct). No change needed to the line itself, but verify that the fallback path does NOT do a separate `window.viewport = snap.viewport.clone()` that would restore a stale `chart_width`. It does (line 16198). Since the snapshot now stores `chart_width = 0.0`, this is safe — the existing line is harmless.

---

## Implementation Order

Steps must be done in this order to avoid compile errors:

1. **`chart_window.rs` struct** — add `restore_scale_mode: Option<ScaleMode>` field, remove `snap_cooldown: u8`. Update `new_with_provider()` and `clone_for_split()` initializers.

2. **`chart_window.rs` `set_bars()`** — rewrite to eager-snap when `chart_width > 0`, deferred when `chart_width == 0`, consuming `restore_scale_mode` in the eager path.

3. **`snapshots.rs` `from_window()`** — change saved_viewport to `Viewport::default()` with only `bar_spacing` and `bar_count` copied.

4. **`lib.rs` `sync_viewport_from_layout()`** — remove `snap_cooldown` decrement arm from non-split path.

5. **`lib.rs` non-split deferred snap block** — remove `snap_cooldown = 3`, remove `saved_mode` round-trip, add `restore_scale_mode.take()`.

6. **`lib.rs` split-mode bar_shift block** — remove `snap_cooldown` decrement arm.

7. **`lib.rs` split-mode deferred snap block** — remove `snap_cooldown = 3`, remove `saved_mode` round-trip, add `restore_scale_mode.take()`.

8. **`input.rs` LoadPreset full-restore path** — replace `window.price_scale.scale_mode = ...` with `window.restore_scale_mode = Some(...)`.

9. **`input.rs` LoadPreset fallback-patch path** — same change.

10. **Compile check** — `cargo check` in `mylittlechart/` root. Fix any remaining `snap_cooldown` references (grep for it; should be zero after steps 1–7).

---

## Internal Consistency Verification

| Decision | Where it lands | Consistent? |
|---|---|---|
| `bar_spacing` saved | `from_window`: `saved_viewport.bar_spacing = window.viewport.bar_spacing` | Yes |
| `bar_count` saved | `from_window`: `saved_viewport.bar_count = window.viewport.bar_count` | Yes |
| `chart_width` NOT saved | `from_window`: starts from `Viewport::default()` (0.0) | Yes |
| `view_start` NOT saved | `from_window`: 0.0 from default | Yes |
| `chart_width = 0` in LoadPreset | Fallback patch: `window.viewport = snap.viewport.clone()` — snap now has 0.0 so this is safe | Yes |
| `sync_viewport_from_layout` skips bar_shift for 0→N transitions | Existing guard: `old_w > 0.0` at line 1763 | Yes — no change needed |
| BarsLoaded sets Auto THEN set_bars eager-snaps | `window.price_scale.scale_mode = default_scale_mode` then `window.set_bars()` | Yes — Auto active during `calc_auto_scale()` |
| restore_scale_mode overrides Auto after snap | `set_bars()` calls `.take()` after `calc_auto_scale()` | Yes |
| New windows / symbol-switch: restore_scale_mode is None | Symbol-switch code never sets `restore_scale_mode` | Yes — stays None, Auto is preserved |
| LoadPreset sets restore_scale_mode | Both restore paths set `window.restore_scale_mode = Some(snap.price_scale.scale_mode.clone())` | Yes |
| snap_cooldown removed | 0 references remaining after plan applied | Yes |
| needs_auto_scale_after_bars stays for chart_width==0 | Only branch that sets it `true` in new `set_bars()` | Yes — first-launch still works |
| Deferred snap also consumes restore_scale_mode | Both deferred snap blocks call `restore_scale_mode.take()` | Yes — no double-apply risk (.take() returns None second time) |
| No "save chart_width" + "zero chart_width" contradiction | `from_window` explicitly builds `Viewport::default()` | Yes |

---

## Dead Code to Remove (Follow-up, Not This Commit)

- `needs_initial_viewport_fit` — if it still exists anywhere, it is superseded by `needs_auto_scale_after_bars`. Remove after confirming zero references.

---

## Testing Plan

**Manual regression tests after implementation:**

1. **App restart** — open app, verify latest bar visible immediately without any blank chart flash.
2. **Preset load** — switch preset, verify new preset's bars snap to end on first frame bars arrive.
3. **Symbol switch** — change symbol via search, verify new symbol snaps to end.
4. **Manual scale mode preserved** — drag price scale to Manual, save preset, reload preset, verify still Manual (not Auto) after bars load.
5. **Zoom preserved** — set narrow zoom (small `bar_spacing`), save preset, reload, verify zoom matches saved value (same number of bars visible).
6. **Split mode** — create split (2 panels), reload app, verify both panels snap to end independently.
7. **Sub-pane layout** — add RSI sub-pane, reload, verify chart does not jump/flicker as sub-pane height settles (no `snap_cooldown` side-effects).

**Unit test to add in `chart_window.rs` `#[cfg(test)]` block:**

```rust
#[test]
fn set_bars_eager_snap_when_chart_width_valid() {
    let mut w = ChartWindow::new("BTCUSDT", Timeframe::h1());
    w.viewport.chart_width = 800.0;
    w.viewport.bar_spacing = 8.0;
    let bars = (0..500).map(|_| Bar::default()).collect();
    w.set_bars(bars);
    // Should snap immediately, not defer
    assert!(!w.needs_auto_scale_after_bars);
    // view_start should be near bars.len() - visible_bars + margin
    let visible = 800.0 / 8.0_f64; // 100
    let margin = 5.0; // dynamic_margin for visible > 100
    let expected = (500.0_f64 + margin - visible).max(0.0);
    assert!((w.viewport.view_start - expected).abs() < 1.0);
}

#[test]
fn set_bars_deferred_when_chart_width_zero() {
    let mut w = ChartWindow::new("BTCUSDT", Timeframe::h1());
    // chart_width = 0.0 (default)
    let bars = (0..500).map(|_| Bar::default()).collect();
    w.set_bars(bars);
    assert!(w.needs_auto_scale_after_bars);
}

#[test]
fn set_bars_consumes_restore_scale_mode() {
    let mut w = ChartWindow::new("BTCUSDT", Timeframe::h1());
    w.viewport.chart_width = 800.0;
    w.viewport.bar_spacing = 8.0;
    w.restore_scale_mode = Some(ScaleMode::Manual);
    let bars = (0..500).map(|_| Bar::default()).collect();
    w.set_bars(bars);
    assert_eq!(w.price_scale.scale_mode, ScaleMode::Manual);
    assert!(w.restore_scale_mode.is_none());
}
```

**Estimated Complexity:** Medium

Three files changed. The logic is straightforward but touches the snap path in four locations (non-split sync, non-split deferred, split sync, split deferred). The `restore_scale_mode` field threading adds one new struct field through two initializers and two LoadPreset code paths. No new traits or async changes.
